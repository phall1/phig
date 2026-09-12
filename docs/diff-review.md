# Diff review experience

## Summary

Make patches easy to read and changed files easy to explore, with reversible
navigation that retains the original commit, selection, and patch position.
Design references: [Pierre](https://diffs.com/),
[OpenCode](https://opencode.ai/v2/docs/cli/config#diffs),
[Hunk](https://hunk.dev/), and [SeenDiff](https://seendiff.com/).
Figma: none provided; the user requested adapting these viewer references.

## Behavior

1. Unified patches show subdued old/new line numbers beside explicit `+`/`-`
   signs. Metadata and hunk headers remain distinct. Replacement lines emphasize
   their differing span; unrelated additions/deletions remain whole-line changes.
   Unicode stays intact, and monochrome retains signs and emphasis.
2. `T` opens a changed-file tree for the active patch. It reads like a file
   browser: box-drawing guides (`├`/`└`/`│`) mark nesting, directories sort
   first (then files, case-insensitively) with every subtree kept contiguous
   for folding, file rows carry a change badge (`A`/`D`/`M`/`R`), and
   addition/deletion counts sit in right-aligned columns instead of trailing
   text. A footer shows the selected full path and counts. The current file is
   selected on open. This is a temporary review surface, available from any
   loaded diff.
3. `j`/`k`, arrows, paging, and first/last navigate the tree. `Left` collapses a
   directory or goes to its parent; `Right` expands it. `-` folds and `+`
   unfolds every directory. `Enter` toggles a directory or opens a file. On wide
   terminals a file selection previews its patch; narrow terminals show the tree
   alone until the file is opened.
4. `Esc` cancels tree browsing and restores the exact original patch position.
   Opening a file retains its chosen position and expands the patch when needed.
   Opening the tree again selects that file. Existing `f` fuzzy search remains
   available after leaving the tree; overlays do not stack.
5. Fullscreen patches retain file/hunk location. `Tab`/`Shift-Tab` move between
   files in every dominant diff, including expanded previews and comparisons.
   Expanding/restoring, tree browsing, and file/hunk jumps issue no Git requests.
6. Empty diffs explain why there is no tree. Binary, rename, mode-only, and
   truncated patches keep Git's metadata; paths retain byte identity. Incoming
   patch replacements dismiss stale tree snapshots. Change badges derive from
   diff header paths, so renames and deletions keep their kind in the tree.
7. `S` switches unified/side-by-side presentation without changing the source
   anchor. Split view pairs equally sized replacement runs and shows context on
   both sides. Below 100 patch-pane columns it falls back to unified; widening
   restores split. Movement and paging advance visual rows in the effective mode.

## Implementation and acceptance evidence

Presentation-only line indexes are built when patch results enter the reducer;
they never enter machine JSON or Git records. Cached navigation constructs only
visible styled rows. `similar` 2.7 (also used by Grok's Rust pager) supplies Unicode
word refinement, bounded to 1,000-byte lines with a 2 ms algorithm deadline per
comparison and equally sized adjacent replacement runs. Refinement runs only for
visible rows; ambiguous pairings use whole-line styling. Public App mutations are
validated against cached line kinds and hunk metadata without rescanning text.
The tree snapshots structural metadata on open,
rebuilds its visible index only on collapse/expand, and paints a viewport slice.
Entries are laid out as a depth-first preorder (directories first, then files,
case-insensitively at every level) so a folded directory hides one contiguous
run; parent lookups and collapse-all/expand-all reuse the same prefix index.

Verify parser-backed line numbers, Unicode emphasis, replacement bounds, reducer
open/cancel/refresh behavior, semantic remaps, narrow/wide/monochrome UI buffers,
and cached large-patch navigation. Run `just check`, review the final diff in a
fresh context, and record measured complexity and performance evidence.

### Measured navigation

Local macOS/Apple Silicon, Rust 1.88 release build, Ratatui TestBackend 140×40,
102,000 synthetic patch rows with replacements, 200 move-and-paint samples per mode:

| Operation | p50 | p95 |
| --- | --- | --- |
| Unified move + buffer paint | 0.584 ms | 0.606 ms |
| Split move + buffer paint | 0.779 ms | 2.708 ms |

Final validation run: index construction took 1.470 ms. Run with
`cargo test --release --lib diff_review_benchmark -- --ignored --nocapture`.
These measure application/buffer work, not terminal transport or end-to-end Git
latency. File/hunk seeking also no longer allocates a complete anchor vector.

### Complexity and checks

Lizard's Rust measurements for the touched navigation/rendering functions:

| Function | Before | After |
| --- | --- | --- |
| `seek_diff_anchor` | 5 | 3 |
| `render_diff_value` | 5 | 6 |
| `page_rows` | 9 | 9 |
| `render_split` | new | 2 |
| `render_split_row` | new | 6 |
| `update_diff_tree` | 6 | 6 |
| `collapse` | 4 | 5 |
| `expand` | 4 | 4 |
| `render_list` | 4 | 4 |
| `entry_line` | 5 | 6 |
| `TreeStatus::of` | new | 3 |
| `collapse_all` / `expand_all` | new | 3 / 2 |
| `parent_prefix` | new | 2 |
| `by_kind` / `flatten` | new | 2 / 4 |
| `guide_prefix` / `has_sibling_after` | new | 6 / 4 |
| `status_style` / `footer_line` | new | 4 / 2 |

Rendering is separated into unified, split, source-content styling, and tree
helpers. New production functions measure at most 6. Validation passes on the
pinned Rust 1.88 toolchain: `just check` (241 tests plus doc-test check),
`just security`, real-PTY tree/open/split/restore navigation, and fresh-context
review with both reported key-handling issues fixed and regression-tested.
