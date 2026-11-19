//! Header, footer, overlays, and actionable error presentation.

use super::Canvas;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::{
    app::{Action, App, Overlay, View},
    sanitize::sanitize_str,
};

use super::{
    format::{display_width, pad_right, truncate_with},
    layout::centered_rect,
    picker::{command_item, matched_label, render_picker},
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

pub(super) fn render_header(
    frame: &mut Canvas<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    let left = header_context(app, context);
    let Some((label, style)) = header_status(app, context) else {
        frame.render_widget(Paragraph::new(left), area);
        return;
    };
    let width = display_width(&label)
        .saturating_add(1)
        .min(usize::from(area.width / 2)) as u16;
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(width)]).areas(area);
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(
        Paragraph::new(label)
            .alignment(Alignment::Right)
            .style(style),
        right_area,
    );
}

fn header_context(app: &App, context: &RenderContext) -> Line<'static> {
    let repository = app
        .repository
        .root
        .file_name()
        .map(|name| sanitize_str(&name.to_string_lossy()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| sanitize_str(&app.repository.root.to_string_lossy()));
    let branch = sanitize_str(app.repository.branch.as_deref().unwrap_or("detached"));
    // A ref label resolves to one object, so it is shown pinned to that object.
    // A ref scope names a family of refs and has no single target to pin to.
    let revision = match (&app.revision_label, app.ref_scope.is_empty()) {
        (Some(label), true) => format!(
            "{}@{}",
            sanitize_str(label),
            truncate_with(&app.revision, 10, context.glyphs().ellipsis)
        ),
        (Some(label), false) => sanitize_str(label),
        (None, _) => sanitize_str(&app.revision),
    };
    // The view badge is reverse video so it reads as a tab in any palette;
    // monochrome keeps plain text, where the uppercase name still stands out.
    let badge = context.emphasize(
        context.strong(context.accent()),
        ratatui::style::Modifier::REVERSED,
    );
    let separator = format!(" {} ", context.glyphs().separator);
    let muted = context.style(context.muted());
    let mut left = vec![
        Span::styled(format!(" {} ", view_label(app.view)), badge),
        Span::raw(" "),
        Span::styled(repository, context.strong(ratatui::style::Color::Reset)),
        Span::styled(separator.clone(), muted),
        Span::styled(branch, context.style(context.added())),
    ];
    // The named branch already describes a default HEAD walk. Keep an
    // explicit revision or ref family, which changes what is on screen.
    if revision != "HEAD" || app.revision_explicit || app.repository.branch.is_none() {
        left.push(Span::styled(separator, muted));
        left.push(Span::styled(revision, muted));
    }
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

    Line::from(left)
}

fn header_status(app: &App, context: &RenderContext) -> Option<(String, Style)> {
    if app.has_errors() {
        return Some(("request failed".into(), context.strong(context.error())));
    }
    if let Some(selection) = &app.selection_contract {
        return Some((
            format!("select {}", selection.target.label().to_ascii_lowercase()),
            context.strong(context.accent()),
        ));
    }
    if app.inspect.compare_picker {
        return Some((
            "choose comparison base".into(),
            context.strong(context.warning()),
        ));
    }
    if app.inspect.loading || app.history_loading || app.preview_loading {
        return Some((
            format!("loading{}", context.glyphs().ellipsis),
            context.style(context.warning()),
        ));
    }
    app.marked_oid.as_ref().map(|marked| {
        (
            format!("marked {}", marked.short(10)),
            context.style(context.muted()),
        )
    })
}

fn index_position(selected: usize, count: usize) -> String {
    if count == 0 {
        "0/0".into()
    } else {
        format!("{}/{}", selected.min(count - 1) + 1, count)
    }
}

fn position(app: &App, context: &RenderContext) -> String {
    if let Overlay::DiffTree(tree) = &app.overlay {
        return index_position(tree.selected, tree.visible.len());
    }
    if app.diff_fullscreen {
        return patch_position(app, context);
    }
    match app.view {
        // More history streams in on demand; `+` says the count is a floor.
        View::Log if app.has_more && !app.commits.is_empty() => {
            format!("{}+", index_position(app.selected, app.commits.len()))
        }
        View::Log => index_position(app.selected, app.commits.len()),
        View::Refs => index_position(app.inspect.selected, app.inspect.refs.len()),
        View::Status => index_position(app.inspect.selected, app.inspect.status_entries().len()),
        View::Tree => index_position(app.inspect.selected, app.inspect.tree.len()),
        View::Blame => index_position(app.inspect.selected, app.inspect.blame.len()),
        View::Stash => index_position(app.inspect.selected, app.inspect.stashes.len()),
        View::Blob => format!("line {}", app.diff_scroll + 1),
        View::Detail | View::Compare | View::StatusDiff => patch_position(app, context),
    }
}

fn patch_position(app: &App, context: &RenderContext) -> String {
    let line = app.diff_scroll + 1;
    let Some(diff) = app.active_diff() else {
        return format!("line {line}");
    };
    if diff.files.len() < 2 {
        return format!("line {}", index_position(app.diff_scroll, diff.lines.len()));
    }
    let file = diff
        .files
        .partition_point(|file| file.header_line <= app.diff_scroll)
        .max(1);
    format!(
        "file {file}/{} {} line {line}",
        diff.files.len(),
        context.glyphs().separator
    )
}

fn key_pair(context: &RenderContext, first: &Action, second: &Action) -> String {
    format!("{}/{}", context.key(first), context.key(second))
}

/// A footer hint: the key in normal weight, then what it does.
type Hint = (String, &'static str);

fn hints(app: &App, context: &RenderContext) -> Vec<Hint> {
    let key = |action: &Action| context.key(action);
    let pair = |first: &Action, second: &Action| key_pair(context, first, second);
    if matches!(app.overlay, Overlay::DiffTree(_)) {
        return vec![
            (key(&Action::Open), "open"),
            (pair(&Action::TreeCollapse, &Action::TreeExpand), "fold"),
            (key(&Action::Back), "cancel"),
            (
                pair(&Action::TreeCollapseAll, &Action::TreeExpandAll),
                "fold all",
            ),
        ];
    }
    let hunk = || pair(&Action::NextHunk(-1), &Action::NextHunk(1));
    if app.diff_fullscreen {
        return vec![
            (key(&Action::ToggleDiffFullscreen), "restore"),
            (hunk(), "hunk"),
            (key(&Action::ToggleDiffTree), "files"),
        ];
    }
    let move_keys = pair(&Action::Move(1), &Action::Move(-1));
    match app.view {
        View::Log => vec![
            (move_keys, "move"),
            (key(&Action::Open), "inspect"),
            (key(&Action::StartSearch), "search"),
            (key(&Action::Mark), "mark"),
        ],
        View::Refs if app.inspect.compare_picker => vec![
            (move_keys, "choose base"),
            (key(&Action::Open), "compare"),
            (key(&Action::Back), "cancel"),
        ],
        View::Refs | View::Blame | View::Stash => vec![
            (move_keys, "move"),
            (key(&Action::Open), "open"),
            (key(&Action::StartSearch), "search"),
        ],
        View::Status => {
            let preview: Hint = if app.inspect.status_entries().is_empty() {
                (String::new(), "no changes")
            } else if app.inspect.working_diff.is_some() {
                (key(&Action::Open), "open")
            } else if app.inspect.loading || app.inspect.working_diff_pending.is_some() {
                (String::new(), "loading diff")
            } else {
                (String::new(), "no diff")
            };
            vec![
                (move_keys, "move"),
                preview,
                (key(&Action::ToggleStatusDiff), "staged/unstaged"),
            ]
        }
        View::Tree => vec![
            (move_keys, "move"),
            (key(&Action::Open), "open"),
            (key(&Action::Ascend), "up"),
        ],
        View::Detail | View::StatusDiff => vec![
            (key(&Action::StartFilePicker), "files"),
            (hunk(), "hunk"),
            (key(&Action::ToggleDiffFullscreen), "expand"),
            (key(&Action::ToggleDiffTree), "tree"),
        ],
        View::Compare => vec![
            (key(&Action::StartFilePicker), "files"),
            (key(&Action::SwapCompare), "swap"),
            (key(&Action::ToggleCompareMode), "mode"),
            (hunk(), "hunk"),
        ],
        View::Blob => vec![
            (move_keys, "scroll"),
            (key(&Action::StartSearch), "search"),
            (key(&Action::ViewBlame), "blame"),
        ],
    }
}

/// Contextual hints first; `?` always closes the row so the full key
/// reference is one keystroke away on every screen.
fn hint_line(
    app: &App,
    left_width: usize,
    key_style: Style,
    context: &RenderContext,
) -> Line<'static> {
    let muted = context.style(context.muted());
    let mut hints = hints(app, context);
    let commands = context.key(&Action::StartPalette);
    if commands != "unbound" {
        hints.push((commands, "commands"));
    }
    let help: Hint = (context.key(&Action::ToggleHelp), "help");
    let separator = format!(" {} ", context.glyphs().separator);
    let hint_width = |(key, label): &Hint| {
        display_width(key) + usize::from(!key.is_empty()) + display_width(label)
    };
    let mut used = 1 + hint_width(&help);
    let mut chosen = Vec::new();
    for hint in hints {
        let width = hint_width(&hint) + display_width(&separator);
        if used + width > left_width {
            continue;
        }
        used += width;
        chosen.push(hint);
    }
    chosen.push(help);
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in chosen.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(separator.clone(), muted));
        }
        if !key.is_empty() {
            spans.push(Span::styled(format!("{key} "), key_style));
        }
        spans.push(Span::styled(label, muted));
    }
    Line::from(spans)
}

pub(super) fn render_footer(
    frame: &mut Canvas<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
    let position = format!("{} ", position(app, context));
    let position_width = u16::try_from(display_width(&position)).unwrap_or(area.width);
    let parts = Layout::horizontal([
        Constraint::Min(1),
        Constraint::Length(position_width.min(area.width)),
    ])
    .split(area);
    let left_width = usize::from(parts[0].width);
    let muted = context.style(context.muted());
    let key_style = if context.is_monochrome() {
        Style::reset()
    } else {
        Style::default().bold()
    };

    let left = if let Some(selection) = &app.selection_contract {
        Line::from(vec![
            Span::raw(" "),
            Span::styled(selection.accept_key.clone(), key_style),
            Span::styled(
                format!(
                    " emit {} {} ",
                    selection.target.label().to_ascii_lowercase(),
                    context.glyphs().separator
                ),
                muted,
            ),
            Span::styled(selection.cancel_keys.clone(), key_style),
            Span::styled(" cancel", muted),
        ])
    } else if let Some(notice) = &app.notice {
        Line::styled(format!(" {notice}"), context.style(context.accent()))
    } else {
        hint_line(app, left_width, key_style, context)
    };
    frame.render_widget(Paragraph::new(left), parts[0]);
    frame.render_widget(
        Paragraph::new(position)
            .alignment(Alignment::Right)
            .style(muted),
        parts[1],
    );
}

fn overlay_regions(
    frame: &mut Canvas<'_>,
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
    frame.render_widget(Clear, popup);
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
    // One cell of horizontal padding keeps content off the frame.
    let inner = Rect::new(
        inner.x.saturating_add(1).min(inner.right()),
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let parts = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    (parts[0], parts[1])
}

/// One help row: the keys for one or more actions and what they do.
struct HelpEntry {
    actions: &'static [Action],
    label: &'static str,
}

const fn entry(actions: &'static [Action], label: &'static str) -> HelpEntry {
    HelpEntry { actions, label }
}

/// The cheat sheet, grouped the way people reach for keys. The palette (`:`)
/// remains the complete, searchable list.
const HELP_SECTIONS: &[(&str, &[HelpEntry])] = &[
    (
        "Move",
        &[
            entry(&[Action::Move(1), Action::Move(-1)], "down/up"),
            entry(&[Action::Page(1), Action::Page(-1)], "page down/up"),
            entry(&[Action::First, Action::Last], "top/bottom"),
            entry(&[Action::Open], "open"),
            entry(&[Action::Back], "back"),
            entry(&[Action::Quit], "quit"),
        ],
    ),
    (
        "Find",
        &[
            entry(&[Action::StartSearch], "search"),
            entry(
                &[Action::NextMatch, Action::PreviousMatch],
                "next/prev match",
            ),
            entry(&[Action::StartFilePicker], "jump to file"),
            entry(&[Action::StartPalette], "every command"),
        ],
    ),
    (
        "Diff",
        &[
            entry(
                &[Action::NextHunk(1), Action::NextHunk(-1)],
                "next/prev hunk",
            ),
            entry(
                &[Action::NextFile(1), Action::NextFile(-1)],
                "next/prev file",
            ),
            entry(&[Action::ToggleDiffTree], "file tree"),
            entry(&[Action::ToggleDiffFullscreen], "expand diff"),
            entry(&[Action::ToggleDiffStyle], "split/unified"),
            entry(&[Action::NextParent], "merge parent"),
            entry(&[Action::TogglePreview], "preview on/off"),
        ],
    ),
    (
        "Views",
        &[
            entry(&[Action::ViewRefs], "refs"),
            entry(&[Action::ViewStatus], "status"),
            entry(&[Action::ViewTree], "tree"),
            entry(&[Action::ViewBlame], "blame"),
            entry(&[Action::ViewStash], "stashes"),
        ],
    ),
    (
        "Compare",
        &[
            entry(&[Action::Mark], "mark endpoint"),
            entry(&[Action::StartCompare], "compare"),
            entry(&[Action::SwapCompare], "swap sides"),
            entry(&[Action::ToggleCompareMode], "base/exact"),
            entry(&[Action::ToggleStatusDiff], "staged/unstaged"),
            entry(&[Action::CopySelection], "copy id/path"),
        ],
    ),
];

const HELP_COLUMN_GAP: usize = 3;

/// Keys for a group of actions; a shared modifier is written once, so
/// `Ctrl+d` and `Ctrl+u` read as `Ctrl+d/u`.
fn help_keys(context: &RenderContext, actions: &[Action]) -> String {
    let keys: Vec<String> = actions.iter().map(|action| context.key(action)).collect();
    let prefix = keys
        .first()
        .and_then(|key| key.rfind('+').map(|end| &key[..=end]))
        .filter(|prefix| {
            keys.iter()
                .all(|key| key.starts_with(prefix) && key.len() > prefix.len())
        });
    match prefix {
        Some(prefix) if keys.len() > 1 => format!(
            "{prefix}{}",
            keys.iter()
                .map(|key| &key[prefix.len()..])
                .collect::<Vec<_>>()
                .join("/")
        ),
        _ => keys.join("/"),
    }
}

/// Render one section as aligned `keys  label` rows under a heading.
fn help_section(
    title: &str,
    entries: &[HelpEntry],
    key_width: usize,
    width: usize,
    context: &RenderContext,
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(
        title.to_owned(),
        context.strong(context.accent()),
    )];
    for entry in entries {
        let keys = help_keys(context, entry.actions);
        lines.push(Line::from(vec![
            Span::styled(
                pad_right(&keys, key_width),
                context.strong(ratatui::style::Color::Reset),
            ),
            Span::raw("  "),
            Span::styled(
                truncate_with(
                    entry.label,
                    width.saturating_sub(key_width + 2),
                    context.glyphs().ellipsis,
                ),
                context.style(context.muted()),
            ),
        ]));
    }
    lines
}

pub(super) fn render_help(frame: &mut Canvas<'_>, app: &App, area: Rect, context: &RenderContext) {
    let key_width = HELP_SECTIONS
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .map(|entry| display_width(&help_keys(context, entry.actions)))
        .max()
        .unwrap_or(0)
        .min(12);
    let column_width = key_width + 2 + 16;
    let inner_width =
        usize::from(area.width.saturating_sub(4)).min(3 * column_width + 2 * HELP_COLUMN_GAP);
    let columns = ((inner_width + HELP_COLUMN_GAP) / (column_width + HELP_COLUMN_GAP)).max(1);
    let column_width = if columns == 1 {
        inner_width
    } else {
        column_width
    };

    // Masonry: each section goes to the shortest column so far.
    let mut stacks: Vec<Vec<Line<'static>>> = vec![Vec::new(); columns];
    for (title, entries) in HELP_SECTIONS {
        let target = (0..columns)
            .min_by_key(|column| stacks[*column].len())
            .unwrap_or(0);
        if !stacks[target].is_empty() {
            stacks[target].push(Line::raw(""));
        }
        stacks[target].extend(help_section(
            title,
            entries,
            key_width,
            column_width,
            context,
        ));
    }
    let content_height = stacks.iter().map(Vec::len).max().unwrap_or(0);
    let width = columns * column_width + (columns - 1) * HELP_COLUMN_GAP + 4;
    let height = content_height + 3;
    let (body, footer) = overlay_regions(
        frame,
        area,
        u16::try_from(width).unwrap_or(u16::MAX),
        u16::try_from(height).unwrap_or(u16::MAX),
        "Keys",
        false,
        context,
    );
    for (column, lines) in stacks.into_iter().enumerate() {
        let x = body.x + u16::try_from(column * (column_width + HELP_COLUMN_GAP)).unwrap_or(0);
        if x >= body.right() {
            break;
        }
        let area = Rect::new(
            x,
            body.y,
            (body.right() - x).min(column_width as u16),
            body.height,
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
    let muted = context.style(context.muted());
    let key = context.strong(ratatui::style::Color::Reset);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(context.key(&Action::StartPalette), key),
            Span::styled(" all commands", muted),
            Span::styled(format!(" {} ", context.glyphs().separator), muted),
            Span::styled(context.key(&Action::ToggleHelp), key),
            Span::styled(" close", muted),
            Span::styled(
                format!(
                    " {} {} view",
                    context.glyphs().separator,
                    view_label(app.view).to_ascii_lowercase()
                ),
                muted,
            ),
        ])),
        footer,
    );
}

pub(super) fn render_search(
    frame: &mut Canvas<'_>,
    app: &App,
    draft: &str,
    body: Rect,
    context: &RenderContext,
) {
    let area = Rect::new(body.x, body.bottom().saturating_sub(1), body.width, 1);
    frame.render_widget(Clear, area);
    let muted = context.style(context.muted());
    let separator = context.glyphs().separator;
    // A miss is stated in words, not just color, and the hint says what
    // still works.
    let status = if app.search_miss && !draft.is_empty() {
        Span::styled(
            format!("  no match {separator} "),
            context.strong(context.warning()),
        )
    } else {
        Span::styled("  ".to_owned(), muted)
    };
    let draft = sanitize_str(draft);
    let prompt_width = 1 + display_width(&draft);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("/", context.strong(context.accent())),
            Span::raw(draft),
            status,
            Span::styled(
                format!("Enter accept {separator} Esc cancel {separator} Ctrl-u clear"),
                muted,
            ),
        ])),
        area,
    );
    if let Ok(x) = u16::try_from(prompt_width)
        && x < area.width
    {
        frame.set_cursor_position((area.x + x, area.y));
    }
}

pub(super) fn render_palette(
    frame: &mut Canvas<'_>,
    app: &App,
    draft: &str,
    selected: usize,
    area: Rect,
    context: &RenderContext,
) {
    let commands = app.commands_for_palette(draft);
    let query = crate::fuzzy::Query::new(draft);
    let regions = overlay_regions(frame, area, 62, 13, "Commands", false, context);
    render_picker(
        frame,
        regions,
        context,
        super::picker::Picker {
            draft,
            selected,
            count: commands.len(),
            prefix: "Find: ",
            empty: "No matching commands",
            accept: "run",
        },
        |index, width| command_item(&commands[index], &query, width, context),
    );
}

pub(super) fn render_file_picker(
    frame: &mut Canvas<'_>,
    app: &App,
    draft: &str,
    selected: usize,
    area: Rect,
    context: &RenderContext,
) {
    let files = app.cached_file_picker_entries(draft);
    let query = crate::fuzzy::Query::new(draft);
    let total = app.active_diff().map_or(0, |diff| diff.files.len());
    let height = (total.clamp(2, 12) as u16 + 5).min(area.height.saturating_sub(2));
    let regions = overlay_regions(frame, area, 68, height, "Changed files", false, context);
    render_picker(
        frame,
        regions,
        context,
        super::picker::Picker {
            draft,
            selected,
            count: files.len(),
            prefix: "Find: ",
            empty: "No matching changed files",
            accept: "jump",
        },
        |index, width| matched_label(&files[index].0, &query, width, context),
    );
}

pub(super) fn render_errors(
    frame: &mut Canvas<'_>,
    app: &App,
    area: Rect,
    context: &RenderContext,
) {
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
            "Esc dismiss {s} select a path, then {}",
            context.key(&Action::ViewBlame),
            s = context.glyphs().separator,
        )
    } else {
        format!("r retry {} Esc dismiss", context.glyphs().separator,)
    };
    frame.render_widget(
        Paragraph::new(recovery).style(context.style(context.warning())),
        footer,
    );
}
