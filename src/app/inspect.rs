//! Logical state for read-only repository inspection views.

use crate::domain::{
    BlameLine, Blob, Comparison, ComparisonMode, Diff, GitPath, RefInfo, StashEntry, Status,
    TreeEntry,
};

#[derive(Debug, Clone, Default)]
pub struct InspectState {
    pub refs: Vec<RefInfo>,
    pub status: Option<Status>,
    pub tree: Vec<TreeEntry>,
    pub tree_path: Option<GitPath>,
    pub blob: Option<Blob>,
    pub blame: Vec<BlameLine>,
    pub blame_path: Option<GitPath>,
    pub stashes: Vec<StashEntry>,
    pub comparison: Option<Comparison>,
    pub working_diff: Option<Diff>,
    pub selected: usize,
    pub scroll: usize,
    pub loading: bool,
    pub compare_base: String,
    pub compare_head: String,
    pub compare_base_label: Option<String>,
    pub compare_head_label: Option<String>,
    pub compare_mode: ComparisonMode,
    pub compare_picker: bool,
    pub status_diff_staged: bool,
    pub working_diff_pending: Option<bool>,
}

impl InspectState {
    pub fn new() -> Self {
        Self {
            compare_base: "main".into(),
            compare_head: "HEAD".into(),
            compare_mode: ComparisonMode::MergeBase,
            ..Self::default()
        }
    }

    pub fn reset_selection(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn status_entries(&self) -> &[crate::domain::StatusEntry] {
        self.status
            .as_ref()
            .map_or(&[], |status| status.entries.as_slice())
    }

    pub fn status_staged(&self) -> bool {
        self.status_diff_staged
    }

    pub fn active_diff(&self) -> Option<&Diff> {
        self.comparison
            .as_ref()
            .map(|comparison| &comparison.diff)
            .or(self.working_diff.as_ref())
    }
}

impl Default for ComparisonMode {
    fn default() -> Self {
        Self::MergeBase
    }
}
