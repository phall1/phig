use std::path::PathBuf;

use crate::{
    domain::{
        BlameLine, Commit, CommitDetail, Comparison, ComparisonMode, ConflictStage, ConflictStages,
        Diff, DiffFile, DiffLine, DiffLineKind, GitPath, HistoryPage, ObjectFormat, Oid, RefInfo,
        RefKind, RefName, Repository, Signature, Status, StatusCode, StatusEntry,
    },
    git::GitError,
};

use super::*;

fn oid(character: char) -> Oid {
    format!("{character:0<40}").parse().unwrap()
}

fn commit(character: char, subject: &str) -> Commit {
    Commit {
        id: oid(character),
        parents: Vec::new(),
        author: Signature {
            name: "Pat".into(),
            email: "pat@example.invalid".into(),
            timestamp: 0,
            timezone: "Z".into(),
        },
        committer: Signature {
            name: "Pat".into(),
            email: "pat@example.invalid".into(),
            timestamp: 0,
            timezone: "Z".into(),
        },
        decorations: Vec::new(),
        subject: subject.into(),
        body: String::new(),
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

fn working_diff(text: &str) -> Diff {
    Diff {
        lines: vec![DiffLine {
            kind: DiffLineKind::Added,
            text: text.into(),
        }],
        files: Vec::new(),
        truncated: false,
    }
}

fn repository() -> Repository {
    Repository {
        root: PathBuf::from("/tmp/repo"),
        worktree: Some(PathBuf::from("/tmp/repo")),
        git_dir: PathBuf::from("/tmp/repo/.git"),
        bare: false,
        object_format: ObjectFormat::Sha1,
        git_version: "2.45.1".into(),
        head: Some(oid('a')),
        branch: Some("main".into()),
    }
}

fn app() -> App {
    let mut app = App::new(repository(), "HEAD".into(), Vec::new(), false);
    app.apply_history(HistoryPage {
        commits: vec![commit('a', "first"), commit('b', "needle")],
        offset: 0,
        limit: 256,
        has_more: false,
    });
    app
}

#[test]
fn diff_tree_browse_cancel_open_and_refresh_are_reversible_without_git() {
    let mut app = app();
    let detail = CommitDetail {
        commit: commit('a', "first"),
        diff: patch::fixture(),
        selected_parent: None,
    };
    app.apply_preview(detail.clone());
    app.diff_scroll = 5;
    let selected = app.selected_oid.clone();
    assert!(app.update(Action::ToggleDiffTree, 10).is_empty());
    let Overlay::DiffTree(tree) = &app.overlay else {
        panic!("tree missing")
    };
    assert_eq!(tree.current().unwrap().label, "main.rs");
    assert!(app.update(Action::Last, 10).is_empty());
    assert_eq!(app.diff_scroll, 8);
    app.update(Action::Back, 10);
    assert_eq!(app.diff_scroll, 5);
    assert_eq!(app.selected_oid, selected);
    assert!(!app.diff_fullscreen);
    app.update(Action::ToggleDiffTree, 10);
    app.update(Action::Last, 10);
    assert!(app.update(Action::Open, 10).is_empty());
    assert!(app.diff_fullscreen);
    assert_eq!(app.diff_scroll, 8);
    app.update(Action::ToggleDiffTree, 10);
    app.apply_preview(detail);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn diff_tree_collapses_nested_directories_without_losing_file_identity() {
    let mut app = app();
    app.view = View::StatusDiff;
    app.apply_working_diff(patch::fixture());
    app.update(Action::ToggleDiffTree, 10);
    app.update(Action::TreeCollapse, 10); // main.rs -> src/
    app.update(Action::TreeCollapse, 10); // fold src/
    let Overlay::DiffTree(tree) = &app.overlay else {
        panic!("tree missing")
    };
    assert_eq!(tree.visible.len(), 2);
    assert_eq!(tree.current().unwrap().label, "src");
    assert_eq!(
        (
            tree.current().unwrap().added,
            tree.current().unwrap().removed
        ),
        (3, 1)
    );
    app.update(Action::TreeExpand, 10);
    app.update(Action::Last, 10);
    let Overlay::DiffTree(tree) = &app.overlay else {
        panic!("tree missing")
    };
    assert_eq!(tree.current().unwrap().label, "view.rs");
    assert_eq!(tree.current().unwrap().header_line, 8);
}

#[test]
fn split_navigation_and_public_patch_replacement_keep_correct_coordinates() {
    let mut app = app();
    app.view = View::StatusDiff;
    app.apply_working_diff(patch::fixture());
    app.diff_scroll = 5;
    app.diff_split_available = true;
    app.update(Action::ToggleDiffStyle, 4);
    app.update(Action::Move(1), 4);
    assert_eq!(app.diff_scroll, 7);
    app.diff_split_available = false;
    app.update(Action::Move(-1), 4);
    assert_eq!(app.diff_scroll, 6);
    let diff = app.inspect.working_diff.as_mut().unwrap();
    diff.files[0].hunks[0].new_start = 100;
    assert_eq!(app.patch_index().lines[6].new, Some(101));
    app.inspect.working_diff.as_mut().unwrap().lines[6].kind = DiffLineKind::Metadata;
    assert_eq!(app.patch_index().lines[5].pair, None);
}

#[test]
fn split_pages_from_added_side_preserve_anchor_across_width_changes() {
    use crate::git::parse::{DiffFileIdentity, parse_diff};
    let diff = parse_diff(
        b"diff --git a/a b/a\n@@ -1,4 +1,4 @@\n a\n-b\n-c\n+B\n+C\n d\n",
        &[DiffFileIdentity {
            old_path: Some(GitPath::new(b"a".to_vec())),
            new_path: Some(GitPath::new(b"a".to_vec())),
        }],
        false,
    )
    .unwrap();
    let mut app = app();
    app.view = View::StatusDiff;
    app.apply_working_diff(diff);
    app.diff_split = true;
    app.diff_split_available = true;
    app.diff_scroll = 5; // added B, paired with removed b
    app.update(Action::Move(1), 2);
    assert_eq!(app.diff_scroll, 4); // next visual row is c/C
    app.update(Action::Page(-1), 2);
    assert_eq!(app.diff_scroll, 2); // context a, two visual rows above
    app.diff_split_available = false;
    app.diff_scroll = 5;
    app.update(Action::Move(1), 2);
    assert_eq!(app.diff_scroll, 6); // unified advances to +C
    app.diff_split_available = true;
    app.update(Action::Move(1), 2);
    assert_eq!(app.diff_scroll, 7); // widening preserves +C's logical row
}

#[test]
fn fullscreen_preserves_view_selection_scroll_and_focus_across_resize() {
    let mut app = app();
    app.apply_preview(CommitDetail {
        commit: commit('a', "first"),
        selected_parent: None,
        diff: working_diff("+patch"),
    });
    app.focus = Focus::Preview;
    app.diff_scroll = 3;
    let selected = app.selected_oid.clone();
    assert!(app.update(Action::ToggleDiffFullscreen, 10).is_empty());
    assert!(app.diff_fullscreen);
    assert_eq!(app.view, View::Log);
    app.set_preview_focus_available(false);
    assert_eq!(app.focus, Focus::Preview);
    app.update(Action::ToggleDiffFullscreen, 10);
    assert!(!app.diff_fullscreen);
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.diff_scroll, 3);
    assert_eq!(app.selected_oid, selected);
    assert!(app.view_stack.is_empty());
}

#[test]
fn fullscreen_status_search_and_cancel_operate_on_patch_not_status_rows() {
    let mut app = app();
    app.view = View::Status;
    app.inspect.working_diff = Some(Diff {
        lines: ["+one", "+needle", "+other", "+needle again"]
            .into_iter()
            .map(|text| DiffLine {
                kind: DiffLineKind::Added,
                text: text.into(),
            })
            .collect(),
        files: Vec::new(),
        truncated: false,
    });
    app.update(Action::ToggleDiffFullscreen, 10);
    app.update(Action::StartSearch, 10);
    for c in "needle".chars() {
        assert!(app.update(Action::SearchInput(c), 10).is_empty());
    }
    assert_eq!(
        app.diff_scroll, 1,
        "incremental search stays on the current matching line"
    );
    app.update(Action::AcceptSearch, 10);
    app.update(Action::NextMatch, 10);
    assert_eq!(app.diff_scroll, 3);
    app.update(Action::PreviousMatch, 10);
    assert_eq!(app.diff_scroll, 1);
    app.update(Action::StartSearch, 10);
    app.update(Action::SearchInput('x'), 10);
    assert!(
        app.update(Action::CancelOverlay, 10).is_empty(),
        "cancel must not reload the patch"
    );
    assert_eq!(app.diff_scroll, 1);
    app.update(Action::Back, 10);
    assert!(!app.diff_fullscreen);
    assert_eq!(app.view, View::Status);
    assert!(app.inspect.working_diff.is_some());
    assert!(!app.should_quit);
}

#[test]
fn cancelling_search_without_a_selection_change_keeps_the_loaded_patch() {
    let mut app = app();
    app.apply_preview(CommitDetail {
        commit: commit('a', "first"),
        selected_parent: None,
        diff: working_diff("+patch"),
    });
    app.update(Action::StartSearch, 10);
    app.update(Action::SearchInput('x'), 10);
    assert!(app.update(Action::CancelOverlay, 10).is_empty());
    assert!(!app.preview_loading);
    assert_eq!(app.preview.as_ref().unwrap().diff.lines[0].text, "+patch");
}

#[test]
fn changing_views_leaves_fullscreen_and_unavailable_views_explain_it() {
    let mut app = app();
    app.update(Action::ToggleDiffFullscreen, 10); // A pending history preview can expand immediately.
    assert!(app.diff_fullscreen);
    app.update(Action::ViewTree, 10);
    assert!(!app.diff_fullscreen);
    app.update(Action::ToggleDiffFullscreen, 10);
    assert!(!app.diff_fullscreen);
    assert!(app.notice.as_ref().unwrap().contains("patch"));
}

#[test]
fn file_picker_cache_reuses_matches_and_refreshes_when_patch_changes() {
    let mut app = app();
    let files = |name: &[u8], header_line| {
        vec![DiffFile {
            header_line,
            new_path: Some(GitPath::new(name.to_vec())),
            old_path: None,
            hunks: Vec::new(),
        }]
    };
    let mut diff = working_diff("+patch");
    diff.files = files(b"src/main.rs", 4);
    app.apply_preview(CommitDetail {
        commit: commit('a', "first"),
        selected_parent: None,
        diff,
    });
    app.update(Action::StartFilePicker, 10);
    for c in "srcrs".chars() {
        app.update(Action::SearchInput(c), 10);
    }
    let pointer = app.cached_file_picker_entries("srcrs").as_ptr();
    assert!(matches!(
        app.cached_file_picker_entries("srcrs"),
        std::borrow::Cow::Borrowed(_)
    ));
    app.update(Action::FilePickerMove(1), 10);
    assert_eq!(app.cached_file_picker_entries("srcrs").as_ptr(), pointer);

    let mut diff = working_diff("+replacement");
    diff.files = files(b"src/replaced.rs", 9);
    app.apply_preview(CommitDetail {
        commit: commit('a', "first"),
        selected_parent: None,
        diff,
    });
    assert_eq!(
        app.cached_file_picker_entries("srcrs")[0],
        ("src/replaced.rs".into(), 9)
    );
    app.update(Action::AcceptFilePicker, 10);
    assert_eq!(app.diff_scroll, 9);
    assert!(
        app.file_picker_cache.is_none(),
        "closing the overlay releases its snapshot"
    );
}

#[test]
fn file_picker_observes_same_sized_public_metadata_mutations() {
    let mut app = app();
    let mut diff = working_diff("+patch");
    diff.files.push(DiffFile {
        header_line: 4,
        old_path: None,
        new_path: Some(GitPath::new(b"old.rs".to_vec())),
        hunks: Vec::new(),
    });
    app.apply_preview(CommitDetail {
        commit: commit('a', "first"),
        selected_parent: None,
        diff,
    });
    app.update(Action::StartFilePicker, 10);
    let file = &mut app.preview.as_mut().unwrap().diff.files[0];
    file.new_path = Some(GitPath::new(b"new.rs".to_vec()));
    file.header_line = 9;
    assert_eq!(app.cached_file_picker_entries("")[0], ("new.rs".into(), 9));
    app.update(Action::SearchInput('n'), 10);
    assert_eq!(app.cached_file_picker_entries("n")[0], ("new.rs".into(), 9));
    app.preview.as_mut().unwrap().diff.files[0].header_line = 11;
    app.update(Action::AcceptFilePicker, 10);
    assert_eq!(app.diff_scroll, 11);
}

#[test]
fn fullscreen_comparison_picker_restores_list_navigation() {
    let mut app = app();
    app.update(Action::ToggleDiffFullscreen, 10);
    assert!(app.diff_fullscreen);
    app.update(Action::StartCompare, 10);
    assert_eq!(app.view, View::Refs);
    assert!(!app.diff_fullscreen);
    app.apply_refs(
        ['a', 'b']
            .map(|id| RefInfo {
                full_name: RefName::new(format!("refs/heads/{id}").into_bytes()),
                short_name: RefName::new(vec![id as u8]),
                kind: RefKind::LocalBranch,
                target: oid(id),
                peeled: None,
                upstream: None,
                subject: String::new(),
                timestamp: None,
                is_head: false,
            })
            .to_vec(),
    );
    app.update(Action::Move(1), 10);
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.inspect.selected, 1);
}

#[test]
fn overlapping_history_pages_keep_order_and_selection_without_duplicates() {
    let mut app = app();
    app.selected = 1;
    app.selected_oid = Some(oid('b'));
    app.apply_history(HistoryPage {
        commits: vec![
            commit('b', "duplicate"),
            commit('c', "new"),
            commit('c', "duplicate new"),
        ],
        offset: 2,
        limit: 3,
        has_more: false,
    });
    assert_eq!(
        app.commits
            .iter()
            .map(|commit| &commit.id)
            .collect::<Vec<_>>(),
        [&oid('a'), &oid('b'), &oid('c')]
    );
    assert_eq!(app.selected, 1);
}

#[test]
fn selection_is_stable_by_oid_across_refresh() {
    let mut app = app();
    let _ = app.update(Action::Move(1), 10);
    let selected = app.selected_oid.clone();
    app.apply_history(HistoryPage {
        commits: vec![
            commit('c', "new"),
            commit('b', "needle"),
            commit('a', "first"),
        ],
        offset: 0,
        limit: 256,
        has_more: false,
    });
    assert_eq!(app.selected_oid, selected);
    assert_eq!(app.selected, 1);
}

#[test]
fn search_and_back_are_semantic_transitions() {
    let mut app = app();
    let _ = app.update(Action::StartSearch, 10);
    let _ = app.update(Action::SearchInput('n'), 10);
    let _ = app.update(Action::SearchInput('e'), 10);
    assert_eq!(app.selected, 1, "search should update incrementally");
    let _ = app.update(Action::AcceptSearch, 10);
    assert_eq!(app.selected, 1);
    let _ = app.update(Action::Open, 10);
    assert_eq!(app.view, View::Detail);
    let _ = app.update(Action::Back, 10);
    assert_eq!(app.view, View::Log);
    assert!(!app.should_quit);
    let _ = app.update(Action::Back, 10);
    assert!(app.should_quit);
}

#[test]
fn cancelling_incremental_search_restores_selection() {
    let mut app = app();
    let _ = app.update(Action::StartSearch, 10);
    for character in "needle".chars() {
        let _ = app.update(Action::SearchInput(character), 10);
    }
    assert_eq!(app.selected, 1);
    let effects = app.update(Action::CancelOverlay, 10);
    assert_eq!(app.selected, 0);
    assert_eq!(app.search_query, "");
    assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));
}

#[test]
fn search_requests_more_history_when_loaded_page_has_no_match() {
    let mut app = app();
    app.has_more = true;
    app.history_loading = false;
    let _ = app.update(Action::StartSearch, 10);
    let effects = app.update(Action::SearchInput('z'), 10);
    assert_eq!(app.search_pending, Some(true));
    assert_eq!(
        effects,
        [Effect::LoadHistory {
            offset: 2,
            limit: PAGE_SIZE
        }]
    );
}

#[test]
fn next_match_advances_past_a_matching_current_commit() {
    let mut app = app();
    app.search_query = "Pat".into();
    let _ = app.update(Action::NextMatch, 10);
    assert_eq!(app.selected, 1);
    let _ = app.update(Action::PreviousMatch, 10);
    assert_eq!(app.selected, 0);
}

#[test]
fn paged_previous_search_preserves_its_direction() {
    let mut app = app();
    app.search_query = "missing".into();
    app.has_more = true;
    app.history_loading = false;
    let effects = app.update(Action::PreviousMatch, 10);
    assert_eq!(app.search_pending, Some(false));
    assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));
    let _ = app.apply_history(HistoryPage {
        commits: vec![commit('c', "missing")],
        offset: 2,
        limit: PAGE_SIZE,
        has_more: false,
    });
    assert_eq!(app.selected, 2, "paged N search must continue backward");
    assert_eq!(app.selected_commit().unwrap().subject, "missing");
}

#[test]
fn unavailable_preview_returns_focus_to_visible_history() {
    let mut app = app();
    app.focus = Focus::Preview;
    app.set_preview_focus_available(false);
    assert_eq!(app.focus, Focus::List);
    let _ = app.update(Action::ToggleFocus, 10);
    assert_eq!(app.focus, Focus::List, "hidden preview accepted focus");
    app.set_preview_focus_available(true);
    let _ = app.update(Action::ToggleFocus, 10);
    assert_eq!(app.focus, Focus::Preview);
}

#[test]
fn empty_preview_views_clear_unrelated_commit_detail() {
    let detail = || CommitDetail {
        commit: commit('a', "stale"),
        diff: working_diff("+stale"),
        selected_parent: None,
    };
    let mut app = app();
    app.preview = Some(detail());
    let _ = app.update(Action::ViewRefs, 10);
    assert!(app.preview.is_none());
    app.preview = Some(detail());
    assert!(app.apply_refs(Vec::new()).is_empty());
    assert!(app.preview.is_none());

    app.view = View::Log;
    app.paths = vec![GitPath::new(b"src/lib.rs".to_vec())];
    app.preview = Some(detail());
    let _ = app.update(Action::ViewBlame, 10);
    assert!(app.preview.is_none());
    app.preview = Some(detail());
    assert!(app.apply_blame(Vec::new()).is_empty());
    assert!(app.preview.is_none());

    app.view = View::Log;
    app.preview = Some(detail());
    let _ = app.update(Action::ViewStash, 10);
    assert!(app.preview.is_none());
    app.preview = Some(detail());
    assert!(app.apply_stashes(Vec::new()).is_empty());
    assert!(app.preview.is_none());
}

#[test]
fn show_start_opens_detail_while_history_loads() {
    let app = App::new(repository(), "HEAD~2".into(), Vec::new(), true);
    assert_eq!(app.view, View::Detail);
    assert!(app.history_loading);
    assert_eq!(
        app.initial_effects(),
        [
            Effect::LoadHistory {
                offset: 0,
                limit: PAGE_SIZE
            },
            Effect::LoadPreview {
                revision: "HEAD~2".into(),
                parent_index: 0
            }
        ]
    );
}

#[test]
fn request_failures_are_isolated_and_retryable() {
    let mut app = app();
    app.history_loading = true;
    app.preview_loading = true;
    app.apply_error(RequestKind::Preview, &GitError::Timeout("commit-detail"));
    assert!(
        app.history_loading,
        "preview failure must not stop history loading"
    );
    assert!(!app.preview_loading);
    assert!(app.preview_error.is_some());

    let _ = app.apply_history(HistoryPage {
        commits: Vec::new(),
        offset: 2,
        limit: PAGE_SIZE,
        has_more: false,
    });
    assert!(
        app.preview_error.is_some(),
        "history success hid preview failure"
    );
    let effects = app.update(Action::RetryFailed, 10);
    assert!(app.preview_error.is_none());
    assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));

    app.apply_error(RequestKind::History, &GitError::Timeout("history"));
    app.apply_error(RequestKind::Preview, &GitError::Timeout("commit-detail"));
    let effects = app.update(Action::RetryFailed, 10);
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadHistory { .. }, Effect::LoadPreview { .. }]
    ));
    app.apply_error(RequestKind::History, &GitError::Timeout("history"));
    let _ = app.update(Action::DismissErrors, 10);
    assert!(!app.has_errors());
}

#[test]
fn fuzzy_palette_ranks_executes_and_cancels_semantic_actions() {
    let mut app = app();
    app.update(Action::StartPalette, 10);
    for character in "tglpr".chars() {
        app.update(Action::SearchInput(character), 10);
    }
    assert_eq!(palette_commands("tglpr")[0].action, Action::TogglePreview);
    app.update(Action::CancelOverlay, 10);
    assert!(app.show_preview);

    app.update(Action::StartPalette, 10);
    for character in "tglpr".chars() {
        app.update(Action::SearchInput(character), 10);
    }
    app.update(Action::ExecutePalette, 10);
    assert!(!app.show_preview);
    assert_eq!(app.overlay, Overlay::None);

    app.update(Action::StartPalette, 10);
    app.update(Action::SearchInput('☃'), 10);
    app.update(Action::PaletteMove(-1), 10);
    app.update(Action::ExecutePalette, 10);
    assert!(
        !app.should_quit,
        "no-match Enter must not execute a command"
    );
}

#[test]
fn fuzzy_file_ranking_keeps_patch_anchors_and_cancel_position() {
    let mut app = app();
    app.view = View::StatusDiff;
    let mut diff = working_diff("+change");
    diff.files = [("src/deep/main.rs", 40), ("src/main.rs", 8), ("old.rs", 20)]
        .into_iter()
        .map(|(path, header_line)| DiffFile {
            header_line,
            old_path: Some(GitPath::new(path.as_bytes().to_vec())),
            new_path: None,
            hunks: Vec::new(),
        })
        .collect();
    app.inspect.working_diff = Some(diff);
    assert_eq!(app.file_picker_entries("")[0].1, 40);
    assert_eq!(app.file_picker_entries("smrs")[0].1, 8);

    app.diff_scroll = 3;
    app.update(Action::StartFilePicker, 10);
    for character in "smrs".chars() {
        app.update(Action::SearchInput(character), 10);
    }
    app.update(Action::FilePickerMove(-1), 10);
    assert!(matches!(
        app.overlay,
        Overlay::FilePicker { selected: 1, .. }
    ));
    app.update(Action::CancelOverlay, 10);
    assert_eq!(app.diff_scroll, 3);

    app.update(Action::StartFilePicker, 10);
    for character in "smrs".chars() {
        app.update(Action::SearchInput(character), 10);
    }
    app.update(Action::AcceptFilePicker, 10);
    assert_eq!(
        app.diff_scroll, 8,
        "ranked row must retain its original anchor"
    );
}

#[test]
fn searchable_palette_executes_semantic_actions() {
    let mut app = app();
    let _ = app.update(Action::StartPalette, 10);
    for character in "toggle preview".chars() {
        let _ = app.update(Action::SearchInput(character), 10);
    }
    let _ = app.update(Action::ExecutePalette, 10);
    assert!(!app.show_preview);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn changed_file_picker_filters_and_jumps_on_every_diff_surface() {
    let diff = Diff {
        lines: (0..8)
            .map(|index| DiffLine {
                kind: DiffLineKind::Metadata,
                text: format!("line {index}"),
            })
            .collect(),
        files: vec![
            DiffFile {
                header_line: 0,
                old_path: None,
                new_path: Some(GitPath::new(b"src/main.rs".to_vec())),
                hunks: Vec::new(),
            },
            DiffFile {
                header_line: 5,
                old_path: None,
                new_path: Some(GitPath::new(b"tests/main.rs".to_vec())),
                hunks: Vec::new(),
            },
        ],
        truncated: false,
    };
    for view in [
        View::Log,
        View::Detail,
        View::Compare,
        View::Status,
        View::StatusDiff,
    ] {
        let mut app = app();
        app.view = view;
        match view {
            View::Log | View::Detail => {
                app.preview = Some(CommitDetail {
                    commit: commit('a', "first"),
                    diff: diff.clone(),
                    selected_parent: None,
                });
            }
            View::Compare => {
                app.inspect.comparison = Some(Comparison {
                    mode: ComparisonMode::Exact,
                    requested_base: "main".into(),
                    requested_head: "HEAD".into(),
                    resolved_base: oid('a'),
                    resolved_head: oid('b'),
                    merge_base: None,
                    ahead: 1,
                    behind: 0,
                    diff: diff.clone(),
                });
            }
            View::Status | View::StatusDiff => {
                app.inspect.working_diff = Some(diff.clone());
            }
            _ => unreachable!(),
        }
        let _ = app.update(Action::StartFilePicker, 10);
        assert!(
            matches!(app.overlay, Overlay::FilePicker { .. }),
            "{view:?}"
        );
        for character in "tests".chars() {
            let _ = app.update(Action::SearchInput(character), 10);
        }
        let _ = app.update(Action::AcceptFilePicker, 10);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.diff_scroll, 5, "{view:?}");
    }

    let mut app = app();
    let commands = palette_commands("");
    assert!(
        commands
            .iter()
            .any(|command| command.action == Action::StartFilePicker)
    );
    assert!(
        commands
            .iter()
            .any(|command| command.action == Action::Redraw)
    );
    assert!(
        commands
            .iter()
            .any(|command| command.action == Action::StartPalette)
    );

    let _ = app.update(Action::StartPalette, 10);
    for character in "redraw".chars() {
        let _ = app.update(Action::SearchInput(character), 10);
    }
    let _ = app.update(Action::ExecutePalette, 10);
    assert!(
        app.take_redraw_request(),
        "palette redraw did not reach the terminal adapter"
    );

    let _ = app.update(Action::StartPalette, 10);
    for character in "copy selection".chars() {
        let _ = app.update(Action::SearchInput(character), 10);
    }
    let _ = app.update(Action::ExecutePalette, 10);
    assert!(
        app.take_copy_request(),
        "palette copy did not reach the terminal adapter"
    );
}

#[test]
fn view_switching_marked_compare_and_ref_picker_are_semantic() {
    let mut app = app();
    let effects = app.update(Action::ViewRefs, 10);
    assert_eq!(app.view, View::Refs);
    assert_eq!(effects, [Effect::LoadRefs]);
    let reference = RefInfo {
        full_name: RefName::new(b"refs/heads/main".to_vec()),
        short_name: RefName::new(b"main".to_vec()),
        kind: RefKind::LocalBranch,
        target: oid('a'),
        peeled: None,
        upstream: None,
        subject: "first".into(),
        timestamp: None,
        is_head: false,
    };
    let effects = app.apply_refs(vec![reference]);
    assert!(matches!(effects.as_slice(), [Effect::LoadPreview { .. }]));
    let effects = app.update(Action::Open, 10);
    assert_eq!(app.view, View::Log);
    assert_eq!(app.revision, oid('a').to_string());
    assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));

    app.commits = vec![commit('a', "one"), commit('b', "two")];
    app.selected = 0;
    let _ = app.update(Action::Mark, 10);
    app.selected = 1;
    let effects = app.update(Action::StartCompare, 10);
    assert_eq!(app.view, View::Compare);
    assert_eq!(app.inspect.compare_mode, ComparisonMode::Exact);
    assert!(matches!(effects.as_slice(), [Effect::LoadCompare]));
}

#[test]
fn configured_compare_mode_survives_in_tui_ref_comparison() {
    let mut app = app();
    app.inspect.compare_mode = ComparisonMode::Exact;
    app.view = View::Refs;
    app.inspect.refs = vec![RefInfo {
        full_name: RefName::new(b"refs/heads/main".to_vec()),
        short_name: RefName::new(b"main".to_vec()),
        kind: RefKind::LocalBranch,
        target: oid('a'),
        peeled: None,
        upstream: None,
        subject: "base".into(),
        timestamp: None,
        is_head: false,
    }];
    let effects = app.update(Action::StartCompare, 10);
    assert_eq!(app.view, View::Compare);
    assert_eq!(app.inspect.compare_mode, ComparisonMode::Exact);
    assert_eq!(effects, [Effect::LoadCompare]);
}

#[test]
fn status_toggle_clears_stale_patch_and_status_opens_dominant_diff() {
    let mut app = app();
    app.view = View::Status;
    let effects = app.apply_status(Status {
        entries: vec![status_entry(
            StatusCode::Modified,
            StatusCode::Modified,
            b"both.txt",
        )],
        ..Status::default()
    });
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadWorkingDiff { staged: true, .. }]
    ));
    app.apply_working_diff(working_diff("old staged"));
    let effects = app.update(Action::ToggleStatusDiff, 10);
    assert!(app.inspect.loading);
    assert!(app.inspect.working_diff.is_none());
    assert!(!app.inspect.status_diff_staged);
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadWorkingDiff { staged: false, .. }]
    ));
    app.apply_error(RequestKind::Inspect, &GitError::Timeout("working diff"));
    assert_eq!(
        app.inspect_error.as_ref().unwrap().operation,
        "load unstaged working diff"
    );
    assert!(
        app.inspect.working_diff.is_none(),
        "failed toggle exposed stale patch"
    );
    app.apply_working_diff(working_diff("old again"));
    app.apply_error(RequestKind::Inspect, &GitError::Timeout("status refresh"));
    assert!(
        app.inspect.working_diff.is_none(),
        "inspect failure retained an old patch"
    );
    app.apply_working_diff(working_diff("fresh unstaged"));
    let _ = app.update(Action::Open, 10);
    assert_eq!(app.view, View::StatusDiff);
    let _ = app.update(Action::Back, 10);
    assert_eq!(app.view, View::Status);
}

#[test]
fn status_sort_keeps_conflicts_first_and_mixed_state_intact() {
    let mut app = app();
    app.view = View::Status;
    let mut conflict = status_entry(
        StatusCode::UpdatedButUnmerged,
        StatusCode::UpdatedButUnmerged,
        b"conflict.txt",
    );
    let stage = ConflictStage {
        mode: "100644".into(),
        oid: None,
    };
    conflict.conflict = Some(ConflictStages {
        base: stage.clone(),
        ours: stage.clone(),
        theirs: stage,
        worktree_mode: "100644".into(),
    });
    let mixed = status_entry(StatusCode::Modified, StatusCode::Modified, b"mixed.txt");
    let _ = app.apply_status(Status {
        entries: vec![mixed, conflict],
        ..Status::default()
    });
    assert_eq!(app.inspect.status_entries()[0].path.display, "conflict.txt");
    assert_eq!(app.inspect.status_entries()[1].index.porcelain_char(), 'M');
    assert_eq!(
        app.inspect.status_entries()[1].worktree.porcelain_char(),
        'M'
    );
}

#[test]
fn inspect_search_cancel_restores_semantic_cursor() {
    let mut app = app();
    app.view = View::Status;
    let _ = app.apply_status(Status {
        entries: vec![
            status_entry(StatusCode::Modified, StatusCode::Unmodified, b"first.txt"),
            status_entry(StatusCode::Unmodified, StatusCode::Modified, b"needle.txt"),
        ],
        ..Status::default()
    });
    let _ = app.update(Action::StartSearch, 10);
    for character in "needle".chars() {
        let _ = app.update(Action::SearchInput(character), 10);
    }
    assert_eq!(app.inspect.selected, 1);
    let effects = app.update(Action::CancelOverlay, 10);
    assert_eq!(app.inspect.selected, 0);
    assert!(matches!(
        effects.as_slice(),
        [Effect::LoadWorkingDiff { .. }]
    ));
}

#[test]
fn compare_picker_cancel_and_missing_blame_path_are_safe() {
    let mut app = app();
    let _ = app.update(Action::StartCompare, 10);
    assert!(app.inspect.compare_picker);
    assert_eq!(app.view, View::Refs);
    let _ = app.update(Action::Back, 10);
    assert!(!app.inspect.compare_picker);
    assert_eq!(app.view, View::Log);

    app.paths.clear();
    app.preview = None;
    let effects = app.update(Action::ViewBlame, 10);
    assert!(effects.is_empty());
    assert_eq!(app.view, View::Log);
    assert_eq!(app.inspect_error.as_ref().unwrap().operation, "open blame");
    assert!(
        app.inspect_error
            .as_ref()
            .unwrap()
            .detail
            .contains("select a file path")
    );
}

#[test]
fn refs_use_peeled_authoritative_oid_and_blame_preview_keeps_path() {
    let mut app = app();
    app.view = View::Refs;
    let peeled = oid('b');
    let reference = RefInfo {
        full_name: RefName::new(b"refs/tags/release-\xff".to_vec()),
        short_name: RefName::new(b"release-\xff".to_vec()),
        kind: RefKind::Tag,
        target: oid('a'),
        peeled: Some(peeled.clone()),
        upstream: None,
        subject: "release".into(),
        timestamp: None,
        is_head: false,
    };
    let effects = app.apply_refs(vec![reference]);
    assert!(
        matches!(effects.as_slice(), [Effect::LoadPreview { revision, .. }] if revision == &peeled.to_string())
    );
    let effects = app.update(Action::Open, 10);
    assert_eq!(app.revision, peeled.to_string());
    assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));

    let path = GitPath::new(b"src/lib.rs".to_vec());
    app.inspect.blame_path = Some(path.clone());
    app.view = View::Blame;
    app.view_stack.clear();
    app.inspect.blame = vec![BlameLine {
        final_line: 1,
        original_line: 1,
        id: oid('a'),
        author: "Pat".into(),
        author_mail: "pat@example.invalid".into(),
        author_time: Some(0),
        summary: "line".into(),
        filename: path.clone(),
        content: "text".into(),
        boundary: false,
        previous: None,
    }];
    let _ = app.update(Action::Open, 10);
    assert_eq!(app.preview_paths(), vec![path]);
}

#[test]
fn non_log_start_and_switch_do_not_claim_history_is_loading() {
    let mut app = App::new(repository(), "HEAD".into(), Vec::new(), false);
    app.set_start_view(View::Status, None, "HEAD".into(), ComparisonMode::Exact);
    assert!(!app.history_loading);
    let effects = app.update(Action::Back, 10);
    assert!(app.history_loading);
    assert!(matches!(effects.as_slice(), [Effect::LoadHistory { .. }]));
    let _ = app.update(Action::ViewRefs, 10);
    assert!(!app.history_loading);
}

#[test]
fn pagination_effect_is_bounded_and_idempotent_while_loading() {
    let mut app = app();
    app.has_more = true;
    app.history_loading = false;
    app.selected = app.commits.len() - 1;
    let effects = app.request_more_if_needed();
    assert_eq!(
        effects,
        [Effect::LoadHistory {
            offset: 2,
            limit: PAGE_SIZE
        }]
    );
    assert!(app.request_more_if_needed().is_empty());
}
