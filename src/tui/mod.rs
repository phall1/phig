//! Terminal adapter façade.

mod driver;
mod effects;
mod input;
mod render;
mod session;
#[cfg(unix)]
mod signals;

use std::io;

use thiserror::Error;

use crate::runtime::CoordinatorError;

pub use driver::{
    TuiOptions, run, run_configured, run_select, run_select_with_options, run_with_options,
};
pub use input::handle_help_key;
pub use render::{
    ColorMode, DateMode, GlyphMode, RenderConfig, RenderContext, RenderTheme, render,
    render_with_context, set_color_mode, set_date_mode, set_theme,
};
pub use session::TerminalSession;

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("terminal error: {0}")]
    Terminal(#[from] io::Error),
    #[error("Git worker error: {0}")]
    Coordinator(#[from] CoordinatorError),
    #[error("selection requires a controlling terminal: {0}")]
    NoControllingTerminal(String),
    #[error("terminated by signal {0}")]
    Terminated(i32),
}

#[cfg(test)]
mod tests;
