//! Terminal event loop and lifecycle orchestration.

use std::{collections::HashMap, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::{
    app::{Action, App, Effect, Overlay},
    cli::SelectionKind,
    config::KeyBindings,
    git::GitClient,
    protocol::{SelectionPayload, selection_from_app},
    runtime::{Coordinator, GitQuery, GitResult, RequestKey},
};

#[cfg(unix)]
use super::signals::SignalMonitor;
use super::{
    TuiError,
    effects::{apply_response, dispatch_effects, invalidate_for_transition, retry_pending},
    input::resolve_action,
    render::{self, RenderConfig, RenderContext},
    session::TerminalSession,
};

#[derive(Debug, Clone, Default)]
pub struct TuiOptions {
    pub no_alt_screen: bool,
    pub mouse: bool,
    pub clipboard_osc52: bool,
    pub bindings: KeyBindings,
    pub render: RenderConfig,
}

pub fn run(app: App, client: GitClient, no_alt_screen: bool) -> Result<(), TuiError> {
    run_configured(
        app,
        client,
        no_alt_screen,
        KeyBindings::default(),
        false,
        false,
    )
}

pub fn run_configured(
    app: App,
    client: GitClient,
    no_alt_screen: bool,
    bindings: KeyBindings,
    mouse: bool,
    clipboard_osc52: bool,
) -> Result<(), TuiError> {
    let render_config = render::legacy_config();
    run_with_options(
        app,
        client,
        TuiOptions {
            no_alt_screen,
            mouse,
            clipboard_osc52,
            bindings,
            render: render_config,
        },
    )
}

pub fn run_with_options(app: App, client: GitClient, options: TuiOptions) -> Result<(), TuiError> {
    let session = TerminalSession::enter_configured(options.no_alt_screen, options.mouse)?;
    run_loop(app, client, None, options, session).map(|_| ())
}

pub fn run_select(
    app: App,
    client: GitClient,
    no_alt_screen: bool,
    bindings: KeyBindings,
    kind: SelectionKind,
    mouse: bool,
    clipboard_osc52: bool,
) -> Result<Option<SelectionPayload>, TuiError> {
    let render_config = render::legacy_config();
    run_select_with_options(
        app,
        client,
        kind,
        TuiOptions {
            no_alt_screen,
            mouse,
            clipboard_osc52,
            bindings,
            render: render_config,
        },
    )
}

pub fn run_select_with_options(
    app: App,
    client: GitClient,
    kind: SelectionKind,
    options: TuiOptions,
) -> Result<Option<SelectionPayload>, TuiError> {
    let session = TerminalSession::enter_controlling_tty(options.no_alt_screen, options.mouse)
        .map_err(|error| {
            if error.to_string().contains("controlling terminal") {
                TuiError::NoControllingTerminal(error.to_string())
            } else {
                TuiError::Terminal(error)
            }
        })?;
    run_loop(app, client, Some(kind), options, session)
}

fn run_loop(
    app: App,
    client: GitClient,
    selection_kind: Option<SelectionKind>,
    options: TuiOptions,
    mut session: TerminalSession,
) -> Result<Option<SelectionPayload>, TuiError> {
    let render_context =
        RenderContext::with_bindings(options.render.clone(), options.bindings.clone());
    let mut state = LoopState {
        app,
        coordinator: Coordinator::new(client, 2, 128),
        options,
        selection_kind,
        pending: HashMap::new(),
        selection: None,
        terminating_signal: None,
        render: render::RenderState::default(),
    };
    #[cfg(unix)]
    let signals = SignalMonitor::new()?;
    state.page_rows(&mut session)?;
    state.dispatch(state.app.initial_effects())?;
    while !state.app.should_quit {
        #[cfg(unix)]
        state.drain_signals(&signals, &mut session)?;
        if state.app.should_quit {
            break;
        }
        state.tick(&mut session, &render_context)?;
    }
    session.restore()?;
    if let Some(signal) = state.terminating_signal {
        Err(TuiError::Terminated(signal))
    } else {
        Ok(state.selection)
    }
}

struct LoopState {
    app: App,
    coordinator: Coordinator,
    options: TuiOptions,
    selection_kind: Option<SelectionKind>,
    pending: HashMap<RequestKey, GitQuery>,
    selection: Option<SelectionPayload>,
    terminating_signal: Option<i32>,
    render: render::RenderState,
}

impl LoopState {
    #[cfg(unix)]
    fn drain_signals(
        &mut self,
        signals: &SignalMonitor,
        session: &mut TerminalSession,
    ) -> Result<(), TuiError> {
        while let Some(signal) = signals.try_recv() {
            self.signal(signal, session)?;
        }
        Ok(())
    }

    fn tick(
        &mut self,
        session: &mut TerminalSession,
        context: &RenderContext,
    ) -> Result<(), TuiError> {
        retry_pending(&self.coordinator, &mut self.pending)?;
        self.receive()?;
        if self.app.dirty {
            session.terminal_mut().draw(|frame| {
                render::render_with_state(frame, &self.app, context, &mut self.render)
            })?;
            self.app.dirty = false;
        }
        self.read_events(session)
    }

    fn dispatch(&mut self, effects: Vec<Effect>) -> Result<(), TuiError> {
        dispatch_effects(&self.coordinator, &self.app, effects, &mut self.pending)
    }

    fn page_rows(&mut self, session: &mut TerminalSession) -> Result<usize, TuiError> {
        let size = session.terminal_mut().size()?;
        self.app
            .set_preview_focus_available(render::preview_focus_available(
                &self.app,
                size.width,
                size.height,
            ));
        Ok(render::page_rows(&self.app, size.width, size.height))
    }

    fn receive(&mut self) -> Result<(), TuiError> {
        while let Some(response) = self.coordinator.try_recv() {
            if matches!(&response.result, Ok(GitResult::History(page)) if page.offset == 0)
                && self
                    .coordinator
                    .is_current(response.key, response.generation)
            {
                self.render.clear_history();
            }
            apply_response(
                &mut self.app,
                &self.coordinator,
                response,
                &mut self.pending,
            )?;
        }
        Ok(())
    }

    fn read_events(&mut self, session: &mut TerminalSession) -> Result<(), TuiError> {
        if !event::poll(Duration::from_millis(16))? {
            return Ok(());
        }
        // Drain a bounded burst before painting; held keys and pasted input need
        // one current frame, not a chain of expensive intermediate frames.
        for _ in 0..32 {
            self.event(event::read()?, session)?;
            if self.app.should_quit || !event::poll(Duration::ZERO)? {
                break;
            }
        }
        Ok(())
    }

    fn event(&mut self, event: Event, session: &mut TerminalSession) -> Result<(), TuiError> {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(key, session)?,
            Event::Paste(value) => self.paste(&value, session)?,
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollDown => self.action(Action::Move(3), session)?,
                MouseEventKind::ScrollUp => self.action(Action::Move(-3), session)?,
                _ => {}
            },
            Event::Resize(width, height) => {
                self.app
                    .set_preview_focus_available(render::preview_focus_available(
                        &self.app, width, height,
                    ));
                self.app.dirty = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn key(
        &mut self,
        key: crossterm::event::KeyEvent,
        session: &mut TerminalSession,
    ) -> Result<(), TuiError> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            #[cfg(unix)]
            {
                self.terminating_signal = Some(signal_hook::consts::signal::SIGINT);
            }
            self.app.should_quit = true;
            return Ok(());
        }
        let Some(action) = resolve_action(&self.app, &self.options.bindings, key) else {
            return Ok(());
        };
        if !self.select(&action) {
            self.action(action, session)?;
        }
        Ok(())
    }

    fn select(&mut self, action: &Action) -> bool {
        let Some(kind) = self.selection_kind else {
            return false;
        };
        if !matches!(self.app.overlay, Overlay::None) {
            return false;
        }
        if *action == Action::Open
            && let Some(value) = selection_from_app(&self.app, kind)
        {
            self.selection = Some(value);
            self.app.should_quit = true;
            return true;
        }
        if matches!(action, Action::Quit | Action::Back) {
            self.app.should_quit = true;
            return true;
        }
        false
    }

    fn action(&mut self, action: Action, session: &mut TerminalSession) -> Result<(), TuiError> {
        let rows = self.page_rows(session)?;
        let previous = self.app.view;
        let effects = self.app.update(action, rows);
        if self.app.view != previous {
            invalidate_for_transition(
                &self.coordinator,
                &mut self.pending,
                previous,
                self.app.view,
            );
        }
        if self.app.commits.is_empty() {
            self.render.clear_history();
        }
        if self.app.take_copy_request() {
            self.copy(session)?;
        }
        if self.app.take_redraw_request() {
            session.force_redraw()?;
            self.app.dirty = true;
        }
        self.dispatch(effects)
    }

    fn copy(&mut self, session: &mut TerminalSession) -> Result<(), TuiError> {
        if !self.options.clipboard_osc52 {
            self.app
                .set_notice("Clipboard copy is disabled by ui.clipboard = \"off\"");
        } else if let Some(value) = self.app.copy_value() {
            session.copy_osc52(&value)?;
            self.app.set_notice("Copied selection with OSC 52");
        } else {
            self.app.set_notice("Nothing stable to copy here");
        }
        Ok(())
    }

    fn paste(&mut self, value: &str, session: &mut TerminalSession) -> Result<(), TuiError> {
        if matches!(
            self.app.overlay,
            Overlay::Search { .. } | Overlay::Palette { .. } | Overlay::FilePicker { .. }
        ) {
            let rows = self.page_rows(session)?;
            let effects = apply_paste(&mut self.app, value, rows);
            self.dispatch(effects)?;
        }
        Ok(())
    }

    #[cfg(unix)]
    fn signal(&mut self, signal: i32, session: &mut TerminalSession) -> Result<(), TuiError> {
        use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM, SIGTSTP};
        match signal {
            SIGINT | SIGTERM | SIGHUP => {
                self.terminating_signal = Some(signal);
                self.app.should_quit = true;
            }
            SIGTSTP => {
                session.restore()?;
                signal_hook::low_level::emulate_default_handler(SIGTSTP)?;
                *session = if self.selection_kind.is_some() {
                    TerminalSession::enter_controlling_tty(
                        self.options.no_alt_screen,
                        self.options.mouse,
                    )?
                } else {
                    TerminalSession::enter_configured(
                        self.options.no_alt_screen,
                        self.options.mouse,
                    )?
                };
                self.page_rows(session)?;
                self.app.dirty = true;
            }
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn apply_paste(app: &mut App, value: &str, page_rows: usize) -> Vec<Effect> {
    value
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(|character| app.update(Action::SearchInput(character), page_rows))
        .collect()
}
