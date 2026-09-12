//! Commit history, graph lanes, metadata, and preview rendering.

use std::collections::{HashMap, HashSet};

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Focus, View},
    domain::{Commit, DiffLineKind},
    sanitize::sanitize_str,
};

use super::{
    decorate::{decoration_spans, lane_name, spans_width},
    diff::render_diff,
    format::{display_date, display_width, format_commit_date, pad_right, truncate_with},
    graph::{GraphCache, GraphRow, lane_limit},
    layout::log_layout,
    render_divider,
    theme::RenderContext,
};

pub(super) fn render_log(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
    cache: &mut GraphCache,
) {
    let layout = log_layout(app, area);
    render_history(frame, app, layout.primary, context, cache);
    render_divider(frame, layout, context);
    if let Some(preview) = layout.secondary {
        render_preview(frame, app, preview, context);
    }
}

fn render_history(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
    cache: &mut GraphCache,
) {
    if app.commits.is_empty() {
        let message = if app.history_loading {
            return frame.render_widget(
                Paragraph::new(format!("Loading history{}", context.glyphs().ellipsis))
                    .alignment(Alignment::Center)
                    .style(context.style(context.muted())),
                area,
            );
        } else if app.history_error.is_some() {
            return frame.render_widget(
                Paragraph::new(format!(
                    "History unavailable {} retry or dismiss",
                    context.glyphs().dash
                ))
                .alignment(Alignment::Center)
                .style(context.style(context.muted())),
                area,
            );
        } else {
            "No commits in this history"
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(context.style(context.muted())),
            area,
        );
        return;
    }

    let visible = usize::from(area.height.max(1));
    let maximum_start = app.commits.len().saturating_sub(visible);
    let start = app.selected.saturating_sub(visible / 2).min(maximum_start);
    let end = (start + visible).min(app.commits.len());
    let rows = cache.rows(
        &app.commits,
        end,
        lane_limit(area.width),
        context.glyphs().graph,
    );
    // Every row is padded to the widest lane column in view so the commit text
    // stays aligned while the graph breathes.
    let graph_width = rows[start..end]
        .iter()
        .map(|row| row.width() + usize::from(row.folded_lanes() > 0))
        .max()
        .unwrap_or(0);
    let lane_names = visible_lane_names(&app.commits[..end], &rows[..end]);
    let named_in_view: HashSet<usize> = app.commits[start..end]
        .iter()
        .zip(&rows[start..end])
        .filter(|(commit, _)| !commit.decorations.is_empty())
        .map(|(_, row)| row.color())
        .collect();
    let mut seen_colors = HashSet::new();
    let selected_branch = rows.get(app.selected).map(GraphRow::color);
    let items: Vec<ListItem<'_>> = app.commits[start..end]
        .iter()
        .enumerate()
        .map(|(offset, commit)| {
            let index = start + offset;
            let row = &rows[index];
            let first_visible = seen_colors.insert(row.color());
            ListItem::new(history_line(
                commit,
                row,
                HistoryLineOpts {
                    width: area.width,
                    graph_width,
                    marked: app.marked_oid.as_ref() == Some(&commit.id),
                    selected: index == app.selected,
                    selected_branch,
                    inherited_label: inherited_lane_label(
                        commit,
                        row.color(),
                        index == app.selected,
                        first_visible,
                        &named_in_view,
                        &lane_names,
                    ),
                },
                context,
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected - start));
    let active = app.focus == Focus::List;
    let list = List::new(items)
        .highlight_style(log_highlight_style(context, active))
        .highlight_symbol("");
    frame.render_stateful_widget(list, area, &mut state);
}

fn visible_lane_names(commits: &[Commit], rows: &[GraphRow]) -> HashMap<usize, String> {
    let mut names = HashMap::new();
    for (commit, row) in commits.iter().zip(rows) {
        if names.contains_key(&row.color()) {
            continue;
        }
        if let Some(name) = lane_name(&commit.decorations) {
            names.insert(row.color(), name);
        }
    }
    names
}

fn inherited_lane_label<'a>(
    commit: &Commit,
    color: usize,
    selected: bool,
    first_visible: bool,
    named_in_view: &HashSet<usize>,
    lane_names: &'a HashMap<usize, String>,
) -> Option<&'a str> {
    if !commit.decorations.is_empty() {
        return None;
    }
    let name = lane_names.get(&color)?;
    if selected || (first_visible && !named_in_view.contains(&color)) {
        Some(name.as_str())
    } else {
        None
    }
}

fn log_highlight_style(context: &RenderContext, active: bool) -> Style {
    if context.is_monochrome() {
        return Style::reset();
    }
    if context.config().theme.selection_bg != ratatui::style::Color::Reset {
        return context.selection_style(active);
    }
    if !active {
        return Style::default();
    }
    // Marker-led: keep graph and decoration colors, emphasize with bold.
    Style::default().add_modifier(Modifier::BOLD)
}

pub(super) struct HistoryLineOpts<'a> {
    pub width: u16,
    pub graph_width: usize,
    pub marked: bool,
    pub selected: bool,
    pub selected_branch: Option<usize>,
    pub inherited_label: Option<&'a str>,
}

pub(super) fn history_line(
    commit: &Commit,
    graph: &GraphRow,
    opts: HistoryLineOpts<'_>,
    context: &RenderContext,
) -> Line<'static> {
    // The selection marker lives in the row so it can stay accent-colored
    // without List highlight replacing graph and decoration colors.
    let item_width = usize::from(opts.width);
    let cursor_width = display_width(context.glyphs().selected);
    let cursor = if opts.selected {
        Span::styled(
            context.glyphs().selected.to_owned(),
            context.strong(context.accent()),
        )
    } else {
        Span::raw(" ".repeat(cursor_width))
    };
    let mark = if opts.marked {
        context.glyphs().marked
    } else {
        "  "
    };
    // Normal rows end on a blank lane gap. Folded rows need one extra cell
    // after the bundle marker to keep it distinct from the object id.
    let graph_width = opts.graph_width.max(graph.width());
    let fixed_width = cursor_width + display_width(mark) + graph_width + 9;
    let mut remaining = item_width.saturating_sub(fixed_width);
    let minimum_subject = display_width(&commit.subject).min(12);

    let mut decoration_field = decoration_spans(
        &commit.decorations,
        opts.inherited_label,
        remaining.saturating_sub(minimum_subject.saturating_add(1)),
        Some(graph.color()),
        context,
    );
    if !decoration_field.is_empty() {
        decoration_field.push(Span::raw(" "));
        remaining = remaining.saturating_sub(spans_width(&decoration_field));
    }

    let age = display_date(
        commit.author.timestamp,
        &commit.author.timezone,
        context.config().date_mode,
    );
    let age_field = format!("{age} ");
    let show_age = display_width(&age_field).saturating_add(minimum_subject) <= remaining;
    if show_age {
        remaining = remaining.saturating_sub(display_width(&age_field));
    }

    let author_field = author_field(
        &commit.author.name,
        opts.width,
        remaining,
        minimum_subject,
        context,
    );
    if let Some(author) = &author_field {
        remaining = remaining.saturating_sub(display_width(author));
    }
    let subject = truncate_with(&commit.subject, remaining, context.glyphs().ellipsis);

    let mut spans = vec![cursor, Span::styled(mark, context.strong(context.accent()))];
    spans.extend(match opts.selected_branch {
        Some(color) => graph.spans_with_highlight(context, Some(color)),
        None => graph.spans(context),
    });
    spans.push(Span::raw(
        " ".repeat(graph_width.saturating_sub(graph.width())),
    ));
    spans.extend([
        Span::styled(
            commit.id.short(8).to_owned(),
            context.style(context.warning()),
        ),
        Span::raw(" "),
    ]);
    spans.extend(decoration_field);
    if show_age {
        spans.push(Span::styled(age_field, context.style(context.muted())));
    }
    if let Some(author) = author_field {
        spans.push(Span::styled(author, context.style(context.muted())));
    }
    spans.push(Span::raw(subject));
    Line::from(spans)
}

fn author_field(
    name: &str,
    width: u16,
    remaining: usize,
    minimum_subject: usize,
    context: &RenderContext,
) -> Option<String> {
    let author_width = if width >= 78 && remaining >= minimum_subject.saturating_add(19) {
        18
    } else if remaining >= minimum_subject.saturating_add(11) {
        10
    } else {
        return None;
    };
    Some(format!(
        "{} ",
        pad_right(
            &truncate_with(name, author_width, context.glyphs().ellipsis),
            author_width
        )
    ))
}

pub(super) fn render_preview(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    if app.preview_loading && app.preview.is_none() {
        frame.render_widget(
            Paragraph::new(format!("Loading diff{}", context.glyphs().ellipsis))
                .style(context.style(context.muted())),
            area,
        );
        return;
    }
    if app.preview.is_none() {
        let message = if app.preview_error.is_some() {
            return frame.render_widget(
                Paragraph::new(format!(
                    "Commit detail unavailable {} retry or dismiss",
                    context.glyphs().dash
                ))
                .style(context.style(context.muted())),
                area,
            );
        } else {
            "Select a commit to preview its diff"
        };
        frame.render_widget(
            Paragraph::new(message).style(context.style(context.muted())),
            area,
        );
        return;
    }
    let header_height = metadata_height(app, area.height);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(area);
    let metadata = detail_metadata(app, usize::from(header_height), context);
    frame.render_widget(
        Paragraph::new(metadata).wrap(Wrap { trim: false }),
        sections[0],
    );
    render_diff(frame, app, sections[1], context);
}

pub(super) fn metadata_height(app: &App, available: u16) -> u16 {
    if app.view != View::Detail && app.view != View::Log {
        return 0;
    }
    let Some(detail) = &app.preview else {
        return 0;
    };
    let body_lines = detail
        .commit
        .body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .min(3) as u16;
    let desired = 5_u16.saturating_add(body_lines);
    desired.min(available.saturating_sub(3)).min(8)
}

fn detail_metadata(app: &App, height: usize, context: &RenderContext) -> Vec<Line<'static>> {
    let Some(detail) = &app.preview else {
        return Vec::new();
    };
    let parent = if detail.commit.parents.len() > 1 {
        format!(
            "  parent {}/{}",
            app.parent_index.saturating_add(1),
            detail.commit.parents.len()
        )
    } else {
        String::new()
    };
    let added = detail
        .diff
        .lines
        .iter()
        .filter(|line| line.kind == DiffLineKind::Added)
        .count();
    let removed = detail
        .diff
        .lines
        .iter()
        .filter(|line| line.kind == DiffLineKind::Removed)
        .count();
    let parents = if detail.commit.parents.is_empty() {
        "root".to_owned()
    } else {
        detail
            .commit
            .parents
            .iter()
            .map(|oid| oid.short(8))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let muted = context.style(context.muted());
    let mut oid = vec![Span::styled(
        detail.commit.id.short(12).to_owned(),
        context.style(context.warning()),
    )];
    let decorations = decoration_spans(&detail.commit.decorations, None, 48, None, context);
    if !decorations.is_empty() {
        oid.push(Span::raw("  "));
        oid.extend(decorations);
    }
    oid.push(Span::styled(parent, muted));
    let mut lines = vec![
        Line::from(oid),
        Line::from(Span::styled(
            sanitize_str(&detail.commit.subject),
            if context.is_monochrome() {
                ratatui::style::Style::reset()
            } else {
                ratatui::style::Style::default().bold()
            },
        )),
        Line::from(Span::styled(
            format!(
                "Author: {} <{}>",
                sanitize_str(&detail.commit.author.name),
                sanitize_str(&detail.commit.author.email)
            ),
            muted,
        )),
        Line::from(Span::styled(
            format!(
                "Date: {}",
                format_commit_date(
                    detail.commit.author.timestamp,
                    &detail.commit.author.timezone
                )
            ),
            muted,
        )),
        Line::from(Span::styled(
            format!(
                "Parents: {parents} {separator} Files: {} (+{added} -{removed})",
                detail.diff.files.len(),
                separator = context.glyphs().separator,
            ),
            muted,
        )),
    ];
    for body_line in detail
        .commit
        .body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(height.saturating_sub(lines.len()))
    {
        lines.push(Line::raw(sanitize_str(body_line)));
    }
    lines.truncate(height);
    lines
}
