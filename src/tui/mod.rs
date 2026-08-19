mod render;
mod session;

use std::{collections::HashMap, io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use thiserror::Error;

use crate::{
    app::{Action, App, Effect, Overlay, RequestKind, View},
    git::GitClient,
    runtime::{Coordinator, CoordinatorError, GitQuery, GitResult, RequestKey, Response},
};

pub use render::render;
pub use session::TerminalSession;

#[derive(Debug, Error)]
pub enum TuiError {
    #[error("terminal error: {0}")]
    Terminal(#[from] io::Error),
    #[error("Git worker error: {0}")]
    Coordinator(#[from] CoordinatorError),
    #[error("terminated by signal {0}")]
    Terminated(i32),
}

pub fn run(mut app: App, client: GitClient, no_alt_screen: bool) -> Result<(), TuiError> {
    // The documented global bound is 128. Cancelled preview generations are
    // skipped by workers, so key-repeat bursts remain responsive without
    // turning a full queue into a terminal-fatal error.
    let coordinator = Coordinator::new(client, 2, 128);
    let mut pending = HashMap::new();
    #[cfg(unix)]
    let signals = SignalMonitor::new()?;
    let mut session = TerminalSession::enter(no_alt_screen)?;
    let mut terminating_signal = None;
    dispatch_effects(&coordinator, &app, app.initial_effects(), &mut pending)?;

    while !app.should_quit {
        #[cfg(unix)]
        while let Some(signal) = signals.try_recv() {
            use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGTSTP};
            match signal {
                SIGINT | SIGTERM | SIGHUP => {
                    terminating_signal = Some(signal);
                    app.should_quit = true;
                }
                SIGTSTP => {
                    session.restore()?;
                    signal_hook::low_level::emulate_default_handler(SIGTSTP)?;
                    session = TerminalSession::enter(no_alt_screen)?;
                    app.dirty = true;
                }
                _ => {}
            }
        }
        retry_pending(&coordinator, &mut pending)?;
        while let Some(response) = coordinator.try_recv() {
            apply_response(&mut app, &coordinator, response, &mut pending)?;
        }

        if app.dirty {
            session
                .terminal_mut()
                .draw(|frame| render::render(frame, &app))?;
            app.dirty = false;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.code == KeyCode::Char('l')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        session.force_redraw()?;
                        app.dirty = true;
                        continue;
                    }
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        #[cfg(unix)]
                        {
                            terminating_signal = Some(signal_hook::consts::signal::SIGINT);
                        }
                        app.should_quit = true;
                        continue;
                    }
                    if handle_help_key(&mut app, &key) {
                        continue;
                    }
                    if let Some(action) = key_action(&app, key) {
                        let size = session.terminal_mut().size()?;
                        let page_rows = render::page_rows(&app, size.width, size.height);
                        let effects = app.update(action, page_rows);
                        dispatch_effects(&coordinator, &app, effects, &mut pending)?;
                    }
                }
                Event::Paste(value) => {
                    if matches!(
                        app.overlay,
                        Overlay::Search { .. } | Overlay::Palette { .. }
                    ) {
                        let size = session.terminal_mut().size()?;
                        let page_rows = render::page_rows(&app, size.width, size.height);
                        let effects = apply_paste(&mut app, &value, page_rows);
                        dispatch_effects(&coordinator, &app, effects, &mut pending)?;
                    }
                }
                Event::Resize(width, height) => app.resize(width, height),
                Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
                _ => {}
            }
        }
    }

    session.restore()?;
    if let Some(signal) = terminating_signal {
        Err(TuiError::Terminated(signal))
    } else {
        Ok(())
    }
}

fn apply_paste(app: &mut App, value: &str, page_rows: usize) -> Vec<Effect> {
    value
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(|character| app.update(Action::SearchInput(character), page_rows))
        .collect()
}

fn apply_response(
    app: &mut App,
    coordinator: &Coordinator,
    response: Response,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    if !coordinator.is_current(response.key, response.generation) {
        return Ok(());
    }
    match response.result {
        Ok(GitResult::History(page)) => {
            let effects = app.apply_history(page);
            dispatch_effects(coordinator, app, effects, pending)?;
        }
        Ok(GitResult::CommitDetail(detail)) => app.apply_preview(detail),
        Ok(_) => {}
        Err(error) => {
            let request = match response.key {
                RequestKey::History => RequestKind::History,
                RequestKey::Preview => RequestKind::Preview,
                _ => return Ok(()),
            };
            app.apply_error(request, &error);
        }
    }
    Ok(())
}

fn dispatch_effects(
    coordinator: &Coordinator,
    app: &App,
    effects: Vec<Effect>,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    for effect in effects {
        match effect {
            Effect::LoadHistory { offset, limit } => {
                let query = GitQuery::History {
                    repository: app.repository.clone(),
                    revision: app.revision.clone(),
                    paths: if app.show_mode {
                        Vec::new()
                    } else {
                        app.paths.clone()
                    },
                    offset,
                    limit,
                };
                submit_or_defer(coordinator, pending, RequestKey::History, query)?;
            }
            Effect::LoadPreview {
                revision,
                parent_index,
            } => {
                let query = GitQuery::CommitDetail {
                    repository: app.repository.clone(),
                    revision,
                    parent_index,
                    paths: app.paths.clone(),
                };
                submit_or_defer(coordinator, pending, RequestKey::Preview, query)?;
            }
        }
    }
    Ok(())
}

fn submit_or_defer(
    coordinator: &Coordinator,
    pending: &mut HashMap<RequestKey, GitQuery>,
    key: RequestKey,
    query: GitQuery,
) -> Result<(), TuiError> {
    match coordinator.submit(key, query.clone()) {
        Ok(_) => {
            pending.remove(&key);
            Ok(())
        }
        Err(CoordinatorError::Busy) => {
            // Same-key work is coalesced: only the newest desired query is
            // retained and retried after workers drain cancelled generations.
            pending.insert(key, query);
            Ok(())
        }
        Err(error @ CoordinatorError::Stopped) => Err(error.into()),
    }
}

fn retry_pending(
    coordinator: &Coordinator,
    pending: &mut HashMap<RequestKey, GitQuery>,
) -> Result<(), TuiError> {
    let queued: Vec<_> = pending
        .iter()
        .map(|(key, query)| (*key, query.clone()))
        .collect();
    for (key, query) in queued {
        submit_or_defer(coordinator, pending, key, query)?;
    }
    Ok(())
}

fn key_action(app: &App, key: KeyEvent) -> Option<Action> {
    match &app.overlay {
        Overlay::Help => {
            return match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                    Some(Action::CancelOverlay)
                }
                _ => None,
            };
        }
        Overlay::Search { .. } => {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelOverlay),
                KeyCode::Enter => Some(Action::AcceptSearch),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(Action::SearchInput(character))
                }
                _ => None,
            };
        }
        Overlay::Palette { .. } => {
            return match key.code {
                KeyCode::Esc => Some(Action::CancelOverlay),
                KeyCode::Enter => Some(Action::ExecutePalette),
                KeyCode::Down => Some(Action::PaletteMove(1)),
                KeyCode::Up => Some(Action::PaletteMove(-1)),
                KeyCode::Backspace => Some(Action::SearchBackspace),
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    Some(Action::SearchInput(character))
                }
                _ => None,
            };
        }
        Overlay::None => {}
    }

    if app.has_errors() {
        match key.code {
            KeyCode::Char('r') => return Some(Action::RetryFailed),
            KeyCode::Esc => return Some(Action::DismissErrors),
            _ => {}
        }
    }

    match key.code {
        KeyCode::Down | KeyCode::Char('j') => Some(Action::Move(1)),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::Move(-1)),
        KeyCode::PageDown => Some(Action::Page(1)),
        KeyCode::PageUp => Some(Action::Page(-1)),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Page(1))
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Page(-1))
        }
        KeyCode::Home | KeyCode::Char('g') => Some(Action::First),
        KeyCode::End | KeyCode::Char('G') => Some(Action::Last),
        KeyCode::Enter => Some(Action::Open),
        KeyCode::Esc | KeyCode::Backspace => Some(Action::Back),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('/') => Some(Action::StartSearch),
        KeyCode::Char(':') => Some(Action::StartPalette),
        KeyCode::Char('n') => Some(Action::NextMatch),
        KeyCode::Char('N') => Some(Action::PreviousMatch),
        KeyCode::Tab if app.view == View::Detail => Some(Action::NextFile(1)),
        KeyCode::BackTab if app.view == View::Detail => Some(Action::NextFile(-1)),
        KeyCode::Tab | KeyCode::BackTab => Some(Action::ToggleFocus),
        KeyCode::Char('p') => Some(Action::TogglePreview),
        KeyCode::Char('P') if app.view == View::Detail => Some(Action::NextParent),
        KeyCode::Char(']') => Some(Action::NextHunk(1)),
        KeyCode::Char('[') => Some(Action::NextHunk(-1)),
        KeyCode::Char('}') => Some(Action::NextFile(1)),
        KeyCode::Char('{') => Some(Action::NextFile(-1)),
        KeyCode::Char('?') => None,
        _ => None,
    }
}

#[cfg(unix)]
struct SignalMonitor {
    receiver: std::sync::mpsc::Receiver<i32>,
    handle: signal_hook::iterator::Handle,
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl SignalMonitor {
    fn new() -> io::Result<Self> {
        use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGTSTP};
        use signal_hook::iterator::Signals;

        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGTSTP])?;
        let handle = signals.handle();
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let worker = std::thread::spawn(move || {
            for signal in signals.forever() {
                if sender.send(signal).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            receiver,
            handle,
            worker: Some(worker),
        })
    }

    fn try_recv(&self) -> Option<i32> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(unix)]
impl Drop for SignalMonitor {
    fn drop(&mut self) {
        self.handle.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Handle keys that mutate state outside the regular action return path.
pub fn handle_help_key(app: &mut App, key: &KeyEvent) -> bool {
    if matches!(app.overlay, Overlay::None) && key.code == KeyCode::Char('?') {
        app.show_help();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::KeyEvent;

    use crate::domain::{Commit, HistoryPage, ObjectFormat, Oid, Repository, Signature};

    use super::*;

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
}
