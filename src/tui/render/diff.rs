//! Shared patch rendering for detail and inspection views.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Paragraph,
};

use crate::{
    app::{App, Focus, View},
    domain::{Diff, DiffLine, DiffLineKind},
};

use super::{
    history::render_preview,
    theme::{accent, added, muted, removed, warning},
};

pub(super) fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    render_preview(frame, app, area);
}

pub(super) fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(detail) = &app.preview else {
        return;
    };
    render_diff_value(
        frame,
        &detail.diff,
        app.diff_scroll,
        area,
        app.focus == Focus::Preview || app.view == View::Detail,
    );
}

pub(super) fn render_diff_value(
    frame: &mut Frame<'_>,
    diff: &Diff,
    scroll: usize,
    area: Rect,
    active: bool,
) {
    let visible = usize::from(area.height);
    let lines: Vec<Line<'_>> = diff
        .lines
        .iter()
        .skip(scroll)
        .take(visible)
        .map(diff_line)
        .collect();
    let style = if active {
        Style::default()
    } else {
        Style::default().fg(Color::Gray)
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
    if diff.truncated && area.height > 0 {
        let warning_area = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        frame.render_widget(
            Paragraph::new("diff truncated at configured limit")
                .style(Style::default().fg(warning()).bg(Color::Black)),
            warning_area,
        );
    }
}

fn diff_line(line: &DiffLine) -> Line<'_> {
    let style = match line.kind {
        DiffLineKind::Added => Style::default().fg(added()),
        DiffLineKind::Removed => Style::default().fg(removed()),
        DiffLineKind::HunkHeader => Style::default().fg(accent()).bold(),
        DiffLineKind::FileHeader => Style::default().fg(warning()).bold(),
        DiffLineKind::Context => Style::default(),
        DiffLineKind::Metadata => Style::default().fg(muted()),
    };
    Line::styled(line.text.as_str(), style)
}
