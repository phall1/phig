//! Top-level reducer dispatch and asynchronous result application.

use crate::domain::{
    BlameLine, Blob, CommitDetail, Comparison, ComparisonMode, Diff, HistoryPage, RefInfo,
    StashEntry, Status, StatusCode, TreeEntry,
};

use super::{
    Action, App, Effect, Focus, Overlay, PREFETCH_DISTANCE, RequestFailure, RequestKind, View,
};

impl App {
    pub fn update(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
        self.notice = None;
        self.dirty = true;
        if self.overlay != Overlay::None {
            return self.update_overlay(action, page_rows);
        }

        match action {
            Action::Move(delta) => self.move_active(delta),
            Action::Page(delta) => self.move_active(delta.saturating_mul(page_rows.max(1) as i32)),
            Action::First => self.first_active(),
            Action::Last => self.last_active(),
            Action::Open => self.open_active(),
            Action::Back | Action::Quit => {
                self.inspect.compare_picker = false;
                if let Some(previous) = self.view_stack.pop() {
                    self.view = previous;
                    self.focus = Focus::List;
                    self.dirty = true;
                } else if self.view != View::Log {
                    self.view = View::Log;
                    self.focus = Focus::List;
                } else {
                    self.should_quit = true;
                }
                self.inspect.loading = false;
                if self.view == View::Log && self.commits.is_empty() && !self.should_quit {
                    self.history_loading = true;
                    vec![Effect::LoadHistory {
                        offset: 0,
                        limit: self.history_page_size,
                    }]
                } else {
                    Vec::new()
                }
            }
            Action::TogglePreview => {
                self.show_preview = !self.show_preview;
                if !self.show_preview {
                    self.focus = Focus::List;
                }
                Vec::new()
            }
            Action::ToggleFocus => {
                if self.view == View::Log && self.show_preview && self.preview_focus_available {
                    self.focus = match self.focus {
                        Focus::List => Focus::Preview,
                        Focus::Preview => Focus::List,
                    };
                }
                Vec::new()
            }
            Action::StartSearch => {
                self.overlay = Overlay::Search {
                    draft: self.search_query.clone(),
                    previous_query: self.search_query.clone(),
                    original_selected: self.selected,
                    original_inspect_selected: self.inspect.selected,
                    original_scroll: self.diff_scroll,
                };
                Vec::new()
            }
            Action::StartPalette => {
                self.overlay = Overlay::Palette {
                    draft: String::new(),
                    selected: 0,
                };
                Vec::new()
            }
            Action::StartFilePicker => {
                if self
                    .active_diff()
                    .is_some_and(|diff| !diff.files.is_empty())
                {
                    self.overlay = Overlay::FilePicker {
                        draft: String::new(),
                        selected: 0,
                        original_scroll: self.diff_scroll,
                    };
                } else {
                    self.notice = Some("No changed files in this diff".into());
                }
                Vec::new()
            }
            Action::ToggleHelp => {
                self.show_help();
                Vec::new()
            }
            Action::RetryFailed => self.retry_failed(),
            Action::DismissErrors => {
                self.history_error = None;
                self.preview_error = None;
                self.inspect_error = None;
                Vec::new()
            }
            Action::NextMatch => self.seek_match(true, false),
            Action::PreviousMatch => self.seek_match(false, false),
            Action::NextHunk(direction) => {
                self.seek_diff_anchor(direction, true);
                Vec::new()
            }
            Action::NextFile(direction) => {
                self.seek_diff_anchor(direction, false);
                Vec::new()
            }
            Action::NextParent => self.next_parent(),
            Action::ViewLog => self.switch_view(View::Log),
            Action::ViewRefs => self.switch_view(View::Refs),
            Action::ViewStatus => self.switch_view(View::Status),
            Action::ViewTree => self.switch_view(View::Tree),
            Action::ViewBlame => self.switch_view(View::Blame),
            Action::ViewStash => self.switch_view(View::Stash),
            Action::Mark => {
                self.marked_oid = self.selected_commit().map(|commit| commit.id.clone());
                Vec::new()
            }
            Action::StartCompare => self.start_compare(),
            Action::SwapCompare => {
                std::mem::swap(
                    &mut self.inspect.compare_base,
                    &mut self.inspect.compare_head,
                );
                std::mem::swap(
                    &mut self.inspect.compare_base_label,
                    &mut self.inspect.compare_head_label,
                );
                self.inspect.comparison = None;
                self.inspect.loading = true;
                vec![Effect::LoadCompare]
            }
            Action::ToggleCompareMode => {
                self.inspect.compare_mode = match self.inspect.compare_mode {
                    ComparisonMode::Exact => ComparisonMode::MergeBase,
                    ComparisonMode::MergeBase => ComparisonMode::Exact,
                };
                self.inspect.comparison = None;
                self.inspect.loading = true;
                vec![Effect::LoadCompare]
            }
            Action::ToggleStatusDiff => {
                if self.view != View::Status {
                    return Vec::new();
                }
                let Some(entry) = self
                    .inspect
                    .status_entries()
                    .get(self.inspect.selected)
                    .cloned()
                else {
                    return Vec::new();
                };
                let staged_available =
                    entry.index != StatusCode::Unmodified && entry.index != StatusCode::Untracked;
                let unstaged_available = entry.worktree != StatusCode::Unmodified
                    && entry.worktree != StatusCode::Untracked;
                if !(staged_available && unstaged_available) {
                    return Vec::new();
                }
                self.inspect.status_diff_staged = !self.inspect.status_diff_staged;
                self.inspect.working_diff_pending = Some(self.inspect.status_diff_staged);
                self.inspect.working_diff = None;
                self.inspect.loading = true;
                self.inspect_error = None;
                vec![Effect::LoadWorkingDiff {
                    path: entry.path.clone(),
                    staged: self.inspect.status_diff_staged,
                }]
            }
            Action::Ascend => self.ascend_tree(),
            Action::CopySelection => {
                self.copy_requested = true;
                Vec::new()
            }
            Action::Redraw => {
                self.redraw_requested = true;
                Vec::new()
            }
            Action::SearchInput(_)
            | Action::SearchBackspace
            | Action::AcceptSearch
            | Action::CancelOverlay
            | Action::PaletteMove(_)
            | Action::ExecutePalette
            | Action::FilePickerMove(_)
            | Action::AcceptFilePicker => Vec::new(),
        }
    }

    pub fn show_help(&mut self) {
        self.overlay = if self.overlay == Overlay::Help {
            Overlay::None
        } else {
            Overlay::Help
        };
        self.dirty = true;
    }

    pub fn apply_history(&mut self, page: HistoryPage) -> Vec<Effect> {
        let previous = self.selected_oid.clone();
        if page.offset == 0 {
            self.commits.clear();
        }
        for commit in page.commits {
            if !self.commits.iter().any(|existing| existing.id == commit.id) {
                self.commits.push(commit);
            }
        }
        self.has_more = page.has_more;
        self.history_loading = false;
        self.history_error = None;
        if let Some(previous) = previous {
            if let Some(index) = self.commits.iter().position(|commit| commit.id == previous) {
                self.selected = index;
            }
        }
        self.selected = self.selected.min(self.commits.len().saturating_sub(1));
        self.selected_oid = self.selected_commit().map(|commit| commit.id.clone());
        self.dirty = true;
        let preview_matches_selection = self
            .preview
            .as_ref()
            .zip(self.selected_oid.as_ref())
            .is_some_and(|(detail, selected)| &detail.commit.id == selected);
        let mut effects = if !preview_matches_selection
            && !self.preview_loading
            && self.preview_error.is_none()
        {
            self.request_preview()
        } else {
            Vec::new()
        };
        if let Some(forward) = self.search_pending.take() {
            effects.extend(self.seek_match(forward, false));
        }
        effects
    }

    pub fn apply_preview(&mut self, detail: CommitDetail) {
        if self.show_mode && self.selected_commit().is_none() {
            self.selected_oid = Some(detail.commit.id.clone());
            self.commits.push(detail.commit.clone());
            self.selected = 0;
        }
        let current = self.selected_commit().map(|commit| &commit.id);
        if current == Some(&detail.commit.id) || !matches!(self.view, View::Log) {
            self.parent_index = detail
                .selected_parent
                .as_ref()
                .and_then(|parent| detail.commit.parents.iter().position(|item| item == parent))
                .unwrap_or(0);
            self.preview = Some(detail);
            self.preview_loading = false;
            self.preview_error = None;
            self.diff_scroll = self.diff_scroll.min(self.diff_len().saturating_sub(1));
            self.dirty = true;
        }
    }

    pub fn apply_refs(&mut self, refs: Vec<RefInfo>) -> Vec<Effect> {
        self.inspect.refs = refs;
        self.inspect.loading = false;
        self.inspect_error = None;
        if self.inspect.refs.is_empty() {
            self.preview = None;
            self.preview_loading = false;
        }
        self.inspect.selected = self
            .inspect
            .selected
            .min(self.inspect.refs.len().saturating_sub(1));
        self.dirty = true;
        self.inspect_selection_effects()
    }

    pub fn apply_status(&mut self, mut status: Status) -> Vec<Effect> {
        status.entries.sort_by_key(|entry| {
            if entry.conflict.is_some() {
                0
            } else if entry.index == StatusCode::Untracked {
                4
            } else if entry.index != StatusCode::Unmodified
                && entry.worktree != StatusCode::Unmodified
            {
                2
            } else if entry.index != StatusCode::Unmodified {
                1
            } else {
                3
            }
        });
        self.inspect.status = Some(status);
        self.inspect.loading = false;
        self.inspect_error = None;
        self.inspect.selected = self
            .inspect
            .selected
            .min(self.inspect.status_entries().len().saturating_sub(1));
        self.dirty = true;
        self.inspect_selection_effects()
    }

    pub fn apply_tree(&mut self, tree: Vec<TreeEntry>) {
        self.inspect.tree = tree;
        self.inspect.loading = false;
        self.inspect_error = None;
        self.inspect.selected = self
            .inspect
            .selected
            .min(self.inspect.tree.len().saturating_sub(1));
        self.dirty = true;
    }

    pub fn apply_blob(&mut self, blob: Blob) {
        self.inspect.blob = Some(blob);
        self.inspect.loading = false;
        self.inspect_error = None;
        self.diff_scroll = 0;
        self.dirty = true;
    }

    pub fn apply_blame(&mut self, blame: Vec<BlameLine>) -> Vec<Effect> {
        self.inspect.blame = blame;
        self.inspect.loading = false;
        self.inspect_error = None;
        if self.inspect.blame.is_empty() {
            self.preview = None;
            self.preview_loading = false;
        }
        self.inspect.selected = self
            .inspect
            .selected
            .min(self.inspect.blame.len().saturating_sub(1));
        self.dirty = true;
        self.inspect_selection_effects()
    }

    pub fn apply_stashes(&mut self, stashes: Vec<StashEntry>) -> Vec<Effect> {
        self.inspect.stashes = stashes;
        self.inspect.loading = false;
        self.inspect_error = None;
        if self.inspect.stashes.is_empty() {
            self.preview = None;
            self.preview_loading = false;
        }
        self.inspect.selected = self
            .inspect
            .selected
            .min(self.inspect.stashes.len().saturating_sub(1));
        self.dirty = true;
        self.inspect_selection_effects()
    }

    pub fn apply_comparison(&mut self, comparison: Comparison) {
        self.inspect.comparison = Some(comparison);
        self.inspect.loading = false;
        self.inspect_error = None;
        self.diff_scroll = 0;
        self.dirty = true;
    }

    pub fn apply_working_diff(&mut self, diff: Diff) {
        self.inspect.working_diff_pending = None;
        self.inspect.working_diff = Some(diff);
        self.inspect.loading = false;
        self.inspect_error = None;
        self.diff_scroll = 0;
        self.dirty = true;
    }

    /// Apply the TUI adapter's semantic decision about preview focus availability.
    pub fn set_preview_focus_available(&mut self, available: bool) {
        if self.preview_focus_available == available {
            return;
        }
        self.preview_focus_available = available;
        if self.view == View::Log && self.focus == Focus::Preview && !available {
            self.focus = Focus::List;
        }
        self.dirty = true;
    }

    pub fn apply_error(&mut self, request: RequestKind, error: &impl std::fmt::Display) {
        let failure = RequestFailure {
            operation: match request {
                RequestKind::History => "load history",
                RequestKind::Preview => "load commit detail",
                RequestKind::Inspect => match self.inspect.working_diff_pending {
                    Some(true) => "load staged working diff",
                    Some(false) => "load unstaged working diff",
                    None => "load repository view",
                },
            },
            detail: error.to_string(),
        };
        match request {
            RequestKind::History => {
                self.history_loading = false;
                self.history_error = Some(failure);
            }
            RequestKind::Preview => {
                self.preview_loading = false;
                self.preview_error = Some(failure);
            }
            RequestKind::Inspect => {
                self.inspect.loading = false;
                self.inspect.working_diff_pending = None;
                if matches!(self.view, View::Status | View::StatusDiff) {
                    self.inspect.working_diff = None;
                }
                self.inspect_error = Some(failure);
            }
        }
        self.dirty = true;
    }

    pub fn has_errors(&self) -> bool {
        self.history_error.is_some() || self.preview_error.is_some() || self.inspect_error.is_some()
    }

    fn retry_failed(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.history_error.take().is_some() {
            self.history_loading = true;
            effects.push(Effect::LoadHistory {
                offset: if self.commits.is_empty() {
                    0
                } else {
                    self.commits.len()
                },
                limit: self.history_page_size,
            });
        }
        if self.preview_error.take().is_some() {
            effects.extend(self.request_preview());
        }
        if self
            .inspect_error
            .as_ref()
            .is_some_and(|failure| failure.operation != "open blame")
        {
            self.inspect_error = None;
            self.inspect.loading = true;
            effects.extend(self.initial_effects());
        }
        effects
    }

    pub fn request_more_if_needed(&mut self) -> Vec<Effect> {
        if self.has_more
            && !self.history_loading
            && self.selected.saturating_add(PREFETCH_DISTANCE) >= self.commits.len()
        {
            self.history_loading = true;
            self.history_error = None;
            vec![Effect::LoadHistory {
                offset: self.commits.len(),
                limit: self.history_page_size,
            }]
        } else {
            Vec::new()
        }
    }

    fn switch_view(&mut self, view: View) -> Vec<Effect> {
        let previous_path = self.active_path();
        if view == View::Blame && previous_path.is_none() {
            self.inspect_error = Some(RequestFailure {
                operation: "open blame",
                detail:
                    "select a file path first (status, tree, blob, or a changed file in a diff)"
                        .into(),
            });
            self.inspect.loading = false;
            return Vec::new();
        }
        if self.view != view {
            self.view_stack.push(self.view);
        }
        self.view = view;
        self.focus = Focus::List;
        self.inspect.reset_selection();
        self.inspect.working_diff_pending = None;
        self.inspect.loading = true;
        self.inspect_error = None;
        if !matches!(view, View::Log | View::Detail) {
            self.history_loading = false;
        }
        match view {
            View::Log => {
                self.inspect.loading = false;
                self.history_loading = self.commits.is_empty();
                if self.commits.is_empty() {
                    vec![Effect::LoadHistory {
                        offset: 0,
                        limit: self.history_page_size,
                    }]
                } else {
                    Vec::new()
                }
            }
            View::Refs => {
                self.preview = None;
                self.preview_loading = false;
                vec![Effect::LoadRefs]
            }
            View::Status => {
                self.inspect.working_diff = None;
                vec![Effect::LoadStatus]
            }
            View::StatusDiff => Vec::new(),
            View::Tree => vec![Effect::LoadTree {
                revision: self.revision.clone(),
                path: self.inspect.tree_path.clone(),
            }],
            View::Blame => {
                self.preview = None;
                self.preview_loading = false;
                self.inspect.blame_path = previous_path
                    .clone()
                    .or_else(|| self.paths.first().cloned());
                self.inspect
                    .blame_path
                    .clone()
                    .map(|path| Effect::LoadBlame {
                        revision: self.revision.clone(),
                        path,
                    })
                    .into_iter()
                    .collect()
            }
            View::Stash => {
                self.preview = None;
                self.preview_loading = false;
                vec![Effect::LoadStashes]
            }
            View::Compare => vec![Effect::LoadCompare],
            View::Detail | View::Blob => Vec::new(),
        }
    }
}
