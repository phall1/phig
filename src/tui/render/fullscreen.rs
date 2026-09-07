//! An expanded patch with a sticky file/hunk location and the original view intact.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::{App, View},
    domain::Diff,
};

use super::{
    diff::render_diff_value,
    format::{display_width, truncate_with},
    inspect::render_compare,
    theme::RenderContext,
};

pub(super) fn render(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    // Comparison endpoints and merge-base semantics remain visible when expanded.
    if app.view == View::Compare {
        render_compare(frame, app, area, context);
        return;
    }
    let Some(diff) = app.active_diff() else {
        let message = if app.preview_loading || app.inspect.loading {
            format!("Loading diff{}", context.glyphs().ellipsis)
        } else {
            "No diff available".into()
        };
        frame.render_widget(Paragraph::new(message), area);
        return;
    };
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let id = match app.view {
        View::Status | View::StatusDiff => {
            if app.inspect.status_diff_staged {
                "staged  ".into()
            } else {
                "unstaged  ".into()
            }
        }
        _ => app.preview.as_ref().map_or_else(String::new, |detail| {
            format!("{}  ", detail.commit.id.short(10))
        }),
    };
    let prefix = format!("DIFF {id}");
    let location_width = usize::from(area.width).saturating_sub(display_width(&prefix));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix, context.style(context.muted())),
            Span::styled(
                location(diff, app.diff_scroll, location_width, context),
                context.strong(context.accent()),
            ),
        ])),
        parts[0],
    );
    render_diff_value(frame, diff, app.diff_scroll, parts[1], true, context);
}

fn location(diff: &Diff, scroll: usize, width: usize, context: &RenderContext) -> String {
    let index = diff
        .files
        .partition_point(|file| file.header_line <= scroll)
        .saturating_sub(1);
    let Some(file) = diff.files.get(index) else {
        return "Patch".into();
    };
    let path = file
        .new_path
        .as_ref()
        .or(file.old_path.as_ref())
        .map_or("patch", |path| path.display.as_str());
    let hunk = file
        .hunks
        .partition_point(|hunk| hunk.header_line <= scroll);
    let separator = context.glyphs().separator;
    let mut ordinal = format!("  {separator}  file {}/{}", index + 1, diff.files.len());
    if !file.hunks.is_empty() {
        ordinal.push_str(&format!("  {separator}  hunk {hunk}/{}", file.hunks.len()));
    }
    let path_width = width.saturating_sub(display_width(&ordinal));
    format!(
        "{}{ordinal}",
        truncate_with(path, path_width, context.glyphs().ellipsis)
    )
}
