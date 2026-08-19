# Phig product contract

Phig is an interactive Git lens: instant history, trustworthy diffs, effortless
revision comparison, and portable human-selected context.

It is a spiritual successor to tig, not a dashboard Git client. The dominant
surface is always the thing being inspected. Secondary context may appear in a
preview, but branches, files, remotes, status, and commands never compete in a
permanent grid.

## Product principles

1. **Speed of thought.** Launching, moving, searching, opening a diff, and
   comparing refs must feel immediate.
2. **Git is authoritative.** Phig presents output from the installed Git rather
   than inventing subtly different revision or diff semantics.
3. **Keyboard first, zero-config first.** Familiar navigation works without a
   tutorial. Configuration refines good defaults rather than repairing them.
4. **One dominant surface.** Phig is a pager with memory, not a terminal IDE.
5. **Agents are callers, not residents.** Stable arguments and machine-readable
   selections compose with shells, phux, and agents. Phig never embeds a model.
6. **Safe observation.** Version 1 does not mutate repositories. External
   commands are explicit and terminal ownership is restored reliably.
7. **Smol is conceptual.** One native executable, no daemon, no mandatory
   index, no network during normal use, bounded dependencies, and narrow modes.

## Version 1 workflows

### Browse history

Bare `phig` opens `phig log HEAD`; use `phig log [REV] [-- PATH…]` for another
revision. The initial
selection is visible as soon as data arrives. Movement updates an optional diff
preview without blocking input. Users can search commit metadata, constrain a
path, copy an object ID, or open commit detail.

### Inspect a commit

`phig show REV [-- PATH…]` opens commit metadata and Git's patch. File and hunk
navigation are first-class. Merge commits expose parent selection explicitly.
Binary files, renames, modes, submodules, and truncated output are identified
rather than silently misrendered.

### Compare revisions

`phig compare [BASE] [HEAD]` defaults HEAD to the current revision and defaults
BASE to the current branch's upstream or a conservative merge-base candidate.
The chosen endpoints and resolved merge base are always visible.

`phig diff LEFT RIGHT` compares exact endpoints. Users may mark and compare two
commits or refs from inside the UI, swap sides, filter files, and move by hunk.

### Inspect repository context

Focused read-only views cover refs, working-tree status, tree entries, blame,
and stash entries. These are alternate dominant surfaces, not permanent panels.
Every view can be entered from the CLI or the command palette.

### Search and hand off context

Incremental text search works in the active surface. `phig select` lets a human
choose a commit, ref, file, or hunk and emits a clean result. JSON output uses a
versioned envelope and never shares stdout with terminal rendering.

### Configure and integrate

Phig reads an XDG TOML config for theme, preview behavior, diff preferences, and
key overrides. `--config`, `--no-config`, `--no-alt-screen`, `NO_COLOR`, and
standard exit behavior make invocation deterministic.

## Non-goals for version 1

- staging, committing, checkout, reset, rebase, merge, push, fetch, or conflict
  resolution
- hosted-service PR, issue, or review APIs
- embedded agents, chat, summaries, credentials, prompts, or model providers
- plugins, an embedded scripting language, or tig configuration compatibility
- daemon, database, semantic index, background network access, or multi-repo UI
- exact visual or feature parity with tig

Read-only external editor/pager handoff and copying data are permitted. A later
mutation release requires a separate authority, recovery, and conflict design.

## Supported environments

- macOS 12 or newer on Apple Silicon and x86_64
- glibc-based Linux 2.31 or newer on x86_64 and aarch64, including WSL
- Git 2.45.1 or newer, required for enforceable no-lazy-fetch inspection; building from source requires Rust 1.88 or newer
- terminals supporting standard ANSI control sequences; true color is optional
- no Nerd Font or patched font requirement

Native Windows is explicitly unsupported in version 1; phig returns a typed
platform error rather than risking lossy repository paths. Windows users run
phig under WSL until terminal, process, and byte-path behavior has a dedicated
CI acceptance lane.

## Budgets and release gates

The committed release benchmark generates 1,000 commits across 100 paths and
records the source/fixture commits, dirty state, platform, tool versions, sample
counts, warm snapshot p50/p95, real-PTY launch-to-first-useful-history-frame
p50/p95, release binary size, and per-process peak RSS for each measured phig
PTY process. The achievable cross-runner release gates are:

- 20 warm snapshot samples with p95 at or below 500 ms
- 10 real-PTY first-useful-frame samples with p95 at or below 1,000 ms
- release binary at or below 15 MiB

CI runs the same gate with five samples per path to catch gross regressions;
the full release check uses 20/10. Memory is recorded rather than given a
cross-platform threshold because `wait4` peak-RSS units differ between Darwin
and Linux even though the harness normalizes both to MiB.

The following remain architectural optimization targets, not unverified release
blockers or current speed claims: 150 ms first useful frame, 16 ms cached
navigation-to-paint, sub-1% idle CPU, and under 100 MiB resident memory on a
100,000-commit/10,000-path stress fixture. That large fixture and those event
latencies are not yet measured by the standard harness and must not be presented
as achieved.

Independent hard bounds still apply: no more than 128 queued events, 32 MiB
metadata, 16 MiB preview patch, or 64 KiB captured stderr per operation unless
explicitly configured.

A release requires formatting, warnings-as-errors linting, unit/integration/UI
snapshot tests, repeated PTY smoke tests, installer tests, clean-checkout builds,
dependency policy checks, the measured release benchmark, a release dry run,
and fresh-context review.

## Success test

A user unfamiliar with the project can install phig in one command, enter a Git
repository, browse commits, understand and inspect a diff, compare their branch
to its base, discover the keymap, emit a selection for another command, and
recover their terminal after every exit path without reading source or writing
configuration, and explicitly check for or install a verified update.
