use crate::{
    domain::{Commit, CommitDetail, GitPath, HistoryPage, Oid, Repository},
    git::GitError,
};

const PAGE_SIZE: usize = 256;
const PREFETCH_DISTANCE: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Log,
    Detail,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    History,
    Preview,
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
            should_quit: false,
            dirty: true,
        }
    }

    pub fn initial_effects(&self) -> Vec<Effect> {
        let mut effects = vec![Effect::LoadHistory {
            offset: 0,
            limit: PAGE_SIZE,
        }];
        if self.show_mode {
            effects.push(Effect::LoadPreview {
                revision: self.revision.clone(),
                parent_index: 0,
            });
        }
        effects
    }

    pub fn selected_commit(&self) -> Option<&Commit> {
        self.commits.get(self.selected)
    }

    pub fn update(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
        self.dirty = true;
        if self.overlay != Overlay::None {
            return self.update_overlay(action, page_rows);
        }

        match action {
            Action::Move(delta) => {
                if self.view == View::Detail || self.focus == Focus::Preview {
                    self.scroll_diff(delta);
                    Vec::new()
                } else {
                    self.move_selection(delta, false)
                }
            }
            Action::Page(delta) => {
                if self.view == View::Detail || self.focus == Focus::Preview {
                    self.scroll_diff(delta.saturating_mul(page_rows.max(1) as i32));
                    Vec::new()
                } else {
                    self.move_selection(delta.saturating_mul(page_rows.max(1) as i32), false)
                }
            }
            Action::First => {
                if self.view == View::Detail || self.focus == Focus::Preview {
                    self.diff_scroll = 0;
                    Vec::new()
                } else {
                    self.select_index(0)
                }
            }
            Action::Last => {
                if self.view == View::Detail || self.focus == Focus::Preview {
                    self.diff_scroll = self.diff_len().saturating_sub(1);
                    Vec::new()
                } else {
                    self.select_index(self.commits.len().saturating_sub(1))
                }
            }
            Action::Open => {
                self.view = View::Detail;
                self.focus = Focus::Preview;
                Vec::new()
            }
            Action::Back => {
                if self.view == View::Detail {
                    self.view = View::Log;
                    self.focus = Focus::List;
                } else {
                    self.should_quit = true;
                }
                Vec::new()
            }
            Action::Quit => {
                if self.view == View::Detail {
                    self.view = View::Log;
                    self.focus = Focus::List;
                } else {
                    self.should_quit = true;
                }
                Vec::new()
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
                    original_scroll,
                    ..
                },
                Action::CancelOverlay | Action::Back | Action::Quit,
            ) => {
                self.search_query = previous_query.clone();
                self.selected = (*original_selected).min(self.commits.len().saturating_sub(1));
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
            self.request_preview()
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
        if current == Some(&detail.commit.id) {
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
        }
        self.dirty = true;
    }

    pub fn has_errors(&self) -> bool {
        self.history_error.is_some() || self.preview_error.is_some()
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
        let Some(commit) = self.selected_commit() else {
            self.preview = None;
            self.preview_loading = false;
            return Vec::new();
        };
        let revision = commit.id.to_string();
        self.preview = None;
        self.preview_loading = true;
        self.preview_error = None;
        vec![Effect::LoadPreview {
            revision,
            parent_index: self.parent_index,
        }]
    }

    fn next_parent(&mut self) -> Vec<Effect> {
        let parent_count = self
            .selected_commit()
            .map_or(0, |commit| commit.parents.len());
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
        self.preview
            .as_ref()
            .map_or(0, |detail| detail.diff.lines.len())
    }

    fn seek_diff_anchor(&mut self, direction: i32, hunks: bool) {
        let Some(detail) = &self.preview else {
            return;
        };
        let anchors: Vec<usize> = if hunks {
            detail
                .diff
                .files
                .iter()
                .flat_map(|file| file.hunks.iter().map(|hunk| hunk.header_line))
                .collect()
        } else {
            detail
                .diff
                .files
                .iter()
                .map(|file| file.header_line)
                .collect()
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
        } else if let Some(detail) = &self.preview {
            let len = detail.diff.lines.len();
            if len == 0 {
                return Vec::new();
            }
            for step in 1..=len {
                let index = if forward {
                    (self.diff_scroll + step) % len
                } else {
                    (self.diff_scroll + len - (step % len)) % len
                };
                if detail.diff.lines[index]
                    .text
                    .to_lowercase()
                    .contains(&needle)
                {
                    self.diff_scroll = index;
                    break;
                }
            }
        }
        Vec::new()
    }
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

    use crate::domain::{ObjectFormat, Signature};

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
