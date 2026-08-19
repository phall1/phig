use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Focus, Overlay, View, palette_commands},
    domain::{Commit, DiffLine, DiffLineKind, Oid},
    sanitize::sanitize_str,
};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, app, rows[0]);
    match app.view {
        View::Log => render_log(frame, app, rows[1]),
        View::Detail => render_detail(frame, app, rows[1]),
    }
    render_footer(frame, app, rows[2]);

    if app.has_errors() && matches!(app.overlay, Overlay::None) {
        render_errors(frame, app, rows[1]);
    }
    match &app.overlay {
        Overlay::Help => render_help(frame, app, area),
        Overlay::Search { draft, .. } => render_search(frame, draft, rows[1]),
        Overlay::Palette { draft, selected } => render_palette(frame, draft, *selected, area),
        Overlay::None => {}
    }

    if std::env::var_os("NO_COLOR").is_some() {
        let buffer = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buffer[(x, y)].set_fg(Color::Reset).set_bg(Color::Reset);
            }
        }
    }
}

fn render_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
    };
    let mut spans = vec![
        Span::styled(
            " phig ",
            Style::default().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::raw(" "),
        Span::styled(repository, Style::default().bold()),
        Span::styled(format!("  {view}  "), Style::default().fg(ACCENT)),
        Span::raw(sanitize_str(&app.revision)),
        Span::styled(format!("  {branch}"), Style::default().fg(MUTED)),
    ];
    if !app.paths.is_empty() {
        let paths = app
            .paths
            .iter()
            .map(|path| path.display.as_str())
            .collect::<Vec<_>>()
            .join(",");
        spans.push(Span::styled(
            format!("  path:{paths}"),
            Style::default().fg(MUTED),
        ));
    }
    match (app.history_loading, app.preview_loading) {
        (true, true) => spans.push(Span::styled(
            "  loading history+detail…",
            Style::default().fg(Color::Yellow),
        )),
        (true, false) => spans.push(Span::styled(
            "  loading history…",
            Style::default().fg(Color::Yellow),
        )),
        (false, true) => spans.push(Span::styled(
            "  loading detail…",
            Style::default().fg(Color::Yellow),
        )),
        (false, false) => {}
    }
    if app.has_errors() {
        spans.push(Span::styled(
            "  request failed",
            Style::default().fg(Color::Red).bold(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_log(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let (history, preview) = log_areas(app, area);
    render_history(frame, app, history);
    if let Some(preview) = preview {
        render_preview(frame, app, preview);
    }
}

fn log_areas(app: &App, area: Rect) -> (Rect, Option<Rect>) {
    let can_preview = app.show_preview && area.height >= 16;
    if can_preview && area.width >= 110 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(area);
        (columns[0], Some(columns[1]))
    } else if can_preview && area.width >= 72 {
        // The 72–109 column contract is intentionally stacked.
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        (rows[0], Some(rows[1]))
    } else {
        (area, None)
    }
}

pub(crate) fn page_rows(app: &App, width: u16, height: u16) -> usize {
    let body = Rect::new(0, 1, width, height.saturating_sub(2));
    if app.view == View::Log && app.focus == Focus::List {
        return usize::from(log_areas(app, body).0.height.max(1));
    }
    let preview = if app.view == View::Detail {
        body
    } else {
        log_areas(app, body).1.unwrap_or(body)
    };
    usize::from(
        preview
            .height
            .saturating_sub(metadata_height(app, preview.height))
            .max(1),
    )
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
                .style(Style::default().fg(MUTED)),
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
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected - start));
    let highlight = if app.focus == Focus::List {
        Style::default().fg(Color::Black).bg(ACCENT).bold()
    } else {
        Style::default().bg(Color::DarkGray)
    };
    let list = List::new(items)
        .highlight_style(highlight)
        .highlight_symbol("› ")
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(list, area, &mut state);
}

fn history_line(commit: &Commit, width: u16, graph: String) -> Line<'static> {
    let author = if width >= 78 {
        truncate(&commit.author.name, 18)
    } else if width >= 54 {
        truncate(&commit.author.name, 10)
    } else {
        String::new()
    };
    let age = relative_age(commit.author.timestamp);
    let decorations = if commit.decorations.is_empty() {
        String::new()
    } else {
        format!(" ({})", truncate(&commit.decorations.join(", "), 22))
    };
    let mut spans = vec![
        Span::styled(graph, Style::default().fg(ACCENT)),
        Span::styled(
            commit.id.short(8).to_owned(),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(format!(" {age:>4} "), Style::default().fg(MUTED)),
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

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.preview_loading && app.preview.is_none() {
        frame.render_widget(
            Paragraph::new("Loading diff…").style(Style::default().fg(MUTED)),
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
            Paragraph::new(message).style(Style::default().fg(MUTED)),
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

fn metadata_height(app: &App, available: u16) -> u16 {
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
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(parent, Style::default().fg(MUTED)),
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
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            format!(
                "Date: {}",
                format_commit_date(
                    detail.commit.author.timestamp,
                    &detail.commit.author.timezone
                )
            ),
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            format!(
                "Parents: {parents} · Files: {} (+{added} -{removed})",
                detail.diff.files.len()
            ),
            Style::default().fg(MUTED),
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

fn format_commit_date(timestamp: i64, timezone: &str) -> String {
    let offset = parse_timezone_offset(timezone);
    let local = timestamp.saturating_add(offset);
    let days = local.div_euclid(86_400);
    let seconds = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} {}",
        seconds / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
        sanitize_str(timezone)
    )
}

fn parse_timezone_offset(timezone: &str) -> i64 {
    let bytes = timezone.as_bytes();
    if !matches!(bytes.first(), Some(b'+' | b'-')) {
        return 0;
    }
    let (hours, minutes) = match bytes {
        [_, h1, h2, b':', m1, m2] | [_, h1, h2, m1, m2] => ([*h1, *h2], [*m1, *m2]),
        _ => return 0,
    };
    let Ok(hours) = std::str::from_utf8(&hours)
        .unwrap_or_default()
        .parse::<i64>()
    else {
        return 0;
    };
    let Ok(minutes) = std::str::from_utf8(&minutes)
        .unwrap_or_default()
        .parse::<i64>()
    else {
        return 0;
    };
    let offset = hours.saturating_mul(3_600) + minutes.saturating_mul(60);
    if bytes[0] == b'-' { -offset } else { offset }
}

// Howard Hinnant's civil-date conversion, with day zero at 1970-01-01.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn render_detail(frame: &mut Frame<'_>, app: &App, area: Rect) {
    render_preview(frame, app, area);
}

fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(detail) = &app.preview else {
        return;
    };
    let visible = usize::from(area.height);
    let lines: Vec<Line<'_>> = detail
        .diff
        .lines
        .iter()
        .skip(app.diff_scroll)
        .take(visible)
        .map(diff_line)
        .collect();
    let style = if app.focus == Focus::Preview || app.view == View::Detail {
        Style::default()
    } else {
        Style::default().fg(Color::Gray)
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
    if detail.diff.truncated && area.height > 0 {
        let warning = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        frame.render_widget(
            Paragraph::new("diff truncated at configured limit")
                .style(Style::default().fg(Color::Yellow).bg(Color::Black)),
            warning,
        );
    }
}

fn diff_line(line: &DiffLine) -> Line<'_> {
    let style = match line.kind {
        DiffLineKind::Added => Style::default().fg(Color::Green),
        DiffLineKind::Removed => Style::default().fg(Color::Red),
        DiffLineKind::HunkHeader => Style::default().fg(ACCENT).bold(),
        DiffLineKind::FileHeader => Style::default().fg(Color::Yellow).bold(),
        DiffLineKind::Context => Style::default(),
        DiffLineKind::Metadata => Style::default().fg(MUTED),
    };
    Line::styled(line.text.as_str(), style)
}

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let position = if app.view == View::Log {
        if app.commits.is_empty() {
            "0/0".to_owned()
        } else {
            format!("{}/{}", app.selected + 1, app.commits.len())
        }
    } else if let Some(detail) = &app.preview {
        format!(
            "line {}/{}",
            app.diff_scroll.saturating_add(1),
            detail.diff.lines.len()
        )
    } else {
        "loading".to_owned()
    };
    let keys = match app.view {
        View::Log => "j/k move  Enter inspect  p preview  / search  : commands  ? help  q quit",
        View::Detail => "j/k scroll  [/] hunk  Tab file  P parent  : commands  q back",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {position} "),
                Style::default().fg(Color::Black).bg(MUTED),
            ),
            Span::raw(" "),
            Span::styled(keys, Style::default().fg(MUTED)),
        ])),
        area,
    );
}

fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let popup = centered_rect(
        72.min(area.width.saturating_sub(4)),
        18.min(area.height.saturating_sub(2)),
        area,
    );
    frame.render_widget(Clear, popup);
    let context = match app.view {
        View::Log => "Log: Enter inspect · Tab focus preview · p toggle preview",
        View::Detail => "Diff: [/] hunks · Tab files · P cycles merge parents",
    };
    let text = Text::from(vec![
        Line::styled("phig keys", Style::default().fg(ACCENT).bold()),
        Line::raw(""),
        Line::raw("j/k or ↑/↓       move / scroll"),
        Line::raw("Ctrl-d/u PgDn/Up page"),
        Line::raw("g/G Home/End     first / last"),
        Line::raw("Enter / q        open / back / quit"),
        Line::raw("/ · n/N          search · next/previous"),
        Line::raw(":                searchable command palette"),
        Line::raw("[ ] · { }        hunk · file"),
        Line::raw("Tab · p · P       section/file · preview · parent"),
        Line::raw("Ctrl-l            redraw"),
        Line::raw(""),
        Line::styled(context, Style::default().fg(MUTED)),
        Line::raw("Esc closes this help"),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(ACCENT)),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_search(frame: &mut Frame<'_>, draft: &str, body: Rect) {
    let area = Rect::new(body.x, body.bottom().saturating_sub(1), body.width, 1);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(ACCENT).bold()),
            Span::raw(draft.to_owned()),
            Span::styled("  Enter accept · Esc cancel", Style::default().fg(MUTED)),
        ]))
        .style(Style::default().bg(Color::Black)),
        area,
    );
}

fn render_palette(frame: &mut Frame<'_>, draft: &str, selected: usize, area: Rect) {
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
            Span::styled(":", Style::default().fg(ACCENT).bold()),
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
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(Style::default().fg(Color::Black).bg(ACCENT).bold()),
        inner[1],
        &mut state,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Commands · type to filter · Enter run · Esc close ")
            .border_style(Style::default().fg(ACCENT)),
        popup,
    );
}

fn render_errors(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let failures = [&app.history_error, &app.preview_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut lines = Vec::new();
    for failure in &failures {
        lines.push(Line::styled(
            format!("Failed to {}", failure.operation),
            Style::default().fg(Color::Red).bold(),
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
        .border_style(Style::default().fg(Color::Red));
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
    frame.render_widget(
        Paragraph::new("r retry failed request(s) · Esc dismiss")
            .style(Style::default().fg(Color::Yellow)),
        sections[1],
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.max(1).min(area.width);
    let height = height.max(1).min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    let mut output: String = value.chars().take(width.saturating_sub(1)).collect();
    output.push('…');
    output
}

fn relative_age(timestamp: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64);
    let seconds = now.saturating_sub(timestamp).max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else if seconds < 2_592_000 {
        format!("{}d", seconds / 86_400)
    } else if seconds < 31_536_000 {
        format!("{}mo", seconds / 2_592_000)
    } else {
        format!("{}y", seconds / 31_536_000)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ratatui::{Terminal, backend::TestBackend};

    use crate::domain::{
        Commit, CommitDetail, Diff, DiffFile, DiffLine, DiffLineKind, HistoryPage, ObjectFormat,
        Oid, Repository, Signature,
    };

    use super::*;

    fn sample_app() -> App {
        let oid: Oid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap();
        let commit = Commit {
            id: oid.clone(),
            parents: Vec::new(),
            author: Signature {
                name: "Pat Example".into(),
                email: "pat@example.invalid".into(),
                timestamp: 1_700_000_000,
                timezone: "-04:00".into(),
            },
            committer: Signature {
                name: "Pat Example".into(),
                email: "pat@example.invalid".into(),
                timestamp: 1_700_000_000,
                timezone: "Z".into(),
            },
            decorations: vec!["HEAD -> main".into()],
            subject: "make history pleasant".into(),
            body: "Explain the change safely.\n\nSecond body paragraph.".into(),
        };
        let repository = Repository {
            root: PathBuf::from("/tmp/phig-demo"),
            worktree: Some(PathBuf::from("/tmp/phig-demo")),
            git_dir: PathBuf::from("/tmp/phig-demo/.git"),
            bare: false,
            object_format: ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: Some(oid.clone()),
            branch: Some("main".into()),
        };
        let mut app = App::new(repository, "HEAD".into(), Vec::new(), false);
        app.apply_history(HistoryPage {
            commits: vec![commit.clone()],
            offset: 0,
            limit: 256,
            has_more: false,
        });
        app.apply_preview(CommitDetail {
            commit,
            selected_parent: None,
            diff: Diff {
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::FileHeader,
                        text: "diff --git a/src/main.rs b/src/main.rs".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::HunkHeader,
                        text: "@@ -1 +1 @@".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "-old".into(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "+new".into(),
                    },
                ],
                files: vec![DiffFile {
                    header_line: 0,
                    old_path: None,
                    new_path: None,
                    hunks: Vec::new(),
                }],
                truncated: false,
            },
        });
        app
    }

    fn screen(width: u16, height: u16, app: &App) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut output = String::new();
        for y in 0..height {
            for x in 0..width {
                output.push_str(buffer[(x, y)].symbol());
            }
            output.push('\n');
        }
        output
    }

    #[test]
    fn narrow_layout_keeps_one_dominant_surface() {
        let output = screen(60, 16, &sample_app());
        assert!(output.contains("phig"));
        assert!(output.contains("make history pleasant"));
        assert!(!output.contains("diff --git"));
        assert!(output.contains("j/k move"));
    }

    #[test]
    fn normal_and_wide_layouts_show_contextual_preview() {
        let normal = screen(100, 28, &sample_app());
        let wide = screen(140, 40, &sample_app());
        assert!(normal.contains("diff --git"));
        assert!(wide.contains("diff --git"));
        assert!(wide.contains("Pat Example"));
    }

    #[test]
    fn commit_detail_uses_available_height_for_metadata_and_body() {
        let mut app = sample_app();
        app.view = View::Detail;
        let output = screen(100, 30, &app);
        assert!(output.contains("Date: 2023-11-14 18:13:20 -04:00"));
        assert!(output.contains("Parents: root · Files: 1 (+1 -1)"));
        assert!(output.contains("Explain the change safely."));
        assert!(output.contains("Second body paragraph."));
    }

    #[test]
    fn merge_history_renders_bounded_topology_lanes() {
        let mut app = sample_app();
        let first = app.commits[0].clone();
        let mut left = first.clone();
        left.id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap();
        let mut right = first.clone();
        right.id = "cccccccccccccccccccccccccccccccccccccccc".parse().unwrap();
        let mut merge = first;
        merge.id = "dddddddddddddddddddddddddddddddddddddddd".parse().unwrap();
        merge.parents = vec![left.id.clone(), right.id.clone()];
        merge.subject = "merge side branch".into();
        left.parents = vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap()];
        right.parents = vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap()];
        app.commits = vec![merge, left, right];
        app.selected = 0;
        let output = screen(60, 16, &app);
        assert!(output.contains('◆'), "merge commit had no topology marker");
        assert!(output.contains('│'), "branch lane was not visible");
    }

    #[test]
    fn incremental_search_overlay_renders_its_draft() {
        let mut app = sample_app();
        let _ = app.update(crate::app::Action::StartSearch, 10);
        for character in "pleasant".chars() {
            let _ = app.update(crate::app::Action::SearchInput(character), 10);
        }
        let output = screen(100, 28, &app);
        assert!(output.contains("/pleasant"));
        assert!(output.contains("Enter accept"));
    }

    #[test]
    fn command_palette_is_searchable_and_error_panel_is_actionable() {
        let mut app = sample_app();
        let _ = app.update(crate::app::Action::StartPalette, 10);
        for character in "toggle preview".chars() {
            let _ = app.update(crate::app::Action::SearchInput(character), 10);
        }
        let palette = screen(100, 28, &app);
        assert!(palette.contains("Commands"));
        assert!(palette.contains("Toggle preview"));

        let _ = app.update(crate::app::Action::CancelOverlay, 10);
        app.apply_error(
            crate::app::RequestKind::Preview,
            &crate::git::GitError::Timeout("commit-detail"),
        );
        let error = screen(52, 20, &app);
        assert!(error.contains("Failed to load commit detail"));
        assert!(error.contains("timed out"));
        assert!(error.contains("r retry failed request(s)"));
    }

    #[test]
    fn help_overlay_is_contextual() {
        let mut app = sample_app();
        app.show_help();
        let output = screen(100, 28, &app);
        assert!(output.contains("phig keys"));
        assert!(output.contains("Log: Enter inspect"));
    }
}
