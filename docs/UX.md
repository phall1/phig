# Interaction specification

## Visual model

The width-budgeted header names the active view in a reverse-video badge and
the repository first, reserves space for critical selection/error state, and
then adds branch, revision, path, loading, and marked-endpoint context as room
permits. The body is one dominant list or document. At 110 columns and wider,
log, refs, status, blame, and stash previews occupy the right side behind one
thin vertical divider; lists keep about half the width (blame, which is read as
source, keeps more) and the preview gets one cell of padding. Normal widths use
a stacked list/preview with one horizontal divider; narrow terminals hide
previews and open details with `Enter`. Empty lists drop the preview and state
why they are empty; loading and unavailable states are centered notices. The
footer shows up to three effective, contextual key hints (four in the log), then
always `? help`, with keys in normal weight and descriptions muted, plus a quiet
right-aligned position (`12/256+` while more history can stream in).

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
| `T` | browse changed-file tree; arrows fold directories, `-`/`+` fold/unfold all, `Enter` opens |
| `S` | toggle unified/side-by-side diff (unified below 100 patch columns) |
| `F` | expand/restore the active diff |
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
| `?` | key sheet (any listed key closes it and runs) |
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

Rows are a grid: graph glyphs, short object ID, date, author, then named refs
and subject. Date and author widths are planned once per screen, so every
column starts on the same cell on every row; the author column narrows and then
drops before the date as width shrinks, and dates are compact (`14d`, or
`2026-09-12 15:43` in absolute modes). Named refs sit inline before the subject,
where they cannot shift the grid, and take at most the room that leaves the
subject readable. HEAD, local branches, remotes, and tags stay distinct by
prefix (`HEAD→`, `tag:`, remote `/`) as well as color; local branch names use
the matching lane color. Git reports full refnames, so a slashed local branch
such as `feat/x` is never mistaken for a remote, and a remote's symbolic
`HEAD` is omitted as redundant.
When a branch tip has scrolled off screen, the first visible commit of that
lane repeats its name more quietly, and the selected commit does the same when
it has no decorations of its own. Graph and text degrade cleanly on narrow
terminals. Preview shows selected commit metadata, including the same named
refs, and the patch. Additional history loads before the cursor reaches the end.

The graph draws one row per commit. Lanes carry real connecting edges: a merge
opens a lane and marks its node distinctly, a lane closes into the commit that
absorbs it, a run crossing a live lane draws a crossing, and a commit with no
parents ends its lane with a root glyph. Lane colors cycle the configured theme
so adjacent branches stay separable, and the whole repertoire has an ASCII
fallback. Lane count is budgeted from terminal width; beyond that budget lanes
fold into a `~`-marked final column rather than pushing the commit text off
screen. The bundle is explicitly approximate on screen; all underlying parent
identities remain intact. Branch colors cycle `theme.graph_lanes` and follow
their ancestry through joins and reused columns, and the selected branch is
bold across the visible graph.
Selection is marker-led: the selected log row keeps graph and decoration colors
and adds emphasis, rather than repainting the row as a single accent.
Previously computed graph rows are reused during navigation.

A ref scope (`--all`, `--branches`, `--remotes`, `--tags`) widens the walk from
one revision to whole ref families, which is what makes remote branches visible
as graph lanes. The header names the active scope instead of pinning it to a
single object, because a scope has no single target.

### Commit/diff

Metadata precedes file summary and patch: the object id and named refs, the
subject in bold, one byline (`author · 14d ago · date · email`), and a stats
line (`parent …` or merge parents with the selected one underlined, file count,
`+N -M`). The body follows, reflowed when the pane is narrower than its
hard-wrapped lines while lists and trailers keep their own lines; the log
preview shows three body rows and the detail view up to two fifths of the
height, ending with `… N more lines` when cut. Each file in the patch opens with
a banner row (see [diff review](diff-review.md)). `f` opens a fuzzy-searchable
changed-file index; `Enter` jumps directly to the selected file header. Hunk headers are
anchors. Merge commits expose explicit parent cycling with `P`; version 1 does
not claim a combined-diff display.

`T` opens a temporary changed-file tree with directory grouping and change counts.
It reads like a file browser: tree guides (`├`/`└`), directories first, a per-file
change badge (`A`/`D`/`M`/`R`), a right-aligned `+N −M` column, and a footer with
the selected full path. Movement previews the selected path at wide widths;
`Enter` opens it as a dominant patch at every width. Left/Right collapse/expand
directories; `-` folds and `+` unfolds the whole tree. `Esc` cancels and restores
the original patch position. Unified patches have old/new line gutters and
stronger intra-line change emphasis. See [diff review](diff-review.md).

`F` expands the active patch from log, refs, status, blame, or stash to the full
body, including on narrow terminals. Its sticky header identifies the commit
or staged/unstaged patch and current file/hunk. `F`, `Enter`, or `Esc` restores
the prior layout without reloading Git or changing the selected item or patch
position. Search, file/hunk jumps, and paging operate on the expanded patch;
resizing keeps it expanded. From commit detail, `F` hides the metadata for more
patch space. Comparison retains its endpoint and merge-base header.

### Compare

The header always states either `LEFT → RIGHT` for exact endpoint comparison or
`merge-base(BASE, HEAD) → HEAD` for branch comparison, with endpoint labels
emphasized and resolved ids beside them. A second row shows ahead/behind counts,
changed files, and `+N -M`, plus the requested inputs when they differ from the
labels. The patch follows. Users can swap endpoints and choose refs
without checkout.

### Refs

Branches, remotes, and tags are searchable. Each row is a grid of kind, name
(`*` marks the checked-out branch), target ID, age, and subject, with the
upstream trailing as `→ origin/name` when there is room. Opening a ref
changes the viewed history; it never checks anything out.

### Status

Porcelain-v2 records use compact `XY` codes and are grouped into conflicted,
staged, mixed staged+unstaged, unstaged, and untracked entries; each group is
named once at its first row, the index letter reads as staged and the worktree
letter as unstaged, and paths keep their directory quiet. The preview header
names the side shown and the key for the other (`staged diff · d unstaged`). `d` switches
between the two patches for mixed entries. Opening an entry displays the
relevant read-only diff as the dominant surface, including on narrow terminals.
No key mutates the index or worktree.

### Tree

Lists the selected revision's tree by name with a trailing `/` on directories,
a word only for unusual modes (`exec`, `link`, `sub`), and a right-aligned
human size. Directories descend; blobs open line-numbered content (clipped, not
wrapped, so one step is one line) or a safe binary summary. `Backspace`
ascends and the header retains the current tree breadcrumb.

### Blame

Shows commit, age, author, and line-numbered source with attribution printed
once per run of lines from the same commit. Opening a blame group
jumps to the commit while retaining path context.

### Stash

Lists stash reflog entries and previews their patch. No apply/drop action exists.

## Overlays

Search, refs selection, comparison selection, command palette, errors, and help
are bounded overlays with one cell of inner padding. Help is a sectioned key
sheet (Move, Find, Diff, Views, Compare) laid out in as many aligned columns as
fit; a shared modifier is written once (`Ctrl+d/u`). It is also a launcher:
`Esc` closes it and any other key closes it and does what it lists, so `:`
opens the palette. They never permanently divide the screen. Errors retain
context, include the failed operation, and offer retry/copy where applicable.

## Command palette

The palette exposes every semantic action by searchable name. This makes
features discoverable without consuming permanent footer space and gives custom
keymaps a universal fallback.

Both the palette and changed-file picker accept case-insensitive subsequences:
`tglpr` finds **Toggle preview**, and `smrs` finds **src/main.rs**. Exact matches
rank first, followed by contiguous matches (favoring word/path boundaries), then
abbreviations with fewer gaps. Empty queries retain the original list order;
equal-ranked results retain their relative order. Leading/trailing query spaces
are ignored; internal spaces remain literal. Active-view `/` search remains
literal, case-insensitive text search in every view, including blobs. While a
query is active its hits are shown in reverse video in commit ids, authors,
subjects, patch lines, and blob lines; the search bar states `no match` in
words when nothing is found and keeps the last position.

The pickers highlight matching characters, show a live result count, and keep
their geometry steady while typing. `Tab`/`Shift-Tab`, `Ctrl-n`/`Ctrl-p`, and
`Ctrl-j`/`Ctrl-k` move through results and `Ctrl-u` clears the query; plain
letters always type. With an empty query the palette lists discoverable
commands before basic movement. The palette also shows effective remapped
shortcuts. Long queries scroll to keep the insertion point visible; empty results
offer a recovery hint. Arrows move through the ranked results, `Enter` runs or
jumps, and `Esc` closes without changing the original diff position. Monochrome
uses the normal marker-led selection without colored match styling.

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
