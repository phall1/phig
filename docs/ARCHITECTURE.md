# Architecture

Phig uses a functional application core surrounded by terminal, Git, config,
and machine-output adapters.

```text
CLI ─┬─ TUI (terminal input + rendering)
     ├─ snapshot/selection output
     └─ generated shell/man artifacts
             │
             ▼
      App state + reducer
      commands and effects
             │
             ▼
       Repository service
             │
             ▼
   bounded Git process adapter
```

## Technology

- Rust 2024 edition
- Ratatui with Crossterm
- installed Git CLI invoked with explicit argument vectors
- Clap for the public command line
- Serde for TOML configuration and versioned JSON
- bounded standard threads/channels for background work; no async runtime in the
  initial architecture

The Cargo package is `phig-cli` because `phig` is occupied on crates.io. The
installed executable and product are `phig`.

## Source seams

- `cli`: syntax, validation, mode dispatch, completions, and exit mapping
- `config`: XDG discovery, TOML decoding, defaults, themes, and key overrides
- `domain`: byte-preserving repository identities and normalized records
- `git`: repository discovery, command construction, parsers, bounds, errors,
  capability detection, and cancellation
- `app`: view-independent state, actions, reducer, selection, search, and effect
  generation
- `tui`: terminal lifecycle, event translation, layout, widgets, styles, help,
  and controlling-terminal selection rendering
- `protocol`: stable JSON envelopes and byte-clean writers

TUI row numbers, split geometry, and cursor coordinates must not leak into the
repository domain or protocol.

## Git process contract

Git 2.45.1 or newer is resolved from `PATH` unless explicitly overridden. This minimum is required because `GIT_NO_LAZY_FETCH` only became enforceable in 2.45.1. Every command:

- uses `std::process::Command`, never a shell
- sets `--no-pager`, `GIT_TERMINAL_PROMPT=0`, and a deterministic locale where
  machine formats permit it
- passes untrusted revisions and paths after validation and option terminators
- prefers documented porcelain/plumbing output with NUL delimiters
- captures stdout and stderr concurrently where blocking is possible
- has output, time, and queue bounds
- is associated with a request generation so stale results are discarded
- reports truncation and unsupported capabilities explicitly

All modes disable color, pagers, prompts, hooks, fsmonitor, textconv, external
diff, signing display, lazy fetches, and other repository/user-configured
helpers unless a future explicit trusted action opts in. Phig never silently
bypasses Git's `safe.directory` checks. Display preferences such as algorithm,
context, and whitespace are expressed as allowlisted command arguments rather
than inherited executable configuration.

## Concurrency

The terminal thread owns `App`, input, and drawing. It never waits for Git.
Effects are sent to a small bounded worker service. Results include an operation
kind and monotonically increasing generation. Moving selection increments the
preview generation; older results may complete but cannot replace current state.

A supervisor owns each child and concurrently drains bounded stdout/stderr. On
Unix each operation uses a separate process group. Completion, cancellation, or
timeout force-kills any remaining group members, then transfers the child to a
reaper that retains ownership through `wait`. Pipe drains and observed reaping
share one one-second cleanup deadline; exceeding it returns a typed timeout while
the reaper still guarantees eventual collection. Queues coalesce previews and
repository refreshes. A busy repository therefore produces bounded stale work
rather than unbounded threads or output. Native Windows is gated until an
equivalent job-object contract is implemented and tested.

## Rendering and terminal ownership

Repository-derived text is treated as untrusted. Git color is always disabled;
phig parses structural patch lines and applies Ratatui styles itself. C0/C1,
DEL, terminal escapes, malformed sequences, and bidirectional formatting
controls are escaped before rendering. Machine output never contains ANSI.

A terminal guard owns raw mode, alternate-screen state, mouse/focus modes, cursor
visibility, and panic restoration. External interactive commands temporarily
restore the real terminal, inherit the TTY, then force a complete redraw.
`--no-alt-screen` renders in place and remains compatible with phux/tmux scrollback.

Selection mode reserves stdout for the result. Its UI opens `/dev/tty` on Unix;
if no controlling terminal exists, it exits with a clear unsupported-context
error rather than mixing escape sequences with output.

## Data identity

Object IDs contain an algorithm plus hexadecimal bytes; no code assumes SHA-1
or forty characters. Repository paths are retained as platform bytes. Native
Windows is rejected at discovery in version 1 rather than converting path bytes
lossily; WSL is supported. Human
JSON fields use escaped UTF-8 when possible and an explicit byte encoding when
not. Display strings are never authoritative identity.

Commit/file/hunk locators distinguish durable fields (repository identity,
object IDs, raw path) from advisory fields (display line and hunk ranges).

## Configuration

Order, from weakest to strongest:

1. built-in defaults
2. `$XDG_CONFIG_HOME/phig/config.toml` or platform equivalent
3. `PHIG_CONFIG`
4. explicit `--config`
5. CLI flags

Unknown top-level configuration keys are errors with source locations and
suggestions. A `version = 1` field controls future migrations. Keybindings map
semantic actions rather than implementation callbacks.

## Testing

- parser and reducer unit tests
- byte fixtures for NUL records, invalid UTF-8 paths, control characters,
  renames, merge parents, SHA-256 IDs, submodules, binary patches, and truncation
- temporary-repository integration tests using the supported Git executable
- Ratatui buffer snapshots at narrow, normal, and wide terminal sizes
- PTY tests for launch, keys, resize, suspend/resume, errors, and cleanup
- exact stdout/exit tests for every machine mode
- installer/update tests against local release fixtures
- benchmark harness for launch, history, diff switching, and large repositories

## Distribution

GitHub releases are the artifact authority. Release automation builds target
archives, checksums, GitHub build-provenance attestations, an installer, and a
Homebrew formula update for `phall1/homebrew-tap`. The curl installer downloads
a versioned archive over authenticated TLS, verifies its published checksum for
transport integrity, installs atomically under a user-selected prefix, and is
safe to rerun for updates. Release documentation explains independent
`gh attestation verify` provenance verification; a checksum served by the same
release authority is not described as a signature.

Normal repository use performs no network access. `phig update --check` is the
only check path that contacts GitHub; `phig update` is the only path that may
execute Homebrew or a downloaded installer. Both require an explicit invocation.
Release updates download the exact version-tagged cargo-dist installer into a
private staging directory beside the destination, request its flat unmanaged
layout, verify and sync the candidate, then atomically replace the current
executable with rollback protection. Homebrew ownership requires canonical
formula-prefix identity. Failures never trigger an unverified or background
self-replacement.
