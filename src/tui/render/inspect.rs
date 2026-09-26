//! Refs, status, compare, tree, blob, blame, and stash rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
};

use crate::{
    app::App,
    domain::{RefInfo, RefKind, StatusCode, TreeEntryKind},
    sanitize::sanitize_str,
};

use super::{
    diff::render_diff_value,
    format::{display_width, highlight_matches, pad_left, pad_right, relative_age, truncate_with},
    history::render_preview,
    layout::{COMPARE_HEADER_ROWS, STATUS_DIFF_HEADER_ROWS, list_preview_layout},
    render_divider, render_notice,
    theme::RenderContext,
};

/// A whole-body empty state: no divider or preview for a list with nothing
/// in it. Returns false when the list has rows to draw instead.
fn render_empty(
    frame: &mut Frame<'_>,
    app: &App,
    empty: bool,
    message: &str,
    area: Rect,
    context: &RenderContext,
) -> bool {
    if !empty {
        return false;
    }
    let text = if app.inspect.loading {
        format!("Loading{}", context.glyphs().ellipsis)
    } else if app.inspect_error.is_some() {
        format!("Unavailable {} retry or dismiss", context.glyphs().dash)
    } else {
        message.to_owned()
    };
    render_notice(frame, area, &text, context);
    true
}

fn render_string_list(
    frame: &mut Frame<'_>,
    rows: Vec<Line<'static>>,
    selected: usize,
    area: Rect,
    context: &RenderContext,
) {
    if rows.is_empty() {
        render_notice(frame, area, "Nothing here", context);
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
        List::new(items)
            .highlight_symbol(context.glyphs().selected)
            .highlight_style(context.selection_style(true)),
        area,
        &mut state,
    );
}

pub(super) fn render_compare(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    let Some(comparison) = &app.inspect.comparison else {
        let message = if app.inspect.loading {
            format!("Resolving comparison{}", context.glyphs().ellipsis)
        } else {
            "Comparison unavailable".into()
        };
        render_notice(frame, area, &message, context);
        return;
    };
    let parts =
        Layout::vertical([Constraint::Length(COMPARE_HEADER_ROWS), Constraint::Min(1)]).split(area);
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
    frame.render_widget(
        Paragraph::new(compare_header(comparison, base_label, head_label, context)),
        parts[0],
    );
    render_diff_value(frame, app, parts[1], true, context);
}

/// Whether `name` is just (a prefix of) the object id itself.
fn names_oid(name: &str, id: &crate::domain::Oid) -> bool {
    name.len() >= 7 && id.to_string().starts_with(&name.to_ascii_lowercase())
}

/// Endpoints and semantics first, then the counts that size the change.
/// A trailing blank row separates the header from the patch.
fn compare_header(
    comparison: &crate::domain::Comparison,
    base_label: &str,
    head_label: &str,
    context: &RenderContext,
) -> Vec<Line<'static>> {
    let glyphs = context.glyphs();
    let muted = context.style(context.muted());
    let label = context.strong(context.accent());
    let oid = context.style(context.warning());
    let arrow = Span::styled(format!(" {} ", glyphs.arrow), muted);
    // An endpoint named by its own id needs no `@id` echo.
    let endpoint = |name: &str, id: &crate::domain::Oid| {
        let mut spans = vec![Span::styled(sanitize_str(name), label)];
        if !names_oid(name, id) {
            spans.push(Span::styled(format!("@{}", id.short(10)), oid));
        }
        spans
    };
    let semantics = match comparison.mode {
        crate::domain::ComparisonMode::Exact => {
            let mut spans = vec![Span::styled("exact ".to_owned(), muted)];
            spans.extend(endpoint(base_label, &comparison.resolved_base));
            spans.push(arrow);
            spans.extend(endpoint(head_label, &comparison.resolved_head));
            spans
        }
        crate::domain::ComparisonMode::MergeBase => vec![
            Span::styled("merge-base(".to_owned(), muted),
            Span::styled(sanitize_str(base_label), label),
            Span::styled(", ".to_owned(), muted),
            Span::styled(sanitize_str(head_label), label),
            Span::styled(")=".to_owned(), muted),
            Span::styled(
                comparison
                    .merge_base
                    .as_ref()
                    .map_or("?", |oid| oid.short(10))
                    .to_owned(),
                oid,
            ),
            arrow,
            Span::styled(comparison.resolved_head.short(10).to_owned(), oid),
        ],
    };
    let (added, removed) = comparison
        .diff
        .lines
        .iter()
        .fold((0, 0), |(added, removed), line| match line.kind {
            crate::domain::DiffLineKind::Added => (added + 1, removed),
            crate::domain::DiffLineKind::Removed => (added, removed + 1),
            _ => (added, removed),
        });
    let separator = format!(" {} ", glyphs.separator);
    let files = comparison.diff.files.len();
    let mut counts = vec![
        Span::styled(
            format!("ahead {}", comparison.ahead),
            context.style(context.added()),
        ),
        Span::styled(separator.clone(), muted),
        Span::styled(
            format!("behind {}", comparison.behind),
            context.style(context.removed()),
        ),
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
    let restated = |requested: &str, label: &str, id: &crate::domain::Oid| {
        requested == label || names_oid(requested, id)
    };
    if !restated(
        &comparison.requested_base,
        base_label,
        &comparison.resolved_base,
    ) || !restated(
        &comparison.requested_head,
        head_label,
        &comparison.resolved_head,
    ) {
        counts.push(Span::styled(
            format!(
                "{separator}from {} {} {}",
                sanitize_str(&comparison.requested_base),
                glyphs.arrow,
                sanitize_str(&comparison.requested_head)
            ),
            muted,
        ));
    }
    vec![Line::from(semantics), Line::from(counts), Line::raw("")]
}

pub(super) fn render_refs(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    if render_empty(
        frame,
        app,
        app.inspect.refs.is_empty(),
        "No refs",
        area,
        context,
    ) {
        return;
    }
    let layout = list_preview_layout(app, area);
    let list = layout.primary;
    let rows = ref_rows(&app.inspect.refs, list.width, context);
    render_string_list(frame, rows, app.inspect.selected, list, context);
    render_divider(frame, layout, context);
    if !app.inspect.refs.is_empty()
        && let Some(preview) = layout.secondary
    {
        render_preview(frame, app, preview, context);
    }
}

/// `kind name oid age subject → upstream`, with the name column sized to the
/// longest name that fits so every later column starts on the same cell.
fn ref_rows(refs: &[RefInfo], width: u16, context: &RenderContext) -> Vec<Line<'static>> {
    let glyphs = context.glyphs();
    let muted = context.style(context.muted());
    // Highlight symbol, kind, and gaps.
    let available = usize::from(width).saturating_sub(display_width(glyphs.selected) + 7);
    let name_width = refs
        .iter()
        .map(|reference| display_width(reference.short_name.display()) + 2)
        .max()
        .unwrap_or(0)
        .min((available * 2 / 5).clamp(12, 36));
    let show_oid = available >= name_width + 9 + 12;
    let show_age = show_oid && available >= name_width + 9 + 5 + 16;
    let age_width = if show_age {
        refs.iter()
            .filter_map(|reference| reference.timestamp)
            .map(|timestamp| display_width(&relative_age(timestamp)))
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    refs.iter()
        .map(|reference| {
            let (kind, name_style) = match reference.kind {
                RefKind::LocalBranch => ("branch", context.style(context.added())),
                RefKind::RemoteBranch => ("remote", context.style(context.removed())),
                RefKind::Tag => ("tag", context.style(context.warning())),
                RefKind::Stash => ("stash", context.style(context.accent())),
                RefKind::Other => ("ref", Style::reset()),
            };
            let (head, name_style) = if reference.is_head {
                (
                    "* ",
                    context.emphasize(name_style, ratatui::style::Modifier::BOLD),
                )
            } else {
                ("  ", name_style)
            };
            let name = format!("{head}{}", sanitize_str(reference.short_name.display()));
            let mut spans = vec![
                Span::styled(format!("{kind:<6} "), muted),
                Span::styled(
                    pad_right(
                        &truncate_with(&name, name_width, glyphs.ellipsis),
                        name_width,
                    ),
                    name_style,
                ),
            ];
            let mut used = 7 + name_width;
            if show_oid {
                spans.push(Span::styled(
                    format!(" {} ", reference.target.short(8)),
                    context.style(context.warning()),
                ));
                used += 10;
            }
            if age_width > 0 {
                let age = reference.timestamp.map(relative_age).unwrap_or_default();
                spans.push(Span::styled(
                    format!("{} ", pad_left(&age, age_width)),
                    muted,
                ));
                used += age_width + 1;
            }
            let upstream = reference
                .upstream
                .as_ref()
                .map(|name| format!(" {} {}", glyphs.arrow, sanitize_str(name.display())))
                .unwrap_or_default();
            let rest = usize::from(width).saturating_sub(display_width(glyphs.selected) + used);
            let subject = sanitize_str(&reference.subject);
            let upstream_width = display_width(&upstream);
            if show_oid && rest > upstream_width + 12 {
                spans.push(Span::raw(truncate_with(
                    &subject,
                    rest - upstream_width,
                    glyphs.ellipsis,
                )));
                spans.push(Span::styled(upstream, muted));
            } else if show_oid {
                spans.push(Span::raw(truncate_with(&subject, rest, glyphs.ellipsis)));
            }
            Line::from(spans)
        })
        .collect()
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

/// `group XY path`, naming each group once at its first row. The index
/// column is colored as staged and the worktree column as unstaged, the way
/// `git status` reads, while the letters carry the meaning without color.
fn status_rows(
    entries: &[crate::domain::StatusEntry],
    context: &RenderContext,
) -> Vec<Line<'static>> {
    let muted = context.style(context.muted());
    let mut previous = None;
    entries
        .iter()
        .map(|entry| {
            let group = status_group(entry);
            let label = if previous == Some(group) { "" } else { group };
            previous = Some(group);
            let code = |status: StatusCode, style: Style| {
                Span::styled(
                    status.porcelain_char().to_string(),
                    if status == StatusCode::Unmodified {
                        muted
                    } else {
                        style
                    },
                )
            };
            let (index_style, worktree_style) = if entry.conflict.is_some() {
                let conflict = context.strong(context.error());
                (conflict, conflict)
            } else if entry.index == StatusCode::Untracked {
                let untracked = context.style(context.warning());
                (untracked, untracked)
            } else {
                (
                    context.style(context.added()),
                    context.style(context.removed()),
                )
            };
            let mut spans = vec![
                Span::styled(format!("{label:<9} "), context.style(context.accent())),
                code(entry.index, index_style),
                code(entry.worktree, worktree_style),
                Span::raw(" "),
            ];
            if let Some(original) = &entry.original_path {
                spans.push(Span::styled(
                    format!("{} {} ", original.display, context.glyphs().arrow),
                    muted,
                ));
            }
            spans.extend(path_spans(&entry.path.display, Style::reset(), context));
            Line::from(spans)
        })
        .collect()
}

/// A path with its directory quiet and its file name in `style`.
fn path_spans(path: &str, style: Style, context: &RenderContext) -> Vec<Span<'static>> {
    match path.rfind('/') {
        Some(split) => vec![
            Span::styled(path[..=split].to_owned(), context.style(context.muted())),
            Span::styled(path[split + 1..].to_owned(), style),
        ],
        None => vec![Span::styled(path.to_owned(), style)],
    }
}

pub(super) fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let empty = app.inspect.status_entries().is_empty();
    if render_empty(frame, app, empty, "Working tree clean", area, context) {
        return;
    }
    let layout = list_preview_layout(app, area);
    let rows = status_rows(app.inspect.status_entries(), context);
    render_string_list(frame, rows, app.inspect.selected, layout.primary, context);
    render_divider(frame, layout, context);
    if let Some(preview) = layout.secondary {
        if app.inspect.working_diff.is_some() {
            let parts =
                Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(preview);
            frame.render_widget(Paragraph::new(status_diff_label(app, context)), parts[0]);
            render_diff_value(frame, app, parts[1], true, context);
        } else {
            let message = if app.inspect.loading {
                format!(
                    "Loading {} diff{}",
                    if app.inspect.status_diff_staged {
                        "staged"
                    } else {
                        "unstaged"
                    },
                    context.glyphs().ellipsis,
                )
            } else if app.inspect_error.is_some() {
                format!(
                    "Working diff unavailable {} retry or dismiss",
                    context.glyphs().dash
                )
            } else {
                "Select a tracked change to preview".into()
            };
            render_notice(frame, preview, &message, context);
        }
    }
}

/// Which side of a working change is shown, with the other side and the key
/// that switches to it: `staged diff · d unstaged`.
fn status_diff_label(app: &App, context: &RenderContext) -> Line<'static> {
    let (shown, other) = if app.inspect.status_diff_staged {
        ("staged", "unstaged")
    } else {
        ("unstaged", "staged")
    };
    let muted = context.style(context.muted());
    Line::from(vec![
        Span::styled(format!("{shown} diff"), context.strong(context.accent())),
        Span::styled(format!(" {} ", context.glyphs().separator), muted),
        Span::styled(
            context.key(&crate::app::Action::ToggleStatusDiff),
            context.strong(ratatui::style::Color::Reset),
        ),
        Span::styled(format!(" {other}"), muted),
    ])
}

pub(super) fn render_status_diff(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    if app.inspect.working_diff.is_none() {
        render_notice(frame, area, "Working diff unavailable", context);
        return;
    }
    let parts = Layout::vertical([
        Constraint::Length(STATUS_DIFF_HEADER_ROWS),
        Constraint::Min(1),
    ])
    .split(area);
    frame.render_widget(Paragraph::new(status_diff_label(app, context)), parts[0]);
    render_diff_value(frame, app, parts[1], true, context);
}

pub(super) fn render_tree(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    if render_empty(
        frame,
        app,
        app.inspect.tree.is_empty(),
        "Empty tree",
        area,
        context,
    ) {
        return;
    }
    let entries = &app.inspect.tree;
    let muted = context.style(context.muted());
    let sizes: Vec<String> = entries
        .iter()
        .map(|entry| entry.size.map(human_size).unwrap_or_default())
        .collect();
    let size_width = sizes
        .iter()
        .map(|size| display_width(size))
        .max()
        .unwrap_or(0);
    let name_room = usize::from(area.width)
        .saturating_sub(display_width(context.glyphs().selected) + 5 + size_width + 2);
    let rows = entries
        .iter()
        .zip(&sizes)
        .map(|(entry, size)| {
            // A trailing slash marks directories without relying on color;
            // unusual modes get a word, ordinary ones stay silent.
            let (kind, suffix, style) = match entry.kind {
                TreeEntryKind::Tree => ("dir", "/", context.strong(context.accent())),
                TreeEntryKind::Commit => ("sub", "@", context.style(context.warning())),
                TreeEntryKind::Blob if entry.mode == "120000" => {
                    ("link", "", context.style(context.accent()))
                }
                TreeEntryKind::Blob if entry.mode == "100755" => {
                    ("exec", "", context.style(context.added()))
                }
                TreeEntryKind::Blob => ("", "", Style::reset()),
                TreeEntryKind::Unknown => ("obj", "", muted),
            };
            let name = format!("{}{suffix}", entry.path.display);
            let name = truncate_with(&name, name_room, context.glyphs().ellipsis);
            let gap = name_room.saturating_sub(display_width(&name));
            Line::from(vec![
                Span::styled(format!("{kind:<4} "), muted),
                Span::styled(name, style),
                Span::raw(" ".repeat(gap + 2)),
                Span::styled(pad_left(size, size_width), muted),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, area, context);
}

/// Binary-prefixed size with one decimal below ten units: `912 B`, `4.2 KiB`.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub(super) fn render_blob(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    let Some(blob) = &app.inspect.blob else {
        render_notice(
            frame,
            area,
            &format!("Loading blob{}", context.glyphs().ellipsis),
            context,
        );
        return;
    };
    if blob.binary == Some(true) {
        let separator = context.glyphs().separator;
        let message = format!(
            "Binary blob {separator} {} {separator} {}{}",
            human_size(blob.size as u64),
            blob.id.short(12),
            if blob.truncated {
                format!(" {separator} preview truncated")
            } else {
                String::new()
            },
        );
        render_notice(frame, area, &message, context);
        return;
    }
    // Source reads with a line-number gutter; long lines are clipped rather
    // than wrapped so one scroll step is always one source line.
    let text = app.blob_lines();
    let number_width = text.len().to_string().len().max(3);
    let gutter = context.style(context.muted());
    let vertical = context.glyphs().vertical;
    let lines = text
        .iter()
        .enumerate()
        .skip(app.diff_scroll)
        .take(usize::from(area.height))
        .map(|(index, line)| {
            let mut spans = vec![Span::styled(
                format!("{:>number_width$} {vertical} ", index + 1),
                gutter,
            )];
            spans.extend(highlight_matches(
                vec![Span::raw(line.clone())],
                &app.search_query,
                context.search_hit(),
            ));
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

const BLAME_AUTHOR: usize = 10;

pub(super) fn render_blame(frame: &mut Frame<'_>, app: &App, area: Rect, context: &RenderContext) {
    if render_empty(
        frame,
        app,
        app.inspect.blame.is_empty(),
        "No blame lines",
        area,
        context,
    ) {
        return;
    }
    let layout = list_preview_layout(app, area);
    let blame = &app.inspect.blame;
    let number_width = blame
        .iter()
        .map(|line| line.final_line.to_string().len())
        .max()
        .unwrap_or(1);
    let age_width = blame
        .iter()
        .filter_map(|line| line.author_time)
        .map(|time| display_width(&relative_age(time)))
        .max()
        .unwrap_or(0);
    let muted = context.style(context.muted());
    let vertical = context.glyphs().vertical;
    let rows = blame
        .iter()
        .enumerate()
        .map(|(index, line)| {
            // Attribution prints once per run of lines from the same commit.
            let repeated = index > 0 && blame[index - 1].id == line.id;
            let mut spans = if repeated {
                vec![Span::raw(" ".repeat(9 + age_width + 1 + BLAME_AUTHOR + 1))]
            } else {
                let age = line.author_time.map(relative_age).unwrap_or_default();
                vec![
                    Span::styled(
                        format!("{} ", line.id.short(8)),
                        context.style(context.warning()),
                    ),
                    Span::styled(format!("{} ", pad_left(&age, age_width)), muted),
                    Span::styled(
                        format!(
                            "{} ",
                            pad_right(
                                &truncate_with(
                                    &sanitize_str(&line.author),
                                    BLAME_AUTHOR,
                                    context.glyphs().ellipsis
                                ),
                                BLAME_AUTHOR,
                            )
                        ),
                        context.style(context.accent()),
                    ),
                ]
            };
            spans.push(Span::styled(
                format!("{:>number_width$} {vertical} ", line.final_line),
                muted,
            ));
            spans.push(Span::raw(line.content.clone()));
            Line::from(spans)
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, layout.primary, context);
    render_divider(frame, layout, context);
    if !blame.is_empty()
        && let Some(preview) = layout.secondary
    {
        render_preview(frame, app, preview, context);
    }
}

pub(super) fn render_stashes(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    if render_empty(
        frame,
        app,
        app.inspect.stashes.is_empty(),
        "No stashes",
        area,
        context,
    ) {
        return;
    }
    let layout = list_preview_layout(app, area);
    let rows = app
        .inspect
        .stashes
        .iter()
        .map(|stash| {
            Line::from(vec![
                Span::styled(
                    format!("{} ", stash.selector),
                    context.style(context.accent()),
                ),
                Span::styled(
                    format!("{} ", stash.id.short(8)),
                    context.style(context.warning()),
                ),
                Span::styled(
                    stash
                        .timestamp
                        .map_or_else(String::new, |time| format!("{:>3} ", relative_age(time))),
                    context.style(context.muted()),
                ),
                Span::raw(sanitize_str(&stash.subject)),
            ])
        })
        .collect();
    render_string_list(frame, rows, app.inspect.selected, layout.primary, context);
    render_divider(frame, layout, context);
    if !app.inspect.stashes.is_empty()
        && let Some(preview) = layout.secondary
    {
        render_preview(frame, app, preview, context);
    }
}
