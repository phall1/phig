//! ratcn scene and paint modules. Git state and semantic actions stay in `app`.

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
mod list;
mod picker;
mod scene;
mod theme;

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Text},
    widgets::Paragraph,
};

use crate::app::{App, Overlay, View};

pub(crate) use layout::{diff_split_available, page_rows, preview_focus_available};
use scene::Canvas;
pub(super) use scene::{Input, Renderer};
pub(crate) use theme::legacy_config;
pub use theme::{
    ColorMode, DateMode, GlyphMode, RenderConfig, RenderContext, RenderTheme, set_color_mode,
    set_date_mode, set_theme,
};

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let context = theme::legacy_context();
    render_with_context(frame, app, &context);
}

fn render_divider(frame: &mut Canvas<'_>, layout: layout::PaneLayout, context: &RenderContext) {
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

/// A quiet status message centered in `area`, for loading, empty, and
/// unavailable states, so every surface reports them the same way.
fn render_notice(frame: &mut Canvas<'_>, area: Rect, text: &str, context: &RenderContext) {
    if area.is_empty() {
        return;
    }
    let rows = format::wrap_words(text, usize::from(area.width));
    let height = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let y = area.y + area.height.saturating_sub(height) / 2;
    frame.render_widget(
        Paragraph::new(rows.into_iter().map(Line::from).collect::<Vec<_>>())
            .alignment(ratatui::layout::Alignment::Center)
            .style(context.style(context.muted())),
        Rect::new(area.x, y, area.width, height),
    );
}

pub fn render_with_context(frame: &mut Frame<'_>, app: &App, context: &RenderContext) {
    Renderer::new(context.clone()).draw(frame, app);
}

fn render_body(
    frame: &mut Canvas<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
    history: &mut graph::GraphCache,
) {
    if area.is_empty() {
        return;
    }
    if let Overlay::DiffTree(tree) = &app.overlay {
        diff_tree::render(frame, app, tree, area, context);
    } else if app.diff_fullscreen {
        fullscreen::render(frame, app, area, context);
    } else {
        match app.view {
            View::Log => history::render_log(frame, app, area, context, history),
            View::Detail => diff::render_detail(frame, app, area, context),
            View::Compare => inspect::render_compare(frame, app, area, context),
            View::Refs => inspect::render_refs(frame, app, area, context),
            View::Status => inspect::render_status(frame, app, area, context),
            View::StatusDiff => inspect::render_status_diff(frame, app, area, context),
            View::Tree => inspect::render_tree(frame, app, area, context),
            View::Blob => inspect::render_blob(frame, app, area, context),
            View::Blame => inspect::render_blame(frame, app, area, context),
            View::Stash => inspect::render_stashes(frame, app, area, context),
        }
    }
}

fn render_overlay(
    frame: &mut Canvas<'_>,
    app: &App,
    area: Rect,
    body: Rect,
    context: &RenderContext,
) {
    match &app.overlay {
        Overlay::Help => chrome::render_help(frame, app, area, context),
        Overlay::Search { draft, .. } if !body.is_empty() => {
            chrome::render_search(frame, app, draft, body, context);
        }
        Overlay::Palette { draft, selected } => {
            chrome::render_palette(frame, app, draft, *selected, area, context)
        }
        Overlay::FilePicker {
            draft, selected, ..
        } => chrome::render_file_picker(frame, app, draft, *selected, area, context),
        Overlay::None if app.has_errors() && !body.is_empty() => {
            chrome::render_errors(frame, app, body, context);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
