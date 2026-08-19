//! Read-only Ratatui renderer composed from layout, chrome, and view modules.

mod chrome;
mod diff;
mod format;
mod history;
mod inspect;
mod layout;
mod theme;

use std::sync::atomic::Ordering;

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
};

use crate::app::{App, Overlay, View};

use chrome::{
    render_errors, render_file_picker, render_footer, render_header, render_help, render_palette,
    render_search,
};
use diff::render_detail;
use history::render_log;
use inspect::{
    render_blame, render_blob, render_compare, render_refs, render_stashes, render_status,
    render_status_diff, render_tree,
};
pub(crate) use layout::{page_rows, preview_focus_available};
use theme::COLOR_MODE;
pub use theme::{RenderTheme, set_color_mode, set_date_mode, set_theme};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, rows[0]);
    match app.view {
        View::Log => render_log(frame, app, rows[1]),
        View::Detail => render_detail(frame, app, rows[1]),
        View::Compare => render_compare(frame, app, rows[1]),
        View::Refs => render_refs(frame, app, rows[1]),
        View::Status => render_status(frame, app, rows[1]),
        View::StatusDiff => render_status_diff(frame, app, rows[1]),
        View::Tree => render_tree(frame, app, rows[1]),
        View::Blob => render_blob(frame, app, rows[1]),
        View::Blame => render_blame(frame, app, rows[1]),
        View::Stash => render_stashes(frame, app, rows[1]),
    }
    render_footer(frame, app, rows[2]);

    if app.has_errors() && matches!(app.overlay, Overlay::None) {
        render_errors(frame, app, rows[1]);
    }
    match &app.overlay {
        Overlay::Help => render_help(frame, app, area),
        Overlay::Search { draft, .. } => render_search(frame, draft, rows[1]),
        Overlay::Palette { draft, selected } => render_palette(frame, draft, *selected, area),
        Overlay::FilePicker {
            draft, selected, ..
        } => render_file_picker(frame, app, draft, *selected, area),
        Overlay::None => {}
    }

    if COLOR_MODE.load(Ordering::SeqCst) < 0
        || (COLOR_MODE.load(Ordering::SeqCst) == 0 && std::env::var_os("NO_COLOR").is_some())
    {
        reset_styles(frame.buffer_mut(), area);
    }
}

fn reset_styles(buffer: &mut Buffer, area: Rect) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buffer[(x, y)].set_style(Style::reset());
        }
    }
}

#[cfg(test)]
mod tests;
