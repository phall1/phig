use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use phig_cli::{
    app::{App, SelectionContract, SelectionTarget, View},
    cli::{
        Cli, Command, CompletionShell, ConfigCommand, SelectionFormat, SelectionKind, StartView,
    },
    config::{self, ConfigError, LoadedConfig},
    domain::{ComparisonMode, GitPath},
    git::{CancellationToken, GitClient, GitError},
    protocol::{self, Envelope, SnapshotError},
    tui::{self, RenderTheme, TuiError},
    update::{self, UpdateError, UpdateResult},
};
use thiserror::Error;

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Tui(#[from] TuiError),
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error("{0}")]
    Unsupported(String),
    #[error("output error: {0}")]
    Output(#[from] io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("update unavailable: {0}")]
    Update(#[from] UpdateError),
}

enum Outcome {
    Success,
    Cancelled,
}

fn main() -> ExitCode {
    match run() {
        Ok(Outcome::Success) => ExitCode::SUCCESS,
        Ok(Outcome::Cancelled) => ExitCode::from(1),
        Err(MainError::Output(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("phig: {error}");
            ExitCode::from(exit_code(&error))
        }
    }
}

fn run() -> Result<Outcome, MainError> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Command::Completions { shell }) => {
            write_completions(*shell)?;
            return Ok(Outcome::Success);
        }
        Some(Command::Manpage(args)) => {
            write_manpages(args.output_dir.as_deref())?;
            return Ok(Outcome::Success);
        }
        Some(Command::Update(args)) => {
            run_update(args.check)?;
            return Ok(Outcome::Success);
        }
        Some(Command::Version(args)) => {
            if args.json {
                write_json(&protocol::version_json())?
            } else {
                write_text(&format!("phig {}\n", env!("CARGO_PKG_VERSION")))?
            };
            return Ok(Outcome::Success);
        }
        Some(Command::Config(args)) => return run_config_command(&cli, args.command.clone()),
        _ => {}
    }
    let loaded = config::load(cli.config.as_deref(), cli.no_config)?;
    apply_theme(&loaded);
    let client = GitClient::default()
        .with_content_limits(
            loaded.config.limits.patch_bytes,
            loaded.config.limits.blob_bytes,
        )
        .with_diff_options(
            loaded.config.diff.context,
            loaded.config.diff.algorithm.clone(),
            loaded.config.diff.whitespace.clone(),
        );
    if let Some(Command::Snapshot(args)) = &cli.command {
        let repo = client.discover(&cli.repo)?;
        let envelope = protocol::snapshot(
            &client,
            &repo,
            &args.target,
            args.offset,
            loaded.config.limits.snapshot_items,
        )?;
        write_json(&envelope)?;
        return Ok(Outcome::Success);
    }
    if let Some(Command::Select(args)) = &cli.command {
        return run_select(&cli, &loaded, client, args.clone());
    }
    run_interactive(cli, loaded, client)?;
    Ok(Outcome::Success)
}

fn run_update(check_only: bool) -> Result<(), MainError> {
    let message = match update::run(check_only)? {
        UpdateResult::Current { current } => format!("phig {current} is current\n"),
        UpdateResult::Available { current, latest } => {
            format!("phig {latest} is available (current {current}); run `phig update`\n")
        }
        UpdateResult::Updated {
            previous,
            installed,
            method,
        } => format!("updated phig {previous} to {installed} with {method}\n"),
    };
    write_text(&message)
}

fn run_config_command(cli: &Cli, command: ConfigCommand) -> Result<Outcome, MainError> {
    match command {
        ConfigCommand::Path => {
            let path = cli
                .config
                .clone()
                .map(Ok)
                .unwrap_or_else(config::default_path)?;
            write_text(&format!("{}\n", path.display()))?;
        }
        ConfigCommand::Init { force } => {
            if cli.no_config {
                return Err(MainError::Unsupported(
                    "config init cannot be combined with --no-config".into(),
                ));
            }
            let path = cli
                .config
                .clone()
                .map(Ok)
                .unwrap_or_else(config::default_path)?;
            config::init(&path, force)?;
            write_text(&format!("{}\n", path.display()))?;
        }
        ConfigCommand::Check => {
            let loaded = config::load(cli.config.as_deref(), cli.no_config)?;
            let message =
                if cli.no_config || loaded.path.as_ref().is_none_or(|value| !value.exists()) {
                    "configuration is valid: built-in defaults\n".to_owned()
                } else {
                    format!(
                        "configuration is valid: {}\n",
                        loaded
                            .path
                            .as_ref()
                            .expect("existing config path")
                            .display()
                    )
                };
            write_text(&message)?;
        }
    }
    Ok(Outcome::Success)
}

struct AppRequest {
    revision: String,
    paths: Vec<GitPath>,
    view: View,
    compare_base: Option<String>,
    compare_head: String,
    compare_mode: ComparisonMode,
}

fn configured_app(
    client: &GitClient,
    repo_path: &std::path::Path,
    request: AppRequest,
    config: &LoadedConfig,
) -> Result<App, MainError> {
    let repository = client.discover(repo_path)?;
    let show = request.view == View::Detail;
    let mut app = App::new(repository, request.revision, request.paths, show);
    app.set_history_page_size(config.config.limits.history_page);
    app.show_preview = config.config.ui.preview;
    if request.view == View::Tree {
        app.inspect.tree_path = app.paths.first().cloned()
    } else if request.view == View::Blame {
        app.inspect.blame_path = app.paths.first().cloned()
    }
    let (base, label) = if request.view == View::Compare && request.compare_base.is_none() {
        if let Some(preferred) = &config.config.compare.preferred_base {
            (Some(preferred.clone()), Some(preferred.clone()))
        } else {
            let (oid, label) =
                client.infer_compare_base(&app.repository, &CancellationToken::new())?;
            (Some(oid), Some(label))
        }
    } else {
        (request.compare_base.clone(), request.compare_base)
    };
    app.set_start_view(
        request.view,
        base,
        request.compare_head.clone(),
        request.compare_mode,
    );
    app.inspect.compare_base_label = label;
    app.inspect.compare_head_label =
        (request.view == View::Compare).then_some(request.compare_head);
    Ok(app)
}

fn run_interactive(cli: Cli, loaded: LoadedConfig, client: GitClient) -> Result<(), MainError> {
    let launch = cli.launch();
    let paths = launch.paths.into_iter().map(git_path_from_os).collect();
    let view = match launch.start_view {
        StartView::Log => View::Log,
        StartView::Show => View::Detail,
        StartView::Compare => View::Compare,
        StartView::Refs => View::Refs,
        StartView::Status => View::Status,
        StartView::Tree => View::Tree,
        StartView::Blame => View::Blame,
        StartView::Stash => View::Stash,
    };
    let mode = if launch.exact_compare {
        ComparisonMode::Exact
    } else if launch.explicit_compare {
        ComparisonMode::MergeBase
    } else {
        loaded.config.compare.mode()
    };
    let app = configured_app(
        &client,
        &launch.repo,
        AppRequest {
            revision: launch.revision,
            paths,
            view,
            compare_base: launch.compare_base,
            compare_head: launch.compare_head,
            compare_mode: mode,
        },
        &loaded,
    )?;
    let no_alt = launch.no_alt_screen || !loaded.config.ui.alternate_screen;
    tui::run_configured(
        app,
        client,
        no_alt,
        loaded.bindings,
        loaded.config.ui.mouse,
        loaded.config.ui.clipboard == "osc52",
    )?;
    Ok(())
}

fn run_select(
    cli: &Cli,
    loaded: &LoadedConfig,
    client: GitClient,
    args: phig_cli::cli::SelectArgs,
) -> Result<Outcome, MainError> {
    let paths = protocol::git_paths(&args.paths);
    let view = match args.kind {
        SelectionKind::Commit => View::Log,
        SelectionKind::Ref => View::Refs,
        SelectionKind::File | SelectionKind::Hunk => View::Detail,
        SelectionKind::Line => {
            if paths.len() != 1 {
                return Err(MainError::Unsupported(
                    "line selection requires exactly one path after --".into(),
                ));
            }
            View::Blame
        }
        SelectionKind::Compare => View::Compare,
    };
    let mut app = configured_app(
        &client,
        &cli.repo,
        AppRequest {
            revision: args.revision.clone(),
            paths,
            view,
            compare_base: args.base,
            compare_head: args.revision,
            compare_mode: loaded.config.compare.mode(),
        },
        loaded,
    )?;
    let target = match args.kind {
        SelectionKind::Commit => SelectionTarget::Commit,
        SelectionKind::Ref => SelectionTarget::Ref,
        SelectionKind::File => SelectionTarget::File,
        SelectionKind::Hunk => SelectionTarget::Hunk,
        SelectionKind::Line => SelectionTarget::Line,
        SelectionKind::Compare => SelectionTarget::Compare,
    };
    let (accept_key, cancel_keys) = loaded.bindings.selection_key_labels();
    app.selection_contract = Some(SelectionContract::new(target, accept_key, cancel_keys));
    let no_alt = cli.no_alt_screen || !loaded.config.ui.alternate_screen;
    let Some(selection) = tui::run_select(
        app,
        client,
        no_alt,
        loaded.bindings.clone(),
        args.kind,
        loaded.config.ui.mouse,
        loaded.config.ui.clipboard == "osc52",
    )?
    else {
        return Ok(Outcome::Cancelled);
    };
    match args.format {
        SelectionFormat::Json => write_json(&Envelope::new("selection", selection))?,
        SelectionFormat::Oid => {
            let oid = selection
                .oid
                .ok_or_else(|| MainError::Unsupported("selected value has no object id".into()))?;
            write_text(&format!("{oid}\n"))?
        }
    }
    Ok(Outcome::Success)
}

fn write_json<T: serde::Serialize>(value: &T) -> Result<(), MainError> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    write_bytes(&bytes)
}

fn write_text(value: &str) -> Result<(), MainError> {
    write_bytes(value.as_bytes())
}

fn write_bytes(value: &[u8]) -> Result<(), MainError> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    out.write_all(value)?;
    out.flush()?;
    Ok(())
}

fn write_completions(shell: CompletionShell) -> Result<(), MainError> {
    let shell = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
        CompletionShell::Elvish => Shell::Elvish,
        CompletionShell::Powershell => Shell::PowerShell,
    };
    let mut command = Cli::command();
    let mut output = Vec::new();
    generate(shell, &mut command, "phig", &mut output);
    write_bytes(&output)
}

fn render_manpage(command: clap::Command, title: &str) -> Result<Vec<u8>, MainError> {
    let mut output = Vec::new();
    clap_mangen::Man::new(command)
        .title(title)
        .section("1")
        .date("2026-08-19")
        .source(format!("phig {}", env!("CARGO_PKG_VERSION")))
        .manual("phig Manual")
        .render(&mut output)?;
    let text = String::from_utf8_lossy(&output);
    let lines = text.lines().map(str::trim_end).collect::<Vec<_>>();
    let mut normalized = lines
        .iter()
        .copied()
        .filter(|line| *line != ".br")
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    normalized.push(b'\n');
    Ok(normalized)
}

fn write_manpages(output_dir: Option<&Path>) -> Result<(), MainError> {
    let command = Cli::command();
    if let Some(directory) = output_dir {
        fs::create_dir_all(directory)?;
        let mut pages = Vec::new();
        collect_manpages(&command, "phig", &mut pages);
        for (name, command) in pages {
            let title = name.to_ascii_uppercase();
            fs::write(
                directory.join(format!("{name}.1")),
                render_manpage(command, &title)?,
            )?;
        }
        Ok(())
    } else {
        write_bytes(&render_manpage(command, "PHIG")?)
    }
}

fn collect_manpages(command: &clap::Command, name: &str, pages: &mut Vec<(String, clap::Command)>) {
    pages.push((name.to_owned(), command.clone()));
    for subcommand in command.get_subcommands() {
        let sub_name = format!("{name}-{}", subcommand.get_name());
        collect_manpages(subcommand, &sub_name, pages);
    }
}

fn apply_theme(loaded: &LoadedConfig) {
    tui::set_color_mode(&loaded.config.ui.color);
    tui::set_date_mode(&loaded.config.ui.date);
    let t = &loaded.config.theme;
    tui::set_theme(RenderTheme {
        accent: config::parse_color(&t.accent).unwrap(),
        muted: config::parse_color(&t.muted).unwrap(),
        added: config::parse_color(&t.added).unwrap(),
        removed: config::parse_color(&t.removed).unwrap(),
        warning: config::parse_color(&t.warning).unwrap(),
        error: config::parse_color(&t.error).unwrap(),
        selection_fg: config::parse_color(&t.selection_fg).unwrap(),
        selection_bg: config::parse_color(&t.selection_bg).unwrap(),
    })
}

fn exit_code(error: &MainError) -> u8 {
    match error {
        MainError::Config(_) | MainError::Snapshot(SnapshotError::InvalidOffset { .. }) => 2,
        MainError::Git(GitError::NotRepository(_))
        | MainError::Snapshot(SnapshotError::Git(GitError::NotRepository(_))) => 3,
        MainError::Git(GitError::UnsupportedGit { .. } | GitError::UnsupportedPlatform { .. })
        | MainError::Snapshot(SnapshotError::Git(
            GitError::UnsupportedGit { .. } | GitError::UnsupportedPlatform { .. },
        ))
        | MainError::Unsupported(_) => 4,
        MainError::Git(_) | MainError::Snapshot(SnapshotError::Git(_)) => 5,
        MainError::Tui(TuiError::NoControllingTerminal(_)) => 4,
        MainError::Tui(TuiError::Terminated(signal)) => {
            u8::try_from(128_i32.saturating_add(*signal)).unwrap_or(255)
        }
        MainError::Tui(TuiError::Terminal(error)) if error.kind() == io::ErrorKind::NotFound => 4,
        MainError::Update(_) => 6,
        MainError::Tui(_) | MainError::Output(_) | MainError::Json(_) => 70,
    }
}

#[cfg(unix)]
fn git_path_from_os(value: OsString) -> GitPath {
    use std::os::unix::ffi::OsStringExt;
    GitPath::new(value.into_vec())
}
#[cfg(not(unix))]
fn git_path_from_os(value: OsString) -> GitPath {
    GitPath::new(value.to_string_lossy().as_bytes().to_vec())
}
