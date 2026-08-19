//! Header, footer, overlays, and actionable error presentation.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{Action, App, View, palette_commands},
    sanitize::sanitize_str,
};

use super::{
    format::{display_width, pad_right, truncate_with},
    layout::centered_rect,
    theme::RenderContext,
};

fn view_label(view: View) -> &'static str {
    match view {
        View::Log => "LOG",
        View::Detail => "SHOW",
        View::Compare => "COMPARE",
        View::Refs => "REFS",
        View::Status => "STATUS",
        View::StatusDiff => "STATUS DIFF",
        View::Tree => "TREE",
        View::Blob => "BLOB",
        View::Blame => "BLAME",
        View::Stash => "STASH",
    }
}

pub(super) fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let repository = app
        .repository
        .root
        .file_name()
        .map(|name| sanitize_str(&name.to_string_lossy()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| sanitize_str(&app.repository.root.to_string_lossy()));
    let branch = sanitize_str(app.repository.branch.as_deref().unwrap_or("detached"));
    let revision = app.revision_label.as_ref().map_or_else(
        || sanitize_str(&app.revision),
        |label| {
            format!(
                "{}@{}",
                sanitize_str(label),
                truncate_with(&app.revision, 10, context.glyphs().ellipsis)
            )
        },
    );
    let mut left = vec![
        Span::styled(" phig ", context.strong(context.accent())),
        Span::styled(
            format!("{} ", view_label(app.view)),
            context.strong(context.accent()),
        ),
        Span::styled(repository, Style::reset()),
        Span::styled(format!(" / {branch}"), context.style(context.muted())),
        Span::styled(format!("  {revision}"), context.style(context.muted())),
    ];
    if app.view == View::Tree {
        left.push(Span::styled(
            format!(
                "  /{}",
                app.inspect
                    .tree_path
                    .as_ref()
                    .map_or("", |path| path.display.as_str())
            ),
            context.style(context.muted()),
        ));
    } else if !app.paths.is_empty() {
        left.push(Span::styled(
            format!(
                "  {}",
                app.paths
                    .iter()
                    .map(|path| path.display.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            context.style(context.muted()),
        ));
    }

    let critical = if app.has_errors() {
        Some(("request failed".to_owned(), context.error(), true))
    } else if let Some(selection) = &app.selection_contract {
        Some((
            format!("select {}", selection.target.label().to_ascii_lowercase()),
            context.accent(),
            true,
        ))
    } else if app.inspect.compare_picker {
        Some(("choose comparison base".into(), context.warning(), true))
    } else if app.inspect.loading || app.history_loading || app.preview_loading {
        Some((
            format!("loading{}", context.glyphs().ellipsis),
            context.warning(),
            false,
        ))
    } else {
        app.marked_oid.as_ref().map(|marked| {
            (
                format!("marked {}", marked.short(10)),
                context.muted(),
                false,
            )
        })
    };

    if let Some((label, color, strong)) = critical {
        let width = display_width(&label)
            .saturating_add(1)
            .min(usize::from(area.width / 2));
        let parts = Layout::horizontal([
            Constraint::Min(1),
            Constraint::Length(u16::try_from(width).unwrap_or(area.width)),
        ])
        .split(area);
        frame.render_widget(Paragraph::new(Line::from(left)), parts[0]);
        frame.render_widget(
            Paragraph::new(label)
                .alignment(Alignment::Right)
                .style(if strong {
                    context.strong(color)
                } else {
                    context.style(color)
                }),
            parts[1],
        );
    } else {
        frame.render_widget(Paragraph::new(Line::from(left)), area);
    }
}

fn index_position(selected: usize, count: usize) -> String {
    if count == 0 {
        "0/0".into()
    } else {
        format!("{}/{}", selected.min(count - 1) + 1, count)
    }
}

fn position(app: &App) -> String {
    match app.view {
        View::Log => index_position(app.selected, app.commits.len()),
        View::Refs => index_position(app.inspect.selected, app.inspect.refs.len()),
        View::Status => index_position(app.inspect.selected, app.inspect.status_entries().len()),
        View::Tree => index_position(app.inspect.selected, app.inspect.tree.len()),
        View::Blame => index_position(app.inspect.selected, app.inspect.blame.len()),
        View::Stash => index_position(app.inspect.selected, app.inspect.stashes.len()),
        View::Blob | View::Detail | View::Compare | View::StatusDiff => {
            format!("line {}", app.diff_scroll + 1)
        }
    }
}

fn key_pair(context: &RenderContext, first: &Action, second: &Action) -> String {
    format!("{}/{}", context.key(first), context.key(second))
}

fn hints(app: &App, context: &RenderContext) -> Vec<String> {
    let move_keys = key_pair(context, &Action::Move(1), &Action::Move(-1));
    match app.view {
        View::Log | View::Refs | View::Blame | View::Stash => vec![
            format!("{move_keys} move"),
            format!("{} open", context.key(&Action::Open)),
            format!("{} search", context.key(&Action::StartSearch)),
        ],
        View::Status => {
            let preview_hint = if app.inspect.status_entries().is_empty() {
                "no changes".into()
            } else if app.inspect.working_diff.is_some() {
                format!("{} open", context.key(&Action::Open))
            } else if app.inspect.loading || app.inspect.working_diff_pending.is_some() {
                "loading diff".into()
            } else {
                "no diff".into()
            };
            vec![
                format!("{move_keys} move"),
                preview_hint,
                format!("{} search", context.key(&Action::StartSearch)),
            ]
        }
        View::Tree => vec![
            format!("{move_keys} move"),
            format!("{} open", context.key(&Action::Open)),
            format!("{} up", context.key(&Action::Ascend)),
        ],
        View::Detail | View::StatusDiff => vec![
            format!("{move_keys} scroll"),
            format!("{} files", context.key(&Action::StartFilePicker)),
            format!(
                "{}/{} hunk",
                context.key(&Action::NextHunk(-1)),
                context.key(&Action::NextHunk(1))
            ),
        ],
        View::Compare => vec![
            format!("{move_keys} scroll"),
            format!("{} swap", context.key(&Action::SwapCompare)),
            format!("{} mode", context.key(&Action::ToggleCompareMode)),
        ],
        View::Blob => vec![
            format!("{move_keys} scroll"),
            format!("{} search", context.key(&Action::StartSearch)),
            format!("{} blame", context.key(&Action::ViewBlame)),
        ],
    }
}

pub(super) fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let position = position(app);
    let position_width = u16::try_from(display_width(&position)).unwrap_or(area.width);
    let parts = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(position_width.min(area.width)),
    ])
    .split(area);
    let left_width = usize::from(parts[0].width);

    let left = if let Some(selection) = &app.selection_contract {
        format!(
            " {} emit {} {} {} cancel",
            selection.accept_key,
            selection.target.label().to_ascii_lowercase(),
            context.glyphs().separator,
            selection.cancel_keys
        )
    } else if let Some(notice) = &app.notice {
        format!(" {notice}")
    } else {
        let mut text = String::from(" ");
        for hint in hints(app, context).into_iter().take(3) {
            let next = if text.trim().is_empty() {
                hint
            } else {
                format!(" {} {hint}", context.glyphs().separator)
            };
            if display_width(&text) + display_width(&next) > left_width {
                break;
            }
            text.push_str(&next);
        }
        text
    };
    frame.render_widget(
        Paragraph::new(left).style(context.style(context.muted())),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(position)
            .alignment(Alignment::Right)
            .style(context.style(context.muted())),
        parts[1],
    );
}

fn overlay_regions(
    frame: &mut Frame<'_>,
    area: Rect,
    width: u16,
    height: u16,
    title: &'static str,
    error: bool,
    context: &RenderContext,
) -> (Rect, Rect) {
    let popup = centered_rect(
        width.min(area.width.saturating_sub(2)),
        height.min(area.height),
        area,
    );
    let clear_band = Rect::new(area.x, popup.y, area.width, popup.height);
    frame.render_widget(Clear, clear_band);
    let color = if error {
        context.error()
    } else {
        context.accent()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(context.glyphs().border())
        .title(format!(" {title} "))
        .title_style(context.strong(color))
        .border_style(context.style(context.muted()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    (parts[0], parts[1])
}

pub(super) fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let (body, footer) = overlay_regions(frame, area, 68, 16, "Help", false, context);
    let move_keys = key_pair(context, &Action::Move(1), &Action::Move(-1));
    let lines = vec![
        Line::raw(format!("{} move / scroll", pad_right(&move_keys, 14))),
        Line::raw(format!(
            "{} open {s} {} back",
            pad_right(&context.key(&Action::Open), 14),
            context.key(&Action::Back),
            s = context.glyphs().separator,
        )),
        Line::raw(format!(
            "{} search {s} {}/{} matches",
            pad_right(&context.key(&Action::StartSearch), 14),
            context.key(&Action::NextMatch),
            context.key(&Action::PreviousMatch),
            s = context.glyphs().separator,
        )),
        Line::raw(format!(
            "{} commands {s} {} changed files",
            pad_right(&context.key(&Action::StartPalette), 14),
            context.key(&Action::StartFilePicker),
            s = context.glyphs().separator,
        )),
        Line::raw(format!(
            "{}/{} hunks {s} {}/{} files",
            context.key(&Action::NextHunk(-1)),
            context.key(&Action::NextHunk(1)),
            context.key(&Action::NextFile(-1)),
            context.key(&Action::NextFile(1)),
            s = context.glyphs().separator,
        )),
        Line::raw(format!(
            "{} refs {s} {} status {s} {} tree {s} {} blame {s} {} stash",
            context.key(&Action::ViewRefs),
            context.key(&Action::ViewStatus),
            context.key(&Action::ViewTree),
            context.key(&Action::ViewBlame),
            context.key(&Action::ViewStash),
            s = context.glyphs().separator,
        )),
        Line::raw(format!(
            "{} mark {s} {} compare {s} {} copy {s} {} preview",
            context.key(&Action::Mark),
            context.key(&Action::StartCompare),
            context.key(&Action::CopySelection),
            context.key(&Action::TogglePreview),
            s = context.glyphs().separator,
        )),
        Line::raw(""),
        Line::styled(
            format!("{} view", view_label(app.view).to_ascii_lowercase()),
            context.style(context.muted()),
        ),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    frame.render_widget(
        Paragraph::new(format!("{} close", context.key(&Action::ToggleHelp)))
            .style(context.style(context.muted())),
        footer,
    );
}

pub(super) fn render_search(
    frame: &mut Frame<'_>,
    draft: &str,
    body: Rect,
    context: &RenderContext,
) {
    let area = Rect::new(body.x, body.bottom().saturating_sub(1), body.width, 1);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/", context.strong(context.accent())),
            Span::raw(draft.to_owned()),
            Span::styled(
                format!("  Enter accept {} Esc cancel", context.glyphs().separator),
                context.style(context.muted()),
            ),
        ])),
        area,
    );
}

pub(super) fn render_palette(
    frame: &mut Frame<'_>,
    draft: &str,
    selected: usize,
    area: Rect,
    context: &RenderContext,
) {
    let commands = palette_commands(draft);
    let height = (commands.len().min(8) as u16).saturating_add(5);
    let (body, footer) = overlay_regions(frame, area, 62, height, "Commands", false, context);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(body);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(":", context.strong(context.accent())),
            Span::raw(draft.to_owned()),
        ])),
        parts[0],
    );
    let visible = usize::from(parts[1].height.max(1));
    let selected = selected.min(commands.len().saturating_sub(1));
    let start = selected
        .saturating_sub(visible / 2)
        .min(commands.len().saturating_sub(visible));
    let items = if commands.is_empty() {
        vec![ListItem::new("No matching commands")]
    } else {
        commands[start..]
            .iter()
            .take(visible)
            .map(|command| ListItem::new(command.name))
            .collect()
    };
    let mut state = ListState::default()
        .with_selected((!commands.is_empty()).then_some(selected.saturating_sub(start)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(context.glyphs().selected)
            .highlight_style(context.selection_style(true)),
        parts[1],
        &mut state,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} move {s} Enter run {s} Esc close",
            context.glyphs().up_down,
            s = context.glyphs().separator,
        ))
        .style(context.style(context.muted())),
        footer,
    );
}

pub(super) fn render_file_picker(
    frame: &mut Frame<'_>,
    app: &App,
    draft: &str,
    selected: usize,
    area: Rect,
    context: &RenderContext,
) {
    let files = app.file_picker_entries(draft);
    let height = (files.len().min(12) as u16).saturating_add(5);
    let (body, footer) = overlay_regions(frame, area, 68, height, "Changed files", false, context);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(body);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("file: ", context.strong(context.accent())),
            Span::raw(draft.to_owned()),
        ])),
        parts[0],
    );
    let visible = usize::from(parts[1].height.max(1));
    let selected = selected.min(files.len().saturating_sub(1));
    let start = selected
        .saturating_sub(visible / 2)
        .min(files.len().saturating_sub(visible));
    let items = if files.is_empty() {
        vec![ListItem::new("No matching changed files")]
    } else {
        files[start..]
            .iter()
            .take(visible)
            .map(|(path, _)| ListItem::new(path.clone()))
            .collect()
    };
    let mut state = ListState::default()
        .with_selected((!files.is_empty()).then_some(selected.saturating_sub(start)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(context.glyphs().selected)
            .highlight_style(context.selection_style(true)),
        parts[1],
        &mut state,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} move {s} Enter jump {s} Esc close",
            context.glyphs().up_down,
            s = context.glyphs().separator,
        ))
        .style(context.style(context.muted())),
        footer,
    );
}

pub(super) fn render_errors(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let failures = [&app.history_error, &app.preview_error, &app.inspect_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for failure in &failures {
        lines.push(Line::styled(
            format!("Failed to {}", failure.operation),
            context.strong(context.error()),
        ));
        lines.push(Line::raw(failure.detail.clone()));
    }
    let height = if failures.len() > 1 { 10 } else { 8 };
    let (body, footer) = overlay_regions(frame, area, 72, height, "Request failed", true, context);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    let recovery = if failures
        .iter()
        .any(|failure| failure.operation == "open blame")
    {
        format!(
            "{} dismiss {s} select a path, then {}",
            context.key(&Action::Back),
            context.key(&Action::ViewBlame),
            s = context.glyphs().separator,
        )
    } else {
        format!(
            "r retry {} {} dismiss",
            context.glyphs().separator,
            context.key(&Action::Back)
        )
    };
    frame.render_widget(
        Paragraph::new(recovery).style(context.style(context.warning())),
        footer,
    );
}
