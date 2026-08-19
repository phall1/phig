use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{app::Action, domain::ComparisonMode};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot determine the XDG configuration directory; pass --config PATH")]
    NoConfigDirectory,
    #[error("configuration file {path} does not exist")]
    Missing { path: PathBuf },
    #[error("cannot read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid configuration {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid configuration {path}: {message}")]
    Validate { path: PathBuf, message: String },
    #[error("cannot write configuration {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub diff: DiffConfig,
    #[serde(default)]
    pub compare: CompareConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            ui: UiConfig::default(),
            diff: DiffConfig::default(),
            compare: CompareConfig::default(),
            limits: LimitsConfig::default(),
            theme: ThemeConfig::default(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub preview: bool,
    pub alternate_screen: bool,
    pub mouse: bool,
    pub date: String,
    pub color: String,
    pub clipboard: String,
}
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            preview: true,
            alternate_screen: true,
            mouse: false,
            date: "relative".into(),
            color: "auto".into(),
            clipboard: "off".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiffConfig {
    pub context: usize,
    pub algorithm: String,
    pub whitespace: String,
}
impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            context: 3,
            algorithm: "histogram".into(),
            whitespace: "show".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompareConfig {
    pub mode: String,
    pub preferred_base: Option<String>,
}
impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            mode: "merge-base".into(),
            preferred_base: None,
        }
    }
}
impl CompareConfig {
    pub fn mode(&self) -> ComparisonMode {
        if self.mode == "exact" {
            ComparisonMode::Exact
        } else {
            ComparisonMode::MergeBase
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsConfig {
    pub history_page: usize,
    pub patch_bytes: usize,
    pub blob_bytes: usize,
    pub snapshot_items: usize,
}
impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            history_page: 256,
            patch_bytes: 16 * 1024 * 1024,
            blob_bytes: 8 * 1024 * 1024,
            snapshot_items: 256,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub accent: String,
    pub muted: String,
    pub added: String,
    pub removed: String,
    pub warning: String,
    pub error: String,
    pub selection_fg: String,
    pub selection_bg: String,
}
impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent: "cyan".into(),
            muted: "dark-gray".into(),
            added: "green".into(),
            removed: "red".into(),
            warning: "yellow".into(),
            error: "red".into(),
            selection_fg: "black".into(),
            selection_bg: "cyan".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub path: Option<PathBuf>,
    pub bindings: KeyBindings,
}

#[derive(Debug, Clone, Default)]
pub struct KeyBindings {
    by_key: HashMap<KeySpec, Action>,
    overridden: HashSet<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct KeySpec {
    code: KeyCodeSpec,
    modifiers: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KeyCodeSpec {
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
        default.filter(|action| !self.overridden.contains(action_name(action)))
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
        if self.overridden.contains(action_name(action)) {
            self.by_key
                .iter()
                .find_map(|(key, candidate)| (candidate == action).then(|| key.label()))
        } else if self.by_key.contains_key(&default_key) {
            None
        } else {
            Some(default_label.into())
        }
    }

    fn from_config(values: &BTreeMap<String, String>) -> Result<Self, String> {
        let mut by_key = HashMap::new();
        let mut overridden = HashSet::new();
        for (requested_action, key_name) in values {
            let action = parse_action(requested_action)
                .ok_or_else(|| format!("unknown semantic action `{requested_action}` in [keys]"))?;
            let key = parse_key(key_name).ok_or_else(|| {
                format!("invalid key `{key_name}` for action `{requested_action}`")
            })?;
            if let Some(previous) = by_key.insert(key, action.clone()) {
                return Err(format!(
                    "key `{key_name}` conflicts with another override ({previous:?})"
                ));
            }
            overridden.insert(action_name(&action));
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
        let code = match event.code {
            KeyCode::Char(v) => KeyCodeSpec::Char(v),
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
        if event.modifiers.contains(KeyModifiers::SHIFT) {
            modifiers |= 4
        };
        Some(Self { code, modifiers })
    }
}
fn parse_key(input: &str) -> Option<KeySpec> {
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
        _ if key_name.chars().count() == 1 => KeyCodeSpec::Char(key_name.chars().next()?),
        _ => return None,
    };
    Some(KeySpec { code, modifiers })
}
fn action_name(action: &Action) -> &'static str {
    match action {
        Action::Move(1) => "move-down",
        Action::Move(_) => "move-up",
        Action::Page(1) => "page-down",
        Action::Page(_) => "page-up",
        Action::First => "first",
        Action::Last => "last",
        Action::Open => "open",
        Action::Back => "back",
        Action::Quit => "quit",
        Action::TogglePreview => "toggle-preview",
        Action::ToggleFocus => "toggle-focus",
        Action::StartSearch => "search",
        Action::StartPalette => "palette",
        Action::ToggleHelp => "help",
        Action::NextMatch => "next-match",
        Action::PreviousMatch => "previous-match",
        Action::NextHunk(1) => "next-hunk",
        Action::NextHunk(_) => "previous-hunk",
        Action::NextFile(1) => "next-file",
        Action::NextFile(_) => "previous-file",
        Action::NextParent => "next-parent",
        Action::ViewLog => "view-log",
        Action::ViewRefs => "view-refs",
        Action::ViewStatus => "view-status",
        Action::ViewTree => "view-tree",
        Action::ViewBlame => "view-blame",
        Action::ViewStash => "view-stash",
        Action::Mark => "mark",
        Action::StartCompare => "compare",
        Action::SwapCompare => "swap-compare",
        Action::ToggleCompareMode => "toggle-compare-mode",
        Action::ToggleStatusDiff => "toggle-status-diff",
        Action::Ascend => "ascend",
        Action::CopySelection => "copy-selection",
        Action::Redraw => "redraw",
        _ => "internal",
    }
}

fn parse_action(value: &str) -> Option<Action> {
    Some(match value {
        "move-down" => Action::Move(1),
        "move-up" => Action::Move(-1),
        "page-down" => Action::Page(1),
        "page-up" => Action::Page(-1),
        "first" => Action::First,
        "last" => Action::Last,
        "open" => Action::Open,
        "back" => Action::Back,
        "quit" => Action::Quit,
        "toggle-preview" => Action::TogglePreview,
        "toggle-focus" => Action::ToggleFocus,
        "search" => Action::StartSearch,
        "palette" => Action::StartPalette,
        "help" => Action::ToggleHelp,
        "next-match" => Action::NextMatch,
        "previous-match" => Action::PreviousMatch,
        "next-hunk" => Action::NextHunk(1),
        "previous-hunk" => Action::NextHunk(-1),
        "next-file" => Action::NextFile(1),
        "previous-file" => Action::NextFile(-1),
        "next-parent" => Action::NextParent,
        "view-log" => Action::ViewLog,
        "view-refs" => Action::ViewRefs,
        "view-status" => Action::ViewStatus,
        "view-tree" => Action::ViewTree,
        "view-blame" => Action::ViewBlame,
        "view-stash" => Action::ViewStash,
        "mark" => Action::Mark,
        "compare" => Action::StartCompare,
        "swap-compare" => Action::SwapCompare,
        "toggle-compare-mode" => Action::ToggleCompareMode,
        "toggle-status-diff" => Action::ToggleStatusDiff,
        "ascend" => Action::Ascend,
        "copy-selection" => Action::CopySelection,
        "redraw" => Action::Redraw,
        _ => return None,
    })
}

pub fn default_path() -> Result<PathBuf, ConfigError> {
    if let Some(base) = env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("phig/config.toml"));
    }
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|home| home.join(".config/phig/config.toml"))
        .ok_or(ConfigError::NoConfigDirectory)
}

pub fn load(explicit: Option<&Path>, disabled: bool) -> Result<LoadedConfig, ConfigError> {
    if disabled {
        return Ok(LoadedConfig {
            config: Config::default(),
            path: None,
            bindings: KeyBindings::default(),
        });
    }
    let path = explicit
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_path)?;
    if !path.exists() {
        if explicit.is_some() {
            return Err(ConfigError::Missing { path });
        }
        return Ok(LoadedConfig {
            config: Config::default(),
            path: Some(path),
            bindings: KeyBindings::default(),
        });
    }
    let text = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
        path: path.clone(),
        source,
    })?;
    let config: Config = toml::from_str(&text).map_err(|error| ConfigError::Parse {
        path: path.clone(),
        message: error.to_string(),
    })?;
    validate(&config, &path)?;
    let bindings =
        KeyBindings::from_config(&config.keys).map_err(|message| ConfigError::Validate {
            path: path.clone(),
            message,
        })?;
    Ok(LoadedConfig {
        config,
        path: Some(path),
        bindings,
    })
}

pub fn validate(config: &Config, path: &Path) -> Result<(), ConfigError> {
    let bad = |message: String| ConfigError::Validate {
        path: path.to_path_buf(),
        message,
    };
    if config.version != CONFIG_VERSION {
        return Err(bad(format!(
            "unsupported version {}; expected {CONFIG_VERSION}",
            config.version
        )));
    }
    if !["relative", "local", "iso", "unix"].contains(&config.ui.date.as_str()) {
        return Err(bad("ui.date must be relative, local, iso, or unix".into()));
    }
    if !["auto", "always", "never"].contains(&config.ui.color.as_str()) {
        return Err(bad("ui.color must be auto, always, or never".into()));
    }
    if !["off", "osc52"].contains(&config.ui.clipboard.as_str()) {
        return Err(bad("ui.clipboard must be off or osc52".into()));
    }
    if !(0..=999).contains(&config.diff.context) {
        return Err(bad("diff.context must be between 0 and 999".into()));
    }
    if !["myers", "minimal", "patience", "histogram"].contains(&config.diff.algorithm.as_str()) {
        return Err(bad(
            "diff.algorithm must be myers, minimal, patience, or histogram".into(),
        ));
    }
    if !["show", "ignore-all", "ignore-space-change", "ignore-eol"]
        .contains(&config.diff.whitespace.as_str())
    {
        return Err(bad("diff.whitespace has an unsupported value".into()));
    }
    if !["merge-base", "exact"].contains(&config.compare.mode.as_str()) {
        return Err(bad("compare.mode must be merge-base or exact".into()));
    }
    if config.limits.history_page == 0
        || config.limits.history_page > 4096
        || config.limits.snapshot_items == 0
        || config.limits.snapshot_items > 4096
        || config.limits.patch_bytes == 0
        || config.limits.blob_bytes == 0
    {
        return Err(bad(
            "limits must be positive; item/page limits may not exceed 4096".into(),
        ));
    }
    for (name, value) in [
        ("accent", &config.theme.accent),
        ("muted", &config.theme.muted),
        ("added", &config.theme.added),
        ("removed", &config.theme.removed),
        ("warning", &config.theme.warning),
        ("error", &config.theme.error),
        ("selection-fg", &config.theme.selection_fg),
        ("selection-bg", &config.theme.selection_bg),
    ] {
        if parse_color(value).is_none() {
            return Err(bad(format!("theme.{name} has invalid color `{value}`")));
        }
    }
    KeyBindings::from_config(&config.keys).map_err(bad)?;
    Ok(())
}

pub fn parse_color(value: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color;
    Some(match value.to_ascii_lowercase().as_str() {
        "reset" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "dark-gray" | "dark-grey" => Color::DarkGray,
        "light-red" => Color::LightRed,
        "light-green" => Color::LightGreen,
        "light-yellow" => Color::LightYellow,
        "light-blue" => Color::LightBlue,
        "light-magenta" => Color::LightMagenta,
        "light-cyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return None,
    })
}

pub fn init(path: &Path, force: bool) -> Result<(), ConfigError> {
    if path.exists() && !force {
        return Err(ConfigError::Validate {
            path: path.to_path_buf(),
            message: "file already exists; use --force to replace it".into(),
        });
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?
    }
    fs::write(path, include_str!("../assets/config.example.toml")).map_err(|source| {
        ConfigError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recursively_rejects_unknown_keys_and_conflicts() {
        let e = toml::from_str::<Config>("version=1\n[ui]\nwat=true")
            .unwrap_err()
            .to_string();
        assert!(e.contains("unknown field"));
        assert!(toml::from_str::<Config>("[ui]\npreview=true").is_err());
        let mut c = Config::default();
        c.keys.insert("open".into(), "x".into());
        c.keys.insert("quit".into(), "x".into());
        assert!(
            validate(&c, Path::new("x"))
                .unwrap_err()
                .to_string()
                .contains("conflicts")
        );
    }
    #[test]
    fn selection_labels_follow_effective_semantic_bindings() {
        assert_eq!(
            KeyBindings::default().selection_key_labels(),
            ("Enter".into(), "Esc/q".into())
        );

        let mut keys = BTreeMap::new();
        keys.insert("quit".into(), "x".into());
        assert_eq!(
            KeyBindings::from_config(&keys)
                .unwrap()
                .selection_key_labels(),
            ("Enter".into(), "Esc/x".into())
        );

        keys.clear();
        keys.insert("open".into(), "q".into());
        assert_eq!(
            KeyBindings::from_config(&keys)
                .unwrap()
                .selection_key_labels(),
            ("q".into(), "Esc".into()),
            "an override that claims q must remove q from the cancel hint"
        );
    }

    #[test]
    fn semantic_override_resolves() {
        let mut c = Config::default();
        c.keys.insert("open".into(), "ctrl+x".into());
        c.keys.insert("help".into(), "h".into());
        let b = KeyBindings::from_config(&c.keys).unwrap();
        assert_eq!(
            b.resolve(
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
                None,
            ),
            Some(Action::Open)
        );
        assert_eq!(
            b.resolve(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                Some(Action::Open)
            ),
            None,
            "an override must disable every old default for that semantic action"
        );
        assert_eq!(
            b.resolve(
                KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
                Some(Action::ToggleHelp),
            ),
            None,
            "remapping help must disable its old question-mark key"
        );
        for invalid in ["wat+x", "ctrl+wat+x", "ctrl+ctrl+x", "ctrl+", "+x"] {
            assert!(parse_key(invalid).is_none(), "accepted {invalid}");
        }
    }
}
