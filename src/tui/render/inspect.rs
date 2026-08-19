//! Refs, status, compare, tree, blob, blame, and stash rendering.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::App,
    domain::{RefKind, StatusCode, TreeEntryKind},
};

use super::{
    diff::render_diff_value,
    format::{compact_date, truncate},
    history::render_preview,
    layout::list_preview_areas,
    theme::{accent, muted, selection_bg, selection_fg, warning},
};

fn render_string_list(
    frame: &mut Frame<'_>,
    rows: Vec<Line<'static>>,
    selected: usize,
    area: Rect,
) {
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("Nothing to show")
                .alignment(Alignment::Center)
                .style(Style::default().fg(muted())),
            area,
        );
        return;
    }
    let visible = usize::from(area.height.max(1));
    let selected = selected.min(rows.len().saturating_sub(1));
    let start = selected
        .saturating_sub(visible / 2)
        .min(rows.len().saturating_sub(visible));
    let items = rows[start..]
        .iter()
        .take(visible)
        .cloned()
        .map(ListItem::new)
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(selected - start));
    frame.render_stateful_widget(
        List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .fg(selection_fg())
                .bg(selection_bg())
                .bold(),
        ),
        area,
        &mut state,
    );
}

pub(super) fn render_compare(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(comparison) = &app.inspect.comparison else {
        frame.render_widget(
            Paragraph::new(if app.inspect.loading {
                "Resolving comparison…"
            } else {
                "Comparison unavailable"
            })
            .alignment(Alignment::Center)
            .style(Style::default().fg(muted())),
            area,
        );
        return;
    };
    let parts = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(area);
    let base_label = app
        .inspect
        .compare_base_label
        .as_deref()
        .unwrap_or(&comparison.requested_base);
    let head_label = app
        .inspect
        .compare_head_label
        .as_deref()
        .unwrap_or(&comparison.requested_head);
    let semantics = match comparison.mode {
        crate::domain::ComparisonMode::Exact => format!(
            "exact {base_label}@{} → {head_label}@{}",
            comparison.resolved_base.short(10),
            comparison.resolved_head.short(10)
        ),
        crate::domain::ComparisonMode::MergeBase => format!(
            "merge-base({base_label}, {head_label})={} → {}",
            comparison
                .merge_base
                .as_ref()
                .map_or("?", |oid| oid.short(10)),
            comparison.resolved_head.short(10)
        ),
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(semantics, Style::default().fg(accent()).bold()),
            Line::raw(format!(
                "resolved inputs: {} → {}",
                comparison.requested_base, comparison.requested_head
            )),
            Line::styled(
                format!(
                    "ahead {} · behind {} · files {}",
                    comparison.ahead,
                    comparison.behind,
                    comparison.diff.files.len()
                ),
                Style::default().fg(muted()),
            ),
        ]),
        parts[0],
    );
    render_diff_value(frame, &comparison.diff, app.diff_scroll, parts[1], true);
}

pub(super) fn render_refs(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (list, preview) = list_preview_areas(app, area);
    let rows = app
        .inspect
        .refs
        .iter()
        .map(|reference| {
            let kind = match reference.kind {
                RefKind::LocalBranch => "branch",
                RefKind::RemoteBranch => "remote",
                RefKind::Tag => "tag",
                RefKind::Stash => "stash",
                RefKind::Other => "ref",
            };
            let head = if reference.is_head { "*" } else { " " };
            let upstream = if list.width >= 84 {
                reference
                    .upstream
                    .as_ref()
                    .map_or(String::new(), |name| format!(" ↑{}", name.display()))
            } else {
                String::new()
            };
            let oid = if list.width >= 54 {
                format!(" {}", reference.target.short(8))
            } else {
                String::new()
            };
            let subject = if list.width >= 70 {
                format!("  {}", reference.subject)
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(format!("{head} {kind:<7} "), Style::default().fg(muted())),
                Span::styled(
                    reference.short_name.display().to_owned(),
                    Style::default().fg(warning()),
                ),
                Span::styled(oid, Style::default().fg(muted())),
                Span::styled(upstream, Style::default().fg(Color::Blue)),
                Span::raw(subject),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, list);
    if preview.height > 0 {
        render_preview(frame, app, preview);
    }
}

fn status_group(entry: &crate::domain::StatusEntry) -> &'static str {
    if entry.conflict.is_some() {
        "conflict"
    } else if entry.index == StatusCode::Untracked {
        "untracked"
    } else if entry.index != StatusCode::Unmodified && entry.worktree != StatusCode::Unmodified {
        "mixed"
    } else if entry.index != StatusCode::Unmodified {
        "staged"
    } else {
        "unstaged"
    }
}

pub(super) fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (list, preview) = list_preview_areas(app, area);
    let rows = app
        .inspect
        .status_entries()
        .iter()
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    format!("{:<9} ", status_group(entry)),
                    Style::default().fg(muted()),
                ),
                Span::styled(
                    format!(
                        "{}{} ",
                        entry.index.porcelain_char(),
                        entry.worktree.porcelain_char()
                    ),
                    Style::default().fg(warning()),
                ),
                Span::raw(entry.path.display.clone()),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, list);
    if preview.height > 0 {
        if let Some(diff) = &app.inspect.working_diff {
            let parts =
                Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(preview);
            frame.render_widget(
                Paragraph::new(if app.inspect.status_diff_staged {
                    "staged diff"
                } else {
                    "unstaged diff"
                })
                .style(Style::default().fg(accent()).bold()),
                parts[0],
            );
            render_diff_value(frame, diff, app.diff_scroll, parts[1], true);
        } else {
            let message = if app.inspect.loading {
                if app.inspect.status_diff_staged {
                    "Loading staged diff…"
                } else {
                    "Loading unstaged diff…"
                }
            } else if app.inspect_error.is_some() {
                "Working diff unavailable — r retries · Esc dismisses"
            } else {
                "Select a tracked change to preview"
            };
            frame.render_widget(
                Paragraph::new(message).style(Style::default().fg(muted())),
                preview,
            );
        }
    }
}

pub(super) fn render_status_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(diff) = &app.inspect.working_diff else {
        frame.render_widget(
            Paragraph::new("Working diff unavailable")
                .alignment(Alignment::Center)
                .style(Style::default().fg(muted())),
            area,
        );
        return;
    };
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    frame.render_widget(
        Paragraph::new(if app.inspect.status_diff_staged {
            "staged working diff"
        } else {
            "unstaged working diff"
        })
        .style(Style::default().fg(accent()).bold()),
        parts[0],
    );
    render_diff_value(frame, diff, app.diff_scroll, parts[1], true);
}

pub(super) fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rows = app
        .inspect
        .tree
        .iter()
        .map(|entry| {
            let icon = match entry.kind {
                TreeEntryKind::Tree => "dir ",
                TreeEntryKind::Blob => "file",
                TreeEntryKind::Commit => "subm",
                TreeEntryKind::Unknown => "obj ",
            };
            Line::from(vec![
                Span::styled(
                    format!("{icon} {} ", entry.mode),
                    Style::default().fg(muted()),
                ),
                Span::styled(
                    entry.path.display.clone(),
                    Style::default().fg(if entry.kind == TreeEntryKind::Tree {
                        accent()
                    } else {
                        Color::Reset
                    }),
                ),
                Span::styled(
                    entry
                        .size
                        .map_or(String::new(), |size| format!("  {size} B")),
                    Style::default().fg(muted()),
                ),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, area);
}

pub(super) fn render_blob(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(blob) = &app.inspect.blob else {
        frame.render_widget(Paragraph::new("Loading blob…"), area);
        return;
    };
    if blob.binary == Some(true) {
        frame.render_widget(
            Paragraph::new(format!(
                "Binary blob · {} bytes · {}{}",
                blob.size,
                blob.id.short(12),
                if blob.truncated {
                    " · preview truncated"
                } else {
                    ""
                }
            ))
            .alignment(Alignment::Center)
            .style(Style::default().fg(muted())),
            area,
        );
    } else {
        let bytes = blob.bytes();
        let lines = bytes
            .split(|byte| *byte == b'\n')
            .skip(app.diff_scroll)
            .take(usize::from(area.height))
            .map(crate::sanitize::sanitize_bytes)
            .map(Line::raw)
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    }
}

pub(super) fn render_blame(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (list, preview) = list_preview_areas(app, area);
    let rows = app
        .inspect
        .blame
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let repeated = index > 0 && app.inspect.blame[index - 1].id == line.id;
            let attribution = if repeated {
                format!("{:>5} {:8} {:10} {:12} ", line.final_line, "", "", "")
            } else {
                format!(
                    "{:>5} {:8} {:10} {:12} ",
                    line.final_line,
                    line.id.short(8),
                    line.author_time
                        .map_or_else(|| "----------".into(), compact_date),
                    truncate(&line.author, 12)
                )
            };
            Line::from(vec![
                Span::styled(
                    attribution,
                    Style::default().fg(if repeated { muted() } else { Color::Yellow }),
                ),
                Span::raw(line.content.clone()),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, list);
    if preview.height > 0 {
        render_preview(frame, app, preview);
    }
}

pub(super) fn render_stashes(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (list, preview) = list_preview_areas(app, area);
    let rows = app
        .inspect
        .stashes
        .iter()
        .map(|stash| {
            Line::from(vec![
                Span::styled(
                    format!("{} {} ", stash.selector, stash.id.short(8)),
                    Style::default().fg(warning()),
                ),
                Span::raw(stash.subject.clone()),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, list);
    if preview.height > 0 {
        render_preview(frame, app, preview);
    }
}
