//! Persisted TOML model and zero-configuration defaults.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::domain::ComparisonMode;

pub const CONFIG_VERSION: u32 = 1;

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
    pub glyphs: String,
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
            glyphs: "auto".into(),
            clipboard: "osc52".into(),
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
            selection_fg: "cyan".into(),
            selection_bg: "reset".into(),
        }
    }
}
