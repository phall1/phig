//! Read-only Ratatui renderer composed from layout, chrome, and view modules.

mod chrome;
mod decorate;
mod diff;
mod diff_tree;
mod format;
mod fullscreen;
mod graph;
mod history;
mod inspect;
mod layout;
mod picker;
mod theme;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::app::{App, Overlay, View};

#[derive(Default)]
pub(crate) struct RenderState {
    history: graph::GraphCache,
}

impl RenderState {
    pub(crate) fn clear_history(&mut self) {
        self.history.clear();
    }
}

pub(crate) use layout::{diff_split_available, page_rows, preview_focus_available};
pub(crate) use theme::legacy_config;
pub use theme::{
    ColorMode, DateMode, GlyphMode, RenderConfig, RenderContext, RenderTheme, set_color_mode,
    set_date_mode, set_theme,
};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let context = theme::legacy_context();
    render_with_context(frame, app, &context);
}

fn render_divider(frame: &mut Frame<'_>, layout: layout::PaneLayout, context: &RenderContext) {
    let Some(area) = layout.divider else {
        return;
    };
    let glyphs = context.glyphs();
    let text = match layout.direction {
        Some(layout::SplitDirection::Vertical) => Text::from(
            (0..area.height)
                .map(|_| Line::from(glyphs.vertical))
                .collect::<Vec<_>>(),
        ),
        Some(layout::SplitDirection::Horizontal) => {
            Text::from(glyphs.horizontal.repeat(usize::from(area.width)))
        }
        None => return,
    };
    frame.render_widget(
        Paragraph::new(text).style(context.style(context.muted())),
        area,
    );
}

pub fn render_with_context(frame: &mut Frame<'_>, app: &App, context: &RenderContext) {
    render_with_state(frame, app, context, &mut RenderState::default());
}

pub(crate) fn render_with_state(
    frame: &mut Frame<'_>,
    app: &App,
    context: &RenderContext,
    state: &mut RenderState,
) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    chrome::render_header(frame, app, rows[0], context);
    if let Overlay::DiffTree(tree) = &app.overlay {
        diff_tree::render(frame, app, tree, rows[1], context);
    } else if app.diff_fullscreen {
        fullscreen::render(frame, app, rows[1], context);
    } else {
        match app.view {
            View::Log => history::render_log(frame, app, rows[1], context, &mut state.history),
            View::Detail => diff::render_detail(frame, app, rows[1], context),
            View::Compare => inspect::render_compare(frame, app, rows[1], context),
            View::Refs => inspect::render_refs(frame, app, rows[1], context),
            View::Status => inspect::render_status(frame, app, rows[1], context),
            View::StatusDiff => inspect::render_status_diff(frame, app, rows[1], context),
            View::Tree => inspect::render_tree(frame, app, rows[1], context),
            View::Blob => inspect::render_blob(frame, app, rows[1], context),
            View::Blame => inspect::render_blame(frame, app, rows[1], context),
            View::Stash => inspect::render_stashes(frame, app, rows[1], context),
        }
    }
    chrome::render_footer(frame, app, rows[2], context);

    if app.has_errors() && matches!(app.overlay, Overlay::None) {
        chrome::render_errors(frame, app, rows[1], context);
    }
    match &app.overlay {
        Overlay::Help => chrome::render_help(frame, app, area, context),
        Overlay::Search { draft, .. } => chrome::render_search(frame, draft, rows[1], context),
        Overlay::Palette { draft, selected } => {
            chrome::render_palette(frame, draft, *selected, area, context)
        }
        Overlay::FilePicker {
            draft, selected, ..
        } => chrome::render_file_picker(frame, app, draft, *selected, area, context),
        Overlay::None | Overlay::DiffTree(_) => {}
    }
}

#[cfg(test)]
mod tests;
