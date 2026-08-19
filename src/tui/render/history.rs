//! Commit history, graph lanes, metadata, and preview rendering.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Focus, View},
    domain::{Commit, DiffLineKind, Oid},
    sanitize::sanitize_str,
};

use super::{
    diff::render_diff,
    format::{display_date, display_width, format_commit_date, pad_right, truncate_with},
    layout::log_layout,
    render_divider,
    theme::RenderContext,
};

pub(super) fn render_log(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let layout = log_layout(app, area);
    render_history(frame, app, layout.primary, context);
    render_divider(frame, layout, context);
    if let Some(preview) = layout.secondary {
        render_preview(frame, app, preview, context);
    }
}

fn render_history(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
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
    let graph = graph_prefixes(&app.commits, end, graph_lane_limit(area.width), context);
    let items: Vec<ListItem<'_>> = app.commits[start..end]
        .iter()
        .enumerate()
        .map(|(offset, commit)| {
            ListItem::new(history_line(
                commit,
                area.width,
                graph[start + offset].clone(),
                app.marked_oid.as_ref() == Some(&commit.id),
                context,
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected - start));
    let active = app.focus == Focus::List;
    let list = List::new(items)
        .highlight_style(context.selection_style(active))
        .highlight_symbol(context.glyphs().selected)
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(list, area, &mut state);
}

pub(super) fn history_line(
    commit: &Commit,
    width: u16,
    graph: String,
    marked: bool,
    context: &RenderContext,
) -> Line<'static> {
    // Ratatui reserves the highlight symbol outside the item. Budget every
    // remaining field by terminal cells so wide names cannot consume the
    // subject or bleed into an adjacent preview.
    let item_width = usize::from(width).saturating_sub(display_width(context.glyphs().selected));
    let mark = if marked {
        context.glyphs().marked
    } else {
        "  "
    };
    let fixed_width = display_width(mark) + display_width(&graph) + 9;
    let mut remaining = item_width.saturating_sub(fixed_width);
    let minimum_subject = display_width(&commit.subject).min(12);

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

    let author_width = if width >= 78 && remaining >= minimum_subject.saturating_add(19) {
        18
    } else if remaining >= minimum_subject.saturating_add(11) {
        10
    } else {
        0
    };
    let author_field = (author_width > 0).then(|| {
        format!(
            "{} ",
            pad_right(
                &truncate_with(&commit.author.name, author_width, context.glyphs().ellipsis,),
                author_width,
            )
        )
    });
    if let Some(author) = &author_field {
        remaining = remaining.saturating_sub(display_width(author));
    }

    let decoration = (!commit.decorations.is_empty()).then(|| {
        format!(
            " ({})",
            truncate_with(
                &commit.decorations.join(", "),
                22,
                context.glyphs().ellipsis,
            )
        )
    });
    let decoration = decoration
        .filter(|value| display_width(value).saturating_add(minimum_subject) <= remaining);
    if let Some(value) = &decoration {
        remaining = remaining.saturating_sub(display_width(value));
    }
    let subject = truncate_with(&commit.subject, remaining, context.glyphs().ellipsis);

    let mut spans = vec![
        Span::styled(mark, context.strong(context.accent())),
        Span::styled(graph, context.style(context.accent())),
        Span::styled(
            commit.id.short(8).to_owned(),
            context.style(context.warning()),
        ),
        Span::raw(" "),
    ];
    if show_age {
        spans.push(Span::styled(age_field, context.style(context.muted())));
    }
    if let Some(author) = author_field {
        spans.push(Span::styled(author, context.style(context.muted())));
    }
    spans.push(Span::raw(subject));
    if let Some(decoration) = decoration {
        spans.push(Span::styled(decoration, context.style(context.muted())));
    }
    Line::from(spans)
}

fn graph_lane_limit(width: u16) -> usize {
    if width < 50 { 2 } else { 4 }
}

fn graph_prefixes(
    commits: &[Commit],
    end: usize,
    lane_limit: usize,
    context: &RenderContext,
) -> Vec<String> {
    let mut lanes: Vec<Oid> = Vec::new();
    let mut prefixes = Vec::with_capacity(end);
    let glyphs = context.glyphs();
    for commit in commits.iter().take(end) {
        let lane = lanes
            .iter()
            .position(|oid| oid == &commit.id)
            .unwrap_or_else(|| {
                lanes.insert(0, commit.id.clone());
                0
            });
        lanes.truncate(lane_limit);
        let visible_lane = lane.min(lane_limit.saturating_sub(1));
        let mut prefix = String::new();
        for index in 0..lanes.len().max(1).min(lane_limit) {
            let symbol = if index == visible_lane {
                if commit.parents.len() > 1 {
                    glyphs.merge
                } else {
                    glyphs.commit
                }
            } else {
                glyphs.lane
            };
            prefix.push(symbol);
            prefix.push(' ');
        }
        prefixes.push(prefix);

        if lane < lanes.len() {
            lanes.remove(lane);
        }
        for parent in commit.parents.iter().rev() {
            if !lanes.iter().any(|oid| oid == parent) {
                lanes.insert(lane.min(lanes.len()), parent.clone());
            }
        }
        lanes.truncate(lane_limit);
    }
    prefixes
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
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                detail.commit.id.short(12).to_owned(),
                context.style(context.warning()),
            ),
            Span::styled(parent, muted),
        ]),
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
