//! Shared patch rendering for detail and inspection views.

use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Paragraph};

use crate::{
    app::{App, Focus, View},
    domain::{Diff, DiffLine, DiffLineKind},
};

use super::{history::render_preview, layout::diff_content_rows, theme::RenderContext};

pub(super) fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    render_preview(frame, app, area, context);
}

pub(super) fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let Some(detail) = &app.preview else {
        return;
    };
    render_diff_value(
        frame,
        &detail.diff,
        app.diff_scroll,
        area,
        app.focus == Focus::Preview || app.view == View::Detail,
        context,
    );
}

pub(super) fn render_diff_value(
    frame: &mut Frame<'_>,
    diff: &Diff,
    scroll: usize,
    area: Rect,
    active: bool,
    context: &RenderContext,
) {
    let visible = usize::from(diff_content_rows(area.height, diff.truncated));
    let lines: Vec<Line<'_>> = diff
        .lines
        .iter()
        .skip(scroll)
        .take(visible)
        .map(|line| diff_line(line, context))
        .collect();
    let style = if active || context.is_monochrome() {
        Style::reset()
    } else {
        context.style(context.muted())
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
    if diff.truncated && area.height > 1 {
        let warning_area = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        frame.render_widget(
            Paragraph::new("diff truncated at configured limit")
                .style(context.style(context.warning())),
            warning_area,
        );
    }
}

fn diff_line<'a>(line: &'a DiffLine, context: &RenderContext) -> Line<'a> {
    let style = match line.kind {
        DiffLineKind::Added => context.style(context.added()),
        DiffLineKind::Removed => context.style(context.removed()),
        DiffLineKind::HunkHeader => context.strong(context.accent()),
        DiffLineKind::FileHeader => context.strong(context.warning()),
        DiffLineKind::Context => Style::reset(),
        DiffLineKind::Metadata => context.style(context.muted()),
    };
    Line::styled(line.text.as_str(), style)
}
