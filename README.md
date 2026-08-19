<div align="center">

# phig

**Git history at the speed of thought.**

A focused, keyboard-first terminal browser for commits, diffs, branches, and
human-to-agent handoff. The parts of tig that make it indispensable, rebuilt for
modern terminals without turning into a Git dashboard.

[Install](#install) · [Quick start](#quick-start) · [Why phig](#why-phig) ·
[Configuration](docs/configuration.md) · [Reference](docs/reference.md)

</div>

> **Development status:** phig is under active initial development. The CLI and
> protocol may change before 1.0.

## Why phig

- **Immediate:** open a repository and move through history without ceremony.
- **Diff-first:** preview commits, files, and hunks without losing your place.
- **Comparison that explains itself:** exact endpoints and merge-base branch
  comparisons are visibly distinct.
- **Focused:** no permanent panel farm, hosted-service client, or mutation UI.
- **Composable:** select context for a shell command, agent, or phux pane using
  stable JSON and ordinary process behavior.
- **Trustworthy:** Git remains the authority for revisions and patches.

## Install

Homebrew and release installers will become available with the first tagged
release. From source today:

```sh
cargo install --path . --locked
```

The installed executable is `phig`; Git 2.35 or newer is required.

## Quick start

```sh
phig                         # browse the current branch
phig show HEAD~3             # inspect one commit
phig compare main            # compare merge-base(main, HEAD) to HEAD
phig diff v1.0.0 HEAD -- src # exact endpoint comparison, path-filtered
phig refs                    # browse branches and tags
phig status                  # inspect working-tree changes
```

The default navigation is intentionally familiar: `j/k`, arrows, `Enter`,
`q`, `/`, `n/N`, `g/G`, `Tab`, `[`/`]`, and `?`.

## Human-to-agent handoff

```sh
selection="$(phig select --kind hunk --format json)"
printf '%s\n' "$selection" | jq .
```

The selection UI uses the controlling terminal while stdout contains only the
result. Normal browsing never contacts the network and does not require an
agent runtime.

## Design

Phig is an interactive Git lens, not a Git IDE. Version 1 is deliberately
read-only. Read the [product contract](docs/PRODUCT.md),
[architecture](docs/ARCHITECTURE.md), [interaction specification](docs/UX.md),
and [composability contract](docs/COMPOSABILITY.md).

## Contributing

The project requires stable Rust and Git. Run `just check` before submitting a
change. Security issues should use GitHub private vulnerability reporting rather
than a public issue.

## License

Licensed under either of Apache License 2.0 or the MIT license, at your option.
