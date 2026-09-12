//! Temporary file navigation alongside a dominant patch preview.

use super::{format::truncate_with, fullscreen, theme::RenderContext};
use crate::app::{App, DiffTree, DiffTreeEntry};
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

pub(super) fn render(
    frame: &mut Frame<'_>,
    app: &App,
    tree: &DiffTree,
    area: Rect,
    context: &RenderContext,
) {
    let width = if area.width >= 100 {
        (area.width / 3).min(40)
    } else {
        area.width
    };
    let list = Rect::new(area.x, area.y, width, area.height);
    render_list(frame, tree, list, context);
    if width == area.width {
        return;
    }
    let divider = Rect::new(area.x + width, area.y, 1, area.height);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(context.glyphs().vertical);
            usize::from(area.height)
        ])
        .style(context.style(context.muted())),
        divider,
    );
    fullscreen::render(
        frame,
        app,
        Rect::new(divider.right(), area.y, area.width - width - 1, area.height),
        context,
    );
}

fn render_list(frame: &mut Frame<'_>, tree: &DiffTree, area: Rect, context: &RenderContext) {
    frame.render_widget(
        Paragraph::new("CHANGED FILES").style(context.strong(context.accent())),
        Rect::new(area.x, area.y, area.width, area.height.min(1)),
    );
    let height = usize::from(area.height.saturating_sub(1));
    let start = tree
        .selected
        .saturating_sub(height / 2)
        .min(tree.visible.len().saturating_sub(height));
    let lines: Vec<_> = tree
        .visible
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(row, index)| {
            let entry = &tree.entries[*index];
            entry_line(
                entry,
                row == tree.selected,
                tree.is_collapsed(*index),
                area.width,
                context,
            )
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        ),
    );
}

fn entry_line(
    entry: &DiffTreeEntry,
    selected: bool,
    collapsed: bool,
    width: u16,
    context: &RenderContext,
) -> Line<'static> {
    let marker = if selected {
        context.glyphs().selected
    } else {
        "  "
    };
    let fold = if !entry.directory {
        " "
    } else if collapsed {
        ">"
    } else {
        "v"
    };
    let suffix = if entry.directory { "/" } else { "" };
    let indent = " ".repeat((entry.depth * 2).min(usize::from(width) / 3));
    let counts = format!(" +{} -{}", entry.added, entry.removed);
    let budget = usize::from(width).saturating_sub(4 + indent.len() + counts.len() + suffix.len());
    Line::from(vec![
        Span::styled(
            format!(
                "{marker}{indent}{fold} {}{suffix}",
                truncate_with(&entry.label, budget, context.glyphs().ellipsis)
            ),
            context.selection_style(selected),
        ),
        Span::styled(format!(" +{}", entry.added), context.style(context.added())),
        Span::styled(
            format!(" -{}", entry.removed),
            context.style(context.removed()),
        ),
    ])
}
