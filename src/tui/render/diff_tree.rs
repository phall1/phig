//! Temporary file navigation alongside a dominant patch preview.

use super::{
    format::{display_width, truncate_with},
    fullscreen,
    theme::RenderContext,
};
use crate::app::{App, DiffTree, DiffTreeEntry, TreeStatus};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
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

/// Column plan shared by every row of one tree frame.
#[derive(Debug, Clone, Copy)]
struct RowMetrics {
    /// Full row width in columns.
    total: usize,
    /// Columns for marker + guides + name + status badge.
    name_zone: usize,
    /// Width of the right-aligned `+N` zone (sign + digits).
    add_zone: usize,
    /// Width of the right-aligned `-M` zone (sign + digits).
    rem_zone: usize,
}

impl RowMetrics {
    fn new(width: usize, tree: &DiffTree) -> Self {
        let digits = |count: usize| count.to_string().len();
        let add_zone = 1 + tree
            .entries
            .iter()
            .map(|entry| digits(entry.added))
            .max()
            .unwrap_or(1);
        let rem_zone = 1 + tree
            .entries
            .iter()
            .map(|entry| digits(entry.removed))
            .max()
            .unwrap_or(1);
        let name_zone = width.saturating_sub(2 + add_zone + rem_zone);
        Self {
            total: width,
            name_zone,
            add_zone,
            rem_zone,
        }
    }
}

fn render_list(frame: &mut Frame<'_>, tree: &DiffTree, area: Rect, context: &RenderContext) {
    frame.render_widget(
        Paragraph::new("CHANGED FILES").style(context.strong(context.accent())),
        Rect::new(area.x, area.y, area.width, area.height.min(1)),
    );
    let footer_rows = u16::from(area.height >= 2);
    let height = usize::from(area.height.saturating_sub(1 + footer_rows));
    let start = tree
        .selected
        .saturating_sub(height / 2)
        .min(tree.visible.len().saturating_sub(height));
    let metrics = RowMetrics::new(usize::from(area.width), tree);
    let lines: Vec<_> = tree
        .visible
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(row, index)| {
            let entry = &tree.entries[*index];
            entry_line(tree, row, entry, row == tree.selected, metrics, context)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            u16::try_from(height).unwrap_or(0),
        ),
    );
    if footer_rows == 0 {
        return;
    }
    let text = tree.current().map_or_else(String::new, |entry| {
        footer_line(entry, metrics.total, context)
    });
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            context.style(context.muted()),
        ))),
        Rect::new(
            area.x,
            area.y
                .saturating_add(1)
                .saturating_add(u16::try_from(height).unwrap_or(0)),
            area.width,
            1,
        ),
    );
}

fn footer_line(entry: &DiffTreeEntry, width: usize, context: &RenderContext) -> String {
    let badge = entry.status.map_or("", |status| {
        // Pad the badge so the counts below line up with file rows.
        static BADGES: [&str; 4] = [" A", " D", " M", " R"];
        BADGES[match status {
            TreeStatus::Added => 0,
            TreeStatus::Deleted => 1,
            TreeStatus::Modified => 2,
            TreeStatus::Renamed => 3,
        }]
    });
    let raw = format!(
        "{}{} +{} -{}",
        entry.full, badge, entry.added, entry.removed
    );
    truncate_with(&raw, width, context.glyphs().ellipsis)
}

fn entry_line(
    tree: &DiffTree,
    row: usize,
    entry: &DiffTreeEntry,
    selected: bool,
    metrics: RowMetrics,
    context: &RenderContext,
) -> Line<'static> {
    let marker = if selected {
        context.glyphs().selected
    } else {
        "  "
    };
    let guides = guide_prefix(
        tree,
        row,
        entry.depth,
        metrics.name_zone.saturating_sub(2),
        context,
    );
    let suffix = if entry.directory { "/" } else { "" };
    let badge = entry
        .status
        .map_or(String::new(), |status| format!(" {}", status.letter()));
    let body = format!(
        "{marker}{guides} {}{suffix}",
        truncate_with(
            &entry.label,
            metrics.name_zone.saturating_sub(
                display_width(&guides) + display_width(suffix) + display_width(&badge) + 3
            ),
            context.glyphs().ellipsis,
        )
    );
    let mut spans = vec![Span::styled(
        body.clone(),
        context.selection_style(selected),
    )];
    if !badge.is_empty() {
        spans.push(Span::styled(
            badge.clone(),
            status_style(entry.status.expect("badge implies status"), context),
        ));
    }
    let pad = metrics
        .name_zone
        .saturating_sub(display_width(&body) + display_width(&badge));
    if pad > 0 {
        spans.push(Span::styled(
            " ".repeat(pad),
            context.selection_style(selected),
        ));
    }
    spans.push(Span::styled(
        format!(
            " {:>w$}",
            format!("+{}", entry.added),
            w = metrics.add_zone - 1
        ),
        context.style(context.added()),
    ));
    spans.push(Span::styled(
        format!(
            " {:>w$}",
            format!("-{}", entry.removed),
            w = metrics.rem_zone - 1
        ),
        context.style(context.removed()),
    ));
    Line::from(spans)
}

fn status_style(status: TreeStatus, context: &RenderContext) -> Style {
    let color = match status {
        TreeStatus::Added => context.added(),
        TreeStatus::Deleted => context.removed(),
        TreeStatus::Renamed => context.accent(),
        TreeStatus::Modified => context.warning(),
    };
    context.style(color)
}

/// Ancestor continuations plus the final tee/corner for one row, capped so a
/// deep path can never push the counts off the row. The single space after
/// the connector is added by the caller.
fn guide_prefix(
    tree: &DiffTree,
    row: usize,
    depth: usize,
    budget: usize,
    context: &RenderContext,
) -> String {
    let mut out = String::new();
    for level in 0..depth {
        if out.len() + 2 > budget {
            return out;
        }
        out.push_str(if has_sibling_after(tree, row, level) {
            context.glyphs().tree.vertical
        } else {
            "  "
        });
    }
    let last = if has_sibling_after(tree, row, depth) {
        context.glyphs().tree.tee
    } else {
        context.glyphs().tree.corner
    };
    if out.len() + last.len() < budget {
        out.push_str(last);
    }
    out
}

/// Whether a later visible row still sits at the same level `depth`, which is
/// what turns an ancestor line into a continuation (`│`) and a final entry
/// into a corner (`└`) rather than a tee (`├`).
fn has_sibling_after(tree: &DiffTree, row: usize, depth: usize) -> bool {
    for &index in &tree.visible[row + 1..] {
        let entry_depth = tree.entries[index].depth;
        if entry_depth < depth {
            return false;
        }
        if entry_depth == depth {
            return true;
        }
    }
    false
}
