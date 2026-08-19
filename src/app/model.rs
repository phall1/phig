//! Application state and public state-machine vocabulary.

use crate::domain::{Commit, CommitDetail, ComparisonMode, GitPath, Oid, Repository};

use super::{PAGE_SIZE, inspect::InspectState};

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
    FilePicker {
        draft: String,
        selected: usize,
        original_scroll: usize,
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
    StartFilePicker,
    ToggleHelp,
    SearchInput(char),
    SearchBackspace,
    AcceptSearch,
    CancelOverlay,
    PaletteMove(i32),
    ExecutePalette,
    FilePickerMove(i32),
    AcceptFilePicker,
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
    CopySelection,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    Commit,
    Ref,
    File,
    Hunk,
    Line,
    Compare,
}

impl SelectionTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Commit => "COMMIT",
            Self::Ref => "REF",
            Self::File => "FILE",
            Self::Hunk => "HUNK",
            Self::Line => "LINE",
            Self::Compare => "COMPARE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionContract {
    pub target: SelectionTarget,
    pub accept_key: String,
    pub cancel_keys: String,
}

impl SelectionContract {
    pub fn new(target: SelectionTarget, accept_key: String, cancel_keys: String) -> Self {
        Self {
            target,
            accept_key,
            cancel_keys,
        }
    }

    #[cfg(test)]
    pub fn default_keys(target: SelectionTarget) -> Self {
        Self::new(target, "Enter".into(), "Esc/q".into())
    }
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
    pub history_page_size: usize,
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
    pub selection_contract: Option<SelectionContract>,
    pub should_quit: bool,
    pub notice: Option<String>,
    pub copy_requested: bool,
    pub redraw_requested: bool,
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
            history_page_size: PAGE_SIZE,
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
            selection_contract: None,
            should_quit: false,
            notice: None,
            copy_requested: false,
            redraw_requested: false,
            dirty: true,
        }
    }

    pub fn set_history_page_size(&mut self, size: usize) {
        self.history_page_size = size.clamp(1, 4096);
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
                limit: self.history_page_size,
            }],
            View::Detail => vec![
                Effect::LoadHistory {
                    offset: 0,
                    limit: self.history_page_size,
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
}
