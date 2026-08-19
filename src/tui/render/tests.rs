use std::{path::PathBuf, sync::Mutex};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ratatui::{Terminal, backend::TestBackend, style::Color};

use crate::app::{SelectionContract, SelectionTarget};
use crate::domain::{
    BlameLine, Blob, Commit, CommitDetail, Comparison, ComparisonMode, Diff, DiffFile, DiffLine,
    DiffLineKind, GitPath, HistoryPage, ObjectFormat, Oid, RefInfo, RefKind, RefName, Repository,
    Signature, Status, StatusCode, StatusEntry,
};

use super::{layout::list_preview_areas, *};

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
    let _guard = GOLDEN_RENDER_LOCK.lock().unwrap();
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

static GOLDEN_RENDER_LOCK: Mutex<()> = Mutex::new(());

fn styled_screen(width: u16, height: u16, app: &App) -> String {
    let _guard = GOLDEN_RENDER_LOCK.lock().unwrap();
    set_date_mode("unix");
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    set_date_mode("relative");
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
