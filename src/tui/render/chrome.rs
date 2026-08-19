//! Header, footer, overlays, and actionable error presentation.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, View, palette_commands},
    sanitize::sanitize_str,
};

use super::{
    format::truncate,
    layout::centered_rect,
    theme::{accent, error_color, muted, selection_bg, selection_fg, warning},
};

pub(super) fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let repository = app
        .repository
        .root
        .file_name()
        .map(|name| sanitize_str(&name.to_string_lossy()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| sanitize_str(&app.repository.root.to_string_lossy()));
    let branch = app.repository.branch.as_deref().unwrap_or("detached");
    let view = match app.view {
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
    };
    let mut spans = vec![
        Span::styled(
            " phig ",
            Style::default()
                .fg(selection_fg())
                .bg(selection_bg())
                .bold(),
        ),
        Span::raw(" "),
        Span::styled(repository, Style::default().bold()),
        Span::styled(format!("  {view}  "), Style::default().fg(accent())),
        Span::raw(app.revision_label.as_ref().map_or_else(
            || sanitize_str(&app.revision),
            |label| format!("{}@{}", sanitize_str(label), truncate(&app.revision, 10)),
        )),
        Span::styled(format!("  {branch}"), Style::default().fg(muted())),
    ];
    if let Some(marked) = &app.marked_oid {
        spans.push(Span::styled(
            format!("  marked:{}", marked.short(10)),
            Style::default().fg(Color::Magenta).bold(),
        ));
    }
    if app.inspect.compare_picker {
        spans.push(Span::styled(
            "  choose comparison base",
            Style::default().fg(warning()).bold(),
        ));
    }
    if !app.paths.is_empty() {
        let paths = app
            .paths
            .iter()
            .map(|path| path.display.as_str())
            .collect::<Vec<_>>()
            .join(",");
        spans.push(Span::styled(
            format!("  path:{paths}"),
            Style::default().fg(muted()),
        ));
    }
    if app.view == View::Tree {
        spans.push(Span::styled(
            format!(
                "  /{}",
                app.inspect
                    .tree_path
                    .as_ref()
                    .map_or("", |path| path.display.as_str())
            ),
            Style::default().fg(muted()),
        ));
    }
    if app.inspect.loading {
        spans.push(Span::styled("  loading…", Style::default().fg(warning())));
    }
    match (
        app.history_loading && matches!(app.view, View::Log | View::Detail),
        app.preview_loading
            && matches!(
                app.view,
                View::Log | View::Detail | View::Refs | View::Blame | View::Stash
            ),
    ) {
        (true, true) => spans.push(Span::styled(
            "  loading history+detail…",
            Style::default().fg(warning()),
        )),
        (true, false) => spans.push(Span::styled(
            "  loading history…",
            Style::default().fg(warning()),
        )),
        (false, true) => spans.push(Span::styled(
            "  loading detail…",
            Style::default().fg(warning()),
        )),
        (false, false) => {}
    }
    if app.has_errors() {
        spans.push(Span::styled(
            "  request failed",
            Style::default().fg(error_color()).bold(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub(super) fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(selection) = &app.selection_contract {
        let contract = format!(
            " SELECT {} · {} emit · {} cancel",
            selection.target.label(),
            selection.accept_key,
            selection.cancel_keys
        );
        let full_navigation = match app.view {
            View::Log | View::Refs | View::Blame | View::Stash | View::Status | View::Tree => {
                " · j/k move · / search · p preview"
            }
            View::Detail | View::StatusDiff => " · j/k scroll · [/] hunk · Tab file",
            View::Compare => " · j/k scroll · [/] hunk · x swap",
            View::Blob => " · j/k scroll · / search",
        };
        let compact_navigation = match app.view {
            View::Log | View::Refs | View::Blame | View::Stash | View::Status | View::Tree => {
                " · j/k move"
            }
            View::Detail | View::Compare | View::StatusDiff | View::Blob => " · j/k scroll",
        };
        let navigation = if contract.chars().count() + full_navigation.chars().count()
            <= usize::from(area.width)
        {
            full_navigation
        } else {
            compact_navigation
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(contract, Style::default().fg(accent()).bold()),
                Span::styled(navigation, Style::default().fg(muted())),
            ])),
            area,
        );
        return;
    }

    let position = match app.view {
        View::Log => {
            if app.commits.is_empty() {
                "0/0".into()
            } else {
                format!("{}/{}", app.selected + 1, app.commits.len())
            }
        }
        View::Refs => format!(
            "{}/{}",
            app.inspect.selected.saturating_add(1),
            app.inspect.refs.len()
        ),
        View::Status => format!(
            "{}/{}",
            app.inspect.selected.saturating_add(1),
            app.inspect.status_entries().len()
        ),
        View::Tree => format!(
            "{}/{}",
            app.inspect.selected.saturating_add(1),
            app.inspect.tree.len()
        ),
        View::Blame => format!(
            "{}/{}",
            app.inspect.selected.saturating_add(1),
            app.inspect.blame.len()
        ),
        View::Stash => format!(
            "{}/{}",
            app.inspect.selected.saturating_add(1),
            app.inspect.stashes.len()
        ),
        View::Blob | View::Detail | View::Compare | View::StatusDiff => {
            format!("line {}", app.diff_scroll.saturating_add(1))
        }
    };
    if let Some(notice) = &app.notice {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" Notice ", Style::default().fg(Color::Black).bg(accent())),
                Span::raw(" "),
                Span::styled(notice.clone(), Style::default().fg(muted())),
            ])),
            area,
        );
        return;
    }
    let keys = match app.view {
        View::Log => "j/k move  Enter inspect  f files  c compare  / search  ? help  q quit",
        View::Detail => "j/k scroll  f files  [/] hunk  Tab file  P parent  b blame  q back",
        View::Compare => "j/k scroll  f files  [/] hunk  x swap  M mode  q back",
        View::Refs => "j/k move  Enter history/base  c compare  p preview  / search  q back",
        View::Status if app.inspect.working_diff.is_some() => {
            "diff ready  Enter inspect  j/k move  f files  d staged/unstaged  p preview  q back"
        }
        View::Status => "loading diff  j/k move  d staged/unstaged  p preview  q back",
        View::StatusDiff => "j/k scroll  f files  [/] hunk  {/} file  b blame  q back",
        View::Tree => "j/k move  Enter open  Backspace up  b blame  q back",
        View::Blob => "j/k scroll  b blame  / search  q back",
        View::Blame => "j/k move  Enter commit  p preview  / search  q back",
        View::Stash => "j/k move  Enter patch  p preview  / search  q back",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {position} "),
                Style::default().fg(Color::Black).bg(muted()),
            ),
            Span::raw(" "),
            Span::styled(keys, Style::default().fg(muted())),
            Span::styled(
                app.marked_oid
                    .as_ref()
                    .map_or(String::new(), |oid| format!("  marked:{}", oid.short(10))),
                Style::default().fg(Color::Magenta),
            ),
        ])),
        area,
    );
}

pub(super) fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let popup = centered_rect(
        72.min(area.width.saturating_sub(4)),
        20.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let context = match app.view {
        View::Log => "Log: Enter inspect · v mark · c compare · p preview",
        View::Detail => "Diff: f file picker · [/] hunks · Tab files · P merge parent",
        View::Compare => "Compare: f file picker · x swap · M exact/merge-base",
        View::Refs => "Refs: Enter opens history or accepts comparison base",
        View::Status => "Status is read-only; Enter opens the selected working diff",
        View::StatusDiff => "Working diff: f file picker · [/] hunks · q returns to status",
        View::Tree => "Tree: Enter descends/opens · Backspace ascends",
        View::Blob => "Blob: sanitized text or a safe binary summary",
        View::Blame => "Blame: Enter opens the selected line's commit",
        View::Stash => "Stash: Enter opens the selected stash patch",
    };
    let text = Text::from(vec![
        Line::styled("phig keys", Style::default().fg(accent()).bold()),
        Line::raw(""),
        Line::raw("j/k or ↑/↓       move / scroll"),
        Line::raw("Ctrl-d/u PgDn/Up page"),
        Line::raw("g/G Home/End     first / last"),
        Line::raw("Enter / q        open / back / quit"),
        Line::raw("/ · n/N          search · next/previous"),
        Line::raw(":                searchable command palette"),
        Line::raw("f                searchable changed-file picker"),
        Line::raw("[ ] · { }        hunk · file"),
        Line::raw("m/r/s/t/b/z       history · refs · status · tree · blame · stash"),
        Line::raw("v · c · x · M      mark · compare · swap · compare mode"),
        Line::raw("Tab · p · P       section/file · preview · parent"),
        Line::raw("y · Ctrl-l        OSC 52 copy · redraw"),
        Line::raw(""),
        Line::styled(context, Style::default().fg(muted())),
        Line::raw("Esc closes this help"),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(accent())),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

pub(super) fn render_search(frame: &mut Frame<'_>, draft: &str, body: Rect) {
    let area = Rect::new(body.x, body.bottom().saturating_sub(1), body.width, 1);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(accent()).bold()),
            Span::raw(draft.to_owned()),
            Span::styled("  Enter accept · Esc cancel", Style::default().fg(muted())),
        ]))
        .style(Style::default().bg(Color::Black)),
        area,
    );
}

pub(super) fn render_palette(frame: &mut Frame<'_>, draft: &str, selected: usize, area: Rect) {
    let commands = palette_commands(draft);
    let height = (commands.len().min(8) as u16).saturating_add(3);
    let popup = centered_rect(
        64.min(area.width.saturating_sub(4)),
        height.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(popup.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        }));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(":", Style::default().fg(accent()).bold()),
            Span::raw(draft.to_owned()),
        ])),
        inner[0],
    );
    let visible = usize::from(inner[1].height.max(1));
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
        List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .fg(selection_fg())
                .bg(selection_bg())
                .bold(),
        ),
        inner[1],
        &mut state,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Commands · type to filter · Enter run · Esc close ")
            .border_style(Style::default().fg(accent())),
        popup,
    );
}

pub(super) fn render_file_picker(
    frame: &mut Frame<'_>,
    app: &App,
    draft: &str,
    selected: usize,
    area: Rect,
) {
    let files = app.file_picker_entries(draft);
    let height = (files.len().min(12) as u16).saturating_add(3);
    let popup = centered_rect(
        72.min(area.width.saturating_sub(4)),
        height.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let inner = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(popup.inner(
        ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        },
    ));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("file: ", Style::default().fg(accent()).bold()),
            Span::raw(draft.to_owned()),
        ])),
        inner[0],
    );
    let visible = usize::from(inner[1].height.max(1));
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
        List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .fg(selection_fg())
                .bg(selection_bg())
                .bold(),
        ),
        inner[1],
        &mut state,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Changed files · type to filter · Enter jump · Esc close ")
            .border_style(Style::default().fg(accent())),
        popup,
    );
}

pub(super) fn render_errors(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let failures = [&app.history_error, &app.preview_error, &app.inspect_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for failure in &failures {
        lines.push(Line::styled(
            format!("Failed to {}", failure.operation),
            Style::default().fg(error_color()).bold(),
        ));
        lines.push(Line::raw(failure.detail.clone()));
    }
    let height = if failures.len() > 1 { 10 } else { 8 };
    let popup = centered_rect(
        76.min(area.width.saturating_sub(2)),
        height.min(area.height),
        area,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Request error ")
        .border_style(Style::default().fg(error_color()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        sections[0],
    );
    let recovery = if failures
        .iter()
        .any(|failure| failure.operation == "open blame")
    {
        "Esc dismiss · select a file path, then press b"
    } else {
        "r retry failed request(s) · Esc dismiss"
    };
    frame.render_widget(
        Paragraph::new(recovery).style(Style::default().fg(warning())),
        sections[1],
    );
}
