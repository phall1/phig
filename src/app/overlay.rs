//! Search, command palette, and changed-file overlay behavior.

use super::{Action, App, Effect, Overlay, View, commands::palette_commands};

impl App {
    pub(super) fn update_overlay(&mut self, action: Action, page_rows: usize) -> Vec<Effect> {
        let file_picker_count = match (&self.overlay, &action) {
            (Overlay::FilePicker { draft, .. }, Action::FilePickerMove(_)) => {
                self.cached_file_picker_entries(draft).len()
            }
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
                restore_preview = self.selected != *original_selected
                    || self.inspect.selected != *original_inspect_selected;
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
        let effects = self.overlay_effects(
            palette_action,
            file_selection,
            seek,
            restore_preview,
            page_rows,
        );
        self.prepare_file_picker();
        effects
    }

    fn overlay_effects(
        &mut self,
        palette_action: Option<Action>,
        file_selection: Option<(String, usize)>,
        seek: bool,
        restore_preview: bool,
        page_rows: usize,
    ) -> Vec<Effect> {
        if let Some(action) = palette_action {
            self.update(action, page_rows)
        } else if let Some((query, selected)) = file_selection {
            let header_line = self
                .cached_file_picker_entries(&query)
                .get(selected)
                .map(|(_, line)| *line);
            if let Some(header_line) = header_line {
                self.diff_scroll = header_line;
            }
            Vec::new()
        } else if seek {
            self.seek_match(true, true)
        } else if restore_preview && !self.diff_fullscreen {
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
        crate::fuzzy::ranked(self.file_picker_source(), query, |(path, _)| path)
            .into_iter()
            .map(|(path, header_line)| (path.to_owned(), header_line))
            .collect()
    }
}
