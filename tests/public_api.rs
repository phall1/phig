use std::{
    io,
    path::{Path, PathBuf},
};

use phig_cli::{
    app::{
        Action, App, Effect, Focus, InspectState, Overlay, RequestFailure, RequestKind,
        SelectionContract, SelectionTarget, View, palette_commands,
    },
    cli::SelectionKind,
    config::{
        CONFIG_VERSION, CompareConfig, Config, ConfigError, DiffConfig, KeyBindings, LimitsConfig,
        LoadedConfig, ThemeConfig, UiConfig, default_path, init, load, parse_color, validate,
    },
    domain::{ObjectFormat, Repository},
    git::GitClient,
    inspect,
    protocol::SelectionPayload,
    tui::{
        ColorMode, DateMode, GlyphMode, RenderConfig, RenderContext, RenderTheme, TerminalSession,
        TuiError, TuiOptions, handle_help_key, render, render_with_context, run, run_configured,
        run_select, run_select_with_options, run_with_options, set_color_mode, set_date_mode,
        set_theme,
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

    let _config_types = (
        CONFIG_VERSION,
        std::mem::size_of::<Config>(),
        std::mem::size_of::<UiConfig>(),
        std::mem::size_of::<DiffConfig>(),
        std::mem::size_of::<CompareConfig>(),
        std::mem::size_of::<LimitsConfig>(),
        std::mem::size_of::<ThemeConfig>(),
        std::mem::size_of::<LoadedConfig>(),
        std::mem::size_of::<ConfigError>(),
    );
    let _config = Config::default();
    let _default_path: fn() -> Result<PathBuf, ConfigError> = default_path;
    let _load: for<'a> fn(Option<&'a Path>, bool) -> Result<LoadedConfig, ConfigError> = load;
    let _validate: fn(&Config, &Path) -> Result<(), ConfigError> = validate;
    let _parse_color: fn(&str) -> Option<ratatui::style::Color> = parse_color;
    let _init: fn(&Path, bool) -> Result<(), ConfigError> = init;

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
    let _render_explicit: fn(&mut ratatui::Frame<'_>, &App, &RenderContext) = render_with_context;
    let _run: fn(App, GitClient, bool) -> Result<(), TuiError> = run;
    let _configured: fn(App, GitClient, bool, KeyBindings, bool, bool) -> Result<(), TuiError> =
        run_configured;
    let _select: RunSelect = run_select;
    let _options = TuiOptions::default();
    let _render_config = RenderConfig::default();
    let _glyphs = [GlyphMode::Auto, GlyphMode::Unicode, GlyphMode::Ascii];
    let _dates = [
        DateMode::Relative,
        DateMode::Local,
        DateMode::Iso,
        DateMode::Unix,
    ];
    let _colors = [ColorMode::Auto, ColorMode::Always, ColorMode::Never];
    let _run_options: fn(App, GitClient, TuiOptions) -> Result<(), TuiError> = run_with_options;
    let _select_options: fn(
        App,
        GitClient,
        SelectionKind,
        TuiOptions,
    ) -> Result<Option<SelectionPayload>, TuiError> = run_select_with_options;
    let _help: fn(&mut App, &crossterm::event::KeyEvent) -> bool = handle_help_key;
    let _theme: fn(RenderTheme) = set_theme;
    let _date: fn(&str) = set_date_mode;
    let _color: fn(&str) = set_color_mode;

    // Keep the constructed value live so this test also checks App remains public.
    assert_eq!(app.view, View::Log);
    let _: io::Result<()> = Ok(());
}
