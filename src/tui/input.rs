//! Pure terminal-key to semantic-action translation.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{Action, App, Overlay, View},
    config::KeyBindings,
};

pub(super) fn resolve_action(app: &App, bindings: &KeyBindings, key: KeyEvent) -> Option<Action> {
    let default = key_action(app, key);
    if matches!(
        app.overlay,
        Overlay::Search { .. } | Overlay::Palette { .. } | Overlay::FilePicker { .. }
    ) {
        // Text-entry overlays are modal: printable text must not be
        // intercepted by a global semantic remap.
        default
    } else {
        bindings.resolve(key, default)
    }
}

pub(super) fn key_action(app: &App, key: KeyEvent) -> Option<Action> {
    match &app.overlay {
        Overlay::Help => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                    Some(Action::CancelOverlay)
                }
                _ => None,
            };
        }
        Overlay::Search { .. } => {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelOverlay),
                KeyCode::Enter => Some(Action::AcceptSearch),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(Action::SearchInput(character))
                }
                _ => None,
            };
        }
        Overlay::Palette { .. } => {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelOverlay),
                KeyCode::Enter => Some(Action::ExecutePalette),
                KeyCode::Down => Some(Action::PaletteMove(1)),
                KeyCode::Up => Some(Action::PaletteMove(-1)),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(Action::SearchInput(character))
                }
                _ => None,
            };
        }
        Overlay::FilePicker { .. } => {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelOverlay),
                KeyCode::Enter => Some(Action::AcceptFilePicker),
                KeyCode::Down => Some(Action::FilePickerMove(1)),
                KeyCode::Up => Some(Action::FilePickerMove(-1)),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(Action::SearchInput(character))
                }
                _ => None,
            };
        }
        Overlay::None => {}
    }

    if app.has_errors() {
        match key.code {
            KeyCode::Char('r') => return Some(Action::RetryFailed),
            KeyCode::Esc => return Some(Action::DismissErrors),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Move(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Move(-1)),
        KeyCode::PageDown => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Page(1))
        }
        KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Redraw),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Page(-1))
        }
        KeyCode::Home | KeyCode::Char('g') => Some(Action::First),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Last),
        KeyCode::Enter => Some(Action::Open),
        KeyCode::Backspace if app.view == View::Tree => Some(Action::Ascend),
        KeyCode::Esc | KeyCode::Backspace => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('m') => Some(Action::ViewLog),
        KeyCode::Char('r') => Some(Action::ViewRefs),
        KeyCode::Char('s') => Some(Action::ViewStatus),
        KeyCode::Char('t') => Some(Action::ViewTree),
        KeyCode::Char('b') => Some(Action::ViewBlame),
        KeyCode::Char('z') => Some(Action::ViewStash),
        KeyCode::Char('v') => Some(Action::Mark),
        KeyCode::Char('c') => Some(Action::StartCompare),
        KeyCode::Char('x') if app.view == View::Compare => Some(Action::SwapCompare),
        KeyCode::Char('M') if app.view == View::Compare => Some(Action::ToggleCompareMode),
        KeyCode::Char('d') if app.view == View::Status => Some(Action::ToggleStatusDiff),
        KeyCode::Char('/') => Some(Action::StartSearch),
        KeyCode::Char(':') => Some(Action::StartPalette),
        KeyCode::Char('f') => Some(Action::StartFilePicker),
        KeyCode::Char('n') => Some(Action::NextMatch),
        KeyCode::Char('N') => Some(Action::PreviousMatch),
        KeyCode::Tab if app.view == View::Detail => Some(Action::NextFile(1)),
        KeyCode::BackTab if app.view == View::Detail => Some(Action::NextFile(-1)),
        KeyCode::Tab | KeyCode::BackTab => Some(Action::ToggleFocus),
        KeyCode::Char('p') => Some(Action::TogglePreview),
        KeyCode::Char('y') => Some(Action::CopySelection),
        KeyCode::Char('P') if app.view == View::Detail => Some(Action::NextParent),
        KeyCode::Char(']') => Some(Action::NextHunk(1)),
        KeyCode::Char('[') => Some(Action::NextHunk(-1)),
        KeyCode::Char('}') => Some(Action::NextFile(1)),
        KeyCode::Char('{') => Some(Action::NextFile(-1)),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        _ => None,
    }
}

/// Handle keys that mutate state outside the regular action return path.
pub fn handle_help_key(app: &mut App, key: &KeyEvent) -> bool {
    if matches!(app.overlay, Overlay::None) && key.code == KeyCode::Char('?') {
        app.show_help();
        true
    } else {
        false
    }
}
