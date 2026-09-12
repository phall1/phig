//! A temporary changed-file navigator with a cached, collapsible directory index.

use super::{Action, App, Overlay};
use crate::domain::{Diff, DiffFile, DiffLineKind, GitPath};
use std::collections::{BTreeMap, HashSet};

/// Change kind of a file entry, derived from its diff header paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

impl TreeStatus {
    fn of(file: &DiffFile) -> Option<Self> {
        match (file.old_path.as_ref(), file.new_path.as_ref()) {
            (None, Some(_)) => Some(Self::Added),
            (Some(_), None) => Some(Self::Deleted),
            (Some(old), Some(new)) if old != new => Some(Self::Renamed),
            _ => Some(Self::Modified),
        }
    }

    pub(crate) fn letter(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Modified => 'M',
            Self::Renamed => 'R',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffTreeEntry {
    pub label: String,
    /// Full path up to and including this node (directories end in `/`).
    pub full: String,
    pub depth: usize,
    pub directory: bool,
    pub header_line: usize,
    pub added: usize,
    pub removed: usize,
    pub status: Option<TreeStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffTree {
    pub entries: Vec<DiffTreeEntry>,
    pub visible: Vec<usize>,
    pub selected: usize,
    pub original_scroll: usize,
    collapsed: HashSet<usize>,
}

impl DiffTree {
    pub fn new(diff: &Diff, scroll: usize) -> Self {
        let mut nodes = BTreeMap::new();
        for (i, file) in diff.files.iter().enumerate() {
            let Some(path) = file.new_path.as_ref().or(file.old_path.as_ref()) else {
                continue;
            };
            let status = TreeStatus::of(file);
            let bytes = path.bytes();
            let end = diff
                .files
                .get(i + 1)
                .map_or(diff.lines.len(), |next| next.header_line);
            let lines =
                &diff.lines[file.header_line.min(diff.lines.len())..end.min(diff.lines.len())];
            let added = lines
                .iter()
                .filter(|line| line.kind == DiffLineKind::Added)
                .count();
            let removed = lines
                .iter()
                .filter(|line| line.kind == DiffLineKind::Removed)
                .count();
            let mut prefix = Vec::new();
            let components: Vec<_> = bytes.split(|byte| *byte == b'/').collect();
            for (depth, component) in components.iter().enumerate() {
                prefix.extend_from_slice(component);
                let directory = depth + 1 < components.len();
                if directory {
                    prefix.push(b'/');
                }
                let node = nodes
                    .entry(prefix.clone())
                    .or_insert_with(|| DiffTreeEntry {
                        label: GitPath::new(component.to_vec()).display,
                        full: GitPath::new(prefix.clone()).display,
                        depth,
                        directory,
                        header_line: file.header_line,
                        added: 0,
                        removed: 0,
                        status: if directory { None } else { status },
                    });
                node.added += added;
                node.removed += removed;
            }
        }
        let mut entries: Vec<_> = nodes.into_values().collect();
        // Flatten depth-first so every directory's subtree stays contiguous:
        // directories first, then files, case-insensitively at each level.
        let ordered = depth_first_order(&entries);
        entries = ordered
            .into_iter()
            .map(|index| entries[index].clone())
            .collect();
        let active = diff
            .files
            .partition_point(|file| file.header_line <= scroll)
            .saturating_sub(1);
        let header = diff.files.get(active).map_or(0, |file| file.header_line);
        let selected = entries
            .iter()
            .position(|entry| !entry.directory && entry.header_line == header)
            .unwrap_or(0);
        Self {
            visible: (0..entries.len()).collect(),
            entries,
            selected,
            original_scroll: scroll,
            collapsed: HashSet::new(),
        }
    }

    pub fn current(&self) -> Option<&DiffTreeEntry> {
        self.visible
            .get(self.selected)
            .and_then(|i| self.entries.get(*i))
    }

    pub fn is_collapsed(&self, index: usize) -> bool {
        self.collapsed.contains(&index)
    }

    fn move_by(&mut self, delta: i32) {
        self.selected = (self.selected as i64 + i64::from(delta))
            .clamp(0, self.visible.len().saturating_sub(1) as i64) as usize;
    }

    fn rebuild(&mut self) {
        let mut hidden_depth = None;
        self.visible.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            if hidden_depth.is_some_and(|depth| entry.depth > depth) {
                continue;
            }
            hidden_depth = None;
            self.visible.push(index);
            if self.collapsed.contains(&index) {
                hidden_depth = Some(entry.depth);
            }
        }
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
    }

    fn expand(&mut self, toggle: bool) {
        let Some(entry) = self.current() else {
            return;
        };
        if !entry.directory {
            return;
        }
        let index = self.visible[self.selected];
        if !self.collapsed.remove(&index) && toggle {
            self.collapsed.insert(index);
        }
        self.rebuild();
    }

    fn collapse(&mut self) {
        let Some(entry) = self.current() else {
            return;
        };
        let index = self.visible[self.selected];
        if entry.directory && !self.collapsed.contains(&index) {
            self.collapsed.insert(index);
            self.rebuild();
            return;
        }
        let Some(parent) = parent_prefix(&entry.full).and_then(|prefix| {
            self.entries
                .iter()
                .position(|entry| entry.directory && entry.full == prefix)
        }) else {
            return;
        };
        if let Some(position) = self.visible.iter().position(|index| *index == parent) {
            self.selected = position;
        }
    }

    pub fn collapse_all(&mut self) {
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.directory {
                self.collapsed.insert(index);
            }
        }
        self.rebuild();
    }

    pub fn expand_all(&mut self) {
        if self.collapsed.is_empty() {
            return;
        }
        self.collapsed.clear();
        self.rebuild();
    }
}

/// The full path of the directory that contains one entry, or `None` for a
/// root-level entry. Directory paths already end in `/`; file paths get their
/// last component stripped and the trailing slash kept (`src/main.rs` →
/// `src/`).
fn parent_prefix(full: &str) -> Option<String> {
    let trimmed = full.strip_suffix('/').unwrap_or(full);
    let index = trimmed.rfind('/')?;
    Some(full[..index + 1].to_owned())
}

/// Depth-first preorder that keeps every directory's subtree contiguous so
/// the collapse walk can hide it as one run. Within each directory, children
/// sort directories first and then files, case-insensitively. Children are
/// bucketed under their parent's path in one pass, so a large diff costs one
/// scan plus per-directory sorts rather than a per-directory full rescan.
fn depth_first_order(entries: &[DiffTreeEntry]) -> Vec<usize> {
    use std::collections::HashMap;

    fn by_kind(entries: &[DiffTreeEntry], indices: &mut [usize]) {
        indices.sort_by(|&a, &b| {
            entries[b]
                .directory
                .cmp(&entries[a].directory)
                .then_with(|| {
                    entries[a]
                        .label
                        .to_lowercase()
                        .cmp(&entries[b].label.to_lowercase())
                })
        });
    }

    fn flatten(
        entries: &[DiffTreeEntry],
        children: &HashMap<String, Vec<usize>>,
        indices: &[usize],
        out: &mut Vec<usize>,
    ) {
        for &index in indices {
            out.push(index);
            if entries[index].directory {
                if let Some(bucket) = children.get(&entries[index].full) {
                    flatten(entries, children, bucket, out);
                }
            }
        }
    }

    let mut children: HashMap<String, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        match parent_prefix(&entry.full) {
            Some(parent) => children.entry(parent).or_default().push(index),
            None => roots.push(index),
        }
    }
    by_kind(entries, &mut roots);
    for bucket in children.values_mut() {
        by_kind(entries, bucket);
    }
    let mut ordered = Vec::with_capacity(entries.len());
    flatten(entries, &children, &roots, &mut ordered);
    ordered
}

impl App {
    pub(super) fn start_diff_tree(&mut self) {
        let Some(diff) = self.active_diff().filter(|diff| !diff.files.is_empty()) else {
            self.notice = Some("No changed files in this diff".into());
            return;
        };
        self.overlay = Overlay::DiffTree(DiffTree::new(diff, self.diff_scroll));
    }

    pub(super) fn update_diff_tree(&mut self, action: Action, page_rows: usize) {
        let Overlay::DiffTree(tree) = &mut self.overlay else {
            return;
        };
        match action {
            Action::Back | Action::Quit | Action::CancelOverlay | Action::ToggleDiffTree => {
                self.diff_scroll = tree.original_scroll;
                self.overlay = Overlay::None;
                return;
            }
            Action::Move(delta) => tree.move_by(delta),
            Action::Page(delta) => tree.move_by(delta.saturating_mul(page_rows.max(1) as i32)),
            Action::First => tree.selected = 0,
            Action::Last => tree.selected = tree.visible.len().saturating_sub(1),
            Action::TreeCollapse => tree.collapse(),
            Action::TreeExpand => tree.expand(false),
            Action::TreeCollapseAll => tree.collapse_all(),
            Action::TreeExpandAll => tree.expand_all(),
            Action::Open => {
                if tree.current().is_some_and(|entry| entry.directory) {
                    tree.expand(true);
                } else {
                    if let Some(entry) = tree.current() {
                        self.diff_scroll = entry.header_line;
                    }
                    self.overlay = Overlay::None;
                    if !self.diff_fullscreen {
                        self.toggle_diff_fullscreen();
                    }
                    return;
                }
            }
            _ => return,
        }
        if let Some(entry) = tree.current() {
            self.diff_scroll = entry.header_line;
        }
    }
}
