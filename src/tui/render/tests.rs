use std::{path::PathBuf, sync::Mutex};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};

use crate::app::{App, SelectionContract, SelectionTarget, View};
use crate::domain::{
    BlameLine, Blob, Commit, CommitDetail, Comparison, ComparisonMode, Diff, DiffFile, DiffLine,
    DiffLineKind, GitPath, HistoryPage, ObjectFormat, Oid, RefInfo, RefKind, RefName, RefScope,
    Repository, Signature, Status, StatusCode, StatusEntry,
};

use super::{
    graph::{graph_rows, lane_limit},
    history::history_line,
    layout::{diff_content_rows, list_preview_layout},
    *,
};

/// A log across several refs, the shape `--all` produces: two merges, a lane
/// that branches and rejoins, and a topic that never merges back.
fn branchy_app() -> App {
    fn oid(seed: u8) -> Oid {
        format!("{seed:02x}").repeat(20).parse().unwrap()
    }
    fn commit(id: u8, parents: &[u8], subject: &str, decorations: &[&str]) -> Commit {
        let signature = Signature {
            name: "Pat Example".into(),
            email: "pat@example.invalid".into(),
            timestamp: 1_700_000_000,
            timezone: "-04:00".into(),
        };
        Commit {
            id: oid(id),
            parents: parents.iter().copied().map(oid).collect(),
            author: signature.clone(),
            committer: signature,
            decorations: decorations
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            subject: subject.into(),
            body: String::new(),
        }
    }
    let commits = vec![
        commit(1, &[2, 3], "merge other", &["HEAD -> main", "origin/main"]),
        commit(3, &[6], "other-a", &[]),
        commit(2, &[4], "main-b", &[]),
        commit(4, &[7, 5], "merge feature", &[]),
        commit(5, &[8], "feat-b", &["origin/feature"]),
        commit(9, &[8], "topic-a", &["topic"]),
        commit(8, &[6], "feat-a", &[]),
        commit(7, &[6], "main-a", &[]),
        commit(6, &[10], "one", &["tag: v1"]),
        commit(10, &[], "base", &[]),
    ];
    let repository = Repository {
        root: PathBuf::from("/tmp/phig-demo"),
        worktree: Some(PathBuf::from("/tmp/phig-demo")),
        git_dir: PathBuf::from("/tmp/phig-demo/.git"),
        bare: false,
        object_format: ObjectFormat::Sha1,
        git_version: "2.45.1".into(),
        head: Some(oid(1)),
        branch: Some("main".into()),
    };
    let mut app = App::new(repository, "HEAD".into(), Vec::new(), false);
    app.revision_explicit = false;
    app.ref_scope = RefScope {
        all: true,
        ..RefScope::default()
    };
    app.revision_label = app.ref_scope.label();
    app.show_preview = false;
    app.apply_history(HistoryPage {
        commits,
        offset: 0,
        limit: 256,
        has_more: false,
    });
    app
}

#[test]
fn golden_ref_scope_graph_100x14() {
    let ascii = RenderContext::new(RenderConfig {
        glyph_mode: GlyphMode::Ascii,
        ..deterministic_config()
    });
    let snapshot = format!(
        "unicode 100x14\n{}\nnarrow 58x14\n{}\nfolded 44x14\n{}\nascii 100x14\n{}",
        screen(100, 14, &branchy_app()),
        screen(58, 14, &branchy_app()),
        screen(44, 14, &branchy_app()),
        screen_with_context(100, 14, &branchy_app(), &ascii),
    );
    insta::assert_snapshot!(
        "ref-scope-graph-100x14",
        snapshot
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn ref_scope_graph_connects_merges_to_their_parents() {
    let rendered = screen(100, 14, &branchy_app());
    // Columns four onward hold the lane cells, after the highlight symbol and
    // the mark gutter. Slice by characters: box drawing is multi-byte.
    let lanes: Vec<String> = rendered
        .lines()
        .map(|line| {
            line.chars()
                .skip(4)
                .take(8)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect();
    // The two merges each open a lane, the topic lane closes into feat-a, and
    // the shared ancestor collapses every remaining lane at once.
    assert!(
        lanes.iter().any(|lane| lane == "◆─╮"),
        "merge did not open a lane: {lanes:?}"
    );
    assert!(
        lanes.iter().any(|lane| lane == "◆─┼─╮"),
        "merge did not cross the live lane: {lanes:?}"
    );
    assert!(
        lanes.iter().any(|lane| lane == "│ │ ●─╯"),
        "topic lane did not close: {lanes:?}"
    );
    assert!(
        lanes.iter().any(|lane| lane == "●─┴─╯"),
        "shared ancestor did not collapse its lanes: {lanes:?}"
    );
    // The header names the scope instead of pinning it to one object.
    assert!(rendered.contains("all refs"), "scope missing from chrome");
    assert!(!rendered.contains("all refs@"), "scope pinned to an object");
}

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

fn review_app() -> App {
    let mut app = sample_app();
    let mut detail = app.preview.take().unwrap();
    detail.diff = crate::app::patch::fixture();
    app.apply_preview(detail);
    app.update(crate::app::Action::ToggleDiffFullscreen, 10);
    app
}

#[test]
fn diff_review_keys_render_tree_gutters_split_and_narrow_fallback() {
    use crate::app::Action;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = review_app();
    let mut scenes = format!("UNIFIED 100x22\n{}", styled_screen(100, 22, &app));
    let action =
        crate::tui::input::key_action(&app, KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE))
            .unwrap();
    assert_eq!(action, Action::ToggleDiffStyle);
    app.update(action, 10);
    scenes.push_str(&format!(
        "\nSPLIT 140x22\n{}\nNARROW 60x16\n{}",
        styled_screen(140, 22, &app),
        screen(60, 16, &app)
    ));
    let action =
        crate::tui::input::key_action(&app, KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE))
            .unwrap();
    app.update(action, 10);
    scenes.push_str(&format!(
        "\nTREE 120x22\n{}\nTREE 60x16\n{}",
        styled_screen(120, 22, &app),
        screen(60, 16, &app)
    ));
    insta::assert_snapshot!("diff-review", scenes);
    let tree_wide = styled_screen(120, 22, &app);
    assert!(tree_wide.contains('├'));
    assert!(tree_wide.contains('└'));
    let tree_narrow = screen(60, 16, &app);
    assert!(tree_narrow.contains("├─ src/"));
    assert!(tree_narrow.contains("│ ├─ ui/"));
    assert!(tree_narrow.contains("└─ main.rs M"));
    assert!(tree_narrow.contains("└─ README.md"));
    assert!(tree_narrow.contains("src/main.rs M +1 -1"));
    let collapse_all =
        crate::tui::input::key_action(&app, KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE))
            .unwrap();
    assert_eq!(collapse_all, Action::TreeCollapseAll);
    let expand_all =
        crate::tui::input::key_action(&app, KeyEvent::new(KeyCode::Char('='), KeyModifiers::SHIFT))
            .unwrap();
    assert_eq!(expand_all, Action::TreeExpandAll);
    app.update(Action::TreeCollapseAll, 10);
    assert!(screen(60, 16, &app).contains("├─ src/"));
    assert!(!screen(60, 16, &app).contains("view.rs"));
    app.update(Action::TreeExpandAll, 10);
    app.update(Action::First, 10);
    app.update(Action::Move(1), 10); // src -> ui
    app.update(Action::Move(1), 10); // ui -> view.rs
    app.update(Action::Open, 10);
    assert!(screen(60, 16, &app).contains("view.rs"));
    assert_eq!(
        crate::tui::input::key_action(&app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Action::NextFile(1))
    );
}

#[test]
fn diff_review_monochrome_retains_signs_and_word_emphasis() {
    let mut app = review_app();
    let context = RenderContext::new(RenderConfig {
        color_mode: ColorMode::Never,
        glyph_mode: GlyphMode::Ascii,
        ..deterministic_config()
    });
    for split in [false, true] {
        app.diff_split = split;
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        terminal
            .draw(|frame| render_with_context(frame, &app, &context))
            .unwrap();
        let cells = &terminal.backend().buffer().content;
        assert!(
            cells
                .iter()
                .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
        );
        assert!(
            cells
                .iter()
                .any(|cell| cell.modifier.contains(ratatui::style::Modifier::UNDERLINED))
        );
        let text = screen_with_context(140, 24, &app, &context);
        assert!(text.contains("-let timeout"));
        assert!(text.contains("+let timeout"));
    }
}

#[test]
fn diff_tree_ascii_guides_and_footer_survive_monochrome() {
    use crate::app::Action;
    let mut app = review_app();
    app.update(Action::ToggleDiffTree, 10);
    let context = RenderContext::new(RenderConfig {
        color_mode: ColorMode::Never,
        glyph_mode: GlyphMode::Ascii,
        ..deterministic_config()
    });
    let text = screen_with_context(60, 16, &app, &context);
    assert!(text.contains("|-- src/"));
    assert!(text.contains("`-- README.md"));
    assert!(text.contains("`-- main.rs M"));
    assert!(text.contains("src/main.rs M +1 -1"));
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
    terminal
        .draw(|frame| render_with_context(frame, &app, &context))
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset)
    );
}

#[test]
#[ignore = "manual release-mode diff navigation benchmark"]
fn diff_review_benchmark() {
    use crate::app::Action;
    use std::time::Instant;
    let mut app = review_app();
    let mut detail = app.preview.take().unwrap();
    let template = crate::app::patch::fixture();
    detail.diff.lines.clear();
    detail.diff.files.clear();
    for _ in 0..6000 {
        let offset = detail.diff.lines.len();
        detail.diff.lines.extend(template.lines.iter().cloned());
        detail
            .diff
            .files
            .extend(template.files.iter().cloned().map(|mut file| {
                file.header_line += offset;
                for hunk in &mut file.hunks {
                    hunk.header_line += offset;
                }
                file
            }));
    }
    let start = Instant::now();
    app.apply_preview(detail);
    eprintln!("index 102,000 patch lines: {:?}", start.elapsed());
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    let context = RenderContext::new(deterministic_config());
    for split in [false, true] {
        app.diff_split = split;
        app.diff_split_available = true;
        app.diff_scroll = 50_000;
        let mut samples = Vec::new();
        for _ in 0..200 {
            let start = Instant::now();
            app.update(Action::Move(1), 37);
            terminal
                .draw(|frame| render_with_context(frame, &app, &context))
                .unwrap();
            samples.push(start.elapsed());
        }
        samples.sort();
        eprintln!(
            "split={split}: move+paint p50={:?} p95={:?}",
            samples[100], samples[190]
        );
    }
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

fn deterministic_config() -> RenderConfig {
    RenderConfig {
        color_mode: ColorMode::Always,
        glyph_mode: GlyphMode::Unicode,
        ..RenderConfig::default()
    }
}

fn screen(width: u16, height: u16, app: &App) -> String {
    screen_with_context(
        width,
        height,
        app,
        &RenderContext::new(deterministic_config()),
    )
}

fn screen_with_context(width: u16, height: u16, app: &App, context: &RenderContext) -> String {
    let _guard = GOLDEN_RENDER_LOCK.lock().unwrap();
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_with_context(frame, app, context))
        .unwrap();
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

static GOLDEN_RENDER_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[ignore = "manual release-mode interaction benchmark"]
fn interaction_benchmark() {
    use crate::app::Action;
    use std::{hint::black_box, time::Instant};

    fn measure(label: &str, mut operation: impl FnMut()) {
        operation();
        let mut samples: Vec<_> = (0..40)
            .map(|_| {
                let start = Instant::now();
                operation();
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "{label}: p50={:.3}ms p95={:.3}ms (40 samples)",
            samples[20], samples[37]
        );
    }

    let mut app = sample_app();
    let template = app.commits[0].clone();
    app.commits = (0..50_000)
        .map(|index| {
            let mut commit = template.clone();
            commit.id = format!("{index:040x}").parse().unwrap();
            // 25k independent tips followed by their roots: crowded --all topology.
            commit.parents = if index < 25_000 {
                vec![format!("{:040x}", index + 25_000).parse().unwrap()]
            } else {
                Vec::new()
            };
            commit
        })
        .collect();
    app.show_preview = false;
    app.selected = 49_970;
    let context = RenderContext::new(deterministic_config());
    let mut terminal = Terminal::new(TestBackend::new(120, 28)).unwrap();
    measure("50k graph + frame / fresh rebuild", || {
        terminal
            .draw(|frame| render_with_context(frame, &app, &context))
            .unwrap();
    });
    let mut state = RenderState::default();
    terminal
        .draw(|frame| render_with_state(frame, &app, &context, &mut state))
        .unwrap();
    measure("50k graph + frame / cached navigation", || {
        app.selected = if app.selected == 49_970 {
            49_969
        } else {
            49_970
        };
        terminal
            .draw(|frame| render_with_state(frame, &app, &context, &mut state))
            .unwrap();
    });
    app.preview.as_mut().unwrap().diff.files = (0..50_000)
        .map(|index| DiffFile {
            header_line: index,
            old_path: None,
            new_path: Some(GitPath::new(
                format!("src/module_{index:05}/implementation.rs").into_bytes(),
            )),
            hunks: Vec::new(),
        })
        .collect();
    measure("50k files / uncached ranking", || {
        black_box(app.file_picker_entries("srcrs"));
    });
    app.update(Action::StartFilePicker, 10);
    for c in "srcrs".chars() {
        app.update(Action::SearchInput(c), 10);
    }
    measure("50k files / cached move + frame", || {
        app.update(Action::FilePickerMove(1), 10);
        terminal
            .draw(|frame| render_with_state(frame, &app, &context, &mut state))
            .unwrap();
    });
}

fn styled_screen(width: u16, height: u16, app: &App) -> String {
    let _guard = GOLDEN_RENDER_LOCK.lock().unwrap();
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let context = RenderContext::new(RenderConfig {
        date_mode: DateMode::Unix,
        ..deterministic_config()
    });
    terminal
        .draw(|frame| render_with_context(frame, app, &context))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut output = String::from("TEXT\n");
    for y in 0..height {
        let row = (0..width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        output.push_str(row.trim_end());
        output.push('\n');
    }
    output.push_str("STYLES\n");
    for y in 0..height {
        let mut x = 0;
        while x < width {
            let cell = &buffer[(x, y)];
            if cell.fg == Color::Reset && cell.bg == Color::Reset && cell.modifier.is_empty() {
                x += 1;
                continue;
            }
            let start = x;
            let style = (cell.fg, cell.bg, cell.modifier);
            let mut symbols = String::new();
            while x < width {
                let candidate = &buffer[(x, y)];
                if (candidate.fg, candidate.bg, candidate.modifier) != style {
                    break;
                }
                symbols.push_str(candidate.symbol());
                x += 1;
            }
            output.push_str(&format!(
                "{y}:{start}-{end} fg={fg:?} bg={bg:?} mod={modifier:?} text={symbols:?}\n",
                end = x.saturating_sub(1),
                fg = style.0,
                bg = style.1,
                modifier = style.2,
            ));
        }
    }
    output
}

#[test]
fn golden_log_narrow_60x16() {
    insta::assert_snapshot!("log-narrow-60x16", styled_screen(60, 16, &sample_app()));
}

#[test]
fn fullscreen_key_path_preserves_patch_across_sizes_and_restores_layout() {
    use crate::{
        app::{Action, Focus},
        config::KeyBindings,
        tui::input::resolve_action,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = sample_app();
    app.preview.as_mut().unwrap().diff.files[0]
        .hunks
        .push(crate::domain::Hunk {
            header_line: 1,
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
        });
    app.diff_scroll = 1;
    let bindings = KeyBindings::default();
    let original = screen(120, 28, &app);
    let action = resolve_action(
        &app,
        &bindings,
        KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT),
    )
    .unwrap();
    assert_eq!(action, Action::ToggleDiffFullscreen);
    assert!(app.update(action, 10).is_empty());
    let mut expanded = String::new();
    for (width, height) in [(120, 28), (60, 16)] {
        app.set_preview_focus_available(preview_focus_available(&app, width, height));
        let text = screen(width, height, &app);
        assert!(text.contains("DIFF aaaaaaaaaa"));
        assert!(text.contains("+new"));
        assert!(text.contains("F restore"));
        assert!(!text.contains("make history pleasant"));
        assert_eq!(page_rows(&app, width, height), usize::from(height - 3));
        let text = text
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        expanded.push_str(&format!("{width}x{height}\n{text}\n\n"));
    }
    insta::assert_snapshot!("fullscreen-diff", expanded);
    assert_eq!(app.focus, Focus::Preview);
    app.set_preview_focus_available(true);
    let action = resolve_action(
        &app,
        &bindings,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .unwrap();
    app.update(action, 10);
    assert_eq!(screen(120, 28, &app), original);
    assert!(!app.should_quit);
}

#[test]
fn cached_render_matches_fresh_after_navigation_resize_append_and_reset() {
    let mut app = branchy_app();
    let mut state = RenderState::default();
    let context = RenderContext::new(deterministic_config());
    for (width, selected) in [(100, 0), (100, 6), (100, 2), (44, 9), (100, 8)] {
        app.selected = selected;
        let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
        terminal
            .draw(|frame| render_with_state(frame, &app, &context, &mut state))
            .unwrap();
        let cached = terminal.backend().buffer().clone();
        terminal
            .draw(|frame| render_with_context(frame, &app, &context))
            .unwrap();
        assert_eq!(&cached, terminal.backend().buffer());
    }
    let mut extra = app.commits.last().unwrap().clone();
    extra.id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap();
    app.commits.push(extra);
    app.selected = 10;
    let mut terminal = Terminal::new(TestBackend::new(100, 8)).unwrap();
    terminal
        .draw(|frame| render_with_state(frame, &app, &context, &mut state))
        .unwrap();
    let cached = terminal.backend().buffer().clone();
    terminal
        .draw(|frame| render_with_context(frame, &app, &context))
        .unwrap();
    assert_eq!(&cached, terminal.backend().buffer());
    state.clear_history();
    app = sample_app();
    terminal
        .draw(|frame| render_with_state(frame, &app, &context, &mut state))
        .unwrap();
    let cached = terminal.backend().buffer().clone();
    terminal
        .draw(|frame| render_with_context(frame, &app, &context))
        .unwrap();
    assert_eq!(&cached, terminal.backend().buffer());
}

#[test]
fn fullscreen_status_keeps_side_and_file_position_visible_for_long_paths() {
    let mut app = sample_app();
    app.view = View::Status;
    app.inspect.status_diff_staged = true;
    let mut diff = app.preview.as_ref().unwrap().diff.clone();
    diff.files[0].new_path = Some(GitPath::new("long/界/".repeat(20).into_bytes()));
    app.inspect.working_diff = Some(diff);
    app.update(crate::app::Action::ToggleDiffFullscreen, 10);
    let text = screen(60, 16, &app);
    let header = text.lines().nth(1).unwrap();
    assert!(header.contains("DIFF staged"));
    assert!(header.contains("file 1/1"));
    assert!(header.contains('…'));
    assert!(!header.contains("aaaaaaaa"));
    assert!(!header.contains("hunk 0/0"));
}

#[test]
fn file_picker_render_tracks_public_patch_metadata_changes() {
    let mut app = sample_app();
    app.update(crate::app::Action::StartFilePicker, 10);
    let file = &mut app.preview.as_mut().unwrap().diff.files[0];
    file.new_path = Some(GitPath::new(b"src/live.rs".to_vec()));
    file.header_line = 2;
    assert!(screen(60, 16, &app).contains("src/live.rs"));
    app.update(crate::app::Action::AcceptFilePicker, 10);
    assert_eq!(app.diff_scroll, 2);
}

#[test]
fn golden_log_stacked_100x28() {
    insta::assert_snapshot!("log-stacked-100x28", styled_screen(100, 28, &sample_app()));
}

#[test]
fn golden_log_layout_boundary_109_to_110() {
    let app = sample_app();
    insta::assert_snapshot!(
        "log-layout-boundary-109-110",
        format!(
            "109x28\n{}\n110x28\n{}",
            styled_screen(109, 28, &app),
            styled_screen(110, 28, &app)
        )
    );
}

#[test]
fn golden_detail_wide_and_narrow_help() {
    let mut detail = sample_app();
    detail.view = View::Detail;
    let mut help = sample_app();
    help.show_help();
    insta::assert_snapshot!(
        "detail-wide-140x40-and-help-60x16",
        format!(
            "DETAIL 140x40\n{}\nHELP 60x16\n{}",
            styled_screen(140, 40, &detail),
            styled_screen(60, 16, &help)
        )
    );
}

#[test]
fn golden_primary_views_at_100x28() {
    let views = [
        View::Log,
        View::Detail,
        View::Compare,
        View::Refs,
        View::Status,
        View::StatusDiff,
        View::Tree,
        View::Blob,
        View::Blame,
        View::Stash,
    ];
    let mut output = String::new();
    for view in views {
        let mut app = sample_app();
        app.view = view;
        output.push_str(&format!(
            "\n=== {view:?} ===\n{}",
            styled_screen(100, 28, &app)
        ));
    }
    insta::assert_snapshot!("primary-views-100x28", output);
}

#[test]
fn golden_overlays_at_supported_minimum() {
    let mut palette = sample_app();
    let _ = palette.update(crate::app::Action::StartPalette, 10);
    let mut files = sample_app();
    let _ = files.update(crate::app::Action::StartFilePicker, 10);
    let mut error = sample_app();
    error.apply_error(
        crate::app::RequestKind::Preview,
        &crate::git::GitError::Timeout("commit-detail"),
    );
    insta::assert_snapshot!(
        "overlays-60x16",
        format!(
            "PALETTE\n{}\nFILES\n{}\nERROR\n{}",
            styled_screen(60, 16, &palette),
            styled_screen(60, 16, &files),
            styled_screen(60, 16, &error),
        )
    );
}

#[test]
fn narrow_layout_keeps_one_dominant_surface() {
    let output = screen(60, 16, &sample_app());
    assert!(output.contains("phig"));
    assert!(output.contains("make history"));
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
        narrow_footer.contains("Enter emit hunk · Esc/q cancel"),
        "selection contract was clipped at 60 columns: {narrow_footer}"
    );
    assert!(!narrow_footer.contains("j/k"));

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
        let expected = format!(
            "Enter emit {} · Esc/q cancel",
            target.label().to_ascii_lowercase()
        );
        assert!(
            footer.contains(&expected),
            "missing {target:?} selection contract: {footer}"
        );
    }
}

#[test]
fn log_footer_keeps_core_actions_visible_at_eighty_columns() {
    let output = screen(80, 16, &sample_app());
    let footer = output.lines().nth(15).unwrap();
    for hint in ["j/k move", "Enter open", "/ search"] {
        assert!(
            footer.contains(hint),
            "missing footer hint {hint:?}: {footer}"
        );
    }
    assert_eq!(footer.matches('·').count(), 2);
    assert!(!footer.contains("compare"));
    assert!(!footer.contains("quit"));
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
    assert!(error.contains("r retry · Esc dismiss"));

    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("back".into(), "x".into());
    overrides.insert("open".into(), "r".into());
    let context = RenderContext::with_bindings(
        deterministic_config(),
        crate::config::KeyBindings::from_config(&overrides).unwrap(),
    );
    let error = screen_with_context(52, 20, &app, &context);
    assert!(
        error.contains("r retry · Esc dismiss"),
        "modal recovery hint followed displaced normal-view bindings: {error}"
    );
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
    assert!(marked.contains("marked aaaaaaaaaa"));

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
    let narrow_layout = list_preview_layout(&app, narrow);
    assert_eq!(narrow_layout.primary, narrow);
    assert_eq!(narrow_layout.secondary, None);
    assert_eq!(page_rows(&app, 60, 28), 26);
    let normal_layout = list_preview_layout(&app, normal);
    assert!(
        normal_layout.secondary.unwrap().y > normal_layout.primary.y,
        "normal preview must be stacked"
    );
    assert_eq!(normal_layout.divider.unwrap().height, 1);
    assert_eq!(page_rows(&app, 100, 28), 12);
    let threshold_layout = list_preview_layout(&app, threshold);
    assert!(threshold_layout.secondary.unwrap().x > threshold_layout.primary.x);
    assert_eq!(threshold_layout.divider.unwrap().width, 1);
    assert_eq!(page_rows(&app, 110, 28), 26);
    let wide_layout = list_preview_layout(&app, wide);
    assert!(
        wide_layout.secondary.unwrap().x > wide_layout.primary.x,
        "wide preview must be right-side"
    );
    assert_eq!(page_rows(&app, 140, 28), 26);
    app.show_preview = false;
    assert_eq!(page_rows(&app, 100, 28), 26);

    app.view = View::Compare;
    assert_eq!(page_rows(&app, 100, 28), 23);
    app.view = View::StatusDiff;
    assert_eq!(page_rows(&app, 100, 28), 25);
}

#[test]
fn page_steps_match_visible_compare_and_status_diff_rows() {
    let mut app = sample_app();
    let mut diff = app.preview.as_ref().unwrap().diff.clone();
    diff.lines = (0..40)
        .map(|index| DiffLine {
            kind: DiffLineKind::Context,
            text: format!(" line {index}"),
        })
        .collect();
    let base: Oid = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".parse().unwrap();
    let head: Oid = "cccccccccccccccccccccccccccccccccccccccc".parse().unwrap();
    app.view = View::Compare;
    app.inspect.comparison = Some(Comparison {
        mode: ComparisonMode::Exact,
        requested_base: "main".into(),
        requested_head: "HEAD".into(),
        resolved_base: base,
        resolved_head: head,
        merge_base: None,
        ahead: 1,
        behind: 0,
        diff: diff.clone(),
    });
    let compare_rows = page_rows(&app, 100, 28);
    let _ = app.update(crate::app::Action::Page(1), compare_rows);
    assert_eq!(app.diff_scroll, 23);
    app.diff_scroll = 0;
    app.inspect.comparison.as_mut().unwrap().diff.truncated = true;
    let truncated_compare_rows = page_rows(&app, 100, 28);
    assert_eq!(truncated_compare_rows, 22);
    assert_eq!(diff_content_rows(23, true), 22);
    let _ = app.update(crate::app::Action::Page(1), truncated_compare_rows);
    assert_eq!(app.diff_scroll, 22);

    app.view = View::StatusDiff;
    app.diff_scroll = 0;
    app.inspect.working_diff = Some(diff);
    let status_rows = page_rows(&app, 100, 28);
    let _ = app.update(crate::app::Action::Page(1), status_rows);
    assert_eq!(app.diff_scroll, 25);
    app.diff_scroll = 0;
    app.inspect.working_diff.as_mut().unwrap().truncated = true;
    let truncated_status_rows = page_rows(&app, 100, 28);
    assert_eq!(truncated_status_rows, 24);
    let _ = app.update(crate::app::Action::Page(1), truncated_status_rows);
    assert_eq!(app.diff_scroll, 24);
}

#[test]
fn fuzzy_picker_layout_is_stable_with_matches_and_empty_results() {
    let mut app = sample_app();
    app.update(crate::app::Action::StartPalette, 14);
    let initial = screen(60, 16, &app);
    for character in "tglpr".chars() {
        app.update(crate::app::Action::SearchInput(character), 14);
    }
    let filtered = screen(60, 16, &app);
    let top = |text: &str| {
        text.lines()
            .position(|line| line.contains("Commands"))
            .unwrap()
    };
    assert_eq!(top(&initial), top(&filtered));
    assert!(filtered.contains("2 results"));
    app.update(crate::app::Action::SearchInput('☃'), 14);
    let empty = screen(60, 16, &app);
    assert_eq!(top(&initial), top(&empty));
    assert!(empty.contains("0 results"));
    assert!(empty.contains("Try fewer letters"));
}

#[test]
fn golden_fuzzy_discovery_at_narrow_and_wide_sizes() {
    let mut app = sample_app();
    app.update(crate::app::Action::StartPalette, 14);
    for character in "tgl".chars() {
        app.update(crate::app::Action::SearchInput(character), 14);
    }
    let palette = styled_screen(60, 16, &app);
    app.update(crate::app::Action::CancelOverlay, 14);
    app.update(crate::app::Action::StartFilePicker, 14);
    for character in "smrs".chars() {
        app.update(crate::app::Action::SearchInput(character), 14);
    }
    insta::assert_snapshot!(
        "fuzzy-discovery",
        format!(
            "palette 60x16\n{palette}\nfiles 120x28\n{}",
            styled_screen(120, 28, &app)
        )
    );
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
    assert!(output.contains("select a path, then b"));
}

#[test]
fn no_color_renders_reset_styles_without_a_post_pass() {
    let backend = TestBackend::new(100, 28);
    let mut terminal = Terminal::new(backend).unwrap();
    let context = RenderContext::new(RenderConfig {
        color_mode: ColorMode::Never,
        glyph_mode: GlyphMode::Unicode,
        ..RenderConfig::default()
    });
    terminal
        .draw(|frame| render_with_context(frame, &sample_app(), &context))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!((0..28).all(|y| (0..100).all(|x| {
        let cell = &buffer[(x, y)];
        cell.fg == Color::Reset && cell.bg == Color::Reset && cell.modifier.is_empty()
    })));
}

#[test]
fn visual_policy_supports_ascii_dividers_calm_selection_and_critical_header() {
    let app = sample_app();
    let ascii = RenderContext::new(RenderConfig {
        glyph_mode: GlyphMode::Ascii,
        date_mode: DateMode::Unix,
        color_mode: ColorMode::Always,
        ..RenderConfig::default()
    });
    let output = screen_with_context(110, 28, &app, &ascii);
    assert!(
        output.is_ascii(),
        "ASCII mode emitted non-ASCII application chrome: {output:?}"
    );
    assert!(
        output
            .lines()
            .skip(1)
            .take(26)
            .all(|line| line.chars().nth(45) == Some('|'))
    );
    let mut ascii_help = sample_app();
    ascii_help.show_help();
    assert!(screen_with_context(60, 16, &ascii_help, &ascii).is_ascii());
    let mut ascii_palette = sample_app();
    let _ = ascii_palette.update(crate::app::Action::StartPalette, 10);
    assert!(screen_with_context(60, 16, &ascii_palette, &ascii).is_ascii());

    let unicode = screen(110, 28, &app);
    assert!(
        unicode
            .lines()
            .skip(1)
            .take(26)
            .all(|line| line.chars().nth(45) == Some('│'))
    );
    let stacked = screen(100, 28, &app);
    assert_eq!(stacked.matches('─').count(), 100);

    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    let context = RenderContext::new(deterministic_config());
    terminal
        .draw(|frame| render_with_context(frame, &app, &context))
        .unwrap();
    let selected = &terminal.backend().buffer()[(0, 1)];
    assert_eq!(selected.bg, Color::Reset);
    assert_eq!(selected.fg, Color::Cyan);

    let mut failed = app;
    failed.repository.root =
        PathBuf::from("/tmp/界界界界界-a-very-long-repository-name-that-must-truncate");
    failed.apply_error(
        crate::app::RequestKind::History,
        &crate::git::GitError::Timeout("history"),
    );
    assert!(
        screen(60, 16, &failed)
            .lines()
            .next()
            .unwrap()
            .contains("request failed")
    );
}

#[test]
fn history_preserves_subjects_and_cell_width_across_date_modes() {
    use unicode_width::UnicodeWidthStr;

    let mut app = sample_app();
    app.commits[0].author.name = "界界e\u{301} author".into();
    app.commits[0].subject = "important subject survives".into();
    for mode in [
        DateMode::Relative,
        DateMode::Local,
        DateMode::Iso,
        DateMode::Unix,
    ] {
        let context = RenderContext::new(RenderConfig {
            date_mode: mode,
            ..deterministic_config()
        });
        for width in [45, 60] {
            let rows = graph_rows(&app.commits, 1, lane_limit(width), context.glyphs().graph);
            let line = history_line(
                &app.commits[0],
                width,
                &rows[0],
                rows[0].width(),
                false,
                None,
                &context,
            );
            let text = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert!(
                text.contains("important"),
                "subject vanished at {width} columns in {mode:?}: {text:?}"
            );
            assert!(
                UnicodeWidthStr::width(text.as_str())
                    <= usize::from(width) - UnicodeWidthStr::width(context.glyphs().selected),
                "history row exceeded its cell budget at {width} columns: {text:?}"
            );
        }
    }
}

#[test]
fn empty_inspection_views_report_zero_of_zero_without_stale_previews() {
    let mut app = sample_app();
    app.commits.clear();
    for view in [
        View::Refs,
        View::Status,
        View::Tree,
        View::Blame,
        View::Stash,
    ] {
        app.view = view;
        let output = screen(100, 28, &app);
        let footer = output.lines().nth(27).unwrap().to_owned();
        assert!(
            footer.ends_with("0/0"),
            "{view:?} had an impossible position: {footer:?}"
        );
        if matches!(view, View::Refs | View::Blame | View::Stash) {
            assert!(
                !output.contains("diff --git"),
                "{view:?} rendered an unrelated commit preview"
            );
        }
        if view == View::Status {
            assert!(footer.contains("no changes"));
            assert!(!footer.contains("loading diff"));
        }
    }
}

#[test]
fn explicit_context_honors_custom_selection_and_color_policy() {
    let app = sample_app();
    let theme = RenderTheme {
        selection_fg: Color::White,
        selection_bg: Color::Blue,
        ..RenderTheme::default()
    };
    let context = RenderContext::new(RenderConfig {
        theme,
        color_mode: ColorMode::Always,
        glyph_mode: GlyphMode::Unicode,
        ..RenderConfig::default()
    });
    assert!(!context.is_monochrome());
    let backend = TestBackend::new(60, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_with_context(frame, &app, &context))
        .unwrap();
    let selected = &terminal.backend().buffer()[(0, 1)];
    assert_eq!(selected.fg, Color::White);
    assert_eq!(selected.bg, Color::Blue);

    let never = RenderContext::new(RenderConfig {
        color_mode: ColorMode::Never,
        ..RenderConfig::default()
    });
    assert!(never.is_monochrome());
}

#[test]
fn footer_and_help_use_effective_remapped_keys() {
    let mut overrides = std::collections::BTreeMap::new();
    overrides.insert("open".into(), "ctrl+x".into());
    overrides.insert("search".into(), "alt+s".into());
    overrides.insert("help".into(), "h".into());
    let bindings = crate::config::KeyBindings::from_config(&overrides).unwrap();
    let context = RenderContext::with_bindings(RenderConfig::default(), bindings);
    let app = sample_app();
    let footer = screen_with_context(100, 28, &app, &context);
    assert!(footer.contains("Ctrl+x open"));
    assert!(footer.contains("Alt+s search"));
    let mut help = app;
    help.show_help();
    let overlay = screen_with_context(60, 16, &help, &context);
    assert!(overlay.contains("Ctrl+x"));
    assert!(overlay.contains("Alt+s"));
    assert!(overlay.contains("h close"));

    let mut displaced = std::collections::BTreeMap::new();
    displaced.insert("open".into(), "r".into());
    let displaced_context = RenderContext::with_bindings(
        deterministic_config(),
        crate::config::KeyBindings::from_config(&displaced).unwrap(),
    );
    let mut displaced_help = sample_app();
    displaced_help.show_help();
    let overlay = screen_with_context(60, 16, &displaced_help, &displaced_context);
    assert!(overlay.contains("r"));
    assert!(overlay.contains("unbound refs"));
    assert!(!overlay.contains("r refs"));

    let mut wide_override = std::collections::BTreeMap::new();
    wide_override.insert("open".into(), "界".into());
    let wide_context = RenderContext::with_bindings(
        deterministic_config(),
        crate::config::KeyBindings::from_config(&wide_override).unwrap(),
    );
    let footer = screen_with_context(60, 16, &sample_app(), &wide_context)
        .lines()
        .nth(15)
        .unwrap()
        .to_owned();
    assert!(
        footer.contains('界') && footer.contains("open"),
        "wide key hint was clipped: {footer:?}"
    );
    assert!(footer.ends_with("1/1"));
}

#[test]
fn help_overlay_is_contextual() {
    let mut app = sample_app();
    app.show_help();
    let output = screen(100, 28, &app);
    assert!(output.contains("Help"));
    assert!(output.contains("Enter"));
    assert!(output.contains("log view"));
}
