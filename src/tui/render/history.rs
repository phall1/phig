//! Commit history, graph lanes, metadata, and preview rendering.

use std::collections::{HashMap, HashSet};

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::{
    app::{App, Focus, View},
    domain::{Commit, DiffLineKind},
    sanitize::sanitize_str,
};

use super::{
    decorate::{decoration_spans, lane_name, spans_width},
    diff::render_diff,
    format::{
        display_width, highlight_matches, list_date, pad_left, pad_right, relative_age,
        truncate_with, wrap_words,
    },
    graph::{GraphCache, GraphRow, lane_limit},
    layout::log_layout,
    render_divider, render_notice,
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
        let ellipsis = context.glyphs().ellipsis;
        let message = if app.history_loading {
            format!("Loading history{ellipsis}")
        } else if app.history_error.is_some() {
            format!(
                "History unavailable {} retry or dismiss",
                context.glyphs().dash
            )
        } else {
            "No commits in this history".to_owned()
        };
        render_notice(frame, area, &message, context);
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
    let columns = LogColumns::plan(&app.commits[start..end], graph_width, area.width, context);
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
                    columns,
                    marked: app.marked_oid.as_ref() == Some(&commit.id),
                    selected: index == app.selected,
                    selected_branch,
                    search: &app.search_query,
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

/// Hash cells: eight hex digits plus the gap before the next column.
const HASH_WIDTH: usize = 9;
/// Author names beyond this are truncated; the subject deserves the room.
const AUTHOR_MAX: usize = 16;
const AUTHOR_COMPACT: usize = 8;
/// Subject cells each optional column must leave behind to be shown.
const SUBJECT_WITH_AUTHOR: usize = 32;
const SUBJECT_WITH_DATE: usize = 16;
/// Narrowest room worth giving a named ref.
const MIN_REF_CELLS: usize = 7;

/// Column widths shared by every row on screen, so hash, date, author, and
/// subject start at the same cell on every row regardless of named refs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LogColumns {
    pub graph: usize,
    pub date: usize,
    pub author: usize,
}

impl LogColumns {
    pub(super) fn plan(
        commits: &[Commit],
        graph: usize,
        width: u16,
        context: &RenderContext,
    ) -> Self {
        let gutter = display_width(context.glyphs().selected);
        let available = usize::from(width).saturating_sub(gutter + graph + HASH_WIDTH);
        let date = commits
            .iter()
            .map(|commit| display_width(&commit_date(commit, context)))
            .max()
            .unwrap_or(0);
        let names = || {
            commits
                .iter()
                .map(|commit| sanitize_str(&commit.author.name))
        };
        let author = names()
            .map(|name| display_width(&name))
            .max()
            .unwrap_or(0)
            .min(AUTHOR_MAX);
        let short = names()
            .map(|name| {
                display_width(&short_name(
                    &name,
                    AUTHOR_COMPACT,
                    context.glyphs().ellipsis,
                ))
            })
            .max()
            .unwrap_or(0);
        let fits = |used: usize, subject: usize| used + subject <= available;
        let (date, author) = if fits(date + 1 + author + 1, SUBJECT_WITH_AUTHOR) {
            (date, author)
        } else if fits(date + 1 + short + 1, SUBJECT_WITH_AUTHOR) {
            (date, short)
        } else if fits(date + 1, SUBJECT_WITH_DATE) {
            (date, 0)
        } else {
            (0, 0)
        };
        Self {
            graph,
            date,
            author,
        }
    }

    fn fixed_width(self, gutter: usize) -> usize {
        gutter
            + self.graph
            + HASH_WIDTH
            + if self.date > 0 { self.date + 1 } else { 0 }
            + if self.author > 0 { self.author + 1 } else { 0 }
    }
}

/// A name that fits `width`: the whole name, else the first name, else a
/// clipped first name. `Maya` reads better than `Maya Ch…`.
fn short_name(name: &str, width: usize, ellipsis: &str) -> String {
    if display_width(name) <= width {
        return name.to_owned();
    }
    let first = name.split_whitespace().next().unwrap_or(name);
    truncate_with(first, width, ellipsis)
}

fn commit_date(commit: &Commit, context: &RenderContext) -> String {
    list_date(
        commit.author.timestamp,
        &commit.author.timezone,
        context.config().date_mode,
    )
}

pub(super) struct HistoryLineOpts<'a> {
    pub width: u16,
    pub columns: LogColumns,
    pub marked: bool,
    pub selected: bool,
    pub selected_branch: Option<usize>,
    pub inherited_label: Option<&'a str>,
    /// Active search query, highlighted where it matches.
    pub search: &'a str,
}

/// One log row: `gutter graph hash date author refs subject`.
///
/// The selection marker lives in the row so it can stay accent-colored
/// without List highlight replacing graph and decoration colors. Named refs
/// sit inline before the subject, where they cannot shift the aligned
/// metadata columns.
pub(super) fn history_line(
    commit: &Commit,
    graph: &GraphRow,
    opts: HistoryLineOpts<'_>,
    context: &RenderContext,
) -> Line<'static> {
    let glyphs = context.glyphs();
    let gutter_width = display_width(glyphs.selected);
    let columns = LogColumns {
        graph: opts.columns.graph.max(graph.width()),
        ..opts.columns
    };
    let muted = context.style(context.muted());
    let gutter = if opts.selected {
        Span::styled(glyphs.selected.to_owned(), context.strong(context.accent()))
    } else if opts.marked {
        Span::styled(glyphs.marked.to_owned(), context.strong(context.accent()))
    } else {
        Span::raw(" ".repeat(gutter_width))
    };
    let mut spans = vec![gutter];
    spans.extend(match opts.selected_branch {
        Some(color) => graph.spans_with_highlight(context, Some(color)),
        None => graph.spans(context),
    });
    spans.push(Span::raw(
        " ".repeat(columns.graph.saturating_sub(graph.width())),
    ));
    let hash_style = if opts.marked {
        context.emphasize(context.strong(context.accent()), Modifier::UNDERLINED)
    } else {
        context.style(context.warning())
    };
    // Search hits are marked in the fields log search reads: id, author,
    // and subject.
    let hit =
        |span: Span<'static>| highlight_matches(vec![span], opts.search, context.search_hit());
    spans.extend(hit(Span::styled(commit.id.short(8).to_owned(), hash_style)));
    spans.push(Span::raw(" "));
    if columns.date > 0 {
        spans.push(Span::styled(
            format!("{} ", pad_left(&commit_date(commit, context), columns.date)),
            muted,
        ));
    }
    if columns.author > 0 {
        let name = short_name(
            &sanitize_str(&commit.author.name),
            columns.author,
            glyphs.ellipsis,
        );
        spans.extend(hit(Span::styled(
            format!("{} ", pad_right(&name, columns.author)),
            muted,
        )));
    }

    let mut remaining = usize::from(opts.width).saturating_sub(columns.fixed_width(gutter_width));
    let subject = sanitize_str(&commit.subject);
    let minimum_subject = display_width(&subject).min(16);
    // A ref squeezed below a few cells is noise (`H…`); leave it out.
    let ref_budget = remaining.saturating_sub(minimum_subject + 1);
    let ref_budget = if ref_budget < MIN_REF_CELLS {
        0
    } else {
        ref_budget
    };
    let mut decorations = decoration_spans(
        &commit.decorations,
        opts.inherited_label,
        ref_budget,
        Some(graph.color()),
        context,
    );
    if !decorations.is_empty() {
        decorations.push(Span::raw(" "));
        remaining = remaining.saturating_sub(spans_width(&decorations));
    }
    spans.extend(decorations);
    spans.extend(hit(Span::raw(truncate_with(
        &subject,
        remaining,
        glyphs.ellipsis,
    ))));
    Line::from(spans)
}

pub(super) fn render_preview(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    if app.preview.is_none() {
        let message = if app.preview_loading {
            format!("Loading diff{}", context.glyphs().ellipsis)
        } else if app.preview_error.is_some() {
            format!(
                "Commit detail unavailable {} retry or dismiss",
                context.glyphs().dash
            )
        } else {
            "Select a commit to preview its diff".to_owned()
        };
        render_notice(frame, area, &message, context);
        return;
    }
    let header_height = metadata_height(app, area.width, area.height);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(area);
    let metadata = detail_metadata(app, area, context);
    frame.render_widget(Paragraph::new(metadata), sections[0]);
    render_diff(frame, app, sections[1], context);
}

/// Commit metadata above a patch, pre-wrapped so height and paint agree.
struct MetadataPlan {
    subject: Vec<String>,
    body: Vec<String>,
    hidden_body: usize,
}

/// Fixed rows around the wrapped subject: ids, byline, and stats.
const METADATA_FIXED_ROWS: usize = 3;
/// Patch rows the header must always leave visible.
const METADATA_MIN_PATCH_ROWS: usize = 3;

impl MetadataPlan {
    fn new(app: &App, width: u16, available: u16) -> Option<Self> {
        if app.view != View::Detail && app.view != View::Log {
            return None;
        }
        let detail = app.preview.as_ref()?;
        let width = usize::from(width.max(1));
        let available = usize::from(available);
        let detail_view = app.view == View::Detail;
        let mut subject = wrap_words(&sanitize_str(&detail.commit.subject), width);
        subject.truncate(if detail_view { 3 } else { 2 });
        let fixed = METADATA_FIXED_ROWS + subject.len() + 1;
        let wanted = if detail_view {
            (available * 2 / 5).max(4)
        } else {
            3
        };
        // One blank row separates body from the stats line.
        let budget = wanted.min(
            available
                .saturating_sub(fixed + METADATA_MIN_PATCH_ROWS)
                .saturating_sub(1),
        );
        let mut body = body_rows(&detail.commit.body, width);
        let hidden_body = body.len().saturating_sub(budget);
        body.truncate(budget);
        Some(Self {
            subject,
            body,
            hidden_body,
        })
    }

    fn height(&self) -> usize {
        let body = if self.body.is_empty() {
            0
        } else {
            self.body.len() + 1
        };
        METADATA_FIXED_ROWS + self.subject.len() + body + 1
    }
}

/// Wrap a commit body for a pane. Hard-wrapped paragraphs are reflowed only
/// when the pane is narrower than their lines; lists, indented blocks, and
/// trailers keep their own lines.
fn body_rows(body: &str, width: usize) -> Vec<String> {
    let lines: Vec<String> = body
        .lines()
        .map(|line| sanitize_str(line.trim_end()))
        .collect();
    let reflow = lines.iter().any(|line| display_width(line) > width);
    let mut paragraphs: Vec<String> = Vec::new();
    let mut joinable = false;
    for line in lines {
        if line.trim().is_empty() {
            if paragraphs.last().is_some_and(|last| !last.is_empty()) {
                paragraphs.push(String::new());
            }
            joinable = false;
            continue;
        }
        let structural = line.starts_with([' ', '\t', '-', '*', '•', '>'])
            || line.split_once(": ").is_some_and(|(key, _)| {
                !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            || line
                .split_once(". ")
                .is_some_and(|(number, _)| number.chars().all(|c| c.is_ascii_digit()));
        let trailer = line.contains(": ");
        match paragraphs.last_mut() {
            Some(last) if reflow && joinable && !structural => {
                last.push(' ');
                last.push_str(line.trim_start());
            }
            _ => paragraphs.push(line),
        }
        joinable = !structural || !trailer;
    }
    while paragraphs.last().is_some_and(String::is_empty) {
        paragraphs.pop();
    }
    paragraphs
        .iter()
        .flat_map(|paragraph| wrap_words(paragraph, width))
        .collect()
}

pub(super) fn metadata_height(app: &App, width: u16, available: u16) -> u16 {
    MetadataPlan::new(app, width, available).map_or(0, |plan| {
        u16::try_from(plan.height())
            .unwrap_or(u16::MAX)
            .min(available.saturating_sub(METADATA_MIN_PATCH_ROWS as u16))
    })
}

fn detail_metadata(app: &App, area: Rect, context: &RenderContext) -> Vec<Line<'static>> {
    let (Some(detail), Some(plan)) = (
        &app.preview,
        MetadataPlan::new(app, area.width, area.height),
    ) else {
        return Vec::new();
    };
    let glyphs = context.glyphs();
    let separator = format!(" {} ", glyphs.separator);
    let muted = context.style(context.muted());
    let width = usize::from(area.width);
    let commit = &detail.commit;

    let mut ids = vec![Span::styled(
        commit.id.short(12).to_owned(),
        context.strong(context.warning()),
    )];
    let decorations = decoration_spans(
        &commit.decorations,
        None,
        width.saturating_sub(14),
        None,
        context,
    );
    if !decorations.is_empty() {
        ids.push(Span::raw("  "));
        ids.extend(decorations);
    }

    let subject_style = if context.is_monochrome() {
        Style::reset()
    } else {
        Style::default().bold()
    };
    let byline = format!(
        "{}{separator}{} ago{separator}{} {}{separator}{}",
        sanitize_str(&commit.author.name),
        relative_age(commit.author.timestamp),
        list_date(
            commit.author.timestamp,
            &commit.author.timezone,
            crate::tui::render::DateMode::Local
        ),
        sanitize_str(&commit.author.timezone),
        sanitize_str(&commit.author.email),
    );

    let mut lines = vec![Line::from(ids)];
    lines.extend(
        plan.subject
            .iter()
            .map(|row| Line::styled(row.clone(), subject_style)),
    );
    lines.push(Line::styled(
        truncate_with(&byline, width, glyphs.ellipsis),
        muted,
    ));
    lines.push(stats_line(app, detail, (&separator, width), context));
    if !plan.body.is_empty() {
        lines.push(Line::raw(""));
        let last = plan.body.len() - 1;
        for (index, row) in plan.body.iter().enumerate() {
            if index == last && plan.hidden_body > 0 {
                lines.push(Line::styled(
                    format!("{} {} more lines", glyphs.ellipsis, plan.hidden_body + 1),
                    muted,
                ));
            } else {
                lines.push(Line::raw(row.clone()));
            }
        }
    }
    lines
}

fn stats_line(
    app: &App,
    detail: &crate::domain::CommitDetail,
    (separator, width): (&str, usize),
    context: &RenderContext,
) -> Line<'static> {
    let muted = context.style(context.muted());
    let (added, removed) =
        detail
            .diff
            .lines
            .iter()
            .fold((0, 0), |(added, removed), line| match line.kind {
                DiffLineKind::Added => (added + 1, removed),
                DiffLineKind::Removed => (added, removed + 1),
                _ => (added, removed),
            });
    let files = detail.diff.files.len();
    let mut totals = vec![
        Span::styled(
            format!(
                "{separator}{files} file{}{separator}",
                if files == 1 { "" } else { "s" }
            ),
            muted,
        ),
        Span::styled(format!("+{added}"), context.style(context.added())),
        Span::raw(" "),
        Span::styled(format!("-{removed}"), context.style(context.removed())),
    ];
    // Totals always fit; the parent description gives way from its least
    // useful part when the pane is narrow.
    let room = width.saturating_sub(spans_width(&totals));
    let mut spans = parent_spans(app, &detail.commit.parents, room, context);
    spans.append(&mut totals);
    Line::from(spans)
}

fn parent_spans(
    app: &App,
    parents: &[crate::domain::Oid],
    room: usize,
    context: &RenderContext,
) -> Vec<Span<'static>> {
    let muted = context.style(context.muted());
    let oid = context.style(context.warning());
    match parents.len() {
        0 => return vec![Span::styled("root commit".to_owned(), muted)],
        1 => {
            return vec![
                Span::styled("parent ".to_owned(), muted),
                Span::styled(parents[0].short(8).to_owned(), oid),
            ];
        }
        _ => {}
    }
    let count = parents.len();
    let mut spans = vec![Span::styled("merge ".to_owned(), muted)];
    for (index, parent) in parents.iter().enumerate() {
        let style = if index == app.parent_index {
            context.emphasize(context.strong(context.warning()), Modifier::UNDERLINED)
        } else {
            oid
        };
        if index > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(parent.short(8).to_owned(), style));
    }
    let hint = Span::styled(
        format!(
            "  parent {}/{count} ({})",
            app.parent_index.saturating_add(1),
            context.key(&crate::app::Action::NextParent)
        ),
        muted,
    );
    if spans_width(&spans) + spans_width(std::slice::from_ref(&hint)) <= room {
        spans.push(hint);
        return spans;
    }
    if spans_width(&spans) <= room {
        return spans;
    }
    vec![Span::styled(
        format!(
            "merge, parent {}/{count}",
            app.parent_index.saturating_add(1)
        ),
        muted,
    )]
}
