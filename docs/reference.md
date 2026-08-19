# Command reference

## Interactive commands

```text
phig                                # equivalent to phig log HEAD
phig log [REV] [-- PATH…]
phig show REV [-- PATH…]
phig compare [BASE] [HEAD] [-- PATH…]   # merge-base semantics
phig diff LEFT RIGHT [-- PATH…]         # exact endpoints
phig refs | status | stash
phig tree [REV] [-- PATH]
phig blame [REV] -- PATH
```

Global options are `--repo PATH`, `--config PATH`, `--no-config`, and
`--no-alt-screen`. Bare `phig` is `phig log HEAD`. Paths are always literal and
must follow `--` so revisions and paths cannot be confused.

## Machine commands

`phig snapshot` takes one explicit target using the same target syntax:

```sh
phig snapshot log HEAD
phig snapshot show HEAD -- src/lib.rs
phig snapshot compare main HEAD
phig snapshot diff v1.0.0 HEAD
phig snapshot refs
phig snapshot status
phig snapshot tree HEAD -- src
phig snapshot blame HEAD -- src/lib.rs
phig snapshot stash
phig snapshot --format json --offset 256 log HEAD
phig snapshot refs --format json --offset 50
```

It writes exactly one compact UTF-8 JSON object plus LF. The envelope is
`{"protocol":"phig/1","kind":"snapshot","payload":…}`. Payloads identify
the repository and object format, requested target, typed domain data,
the starting `offset`, truncation, and a directly consumable continuation
offset when another page exists. Item and byte bounds come from configuration.
Arrays have deterministic Git/path ordering. `--format` and `--offset` are
global within `snapshot`, so they work before or after its target. Nonzero
offsets are rejected for singleton `show`, `compare`, and `diff` snapshots.

```sh
phig select --kind commit --format oid
phig select --kind ref --format json
phig select --kind file --format json HEAD
phig select --kind hunk --format json HEAD -- src/lib.rs
phig select --kind line --format json HEAD -- src/lib.rs
phig select --kind compare --base main --format json
```

Selection opens the TUI on `/dev/tty`, leaving stdout reserved for the accepted
result. Enter accepts an available semantic target. `q` or Escape cancels with
exit 1 and empty stdout. JSON locators include repository generation, full
algorithm-tagged OIDs, optional parent, authoritative base64 path bytes plus a
safe display form, and advisory hunk/line coordinates. `oid` prints one full OID
plus LF.

```sh
selection="$(phig select --kind hunk --format json)"
phig snapshot status | jq '.payload.data.entries[] | .path'
phig --no-alt-screen refs                 # phux/tmux-friendly scrollback
phig completions bash > ~/.local/share/bash-completion/completions/phig
phig manpage > phig.1
phig manpage --output-dir ./man  # root plus every nested subcommand page
phig version --json
phig update --check          # explicit network check, no installation
phig update                  # Homebrew or release-installer update
```

Machine modes do not infer themselves from redirection. They never prompt,
emit terminal escapes, invoke a pager/editor/repository helper, contact a
remote, or write diagnostics to stdout. Consumers must ignore unknown JSON
fields. See [`schema/phig-1.schema.json`](schema/phig-1.schema.json).

## Exit status

| Code | Meaning |
| ---: | --- |
| 0 | success |
| 1 | clean cancellation/no selection |
| 2 | invalid CLI or configuration |
| 3 | repository unavailable |
| 4 | unsupported Git/platform/context or no controlling terminal |
| 5 | Git operation failed |
| 6 | update unavailable or failed |
| 70 | internal or terminal invariant failure |
| 128+N | terminated by signal, when representable |

A failure before a machine result leaves stdout empty. Human diagnostics go to
stderr.
