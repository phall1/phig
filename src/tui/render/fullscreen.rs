//! An expanded patch with a sticky file/hunk location and the original view intact.

use super::Canvas;
use ratatui::{
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
    layout::can_split_patch,
    theme::RenderContext,
};

pub(super) fn render(frame: &mut Canvas<'_>, app: &App, area: Rect, context: &RenderContext) {
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
        super::render_notice(frame, area, &message, context);
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
    let style = if !can_split_patch(area.width) {
        ""
    } else if app.diff_split {
        "split  "
    } else {
        "unified  "
    };
    let prefix = format!("DIFF {id}{style}");
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
    render_diff_value(frame, app, parts[1], true, context);
}

fn location(diff: &Diff, scroll: usize, width: usize, context: &RenderContext) -> String {
    let Some((index, file)) = diff
        .file_at(scroll)
        .or_else(|| diff.files.first().map(|file| (0, file)))
    else {
        return "Patch".into();
    };
    let path = file.path().map_or("patch", |path| path.display.as_str());
    let separator = context.glyphs().separator;
    let mut ordinal = format!("  {separator}  file {}/{}", index + 1, diff.files.len());
    // Above the first hunk there is no current hunk to count.
    if let Some((hunk, _)) = file.hunk_at(scroll) {
        ordinal.push_str(&format!(
            "  {separator}  hunk {}/{}",
            hunk + 1,
            file.hunks.len()
        ));
    }
    let path_width = width.saturating_sub(display_width(&ordinal));
    format!(
        "{}{ordinal}",
        truncate_with(path, path_width, context.glyphs().ellipsis)
    )
}
