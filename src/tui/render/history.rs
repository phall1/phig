//! Commit history, graph lanes, metadata, and preview rendering.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
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
    format::{display_date, format_commit_date, truncate},
    layout::log_areas,
    theme::{accent, muted, selection_bg, selection_fg, warning},
};

pub(super) fn render_log(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (history, preview) = log_areas(app, area);
    render_history(frame, app, history);
    if let Some(preview) = preview {
        render_preview(frame, app, preview);
    }
}

fn render_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.commits.is_empty() {
        let message = if app.history_loading {
            "Loading history…"
        } else if app.history_error.is_some() {
            "History unavailable — r retries · Esc dismisses"
        } else {
            "No commits in this history"
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::default().fg(muted())),
            area,
        );
        return;
    }

    let visible = usize::from(area.height.max(1));
    let maximum_start = app.commits.len().saturating_sub(visible);
    let start = app.selected.saturating_sub(visible / 2).min(maximum_start);
    let end = (start + visible).min(app.commits.len());
    let graph = graph_prefixes(&app.commits, end, graph_lane_limit(area.width));
    let items: Vec<ListItem<'_>> = app.commits[start..end]
        .iter()
        .enumerate()
        .map(|(offset, commit)| {
            ListItem::new(history_line(
                commit,
                area.width,
                graph[start + offset].clone(),
                app.marked_oid.as_ref() == Some(&commit.id),
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected - start));
    let highlight = if app.focus == Focus::List {
        Style::default()
            .fg(selection_fg())
            .bg(selection_bg())
            .bold()
    } else {
        Style::default().bg(Color::DarkGray)
    };
    let list = List::new(items)
        .highlight_style(highlight)
        .highlight_symbol("› ")
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(list, area, &mut state);
}

fn history_line(commit: &Commit, width: u16, graph: String, marked: bool) -> Line<'static> {
    let author = if width >= 78 {
        truncate(&commit.author.name, 18)
    } else if width >= 54 {
        truncate(&commit.author.name, 10)
    } else {
        String::new()
    };
    let age = display_date(commit.author.timestamp, &commit.author.timezone);
    let decorations = if commit.decorations.is_empty() {
        String::new()
    } else {
        format!(" ({})", truncate(&commit.decorations.join(", "), 22))
    };
    let mut spans = vec![
        Span::styled(
            if marked { "◆ " } else { "  " },
            Style::default().fg(Color::Magenta).bold(),
        ),
        Span::styled(graph, Style::default().fg(accent())),
        Span::styled(
            commit.id.short(8).to_owned(),
            Style::default().fg(warning()),
        ),
        Span::styled(format!(" {age:>4} "), Style::default().fg(muted())),
    ];
    if !author.is_empty() {
        spans.push(Span::styled(
            format!("{author:<18} "),
            Style::default().fg(Color::Blue),
        ));
    }
    spans.push(Span::raw(commit.subject.clone()));
    spans.push(Span::styled(
        decorations,
        Style::default().fg(Color::Magenta),
    ));
    Line::from(spans)
}

fn graph_lane_limit(width: u16) -> usize {
    if width < 50 { 2 } else { 4 }
}

fn graph_prefixes(commits: &[Commit], end: usize, lane_limit: usize) -> Vec<String> {
    let mut lanes: Vec<Oid> = Vec::new();
    let mut prefixes = Vec::with_capacity(end);
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
                    '◆'
                } else {
                    '●'
                }
            } else {
                '│'
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

pub(super) fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.preview_loading && app.preview.is_none() {
        frame.render_widget(
            Paragraph::new("Loading diff…").style(Style::default().fg(muted())),
            area,
        );
        return;
    }
    if app.preview.is_none() {
        let message = if app.preview_error.is_some() {
            "Commit detail unavailable — r retries · Esc dismisses"
        } else {
            "Select a commit to preview its diff"
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(muted())),
            area,
        );
        return;
    }
    let header_height = metadata_height(app, area.height);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(area);
    let metadata = detail_metadata(app, usize::from(header_height));
    frame.render_widget(
        Paragraph::new(metadata).wrap(Wrap { trim: false }),
        sections[0],
    );
    render_diff(frame, app, sections[1]);
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

fn detail_metadata(app: &App, height: usize) -> Vec<Line<'static>> {
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
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                detail.commit.id.short(12).to_owned(),
                Style::default().fg(warning()),
            ),
            Span::styled(parent, Style::default().fg(muted())),
        ]),
        Line::from(Span::styled(
            sanitize_str(&detail.commit.subject),
            Style::default().bold(),
        )),
        Line::from(Span::styled(
            format!(
                "Author: {} <{}>",
                sanitize_str(&detail.commit.author.name),
                sanitize_str(&detail.commit.author.email)
            ),
            Style::default().fg(muted()),
        )),
        Line::from(Span::styled(
            format!(
                "Date: {}",
                format_commit_date(
                    detail.commit.author.timestamp,
                    &detail.commit.author.timezone
                )
            ),
            Style::default().fg(muted()),
        )),
        Line::from(Span::styled(
            format!(
                "Parents: {parents} · Files: {} (+{added} -{removed})",
                detail.diff.files.len()
            ),
            Style::default().fg(muted()),
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
