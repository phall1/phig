# Changelog

All notable changes to phig are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- The changed-file tree (`T`) now reads like a file browser: box-drawing
  guides mark nesting, directories sort first with every subtree kept
  contiguous for folding, file rows show a change badge (`A`/`D`/`M`/`R`),
  addition/deletion counts sit in right-aligned columns, and a footer shows
  the selected full path. `-` folds and `+` unfolds the whole tree; status
  letters derive from diff header paths so renames and deletions keep their kind.

## [1.3.0] - 2026-09-12

### Added

- Old/new line-number gutters and bounded Unicode word-level change emphasis,
  powered by `similar`, in unified and side-by-side patches.
- `S` toggles split/unified diffs with source-anchor-preserving navigation and
  automatic unified fallback below 100 patch-pane columns.
- `T` opens a collapsible changed-file tree with directory change counts, live
  wide-screen previews, file opening, and exact position restoration on cancel.

### Changed

- `Tab`/`Shift-Tab` navigate files in all dominant diff views, including expanded
  previews and comparisons. File/hunk seeking avoids allocating an anchor list.
- Cache source coordinates and split-row mappings while constructing styled
  content only for visible rows; add large-patch navigation measurements.

## [1.2.0] - 2026-09-07

### Added

- `F` expands/restores the active diff while retaining its selection and scroll,
  with sticky file/hunk context and patch-aware search/paging on narrow terminals.
- Ranked fuzzy matching in the command palette and changed-file picker, with
  highlighted matches, live result counts, effective shortcut labels, stable
  overlay geometry, and a visible insertion point for long queries.
- Ref scope flags `--all`, `--branches`, `--remotes`, and `--tags` for `phig`,
  `phig log`, and `phig snapshot log`, so history can span remote-tracking
  branches and tags instead of only HEAD's ancestry. Naming a revision unions it
  with the scope; omitting one lets the scope define the walk. A scope also
  selects topological ordering, and commands that never walk history reject the
  flags as a usage error.

### Changed

- Crowded graphs preserve logical ancestry beyond screen width, explicitly mark
  folded lanes, retain branch colors, and emphasize the selected branch.
- Cache history graph prefixes and changed-file query results during navigation;
  deduplicate history pages with an OID set and batch bounded input bursts before
  drawing the next frame.
- Run Linux tests once in CI; validate release plans on PRs and scope heavy
  installer/package rehearsals to release-related changes, weekly and manual runs.
- Replaced the log's lane-glyph prefix with a connector graph: merges open
  lanes, joins close them, runs cross live lanes, and root commits terminate
  their lane. Lane colors cycle the configured theme, the full box-drawing
  repertoire has an ASCII fallback, and lane count is budgeted from terminal
  width.
- The log header names an active ref scope rather than pinning it to a single
  object.

## [1.1.1] - 2026-08-19

### Fixed

- Isolated shell-installer tests with an explicit temporary `CARGO_HOME` so
  running the release suite can never shadow a developer's installed `phig`.

## [1.1.0] - 2026-08-19

### Added

- Explicit `auto`, `unicode`, and `ascii` glyph policies with coherent graph,
  selection, divider, and overlay fallbacks.
- Style-aware golden coverage for every primary view, adaptive breakpoints,
  overlays, themes, remapped keys, ASCII, and monochrome rendering.
- Typed per-session rendering options for themes, color, dates, glyphs, and
  effective key labels while retaining the 1.0 compatibility entry points.

### Changed

- Refined the interface around native terminal backgrounds, marker-led
  selection, thin functional dividers, quieter contextual footers,
  width-prioritized headers, and compact adaptive overlays.
- Reorganized the application core, terminal adapter, renderer, and
  configuration system into small responsibility-based modules with stable
  public façades.
- Made all width budgeting terminal-cell aware and kept commit subjects useful
  across narrow layouts and every date mode.
- Made the benchmark fixture self-identifying so performance gates cannot
  silently reuse an unrelated repository.

### Fixed

- Honored semantic key remaps consistently in help and text-entry overlays,
  including bracketed paste in the changed-file picker.
- Rejected stale cross-view Git responses and prevented hidden preview focus in
  narrow layouts.
- Kept page movement aligned with visible compare, status-diff, and truncated
  patch rows.
- Cleared stale previews and reported honest positions and footer state for
  empty refs, status, blame, tree, and stash views.
- Normalized uppercase key bindings and rejected the reserved `Ctrl-C` binding
  with an actionable configuration error.

## [1.0.0] - 2026-08-19

### Added

- Fast asynchronous commit history with graph lanes, search, pagination, and
  diff previews.
- Commit detail, explicit parent navigation, searchable changed-file picker,
  file/hunk movement, and full-screen patches.
- Explicit merge-base `compare` and exact-endpoint `diff` workflows with marked
  endpoints and mode switching.
- Read-only refs, status/conflict, revision tree/blob, blame, and stash views.
- Strict XDG TOML configuration, semantic key remapping, themes, color/date
  policy, diff options, and bounded resource limits.
- Interactive commit/ref/file/hunk/line/comparison selection with a controlling
  terminal and clean stdout.
- Bounded deterministic `phig/1` JSON snapshots, schema, version envelope,
  pagination offsets, shell completions, and complete manual pages.
- Alternate-screen opt-out, mouse opt-in, zero-config OSC 52 copying with an
  explicit off mode, suspend/resume, signal restoration, and adaptive narrow,
  stacked, and wide right-side preview layouts.
- Explicit installer-aware update checks and updates.
- GitHub Release automation for macOS and Linux ARM64/x86-64 archives,
  SHA-256 checksums, attestations, shell installer, and Homebrew tap formula.

### Security

- Repository-controlled commands, prompts, hooks, external diff/textconv,
  pagers, lazy object fetching, replacements, and terminal control sequences
  are disabled or sanitized on inspection paths.

[Unreleased]: https://github.com/phall1/phig/compare/v1.1.1...HEAD
[1.1.1]: https://github.com/phall1/phig/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/phall1/phig/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/phall1/phig/releases/tag/v1.0.0
