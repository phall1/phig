//! Shared discovery presentation: matched text, effective shortcuts, and query chrome.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{ListItem, Paragraph},
};

use crate::{app::PaletteCommand, fuzzy::Query, sanitize::sanitize_str};

use super::{format::display_width, theme::RenderContext};

pub(super) fn matched_label(
    text: &str,
    query: &Query,
    width: usize,
    context: &RenderContext,
) -> Line<'static> {
    let positions = query
        .find(text)
        .map_or_else(Vec::new, |matched| matched.positions);
    let (label, content_len) = clip_label(text, width, context.glyphs().ellipsis);
    let source = Span::raw(label);
    let mut index = 0;
    let spans = source
        .styled_graphemes(Style::default())
        .map(|grapheme| {
            let end = index + grapheme.symbol.chars().count();
            let matched = (index..end.min(content_len))
                .any(|position| positions.binary_search(&position).is_ok());
            index = end;
            Span::styled(grapheme.symbol.to_owned(), match_style(matched, context))
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Use the same per-grapheme advance as Ratatui, including variation selectors,
/// ZWJ emoji, and scripts whose whole-string width differs from separate cells.
fn cell_width(text: &str) -> usize {
    Span::raw(text)
        .styled_graphemes(Style::default())
        .map(|grapheme| display_width(grapheme.symbol))
        .sum()
}

fn take_cells(text: &str, width: usize) -> String {
    let mut used = 0;
    Span::raw(text)
        .styled_graphemes(Style::default())
        .take_while(|grapheme| {
            used += display_width(grapheme.symbol);
            used <= width
        })
        .map(|grapheme| grapheme.symbol)
        .collect()
}

fn clip_label(text: &str, width: usize, ellipsis: &str) -> (String, usize) {
    if cell_width(text) <= width {
        return (text.to_owned(), text.chars().count());
    }
    let suffix = take_cells(ellipsis, width);
    let mut label = take_cells(text, width.saturating_sub(cell_width(&suffix)));
    let content_len = label.chars().count();
    label.push_str(&suffix);
    (label, content_len)
}

fn match_style(matched: bool, context: &RenderContext) -> Style {
    if matched && !context.is_monochrome() {
        context.strong(context.accent()).underlined()
    } else {
        Style::default()
    }
}

pub(super) fn command_item(
    command: &PaletteCommand,
    query: &Query,
    width: usize,
    context: &RenderContext,
) -> ListItem<'static> {
    let key = sanitize_str(&context.key(&command.action));
    let key_width = cell_width(&key).min(width / 3);
    let label_width = width.saturating_sub(key_width + 2);
    let mut line = matched_label(command.name, query, label_width, context);
    line.spans.push(Span::raw(
        " ".repeat(width.saturating_sub(line.width() + key_width)),
    ));
    line.spans.push(Span::styled(
        clip_label(&key, key_width, context.glyphs().ellipsis).0,
        context.style(context.muted()),
    ));
    ListItem::new(line)
}

pub(super) fn render_prompt(
    frame: &mut Frame<'_>,
    draft: &str,
    prefix: &str,
    count: usize,
    area: Rect,
    context: &RenderContext,
) {
    let noun = if count == 1 { "result" } else { "results" };
    let counter = format!("{count} {noun}");
    let parts = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length((cell_width(&counter) as u16).min(area.width / 3)),
    ])
    .split(area);
    let query_width = usize::from(parts[0].width).saturating_sub(cell_width(prefix) + 1);
    let tail = query_tail(&sanitize_str(draft), query_width);
    let content = if draft.is_empty() {
        Span::styled("type to fuzzy-find", context.style(context.muted()))
    } else {
        Span::raw(tail.clone())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prefix.to_owned(), context.strong(context.accent())),
            content,
        ])),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(counter)
            .alignment(Alignment::Right)
            .style(context.style(context.muted())),
        parts[1],
    );
    let cursor = cell_width(prefix) + cell_width(&tail);
    if area.height > 0 && cursor < usize::from(parts[0].width) {
        frame.set_cursor_position((parts[0].x + cursor as u16, parts[0].y));
    }
}

fn query_tail(text: &str, width: usize) -> String {
    let mut used = 0;
    let source = Span::raw(text);
    let graphemes: Vec<_> = source.styled_graphemes(Style::default()).collect();
    let tail: Vec<_> = graphemes
        .iter()
        .rev()
        .take_while(|grapheme| {
            used += display_width(grapheme.symbol);
            used <= width
        })
        .map(|grapheme| grapheme.symbol)
        .collect();
    tail.into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
    };

    use super::*;
    use crate::tui::render::{ColorMode, GlyphMode, RenderConfig};

    #[test]
    fn highlights_unicode_graphemes_without_splitting_combining_marks() {
        let context = RenderContext::new(RenderConfig {
            color_mode: ColorMode::Always,
            ..RenderConfig::default()
        });
        let line = matched_label("İ/界/cafe\u{301}.rs", &Query::new("i界e"), 40, &context);
        let highlighted: String = line
            .spans
            .iter()
            .filter(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(highlighted, "İ界e\u{301}");
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(line), frame.area()))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(8, 0)].symbol(), "e\u{301}");
    }

    #[test]
    fn clipped_labels_and_long_queries_respect_cell_budgets() {
        let context = RenderContext::new(RenderConfig {
            glyph_mode: GlyphMode::Ascii,
            ..RenderConfig::default()
        });
        for width in 0..18 {
            let line = matched_label("src/界界界/file.rs", &Query::new("rs"), width, &context);
            assert!(line.width() <= width);
            assert!(
                line.spans
                    .iter()
                    .all(|span| !span.style.add_modifier.contains(Modifier::UNDERLINED)),
                "hidden matches must not underline the ellipsis"
            );
        }
        assert_eq!(query_tail("long/界e\u{301}", 3), "界e\u{301}");
        assert_eq!(query_tail("long/界e\u{301}", 2), "e\u{301}");
    }

    #[test]
    fn emoji_labels_fit_without_splitting_variation_selectors_or_zwj_sequences() {
        let context = RenderContext::new(RenderConfig::default());
        for (text, width, expected) in
            [("☀️x", 2, "…"), ("abc☀️xyz", 5, "abc…"), ("👩‍💻xx", 3, "👩‍💻…")]
        {
            let line = matched_label(text, &Query::new(""), width, &context);
            assert!(line.width() <= width, "{text:?} exceeded {width} cells");
            assert_eq!(
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>(),
                expected
            );
        }
    }

    #[test]
    fn query_cursor_follows_rendered_cells_and_survives_long_input_and_resize() {
        let context = RenderContext::new(RenderConfig::default());
        for width in [20, 60, 100] {
            for draft in ["لا", "abcلا", "long/界/☀️/👩‍💻/cafe\u{301}/لا"] {
                let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
                terminal
                    .draw(|frame| render_prompt(frame, draft, ": ", 2, frame.area(), &context))
                    .unwrap();
                let position = terminal.get_cursor_position().unwrap();
                let counter_width = 9.min(width / 3);
                let tail = query_tail(draft, usize::from(width - counter_width).saturating_sub(3));
                assert_eq!(position.x, 2 + cell_width(&tail) as u16);
                assert!(position.x < width - counter_width);
                assert_eq!(
                    terminal.backend().buffer()[(position.x, 0)].symbol(),
                    " ",
                    "cursor must follow the last rendered glyph"
                );
            }
        }
    }

    #[test]
    fn monochrome_picker_labels_keep_reset_colors_and_readable_text() {
        let context = RenderContext::new(RenderConfig {
            color_mode: ColorMode::Never,
            ..RenderConfig::default()
        });
        let line = matched_label("Toggle preview", &Query::new("tglpr"), 40, &context);
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(line), frame.area()))
            .unwrap();
        for cell in &terminal.backend().buffer().content {
            assert_eq!(cell.fg, Color::Reset);
            assert_eq!(cell.bg, Color::Reset);
            assert!(cell.modifier.is_empty());
        }
    }
}
