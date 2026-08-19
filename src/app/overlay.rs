//! Search, command palette, and changed-file overlay behavior.

use super::{Action, App, Effect, Focus, Overlay, View, commands::palette_commands};

impl App {
    pub(super) fn update_overlay(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
        let file_picker_count = match &self.overlay {
            Overlay::FilePicker { draft, .. } => self.file_picker_entries(draft).len(),
            _ => 0,
        };
        let mut seek = false;
        let mut restore_preview = false;
        let mut palette_action = None;
        let mut file_selection = None;
        match (&mut self.overlay, action) {
            (
                Overlay::Help,
                Action::CancelOverlay | Action::Back | Action::Quit | Action::ToggleHelp,
            ) => {
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
            (
                Overlay::FilePicker {
                    draft, selected, ..
                },
                Action::SearchInput(character),
            ) => {
                draft.push(character);
                *selected = 0;
            }
            (
                Overlay::FilePicker {
                    draft, selected, ..
                },
                Action::SearchBackspace,
            ) => {
                draft.pop();
                *selected = 0;
            }
            (Overlay::FilePicker { selected, .. }, Action::FilePickerMove(delta)) => {
                if file_picker_count > 0 {
                    *selected = ((*selected as i64 + i64::from(delta))
                        .rem_euclid(file_picker_count as i64))
                        as usize;
                }
            }
            (
                Overlay::FilePicker {
                    draft, selected, ..
                },
                Action::AcceptFilePicker,
            ) => {
                file_selection = Some((draft.clone(), *selected));
                self.overlay = Overlay::None;
            }
            (
                Overlay::FilePicker {
                    original_scroll, ..
                },
                Action::CancelOverlay | Action::Back | Action::Quit,
            ) => {
                self.diff_scroll = *original_scroll;
                self.overlay = Overlay::None;
            }
            (_, Action::CancelOverlay) => self.overlay = Overlay::None,
            _ => {}
        }
        if let Some(action) = palette_action {
            self.update(action, page_rows)
        } else if let Some((query, selected)) = file_selection {
            if let Some((_, header_line)) = self.file_picker_entries(&query).get(selected) {
                self.diff_scroll = *header_line;
            }
            Vec::new()
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

    pub fn file_picker_entries(&self, query: &str) -> Vec<(String, usize)> {
        let needle = query.to_lowercase();
        self.active_diff()
            .into_iter()
            .flat_map(|diff| &diff.files)
            .filter_map(|file| {
                let path = file.new_path.as_ref().or(file.old_path.as_ref())?;
                (needle.is_empty() || path.display.to_lowercase().contains(&needle))
                    .then(|| (path.display.clone(), file.header_line))
            })
            .collect()
    }

    pub(super) fn seek_match(&mut self, forward: bool, include_current: bool) -> Vec<Effect> {
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
                        limit: self.history_page_size,
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
