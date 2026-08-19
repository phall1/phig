use std::{ffi::OsString, process::ExitCode};

use clap::Parser;
use phig_cli::{
    app::{App, View},
    cli::{Cli, StartView},
    domain::{ComparisonMode, GitPath},
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
    let compare_mode = if launch.exact_compare {
        ComparisonMode::Exact
    } else {
        ComparisonMode::MergeBase
    };
    let (compare_base, compare_base_label) =
        if view == View::Compare && launch.compare_base.is_none() {
            let (revision, label) =
                client.infer_compare_base(&repository, &phig_cli::git::CancellationToken::new())?;
            (Some(revision), Some(label))
        } else {
            (launch.compare_base.clone(), launch.compare_base.clone())
        };
    let compare_head_label = (view == View::Compare).then(|| launch.compare_head.clone());
    let mut app = App::new(repository, launch.revision, paths, view == View::Detail);
    if view == View::Tree {
        app.inspect.tree_path = app.paths.first().cloned();
    } else if view == View::Blame {
        app.inspect.blame_path = app.paths.first().cloned();
    }
    app.set_start_view(view, compare_base, launch.compare_head, compare_mode);
    app.inspect.compare_base_label = compare_base_label;
    app.inspect.compare_head_label = compare_head_label;
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
