//! Immutable rendering policy and compatibility defaults.

use std::sync::{LazyLock, RwLock};

use ratatui::{
    style::{Color, Style},
    symbols::border,
};

use crate::{app::Action, config::KeyBindings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderTheme {
    pub accent: Color,
    pub muted: Color,
    pub added: Color,
    pub removed: Color,
    pub warning: Color,
    pub error: Color,
    pub selection_fg: Color,
    pub selection_bg: Color,
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan,
            muted: Color::DarkGray,
            added: Color::Green,
            removed: Color::Red,
            warning: Color::Yellow,
            error: Color::Red,
            selection_fg: Color::Cyan,
            selection_bg: Color::Reset,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateMode {
    Relative,
    Local,
    Iso,
    Unix,
}

impl DateMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "relative" => Some(Self::Relative),
            "local" => Some(Self::Local),
            "iso" => Some(Self::Iso),
            "unix" => Some(Self::Unix),
            _ => None,
        }
    }
}

impl Default for DateMode {
    fn default() -> Self {
        Self::Relative
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

impl Default for ColorMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphMode {
    Auto,
    Unicode,
    Ascii,
}

impl GlyphMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "unicode" => Some(Self::Unicode),
            "ascii" => Some(Self::Ascii),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub theme: RenderTheme,
    pub date_mode: DateMode,
    pub color_mode: ColorMode,
    pub glyph_mode: GlyphMode,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            theme: RenderTheme::default(),
            date_mode: DateMode::default(),
            color_mode: ColorMode::default(),
            glyph_mode: GlyphMode::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphSet {
    pub selected: &'static str,
    pub marked: &'static str,
    pub commit: char,
    pub merge: char,
    pub lane: char,
    pub vertical: &'static str,
    pub horizontal: &'static str,
    pub separator: &'static str,
    pub arrow: &'static str,
    pub ellipsis: &'static str,
    pub up_down: &'static str,
    pub dash: &'static str,
    border: border::Set<'static>,
}

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

impl GlyphSet {
    fn unicode() -> Self {
        Self {
            selected: "› ",
            marked: "◆ ",
            commit: '●',
            merge: '◆',
            lane: '│',
            vertical: "│",
            horizontal: "─",
            separator: "·",
            arrow: "→",
            ellipsis: "…",
            up_down: "↑/↓",
            dash: "—",
            border: border::ROUNDED,
        }
    }

    fn ascii() -> Self {
        Self {
            selected: "> ",
            marked: "* ",
            commit: 'o',
            merge: '*',
            lane: '|',
            vertical: "|",
            horizontal: "-",
            separator: "|",
            arrow: "->",
            ellipsis: "...",
            up_down: "up/down",
            dash: "-",
            border: ASCII_BORDER,
        }
    }

    pub fn border(self) -> border::Set<'static> {
        self.border
    }
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    config: RenderConfig,
    glyphs: GlyphSet,
    bindings: KeyBindings,
    monochrome: bool,
}

fn resolve_monochrome(color_mode: ColorMode, no_color: bool) -> bool {
    match color_mode {
        ColorMode::Never => true,
        ColorMode::Always => false,
        ColorMode::Auto => no_color,
    }
}

fn resolve_ascii(glyph_mode: GlyphMode, dumb_terminal: bool) -> bool {
    match glyph_mode {
        GlyphMode::Ascii => true,
        GlyphMode::Unicode => false,
        GlyphMode::Auto => dumb_terminal,
    }
}

impl RenderContext {
    pub fn new(config: RenderConfig) -> Self {
        Self::with_bindings(config, KeyBindings::default())
    }

    pub fn with_bindings(config: RenderConfig, bindings: KeyBindings) -> Self {
        let monochrome =
            resolve_monochrome(config.color_mode, std::env::var_os("NO_COLOR").is_some());
        let ascii = resolve_ascii(
            config.glyph_mode,
            std::env::var("TERM").is_ok_and(|term| term == "dumb"),
        );
        Self {
            config,
            glyphs: if ascii {
                GlyphSet::ascii()
            } else {
                GlyphSet::unicode()
            },
            bindings,
            monochrome,
        }
    }

    pub fn config(&self) -> &RenderConfig {
        &self.config
    }

    pub fn glyphs(&self) -> GlyphSet {
        self.glyphs
    }

    pub fn is_monochrome(&self) -> bool {
        self.monochrome
    }

    pub fn accent(&self) -> Color {
        self.color(self.config.theme.accent)
    }
    pub fn muted(&self) -> Color {
        self.color(self.config.theme.muted)
    }
    pub fn added(&self) -> Color {
        self.color(self.config.theme.added)
    }
    pub fn removed(&self) -> Color {
        self.color(self.config.theme.removed)
    }
    pub fn warning(&self) -> Color {
        self.color(self.config.theme.warning)
    }
    pub fn error(&self) -> Color {
        self.color(self.config.theme.error)
    }

    pub fn style(&self, color: Color) -> Style {
        if self.monochrome {
            Style::reset()
        } else {
            Style::default().fg(color)
        }
    }

    pub fn strong(&self, color: Color) -> Style {
        if self.monochrome {
            Style::reset()
        } else {
            Style::default().fg(color).bold()
        }
    }

    pub fn selection_style(&self, active: bool) -> Style {
        if self.monochrome {
            return Style::reset();
        }
        if !active {
            return Style::default().fg(self.config.theme.muted);
        }
        let mut style = Style::default().fg(self.config.theme.selection_fg).bold();
        if self.config.theme.selection_bg != Color::Reset {
            style = style.bg(self.config.theme.selection_bg);
        }
        style
    }

    pub fn key(&self, action: &Action) -> String {
        self.bindings.action_key_label(action)
    }

    fn color(&self, color: Color) -> Color {
        if self.monochrome { Color::Reset } else { color }
    }
}

static LEGACY_CONFIG: LazyLock<RwLock<RenderConfig>> =
    LazyLock::new(|| RwLock::new(RenderConfig::default()));

pub fn set_date_mode(mode: &str) {
    LEGACY_CONFIG.write().expect("render config lock").date_mode =
        DateMode::parse(mode).unwrap_or_default();
}

pub fn set_color_mode(mode: &str) {
    LEGACY_CONFIG
        .write()
        .expect("render config lock")
        .color_mode = ColorMode::parse(mode).unwrap_or_default();
}

pub fn set_theme(theme: RenderTheme) {
    LEGACY_CONFIG.write().expect("render config lock").theme = theme;
}

pub(crate) fn legacy_config() -> RenderConfig {
    LEGACY_CONFIG.read().expect("render config lock").clone()
}

pub(super) fn legacy_context() -> RenderContext {
    RenderContext::new(legacy_config())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_color_and_glyph_policies_override_environment_signals() {
        assert!(resolve_monochrome(ColorMode::Auto, true));
        assert!(!resolve_monochrome(ColorMode::Always, true));
        assert!(resolve_monochrome(ColorMode::Never, false));
        assert!(resolve_ascii(GlyphMode::Auto, true));
        assert!(!resolve_ascii(GlyphMode::Unicode, true));
        assert!(resolve_ascii(GlyphMode::Ascii, false));
        assert_eq!(DateMode::parse("iso"), Some(DateMode::Iso));
        assert_eq!(ColorMode::parse("never"), Some(ColorMode::Never));
        assert_eq!(DateMode::parse("surprise"), None);
        assert_eq!(ColorMode::parse("surprise"), None);
    }
}
