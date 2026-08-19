use std::{ffi::OsString, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    app::{App, View},
    cli::{BlameArgs, SelectionKind, SnapshotTarget, TreeArgs},
    domain::{ComparisonMode, Diff, GitPath, Oid, Repository},
    git::{CancellationToken, GitClient, GitError},
};

pub const PROTOCOL: &str = "phig/1";

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("snapshot target `{target}` does not accept a nonzero --offset")]
    InvalidOffset { target: &'static str },
}

#[derive(Debug, Serialize)]
pub struct Envelope<T> {
    pub protocol: &'static str,
    pub kind: &'static str,
    pub payload: T,
}
impl<T> Envelope<T> {
    pub fn new(kind: &'static str, payload: T) -> Self {
        Self {
            protocol: PROTOCOL,
            kind,
            payload,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryLocator {
    pub root: EncodedPath,
    pub generation: Option<Oid>,
    pub object_format: String,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EncodedPath {
    pub display: String,
    pub bytes_base64: String,
}

impl RepositoryLocator {
    pub fn from_repository(repo: &Repository) -> Self {
        Self {
            root: encoded_native(&repo.root),
            generation: repo.head.clone(),
            object_format: repo.object_format.name().into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPayload {
    pub repository: RepositoryLocator,
    pub target: &'static str,
    pub request: Value,
    pub offset: usize,
    pub data: Value,
    pub truncated: bool,
    pub continuation: Option<usize>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPayload {
    pub repository: EncodedPath,
    pub generation: Option<Oid>,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oid: Option<Oid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<Oid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<GitPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<GitPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<GitPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<GitPath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hunk: Option<HunkLocator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineLocator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare: Option<CompareLocator>,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HunkLocator {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LineLocator {
    pub number: usize,
    pub original_number: usize,
}
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompareLocator {
    pub mode: ComparisonMode,
    pub base: Oid,
    pub head: Oid,
    pub merge_base: Option<Oid>,
}

impl SelectionPayload {
    fn base(app: &App, kind: &'static str) -> Self {
        Self {
            repository: encoded_native(&app.repository.root),
            generation: app.repository.head.clone(),
            kind,
            oid: None,
            parent: None,
            path: None,
            reference: None,
            old_path: None,
            source_path: None,
            hunk: None,
            line: None,
            compare: None,
        }
    }
}

pub fn selection_from_app(app: &App, kind: SelectionKind) -> Option<SelectionPayload> {
    match kind {
        SelectionKind::Commit => {
            let commit = app
                .selected_commit()
                .or_else(|| app.preview.as_ref().map(|d| &d.commit))?;
            let mut s = SelectionPayload::base(app, "commit");
            s.oid = Some(commit.id.clone());
            s.parent = app.preview.as_ref().and_then(|d| d.selected_parent.clone());
            Some(s)
        }
        SelectionKind::Ref => {
            let r = app.inspect.refs.get(app.inspect.selected)?;
            let mut s = SelectionPayload::base(app, "ref");
            s.oid = Some(r.peeled.clone().unwrap_or_else(|| r.target.clone()));
            s.reference = Some(r.full_name.0.clone());
            Some(s)
        }
        SelectionKind::File => {
            let diff = active_diff(app)?;
            let file = file_at(diff, app.diff_scroll)?;
            let mut s = SelectionPayload::base(app, "file");
            s.oid = current_oid(app);
            s.parent = current_parent(app);
            s.path = file.new_path.clone().or_else(|| file.old_path.clone());
            s.old_path = file.old_path.clone();
            Some(s)
        }
        SelectionKind::Hunk => {
            let diff = active_diff(app)?;
            let file = file_at(diff, app.diff_scroll)?;
            let h = file
                .hunks
                .iter()
                .rev()
                .find(|h| h.header_line <= app.diff_scroll)
                .or_else(|| file.hunks.first())?;
            let mut s = SelectionPayload::base(app, "hunk");
            s.oid = current_oid(app);
            s.parent = current_parent(app);
            s.path = file.new_path.clone().or_else(|| file.old_path.clone());
            s.old_path = file.old_path.clone();
            s.hunk = Some(HunkLocator {
                old_start: h.old_start,
                old_lines: h.old_lines,
                new_start: h.new_start,
                new_lines: h.new_lines,
            });
            Some(s)
        }
        SelectionKind::Line => {
            let line = app.inspect.blame.get(app.inspect.selected)?;
            let mut s = SelectionPayload::base(app, "line");
            s.oid = Some(line.id.clone());
            s.path = app.inspect.blame_path.clone();
            s.source_path =
                (s.path.as_ref() != Some(&line.filename)).then(|| line.filename.clone());
            s.line = Some(LineLocator {
                number: line.final_line,
                original_number: line.original_line,
            });
            Some(s)
        }
        SelectionKind::Compare => {
            let c = app.inspect.comparison.as_ref()?;
            let mut s = SelectionPayload::base(app, "compare");
            s.oid = Some(c.resolved_head.clone());
            s.compare = Some(CompareLocator {
                mode: c.mode,
                base: c.resolved_base.clone(),
                head: c.resolved_head.clone(),
                merge_base: c.merge_base.clone(),
            });
            Some(s)
        }
    }
}
fn active_diff(app: &App) -> Option<&Diff> {
    match app.view {
        View::Detail => app.preview.as_ref().map(|d| &d.diff),
        View::Compare => app.inspect.comparison.as_ref().map(|c| &c.diff),
        View::Status | View::StatusDiff => app.inspect.working_diff.as_ref(),
        _ => app.preview.as_ref().map(|d| &d.diff),
    }
}
fn file_at(diff: &Diff, line: usize) -> Option<&crate::domain::DiffFile> {
    diff.files
        .iter()
        .rev()
        .find(|f| f.header_line <= line)
        .or_else(|| diff.files.first())
}
fn current_oid(app: &App) -> Option<Oid> {
    app.preview
        .as_ref()
        .map(|d| d.commit.id.clone())
        .or_else(|| {
            app.inspect
                .comparison
                .as_ref()
                .map(|c| c.resolved_head.clone())
        })
        .or_else(|| app.repository.head.clone())
}
fn current_parent(app: &App) -> Option<Oid> {
    app.preview.as_ref().and_then(|d| d.selected_parent.clone())
}

pub fn snapshot(
    client: &GitClient,
    repo: &Repository,
    target: &SnapshotTarget,
    offset: usize,
    limit: usize,
) -> Result<Envelope<SnapshotPayload>, SnapshotError> {
    let request = snapshot_request(target);
    let token = CancellationToken::new();
    let truncated;
    let (name, data, continuation) = match target {
        SnapshotTarget::Log(a) => {
            let paths = git_paths(&a.paths);
            let page = client.history(repo, &a.revision, &paths, offset, limit, &token)?;
            truncated = page.has_more;
            (
                "log",
                serde_json::to_value(&page).expect("serializable"),
                page.has_more
                    .then_some(offset.saturating_add(page.commits.len())),
            )
        }
        SnapshotTarget::Show(a) => {
            reject_offset(offset, "show")?;
            let paths = git_paths(&a.paths);
            let d = client.commit_detail(repo, &a.revision, 0, &paths, &token)?;
            truncated = d.diff.truncated;
            ("show", serde_json::to_value(d).expect("serializable"), None)
        }
        SnapshotTarget::Compare(a) => {
            reject_offset(offset, "compare")?;
            let paths = git_paths(&a.paths);
            let (base, _) = match &a.base {
                Some(b) => (b.clone(), b.clone()),
                None => client.infer_compare_base(repo, &token)?,
            };
            let d = client.compare(
                repo,
                &base,
                &a.head,
                ComparisonMode::MergeBase,
                &paths,
                &token,
            )?;
            truncated = d.diff.truncated;
            (
                "compare",
                serde_json::to_value(d).expect("serializable"),
                None,
            )
        }
        SnapshotTarget::Diff(a) => {
            reject_offset(offset, "diff")?;
            let paths = git_paths(&a.paths);
            let d = client.compare(
                repo,
                &a.left,
                &a.right,
                ComparisonMode::Exact,
                &paths,
                &token,
            )?;
            truncated = d.diff.truncated;
            ("diff", serde_json::to_value(d).expect("serializable"), None)
        }
        SnapshotTarget::Refs => {
            let mut v = client.refs(repo, &token)?;
            v.sort_by(|a, b| a.full_name.bytes().cmp(&b.full_name.bytes()));
            let (v, more, next) = paginate(v, offset, limit);
            truncated = more;
            ("refs", serde_json::to_value(v).expect("serializable"), next)
        }
        SnapshotTarget::Status => {
            let mut v = client.status(repo, false, &token)?;
            v.entries
                .sort_by(|a, b| a.path.bytes().cmp(&b.path.bytes()));
            let (entries, more, next) = paginate(v.entries, offset, limit);
            v.entries = entries;
            truncated = more;
            (
                "status",
                serde_json::to_value(v).expect("serializable"),
                next,
            )
        }
        SnapshotTarget::Tree(TreeArgs { revision, path }) => {
            let p = path.first().cloned().map(git_path);
            let mut v = client.tree(repo, revision, p.as_ref(), &token)?;
            v.sort_by(|a, b| a.path.bytes().cmp(&b.path.bytes()));
            let (v, more, next) = paginate(v, offset, limit);
            truncated = more;
            ("tree", serde_json::to_value(v).expect("serializable"), next)
        }
        SnapshotTarget::Blame(BlameArgs { revision, path }) => {
            let p = git_path(path[0].clone());
            let v = client.blame(repo, revision, &p, &token)?;
            let (v, more, next) = paginate(v, offset, limit);
            truncated = more;
            (
                "blame",
                serde_json::to_value(v).expect("serializable"),
                next,
            )
        }
        SnapshotTarget::Stash => {
            let v = client.stashes(repo, &token)?;
            let (v, more, next) = paginate(v, offset, limit);
            truncated = more;
            (
                "stash",
                serde_json::to_value(v).expect("serializable"),
                next,
            )
        }
    };
    Ok(Envelope::new(
        "snapshot",
        SnapshotPayload {
            repository: RepositoryLocator::from_repository(repo),
            target: name,
            request,
            offset,
            data,
            truncated,
            continuation,
        },
    ))
}

fn reject_offset(offset: usize, target: &'static str) -> Result<(), SnapshotError> {
    if offset == 0 {
        Ok(())
    } else {
        Err(SnapshotError::InvalidOffset { target })
    }
}

fn paginate<T>(values: Vec<T>, offset: usize, limit: usize) -> (Vec<T>, bool, Option<usize>) {
    let total = values.len();
    let end = offset.saturating_add(limit).min(total);
    let page = values
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let more = end < total;
    (page, more, more.then_some(end))
}

fn snapshot_request(target: &SnapshotTarget) -> Value {
    match target {
        SnapshotTarget::Log(args) | SnapshotTarget::Show(args) => {
            json!({"revision": args.revision, "paths": git_paths(&args.paths)})
        }
        SnapshotTarget::Compare(args) => {
            json!({"base": args.base, "head": args.head, "mode": "merge-base", "paths": git_paths(&args.paths)})
        }
        SnapshotTarget::Diff(args) => {
            json!({"left": args.left, "right": args.right, "mode": "exact", "paths": git_paths(&args.paths)})
        }
        SnapshotTarget::Tree(args) => {
            json!({"revision": args.revision, "path": args.path.first().cloned().map(git_path)})
        }
        SnapshotTarget::Blame(args) => {
            json!({"revision": args.revision, "path": git_path(args.path[0].clone())})
        }
        SnapshotTarget::Refs | SnapshotTarget::Status | SnapshotTarget::Stash => json!({}),
    }
}

pub fn version_json() -> Envelope<Value> {
    Envelope::new(
        "version",
        json!({"version":env!("CARGO_PKG_VERSION"),"gitMinimum":"2.45.1"}),
    )
}

pub fn git_paths(values: &[OsString]) -> Vec<GitPath> {
    values.iter().cloned().map(git_path).collect()
}
#[cfg(unix)]
fn git_path(v: OsString) -> GitPath {
    use std::os::unix::ffi::OsStringExt;
    GitPath::new(v.into_vec())
}
#[cfg(not(unix))]
fn git_path(v: OsString) -> GitPath {
    GitPath::new(v.to_string_lossy().as_bytes().to_vec())
}
#[cfg(unix)]
fn encoded_native(path: &Path) -> EncodedPath {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    EncodedPath {
        display: crate::sanitize::sanitize_bytes(bytes),
        bytes_base64: STANDARD.encode(bytes),
    }
}
#[cfg(not(unix))]
fn encoded_native(path: &Path) -> EncodedPath {
    let bytes = path.to_string_lossy();
    EncodedPath {
        display: crate::sanitize::sanitize_str(&bytes),
        bytes_base64: STANDARD.encode(bytes.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_is_compact_and_versioned() {
        let s = serde_json::to_string(&version_json()).unwrap();
        assert_eq!(s, include_str!("../tests/fixtures/version.json").trim());
        assert!(!s.contains("\\u001b"));
    }
}
