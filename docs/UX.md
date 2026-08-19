# Interaction specification

## Visual model

The width-budgeted header names the active view and repository first, reserves
space for critical selection/error state, and then adds revision, branch, path,
loading, and marked-endpoint context as room permits. The body is one dominant
list or document. At 110 columns and wider, log, refs, status, blame, and stash
previews occupy the right side behind one thin vertical divider. Normal widths
use a stacked list/preview with one horizontal divider; narrow terminals hide
previews and open details with `Enter`. The footer shows at most three effective,
contextual key hints plus a quiet right-aligned position.

Phig must remain usable at 60×16, comfortable at 100×28, and information-dense
without decorative borders at larger sizes. It uses the terminal's native
background, marker-led selection, restrained semantic color, and thin functional
dividers rather than boxed panes or cards. Overlays alone use a thin adaptive
frame with a short title and a persistent action footer.

## Default keys

| Keys | Action |
| --- | --- |
| `j`, `Down` | next item/line |
| `k`, `Up` | previous item/line |
| `Ctrl-d`, `PageDown` | page down |
| `Ctrl-u`, `PageUp` | page up |
| `g`, `Home` | first item/top |
| `G`, `End` | last loaded item/bottom |
| `Enter` | open selected item or toggle detail |
| `Esc` | close overlay/back to previous view |
| `q` | back, then quit at the root |
| `/` | search active view |
| `n`, `N` | next/previous search match |
| `Tab`, `Shift-Tab` | next/previous file or logical section |
| `]`, `[` | next/previous hunk |
| `}`, `{` | next/previous changed file |
| `P` | cycle merge parent in commit detail |
| `f` | filter changed files and jump to one |
| `r` | refs view |
| `s` | status view |
| `t` | tree view |
| `b` | blame selected path |
| `z` | stash view |
| `v` | mark comparison endpoint |
| `c` | begin/complete comparison |
| `x` | swap comparison sides |
| `M` | toggle exact/merge-base comparison |
| `d` | toggle staged/unstaged patch for a mixed status entry |
| `p` | toggle preview |
| `y` | copy selected stable identifier |
| `:` | command palette |
| `?` | contextual help |
| `Ctrl-l` | redraw |

Printable keys in search or command overlays edit their query. `:` opens a
searchable palette that lists the semantic actions implemented in the current
build, providing a universal discovery fallback even when a shortcut is unknown.
When a request fails, an actionable wrapped error panel identifies the failed
operation; `r` retries failed requests and `Esc` dismisses the panel. Key
overrides are resolved to semantic actions and conflict diagnostics name both
actions.

## Views

### Log

Rows contain graph glyphs, short object ID, relative/absolute date according to
available width, author, decorations, and subject. Graph and text degrade
cleanly on narrow terminals. Preview shows selected commit metadata and patch.
Additional history loads before the cursor reaches the end.

### Commit/diff

Metadata precedes file summary and patch. `f` opens a searchable changed-file
index; `Enter` jumps directly to the selected file header. Hunk headers are
anchors. Merge commits expose explicit parent cycling with `P`; version 1 does
not claim a combined-diff display.

### Compare

The header always states either `LEFT → RIGHT` for exact endpoint comparison or
`merge-base(BASE, HEAD) → HEAD` for branch comparison. The UI shows ahead/behind
counts, changed files, and patch. Users can swap endpoints and choose refs
without checkout.

### Refs

Branches, remotes, and tags are searchable. Each row includes checked-out state,
upstream, ahead/behind when inexpensive, target ID, and subject. Opening a ref
changes the viewed history; it never checks anything out.

### Status

Porcelain-v2 records use compact `XY` codes and are grouped into conflicted,
staged, mixed staged+unstaged, unstaged, and untracked entries. `d` switches
between the two patches for mixed entries. Opening an entry displays the
relevant read-only diff as the dominant surface, including on narrow terminals.
No key mutates the index or worktree.

### Tree

Lists the selected revision's tree with type, mode, size when known, and name.
Directories descend; blobs open content or a safe binary summary. `Backspace`
ascends and the header retains the current tree breadcrumb.

### Blame

Shows commit, author, date, and source line with grouping. Opening a blame group
jumps to the commit while retaining path context.

### Stash

Lists stash reflog entries and previews their patch. No apply/drop action exists.

## Overlays

Search, refs selection, comparison selection, command palette, errors, and help
are bounded overlays. They never permanently divide the screen. Errors retain
context, include the failed operation, and offer retry/copy where applicable.

## Command palette

The palette exposes every semantic action by searchable name. This makes
features discoverable without consuming permanent footer space and gives custom
keymaps a universal fallback.

## Selection mode

Selection may target commit, ref, file, or hunk. The footer clearly states that
`Enter` emits and exits while `Esc` cancels. Cancellation returns exit code 1
and writes nothing to stdout.

## Accessibility and terminal behavior

- Meaning is never conveyed by color alone.
- `NO_COLOR` and monochrome themes preserve selection and diff semantics.
- Unicode graph, selection, divider, and overlay glyphs have a centralized ASCII
  set. `ui.glyphs` can force either set; `auto` selects ASCII for `TERM=dumb`.
- Mouse is optional and disabled by default.
- Resize is lossless; the logical selection survives reflow.
- Every exit path restores cursor, raw mode, mouse/focus state, and screen.
