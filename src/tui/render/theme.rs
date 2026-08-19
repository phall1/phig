//! Process rendering preferences and semantic color roles.

use std::sync::{
    LazyLock, RwLock,
    atomic::{AtomicI8, Ordering},
};

use ratatui::style::Color;

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
            selection_fg: Color::Black,
            selection_bg: Color::Cyan,
        }
    }
}
static THEME: LazyLock<RwLock<RenderTheme>> = LazyLock::new(|| RwLock::new(RenderTheme::default()));
pub(super) static COLOR_MODE: AtomicI8 = AtomicI8::new(0);
pub(super) static DATE_MODE: LazyLock<RwLock<String>> =
    LazyLock::new(|| RwLock::new("relative".into()));
pub fn set_date_mode(mode: &str) {
    *DATE_MODE.write().expect("date mode lock") = mode.to_owned();
}
pub fn set_color_mode(mode: &str) {
    COLOR_MODE.store(
        match mode {
            "never" => -1,
            "always" => 1,
            _ => 0,
        },
        Ordering::SeqCst,
    );
}
pub fn set_theme(theme: RenderTheme) {
    *THEME.write().expect("theme lock") = theme;
}
pub(super) fn accent() -> Color {
    THEME.read().expect("theme lock").accent
}
pub(super) fn muted() -> Color {
    THEME.read().expect("theme lock").muted
}
pub(super) fn added() -> Color {
    THEME.read().expect("theme lock").added
}
pub(super) fn removed() -> Color {
    THEME.read().expect("theme lock").removed
}
pub(super) fn warning() -> Color {
    THEME.read().expect("theme lock").warning
}
pub(super) fn error_color() -> Color {
    THEME.read().expect("theme lock").error
}
pub(super) fn selection_fg() -> Color {
    THEME.read().expect("theme lock").selection_fg
}
pub(super) fn selection_bg() -> Color {
    THEME.read().expect("theme lock").selection_bg
}
