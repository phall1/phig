# Composability contract

Phig composes as a conventional local process. It does not require phux, tmux,
an agent runtime, a daemon, or a network service.

## Modes

```text
phig [log] [REV] [-- PATH…]
phig show REV [-- PATH…]
phig compare [BASE] [HEAD] [-- PATH…]
phig diff LEFT RIGHT [-- PATH…]
phig refs
phig status
phig tree [REV] [-- PATH]
phig blame [REV] -- PATH
phig stash
phig select [TARGETING OPTIONS] --kind commit|ref|file|hunk|line|compare --format oid|json
phig snapshot [--format json] [--offset N] TARGET [TARGETING OPTIONS]
phig completions SHELL
phig manpage
phig version --json
```

Bare `phig` is identical to `phig log`. UI and machine modes are explicit; phig
never changes mode merely because stdout is redirected.

## Streams

| Mode | stdin | stdout | stderr |
| --- | --- | --- | --- |
| interactive UI | terminal input | terminal rendering | fatal diagnostics before/after UI |
| `select` | controlling terminal | exactly one result | diagnostics |
| `snapshot` | unused | exactly one JSON object + LF | diagnostics |
| `version --json` | unused | exactly one JSON object + LF | diagnostics |
| completions/manpage | unused | generated document | diagnostics |

Machine modes never prompt, invoke a pager/editor, emit ANSI, perform network
access, or execute repository-controlled helpers. A failure before output leaves
stdout empty. Exit codes are the stable automation contract on failure;
diagnostics on stderr are human-readable unless a command explicitly adds an
`--error-format json` option.

## JSON envelope

```json
{
  "protocol": "phig/1",
  "kind": "selection",
  "payload": {}
}
```

Consumers must ignore unknown fields. Breaking changes use a new protocol
identifier. The checked-in schema is `docs/schema/phig-1.schema.json`.
Serialization is compact UTF-8 with stable field names. Raw paths
that are not UTF-8 include an explicit encoded representation.

Selection payloads contain the repository root, repository generation when
known, object identifiers with hash algorithm, raw/display paths as applicable,
optional parent, and optional advisory hunk/line coordinates.

Snapshots are bounded descriptions of the requested initial view, not a generic
Git RPC API. Truncation and continuation information are explicit.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | success |
| `1` | clean cancellation/no selection |
| `2` | invalid command line, input, or configuration |
| `3` | repository unavailable |
| `4` | unsupported capability or context |
| `5` | Git operation failed |
| `70` | internal invariant failure |
| `128+N` | terminated by signal where representable |

## Phux and multiplexers

`--no-alt-screen` keeps output in normal scrollback. Phig responds conventionally
to resize, interrupt, terminate, suspend, and continue signals. It neither reads
nor requires `PHUX_*` variables. A phux pane can therefore launch, move, signal,
or close phig like any foreground terminal process.

## Stability

The CLI, exit table, and `phig/1` envelope become stable at 1.0. During 0.x,
breaking protocol changes are called out in the changelog and retain a migration
note. Interactive layout is intentionally free to improve while semantic actions
and documented default keys remain compatible.
