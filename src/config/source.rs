//! XDG discovery, precedence, decoding, validation, and initialization.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    keys::KeyBindings,
    model::{CONFIG_VERSION, Config},
};

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

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub path: Option<PathBuf>,
    pub bindings: KeyBindings,
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
    if !["auto", "unicode", "ascii"].contains(&config.ui.glyphs.as_str()) {
        return Err(bad("ui.glyphs must be auto, unicode, or ascii".into()));
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
    fs::write(path, include_str!("../../assets/config.example.toml")).map_err(|source| {
        ConfigError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}
