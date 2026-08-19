//! Strict XDG configuration, semantic key bindings, and validated display policy.
//!
//! Persisted models, compiled key bindings, and configuration source handling
//! remain separate while this façade preserves the public `phig_cli::config`
//! API.

mod keys;
mod model;
mod source;

pub use keys::KeyBindings;
pub use model::{
    CONFIG_VERSION, CompareConfig, Config, DiffConfig, LimitsConfig, ThemeConfig, UiConfig,
};
pub use source::{ConfigError, LoadedConfig, default_path, init, load, parse_color, validate};

#[cfg(test)]
mod tests;
