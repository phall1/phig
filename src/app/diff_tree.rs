//! A temporary changed-file navigator with a cached, collapsible directory index.

use super::{Action, App, Overlay};
use crate::domain::{Diff, DiffLineKind, GitPath};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffTreeEntry {
    pub label: String,
    pub depth: usize,
    pub directory: bool,
    pub header_line: usize,
    pub added: usize,
    pub removed: usize,
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
                        depth,
                        directory,
                        header_line: file.header_line,
                        added: 0,
                        removed: 0,
                    });
                node.added += added;
                node.removed += removed;
            }
        }
        let entries: Vec<_> = nodes.into_values().collect();
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
        let depth = entry.depth;
        let index = self.visible[self.selected];
        if entry.directory && !self.collapsed.contains(&index) {
            self.collapsed.insert(index);
            self.rebuild();
            return;
        }
        if let Some(parent) = self.visible[..self.selected]
            .iter()
            .rposition(|i| self.entries[*i].depth < depth)
        {
            self.selected = parent;
        }
    }
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
