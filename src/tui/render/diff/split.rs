//! Left/right source columns; structural rows span the full patch.

use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::{App, patch::PatchIndex},
    domain::{Diff, DiffLine, DiffLineKind},
};

use super::{
    Canvas, Range, RenderContext, anchor_line, content_spans, gutter_style, highlight_matches,
    line_style,
};

pub(super) fn render(frame: &mut Canvas<'_>, app: &App, area: Rect, context: &RenderContext) {
    let Some(diff) = app.active_diff() else {
        return;
    };
    let index = app.patch_index();
    let map = index.rows(true, app.diff_changes_only);
    let start = map.first_row(app.diff_scroll, usize::from(area.height));
    let painter = Split {
        diff,
        index: &index,
        search: &app.search_query,
        context,
    };
    for (y, &raw) in map.rows[start..]
        .iter()
        .take(usize::from(area.height))
        .enumerate()
    {
        painter.row(
            frame,
            raw,
            Rect::new(area.x, area.y + y as u16, area.width, 1),
        );
    }
}

struct Split<'a> {
    diff: &'a Diff,
    index: &'a PatchIndex,
    search: &'a str,
    context: &'a RenderContext,
}

impl Split<'_> {
    fn row(&self, frame: &mut Canvas<'_>, raw: usize, row: Rect) {
        let line = &self.diff.lines[raw];
        let source = &self.index.lines[raw];
        if source.old.is_none() && source.new.is_none() {
            let anchor = anchor_line(self.diff, self.index, raw, row.width, self.context)
                .unwrap_or_else(|| {
                    Line::styled(line.text.as_str(), line_style(line, self.context))
                });
            frame.render_widget(Paragraph::new(anchor), row);
            return;
        }

        let left = Rect::new(row.x, row.y, (row.width - 1) / 2, 1);
        let right = Rect::new(left.right() + 1, row.y, row.width - left.width - 1, 1);
        frame.render_widget(
            Paragraph::new(self.context.glyphs().vertical)
                .style(self.context.style(self.context.muted())),
            Rect::new(left.right(), row.y, 1, 1),
        );
        let changes = self.index.word_changes(self.diff, raw);
        if let Some(n) = source.old {
            let words = changes
                .as_ref()
                .map_or(&[][..], |words| words.old.as_slice());
            self.side(frame, line, n, words, left);
        }
        let new = if line.kind == DiffLineKind::Removed {
            source.pair
        } else {
            Some(raw)
        };
        if let Some((new, n)) = new.and_then(|i| self.index.lines[i].new.map(|n| (i, n))) {
            let words = changes
                .as_ref()
                .map_or(&[][..], |words| words.new.as_slice());
            self.side(frame, &self.diff.lines[new], n, words, right);
        }
    }

    fn side(
        &self,
        frame: &mut Canvas<'_>,
        line: &DiffLine,
        n: usize,
        words: &[Range<usize>],
        area: Rect,
    ) {
        let context = self.context;
        let mut spans = vec![Span::styled(
            format!(
                "{n:>width$} {} ",
                context.glyphs().vertical,
                width = self.index.number_width
            ),
            gutter_style(line.kind, context),
        )];
        spans.extend(highlight_matches(
            content_spans(line, words, line_style(line, context)),
            self.search,
            context.search_hit(),
        ));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
