//! Semantic key override compilation and effective display labels.

use std::collections::{BTreeMap, HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

#[derive(Debug, Clone, Default)]
pub struct KeyBindings {
    by_key: HashMap<KeySpec, Action>,
    overridden: HashSet<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct KeySpec {
    code: KeyCodeSpec,
    modifiers: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum KeyCodeSpec {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

impl KeyBindings {
    pub fn resolve(&self, event: KeyEvent, default: Option<Action>) -> Option<Action> {
        if let Some(key) = KeySpec::from_event(event)
            && let Some(action) = self.by_key.get(&key)
        {
            return Some(action.clone());
        }
        default.filter(|action| !self.overridden.contains(action.semantic_name()))
    }

    pub fn action_key_label(&self, action: &Action) -> String {
        if self.overridden.contains(action.semantic_name()) {
            return self
                .by_key
                .iter()
                .find_map(|(key, candidate)| (candidate == action).then(|| key.label()))
                .unwrap_or_else(|| "unbound".into());
        }
        match action {
            Action::Move(1) => "j",
            Action::Move(-1) => "k",
            Action::Page(1) => "Ctrl+d",
            Action::Page(-1) => "Ctrl+u",
            Action::First => "g",
            Action::Last => "G",
            Action::Open => "Enter",
            Action::Back => "Esc",
            Action::Quit => "q",
            Action::TogglePreview => "p",
            Action::ToggleFocus => "Tab",
            Action::StartSearch => "/",
            Action::StartPalette => ":",
            Action::StartFilePicker => "f",
            Action::ToggleHelp => "?",
            Action::NextMatch => "n",
            Action::PreviousMatch => "N",
            Action::NextHunk(1) => "]",
            Action::NextHunk(-1) => "[",
            Action::NextFile(1) => "}",
            Action::NextFile(-1) => "{",
            Action::NextParent => "P",
            Action::ViewLog => "m",
            Action::ViewRefs => "r",
            Action::ViewStatus => "s",
            Action::ViewTree => "t",
            Action::ViewBlame => "b",
            Action::ViewStash => "z",
            Action::Mark => "v",
            Action::StartCompare => "c",
            Action::SwapCompare => "x",
            Action::ToggleCompareMode => "M",
            Action::ToggleStatusDiff => "d",
            Action::Ascend => "Backspace",
            Action::CopySelection => "y",
            Action::Redraw => "Ctrl+l",
            _ => "unbound",
        }
        .into()
    }

    pub fn selection_key_labels(&self) -> (String, String) {
        let accept = self
            .effective_key_label(
                &Action::Open,
                KeySpec {
                    code: KeyCodeSpec::Enter,
                    modifiers: 0,
                },
                "Enter",
            )
            .unwrap_or_else(|| "unbound".into());
        let mut cancel = [
            self.effective_key_label(
                &Action::Back,
                KeySpec {
                    code: KeyCodeSpec::Esc,
                    modifiers: 0,
                },
                "Esc",
            ),
            self.effective_key_label(
                &Action::Quit,
                KeySpec {
                    code: KeyCodeSpec::Char('q'),
                    modifiers: 0,
                },
                "q",
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        cancel.dedup();
        let cancel = if cancel.is_empty() {
            "Ctrl+C".into()
        } else {
            cancel.join("/")
        };
        (accept, cancel)
    }

    fn effective_key_label(
        &self,
        action: &Action,
        default_key: KeySpec,
        default_label: &str,
    ) -> Option<String> {
        if self.overridden.contains(action.semantic_name()) {
            self.by_key
                .iter()
                .find_map(|(key, candidate)| (candidate == action).then(|| key.label()))
        } else if self.by_key.contains_key(&default_key) {
            None
        } else {
            Some(default_label.into())
        }
    }

    pub(crate) fn from_config(values: &BTreeMap<String, String>) -> Result<Self, String> {
        let mut by_key = HashMap::new();
        let mut overridden = HashSet::new();
        for (requested_action, key_name) in values {
            let action = Action::from_semantic_name(requested_action)
                .ok_or_else(|| format!("unknown semantic action `{requested_action}` in [keys]"))?;
            let key = parse_key(key_name).ok_or_else(|| {
                format!("invalid key `{key_name}` for action `{requested_action}`")
            })?;
            if key
                == (KeySpec {
                    code: KeyCodeSpec::Char('c'),
                    modifiers: 1,
                })
            {
                return Err(format!(
                    "key `ctrl+c` is reserved for interrupt and cannot bind `{requested_action}`"
                ));
            }
            if let Some(previous) = by_key.insert(key, action.clone()) {
                return Err(format!(
                    "key `{key_name}` conflicts with another override ({previous:?})"
                ));
            }
            overridden.insert(action.semantic_name());
        }
        Ok(Self { by_key, overridden })
    }
}

impl KeySpec {
    fn label(self) -> String {
        let mut label = String::new();
        if self.modifiers & 1 != 0 {
            label.push_str("Ctrl+");
        }
        if self.modifiers & 2 != 0 {
            label.push_str("Alt+");
        }
        if self.modifiers & 4 != 0 {
            label.push_str("Shift+");
        }
        label.push_str(match self.code {
            KeyCodeSpec::Char(value) => return format!("{label}{value}"),
            KeyCodeSpec::Enter => "Enter",
            KeyCodeSpec::Esc => "Esc",
            KeyCodeSpec::Tab => "Tab",
            KeyCodeSpec::BackTab => "BackTab",
            KeyCodeSpec::Backspace => "Backspace",
            KeyCodeSpec::Up => "Up",
            KeyCodeSpec::Down => "Down",
            KeyCodeSpec::Left => "Left",
            KeyCodeSpec::Right => "Right",
            KeyCodeSpec::PageUp => "PageUp",
            KeyCodeSpec::PageDown => "PageDown",
            KeyCodeSpec::Home => "Home",
            KeyCodeSpec::End => "End",
        });
        label
    }

    fn from_event(event: KeyEvent) -> Option<Self> {
        let mut implied_shift = false;
        let code = match event.code {
            KeyCode::Char(value) => {
                implied_shift = value.is_ascii_uppercase();
                KeyCodeSpec::Char(value.to_ascii_lowercase())
            }
            KeyCode::Enter => KeyCodeSpec::Enter,
            KeyCode::Esc => KeyCodeSpec::Esc,
            KeyCode::Tab => KeyCodeSpec::Tab,
            KeyCode::BackTab => KeyCodeSpec::BackTab,
            KeyCode::Backspace => KeyCodeSpec::Backspace,
            KeyCode::Up => KeyCodeSpec::Up,
            KeyCode::Down => KeyCodeSpec::Down,
            KeyCode::Left => KeyCodeSpec::Left,
            KeyCode::Right => KeyCodeSpec::Right,
            KeyCode::PageUp => KeyCodeSpec::PageUp,
            KeyCode::PageDown => KeyCodeSpec::PageDown,
            KeyCode::Home => KeyCodeSpec::Home,
            KeyCode::End => KeyCodeSpec::End,
            _ => return None,
        };
        let mut modifiers = 0;
        if event.modifiers.contains(KeyModifiers::CONTROL) {
            modifiers |= 1
        };
        if event.modifiers.contains(KeyModifiers::ALT) {
            modifiers |= 2
        };
        if event.modifiers.contains(KeyModifiers::SHIFT) || implied_shift {
            modifiers |= 4
        };
        Some(Self { code, modifiers })
    }
}

pub(super) fn parse_key(input: &str) -> Option<KeySpec> {
    let components = input.split('+').collect::<Vec<_>>();
    let (key_name, modifier_names) = components.split_last()?;
    if key_name.is_empty() {
        return None;
    }
    let mut modifiers = 0;
    for part in modifier_names {
        let flag = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => 1,
            "alt" => 2,
            "shift" => 4,
            _ => return None,
        };
        if modifiers & flag != 0 {
            return None;
        }
        modifiers |= flag;
    }
    let code = match key_name.to_ascii_lowercase().as_str() {
        "enter" => KeyCodeSpec::Enter,
        "esc" | "escape" => KeyCodeSpec::Esc,
        "tab" => KeyCodeSpec::Tab,
        "backtab" => KeyCodeSpec::BackTab,
        "backspace" => KeyCodeSpec::Backspace,
        "up" => KeyCodeSpec::Up,
        "down" => KeyCodeSpec::Down,
        "left" => KeyCodeSpec::Left,
        "right" => KeyCodeSpec::Right,
        "pageup" => KeyCodeSpec::PageUp,
        "pagedown" => KeyCodeSpec::PageDown,
        "home" => KeyCodeSpec::Home,
        "end" => KeyCodeSpec::End,
        _ if key_name.chars().count() == 1 => {
            let value = key_name.chars().next()?;
            if value.is_ascii_uppercase() {
                modifiers |= 4;
            }
            KeyCodeSpec::Char(value.to_ascii_lowercase())
        }
        _ => return None,
    };
    Some(KeySpec { code, modifiers })
}
