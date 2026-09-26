//! Shared patch rendering for detail and inspection views.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::patch::{PatchIndex, RowMap},
    app::{App, Focus, TreeStatus, View},
    domain::{Diff, DiffLine, DiffLineKind},
};

use super::{
    format::{display_width, truncate_with},
    history::render_preview,
    layout::diff_content_rows,
    theme::RenderContext,
};

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
    let map = index.rows(false);
    let lines: Vec<Line<'_>> = map
        .rows
        .iter()
        .skip(first_row(map, app.diff_scroll, area.height))
        .take(usize::from(area.height))
        .map(|&raw| {
            let line = &diff.lines[raw];
            if let Some(line) = anchor_line(diff, &index, raw, area.width, context) {
                return line;
            }
            let source = &index.lines[raw];
            let pair = source.pair.and_then(|pair| diff.lines.get(pair));
            let mut spans = vec![Span::styled(
                format!(
                    "{:>width$} {:>width$} {} ",
                    number(source.old),
                    number(source.new),
                    context.glyphs().vertical,
                    width = index.number_width,
                ),
                gutter_style(line.kind, context),
            )];
            spans.extend(content_spans(line, pair, line_style(line, context)));
            Line::from(spans)
        })
        .collect();
    let style = if active || context.is_monochrome() {
        Style::reset()
    } else {
        context.style(context.muted())
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
}

/// Jumps may target a row near the end; keep the final page full.
fn first_row(map: &RowMap, scroll: usize, height: u16) -> usize {
    map.position(scroll)
        .min(map.rows.len().saturating_sub(usize::from(height)))
}

/// Rows that are not source lines: file banners, hunk headers, and Git's
/// notes such as mode changes or a missing final newline.
fn anchor_line(
    diff: &Diff,
    index: &PatchIndex,
    raw: usize,
    width: u16,
    context: &RenderContext,
) -> Option<Line<'static>> {
    let line = &diff.lines[raw];
    match line.kind {
        DiffLineKind::FileHeader => Some(file_banner(diff, index, raw, width, context)),
        DiffLineKind::HunkHeader => Some(hunk_header(line, index, context)),
        DiffLineKind::Metadata => Some(Line::from(vec![
            Span::raw(gutter_blank(index)),
            Span::styled(
                format!("{} {}", context.glyphs().vertical, line.text),
                context.style(context.muted()),
            ),
        ])),
        _ => None,
    }
}

fn gutter_blank(index: &PatchIndex) -> String {
    " ".repeat(index.number_width * 2 + 2)
}

/// `M path/to/file ──────────── +3 -1`: change kind, path (with the source
/// of a rename), a quiet rule, and the file's line counts.
fn file_banner(
    diff: &Diff,
    index: &PatchIndex,
    raw: usize,
    width: u16,
    context: &RenderContext,
) -> Line<'static> {
    let line = &diff.lines[raw];
    // Files are in patch order, so their header lines are sorted.
    let Ok(ordinal) = diff
        .files
        .binary_search_by_key(&raw, |file| file.header_line)
    else {
        return Line::styled(line.text.clone(), context.style(context.muted()));
    };
    let file = &diff.files[ordinal];
    let stats = index.file_stats.get(ordinal).copied().unwrap_or_default();
    let status = TreeStatus::of(file);
    let glyphs = context.glyphs();
    let mut path_spans = Vec::new();
    match (&file.old_path, &file.new_path) {
        (Some(old), Some(new)) if old != new => {
            path_spans.push(Span::styled(
                format!("{} {} ", old.display, glyphs.arrow),
                context.style(context.muted()),
            ));
            path_spans.push(Span::styled(
                new.display.clone(),
                context.strong(Color::Reset),
            ));
        }
        (old, new) => {
            let path = new
                .as_ref()
                .or(old.as_ref())
                .map_or("", |path| path.display.as_str());
            path_spans.push(Span::styled(path.to_owned(), context.strong(Color::Reset)));
        }
    }
    let counts = format!(" +{} -{}", stats.added, stats.removed);
    let letter = status.map_or(' ', TreeStatus::letter);
    let width = usize::from(width);
    let fixed = 2 + display_width(&counts) + 1;
    let path_width = super::decorate::spans_width(&path_spans);
    let path_budget = width.saturating_sub(fixed + 2);
    if path_width > path_budget {
        // Keep the tail of a long path: the file name matters most.
        let full: String = path_spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        path_spans = vec![Span::styled(
            truncate_start(&full, path_budget, glyphs.ellipsis),
            context.strong(Color::Reset),
        )];
    }
    let rule = width.saturating_sub(fixed + super::decorate::spans_width(&path_spans) + 1);
    let mut spans = vec![Span::styled(
        format!("{letter} "),
        status.map_or_else(Style::reset, |status| {
            context.emphasize(status_style(status, context), Modifier::BOLD)
        }),
    )];
    spans.extend(path_spans);
    spans.push(Span::styled(
        format!(" {}", glyphs.horizontal.repeat(rule)),
        context.style(context.muted()),
    ));
    spans.push(Span::styled(
        format!(" +{}", stats.added),
        context.style(context.added()),
    ));
    spans.push(Span::styled(
        format!(" -{}", stats.removed),
        context.style(context.removed()),
    ));
    Line::from(spans)
}

pub(super) fn status_style(status: TreeStatus, context: &RenderContext) -> Style {
    let color = match status {
        TreeStatus::Added => context.added(),
        TreeStatus::Deleted => context.removed(),
        TreeStatus::Renamed => context.accent(),
        TreeStatus::Modified => context.warning(),
    };
    context.style(color)
}

/// `@@ -21,6 +21,7 @@ fn name` with the range quiet and the enclosing
/// function name, when Git found one, kept readable.
fn hunk_header(line: &DiffLine, index: &PatchIndex, context: &RenderContext) -> Line<'static> {
    let text = line.text.as_str();
    let (range, scope) = text
        .strip_prefix("@@")
        .and_then(|rest| rest.find("@@").map(|end| end + 4))
        .and_then(|end| text.get(..end).zip(text.get(end..)))
        .unwrap_or((text, ""));
    Line::from(vec![
        Span::styled(
            format!("{}{} ", gutter_blank(index), context.glyphs().vertical),
            context.style(context.muted()),
        ),
        Span::styled(range.to_owned(), context.style(context.accent())),
        Span::styled(scope.to_owned(), context.strong(Color::Reset)),
    ])
}

fn truncate_start(value: &str, width: usize, ellipsis: &str) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    let reversed: String = value.chars().rev().collect();
    let tail = truncate_with(&reversed, width, "");
    let keep = width.saturating_sub(display_width(ellipsis));
    let tail: String = tail
        .chars()
        .take_while({
            let mut used = 0;
            move |c| {
                used += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
                used <= keep
            }
        })
        .collect();
    format!("{ellipsis}{}", tail.chars().rev().collect::<String>())
}

fn number(value: Option<usize>) -> String {
    value.map_or_else(String::new, |n| n.to_string())
}

fn gutter_style(kind: DiffLineKind, context: &RenderContext) -> Style {
    match kind {
        DiffLineKind::Added => context.emphasize(context.style(context.added()), Modifier::DIM),
        DiffLineKind::Removed => context.emphasize(context.style(context.removed()), Modifier::DIM),
        _ => context.style(context.muted()),
    }
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
    let map = index.rows(true);
    for (y, raw) in map
        .rows
        .iter()
        .skip(first_row(map, app.diff_scroll, area.height))
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
        let line = anchor_line(diff, index, raw, row.width, context)
            .unwrap_or_else(|| Line::styled(line.text.clone(), line_style(line, context)));
        frame.render_widget(Paragraph::new(line), row);
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
        render_side(
            frame,
            line,
            pair,
            source.old,
            index.number_width,
            left,
            context,
        );
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
        render_side(
            frame,
            new_line,
            Some(line),
            new_number,
            index.number_width,
            right,
            context,
        );
    }
}

fn render_side(
    frame: &mut Frame<'_>,
    line: &DiffLine,
    pair: Option<&DiffLine>,
    n: Option<usize>,
    number_width: usize,
    area: Rect,
    context: &RenderContext,
) {
    let mut spans = vec![Span::styled(
        format!(
            "{:>number_width$} {} ",
            number(n),
            context.glyphs().vertical
        ),
        gutter_style(line.kind, context),
    )];
    spans.extend(content_spans(line, pair, line_style(line, context)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Longest line, in bytes, that gets changed-word emphasis.
const MAX_REFINED_LINE: usize = 400;

fn changed_words(line: &DiffLine, pair: Option<&DiffLine>) -> Vec<std::ops::Range<usize>> {
    let Some(pair) = pair else {
        return Vec::new();
    };
    // Bounded by length rather than a wall-clock deadline, so emphasis is
    // identical on every frame and machine; long or minified lines fall back
    // to whole-line color.
    if line.text.len().max(pair.text.len()) > MAX_REFINED_LINE {
        return Vec::new();
    }
    let (Some(text), Some(other)) = (line.text.get(1..), pair.text.get(1..)) else {
        return Vec::new();
    };
    let diff = similar::TextDiff::configure().diff_unicode_words(text, other);
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
            text: format!("-{}", "a".repeat(MAX_REFINED_LINE)),
        };
        let pair = DiffLine {
            kind: DiffLineKind::Added,
            text: "+short".into(),
        };
        assert!(changed_words(&line, Some(&pair)).is_empty());
    }
}
