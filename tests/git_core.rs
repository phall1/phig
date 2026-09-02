use std::{ffi::OsStr, fs, path::Path, process::Command};

use phig_cli::{
    domain::{
        ComparisonMode, GitPath, HistoryRange, Oid, RefKind, RefScope, StatusCode, TreeEntryKind,
    },
    git::{CancellationToken, GitClient, GitError, GitLimits, GitRunner},
    runtime::{Coordinator, GitQuery, RequestKey},
};
use tempfile::TempDir;

struct TestRepo {
    directory: TempDir,
}

impl TestRepo {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), ["init", "-b", "main"]);
        git(directory.path(), ["config", "user.name", "Phig Test"]);
        git(
            directory.path(),
            ["config", "user.email", "phig@example.invalid"],
        );
        Self { directory }
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn commit_all(&self, message: &str) {
        git(self.path(), ["add", "--all"]);
        git(self.path(), ["commit", "-m", message]);
    }
}

fn git<I, S>(directory: &Path, args: I) -> Vec<u8>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn exercises_read_only_repository_surface() {
    let repo = TestRepo::new();
    repo.write("src/lib.rs", "pub fn one() -> u8 { 1 }\n");
    repo.write("README.md", "# demo\n");
    repo.commit_all("initial");
    git(repo.path(), ["tag", "v0.1.0"]);

    git(repo.path(), ["checkout", "-b", "feature"]);
    repo.write("src/lib.rs", "pub fn one() -> u8 { 2 }\n");
    repo.write("space name.txt", "hello\n");
    repo.commit_all("feature change");
    git(repo.path(), ["branch", "comma,name"]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    assert_eq!(repository.branch.as_deref(), Some("feature"));
    assert!(!repository.bare);
    assert!(repository.head.is_some());

    let token = CancellationToken::new();
    let history = client
        .history(
            &repository,
            &HistoryRange::revision("HEAD"),
            &[],
            0,
            1,
            &token,
        )
        .unwrap();
    assert_eq!(history.commits.len(), 1);
    assert!(history.has_more);
    assert_eq!(history.commits[0].subject, "feature change");
    assert!(
        history.commits[0]
            .decorations
            .iter()
            .any(|decoration| decoration == "comma,name")
    );

    let detail = client
        .commit_detail(&repository, "HEAD", 0, &[], &token)
        .unwrap();
    assert_eq!(detail.commit.subject, "feature change");
    assert!(detail.diff.files.len() >= 2);
    assert!(
        detail
            .diff
            .lines
            .iter()
            .any(|line| line.text.contains("+pub fn"))
    );

    let refs = client.refs(&repository, &token).unwrap();
    assert!(refs.iter().any(|reference| {
        reference.kind == RefKind::LocalBranch && reference.short_name.display() == "feature"
    }));
    assert!(refs.iter().any(|reference| {
        reference.kind == RefKind::LocalBranch && reference.short_name.display() == "comma,name"
    }));
    assert!(refs.iter().any(|reference| {
        reference.kind == RefKind::Tag && reference.short_name.display() == "v0.1.0"
    }));

    let tree = client.tree(&repository, "HEAD", None, &token).unwrap();
    let source = tree
        .iter()
        .find(|entry| entry.path.display == "src")
        .unwrap();
    assert_eq!(source.kind, TreeEntryKind::Tree);
    let source_path = GitPath::new(b"src".to_vec());
    let nested = client
        .tree(&repository, "HEAD", Some(&source_path), &token)
        .unwrap();
    let blob_entry = nested
        .iter()
        .find(|entry| entry.path.display.ends_with("lib.rs"))
        .unwrap();
    let blob = client
        .blob(
            &repository,
            &blob_entry.id,
            Some(blob_entry.path.clone()),
            &token,
        )
        .unwrap();
    assert!(String::from_utf8(blob.bytes()).unwrap().contains("2"));
    assert_eq!(blob.binary, Some(false));

    let blame = client
        .blame(
            &repository,
            "HEAD",
            &GitPath::new(b"src/lib.rs".to_vec()),
            &token,
        )
        .unwrap();
    assert_eq!(blame.len(), 1);
    assert!(blame[0].content.contains("pub fn"));

    let comparison = client
        .compare(
            &repository,
            "main",
            "feature",
            ComparisonMode::MergeBase,
            &[],
            &token,
        )
        .unwrap();
    assert!(comparison.merge_base.is_some());
    assert_eq!(comparison.ahead, 1);
    assert_eq!(comparison.behind, 0);
    assert!(!comparison.diff.files.is_empty());

    repo.write("scratch.txt", "stash me\n");
    git(repo.path(), ["add", "scratch.txt"]);
    git(repo.path(), ["stash", "push", "-m", "test stash"]);
    let stashes = client.stashes(&repository, &token).unwrap();
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].subject.contains("test stash"));

    repo.write("src/lib.rs", "pub fn one() -> u8 { 3 }\n");
    repo.write("new.txt", "untracked\n");
    let status = client.status(&repository, false, &token).unwrap();
    assert!(status.entries.iter().any(|entry| {
        entry.path.display == "src/lib.rs" && entry.worktree == StatusCode::Modified
    }));
    assert!(
        status.entries.iter().any(|entry| {
            entry.path.display == "new.txt" && entry.index == StatusCode::Untracked
        })
    );
}

#[test]
fn rename_and_output_bounds_are_explicit() {
    let repo = TestRepo::new();
    repo.write("old name.txt", "before\n");
    repo.commit_all("base");
    fs::rename(
        repo.path().join("old name.txt"),
        repo.path().join("new name.txt"),
    )
    .unwrap();
    git(repo.path(), ["add", "--all"]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let status = client
        .status(&repository, false, &CancellationToken::new())
        .unwrap();
    let renamed = status
        .entries
        .iter()
        .find(|entry| entry.index == StatusCode::Renamed)
        .unwrap();
    assert_eq!(renamed.path.display, "new name.txt");
    assert_eq!(
        renamed.original_path.as_ref().unwrap().display,
        "old name.txt"
    );

    let runner = GitRunner::new(
        "git".into(),
        GitLimits {
            stdout_bytes: 4,
            stderr_bytes: 1024,
            timeout: std::time::Duration::from_secs(5),
        },
    );
    let error = runner
        .run_simple(None, "version", &["--version"])
        .unwrap_err();
    assert!(matches!(error, GitError::OutputLimit { .. }));
}

#[test]
fn bounded_patch_and_blob_reads_return_explicit_truncation() {
    let repo = TestRepo::new();
    repo.write("large.txt", &"a\n".repeat(12_000));
    repo.commit_all("base");
    repo.write("large.txt", &"b\n".repeat(12_000));
    repo.commit_all("large change");

    let default_client = GitClient::default();
    let repository = default_client.discover(repo.path()).unwrap();
    let runner = GitRunner::new(
        "git".into(),
        GitLimits {
            stdout_bytes: 2_048,
            stderr_bytes: 1_024,
            timeout: std::time::Duration::from_secs(5),
        },
    );
    let bounded = GitClient::new(runner).with_content_limits(1_024, 1_024);
    let detail = bounded
        .commit_detail(&repository, "HEAD", 0, &[], &CancellationToken::new())
        .unwrap();
    assert!(detail.diff.truncated);
    assert!(!detail.diff.lines.is_empty());

    let tree = default_client
        .tree(&repository, "HEAD", None, &CancellationToken::new())
        .unwrap();
    let text_id = tree
        .iter()
        .find(|entry| entry.path.display == "large.txt")
        .unwrap()
        .id
        .clone();
    let text = bounded
        .blob(&repository, &text_id, None, &CancellationToken::new())
        .unwrap();
    assert_eq!(text.size, 24_000);
    assert!(text.truncated);
    assert_eq!(text.binary, None);

    let mut binary_bytes = vec![b'x'; 12_000];
    binary_bytes[3] = 0;
    fs::write(repo.path().join("binary"), &binary_bytes).unwrap();
    repo.commit_all("binary");
    let repository = default_client.discover(repo.path()).unwrap();
    let tree = default_client
        .tree(&repository, "HEAD", None, &CancellationToken::new())
        .unwrap();
    let binary_id = tree
        .iter()
        .find(|entry| entry.path.display == "binary")
        .unwrap()
        .id
        .clone();
    let binary = bounded
        .blob(&repository, &binary_id, None, &CancellationToken::new())
        .unwrap();
    assert_eq!(binary.binary, Some(true));
    assert!(binary.truncated);
}

#[test]
fn structured_queries_reject_incomplete_bounded_output() {
    let repo = TestRepo::new();
    repo.write("file", "x\n");
    repo.commit_all(&"large-subject".repeat(100));
    let repository = GitClient::default().discover(repo.path()).unwrap();
    let client = GitClient::new(GitRunner::new(
        "git".into(),
        GitLimits {
            stdout_bytes: 128,
            stderr_bytes: 1_024,
            timeout: std::time::Duration::from_secs(5),
        },
    ));
    let result = client.history(
        &repository,
        &HistoryRange::revision("HEAD"),
        &[],
        0,
        10,
        &CancellationToken::new(),
    );
    assert!(matches!(result, Err(GitError::OutputLimit { .. })));
}

#[test]
fn real_merge_conflict_exposes_all_three_index_stages() {
    let repo = TestRepo::new();
    repo.write("conflict.txt", "base\n");
    repo.commit_all("base");
    git(repo.path(), ["checkout", "-b", "side"]);
    repo.write("conflict.txt", "side\n");
    repo.commit_all("side");
    git(repo.path(), ["checkout", "main"]);
    repo.write("conflict.txt", "main\n");
    repo.commit_all("main");
    let merge = Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["merge", "side"])
        .output()
        .unwrap();
    assert!(!merge.status.success());

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let status = client
        .status(&repository, false, &CancellationToken::new())
        .unwrap();
    let entry = status
        .entries
        .iter()
        .find(|entry| entry.path.display == "conflict.txt")
        .unwrap();
    let stages = entry.conflict.as_ref().expect("unmerged stages");
    for (stage, selector) in [
        (&stages.base, ":1:conflict.txt"),
        (&stages.ours, ":2:conflict.txt"),
        (&stages.theirs, ":3:conflict.txt"),
    ] {
        let expected = String::from_utf8(git(repo.path(), ["rev-parse", selector]))
            .unwrap()
            .trim()
            .to_owned();
        assert_eq!(stage.mode, "100644");
        assert_eq!(stage.oid.as_ref().unwrap().hex, expected);
    }
}

#[test]
fn cancellation_is_observed_before_spawn() {
    let token = CancellationToken::new();
    token.cancel();
    let error = GitRunner::default()
        .run(None, "cancel-test", ["--version"], &token)
        .unwrap_err();
    assert!(matches!(error, GitError::Cancelled("cancel-test")));
}

#[test]
fn commit_and_tree_operations_require_singletons_and_valid_parents() {
    let repo = TestRepo::new();
    repo.write("root", "root\n");
    repo.commit_all("root");
    let root = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(repo.path(), ["checkout", "-b", "side"]);
    repo.write("side", "side\n");
    repo.commit_all("side");
    git(repo.path(), ["checkout", "main"]);
    repo.write("main", "main\n");
    repo.commit_all("main");
    git(repo.path(), ["merge", "--no-ff", "side", "-m", "merge"]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let token = CancellationToken::new();
    let root_detail = client
        .commit_detail(&repository, &root, 0, &[], &token)
        .unwrap();
    assert!(root_detail.selected_parent.is_none());
    assert_eq!(root_detail.diff.files.len(), 1);
    assert!(root_detail.diff.files[0].old_path.is_none());
    assert_eq!(
        root_detail.diff.files[0].new_path.as_ref().unwrap().bytes(),
        b"root"
    );
    assert!(matches!(
        client.commit_detail(&repository, &root, 1, &[], &token),
        Err(GitError::InvalidParentIndex { .. })
    ));
    let merge = client
        .commit_detail(&repository, "HEAD", 0, &[], &token)
        .unwrap();
    assert_eq!(merge.commit.parents.len(), 2);
    assert_eq!(merge.diff.files.len(), 1);
    assert_eq!(
        merge.diff.files[0].new_path.as_ref().unwrap().bytes(),
        b"side"
    );
    let second = client
        .commit_detail(&repository, "HEAD", 1, &[], &token)
        .unwrap();
    assert_eq!(second.selected_parent, merge.commit.parents.get(1).cloned());
    assert_eq!(second.diff.files.len(), 1);
    assert_eq!(
        second.diff.files[0].new_path.as_ref().unwrap().bytes(),
        b"main"
    );
    assert!(matches!(
        client.commit_detail(&repository, "HEAD", 2, &[], &token),
        Err(GitError::InvalidParentIndex { .. })
    ));
    assert!(
        client
            .commit_detail(&repository, "HEAD~2..HEAD", 0, &[], &token)
            .is_err()
    );
    assert!(
        client
            .tree(&repository, "HEAD~2..HEAD", None, &token)
            .is_err()
    );
}

#[test]
fn multiple_merge_bases_are_rejected_explicitly() {
    let repo = TestRepo::new();
    repo.write("root", "root\n");
    repo.commit_all("root");
    let root = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    repo.write("a", "a\n");
    repo.commit_all("a");
    let a = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(repo.path(), ["reset", "--hard", &root]);
    repo.write("b", "b\n");
    repo.commit_all("b");
    let b = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let tree_a = String::from_utf8(git(repo.path(), ["show", "-s", "--format=%T", &a]))
        .unwrap()
        .trim()
        .to_owned();
    let tree_b = String::from_utf8(git(repo.path(), ["show", "-s", "--format=%T", &b]))
        .unwrap()
        .trim()
        .to_owned();
    let m1 = String::from_utf8(git(
        repo.path(),
        ["commit-tree", &tree_a, "-p", &a, "-p", &b, "-m", "m1"],
    ))
    .unwrap()
    .trim()
    .to_owned();
    let m2 = String::from_utf8(git(
        repo.path(),
        ["commit-tree", &tree_b, "-p", &b, "-p", &a, "-m", "m2"],
    ))
    .unwrap()
    .trim()
    .to_owned();
    git(repo.path(), ["branch", "m1", &m1]);
    git(repo.path(), ["branch", "m2", &m2]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let result = client.compare(
        &repository,
        "m1",
        "m2",
        ComparisonMode::MergeBase,
        &[],
        &CancellationToken::new(),
    );
    assert!(matches!(
        result,
        Err(GitError::AmbiguousMergeBase { count: 2 })
    ));
}

#[test]
fn unrelated_histories_report_no_merge_base() {
    let repo = TestRepo::new();
    repo.write("root", "root\n");
    repo.commit_all("root");
    let tree = String::from_utf8(git(repo.path(), ["show", "-s", "--format=%T", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let independent = String::from_utf8(git(
        repo.path(),
        ["commit-tree", &tree, "-m", "independent"],
    ))
    .unwrap()
    .trim()
    .to_owned();
    git(repo.path(), ["branch", "independent", &independent]);
    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let result = client.compare(
        &repository,
        "main",
        "independent",
        ComparisonMode::MergeBase,
        &[],
        &CancellationToken::new(),
    );
    assert!(matches!(result, Err(GitError::NoMergeBase)));
}

#[test]
fn discovers_sha256_repository_when_git_supports_it() {
    let directory = tempfile::tempdir().unwrap();
    let init = Command::new("git")
        .arg("-C")
        .arg(directory.path())
        .args(["init", "--object-format=sha256", "-b", "main"])
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "Git 2.45.1+ should support SHA-256 repositories: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    git(directory.path(), ["config", "user.name", "Phig Test"]);
    git(
        directory.path(),
        ["config", "user.email", "phig@example.invalid"],
    );
    fs::write(directory.path().join("file"), b"data\n").unwrap();
    git(directory.path(), ["add", "file"]);
    git(directory.path(), ["commit", "-m", "sha256"]);

    let client = GitClient::default();
    let repository = client.discover(directory.path()).unwrap();
    assert_eq!(
        repository.object_format,
        phig_cli::domain::ObjectFormat::Sha256
    );
    assert_eq!(repository.head.as_ref().unwrap().hex.len(), 64);
    let history = client
        .history(
            &repository,
            &HistoryRange::revision("HEAD"),
            &[],
            0,
            10,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(history.commits[0].id.hex.len(), 64);
    let detail = client
        .commit_detail(&repository, "HEAD", 0, &[], &CancellationToken::new())
        .unwrap();
    assert!(detail.selected_parent.is_none());
    assert_eq!(detail.commit.subject, "sha256");
}

#[cfg(unix)]
#[test]
fn configured_external_diff_is_never_executed() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("one");
    repo.write("file", "two\n");
    repo.commit_all("two");
    let canary = repo.path().join("external-diff-ran");
    let helper = repo.path().join("external-diff-helper");
    fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\n", canary.display()),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    git(
        repo.path(),
        [
            OsStr::new("config"),
            OsStr::new("diff.external"),
            helper.as_os_str(),
        ],
    );

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let detail = client
        .commit_detail(&repository, "HEAD", 0, &[], &CancellationToken::new())
        .unwrap();
    assert!(!detail.diff.lines.is_empty());
    assert!(!canary.exists(), "repository-configured helper executed");
}

#[test]
fn binary_diff_paths_with_embedded_b_slash_are_authoritative() {
    let repo = TestRepo::new();
    let path = "alpha b/blob.bin";
    let full_path = repo.path().join(path);
    fs::create_dir_all(full_path.parent().unwrap()).unwrap();
    fs::write(&full_path, b"\0root binary").unwrap();
    repo.commit_all("binary root");
    let root = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let token = CancellationToken::new();
    let root_detail = client
        .commit_detail(&repository, &root, 0, &[], &token)
        .unwrap();
    assert_eq!(root_detail.diff.files.len(), 1);
    assert!(root_detail.diff.files[0].old_path.is_none());
    assert_eq!(
        root_detail.diff.files[0].new_path.as_ref().unwrap().bytes(),
        path.as_bytes()
    );
    let header = root_detail.diff.files[0].header_line;
    assert!(
        root_detail.diff.lines[header]
            .text
            .starts_with("diff --git ")
    );
    assert!(
        root_detail
            .diff
            .lines
            .iter()
            .any(|line| line.text.contains("Binary files"))
    );

    fs::write(&full_path, b"\0changed binary").unwrap();
    repo.commit_all("binary changed");
    let detail = client
        .commit_detail(&repository, "HEAD", 0, &[], &token)
        .unwrap();
    assert_eq!(
        detail.diff.files[0].old_path.as_ref().unwrap().bytes(),
        path.as_bytes()
    );
    assert_eq!(
        detail.diff.files[0].new_path.as_ref().unwrap().bytes(),
        path.as_bytes()
    );

    let comparison = client
        .compare(
            &repository,
            &root,
            "HEAD",
            ComparisonMode::Exact,
            &[],
            &token,
        )
        .unwrap();
    assert_eq!(comparison.diff.files.len(), 1);
    assert_eq!(
        comparison.diff.files[0].old_path.as_ref().unwrap().bytes(),
        path.as_bytes()
    );
    assert_eq!(
        comparison.diff.files[0].new_path.as_ref().unwrap().bytes(),
        path.as_bytes()
    );
}

#[test]
fn diff_prefix_configuration_cannot_change_parser_contract() {
    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("one");
    repo.write("file", "two\n");
    repo.commit_all("two");
    git(repo.path(), ["config", "diff.mnemonicPrefix", "true"]);
    git(repo.path(), ["config", "diff.noprefix", "true"]);
    git(repo.path(), ["config", "diff.srcPrefix", "old/"]);
    git(repo.path(), ["config", "diff.dstPrefix", "new/"]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let detail = client
        .commit_detail(&repository, "HEAD", 0, &[], &CancellationToken::new())
        .unwrap();
    assert_eq!(
        detail.diff.files[0].old_path.as_ref().unwrap().bytes(),
        b"file"
    );
    assert_eq!(
        detail.diff.files[0].new_path.as_ref().unwrap().bytes(),
        b"file"
    );
}

#[test]
fn pathspec_magic_is_always_treated_literally() {
    let repo = TestRepo::new();
    repo.write("x.rs", "ordinary\n");
    repo.commit_all("ordinary");
    repo.write("*.rs", "literal glob\n");
    repo.commit_all("literal glob");
    repo.write(":(glob)*", "literal magic\n");
    repo.commit_all("literal magic");

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let glob_history = client
        .history(
            &repository,
            &HistoryRange::revision("HEAD"),
            &[GitPath::new(b"*.rs".to_vec())],
            0,
            10,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(glob_history.commits.len(), 1);
    assert_eq!(glob_history.commits[0].subject, "literal glob");
    let magic_history = client
        .history(
            &repository,
            &HistoryRange::revision("HEAD"),
            &[GitPath::new(b":(glob)*".to_vec())],
            0,
            10,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(magic_history.commits.len(), 1);
    assert_eq!(magic_history.commits[0].subject, "literal magic");
}

#[test]
fn replacement_objects_do_not_change_inspected_history() {
    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("original subject");
    let original = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    repo.write("file", "two\n");
    repo.commit_all("current subject");
    let current = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    git(repo.path(), ["replace", &current, &original]);

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let history = client
        .history(
            &repository,
            &HistoryRange::revision("HEAD"),
            &[],
            0,
            1,
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(history.commits[0].subject, "current subject");
}

#[cfg(unix)]
#[test]
fn cancellation_and_descendant_pipe_cleanup_are_bounded() {
    use std::os::unix::fs::PermissionsExt;
    use std::{
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-git");
    fs::write(&helper, "#!/bin/sh\nsleep 30 &\nwait\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = GitRunner::new(
        helper,
        GitLimits {
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            timeout: Duration::from_secs(10),
        },
    );
    let token = CancellationToken::new();
    let child_token = token.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(runner.run(None, "cancel-running", ["ignored"], &child_token));
    });
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    token.cancel();
    let result = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(result, Err(GitError::Cancelled("cancel-running"))));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn successful_parent_cleans_descendant_that_retains_pipes() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-git");
    fs::write(&helper, "#!/bin/sh\nsleep 30 &\nexit 0\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = GitRunner::new(
        helper,
        GitLimits {
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            timeout: std::time::Duration::from_secs(10),
        },
    );
    let started = std::time::Instant::now();
    runner
        .run_simple(None, "successful-parent", &["ignored"])
        .unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn failed_output_reports_truncation() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-git");
    fs::write(&helper, "#!/bin/sh\nprintf '0123456789' >&2\nexit 7\n").unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = GitRunner::new(
        helper,
        GitLimits {
            stdout_bytes: 1024,
            stderr_bytes: 4,
            timeout: std::time::Duration::from_secs(2),
        },
    );
    let error = runner.run_simple(None, "failed", &["ignored"]).unwrap_err();
    assert!(matches!(
        error,
        GitError::Failed {
            truncated: true,
            ..
        }
    ));
}

#[test]
fn corrupt_repository_failure_is_not_collapsed_to_not_repository() {
    let repo = TestRepo::new();
    fs::write(repo.path().join(".git/HEAD"), b"not a valid head\n").unwrap();
    let error = GitClient::default().discover(repo.path()).unwrap_err();
    assert!(
        !matches!(error, GitError::NotRepository(_)),
        "corrupt repository was misclassified: {error:?}"
    );
}

#[cfg(unix)]
#[test]
fn safe_directory_failure_is_preserved() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-git");
    fs::write(
        &helper,
        "#!/bin/sh\ncase \"$*\" in *--version*) echo 'git version 2.45.1'; exit 0;; esac\necho 'fatal: detected dubious ownership in repository' >&2\nexit 128\n",
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let client = GitClient::new(GitRunner::new(helper, GitLimits::default()));
    let error = client.discover(directory.path()).unwrap_err();
    assert!(matches!(error, GitError::Failed { .. }));
    assert!(error.to_string().contains("dubious ownership"));
}

#[test]
fn unborn_head_is_the_only_missing_head_case() {
    let repo = TestRepo::new();
    let repository = GitClient::default().discover(repo.path()).unwrap();
    assert_eq!(repository.branch.as_deref(), Some("main"));
    assert!(repository.head.is_none());
}

#[test]
fn non_repository_has_typed_error() {
    let directory = tempfile::tempdir().unwrap();
    let error = GitClient::default().discover(directory.path()).unwrap_err();
    assert!(matches!(error, GitError::NotRepository(_)));
}

#[test]
fn saturated_response_channel_delivers_current_results() {
    let repo = TestRepo::new();
    repo.write("file", "one\n");
    repo.commit_all("one");
    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let coordinator = Coordinator::new(client, 2, 1);
    coordinator
        .submit(
            RequestKey::Refs,
            GitQuery::Refs {
                repository: repository.clone(),
            },
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match coordinator.submit(
            RequestKey::Status,
            GitQuery::Status {
                repository: repository.clone(),
                include_ignored: false,
            },
        ) {
            Ok(_) => break,
            Err(phig_cli::runtime::CoordinatorError::Busy)
                if std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => panic!("second query was not accepted: {other:?}"),
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let first = coordinator
        .responses()
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let second = coordinator
        .responses()
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    assert!(first.result.is_ok());
    assert!(second.result.is_ok());
    assert_ne!(first.key, second.key);
}

#[test]
fn control_byte_paths_remain_lossless_and_safe_to_display() {
    let repo = TestRepo::new();
    repo.write("normal", "x\n");
    repo.commit_all("base");
    let raw = b"bad\n\x1bname".to_vec();
    fs::write(repo.path().join("bad\n\u{1b}name"), b"x").unwrap();

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let status = client
        .status(&repository, false, &CancellationToken::new())
        .unwrap();
    let entry = status
        .entries
        .iter()
        .find(|entry| entry.path.bytes() == raw)
        .expect("control-byte path was preserved");
    assert!(entry.path.display.contains("\\n"));
    assert!(entry.path.display.contains("\\e"));
    assert!(!entry.path.display.contains('\n'));
    assert!(!entry.path.display.contains('\u{1b}'));
}

#[test]
fn bare_and_empty_inspection_states_are_explicit() {
    let repo = TestRepo::new();
    repo.write("file", "x\n");
    repo.commit_all("base");
    let bare = tempfile::tempdir().unwrap();
    let source = repo.path().to_str().unwrap();
    git(bare.path(), ["clone", "--bare", source, "."]);
    let client = GitClient::default();
    let repository = client.discover(bare.path()).unwrap();
    assert!(repository.bare);
    assert!(matches!(
        client.status(&repository, false, &CancellationToken::new()),
        Err(GitError::Unsupported(_))
    ));
    let normal = client.discover(repo.path()).unwrap();
    assert!(
        client
            .stashes(&normal, &CancellationToken::new())
            .unwrap()
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn inferred_non_utf8_ref_uses_oid_not_sanitized_display_as_revision() {
    let repo = TestRepo::new();
    repo.write("file.txt", "one\n");
    repo.commit_all("base");
    let head = String::from_utf8(git(repo.path(), ["rev-parse", "HEAD"])).unwrap();
    git(repo.path(), ["checkout", "-b", "feature"]);
    git(repo.path(), ["branch", "-D", "main"]);
    let mut packed = b"# pack-refs with: peeled fully-peeled sorted\n".to_vec();
    packed.extend_from_slice(head.trim().as_bytes());
    packed.extend_from_slice(b" refs/heads/base-\xff\n");
    fs::write(repo.path().join(".git/packed-refs"), packed).unwrap();

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let (revision, label) = client
        .infer_compare_base(&repository, &CancellationToken::new())
        .unwrap();
    assert!(revision.parse::<Oid>().is_ok());
    assert!(label.contains("\\xFF"));
    let comparison = client
        .compare(
            &repository,
            &revision,
            "HEAD",
            ComparisonMode::MergeBase,
            &[],
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(comparison.resolved_base, comparison.resolved_head);
}

#[test]
fn comparison_base_inference_and_working_diffs_are_read_only() {
    let repo = TestRepo::new();
    repo.write("file.txt", "one\n");
    repo.commit_all("base");
    git(repo.path(), ["branch", "main-base"]);
    git(repo.path(), ["checkout", "-b", "feature"]);
    repo.write("file.txt", "one\ntwo\n");

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let token = CancellationToken::new();
    let (base, label) = client.infer_compare_base(&repository, &token).unwrap();
    assert_eq!(label, "main");
    assert!(
        base.parse::<Oid>().is_ok(),
        "inferred input must be an authoritative OID"
    );
    let diff = client
        .working_diff(
            &repository,
            &GitPath::new(b"file.txt".to_vec()),
            false,
            &token,
        )
        .unwrap();
    assert!(diff.lines.iter().any(|line| line.text == "+two"));

    git(repo.path(), ["add", "file.txt"]);
    let staged = client
        .working_diff(
            &repository,
            &GitPath::new(b"file.txt".to_vec()),
            true,
            &token,
        )
        .unwrap();
    assert!(staged.lines.iter().any(|line| line.text == "+two"));
    let status = client.status(&repository, false, &token).unwrap();
    assert_eq!(
        status.entries.len(),
        1,
        "inspection changed repository state"
    );
}

#[test]
fn ref_scope_walks_ref_families_instead_of_only_head() {
    let repo = TestRepo::new();
    repo.write("base.txt", "base\n");
    repo.commit_all("base");
    git(repo.path(), ["checkout", "-b", "side"]);
    repo.write("side.txt", "side\n");
    repo.commit_all("side-only");
    git(repo.path(), ["checkout", "main"]);
    repo.write("main.txt", "main\n");
    repo.commit_all("main-only");
    git(repo.path(), ["tag", "release", "side"]);
    git(
        repo.path(),
        ["update-ref", "refs/remotes/origin/side", "refs/heads/side"],
    );

    let client = GitClient::default();
    let repository = client.discover(repo.path()).unwrap();
    let subjects = |revision: Option<&str>, scope: RefScope| {
        let range = HistoryRange {
            revision: revision.map(str::to_owned),
            scope,
        };
        let page = client
            .history(&repository, &range, &[], 0, 50, &CancellationToken::new())
            .unwrap();
        page.commits
            .into_iter()
            .map(|commit| commit.subject)
            .collect::<Vec<_>>()
    };

    // The default walk still sees only HEAD's ancestry.
    let head = subjects(Some("HEAD"), RefScope::default());
    assert!(head.contains(&"main-only".to_owned()));
    assert!(!head.contains(&"side-only".to_owned()));

    for scope in [
        RefScope {
            all: true,
            ..RefScope::default()
        },
        RefScope {
            branches: true,
            ..RefScope::default()
        },
    ] {
        let walked = subjects(None, scope);
        assert!(
            walked.contains(&"side-only".to_owned()) && walked.contains(&"main-only".to_owned()),
            "{scope:?} missed a branch: {walked:?}"
        );
    }

    // A scope naming only non-HEAD families must not fold HEAD back in.
    for scope in [
        RefScope {
            remotes: true,
            ..RefScope::default()
        },
        RefScope {
            tags: true,
            ..RefScope::default()
        },
    ] {
        let walked = subjects(None, scope);
        assert!(
            walked.contains(&"side-only".to_owned()) && !walked.contains(&"main-only".to_owned()),
            "{scope:?} leaked HEAD: {walked:?}"
        );
    }

    // An explicit endpoint is unioned with the scope rather than replaced.
    let union = subjects(
        Some("HEAD"),
        RefScope {
            remotes: true,
            ..RefScope::default()
        },
    );
    assert!(union.contains(&"main-only".to_owned()) && union.contains(&"side-only".to_owned()));
}
