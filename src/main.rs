use std::{ffi::OsString, process::ExitCode};

use clap::Parser;
use phig_cli::{
    app::App,
    cli::{Cli, StartView},
    domain::GitPath,
    git::{GitClient, GitError},
    tui::{self, TuiError},
};
use thiserror::Error;

#[derive(Debug, Error)]
enum MainError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Tui(#[from] TuiError),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("phig: {error}");
            ExitCode::from(exit_code(&error))
        }
    }
}

fn run() -> Result<(), MainError> {
    let launch = Cli::parse().launch();
    let client = GitClient::default();
    let repository = client.discover(&launch.repo)?;
    let paths = launch
        .paths
        .into_iter()
        .map(git_path_from_os)
        .collect::<Vec<_>>();
    let app = App::new(
        repository,
        launch.revision,
        paths,
        launch.start_view == StartView::Show,
    );
    tui::run(app, client, launch.no_alt_screen)?;
    Ok(())
}

fn exit_code(error: &MainError) -> u8 {
    match error {
        MainError::Git(GitError::NotRepository(_)) => 3,
        MainError::Git(GitError::UnsupportedGit { .. } | GitError::UnsupportedPlatform { .. }) => 4,
        MainError::Git(_) => 5,
        MainError::Tui(TuiError::Terminated(signal)) => {
            u8::try_from(128_i32.saturating_add(*signal)).unwrap_or(255)
        }
        MainError::Tui(_) => 70,
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
