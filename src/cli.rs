use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand};

/// A fast, focused terminal Git history and diff browser.
#[derive(Debug, Clone, Parser)]
#[command(name = "phig", version, about, propagate_version = true)]
pub struct Cli {
    /// Repository or path inside a repository.
    #[arg(long, global = true, value_name = "PATH", default_value = ".")]
    pub repo: PathBuf,

    /// Render on the normal screen so terminal scrollback remains available.
    #[arg(long, global = true)]
    pub no_alt_screen: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Browse commit history (the default command).
    Log(RevisionArgs),
    /// Open one commit and its diff.
    Show(RevisionArgs),
}

#[derive(Debug, Clone, Args)]
pub struct RevisionArgs {
    /// Revision or revision expression to inspect.
    #[arg(value_name = "REV", default_value = "HEAD")]
    pub revision: String,

    /// Literal paths to constrain history or diff output.
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartView {
    Log,
    Show,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub repo: PathBuf,
    pub no_alt_screen: bool,
    pub start_view: StartView,
    pub revision: String,
    pub paths: Vec<OsString>,
}

impl Cli {
    pub fn launch(self) -> Launch {
        let (start_view, revision, paths) = match self.command {
            None => (StartView::Log, "HEAD".to_owned(), Vec::new()),
            Some(Command::Log(args)) => (StartView::Log, args.revision, args.paths),
            Some(Command::Show(args)) => (StartView::Show, args.revision, args.paths),
        };
        Launch {
            repo: self.repo,
            no_alt_screen: self.no_alt_screen,
            start_view,
            revision,
            paths,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn bare_invocation_is_head_log() {
        let launch = Cli::try_parse_from(["phig"]).unwrap().launch();
        assert_eq!(launch.start_view, StartView::Log);
        assert_eq!(launch.revision, "HEAD");
        assert!(launch.paths.is_empty());
    }

    #[test]
    fn parses_show_paths_after_separator() {
        let launch = Cli::try_parse_from(["phig", "show", "HEAD~2", "--", "src/lib.rs"])
            .unwrap()
            .launch();
        assert_eq!(launch.start_view, StartView::Show);
        assert_eq!(launch.revision, "HEAD~2");
        assert_eq!(launch.paths, [OsString::from("src/lib.rs")]);
    }
}
