use std::{ffi::OsString, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    domain::{
        BlameLine, Blob, CommitDetail, Comparison, ComparisonMode, Diff, GitPath, HistoryPage,
        ObjectFormat, Oid, RefInfo, Repository, StashEntry, Status, TreeEntry,
    },
    git::{
        parse::{
            parse_blame, parse_commit, parse_diff, parse_history, parse_raw_diff_metadata,
            parse_refs, parse_stashes, parse_status, parse_tree,
        },
        process::{CancellationToken, GitError, GitRunner},
    },
};

const DECORATIONS: &str = "%(decorate:prefix=,suffix=,separator=%x1f)";
const LOG_FORMAT: &str = "%H%x00%P%x00%an%x00%ae%x00%at%x00%aI%x00%cn%x00%ce%x00%ct%x00%cI%x00";
const DETAIL_FORMAT: &str = "%H%x00%P%x00%an%x00%ae%x00%at%x00%aI%x00%cn%x00%ce%x00%ct%x00%cI%x00";
const REF_FORMAT: &str = "%(refname)%00%(refname:short)%00%(objectname)%00%(*objectname)%00%(upstream:short)%00%(subject)%00%(creatordate:unix)%00%(HEAD)%00";
const STASH_FORMAT: &str = "%gd%x1f%H%x1f%P%x1f%ct%x1f%gs";

#[derive(Debug, Clone)]
pub struct GitClient {
    runner: GitRunner,
    max_patch_bytes: usize,
    max_blob_bytes: usize,
    diff_context: usize,
    diff_algorithm: String,
    diff_whitespace: String,
}

impl Default for GitClient {
    fn default() -> Self {
        Self {
            runner: GitRunner::default(),
            max_patch_bytes: 16 * 1024 * 1024,
            max_blob_bytes: 8 * 1024 * 1024,
            diff_context: 3,
            diff_algorithm: "histogram".into(),
            diff_whitespace: "show".into(),
        }
    }
}

impl GitClient {
    pub fn new(runner: GitRunner) -> Self {
        Self {
            runner,
            ..Self::default()
        }
    }

    pub fn runner(&self) -> &GitRunner {
        &self.runner
    }

    pub fn with_content_limits(mut self, max_patch_bytes: usize, max_blob_bytes: usize) -> Self {
        self.max_patch_bytes = max_patch_bytes.max(1);
        self.max_blob_bytes = max_blob_bytes.max(1);
        self
    }

    pub fn with_diff_options(
        mut self,
        context: usize,
        algorithm: impl Into<String>,
        whitespace: impl Into<String>,
    ) -> Self {
        self.diff_context = context.min(999);
        self.diff_algorithm = algorithm.into();
        self.diff_whitespace = whitespace.into();
        self
    }

    pub fn discover(&self, path: &Path) -> Result<Repository, GitError> {
        if cfg!(windows) {
            return Err(GitError::UnsupportedPlatform {
                platform: "native Windows",
                guidance: "use phig under WSL",
            });
        }
        let cancellation = CancellationToken::new();
        let version_output =
            self.runner
                .run_complete(None, "version", ["--version"], &cancellation)?;
        let version_bytes = version_output
            .stdout
            .strip_suffix(b"\n")
            .unwrap_or(&version_output.stdout);
        let version_display = crate::sanitize::sanitize_bytes(version_bytes);
        let git_version = version_display
            .strip_prefix("git version ")
            .unwrap_or(&version_display)
            .to_owned();
        ensure_minimum_git(&git_version)?;

        let git_dir = match self.runner.run_complete(
            Some(path),
            "discover",
            ["rev-parse", "--path-format=absolute", "--git-dir"],
            &cancellation,
        ) {
            Ok(output) => path_from_output(&output.stdout)?,
            Err(GitError::Failed { ref stderr, .. })
                if stderr.to_ascii_lowercase().contains("not a git repository")
                    && !has_git_marker(path) =>
            {
                return Err(GitError::NotRepository(path.to_path_buf()));
            }
            Err(error) => return Err(error),
        };
        let bare = output_bool(
            &self
                .runner
                .run_complete(
                    Some(path),
                    "discover",
                    ["rev-parse", "--is-bare-repository"],
                    &cancellation,
                )?
                .stdout,
        );
        let root = if bare {
            git_dir.clone()
        } else {
            path_from_output(
                &self
                    .runner
                    .run_complete(
                        Some(path),
                        "discover",
                        ["rev-parse", "--path-format=absolute", "--show-toplevel"],
                        &cancellation,
                    )?
                    .stdout,
            )?
        };
        let object_format = ObjectFormat::from_git(
            std::str::from_utf8(
                &self
                    .runner
                    .run_complete(
                        Some(path),
                        "object-format",
                        ["rev-parse", "--show-object-format"],
                        &cancellation,
                    )?
                    .stdout,
            )
            .unwrap_or("unknown"),
        );
        let symbolic_ref = match self.runner.run_complete(
            Some(&root),
            "branch",
            ["symbolic-ref", "--quiet", "HEAD"],
            &cancellation,
        ) {
            Ok(output) => {
                let value = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
                (!value.is_empty()).then(|| GitPath::new(value.to_vec()))
            }
            Err(GitError::Failed {
                code: Some(1),
                ref stderr,
                ..
            }) if stderr.is_empty() => None,
            Err(error) => return Err(error),
        };
        let branch = symbolic_ref.as_ref().map(|name| {
            name.display
                .strip_prefix("refs/heads/")
                .unwrap_or(&name.display)
                .to_owned()
        });
        let head = match self.resolve(&root, "HEAD", object_format, &cancellation) {
            Ok(head) => Some(head),
            Err(resolve_error @ GitError::Failed { .. }) => {
                let Some(symbolic_ref) = &symbolic_ref else {
                    return Err(resolve_error);
                };
                let check = self.runner.run_complete(
                    Some(&root),
                    "head-ref",
                    [
                        OsString::from("show-ref"),
                        OsString::from("--verify"),
                        OsString::from("--quiet"),
                        symbolic_ref.to_os_string()?,
                    ],
                    &cancellation,
                );
                match check {
                    Err(GitError::Failed {
                        code: Some(1),
                        ref stderr,
                        ..
                    }) if stderr.is_empty() => None,
                    _ => return Err(resolve_error),
                }
            }
            Err(error) => return Err(error),
        };
        Ok(Repository {
            root: root.clone(),
            worktree: (!bare).then_some(root),
            git_dir,
            bare,
            object_format,
            git_version,
            head,
            branch,
        })
    }

    pub fn history(
        &self,
        repository: &Repository,
        revision: &str,
        paths: &[GitPath],
        offset: usize,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<HistoryPage, GitError> {
        let limit = limit.clamp(1, 4096);
        let mut args = vec![
            OsString::from("log"),
            OsString::from("-z"),
            OsString::from("--decorate=short"),
            OsString::from(format!("--format={LOG_FORMAT}{DECORATIONS}%x00%s%x00")),
            OsString::from(format!("--max-count={}", limit + 1)),
            OsString::from(format!("--skip={offset}")),
            OsString::from("--end-of-options"),
            OsString::from(revision),
            OsString::from("--"),
        ];
        append_paths(&mut args, paths)?;
        let output =
            self.runner
                .run_complete(Some(&repository.root), "history", args, cancellation)?;
        parse_history(&output.stdout, repository.object_format, offset, limit)
    }

    pub fn commit_detail(
        &self,
        repository: &Repository,
        revision: &str,
        parent_index: usize,
        paths: &[GitPath],
        cancellation: &CancellationToken,
    ) -> Result<CommitDetail, GitError> {
        let resolved = self.resolve(
            &repository.root,
            revision,
            repository.object_format,
            cancellation,
        )?;
        let metadata_args = [
            OsString::from("show"),
            OsString::from("-s"),
            OsString::from("-z"),
            OsString::from(format!(
                "--format={DETAIL_FORMAT}{DECORATIONS}%x00%s%x00%b%x00"
            )),
            OsString::from("--end-of-options"),
            OsString::from(resolved.to_string()),
        ];
        let metadata = self.runner.run_complete(
            Some(&repository.root),
            "commit-detail",
            metadata_args,
            cancellation,
        )?;
        let commit = parse_commit(&metadata.stdout, repository.object_format)?;
        let selected_parent =
            if commit.parents.is_empty() && parent_index == 0 {
                None
            } else {
                Some(commit.parents.get(parent_index).cloned().ok_or(
                    GitError::InvalidParentIndex {
                        requested: parent_index,
                        available: commit.parents.len(),
                    },
                )?)
            };
        let (mut patch_args, mut metadata_args) = if let Some(parent) = &selected_parent {
            let revisions = [
                OsString::from("--end-of-options"),
                OsString::from(parent.to_string()),
                OsString::from(commit.id.to_string()),
                OsString::from("--"),
            ];
            (
                self.diff_args("diff", "--patch", &revisions),
                self.diff_args("diff", "--raw", &revisions),
            )
        } else {
            let revision = [
                OsString::from("--end-of-options"),
                OsString::from(commit.id.to_string()),
                OsString::from("--"),
            ];
            (
                self.diff_tree_args("--patch", &revision),
                self.diff_tree_args("--raw", &revision),
            )
        };
        append_paths(&mut patch_args, paths)?;
        append_paths(&mut metadata_args, paths)?;
        let diff = self.load_diff(
            repository,
            "commit-diff",
            patch_args,
            "commit-diff-metadata",
            metadata_args,
            cancellation,
        )?;
        Ok(CommitDetail {
            commit,
            diff,
            selected_parent,
        })
    }

    pub fn refs(
        &self,
        repository: &Repository,
        cancellation: &CancellationToken,
    ) -> Result<Vec<RefInfo>, GitError> {
        let args = [
            OsString::from("for-each-ref"),
            OsString::from(format!("--format={REF_FORMAT}")),
            OsString::from("refs/heads"),
            OsString::from("refs/remotes"),
            OsString::from("refs/tags"),
            OsString::from("refs/stash"),
        ];
        let output =
            self.runner
                .run_complete(Some(&repository.root), "refs", args, cancellation)?;
        parse_refs(&output.stdout, repository.object_format)
    }

    pub fn status(
        &self,
        repository: &Repository,
        include_ignored: bool,
        cancellation: &CancellationToken,
    ) -> Result<Status, GitError> {
        if repository.bare {
            return Err(GitError::Unsupported(
                "working-tree status is unavailable in a bare repository".into(),
            ));
        }
        let mut args = vec![
            OsString::from("status"),
            OsString::from("--porcelain=v2"),
            OsString::from("-z"),
            OsString::from("--branch"),
            OsString::from("--untracked-files=all"),
        ];
        if include_ignored {
            args.push(OsString::from("--ignored=matching"));
        }
        let output =
            self.runner
                .run_complete(Some(&repository.root), "status", args, cancellation)?;
        parse_status(&output.stdout, repository.object_format)
    }

    pub fn tree(
        &self,
        repository: &Repository,
        revision: &str,
        path: Option<&GitPath>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<TreeEntry>, GitError> {
        let resolved = self.resolve(
            &repository.root,
            revision,
            repository.object_format,
            cancellation,
        )?;
        let treeish = match path {
            Some(path) => revision_path_spec(&resolved.hex, path)?,
            None => OsString::from(resolved.to_string()),
        };
        let args = vec![
            OsString::from("ls-tree"),
            OsString::from("-z"),
            OsString::from("-l"),
            OsString::from("--end-of-options"),
            treeish,
        ];
        let output =
            self.runner
                .run_complete(Some(&repository.root), "tree", args, cancellation)?;
        parse_tree(&output.stdout, repository.object_format)
    }

    pub fn blob(
        &self,
        repository: &Repository,
        id: &Oid,
        path: Option<GitPath>,
        cancellation: &CancellationToken,
    ) -> Result<Blob, GitError> {
        let size_output = self.runner.run_complete(
            Some(&repository.root),
            "blob-size",
            [
                OsString::from("cat-file"),
                OsString::from("-s"),
                OsString::from("--end-of-options"),
                OsString::from(id.to_string()),
            ],
            cancellation,
        )?;
        let size = parse_single_usize(&size_output.stdout, "blob-size")?;
        let output = self.runner.run(
            Some(&repository.root),
            "blob",
            [
                OsString::from("cat-file"),
                OsString::from("blob"),
                OsString::from("--end-of-options"),
                OsString::from(id.to_string()),
            ],
            cancellation,
        )?;
        let (bytes, client_truncated) = truncate(&output.stdout, self.max_blob_bytes);
        let truncated = output.stdout_truncated
            || output.stderr_truncated
            || client_truncated
            || bytes.len() < size;
        let probe_bytes = size.min(8_000);
        let binary = if bytes[..bytes.len().min(probe_bytes)].contains(&0) {
            Some(true)
        } else if bytes.len() >= probe_bytes {
            Some(false)
        } else {
            None
        };
        Ok(Blob {
            id: id.clone(),
            path,
            bytes_base64: STANDARD.encode(bytes),
            size,
            binary,
            truncated,
        })
    }

    pub fn blame(
        &self,
        repository: &Repository,
        revision: &str,
        path: &GitPath,
        cancellation: &CancellationToken,
    ) -> Result<Vec<BlameLine>, GitError> {
        // `git blame` does not accept rev-parse's `--end-of-options`; resolve
        // first so the argument cannot be interpreted as an option.
        let resolved = self.resolve(
            &repository.root,
            revision,
            repository.object_format,
            cancellation,
        )?;
        let args = vec![
            OsString::from("blame"),
            OsString::from("--line-porcelain"),
            OsString::from("--root"),
            OsString::from("--no-textconv"),
            OsString::from(resolved.to_string()),
            OsString::from("--"),
            path.to_os_string()?,
        ];
        let output =
            self.runner
                .run_complete(Some(&repository.root), "blame", args, cancellation)?;
        parse_blame(&output.stdout, repository.object_format)
    }

    pub fn stashes(
        &self,
        repository: &Repository,
        cancellation: &CancellationToken,
    ) -> Result<Vec<StashEntry>, GitError> {
        let args = [
            OsString::from("stash"),
            OsString::from("list"),
            OsString::from("-z"),
            OsString::from(format!("--format={STASH_FORMAT}")),
        ];
        let output =
            self.runner
                .run_complete(Some(&repository.root), "stash", args, cancellation)?;
        parse_stashes(&output.stdout, repository.object_format)
    }

    /// Pick a local comparison base without contacting a remote.
    pub fn infer_compare_base(
        &self,
        repository: &Repository,
        cancellation: &CancellationToken,
    ) -> Result<(String, String), GitError> {
        let refs = self.refs(repository, cancellation)?;
        if let Some(upstream) = refs
            .iter()
            .find(|reference| reference.is_head)
            .and_then(|reference| reference.upstream.as_ref())
            && let Some(reference) = refs.iter().find(|reference| {
                reference.short_name.bytes() == upstream.bytes()
                    || reference.full_name.bytes() == upstream.bytes()
            })
        {
            let oid = reference.peeled.as_ref().unwrap_or(&reference.target);
            return Ok((oid.to_string(), upstream.display().to_owned()));
        }
        for preferred in [b"main".as_slice(), b"master".as_slice()] {
            if let Some(reference) = refs.iter().find(|reference| {
                reference.kind == crate::domain::RefKind::LocalBranch
                    && reference.short_name.bytes() == preferred
                    && !reference.is_head
            }) {
                let oid = reference.peeled.as_ref().unwrap_or(&reference.target);
                return Ok((oid.to_string(), reference.short_name.display().to_owned()));
            }
        }
        refs.iter()
            .find(|reference| {
                reference.kind == crate::domain::RefKind::LocalBranch && !reference.is_head
            })
            .map(|reference| {
                (
                    reference
                        .peeled
                        .as_ref()
                        .unwrap_or(&reference.target)
                        .to_string(),
                    reference.short_name.display().to_owned(),
                )
            })
            .ok_or_else(|| GitError::Unsupported("no local comparison base is available".into()))
    }

    /// Read a staged or working-tree patch for one literal path.
    pub fn working_diff(
        &self,
        repository: &Repository,
        path: &GitPath,
        staged: bool,
        cancellation: &CancellationToken,
    ) -> Result<Diff, GitError> {
        if repository.bare {
            return Err(GitError::Unsupported(
                "working-tree diff is unavailable in a bare repository".into(),
            ));
        }
        let mut suffix = Vec::new();
        if staged {
            suffix.push(OsString::from("--cached"));
        }
        suffix.push(OsString::from("--"));
        suffix.push(path.to_os_string()?);
        let patch_args = self.diff_args("diff", "--patch", &suffix);
        let metadata_args = self.diff_args("diff", "--raw", &suffix);
        self.load_diff(
            repository,
            "working-diff",
            patch_args,
            "working-diff-metadata",
            metadata_args,
            cancellation,
        )
    }

    pub fn compare(
        &self,
        repository: &Repository,
        base: &str,
        head: &str,
        mode: ComparisonMode,
        paths: &[GitPath],
        cancellation: &CancellationToken,
    ) -> Result<Comparison, GitError> {
        let resolved_head = self.resolve(
            &repository.root,
            head,
            repository.object_format,
            cancellation,
        )?;
        let requested_base = self.resolve(
            &repository.root,
            base,
            repository.object_format,
            cancellation,
        )?;
        let merge_base = if mode == ComparisonMode::MergeBase {
            let output = match self.runner.run_complete(
                Some(&repository.root),
                "merge-base",
                [
                    OsString::from("merge-base"),
                    OsString::from("--all"),
                    OsString::from("--end-of-options"),
                    OsString::from(requested_base.to_string()),
                    OsString::from(resolved_head.to_string()),
                ],
                cancellation,
            ) {
                Ok(output) => output,
                Err(GitError::Failed {
                    code: Some(1),
                    ref stderr,
                    ..
                }) if stderr.is_empty() => {
                    return Err(GitError::NoMergeBase);
                }
                Err(error) => return Err(error),
            };
            let bases = parse_oid_lines(&output.stdout, repository.object_format, "merge-base")?;
            match bases.as_slice() {
                [base] => Some(base.clone()),
                [] => {
                    return Err(GitError::NoMergeBase);
                }
                _ => {
                    return Err(GitError::AmbiguousMergeBase { count: bases.len() });
                }
            }
        } else {
            None
        };
        let resolved_base = merge_base.clone().unwrap_or_else(|| requested_base.clone());
        let (ahead, behind) =
            self.ahead_behind(repository, &requested_base, &resolved_head, cancellation)?;
        let revisions = [
            OsString::from("--end-of-options"),
            OsString::from(resolved_base.to_string()),
            OsString::from(resolved_head.to_string()),
            OsString::from("--"),
        ];
        let mut patch_args = self.diff_args("diff", "--patch", &revisions);
        let mut metadata_args = self.diff_args("diff", "--raw", &revisions);
        append_paths(&mut patch_args, paths)?;
        append_paths(&mut metadata_args, paths)?;
        let diff = self.load_diff(
            repository,
            "compare",
            patch_args,
            "compare-metadata",
            metadata_args,
            cancellation,
        )?;
        Ok(Comparison {
            mode,
            requested_base: base.to_owned(),
            requested_head: head.to_owned(),
            resolved_base,
            resolved_head,
            merge_base,
            ahead,
            behind,
            diff,
        })
    }

    fn load_diff(
        &self,
        repository: &Repository,
        patch_operation: &'static str,
        patch_args: Vec<OsString>,
        metadata_operation: &'static str,
        metadata_args: Vec<OsString>,
        cancellation: &CancellationToken,
    ) -> Result<Diff, GitError> {
        let metadata = self.runner.run_complete(
            Some(&repository.root),
            metadata_operation,
            metadata_args,
            cancellation,
        )?;
        let identities = parse_raw_diff_metadata(&metadata.stdout)?;
        let patch = self.runner.run(
            Some(&repository.root),
            patch_operation,
            patch_args,
            cancellation,
        )?;
        let (bytes, client_truncated) = truncate(&patch.stdout, self.max_patch_bytes);
        let truncated = patch.stdout_truncated || patch.stderr_truncated || client_truncated;
        parse_diff(bytes, &identities, truncated)
    }

    fn ahead_behind(
        &self,
        repository: &Repository,
        base: &Oid,
        head: &Oid,
        cancellation: &CancellationToken,
    ) -> Result<(usize, usize), GitError> {
        let range = format!("{}...{}", base.hex, head.hex);
        let args = [
            OsString::from("rev-list"),
            OsString::from("--left-right"),
            OsString::from("--count"),
            OsString::from("--end-of-options"),
            OsString::from(range),
        ];
        let output =
            self.runner
                .run_complete(Some(&repository.root), "ahead-behind", args, cancellation)?;
        let (left, right) = parse_exact_pair(&output.stdout, "ahead-behind")?;
        Ok((right, left))
    }

    fn resolve(
        &self,
        root: &Path,
        revision: &str,
        format: ObjectFormat,
        cancellation: &CancellationToken,
    ) -> Result<Oid, GitError> {
        let expression = format!("{revision}^{{commit}}");
        let args = [
            OsString::from("rev-parse"),
            OsString::from("--verify"),
            OsString::from("--end-of-options"),
            OsString::from(expression),
        ];
        let output = self
            .runner
            .run_complete(Some(root), "resolve", args, cancellation)?;
        let value = std::str::from_utf8(&output.stdout)
            .map_err(|error| GitError::Parse {
                operation: "resolve",
                offset: error.valid_up_to(),
                message: error.to_string(),
            })?
            .trim();
        Oid::parse_with_format(value, format).map_err(|error| GitError::Parse {
            operation: "resolve",
            offset: 0,
            message: error.to_string(),
        })
    }

    fn diff_args(&self, command: &str, output_mode: &str, revisions: &[OsString]) -> Vec<OsString> {
        let mut args = vec![
            OsString::from(command),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--color=never"),
            OsString::from("--find-renames"),
            OsString::from(output_mode),
        ];
        if output_mode == "--raw" {
            args.push(OsString::from("-z"));
        } else {
            args.push(OsString::from(format!("--unified={}", self.diff_context)));
            args.push(OsString::from(format!(
                "--diff-algorithm={}",
                self.diff_algorithm
            )));
            match self.diff_whitespace.as_str() {
                "ignore-all" => args.push(OsString::from("--ignore-all-space")),
                "ignore-space-change" => args.push(OsString::from("--ignore-space-change")),
                "ignore-eol" => args.push(OsString::from("--ignore-space-at-eol")),
                _ => {}
            }
        }
        args.extend(revisions.iter().cloned());
        args
    }

    fn diff_tree_args(&self, output_mode: &str, revision: &[OsString]) -> Vec<OsString> {
        let mut args = self.diff_args("diff-tree", output_mode, &[]);
        args.splice(
            1..1,
            [
                OsString::from("--root"),
                OsString::from("--no-commit-id"),
                OsString::from("-r"),
            ],
        );
        args.extend(revision.iter().cloned());
        args
    }
}

#[cfg(unix)]
fn revision_path_spec(revision: &str, path: &GitPath) -> Result<OsString, GitError> {
    use std::os::unix::ffi::OsStringExt;
    let mut value = revision.as_bytes().to_vec();
    value.push(b':');
    value.extend(path.bytes());
    Ok(OsString::from_vec(value))
}

#[cfg(not(unix))]
fn revision_path_spec(revision: &str, path: &GitPath) -> Result<OsString, GitError> {
    let mut value = OsString::from(format!("{revision}:"));
    value.push(path.to_os_string()?);
    Ok(value)
}

fn append_paths(args: &mut Vec<OsString>, paths: &[GitPath]) -> Result<(), GitError> {
    for path in paths {
        args.push(path.to_os_string()?);
    }
    Ok(())
}

fn parse_single_usize(output: &[u8], operation: &'static str) -> Result<usize, GitError> {
    let text = std::str::from_utf8(output).map_err(|error| GitError::Parse {
        operation,
        offset: error.valid_up_to(),
        message: error.to_string(),
    })?;
    let mut fields = text.split_ascii_whitespace();
    let value = fields
        .next()
        .ok_or_else(|| GitError::Parse {
            operation,
            offset: 0,
            message: "expected one integer".into(),
        })?
        .parse()
        .map_err(|error| GitError::Parse {
            operation,
            offset: 0,
            message: format!("invalid integer: {error}"),
        })?;
    if fields.next().is_some() {
        return Err(GitError::Parse {
            operation,
            offset: 0,
            message: "unexpected extra fields".into(),
        });
    }
    Ok(value)
}

fn parse_exact_pair(output: &[u8], operation: &'static str) -> Result<(usize, usize), GitError> {
    let body = output.strip_suffix(b"\n").ok_or_else(|| GitError::Parse {
        operation,
        offset: output.len(),
        message: "count pair is missing its final newline".into(),
    })?;
    if body.contains(&b'\n') || body.contains(&b'\r') {
        return Err(GitError::Parse {
            operation,
            offset: 0,
            message: "count pair contains an unexpected line break".into(),
        });
    }
    let separator = body
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| GitError::Parse {
            operation,
            offset: 0,
            message: "count pair must use one tab separator".into(),
        })?;
    if body[separator + 1..].contains(&b'\t') {
        return Err(GitError::Parse {
            operation,
            offset: separator + 1,
            message: "count pair contains extra fields".into(),
        });
    }
    let left = std::str::from_utf8(&body[..separator]).map_err(|error| GitError::Parse {
        operation,
        offset: error.valid_up_to(),
        message: error.to_string(),
    })?;
    let right = std::str::from_utf8(&body[separator + 1..]).map_err(|error| GitError::Parse {
        operation,
        offset: separator + 1 + error.valid_up_to(),
        message: error.to_string(),
    })?;
    Ok((
        parse_count(left, operation, "left")?,
        parse_count(right, operation, "right")?,
    ))
}

fn parse_count(
    value: &str,
    operation: &'static str,
    label: &'static str,
) -> Result<usize, GitError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(GitError::Parse {
            operation,
            offset: 0,
            message: format!("invalid {label} count"),
        });
    }
    value.parse().map_err(|error| GitError::Parse {
        operation,
        offset: 0,
        message: format!("invalid {label} count: {error}"),
    })
}

fn parse_oid_lines(
    output: &[u8],
    format: ObjectFormat,
    operation: &'static str,
) -> Result<Vec<Oid>, GitError> {
    let mut ids = Vec::new();
    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if ids.len() >= 16 {
            return Err(GitError::Parse {
                operation,
                offset: 0,
                message: "too many object ids".into(),
            });
        }
        let text = std::str::from_utf8(line).map_err(|error| GitError::Parse {
            operation,
            offset: error.valid_up_to(),
            message: error.to_string(),
        })?;
        ids.push(
            Oid::parse_with_format(text, format).map_err(|error| GitError::Parse {
                operation,
                offset: 0,
                message: error.to_string(),
            })?,
        );
    }
    Ok(ids)
}

fn truncate(bytes: &[u8], limit: usize) -> (&[u8], bool) {
    if bytes.len() > limit {
        (&bytes[..limit], true)
    } else {
        (bytes, false)
    }
}

fn has_git_marker(path: &Path) -> bool {
    path.ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
        || (path.join("HEAD").exists() && path.join("objects").exists())
}

fn path_from_output(output: &[u8]) -> Result<std::path::PathBuf, GitError> {
    let value = output.strip_suffix(b"\n").unwrap_or(output);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::path::PathBuf::from(std::ffi::OsString::from_vec(
            value.to_vec(),
        )))
    }
    #[cfg(not(unix))]
    {
        let value = std::str::from_utf8(value).map_err(|error| GitError::Parse {
            operation: "repository-path",
            offset: error.valid_up_to(),
            message: "repository path is not representable on this platform".into(),
        })?;
        Ok(std::path::PathBuf::from(value))
    }
}

fn output_bool(output: &[u8]) -> bool {
    output.starts_with(b"true")
}

fn ensure_minimum_git(version: &str) -> Result<(), GitError> {
    let numeric = version.split_ascii_whitespace().next().unwrap_or(version);
    let mut pieces = numeric.split('.');
    let major: u32 = pieces
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let minor: u32 = pieces
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let patch: u32 = pieces
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if (major, minor, patch) < (2, 45, 1) {
        Err(GitError::UnsupportedGit {
            found: version.to_owned(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_git_versions() {
        assert!(ensure_minimum_git("2.45.1").is_ok());
        assert!(ensure_minimum_git("2.45.0").is_err());
        assert!(ensure_minimum_git("2.44.9").is_err());
        assert!(ensure_minimum_git("3.0.0.windows.1").is_ok());
    }

    #[test]
    fn strictly_parses_count_pairs_and_merge_bases() {
        assert_eq!(parse_exact_pair(b"2\t3\n", "test").unwrap(), (2, 3));
        assert!(parse_exact_pair(b"2", "test").is_err());
        assert!(parse_exact_pair(b"2 3\n", "test").is_err());
        assert!(parse_exact_pair(b"2\n3\n", "test").is_err());
        assert!(parse_exact_pair(b"2\t3\t4\n", "test").is_err());
        assert!(parse_exact_pair(b"x\t3\n", "test").is_err());
        assert!(parse_exact_pair(b"+2\t3\n", "test").is_err());
        let ids = parse_oid_lines(
            format!("{}\n{}\n", "a".repeat(40), "b".repeat(40)).as_bytes(),
            ObjectFormat::Sha1,
            "test",
        )
        .unwrap();
        assert_eq!(ids.len(), 2);
    }
}
