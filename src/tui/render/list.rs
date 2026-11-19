//! Shared viewport policy for history, inspection, and discovery.

use std::ops::Range;

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Text},
};
use ratcn::{ListStyle, ListWidget};

use super::{Canvas, RenderContext};

pub(super) struct Window {
    pub rows: Range<usize>,
    pub selected: Option<usize>,
}

impl Window {
    pub(super) fn new(count: usize, selected: usize, area: Rect) -> Self {
        let height = usize::from(area.height);
        let selected = selected.min(count.saturating_sub(1));
        let start = selected
            .saturating_sub(height / 2)
            .min(count.saturating_sub(height));
        Self {
            rows: start..(start + height).min(count),
            selected: (count != 0).then_some(selected),
        }
    }

    pub(super) fn paint(
        &self,
        canvas: &mut Canvas<'_>,
        area: Rect,
        context: &RenderContext,
        line: impl FnMut(usize) -> Line<'static>,
    ) {
        self.paint_rows(
            canvas,
            area,
            context,
            context.glyphs().selected,
            context.selection_style(true),
            self.rows.clone().map(line),
        );
    }

    pub(super) fn paint_rows(
        &self,
        canvas: &mut Canvas<'_>,
        area: Rect,
        context: &RenderContext,
        marker: &str,
        selection: Style,
        lines: impl IntoIterator<Item = Line<'static>>,
    ) {
        let mut style = ListStyle::from_theme(&context.ratcn_theme());
        style.foreground = Color::Reset;
        style.focused_row_background = selection.bg.unwrap_or(Color::Reset);
        style.focused_foreground = selection.fg.unwrap_or(Color::Reset);
        let rows = lines.into_iter().map(Text::from).collect::<Vec<_>>();
        canvas.render_widget(
            ListWidget::new(&rows)
                .style(style)
                .focused(true)
                .first_item(self.rows.start)
                .focused_item(self.selected)
                .focus_symbol(marker),
            area,
        );
        if let Some(offset) = self
            .selected
            .and_then(|index| index.checked_sub(self.rows.start))
            && offset < usize::from(area.height)
        {
            // ratcn supplies the base colors and marker; apply Phig's full
            // selection style after inline spans, including explicit overrides
            // and reset policy. A bold-only selection preserves span colors.
            canvas.set_style(
                Rect::new(area.x, area.y + offset as u16, area.width, 1),
                selection,
            );
        }
    }
}
