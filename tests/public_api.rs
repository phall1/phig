use std::{io, path::PathBuf};

use phig_cli::{
    app::{
        Action, App, Effect, Focus, InspectState, Overlay, RequestFailure, RequestKind,
        SelectionContract, SelectionTarget, View, palette_commands,
    },
    cli::SelectionKind,
    config::KeyBindings,
    domain::{ObjectFormat, Repository},
    git::GitClient,
    inspect,
    protocol::SelectionPayload,
    tui::{
        RenderTheme, TerminalSession, TuiError, handle_help_key, render, run, run_configured,
        run_select, set_color_mode, set_date_mode, set_theme,
    },
};

#[allow(clippy::type_complexity)]
type RunSelect = fn(
    App,
    GitClient,
    bool,
    KeyBindings,
    SelectionKind,
    bool,
    bool,
) -> Result<Option<SelectionPayload>, TuiError>;

#[test]
fn documented_public_facades_remain_source_compatible() {
    let repository = Repository {
        root: PathBuf::from("/tmp/repo"),
        worktree: Some(PathBuf::from("/tmp/repo")),
        git_dir: PathBuf::from("/tmp/repo/.git"),
        bare: false,
        object_format: ObjectFormat::Sha1,
        git_version: "2.45.1".into(),
        head: None,
        branch: Some("main".into()),
    };
    let app = App::new(repository, "HEAD".into(), Vec::new(), false);
    let _: InspectState = inspect::InspectState::new();
    let _: Option<Action> = Action::from_semantic_name("open");
    assert_eq!(Action::Open.semantic_name(), "open");
    assert!(!palette_commands("").is_empty());

    let _types = (
        std::mem::size_of::<Effect>(),
        std::mem::size_of::<Focus>(),
        std::mem::size_of::<Overlay>(),
        std::mem::size_of::<RequestFailure>(),
        std::mem::size_of::<RequestKind>(),
        std::mem::size_of::<SelectionContract>(),
        std::mem::size_of::<SelectionTarget>(),
        std::mem::size_of::<SelectionPayload>(),
        std::mem::size_of::<TerminalSession>(),
        std::mem::size_of::<TuiError>(),
        std::mem::size_of::<View>(),
    );
    let _render: fn(&mut ratatui::Frame<'_>, &App) = render;
    let _run: fn(App, GitClient, bool) -> Result<(), TuiError> = run;
    let _configured: fn(App, GitClient, bool, KeyBindings, bool, bool) -> Result<(), TuiError> =
        run_configured;
    let _select: RunSelect = run_select;
    let _help: fn(&mut App, &crossterm::event::KeyEvent) -> bool = handle_help_key;
    let _theme: fn(RenderTheme) = set_theme;
    let _date: fn(&str) = set_date_mode;
    let _color: fn(&str) = set_color_mode;

    // Keep the constructed value live so this test also checks App remains public.
    assert_eq!(app.view, View::Log);
    let _: io::Result<()> = Ok(());
}
