use std::{ffi::OsString, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

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

    /// Load configuration from this exact path.
    #[arg(long, global = true, value_name = "PATH", env = "PHIG_CONFIG")]
    pub config: Option<PathBuf>,

    /// Ignore all configuration files and use built-in defaults.
    #[arg(long, global = true)]
    pub no_config: bool,

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
    /// Print or create the XDG configuration.
    Config(ConfigArgs),
    /// Emit a bounded, deterministic phig/1 JSON snapshot.
    Snapshot(SnapshotArgs),
    /// Interactively select one exact object using the controlling terminal.
    Select(SelectArgs),
    /// Generate shell completions.
    Completions { shell: CompletionShell },
    /// Generate the phig manual page.
    Manpage(ManpageArgs),
    /// Check for or explicitly install the latest release.
    Update(UpdateArgs),
    /// Print version information.
    Version(VersionArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}
#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration path.
    Path,
    /// Write the documented default configuration.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Parse and validate the effective configuration.
    Check,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    Powershell,
}

#[derive(Debug, Clone, Args)]
pub struct ManpageArgs {
    /// Write the root and every subcommand manpage to this directory.
    #[arg(long, value_name = "DIR")]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    /// Check for a newer stable release without installing it.
    #[arg(long)]
    pub check: bool,
}

#[derive(Debug, Clone, Args)]
pub struct VersionArgs {
    /// Emit a phig/1 JSON object.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct SnapshotArgs {
    /// Output format. `json` is currently the stable format.
    #[arg(long, global = true, value_enum, default_value = "json")]
    pub format: MachineFormat,
    /// Start at this deterministic item offset. Singleton targets require zero.
    #[arg(long, global = true, default_value_t = 0)]
    pub offset: usize,
    #[command(subcommand)]
    pub target: SnapshotTarget,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SnapshotTarget {
    Log(RevisionArgs),
    Show(RevisionArgs),
    Compare(CompareArgs),
    Diff(DiffArgs),
    Refs,
    Status,
    Tree(TreeArgs),
    Blame(BlameArgs),
    Stash,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum MachineFormat {
    Json,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SelectionFormat {
    Oid,
    Json,
}
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum SelectionKind {
    Commit,
    Ref,
    File,
    Hunk,
    Line,
    Compare,
}

#[derive(Debug, Clone, Args)]
pub struct SelectArgs {
    #[arg(long, value_enum)]
    pub kind: SelectionKind,
    #[arg(long, value_enum, default_value = "json")]
    pub format: SelectionFormat,
    #[arg(value_name = "REV", default_value = "HEAD")]
    pub revision: String,
    #[arg(long)]
    pub base: Option<String>,
    #[arg(last = true, value_name = "PATH")]
    pub paths: Vec<OsString>,
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
    pub config: Option<PathBuf>,
    pub no_config: bool,
    pub start_view: StartView,
    pub revision: String,
    pub paths: Vec<OsString>,
    pub compare_base: Option<String>,
    pub compare_head: String,
    pub explicit_compare: bool,
    pub exact_compare: bool,
}

impl Cli {
    pub fn launch(self) -> Launch {
        let defaults = |start_view, revision, paths| Launch {
            repo: self.repo.clone(),
            no_alt_screen: self.no_alt_screen,
            config: self.config.clone(),
            no_config: self.no_config,
            start_view,
            revision,
            paths,
            compare_base: None,
            compare_head: "HEAD".into(),
            explicit_compare: false,
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
                launch.explicit_compare = true;
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
            Some(
                Command::Config(_)
                | Command::Snapshot(_)
                | Command::Select(_)
                | Command::Completions { .. }
                | Command::Manpage(_)
                | Command::Update(_)
                | Command::Version(_),
            ) => unreachable!("non-interactive commands do not create a launch"),
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
        assert!(compare.explicit_compare);
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
