# Changelog

All notable changes to phig are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and releases follow
[Semantic Versioning](https://semver.org/).

## [1.9.0](https://github.com/phall1/phig/compare/v1.8.0...v1.9.0) (2026-11-26)


### Added

* **cli:** add config and agent-friendly composition ([169ed31](https://github.com/phall1/phig/commit/169ed31fc2bd927af1491fdd1a78df17e113f30b))
* **cli:** add ref scope flags to history walks ([ea50f95](https://github.com/phall1/phig/commit/ea50f95952b49d041628400a80deec68fc37ab66))
* **core:** implement safe Git inspection engine ([933712e](https://github.com/phall1/phig/commit/933712e7c30e3e3a28af6ac2de24dcbe39bfcf9c))
* ref scope flags and a real connector graph ([b569d01](https://github.com/phall1/phig/commit/b569d016617bdb01443acad610452608f674b35c))
* **tree:** browse changed files like a clean file browser ([d9c2b41](https://github.com/phall1/phig/commit/d9c2b41347d344f63cf436b43dc56d669f11c6a0))
* **tui:** add a faster changes-only diff lens ([#29](https://github.com/phall1/phig/issues/29)) ([f416fcc](https://github.com/phall1/phig/commit/f416fccccef4afea34efc6c12dfa6efdea4418a1))
* **tui:** add fast history and diff browser ([d893840](https://github.com/phall1/phig/commit/d8938403248fcd203674e621bd9439ba926bd8bb))
* **tui:** add ranked fuzzy command and file discovery ([065546b](https://github.com/phall1/phig/commit/065546b59806a744f02734d4537d6242fcb5b159))
* **tui:** aligned log grid, file banners, and a sharper visual system ([0dc21b8](https://github.com/phall1/phig/commit/0dc21b81d047089b1c972447a0f02a79b2bfce5d))
* **tui:** draw a real connector graph in the log ([2ecd111](https://github.com/phall1/phig/commit/2ecd11175c9dce9cbe69d0451f9a6718cbca8475))
* **tui:** highlight search hits, report misses, search blobs ([314445b](https://github.com/phall1/phig/commit/314445bd36f0e5860abcf8c6e505ff182162774c))
* **tui:** name graph lanes so branches stay identifiable ([53e7907](https://github.com/phall1/phig/commit/53e790753d120ea012d88e6144d362e45b7a26b5))
* **tui:** name graph lanes so branches stay identifiable ([222f0e0](https://github.com/phall1/phig/commit/222f0e040a71624e4ab79d80b00d9c8c82436605))
* **tui:** polish diff reading and changed-file navigation ([194e203](https://github.com/phall1/phig/commit/194e2034c789e1074c4c914fa2d6ae9f8202a211))
* **tui:** rebuild Phig around ratcn ([fd11258](https://github.com/phall1/phig/commit/fd11258431d7f3f36cead795add4016b9e746101))
* **tui:** refine the visual system ([28ef911](https://github.com/phall1/phig/commit/28ef91100f9a63d3b3ed38e6a1e37cfe91e74913))
* **tui:** roomier log split, first-name authors, faithful demo ([b0b5ed8](https://github.com/phall1/phig/commit/b0b5ed84cc553c076cf5132e5c351bc8f8c28be4))
* **tui:** speed up branch graphs and add fullscreen diffs ([6163716](https://github.com/phall1/phig/commit/61637165aab9e57ae96897287aa3e6e5101523f3))
* **views:** add compare and inspection workflows ([487a41a](https://github.com/phall1/phig/commit/487a41a2e47ee7c40961cbe0d76026ce8fb0983e))


### Fixed

* **ci:** make platform gates deterministic ([bcdbe3d](https://github.com/phall1/phig/commit/bcdbe3d7d57c7f5eb46ba80368d9b739f3d528f6))
* **ci:** use portable pipe and shared-runner budgets ([d4ba7c6](https://github.com/phall1/phig/commit/d4ba7c6732d36bc07490123a5b35bd61e60412b9))
* **release:** harden final 1.0 gates and workflows ([ecaf54b](https://github.com/phall1/phig/commit/ecaf54b1673d997fc5034698b2586b7bcc123430))
* **release:** isolate installer test environment ([4b89ad3](https://github.com/phall1/phig/commit/4b89ad3e922bfc78f9ed5b4fdd12d21237174be7))
* **tui:** address review findings on scrolling, split, detail, and output ([93194be](https://github.com/phall1/phig/commit/93194be846645fc52fb8d4430e429e5852298a68))
* **tui:** harden modal and terminal invariants ([79372b2](https://github.com/phall1/phig/commit/79372b214888377d9f47ad8795c2665037a99115))
* **tui:** keep Esc cancel visible in the changed-file tree footer ([a86191e](https://github.com/phall1/phig/commit/a86191e6e64e2c2edefa68474c4d8cb77a7283be))
* **tui:** keep history columns stable while scrolling ([#32](https://github.com/phall1/phig/issues/32)) ([2ea2bd5](https://github.com/phall1/phig/commit/2ea2bd5159bc6edd7226cfa9c81f7382e0284e45))
* **tui:** quieter exact-compare header, named ref previews, picker footer ([599a2e8](https://github.com/phall1/phig/commit/599a2e8264ed8f9acdeac0fbdcf3a65d2738cf90))
* **tui:** settle abandoned requests; help launches; readline picker keys ([baa9f6a](https://github.com/phall1/phig/commit/baa9f6a2137ef0e14f2ab1d26518da44a2d50a8c))


### Changed

* **config:** separate models keys and sources ([327cf7e](https://github.com/phall1/phig/commit/327cf7ef7d0e17e0eb4e0278d5ea20d4d9e16507))
* **core:** shape cohesive app and TUI modules ([60d5c55](https://github.com/phall1/phig/commit/60d5c55976321de272b96f0cf42594dcfa07fe00))
* **deps:** bump the rust-dependencies group across 1 directory with 11 updates ([#31](https://github.com/phall1/phig/issues/31)) ([46f345f](https://github.com/phall1/phig/commit/46f345f97920d0a9dc75cdb9bcf585d25b21f15e))
* **deps:** bump the rust-dependencies group across 1 directory with 3 updates ([eabd735](https://github.com/phall1/phig/commit/eabd735a12856dcd8d043982fa656c09d5408e6d))
* **deps:** bump the rust-dependencies group across 1 directory with 3 updates ([6f84652](https://github.com/phall1/phig/commit/6f846522aac6c796e78defdbac2a60b529f19ac9))
* **deps:** bump the rust-dependencies group across 1 directory with 4 updates ([795cee5](https://github.com/phall1/phig/commit/795cee5b955715ece80379d235e57783c19926ce))
* **deps:** bump the rust-dependencies group across 1 directory with 4 updates ([3544ed0](https://github.com/phall1/phig/commit/3544ed093522af9981ce147e2d02f49f3e5798e7))
* establish phig product and architecture ([8855e2e](https://github.com/phall1/phig/commit/8855e2e8ece882c280234cd41008741c653badad))
* **main:** release 1.5.0 ([fcbd086](https://github.com/phall1/phig/commit/fcbd0868b01072c1b96e276f722643ae154d3276))
* **main:** release 1.5.0 ([cd30726](https://github.com/phall1/phig/commit/cd307260cd1fb8cd8267ec7ff75e54254f516e7b))
* **main:** release 1.6.0 ([#20](https://github.com/phall1/phig/issues/20)) ([4b43960](https://github.com/phall1/phig/commit/4b4396060900c07e5e0d6e4baf4dbce2fc99579b))
* **main:** release 1.7.0 ([#28](https://github.com/phall1/phig/issues/28)) ([35971e8](https://github.com/phall1/phig/commit/35971e8518633d7d8464da241e034597dae3a91f))
* **main:** release 1.8.0 ([#30](https://github.com/phall1/phig/issues/30)) ([9ae7dfd](https://github.com/phall1/phig/commit/9ae7dfd383ac465302f5770b215688dba3fee5f8))
* **main:** release phig-cli vX.Y.Z. ([df624fc](https://github.com/phall1/phig/commit/df624fcf99cff2c5831b3fb2cb1c2d0fd4138854))
* **release:** let the tap render the formula instead of pushing it ([bdc45fe](https://github.com/phall1/phig/commit/bdc45fe15e7edcaa2514b1631e1f86530126c696))
* **release:** let the tap render the formula instead of pushing it ([b24b297](https://github.com/phall1/phig/commit/b24b2978ac04730ade4b04f9a91464f8bb770a1f))
* **release:** prepare v1.2.0 ([cde2b70](https://github.com/phall1/phig/commit/cde2b70cb2995839c7ea5706ad11f2ef6dc5e156))
* **release:** prepare v1.3.0 ([2210385](https://github.com/phall1/phig/commit/221038598a9aa1c22eba3f5159c62571fe875268))
* **release:** prepare v1.4.0 ([4889345](https://github.com/phall1/phig/commit/48893450119b07a67d751fd74cbfae6d650329ff))
* **tui:** share text-overlay keys; split footer hints ([63a50ce](https://github.com/phall1/phig/commit/63a50cebb7e544403ef79c015164580533bbd4c0))

## [1.8.0](https://github.com/phall1/phig/compare/v1.7.0...v1.8.0) (2026-11-26)


### Added

* **tui:** add a faster changes-only diff lens ([#29](https://github.com/phall1/phig/issues/29)) ([f416fcc](https://github.com/phall1/phig/commit/f416fccccef4afea34efc6c12dfa6efdea4418a1))


### Fixed

* **tui:** keep history columns stable while scrolling ([#32](https://github.com/phall1/phig/issues/32)) ([2ea2bd5](https://github.com/phall1/phig/commit/2ea2bd5159bc6edd7226cfa9c81f7382e0284e45))


### Changed

* **deps:** bump the rust-dependencies group across 1 directory with 11 updates ([#31](https://github.com/phall1/phig/issues/31)) ([46f345f](https://github.com/phall1/phig/commit/46f345f97920d0a9dc75cdb9bcf585d25b21f15e))

## [1.7.0](https://github.com/phall1/phig/compare/v1.6.0...v1.7.0) (2026-11-19)


### Added

* **tui:** rebuild Phig around ratcn ([fd11258](https://github.com/phall1/phig/commit/fd11258431d7f3f36cead795add4016b9e746101))

## [1.6.0](https://github.com/phall1/phig/compare/v1.5.0...v1.6.0) (2026-09-27)


### Added

* **tui:** aligned log grid, file banners, and a sharper visual system ([0dc21b8](https://github.com/phall1/phig/commit/0dc21b81d047089b1c972447a0f02a79b2bfce5d))
* **tui:** highlight search hits, report misses, search blobs ([314445b](https://github.com/phall1/phig/commit/314445bd36f0e5860abcf8c6e505ff182162774c))
* **tui:** roomier log split, first-name authors, faithful demo ([b0b5ed8](https://github.com/phall1/phig/commit/b0b5ed84cc553c076cf5132e5c351bc8f8c28be4))


### Fixed

* **tui:** address review findings on scrolling, split, detail, and output ([93194be](https://github.com/phall1/phig/commit/93194be846645fc52fb8d4430e429e5852298a68))
* **tui:** keep Esc cancel visible in the changed-file tree footer ([a86191e](https://github.com/phall1/phig/commit/a86191e6e64e2c2edefa68474c4d8cb77a7283be))
* **tui:** quieter exact-compare header, named ref previews, picker footer ([599a2e8](https://github.com/phall1/phig/commit/599a2e8264ed8f9acdeac0fbdcf3a65d2738cf90))
* **tui:** settle abandoned requests; help launches; readline picker keys ([baa9f6a](https://github.com/phall1/phig/commit/baa9f6a2137ef0e14f2ab1d26518da44a2d50a8c))


### Changed

* **tui:** share text-overlay keys; split footer hints ([63a50ce](https://github.com/phall1/phig/commit/63a50cebb7e544403ef79c015164580533bbd4c0))

## [1.5.0](https://github.com/phall1/phig/compare/v1.4.0...v1.5.0) (2026-09-12)


### Added

* **tui:** name graph lanes so branches stay identifiable ([53e7907](https://github.com/phall1/phig/commit/53e790753d120ea012d88e6144d362e45b7a26b5))
* **tui:** name graph lanes so branches stay identifiable ([222f0e0](https://github.com/phall1/phig/commit/222f0e040a71624e4ab79d80b00d9c8c82436605))


### Changed

* **main:** release phig-cli vX.Y.Z. ([df624fc](https://github.com/phall1/phig/commit/df624fcf99cff2c5831b3fb2cb1c2d0fd4138854))

## [Unreleased]

### Changed

- Log rows name their refs before the subject: `HEAD→` (or `HEAD->` in ASCII),
  local branches in the matching lane color, remotes, and `tag:` labels. Those
  names keep the width budget ahead of the author column, so they remain at
  60 columns. When a tip scrolls off screen, the first visible commit of that
  lane — and the selected mid-branch commit — repeat the name more quietly.
  Marker-led selection keeps graph and decoration colors instead of washing
  the selected row with accent. Commit preview metadata shows the same refs.

## [1.4.0] - 2026-09-12

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

[Unreleased]: https://github.com/phall1/phig/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/phall1/phig/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/phall1/phig/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/phall1/phig/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/phall1/phig/compare/v1.1.0...v1.1.1
[1.1.0]: https://github.com/phall1/phig/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/phall1/phig/releases/tag/v1.0.0
