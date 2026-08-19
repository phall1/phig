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
    /// Compare a branch to its merge base with HEAD.
    Compare(CompareArgs),
    /// Compare two exact revision endpoints.
    Diff(DiffArgs),
    /// Browse local, remote, and tag refs.
    Refs,
    /// Inspect working-tree and index status without mutation.
    Status,
    /// Browse a revision's tree.
    Tree(TreeArgs),
    /// Inspect line attribution for a path.
    Blame(BlameArgs),
    /// Inspect stash entries and patches.
    Stash,
}

#[derive(Debug, Clone, Args)]
pub struct RevisionArgs {
    #[arg(value_name = "REV", default_value = "HEAD")]
    pub revision: String,
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<OsString>,
}

#[derive(Debug, Clone, Args)]
pub struct CompareArgs {
    /// Base ref; inferred from upstream/main/master when omitted.
    #[arg(value_name = "BASE")]
    pub base: Option<String>,
    #[arg(value_name = "HEAD", default_value = "HEAD")]
    pub head: String,
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<OsString>,
}

#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    pub left: String,
    pub right: String,
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<OsString>,
}

#[derive(Debug, Clone, Args)]
pub struct TreeArgs {
    #[arg(value_name = "REV", default_value = "HEAD")]
    pub revision: String,
    #[arg(last = true, value_name = "PATH", num_args = 0..=1)]
    pub path: Vec<OsString>,
}

#[derive(Debug, Clone, Args)]
pub struct BlameArgs {
    #[arg(value_name = "REV", default_value = "HEAD")]
    pub revision: String,
    /// Path is required and separated from the revision by `--`.
    #[arg(last = true, value_name = "PATH", num_args = 1, required = true)]
    pub path: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartView {
    Log,
    Show,
    Compare,
    Refs,
    Status,
    Tree,
    Blame,
    Stash,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub repo: PathBuf,
    pub no_alt_screen: bool,
    pub start_view: StartView,
    pub revision: String,
    pub paths: Vec<OsString>,
    pub compare_base: Option<String>,
    pub compare_head: String,
    pub exact_compare: bool,
}

impl Cli {
    pub fn launch(self) -> Launch {
        let defaults = |start_view, revision, paths| Launch {
            repo: self.repo.clone(),
            no_alt_screen: self.no_alt_screen,
            start_view,
            revision,
            paths,
            compare_base: None,
            compare_head: "HEAD".into(),
            exact_compare: false,
        };
        match self.command {
            None => defaults(StartView::Log, "HEAD".into(), Vec::new()),
            Some(Command::Log(args)) => defaults(StartView::Log, args.revision, args.paths),
            Some(Command::Show(args)) => defaults(StartView::Show, args.revision, args.paths),
            Some(Command::Compare(args)) => {
                let mut launch = defaults(StartView::Compare, args.head.clone(), args.paths);
                launch.compare_base = args.base;
                launch.compare_head = args.head;
                launch
            }
            Some(Command::Diff(args)) => {
                let mut launch = defaults(StartView::Compare, args.right.clone(), args.paths);
                launch.compare_base = Some(args.left);
                launch.compare_head = args.right;
                launch.exact_compare = true;
                launch
            }
            Some(Command::Refs) => defaults(StartView::Refs, "HEAD".into(), Vec::new()),
            Some(Command::Status) => defaults(StartView::Status, "HEAD".into(), Vec::new()),
            Some(Command::Tree(args)) => defaults(StartView::Tree, args.revision, args.path),
            Some(Command::Blame(args)) => defaults(StartView::Blame, args.revision, args.path),
            Some(Command::Stash) => defaults(StartView::Stash, "HEAD".into(), Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_complete_view_surface() {
        assert_eq!(
            Cli::try_parse_from(["phig"]).unwrap().launch().start_view,
            StartView::Log
        );
        let show = Cli::try_parse_from(["phig", "show", "HEAD~2", "--", "src/lib.rs"])
            .unwrap()
            .launch();
        assert_eq!(show.start_view, StartView::Show);
        assert_eq!(show.paths, [OsString::from("src/lib.rs")]);
        let compare = Cli::try_parse_from(["phig", "compare", "main", "feature"])
            .unwrap()
            .launch();
        assert_eq!(compare.compare_base.as_deref(), Some("main"));
        assert!(!compare.exact_compare);
        let diff = Cli::try_parse_from(["phig", "diff", "a", "b"])
            .unwrap()
            .launch();
        assert!(diff.exact_compare);
        assert_eq!(diff.compare_head, "b");
        assert_eq!(
            Cli::try_parse_from(["phig", "refs"])
                .unwrap()
                .launch()
                .start_view,
            StartView::Refs
        );
        assert!(Cli::try_parse_from(["phig", "blame", "HEAD", "--", "file"]).is_ok());
        assert!(Cli::try_parse_from(["phig", "blame", "HEAD"]).is_err());
    }
}
