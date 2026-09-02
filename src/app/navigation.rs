//! View-independent navigation and selection transitions.

use crate::domain::{ComparisonMode, Diff, GitPath, StatusCode, TreeEntryKind};

use super::{Action, App, Effect, Focus, View};

impl App {
    pub fn take_copy_request(&mut self) -> bool {
        std::mem::take(&mut self.copy_requested)
    }

    pub fn take_redraw_request(&mut self) -> bool {
        std::mem::take(&mut self.redraw_requested)
    }

    pub fn set_notice(&mut self, message: impl Into<String>) {
        self.notice = Some(message.into());
        self.dirty = true;
    }

    pub fn active_diff(&self) -> Option<&Diff> {
        match self.view {
            View::Compare => self
                .inspect
                .comparison
                .as_ref()
                .map(|comparison| &comparison.diff),
            View::Status | View::StatusDiff => self.inspect.working_diff.as_ref(),
            View::Blob | View::Tree => None,
            View::Log | View::Detail | View::Refs | View::Blame | View::Stash => {
                self.preview.as_ref().map(|detail| &detail.diff)
            }
        }
    }

    pub fn copy_value(&self) -> Option<String> {
        match self.view {
            View::Log => self.selected_commit().map(|commit| commit.id.to_string()),
            View::Detail => self
                .preview
                .as_ref()
                .map(|detail| detail.commit.id.to_string()),
            View::Compare => self
                .inspect
                .comparison
                .as_ref()
                .map(|value| value.resolved_head.to_string()),
            View::Refs => self
                .inspect
                .refs
                .get(self.inspect.selected)
                .map(|value| value.peeled.as_ref().unwrap_or(&value.target).to_string()),
            View::Status | View::StatusDiff | View::Tree | View::Blob | View::Blame => {
                self.active_path().map(|path| path.display)
            }
            View::Stash => self
                .inspect
                .stashes
                .get(self.inspect.selected)
                .map(|value| value.id.to_string()),
        }
    }

    pub(super) fn active_path(&self) -> Option<GitPath> {
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

    pub(super) fn join_tree_path(&self, child: &GitPath) -> GitPath {
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

    pub(super) fn active_len(&self) -> usize {
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

    pub(super) fn move_active(&mut self, delta: i32) -> Vec<Effect> {
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

    pub(super) fn first_active(&mut self) -> Vec<Effect> {
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

    pub(super) fn last_active(&mut self) -> Vec<Effect> {
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

    pub(super) fn inspect_selection_effects(&mut self) -> Vec<Effect> {
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

    pub(super) fn open_active(&mut self) -> Vec<Effect> {
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
                    // Opening one ref narrows the log to that endpoint, so any
                    // active ref scope no longer applies.
                    self.focus_revision(revision, Some(label));
                    self.commits.clear();
                    self.preview = None;
                    self.selected = 0;
                    self.history_loading = true;
                    self.view_stack.push(View::Refs);
                    self.view = View::Log;
                    vec![Effect::LoadHistory {
                        offset: 0,
                        limit: self.history_page_size,
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

    pub(super) fn start_compare(&mut self) -> Vec<Effect> {
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
            self.preview = None;
            self.preview_loading = false;
            self.inspect.loading = true;
            vec![Effect::LoadRefs]
        }
    }

    pub(super) fn ascend_tree(&mut self) -> Vec<Effect> {
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

    pub(super) fn move_selection(&mut self, delta: i32, wrap: bool) -> Vec<Effect> {
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

    pub(super) fn select_index(&mut self, index: usize) -> Vec<Effect> {
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

    pub(super) fn request_preview(&mut self) -> Vec<Effect> {
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

    pub(super) fn next_parent(&mut self) -> Vec<Effect> {
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

    pub(super) fn scroll_diff(&mut self, delta: i32) {
        let maximum = self.diff_len().saturating_sub(1) as i64;
        self.diff_scroll = (self.diff_scroll as i64 + i64::from(delta)).clamp(0, maximum) as usize;
    }

    pub(super) fn diff_len(&self) -> usize {
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

    pub(super) fn seek_diff_anchor(&mut self, direction: i32, hunks: bool) {
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
}

fn diff_path_at(diff: &Diff, line: usize) -> Option<GitPath> {
    diff.files
        .iter()
        .rev()
        .find(|file| file.header_line <= line)
        .and_then(|file| file.new_path.clone().or_else(|| file.old_path.clone()))
}
