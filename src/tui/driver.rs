//! Terminal event loop and lifecycle orchestration.

use std::{collections::HashMap, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::{
    app::{Action, App, Effect, Overlay},
    cli::SelectionKind,
    config::KeyBindings,
    git::GitClient,
    protocol::{SelectionPayload, selection_from_app},
    runtime::Coordinator,
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
    mut app: App,
    client: GitClient,
    selection_kind: Option<SelectionKind>,
    options: TuiOptions,
    mut session: TerminalSession,
) -> Result<Option<SelectionPayload>, TuiError> {
    let coordinator = Coordinator::new(client, 2, 128);
    let render_context =
        RenderContext::with_bindings(options.render.clone(), options.bindings.clone());
    let mut pending = HashMap::new();
    #[cfg(unix)]
    let signals = SignalMonitor::new()?;
    let mut terminating_signal = None;
    let mut selection = None;
    let size = session.terminal_mut().size()?;
    app.set_preview_focus_available(render::preview_focus_available(
        &app,
        size.width,
        size.height,
    ));
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
                    session = if selection_kind.is_some() {
                        TerminalSession::enter_controlling_tty(
                            options.no_alt_screen,
                            options.mouse,
                        )?
                    } else {
                        TerminalSession::enter_configured(options.no_alt_screen, options.mouse)?
                    };
                    let size = session.terminal_mut().size()?;
                    app.set_preview_focus_available(render::preview_focus_available(
                        &app,
                        size.width,
                        size.height,
                    ));
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
                .draw(|frame| render::render_with_context(frame, &app, &render_context))?;
            app.dirty = false;
        }
        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
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
                    let size = session.terminal_mut().size()?;
                    app.set_preview_focus_available(render::preview_focus_available(
                        &app,
                        size.width,
                        size.height,
                    ));
                    let action = resolve_action(&app, &options.bindings, key);
                    if let Some(kind) = selection_kind
                        && matches!(app.overlay, Overlay::None)
                    {
                        if action == Some(Action::Open)
                            && let Some(value) = selection_from_app(&app, kind)
                        {
                            selection = Some(value);
                            app.should_quit = true;
                            continue;
                        }
                        if matches!(action, Some(Action::Quit | Action::Back)) {
                            app.should_quit = true;
                            continue;
                        }
                    }
                    if let Some(action) = action {
                        let rows = render::page_rows(&app, size.width, size.height);
                        let previous_view = app.view;
                        let effects = app.update(action, rows);
                        if app.view != previous_view {
                            invalidate_for_transition(
                                &coordinator,
                                &mut pending,
                                previous_view,
                                app.view,
                            );
                        }
                        if app.take_copy_request() {
                            if options.clipboard_osc52 {
                                if let Some(value) = app.copy_value() {
                                    session.copy_osc52(&value)?;
                                    app.set_notice("Copied selection with OSC 52");
                                } else {
                                    app.set_notice("Nothing stable to copy here");
                                }
                            } else {
                                app.set_notice(
                                    "Clipboard copy is disabled by ui.clipboard = \"off\"",
                                );
                            }
                        }
                        if app.take_redraw_request() {
                            session.force_redraw()?;
                            app.dirty = true;
                        }
                        dispatch_effects(&coordinator, &app, effects, &mut pending)?;
                    }
                }
                Event::Paste(value) => {
                    if matches!(
                        app.overlay,
                        Overlay::Search { .. }
                            | Overlay::Palette { .. }
                            | Overlay::FilePicker { .. }
                    ) {
                        let size = session.terminal_mut().size()?;
                        let rows = render::page_rows(&app, size.width, size.height);
                        let effects = apply_paste(&mut app, &value, rows);
                        dispatch_effects(&coordinator, &app, effects, &mut pending)?;
                    }
                }
                Event::Mouse(event) => {
                    let action = match event.kind {
                        MouseEventKind::ScrollDown => Some(Action::Move(3)),
                        MouseEventKind::ScrollUp => Some(Action::Move(-3)),
                        _ => None,
                    };
                    if let Some(action) = action {
                        let size = session.terminal_mut().size()?;
                        app.set_preview_focus_available(render::preview_focus_available(
                            &app,
                            size.width,
                            size.height,
                        ));
                        let rows = render::page_rows(&app, size.width, size.height);
                        let effects = app.update(action, rows);
                        dispatch_effects(&coordinator, &app, effects, &mut pending)?;
                    }
                }
                Event::Resize(width, height) => app.set_preview_focus_available(
                    render::preview_focus_available(&app, width, height),
                ),
                Event::FocusGained | Event::FocusLost => {}
                _ => {}
            }
        }
    }
    session.restore()?;
    if let Some(signal) = terminating_signal {
        Err(TuiError::Terminated(signal))
    } else {
        Ok(selection)
    }
}

pub(super) fn apply_paste(app: &mut App, value: &str, page_rows: usize) -> Vec<Effect> {
    value
        .chars()
        .filter(|character| !character.is_control())
        .flat_map(|character| app.update(Action::SearchInput(character), page_rows))
        .collect()
}
