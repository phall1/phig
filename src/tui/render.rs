use std::sync::{
    LazyLock, RwLock,
    atomic::{AtomicI8, Ordering},
};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::{
    app::{App, Focus, Overlay, View, palette_commands},
    domain::{Commit, Diff, DiffLine, DiffLineKind, Oid, RefKind, StatusCode, TreeEntryKind},
    sanitize::sanitize_str,
};

#[derive(Debug, Clone)]
pub struct RenderTheme {
    pub accent: Color,
    pub muted: Color,
    pub added: Color,
    pub removed: Color,
    pub warning: Color,
    pub error: Color,
    pub selection_fg: Color,
    pub selection_bg: Color,
}
impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan,
            muted: Color::DarkGray,
            added: Color::Green,
            removed: Color::Red,
            warning: Color::Yellow,
            error: Color::Red,
            selection_fg: Color::Black,
            selection_bg: Color::Cyan,
        }
    }
}
static THEME: LazyLock<RwLock<RenderTheme>> = LazyLock::new(|| RwLock::new(RenderTheme::default()));
static COLOR_MODE: AtomicI8 = AtomicI8::new(0);
static DATE_MODE: LazyLock<RwLock<String>> = LazyLock::new(|| RwLock::new("relative".into()));
pub fn set_date_mode(mode: &str) {
    *DATE_MODE.write().expect("date mode lock") = mode.to_owned();
}
pub fn set_color_mode(mode: &str) {
    COLOR_MODE.store(
        match mode {
            "never" => -1,
            "always" => 1,
            _ => 0,
        },
        Ordering::SeqCst,
    );
}
pub fn set_theme(theme: RenderTheme) {
    *THEME.write().expect("theme lock") = theme;
}
fn accent() -> Color {
    THEME.read().expect("theme lock").accent
}
fn muted() -> Color {
    THEME.read().expect("theme lock").muted
}
fn added() -> Color {
    THEME.read().expect("theme lock").added
}
fn removed() -> Color {
    THEME.read().expect("theme lock").removed
}
fn warning() -> Color {
    THEME.read().expect("theme lock").warning
}
fn error_color() -> Color {
    THEME.read().expect("theme lock").error
}
fn selection_fg() -> Color {
    THEME.read().expect("theme lock").selection_fg
}
fn selection_bg() -> Color {
    THEME.read().expect("theme lock").selection_bg
}

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
        View::Compare => render_compare(frame, app, rows[1]),
        View::Refs => render_refs(frame, app, rows[1]),
        View::Status => render_status(frame, app, rows[1]),
        View::StatusDiff => render_status_diff(frame, app, rows[1]),
        View::Tree => render_tree(frame, app, rows[1]),
        View::Blob => render_blob(frame, app, rows[1]),
        View::Blame => render_blame(frame, app, rows[1]),
        View::Stash => render_stashes(frame, app, rows[1]),
    }
    render_footer(frame, app, rows[2]);

    if app.has_errors() && matches!(app.overlay, Overlay::None) {
        render_errors(frame, app, rows[1]);
    }
    match &app.overlay {
        Overlay::Help => render_help(frame, app, area),
        Overlay::Search { draft, .. } => render_search(frame, draft, rows[1]),
        Overlay::Palette { draft, selected } => render_palette(frame, draft, *selected, area),
        Overlay::FilePicker {
            draft, selected, ..
        } => render_file_picker(frame, app, draft, *selected, area),
        Overlay::None => {}
    }

    if COLOR_MODE.load(Ordering::SeqCst) < 0
        || (COLOR_MODE.load(Ordering::SeqCst) == 0 && std::env::var_os("NO_COLOR").is_some())
    {
        reset_styles(frame.buffer_mut(), area);
    }
}

fn reset_styles(buffer: &mut Buffer, area: Rect) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buffer[(x, y)].set_style(Style::reset());
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
    if matches!(
        app.view,
        View::Refs | View::Status | View::Blame | View::Stash
    ) {
        return usize::from(list_preview_areas(app, body).0.height.max(1));
    }
    if app.view == View::Tree {
        return usize::from(body.height.max(1));
    }
    let preview = if matches!(
        app.view,
        View::Detail | View::Compare | View::StatusDiff | View::Blob
    ) {
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

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn metadata_height(app: &App, available: u16) -> u16 {
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

fn compact_date(timestamp: i64) -> String {
    let (year, month, day) = civil_from_days(timestamp.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
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
    render_diff_value(
        frame,
        &detail.diff,
        app.diff_scroll,
        area,
        app.focus == Focus::Preview || app.view == View::Detail,
    );
}

fn render_diff_value(frame: &mut Frame<'_>, diff: &Diff, scroll: usize, area: Rect, active: bool) {
    let visible = usize::from(area.height);
    let lines: Vec<Line<'_>> = diff
        .lines
        .iter()
        .skip(scroll)
        .take(visible)
        .map(diff_line)
        .collect();
    let style = if active {
        Style::default()
    } else {
        Style::default().fg(Color::Gray)
    };
    frame.render_widget(Paragraph::new(lines).style(style), area);
    if diff.truncated && area.height > 0 {
        let warning_area = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        frame.render_widget(
            Paragraph::new("diff truncated at configured limit")
                .style(Style::default().fg(warning()).bg(Color::Black)),
            warning_area,
        );
    }
}

fn diff_line(line: &DiffLine) -> Line<'_> {
    let style = match line.kind {
        DiffLineKind::Added => Style::default().fg(added()),
        DiffLineKind::Removed => Style::default().fg(removed()),
        DiffLineKind::HunkHeader => Style::default().fg(accent()).bold(),
        DiffLineKind::FileHeader => Style::default().fg(warning()).bold(),
        DiffLineKind::Context => Style::default(),
        DiffLineKind::Metadata => Style::default().fg(muted()),
    };
    Line::styled(line.text.as_str(), style)
}

fn list_preview_areas(app: &App, area: Rect) -> (Rect, Rect) {
    if !app.show_preview || area.height < 16 || area.width < 72 {
        return (area, Rect::default());
    }
    let parts = if area.width >= 110 {
        Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)]).split(area)
    } else {
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area)
    };
    (parts[0], parts[1])
}

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

fn render_compare(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_refs(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_status_diff(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_blob(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_blame(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_stashes(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn render_search(frame: &mut Frame<'_>, draft: &str, body: Rect) {
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

fn render_file_picker(frame: &mut Frame<'_>, app: &App, draft: &str, selected: usize, area: Rect) {
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

fn render_errors(frame: &mut Frame<'_>, app: &App, area: Rect) {
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

fn display_date(timestamp: i64, timezone: &str) -> String {
    match DATE_MODE.read().expect("date mode lock").as_str() {
        "unix" => timestamp.to_string(),
        "iso" => format_commit_date(timestamp, "+00:00"),
        "local" => format_commit_date(timestamp, timezone),
        _ => relative_age(timestamp),
    }
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

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ratatui::{Terminal, backend::TestBackend};

    use crate::app::{SelectionContract, SelectionTarget};
    use crate::domain::{
        BlameLine, Blob, Commit, CommitDetail, Comparison, ComparisonMode, Diff, DiffFile,
        DiffLine, DiffLineKind, GitPath, HistoryPage, ObjectFormat, Oid, RefInfo, RefKind, RefName,
        Repository, Signature, Status, StatusCode, StatusEntry,
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
                    old_path: Some(GitPath::new(b"src/main.rs".to_vec())),
                    new_path: Some(GitPath::new(b"src/main.rs".to_vec())),
                    hunks: Vec::new(),
                }],
                truncated: false,
            },
        });
        app
    }

    fn status_entry(index: StatusCode, worktree: StatusCode, path: &[u8]) -> StatusEntry {
        StatusEntry {
            index,
            worktree,
            path: GitPath::new(path.to_vec()),
            original_path: None,
            submodule: "N...".into(),
            head_mode: None,
            index_mode: None,
            worktree_mode: None,
            head_oid: None,
            index_oid: None,
            conflict: None,
        }
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
    fn selection_footer_is_explicit_and_adapts_to_narrow_and_normal_widths() {
        let mut app = sample_app();
        app.view = View::Detail;
        app.selection_contract = Some(SelectionContract::default_keys(SelectionTarget::Hunk));
        let narrow = screen(60, 16, &app);
        let narrow_footer = narrow.lines().nth(15).unwrap();
        assert!(
            narrow_footer.contains("SELECT HUNK · Enter emit · Esc/q cancel"),
            "selection contract was clipped at 60 columns: {narrow_footer}"
        );
        assert!(narrow_footer.contains("j/k scroll"));
        assert!(!narrow_footer.contains("q back"));

        for (target, view) in [
            (SelectionTarget::Commit, View::Log),
            (SelectionTarget::Ref, View::Refs),
            (SelectionTarget::File, View::Detail),
            (SelectionTarget::Hunk, View::Detail),
            (SelectionTarget::Line, View::Blame),
            (SelectionTarget::Compare, View::Compare),
        ] {
            app.selection_contract = Some(SelectionContract::default_keys(target));
            app.view = view;
            let normal = screen(80, 16, &app);
            let footer = normal.lines().nth(15).unwrap();
            let expected = format!("SELECT {} · Enter emit · Esc/q cancel", target.label());
            assert!(
                footer.contains(&expected),
                "missing {target:?} selection contract: {footer}"
            );
            assert!(
                footer.contains("j/k"),
                "selection footer lost context navigation: {footer}"
            );
        }
    }

    #[test]
    fn log_footer_keeps_core_actions_visible_at_eighty_columns() {
        let output = screen(80, 16, &sample_app());
        let footer = output.lines().nth(15).unwrap();
        for hint in [
            "j/k move",
            "Enter inspect",
            "c compare",
            "/ search",
            "? help",
            "q quit",
        ] {
            assert!(
                footer.contains(hint),
                "missing footer hint {hint:?}: {footer}"
            );
        }
        assert!(!footer.contains("refs"));
        assert!(!footer.contains("status"));
        assert!(!footer.contains("tree"));
        assert!(!footer.contains("mark"));
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
    fn comparison_and_inspection_views_keep_one_dominant_surface() {
        let mut app = sample_app();
        let base: Oid = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap();
        let head: Oid = "cccccccccccccccccccccccccccccccccccccccc".parse().unwrap();
        app.view = View::Compare;
        app.inspect.comparison = Some(Comparison {
            mode: ComparisonMode::MergeBase,
            requested_base: "main".into(),
            requested_head: "feature".into(),
            resolved_base: base.clone(),
            resolved_head: head,
            merge_base: Some(base),
            ahead: 2,
            behind: 0,
            diff: app.preview.as_ref().unwrap().diff.clone(),
        });
        let compare = screen(100, 28, &app);
        assert!(compare.contains("merge-base(main, feature)"));
        assert!(compare.contains("ahead 2"));
        app.view = View::Refs;
        let refs = screen(60, 16, &app);
        assert!(refs.contains("REFS"));
        assert!(!refs.contains("diff --git"));
    }

    #[test]
    fn status_at_60x16_opens_a_dominant_diff_and_uses_porcelain_codes() {
        let mut app = sample_app();
        app.view = View::Status;
        let _ = app.apply_status(Status {
            entries: vec![status_entry(
                StatusCode::Modified,
                StatusCode::Modified,
                b"mixed.txt",
            )],
            ..Status::default()
        });
        app.apply_working_diff(app.preview.as_ref().unwrap().diff.clone());
        let list = screen(60, 16, &app);
        assert!(list.contains("mixed"));
        assert!(list.contains("MM"));
        assert!(!list.contains("diff --git"));
        let _ = app.update(crate::app::Action::Open, 14);
        let detail = screen(60, 16, &app);
        assert!(detail.contains("STATUS DIFF"));
        assert!(detail.contains("diff --git"));
    }

    #[test]
    fn refs_mark_tree_breadcrumb_and_blame_groups_are_visible() {
        let mut app = sample_app();
        app.marked_oid = Some(app.commits[0].id.clone());
        let marked = screen(100, 28, &app);
        assert!(marked.contains("marked:aaaaaaaaaa"));

        app.view = View::Refs;
        app.inspect.refs = vec![RefInfo {
            full_name: RefName::new(b"refs/heads/feature".to_vec()),
            short_name: RefName::new(b"feature".to_vec()),
            kind: RefKind::LocalBranch,
            target: app.commits[0].id.clone(),
            peeled: None,
            upstream: Some(RefName::new(b"origin/feature".to_vec())),
            subject: "work".into(),
            timestamp: None,
            is_head: false,
        }];
        let refs = screen(100, 28, &app);
        assert!(refs.contains("aaaaaaaa"));
        assert!(refs.contains("origin/feature"));

        app.view = View::Tree;
        app.inspect.tree_path = Some(GitPath::new(b"src/deep".to_vec()));
        let tree = screen(80, 20, &app);
        assert!(tree.contains("/src/deep"));

        app.view = View::Blame;
        app.marked_oid = None;
        app.show_preview = false;
        app.inspect.blame = (1..=2)
            .map(|line| BlameLine {
                final_line: line,
                original_line: line,
                id: app.commits[0].id.clone(),
                author: "Pat Example".into(),
                author_mail: "pat@example.invalid".into(),
                author_time: Some(1_700_000_000),
                summary: "same commit".into(),
                filename: GitPath::new(b"src/lib.rs".to_vec()),
                content: format!("line {line}"),
                boundary: false,
                previous: None,
            })
            .collect();
        let blame = screen(100, 28, &app);
        assert_eq!(blame.matches("aaaaaaaa").count(), 1);
        assert_eq!(blame.matches("2023-11-14").count(), 1);
    }

    #[test]
    fn inspection_preview_layout_and_page_size_follow_width_class() {
        let mut app = sample_app();
        app.view = View::Refs;
        let narrow = Rect::new(0, 1, 60, 26);
        let normal = Rect::new(0, 1, 100, 26);
        let threshold = Rect::new(0, 1, 110, 26);
        let wide = Rect::new(0, 1, 140, 26);
        let (narrow_list, narrow_preview) = list_preview_areas(&app, narrow);
        assert_eq!(narrow_list, narrow);
        assert_eq!(narrow_preview, Rect::default());
        assert_eq!(page_rows(&app, 60, 28), 26);
        let (normal_list, normal_preview) = list_preview_areas(&app, normal);
        assert!(
            normal_preview.y > normal_list.y,
            "normal preview must be stacked"
        );
        assert_eq!(page_rows(&app, 100, 28), 13);
        let (threshold_list, threshold_preview) = list_preview_areas(&app, threshold);
        assert!(threshold_preview.x > threshold_list.x);
        assert_eq!(page_rows(&app, 110, 28), 26);
        let (wide_list, wide_preview) = list_preview_areas(&app, wide);
        assert!(
            wide_preview.x > wide_list.x,
            "wide preview must be right-side"
        );
        assert_eq!(page_rows(&app, 140, 28), 26);
        app.show_preview = false;
        assert_eq!(page_rows(&app, 100, 28), 26);
    }

    #[test]
    fn changed_file_picker_is_visible_and_filterable() {
        let mut app = sample_app();
        let _ = app.update(crate::app::Action::StartFilePicker, 20);
        let output = screen(100, 28, &app);
        assert!(output.contains("Changed files"));
        assert!(output.contains("src/main.rs"));
    }

    #[test]
    fn multiline_blob_renders_and_scrolls_by_raw_lines() {
        let mut app = sample_app();
        app.view = View::Blob;
        app.inspect.blob = Some(Blob {
            id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
            path: Some(crate::domain::GitPath::new(b"notes.txt".to_vec())),
            bytes_base64: STANDARD.encode(b"first line\nsecond \x1b line\nthird line"),
            size: 35,
            binary: Some(false),
            truncated: false,
        });

        let output = screen(60, 16, &app);
        assert!(output.lines().any(|line| line.starts_with("first line")));
        assert!(
            output
                .lines()
                .any(|line| line.starts_with("second \\e line"))
        );
        assert!(output.lines().any(|line| line.starts_with("third line")));
        assert!(!output.contains("first line\\nsecond"));

        let _ = app.update(crate::app::Action::Move(1), 14);
        assert_eq!(app.diff_scroll, 1);
        let scrolled = screen(60, 16, &app);
        assert!(!scrolled.contains("first line"));
        assert!(
            scrolled
                .lines()
                .any(|line| line.starts_with("second \\e line"))
        );
        assert!(scrolled.lines().any(|line| line.starts_with("third line")));
    }

    #[test]
    fn binary_blob_is_summarized_without_rendering_bytes() {
        let mut app = sample_app();
        app.view = View::Blob;
        app.inspect.blob = Some(Blob {
            id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap(),
            path: Some(crate::domain::GitPath::new(b"image.bin".to_vec())),
            bytes_base64: "AAEC".into(),
            size: 3,
            binary: Some(true),
            truncated: false,
        });
        let output = screen(60, 16, &app);
        assert!(output.contains("Binary blob · 3 bytes"));
        assert!(!output.contains('\0'));
    }

    #[test]
    fn missing_blame_path_error_explains_recovery() {
        let mut app = sample_app();
        app.preview = None;
        let _ = app.update(crate::app::Action::ViewBlame, 10);
        let output = screen(80, 20, &app);
        assert!(output.contains("select a file path first"));
        assert!(output.contains("select a file path, then press b"));
    }

    #[test]
    fn no_color_resets_modifiers_as_well_as_colors() {
        use ratatui::style::Modifier;
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer[(0, 0)].set_style(
            Style::default()
                .fg(Color::Red)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        );
        reset_styles(&mut buffer, Rect::new(0, 0, 1, 1));
        let cell = &buffer[(0, 0)];
        assert_eq!(cell.fg, Color::Reset);
        assert_eq!(cell.bg, Color::Reset);
        assert!(cell.modifier.is_empty());
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
