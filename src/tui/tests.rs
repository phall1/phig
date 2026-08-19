use std::{collections::HashMap, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{Action, App, Effect, Overlay, RequestKind, View},
    config,
    domain::{Commit, HistoryPage, ObjectFormat, Oid, Repository, Signature, Status},
    git::{GitClient, GitError},
    runtime::{Coordinator, GitQuery, GitResult, RequestKey, Response},
};

use super::{driver::apply_paste, effects::*, input::*, render};

fn app() -> App {
    App::new(
        Repository {
            root: PathBuf::from("/tmp/repo"),
            worktree: Some(PathBuf::from("/tmp/repo")),
            git_dir: PathBuf::from("/tmp/repo/.git"),
            bare: false,
            object_format: ObjectFormat::Sha1,
            git_version: "2.45.1".into(),
            head: None,
            branch: Some("main".into()),
        },
        "HEAD".into(),
        Vec::new(),
        false,
    )
}

#[cfg(unix)]
#[test]
fn a_full_worker_queue_coalesces_instead_of_failing_the_tui() {
    use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

    let directory = tempfile::tempdir().unwrap();
    let helper = directory.path().join("fake-git");
    let marker = directory.path().join("started");
    fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\nsleep 30\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = crate::git::GitRunner::new(
        helper,
        crate::git::GitLimits {
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            timeout: Duration::from_secs(10),
        },
    );
    let coordinator = Coordinator::new(GitClient::new(runner), 1, 1);
    let repository = app().repository;
    let query = || GitQuery::Refs {
        repository: repository.clone(),
    };
    coordinator.submit(RequestKey::Custom(1), query()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !marker.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists());
    coordinator.submit(RequestKey::History, query()).unwrap();

    let mut pending = HashMap::new();
    submit_or_defer(&coordinator, &mut pending, RequestKey::Preview, query()).unwrap();
    assert!(pending.contains_key(&RequestKey::Preview));
}

#[test]
fn pasted_search_returns_every_generated_effect_for_dispatch() {
    let mut app = app();
    let oid: Oid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".parse().unwrap();
    let commit = Commit {
        id: oid,
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
        subject: "needle".into(),
        body: String::new(),
    };
    let _ = app.apply_history(HistoryPage {
        commits: vec![commit],
        offset: 0,
        limit: 256,
        has_more: false,
    });
    let _ = app.update(Action::StartSearch, 10);
    let effects = apply_paste(&mut app, "ne", 10);
    assert_eq!(
        effects.len(),
        2,
        "paste dropped an incremental preview effect"
    );
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::LoadPreview { .. }))
    );
    let coordinator = Coordinator::new(GitClient::default(), 1, 8);
    let mut pending = HashMap::new();
    dispatch_effects(&coordinator, &app, effects, &mut pending).unwrap();
    assert!(
        coordinator.is_current(RequestKey::Preview, 2),
        "both pasted-search effects were not dispatched/coalesced"
    );
}

#[test]
fn overlay_text_and_file_picker_paste_bypass_global_remaps() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    std::fs::write(&path, "version = 1\n[keys]\nquit = \"x\"\n").unwrap();
    let bindings = config::load(Some(&path), false).unwrap().bindings;
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);

    let mut app = app();
    for overlay in [
        Overlay::Search {
            draft: String::new(),
            previous_query: String::new(),
            original_selected: 0,
            original_inspect_selected: 0,
            original_scroll: 0,
        },
        Overlay::Palette {
            draft: String::new(),
            selected: 0,
        },
        Overlay::FilePicker {
            draft: String::new(),
            selected: 0,
            original_scroll: 0,
        },
    ] {
        app.overlay = overlay;
        assert_eq!(
            resolve_action(&app, &bindings, key),
            Some(Action::SearchInput('x'))
        );
    }

    app.overlay = Overlay::FilePicker {
        draft: String::new(),
        selected: 0,
        original_scroll: 0,
    };
    let effects = apply_paste(&mut app, "xy", 10);
    assert!(effects.is_empty());
    assert!(matches!(
        &app.overlay,
        Overlay::FilePicker { draft, .. } if draft == "xy"
    ));
}

#[test]
fn invalidated_status_responses_cannot_mutate_a_new_tree_view() {
    for result in [
        Ok(GitResult::Status(Status {
            branch: Some("stale".into()),
            ..Status::default()
        })),
        Err(GitError::Timeout("status")),
    ] {
        let mut app = app();
        app.view = View::Status;
        let coordinator = Coordinator::new(GitClient::default(), 1, 8);
        let generation = coordinator
            .submit(
                RequestKey::Status,
                GitQuery::Status {
                    repository: app.repository.clone(),
                    include_ignored: false,
                },
            )
            .unwrap();
        let _ = app.update(Action::ViewTree, 10);
        let mut pending = HashMap::new();
        invalidate_for_transition(&coordinator, &mut pending, View::Status, app.view);
        apply_response(
            &mut app,
            &coordinator,
            Response {
                key: RequestKey::Status,
                generation,
                result,
            },
            &mut pending,
        )
        .unwrap();
        assert_eq!(app.view, View::Tree);
        assert!(app.inspect.status.is_none());
        assert!(app.inspect_error.is_none());
        assert!(app.inspect.loading);
    }
}

#[test]
fn preview_responses_are_invalidated_across_distinct_view_contexts() {
    let mut app = app();
    let coordinator = Coordinator::new(GitClient::default(), 1, 8);
    let generation = coordinator
        .submit(
            RequestKey::Preview,
            GitQuery::CommitDetail {
                repository: app.repository.clone(),
                revision: "HEAD".into(),
                parent_index: 0,
                paths: Vec::new(),
            },
        )
        .unwrap();
    let previous = app.view;
    let _ = app.update(Action::ViewRefs, 10);
    let mut pending = HashMap::new();
    invalidate_for_transition(&coordinator, &mut pending, previous, app.view);
    apply_response(
        &mut app,
        &coordinator,
        Response {
            key: RequestKey::Preview,
            generation,
            result: Err(GitError::Timeout("preview")),
        },
        &mut pending,
    )
    .unwrap();
    assert_eq!(app.view, View::Refs);
    assert!(app.preview_error.is_none());
    assert!(app.inspect_error.is_none());
}

#[test]
fn page_keys_use_each_layouts_real_history_height() {
    let mut app = app();
    let commits = (0_u64..40)
        .map(|index| Commit {
            id: format!("{index:040x}").parse().unwrap(),
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
            subject: format!("commit {index}"),
            body: String::new(),
        })
        .collect();
    let _ = app.apply_history(HistoryPage {
        commits,
        offset: 0,
        limit: 256,
        has_more: false,
    });
    for (width, expected) in [(60, 28), (100, 13), (140, 28)] {
        app.selected = 0;
        app.selected_oid = app.selected_commit().map(|commit| commit.id.clone());
        let rows = render::page_rows(&app, width, 30);
        let _ = app.update(Action::Page(1), rows);
        assert_eq!(app.selected, expected, "wrong page at width {width}");
    }
}

#[test]
fn keymap_uses_semantic_actions() {
    let mut app = app();
    assert_eq!(
        key_action(&app, KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
        Some(Action::Move(1))
    );
    assert!(handle_help_key(
        &mut app,
        &KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)
    ));
    assert_eq!(app.overlay, Overlay::Help);
    app.overlay = Overlay::None;
    assert_eq!(
        key_action(&app, KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE)),
        Some(Action::StartPalette)
    );
    assert_eq!(
        key_action(
            &app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)
        ),
        Some(Action::Redraw)
    );
    for (key, expected) in [
        ('m', Action::ViewLog),
        ('r', Action::ViewRefs),
        ('s', Action::ViewStatus),
        ('t', Action::ViewTree),
        ('b', Action::ViewBlame),
        ('z', Action::ViewStash),
        ('v', Action::Mark),
        ('c', Action::StartCompare),
    ] {
        assert_eq!(
            key_action(&app, KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE)),
            Some(expected)
        );
    }
    app.apply_error(
        RequestKind::History,
        &crate::git::GitError::Timeout("history"),
    );
    assert_eq!(
        key_action(&app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(Action::DismissErrors)
    );
    assert_eq!(
        key_action(&app, KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        Some(Action::RetryFailed)
    );
}
