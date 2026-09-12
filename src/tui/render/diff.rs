//! Shared patch rendering for detail and inspection views.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::patch::{PatchIndex, PatchLine},
    app::{App, Focus, View},
    domain::{Diff, DiffLine, DiffLineKind},
};

use super::{history::render_preview, layout::diff_content_rows, theme::RenderContext};

pub(super) fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    render_preview(frame, app, area, context);
}

pub(super) fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    render_diff_value(
        frame,
        app,
        area,
        app.focus == Focus::Preview || app.view == View::Detail,
        context,
    );
}

pub(super) fn render_diff_value(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    active: bool,
    context: &RenderContext,
) {
    let Some(diff) = app.active_diff() else {
        return;
    };
    if area.is_empty() {
        return;
    }
    let content = Rect::new(
        area.x,
        area.y,
        area.width,
        diff_content_rows(area.height, diff.truncated),
    );
    if app.diff_split && area.width >= 100 {
        render_split(frame, app, content, context);
    } else {
        render_unified(frame, app, content, active, context);
    }
    if diff.truncated && area.height > 1 {
        let warning_area = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        frame.render_widget(
            Paragraph::new("diff truncated at configured limit")
                .style(context.style(context.warning())),
            warning_area,
        );
    }
}

fn render_unified(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    active: bool,
    context: &RenderContext,
) {
    let Some(diff) = app.active_diff() else {
        return;
    };
    let index = app.patch_index();
    let lines: Vec<Line<'_>> = diff
        .lines
        .iter()
        .enumerate()
        .skip(app.diff_scroll)
        .take(usize::from(area.height))
        .map(|(i, line)| {
            let source = index.lines.get(i);
            let pair = source
                .and_then(|source| source.pair)
                .and_then(|pair| diff.lines.get(pair));
            diff_line(line, source, pair, context)
        })
        .collect();
    let style = if active || context.is_monochrome() {
        Style::reset()
    } else {
        context.style(context.muted())
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
}

fn diff_line<'a>(
    line: &'a DiffLine,
    source: Option<&PatchLine>,
    pair: Option<&DiffLine>,
    context: &RenderContext,
) -> Line<'a> {
    let style = line_style(line, context);
    let Some(source) = source.filter(|source| source.old.is_some() || source.new.is_some()) else {
        return Line::styled(line.text.as_str(), style);
    };
    let mut spans = vec![Span::styled(
        format!(
            "{:>4} {:>4} {} ",
            number(source.old),
            number(source.new),
            context.glyphs().vertical
        ),
        context.style(context.muted()),
    )];
    spans.extend(content_spans(line, pair, style));
    Line::from(spans)
}

fn number(value: Option<usize>) -> String {
    value.map_or_else(String::new, |n| n.to_string())
}

fn line_style(line: &DiffLine, context: &RenderContext) -> Style {
    match line.kind {
        DiffLineKind::Added => context.style(context.added()),
        DiffLineKind::Removed => context.style(context.removed()),
        DiffLineKind::HunkHeader => context.strong(context.accent()),
        DiffLineKind::FileHeader => context.strong(context.warning()),
        DiffLineKind::Context => Style::reset(),
        DiffLineKind::Metadata => context.style(context.muted()),
    }
}

fn content_spans<'a>(line: &'a DiffLine, pair: Option<&DiffLine>, style: Style) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    for range in changed_words(line, pair) {
        let Some(text) = line.text.get(range.clone()) else {
            continue;
        };
        spans.push(Span::styled(&line.text[cursor..range.start], style));
        spans.push(Span::styled(text, style.bold().underlined()));
        cursor = range.end;
    }
    spans.push(Span::styled(&line.text[cursor..], style));
    spans
}

fn render_split(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let Some(diff) = app.active_diff() else {
        return;
    };
    let index = app.patch_index();
    let start = index
        .split_positions
        .get(app.diff_scroll)
        .copied()
        .unwrap_or(0);
    for (y, raw) in index
        .split_rows
        .iter()
        .skip(start)
        .take(usize::from(area.height))
        .enumerate()
    {
        let row = Rect::new(area.x, area.y + y as u16, area.width, 1);
        render_split_row(frame, diff, &index, *raw, row, context);
    }
}

fn render_split_row(
    frame: &mut Frame<'_>,
    diff: &Diff,
    index: &PatchIndex,
    raw: usize,
    row: Rect,
    context: &RenderContext,
) {
    let Some(line) = diff.lines.get(raw) else {
        return;
    };
    let source = &index.lines[raw];
    let pair = source.pair.and_then(|i| diff.lines.get(i));
    if source.old.is_none() && source.new.is_none() {
        frame.render_widget(
            Paragraph::new(Line::styled(&line.text, line_style(line, context))),
            row,
        );
        return;
    }
    let left_width = (row.width - 1) / 2;
    let left = Rect::new(row.x, row.y, left_width, 1);
    let right = Rect::new(left.right() + 1, row.y, row.width - left_width - 1, 1);
    frame.render_widget(
        Paragraph::new(context.glyphs().vertical).style(context.style(context.muted())),
        Rect::new(left.right(), row.y, 1, 1),
    );
    if source.old.is_some() {
        render_side(frame, line, pair, source.old, left, context);
    }
    let new_line = if line.kind == DiffLineKind::Removed {
        pair
    } else {
        Some(line)
    };
    if let Some(new_line) = new_line {
        let new_number = source
            .pair
            .and_then(|i| index.lines.get(i))
            .and_then(|source| source.new)
            .or(source.new);
        render_side(frame, new_line, Some(line), new_number, right, context);
    }
}

fn render_side(
    frame: &mut Frame<'_>,
    line: &DiffLine,
    pair: Option<&DiffLine>,
    n: Option<usize>,
    area: Rect,
    context: &RenderContext,
) {
    let mut spans = vec![Span::styled(
        format!("{:>4} {} ", number(n), context.glyphs().vertical),
        context.style(context.muted()),
    )];
    spans.extend(content_spans(line, pair, line_style(line, context)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn changed_words(line: &DiffLine, pair: Option<&DiffLine>) -> Vec<std::ops::Range<usize>> {
    let Some(pair) = pair else {
        return Vec::new();
    };
    if line.text.len().max(pair.text.len()) > 1000 {
        return Vec::new();
    }
    let (Some(text), Some(other)) = (line.text.get(1..), pair.text.get(1..)) else {
        return Vec::new();
    };
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(2))
        .diff_unicode_words(text, other);
    let mut offset = 1;
    let mut ranges = Vec::new();
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => {
                ranges.push(offset..offset + change.value().len());
                offset += change.value().len();
            }
            similar::ChangeTag::Equal => offset += change.value().len(),
            similar::ChangeTag::Insert => {}
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_refinement_separates_changed_words_and_preserves_unicode() {
        let old = DiffLine {
            kind: DiffLineKind::Removed,
            text: "-let café = 100; // old value".into(),
        };
        let new = DiffLine {
            kind: DiffLineKind::Added,
            text: "+let café = 250; // new value".into(),
        };
        let changed: Vec<_> = changed_words(&old, Some(&new))
            .into_iter()
            .map(|range| &old.text[range])
            .collect();
        assert_eq!(changed, ["100", "old"]);
        let spans = content_spans(&old, Some(&new), Style::reset());
        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            old.text
        );
        assert!(spans.iter().any(|span| {
            span.style
                .add_modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        }));
    }

    #[test]
    fn minified_lines_use_whole_line_fallback() {
        let line = DiffLine {
            kind: DiffLineKind::Removed,
            text: format!("-{}", "a".repeat(1000)),
        };
        let pair = DiffLine {
            kind: DiffLineKind::Added,
            text: "+short".into(),
        };
        assert!(changed_words(&line, Some(&pair)).is_empty());
    }
}
