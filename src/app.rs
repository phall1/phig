use crate::{
    domain::{
        BlameLine, Blob, Commit, CommitDetail, Comparison, ComparisonMode, Diff, GitPath,
        HistoryPage, Oid, RefInfo, Repository, StashEntry, Status, StatusCode, TreeEntry,
        TreeEntryKind,
    },
    git::GitError,
    inspect::InspectState,
};

const PAGE_SIZE: usize = 256;
const PREFETCH_DISTANCE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Log,
    Detail,
    Compare,
    Refs,
    Status,
    StatusDiff,
    Tree,
    Blob,
    Blame,
    Stash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    Search {
        draft: String,
        previous_query: String,
        original_selected: usize,
        original_inspect_selected: usize,
        original_scroll: usize,
    },
    Palette {
        draft: String,
        selected: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Move(i32),
    Page(i32),
    First,
    Last,
    Open,
    Back,
    Quit,
    TogglePreview,
    ToggleFocus,
    StartSearch,
    StartPalette,
    ToggleHelp,
    SearchInput(char),
    SearchBackspace,
    AcceptSearch,
    CancelOverlay,
    PaletteMove(i32),
    ExecutePalette,
    RetryFailed,
    DismissErrors,
    NextMatch,
    PreviousMatch,
    NextHunk(i32),
    NextFile(i32),
    NextParent,
    ViewLog,
    ViewRefs,
    ViewStatus,
    ViewTree,
    ViewBlame,
    ViewStash,
    Mark,
    StartCompare,
    SwapCompare,
    ToggleCompareMode,
    ToggleStatusDiff,
    Ascend,
    Redraw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    LoadHistory {
        offset: usize,
        limit: usize,
    },
    LoadPreview {
        revision: String,
        parent_index: usize,
    },
    LoadRefs,
    LoadStatus,
    LoadTree {
        revision: String,
        path: Option<GitPath>,
    },
    LoadBlob {
        id: Oid,
        path: Option<GitPath>,
    },
    LoadBlame {
        revision: String,
        path: GitPath,
    },
    LoadStashes,
    LoadCompare,
    LoadWorkingDiff {
        path: GitPath,
        staged: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    History,
    Preview,
    Inspect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFailure {
    pub operation: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteCommand {
    pub name: &'static str,
    pub action: Action,
}

#[derive(Debug)]
pub struct App {
    pub repository: Repository,
    pub revision: String,
    pub revision_label: Option<String>,
    pub paths: Vec<GitPath>,
    pub show_mode: bool,
    pub view: View,
    pub focus: Focus,
    pub overlay: Overlay,
    pub commits: Vec<Commit>,
    pub selected: usize,
    pub selected_oid: Option<Oid>,
    pub preview: Option<CommitDetail>,
    pub history_loading: bool,
    pub preview_loading: bool,
    pub has_more: bool,
    pub show_preview: bool,
    pub diff_scroll: usize,
    pub parent_index: usize,
    pub search_query: String,
    /// Direction of a search waiting for another history page (`true` is forward).
    pub search_pending: Option<bool>,
    pub history_error: Option<RequestFailure>,
    pub preview_error: Option<RequestFailure>,
    pub inspect_error: Option<RequestFailure>,
    pub inspect: InspectState,
    pub view_stack: Vec<View>,
    pub marked_oid: Option<Oid>,
    pub should_quit: bool,
    pub dirty: bool,
}

impl App {
    pub fn new(
        repository: Repository,
        revision: String,
        paths: Vec<GitPath>,
        start_in_detail: bool,
    ) -> Self {
        Self {
            repository,
            revision,
            revision_label: None,
            paths,
            show_mode: start_in_detail,
            view: if start_in_detail {
                View::Detail
            } else {
                View::Log
            },
            focus: Focus::List,
            overlay: Overlay::None,
            commits: Vec::new(),
            selected: 0,
            selected_oid: None,
            preview: None,
            history_loading: true,
            preview_loading: start_in_detail,
            has_more: true,
            show_preview: true,
            diff_scroll: 0,
            parent_index: 0,
            search_query: String::new(),
            search_pending: None,
            history_error: None,
            preview_error: None,
            inspect_error: None,
            inspect: InspectState::new(),
            view_stack: Vec::new(),
            marked_oid: None,
            should_quit: false,
            dirty: true,
        }
    }

    pub fn set_start_view(
        &mut self,
        view: View,
        compare_base: Option<String>,
        compare_head: String,
        compare_mode: ComparisonMode,
    ) {
        self.view = view;
        self.show_mode = view == View::Detail;
        self.inspect.compare_base = compare_base.unwrap_or_else(|| "main".into());
        self.inspect.compare_head = compare_head;
        self.inspect.compare_mode = compare_mode;
        self.inspect.loading = !matches!(view, View::Log | View::Detail);
        self.history_loading = matches!(view, View::Log | View::Detail);
    }

    pub fn initial_effects(&self) -> Vec<Effect> {
        match self.view {
            View::Log => vec![Effect::LoadHistory {
                offset: 0,
                limit: PAGE_SIZE,
            }],
            View::Detail => vec![
                Effect::LoadHistory {
                    offset: 0,
                    limit: PAGE_SIZE,
                },
                Effect::LoadPreview {
                    revision: self.revision.clone(),
                    parent_index: 0,
                },
            ],
            View::Compare => vec![Effect::LoadCompare],
            View::Refs => vec![Effect::LoadRefs],
            View::Status => vec![Effect::LoadStatus],
            View::StatusDiff => Vec::new(),
            View::Tree => vec![Effect::LoadTree {
                revision: self.revision.clone(),
                path: self.inspect.tree_path.clone(),
            }],
            View::Blame => self
                .inspect
                .blame_path
                .clone()
                .or_else(|| self.paths.first().cloned())
                .map(|path| Effect::LoadBlame {
                    revision: self.revision.clone(),
                    path,
                })
                .into_iter()
                .collect(),
            View::Stash => vec![Effect::LoadStashes],
            View::Blob => Vec::new(),
        }
    }

    pub fn selected_commit(&self) -> Option<&Commit> {
        self.commits.get(self.selected)
    }

    pub fn preview_paths(&self) -> Vec<GitPath> {
        if self.view == View::Blame
            || (self.view == View::Detail && self.view_stack.last() == Some(&View::Blame))
        {
            self.inspect.blame_path.clone().into_iter().collect()
        } else {
            self.paths.clone()
        }
    }

    pub fn update(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
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
                        limit: PAGE_SIZE,
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
                if self.view == View::Log && self.show_preview {
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
            Action::Redraw => Vec::new(),
            Action::SearchInput(_)
            | Action::SearchBackspace
            | Action::AcceptSearch
            | Action::CancelOverlay
            | Action::PaletteMove(_)
            | Action::ExecutePalette => Vec::new(),
        }
    }

    fn update_overlay(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
        let mut seek = false;
        let mut restore_preview = false;
        let mut palette_action = None;
        match (&mut self.overlay, action) {
            (Overlay::Help, Action::CancelOverlay | Action::Back | Action::Quit) => {
                self.overlay = Overlay::None;
            }
            (Overlay::Help, Action::StartSearch | Action::StartPalette) => {
                self.overlay = Overlay::None;
            }
            (Overlay::Search { draft, .. }, Action::SearchInput(character)) => {
                draft.push(character);
                self.search_query = draft.clone();
                seek = true;
            }
            (Overlay::Search { draft, .. }, Action::SearchBackspace) => {
                draft.pop();
                self.search_query = draft.clone();
                seek = true;
            }
            (
                Overlay::Search {
                    previous_query,
                    original_selected,
                    original_inspect_selected,
                    original_scroll,
                    ..
                },
                Action::CancelOverlay | Action::Back | Action::Quit,
            ) => {
                self.search_query = previous_query.clone();
                self.selected = (*original_selected).min(self.commits.len().saturating_sub(1));
                self.inspect.selected = *original_inspect_selected;
                self.selected_oid = self
                    .commits
                    .get(self.selected)
                    .map(|commit| commit.id.clone());
                self.diff_scroll = *original_scroll;
                self.search_pending = None;
                self.overlay = Overlay::None;
                restore_preview = true;
            }
            (Overlay::Search { draft, .. }, Action::AcceptSearch) => {
                self.search_query = draft.clone();
                self.search_pending = None;
                self.overlay = Overlay::None;
                seek = true;
            }
            (Overlay::Palette { draft, selected }, Action::SearchInput(character)) => {
                draft.push(character);
                *selected = 0;
            }
            (Overlay::Palette { draft, selected }, Action::SearchBackspace) => {
                draft.pop();
                *selected = 0;
            }
            (Overlay::Palette { draft, selected }, Action::PaletteMove(delta)) => {
                let count = palette_commands(draft).len();
                if count > 0 {
                    *selected =
                        ((*selected as i64 + i64::from(delta)).rem_euclid(count as i64)) as usize;
                }
            }
            (Overlay::Palette { draft, selected }, Action::ExecutePalette) => {
                palette_action = palette_commands(draft)
                    .get(*selected)
                    .map(|command| command.action.clone());
                self.overlay = Overlay::None;
            }
            (Overlay::Palette { .. }, Action::CancelOverlay | Action::Back | Action::Quit) => {
                self.overlay = Overlay::None;
            }
            (_, Action::CancelOverlay) => self.overlay = Overlay::None,
            _ => {}
        }
        if let Some(action) = palette_action {
            self.update(action, page_rows)
        } else if seek {
            self.seek_match(true, true)
        } else if restore_preview {
            if matches!(
                self.view,
                View::Refs | View::Status | View::Blame | View::Stash
            ) {
                self.inspect_selection_effects()
            } else {
                self.request_preview()
            }
        } else {
            Vec::new()
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

    pub fn resize(&mut self, width: u16, height: u16) {
        let body_height = height.saturating_sub(2);
        if self.view == View::Log
            && self.focus == Focus::Preview
            && (!self.show_preview || width < 72 || body_height < 16)
        {
            self.focus = Focus::List;
        }
        self.dirty = true;
    }

    pub fn apply_error(&mut self, request: RequestKind, error: &GitError) {
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
                limit: PAGE_SIZE,
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
                limit: PAGE_SIZE,
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
                        limit: PAGE_SIZE,
                    }]
                } else {
                    Vec::new()
                }
            }
            View::Refs => vec![Effect::LoadRefs],
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
            View::Stash => vec![Effect::LoadStashes],
            View::Compare => vec![Effect::LoadCompare],
            View::Detail | View::Blob => Vec::new(),
        }
    }

    fn active_path(&self) -> Option<GitPath> {
        match self.view {
            View::Status | View::StatusDiff => self
                .inspect
                .status_entries()
                .get(self.inspect.selected)
                .map(|entry| entry.path.clone()),
            View::Tree => self
                .inspect
                .tree
                .get(self.inspect.selected)
                .map(|entry| self.join_tree_path(&entry.path)),
            View::Blob => self
                .inspect
                .blob
                .as_ref()
                .and_then(|blob| blob.path.clone()),
            View::Log | View::Detail => self
                .preview
                .as_ref()
                .and_then(|detail| diff_path_at(&detail.diff, self.diff_scroll))
                .or_else(|| self.paths.first().cloned()),
            View::Compare => self
                .inspect
                .comparison
                .as_ref()
                .and_then(|comparison| diff_path_at(&comparison.diff, self.diff_scroll))
                .or_else(|| self.paths.first().cloned()),
            _ => self.paths.first().cloned(),
        }
    }

    fn join_tree_path(&self, child: &GitPath) -> GitPath {
        let mut bytes = self
            .inspect
            .tree_path
            .as_ref()
            .map(GitPath::bytes)
            .unwrap_or_default();
        if !bytes.is_empty() {
            bytes.push(b'/');
        }
        bytes.extend(child.bytes());
        GitPath::new(bytes)
    }

    fn active_len(&self) -> usize {
        match self.view {
            View::Log => self.commits.len(),
            View::Refs => self.inspect.refs.len(),
            View::Status => self.inspect.status_entries().len(),
            View::Tree => self.inspect.tree.len(),
            View::Blame => self.inspect.blame.len(),
            View::Stash => self.inspect.stashes.len(),
            View::Detail | View::Compare | View::StatusDiff => self.diff_len(),
            View::Blob => self
                .inspect
                .blob
                .as_ref()
                .map_or(0, |blob| blob.bytes().split(|byte| *byte == b'\n').count()),
        }
    }

    fn move_active(&mut self, delta: i32) -> Vec<Effect> {
        if self.view == View::Detail
            || self.view == View::Compare
            || self.view == View::StatusDiff
            || self.view == View::Blob
            || self.focus == Focus::Preview
        {
            self.scroll_diff(delta);
            return Vec::new();
        }
        if self.view == View::Log {
            return self.move_selection(delta, false);
        }
        let len = self.active_len();
        if len == 0 {
            return Vec::new();
        }
        self.inspect.selected = (self.inspect.selected as i64 + i64::from(delta))
            .clamp(0, len.saturating_sub(1) as i64) as usize;
        self.inspect.scroll = 0;
        self.inspect_selection_effects()
    }

    fn first_active(&mut self) -> Vec<Effect> {
        if matches!(
            self.view,
            View::Detail | View::Compare | View::StatusDiff | View::Blob
        ) || self.focus == Focus::Preview
        {
            self.diff_scroll = 0;
            Vec::new()
        } else if self.view == View::Log {
            self.select_index(0)
        } else {
            self.inspect.selected = 0;
            self.inspect_selection_effects()
        }
    }

    fn last_active(&mut self) -> Vec<Effect> {
        if matches!(
            self.view,
            View::Detail | View::Compare | View::StatusDiff | View::Blob
        ) || self.focus == Focus::Preview
        {
            self.diff_scroll = self.diff_len().saturating_sub(1);
            Vec::new()
        } else if self.view == View::Log {
            self.select_index(self.commits.len().saturating_sub(1))
        } else {
            self.inspect.selected = self.active_len().saturating_sub(1);
            self.inspect_selection_effects()
        }
    }

    fn inspect_selection_effects(&mut self) -> Vec<Effect> {
        let effects = match self.view {
            View::Refs => self
                .inspect
                .refs
                .get(self.inspect.selected)
                .map(|reference| Effect::LoadPreview {
                    revision: reference
                        .peeled
                        .as_ref()
                        .unwrap_or(&reference.target)
                        .to_string(),
                    parent_index: 0,
                })
                .into_iter()
                .collect(),
            View::Status => {
                let Some(entry) = self
                    .inspect
                    .status_entries()
                    .get(self.inspect.selected)
                    .cloned()
                else {
                    return Vec::new();
                };
                self.inspect.status_diff_staged =
                    entry.index != StatusCode::Unmodified && entry.index != StatusCode::Untracked;
                self.inspect.working_diff = None;
                self.inspect_error = None;
                let loadable =
                    entry.index != StatusCode::Untracked && entry.index != StatusCode::Ignored;
                self.inspect.loading = loadable;
                self.inspect.working_diff_pending =
                    loadable.then_some(self.inspect.status_diff_staged);
                loadable
                    .then(|| Effect::LoadWorkingDiff {
                        path: entry.path.clone(),
                        staged: self.inspect.status_diff_staged,
                    })
                    .into_iter()
                    .collect()
            }
            View::Blame => self
                .inspect
                .blame
                .get(self.inspect.selected)
                .map(|line| Effect::LoadPreview {
                    revision: line.id.to_string(),
                    parent_index: 0,
                })
                .into_iter()
                .collect(),
            View::Stash => self
                .inspect
                .stashes
                .get(self.inspect.selected)
                .map(|stash| Effect::LoadPreview {
                    revision: stash.id.to_string(),
                    parent_index: 0,
                })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        };
        if effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadPreview { .. }))
        {
            self.preview_loading = true;
            self.preview = None;
        }
        if effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadWorkingDiff { .. }))
        {
            self.inspect.loading = true;
            self.inspect.working_diff = None;
        }
        effects
    }

    fn open_active(&mut self) -> Vec<Effect> {
        match self.view {
            View::Log => {
                self.view_stack.push(View::Log);
                self.view = View::Detail;
                self.focus = Focus::Preview;
                Vec::new()
            }
            View::Refs => {
                let Some(reference) = self.inspect.refs.get(self.inspect.selected) else {
                    return Vec::new();
                };
                let revision = reference
                    .peeled
                    .as_ref()
                    .unwrap_or(&reference.target)
                    .to_string();
                let label = reference.short_name.display().to_owned();
                if self.inspect.compare_picker {
                    self.inspect.compare_base = revision;
                    self.inspect.compare_base_label = Some(label);
                    self.inspect.compare_picker = false;
                    self.view = View::Compare;
                    self.inspect.comparison = None;
                    self.inspect.loading = true;
                    vec![Effect::LoadCompare]
                } else {
                    self.revision = revision;
                    self.revision_label = Some(label);
                    self.commits.clear();
                    self.preview = None;
                    self.selected = 0;
                    self.history_loading = true;
                    self.view_stack.push(View::Refs);
                    self.view = View::Log;
                    vec![Effect::LoadHistory {
                        offset: 0,
                        limit: PAGE_SIZE,
                    }]
                }
            }
            View::Status => {
                if self.inspect.working_diff.is_none() {
                    return Vec::new();
                }
                self.view_stack.push(View::Status);
                self.view = View::StatusDiff;
                self.diff_scroll = 0;
                Vec::new()
            }
            View::Tree => {
                let Some(entry) = self.inspect.tree.get(self.inspect.selected).cloned() else {
                    return Vec::new();
                };
                let path = self.join_tree_path(&entry.path);
                match entry.kind {
                    TreeEntryKind::Tree => {
                        self.inspect.tree_path = Some(path.clone());
                        self.inspect.reset_selection();
                        self.inspect.loading = true;
                        vec![Effect::LoadTree {
                            revision: self.revision.clone(),
                            path: Some(path),
                        }]
                    }
                    TreeEntryKind::Blob => {
                        self.view_stack.push(View::Tree);
                        self.view = View::Blob;
                        self.inspect.loading = true;
                        vec![Effect::LoadBlob {
                            id: entry.id,
                            path: Some(path),
                        }]
                    }
                    _ => Vec::new(),
                }
            }
            View::Blame => {
                let Some(line) = self.inspect.blame.get(self.inspect.selected) else {
                    return Vec::new();
                };
                self.view_stack.push(View::Blame);
                self.view = View::Detail;
                self.focus = Focus::Preview;
                vec![Effect::LoadPreview {
                    revision: line.id.to_string(),
                    parent_index: 0,
                }]
            }
            View::Stash => {
                let Some(stash) = self.inspect.stashes.get(self.inspect.selected) else {
                    return Vec::new();
                };
                self.view_stack.push(View::Stash);
                self.view = View::Detail;
                self.focus = Focus::Preview;
                vec![Effect::LoadPreview {
                    revision: stash.id.to_string(),
                    parent_index: 0,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn start_compare(&mut self) -> Vec<Effect> {
        if self.view == View::Refs {
            if let Some(reference) = self.inspect.refs.get(self.inspect.selected) {
                self.inspect.compare_base = reference
                    .peeled
                    .as_ref()
                    .unwrap_or(&reference.target)
                    .to_string();
                self.inspect.compare_base_label = Some(reference.short_name.display().to_owned());
                self.inspect.compare_head = self
                    .repository
                    .head
                    .as_ref()
                    .map_or_else(|| "HEAD".into(), ToString::to_string);
                self.inspect.compare_head_label = Some(
                    self.repository
                        .branch
                        .clone()
                        .unwrap_or_else(|| "HEAD".into()),
                );
                self.inspect.compare_mode = ComparisonMode::MergeBase;
                self.view_stack.push(View::Refs);
                self.view = View::Compare;
                self.inspect.comparison = None;
                self.inspect.loading = true;
                return vec![Effect::LoadCompare];
            }
        }
        if let (Some(marked), Some(selected)) = (
            self.marked_oid.clone(),
            self.selected_commit().map(|commit| commit.id.clone()),
        ) {
            self.inspect.compare_base = marked.to_string();
            self.inspect.compare_head = selected.to_string();
            self.inspect.compare_base_label = Some(marked.short(10).to_owned());
            self.inspect.compare_head_label = Some(selected.short(10).to_owned());
            self.inspect.compare_mode = ComparisonMode::Exact;
            self.view_stack.push(self.view);
            self.view = View::Compare;
            self.inspect.comparison = None;
            self.inspect.loading = true;
            vec![Effect::LoadCompare]
        } else {
            self.inspect.compare_picker = true;
            self.view_stack.push(self.view);
            self.view = View::Refs;
            self.inspect.loading = true;
            vec![Effect::LoadRefs]
        }
    }

    fn ascend_tree(&mut self) -> Vec<Effect> {
        if self.view != View::Tree {
            return Vec::new();
        }
        let Some(path) = self.inspect.tree_path.as_ref() else {
            return self.update(Action::Back, 1);
        };
        let mut bytes = path.bytes();
        if let Some(index) = bytes.iter().rposition(|byte| *byte == b'/') {
            bytes.truncate(index);
        } else {
            bytes.clear();
        }
        self.inspect.tree_path = (!bytes.is_empty()).then(|| GitPath::new(bytes));
        self.inspect.reset_selection();
        self.inspect.loading = true;
        vec![Effect::LoadTree {
            revision: self.revision.clone(),
            path: self.inspect.tree_path.clone(),
        }]
    }

    fn move_selection(&mut self, delta: i32, wrap: bool) -> Vec<Effect> {
        if self.commits.is_empty() {
            return Vec::new();
        }
        let len = self.commits.len() as i64;
        let current = self.selected as i64;
        let next = if wrap {
            (current + i64::from(delta)).rem_euclid(len)
        } else {
            (current + i64::from(delta)).clamp(0, len - 1)
        } as usize;
        self.select_index(next)
    }

    fn select_index(&mut self, index: usize) -> Vec<Effect> {
        if self.commits.is_empty() {
            return Vec::new();
        }
        let next = index.min(self.commits.len() - 1);
        if next == self.selected && self.preview.is_some() {
            return self.request_more_if_needed();
        }
        self.selected = next;
        self.selected_oid = self.selected_commit().map(|commit| commit.id.clone());
        self.parent_index = 0;
        self.diff_scroll = 0;
        let mut effects = self.request_preview();
        effects.extend(self.request_more_if_needed());
        effects
    }

    fn request_preview(&mut self) -> Vec<Effect> {
        let revision = if matches!(self.view, View::Log) {
            let Some(commit) = self.selected_commit() else {
                self.preview = None;
                self.preview_loading = false;
                return Vec::new();
            };
            commit.id.to_string()
        } else if let Some(detail) = &self.preview {
            detail.commit.id.to_string()
        } else {
            self.preview_loading = false;
            return Vec::new();
        };
        self.preview = None;
        self.preview_loading = true;
        self.preview_error = None;
        vec![Effect::LoadPreview {
            revision,
            parent_index: self.parent_index,
        }]
    }

    fn next_parent(&mut self) -> Vec<Effect> {
        let parent_count = self.preview.as_ref().map_or_else(
            || {
                self.selected_commit()
                    .map_or(0, |commit| commit.parents.len())
            },
            |detail| detail.commit.parents.len(),
        );
        if parent_count <= 1 {
            return Vec::new();
        }
        self.parent_index = (self.parent_index + 1) % parent_count;
        self.diff_scroll = 0;
        self.request_preview()
    }

    fn scroll_diff(&mut self, delta: i32) {
        let maximum = self.diff_len().saturating_sub(1) as i64;
        self.diff_scroll = (self.diff_scroll as i64 + i64::from(delta)).clamp(0, maximum) as usize;
    }

    fn diff_len(&self) -> usize {
        match self.view {
            View::Compare => self
                .inspect
                .comparison
                .as_ref()
                .map_or(0, |comparison| comparison.diff.lines.len()),
            View::Status | View::StatusDiff => self
                .inspect
                .working_diff
                .as_ref()
                .map_or(0, |diff| diff.lines.len()),
            View::Blob => self
                .inspect
                .blob
                .as_ref()
                .map_or(0, |blob| blob.bytes().split(|byte| *byte == b'\n').count()),
            _ => self
                .preview
                .as_ref()
                .map_or(0, |detail| detail.diff.lines.len()),
        }
    }

    fn seek_diff_anchor(&mut self, direction: i32, hunks: bool) {
        let diff = match self.view {
            View::Compare => self
                .inspect
                .comparison
                .as_ref()
                .map(|comparison| &comparison.diff),
            View::Status | View::StatusDiff => self.inspect.working_diff.as_ref(),
            _ => self.preview.as_ref().map(|detail| &detail.diff),
        };
        let Some(diff) = diff else {
            return;
        };
        let anchors: Vec<usize> = if hunks {
            diff.files
                .iter()
                .flat_map(|file| file.hunks.iter().map(|hunk| hunk.header_line))
                .collect()
        } else {
            diff.files.iter().map(|file| file.header_line).collect()
        };
        if anchors.is_empty() {
            return;
        }
        self.diff_scroll = if direction >= 0 {
            anchors
                .iter()
                .copied()
                .find(|anchor| *anchor > self.diff_scroll)
                .unwrap_or(anchors[0])
        } else {
            anchors
                .iter()
                .copied()
                .rev()
                .find(|anchor| *anchor < self.diff_scroll)
                .unwrap_or(*anchors.last().unwrap_or(&0))
        };
    }

    fn seek_match(&mut self, forward: bool, include_current: bool) -> Vec<Effect> {
        if self.search_query.is_empty() {
            return Vec::new();
        }
        let needle = self.search_query.to_lowercase();
        if matches!(
            self.view,
            View::Refs | View::Status | View::Tree | View::Blame | View::Stash
        ) {
            let len = self.active_len();
            if len == 0 {
                return Vec::new();
            }
            for step in usize::from(!include_current)..=len {
                let index = if forward {
                    (self.inspect.selected + step) % len
                } else {
                    (self.inspect.selected + len - (step % len)) % len
                };
                let haystack =
                    match self.view {
                        View::Refs => self.inspect.refs.get(index).map(|item| {
                            format!(
                                "{} {} {}",
                                item.short_name.display(),
                                item.full_name.display(),
                                item.subject
                            )
                        }),
                        View::Status => self
                            .inspect
                            .status_entries()
                            .get(index)
                            .map(|item| item.path.display.clone()),
                        View::Tree => self
                            .inspect
                            .tree
                            .get(index)
                            .map(|item| item.path.display.clone()),
                        View::Blame => self.inspect.blame.get(index).map(|item| {
                            format!("{} {} {}", item.author, item.summary, item.content)
                        }),
                        View::Stash => self
                            .inspect
                            .stashes
                            .get(index)
                            .map(|item| format!("{} {}", item.selector, item.subject)),
                        _ => None,
                    }
                    .unwrap_or_default()
                    .to_lowercase();
                if haystack.contains(&needle) {
                    self.inspect.selected = index;
                    return self.inspect_selection_effects();
                }
            }
            return Vec::new();
        }
        if self.view == View::Log && self.focus == Focus::List {
            if self.commits.is_empty() {
                return Vec::new();
            }
            let len = self.commits.len();
            let first_step = usize::from(!include_current);
            for step in first_step..=len {
                let index = if forward {
                    (self.selected + step) % len
                } else {
                    (self.selected + len - (step % len)) % len
                };
                let commit = &self.commits[index];
                let haystack = format!(
                    "{} {} {} {}",
                    commit.id.hex, commit.subject, commit.author.name, commit.author.email
                )
                .to_lowercase();
                if haystack.contains(&needle) {
                    return self.select_index(index);
                }
            }
            if self.has_more {
                self.search_pending = Some(forward);
                if !self.history_loading {
                    self.history_loading = true;
                    self.history_error = None;
                    return vec![Effect::LoadHistory {
                        offset: self.commits.len(),
                        limit: PAGE_SIZE,
                    }];
                }
            }
        } else {
            let diff = match self.view {
                View::Compare => self.inspect.comparison.as_ref().map(|value| &value.diff),
                View::Status | View::StatusDiff => self.inspect.working_diff.as_ref(),
                _ => self.preview.as_ref().map(|value| &value.diff),
            };
            if let Some(diff) = diff {
                let len = diff.lines.len();
                if len == 0 {
                    return Vec::new();
                }
                for step in 1..=len {
                    let index = if forward {
                        (self.diff_scroll + step) % len
                    } else {
                        (self.diff_scroll + len - (step % len)) % len
                    };
                    if diff.lines[index].text.to_lowercase().contains(&needle) {
                        self.diff_scroll = index;
                        break;
                    }
                }
            }
        }
        Vec::new()
    }
}

fn diff_path_at(diff: &Diff, line: usize) -> Option<GitPath> {
    diff.files
        .iter()
        .rev()
        .find(|file| file.header_line <= line)
        .and_then(|file| file.new_path.clone().or_else(|| file.old_path.clone()))
}

pub fn palette_commands(query: &str) -> Vec<PaletteCommand> {
    let commands = [
        ("Move down", Action::Move(1)),
        ("Move up", Action::Move(-1)),
        ("Page down", Action::Page(1)),
        ("Page up", Action::Page(-1)),
        ("First item", Action::First),
        ("Last item", Action::Last),
        ("Inspect commit", Action::Open),
        ("Back", Action::Back),
        ("Quit", Action::Quit),
        ("Toggle preview", Action::TogglePreview),
        ("Toggle focus", Action::ToggleFocus),
        ("Search", Action::StartSearch),
        ("Show help", Action::ToggleHelp),
        ("Next search match", Action::NextMatch),
        ("Previous search match", Action::PreviousMatch),
        ("Next hunk", Action::NextHunk(1)),
        ("Previous hunk", Action::NextHunk(-1)),
        ("Next changed file", Action::NextFile(1)),
        ("Previous changed file", Action::NextFile(-1)),
        ("Next merge parent", Action::NextParent),
        ("View history", Action::ViewLog),
        ("View refs", Action::ViewRefs),
        ("View status", Action::ViewStatus),
        ("View tree", Action::ViewTree),
        ("View blame", Action::ViewBlame),
        ("View stashes", Action::ViewStash),
        ("Mark comparison endpoint", Action::Mark),
        ("Compare revisions", Action::StartCompare),
        ("Swap comparison sides", Action::SwapCompare),
        ("Toggle merge-base comparison", Action::ToggleCompareMode),
        ("Toggle staged/unstaged diff", Action::ToggleStatusDiff),
        ("Ascend tree", Action::Ascend),
        ("Retry failed request", Action::RetryFailed),
        ("Dismiss errors", Action::DismissErrors),
    ];
    let needle = query.to_lowercase();
    commands
        .into_iter()
        .filter(|(name, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
        .map(|(name, action)| PaletteCommand { name, action })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::{
        ConflictStage, ConflictStages, DiffLine, DiffLineKind, ObjectFormat, RefInfo, RefKind,
        RefName, Signature, StatusEntry,
    };

    use super::*;

    fn oid(character: char) -> Oid {
        format!("{character:0<40}").parse().unwrap()
    }

    fn commit(character: char, subject: &str) -> Commit {
        Commit {
            id: oid(character),
            parents: Vec::new(),
            author: Signature {
                name: "Pat".into(),
                email: "pat@example.invalid".into(),
                timestamp: 0,
                timezone: "Z".into(),
            },
            committer: Signature {
                name: "Pat".into(),
                email: "pat@example.invalid".into(),
                timestamp: 0,
                timezone: "Z".into(),
            },
            decorations: Vec::new(),
            subject: subject.into(),
            body: String::new(),
        }
    }

    fn status_entry(index: StatusCode, worktree: StatusCode, path: &[u8]) -> StatusEntry {
        StatusEntry {
            index,
            worktree,
            path: GitPath::new(path.to_vec()),
            original_path: None,
            submodule: "N...".into(),
            head_mode: None,
            index_mode: None,
            worktree_mode: None,
            head_oid: None,
            index_oid: None,
            conflict: None,
        }
    }

    fn working_diff(text: &str) -> Diff {
        Diff {
            lines: vec![DiffLine {
                kind: DiffLineKind::Added,
                text: text.into(),
            }],
            files: Vec::new(),
            truncated: false,
        }
    }

    fn repository() -> Repository {
        Repository {
            root: PathBuf::from("/tmp/repo"),
            worktree: Some(PathBuf::from("/tmp/repo")),
            git_dir: PathBuf::from("/tmp/repo/.git"),
            bare: false,
            object_format: ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: Some(oid('a')),
            branch: Some("main".into()),
        }
    }

    fn app() -> App {
        let mut app = App::new(repository(), "HEAD".into(), Vec::new(), false);
        app.apply_history(HistoryPage {
            commits: vec![commit('a', "first"), commit('b', "needle")],
            offset: 0,
            limit: 256,
            has_more: false,
        });
        app
    }

    #[test]
    fn selection_is_stable_by_oid_across_refresh() {
        let mut app = app();
        let _ = app.update(Action::Move(1), 10);
        let selected = app.selected_oid.clone();
        app.apply_history(HistoryPage {
            commits: vec![
                commit('c', "new"),
                commit('b', "needle"),
                commit('a', "first"),
            ],
            offset: 0,
            limit: 256,
            has_more: false,
        });
        assert_eq!(app.selected_oid, selected);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn search_and_back_are_semantic_transitions() {
        let mut app = app();
        let _ = app.update(Action::StartSearch, 10);
        let _ = app.update(Action::SearchInput('n'), 10);
        let _ = app.update(Action::SearchInput('e'), 10);
        assert_eq!(app.selected, 1, "search should update incrementally");
        let _ = app.update(Action::AcceptSearch, 10);
        assert_eq!(app.selected, 1);
        let _ = app.update(Action::Open, 10);
        assert_eq!(app.view, View::Detail);
        let _ = app.update(Action::Back, 10);
        assert_eq!(app.view, View::Log);
        assert!(!app.should_quit);
        let _ = app.update(Action::Back, 10);
        assert!(app.should_quit);
    }

    #[test]
    fn cancelling_incremental_search_restores_selection() {
        let mut app = app();
        let _ = app.update(Action::StartSearch, 10);
        for character in "needle".chars() {
            let _ = app.update(Action::SearchInput(character), 10);
        }
        assert_eq!(app.selected, 1);
        let effects = app.update(Action::CancelOverlay, 10);
        assert_eq!(app.selected, 0);
        assert_eq!(app.search_query, "");
        assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));
    }

    #[test]
    fn search_requests_more_history_when_loaded_page_has_no_match() {
        let mut app = app();
        app.has_more = true;
        app.history_loading = false;
        let _ = app.update(Action::StartSearch, 10);
        let effects = app.update(Action::SearchInput('z'), 10);
        assert_eq!(app.search_pending, Some(true));
        assert_eq!(
            effects,
            [Effect::LoadHistory {
                offset: 2,
                limit: PAGE_SIZE
            }]
        );
    }

    #[test]
    fn next_match_advances_past_a_matching_current_commit() {
        let mut app = app();
        app.search_query = "Pat".into();
        let _ = app.update(Action::NextMatch, 10);
        assert_eq!(app.selected, 1);
        let _ = app.update(Action::PreviousMatch, 10);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn paged_previous_search_preserves_its_direction() {
        let mut app = app();
        app.search_query = "missing".into();
        app.has_more = true;
        app.history_loading = false;
        let effects = app.update(Action::PreviousMatch, 10);
        assert_eq!(app.search_pending, Some(false));
        assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));
        let _ = app.apply_history(HistoryPage {
            commits: vec![commit('c', "missing")],
            offset: 2,
            limit: PAGE_SIZE,
            has_more: false,
        });
        assert_eq!(app.selected, 2, "paged N search must continue backward");
        assert_eq!(app.selected_commit().unwrap().subject, "missing");
    }

    #[test]
    fn narrowing_layout_returns_focus_to_visible_history() {
        let mut app = app();
        app.focus = Focus::Preview;
        app.resize(60, 16);
        assert_eq!(app.focus, Focus::List);
    }

    #[test]
    fn show_start_opens_detail_while_history_loads() {
        let app = App::new(repository(), "HEAD~2".into(), Vec::new(), true);
        assert_eq!(app.view, View::Detail);
        assert!(app.history_loading);
        assert_eq!(
            app.initial_effects(),
            [
                Effect::LoadHistory {
                    offset: 0,
                    limit: PAGE_SIZE
                },
                Effect::LoadPreview {
                    revision: "HEAD~2".into(),
                    parent_index: 0
                }
            ]
        );
    }

    #[test]
    fn request_failures_are_isolated_and_retryable() {
        let mut app = app();
        app.history_loading = true;
        app.preview_loading = true;
        app.apply_error(RequestKind::Preview, &GitError::Timeout("commit-detail"));
        assert!(
            app.history_loading,
            "preview failure must not stop history loading"
        );
        assert!(!app.preview_loading);
        assert!(app.preview_error.is_some());

        let _ = app.apply_history(HistoryPage {
            commits: Vec::new(),
            offset: 2,
            limit: PAGE_SIZE,
            has_more: false,
        });
        assert!(
            app.preview_error.is_some(),
            "history success hid preview failure"
        );
        let effects = app.update(Action::RetryFailed, 10);
        assert!(app.preview_error.is_none());
        assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));

        app.apply_error(RequestKind::History, &GitError::Timeout("history"));
        app.apply_error(RequestKind::Preview, &GitError::Timeout("commit-detail"));
        let effects = app.update(Action::RetryFailed, 10);
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadHistory { .. }, Effect::LoadPreview { .. }]
        ));
        app.apply_error(RequestKind::History, &GitError::Timeout("history"));
        let _ = app.update(Action::DismissErrors, 10);
        assert!(!app.has_errors());
    }

    #[test]
    fn searchable_palette_executes_semantic_actions() {
        let mut app = app();
        let _ = app.update(Action::StartPalette, 10);
        for character in "toggle preview".chars() {
            let _ = app.update(Action::SearchInput(character), 10);
        }
        let _ = app.update(Action::ExecutePalette, 10);
        assert!(!app.show_preview);
        assert_eq!(app.overlay, Overlay::None);
    }

    #[test]
    fn view_switching_marked_compare_and_ref_picker_are_semantic() {
        let mut app = app();
        let effects = app.update(Action::ViewRefs, 10);
        assert_eq!(app.view, View::Refs);
        assert_eq!(effects, [Effect::LoadRefs]);
        let reference = RefInfo {
            full_name: RefName::new(b"refs/heads/main".to_vec()),
            short_name: RefName::new(b"main".to_vec()),
            kind: RefKind::LocalBranch,
            target: oid('a'),
            peeled: None,
            upstream: None,
            subject: "first".into(),
            timestamp: None,
            is_head: false,
        };
        let effects = app.apply_refs(vec![reference]);
        assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));
        let effects = app.update(Action::Open, 10);
        assert_eq!(app.view, View::Log);
        assert_eq!(app.revision, oid('a').to_string());
        assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));

        app.commits = vec![commit('a', "one"), commit('b', "two")];
        app.selected = 0;
        let _ = app.update(Action::Mark, 10);
        app.selected = 1;
        let effects = app.update(Action::StartCompare, 10);
        assert_eq!(app.view, View::Compare);
        assert_eq!(app.inspect.compare_mode, ComparisonMode::Exact);
        assert!(matches!(effects.as_slice(), [Effect::LoadCompare]));
    }

    #[test]
    fn status_toggle_clears_stale_patch_and_status_opens_dominant_diff() {
        let mut app = app();
        app.view = View::Status;
        let effects = app.apply_status(Status {
            entries: vec![status_entry(
                StatusCode::Modified,
                StatusCode::Modified,
                b"both.txt",
            )],
            ..Status::default()
        });
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadWorkingDiff { staged: true, .. }]
        ));
        app.apply_working_diff(working_diff("old staged"));
        let effects = app.update(Action::ToggleStatusDiff, 10);
        assert!(app.inspect.loading);
        assert!(app.inspect.working_diff.is_none());
        assert!(!app.inspect.status_diff_staged);
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadWorkingDiff { staged: false, .. }]
        ));
        app.apply_error(RequestKind::Inspect, &GitError::Timeout("working diff"));
        assert_eq!(
            app.inspect_error.as_ref().unwrap().operation,
            "load unstaged working diff"
        );
        assert!(
            app.inspect.working_diff.is_none(),
            "failed toggle exposed stale patch"
        );
        app.apply_working_diff(working_diff("old again"));
        app.apply_error(RequestKind::Inspect, &GitError::Timeout("status refresh"));
        assert!(
            app.inspect.working_diff.is_none(),
            "inspect failure retained an old patch"
        );
        app.apply_working_diff(working_diff("fresh unstaged"));
        let _ = app.update(Action::Open, 10);
        assert_eq!(app.view, View::StatusDiff);
        let _ = app.update(Action::Back, 10);
        assert_eq!(app.view, View::Status);
    }

    #[test]
    fn status_sort_keeps_conflicts_first_and_mixed_state_intact() {
        let mut app = app();
        app.view = View::Status;
        let mut conflict = status_entry(
            StatusCode::UpdatedButUnmerged,
            StatusCode::UpdatedButUnmerged,
            b"conflict.txt",
        );
        let stage = ConflictStage {
            mode: "100644".into(),
            oid: None,
        };
        conflict.conflict = Some(ConflictStages {
            base: stage.clone(),
            ours: stage.clone(),
            theirs: stage,
            worktree_mode: "100644".into(),
        });
        let mixed = status_entry(StatusCode::Modified, StatusCode::Modified, b"mixed.txt");
        let _ = app.apply_status(Status {
            entries: vec![mixed, conflict],
            ..Status::default()
        });
        assert_eq!(app.inspect.status_entries()[0].path.display, "conflict.txt");
        assert_eq!(app.inspect.status_entries()[1].index.porcelain_char(), 'M');
        assert_eq!(
            app.inspect.status_entries()[1].worktree.porcelain_char(),
            'M'
        );
    }

    #[test]
    fn inspect_search_cancel_restores_semantic_cursor() {
        let mut app = app();
        app.view = View::Status;
        let _ = app.apply_status(Status {
            entries: vec![
                status_entry(StatusCode::Modified, StatusCode::Unmodified, b"first.txt"),
                status_entry(StatusCode::Unmodified, StatusCode::Modified, b"needle.txt"),
            ],
            ..Status::default()
        });
        let _ = app.update(Action::StartSearch, 10);
        for character in "needle".chars() {
            let _ = app.update(Action::SearchInput(character), 10);
        }
        assert_eq!(app.inspect.selected, 1);
        let effects = app.update(Action::CancelOverlay, 10);
        assert_eq!(app.inspect.selected, 0);
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadWorkingDiff { .. }]
        ));
    }

    #[test]
    fn compare_picker_cancel_and_missing_blame_path_are_safe() {
        let mut app = app();
        let _ = app.update(Action::StartCompare, 10);
        assert!(app.inspect.compare_picker);
        assert_eq!(app.view, View::Refs);
        let _ = app.update(Action::Back, 10);
        assert!(!app.inspect.compare_picker);
        assert_eq!(app.view, View::Log);

        app.paths.clear();
        app.preview = None;
        let effects = app.update(Action::ViewBlame, 10);
        assert!(effects.is_empty());
        assert_eq!(app.view, View::Log);
        assert_eq!(app.inspect_error.as_ref().unwrap().operation, "open blame");
        assert!(
            app.inspect_error
                .as_ref()
                .unwrap()
                .detail
                .contains("select a file path")
        );
    }

    #[test]
    fn refs_use_peeled_authoritative_oid_and_blame_preview_keeps_path() {
        let mut app = app();
        app.view = View::Refs;
        let peeled = oid('b');
        let reference = RefInfo {
            full_name: RefName::new(b"refs/tags/release-\xff".to_vec()),
            short_name: RefName::new(b"release-\xff".to_vec()),
            kind: RefKind::Tag,
            target: oid('a'),
            peeled: Some(peeled.clone()),
            upstream: None,
            subject: "release".into(),
            timestamp: None,
            is_head: false,
        };
        let effects = app.apply_refs(vec![reference]);
        assert!(
            matches!(effects.as_slice(), [Effect::LoadPreview { revision, .. }] if revision == &peeled.to_string())
        );
        let effects = app.update(Action::Open, 10);
        assert_eq!(app.revision, peeled.to_string());
        assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));

        let path = GitPath::new(b"src/lib.rs".to_vec());
        app.inspect.blame_path = Some(path.clone());
        app.view = View::Blame;
        app.view_stack.clear();
        app.inspect.blame = vec![BlameLine {
            final_line: 1,
            original_line: 1,
            id: oid('a'),
            author: "Pat".into(),
            author_mail: "pat@example.invalid".into(),
            author_time: Some(0),
            summary: "line".into(),
            filename: path.clone(),
            content: "text".into(),
            boundary: false,
            previous: None,
        }];
        let _ = app.update(Action::Open, 10);
        assert_eq!(app.preview_paths(), vec![path]);
    }

    #[test]
    fn non_log_start_and_switch_do_not_claim_history_is_loading() {
        let mut app = App::new(repository(), "HEAD".into(), Vec::new(), false);
        app.set_start_view(View::Status, None, "HEAD".into(), ComparisonMode::Exact);
        assert!(!app.history_loading);
        let effects = app.update(Action::Back, 10);
        assert!(app.history_loading);
        assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));
        let _ = app.update(Action::ViewRefs, 10);
        assert!(!app.history_loading);
    }

    #[test]
    fn pagination_effect_is_bounded_and_idempotent_while_loading() {
        let mut app = app();
        app.has_more = true;
        app.history_loading = false;
        app.selected = app.commits.len() - 1;
        let effects = app.request_more_if_needed();
        assert_eq!(
            effects,
            [Effect::LoadHistory {
                offset: 2,
                limit: PAGE_SIZE
            }]
        );
        assert!(app.request_more_if_needed().is_empty());
    }
}
