# Configuration

Phig works without configuration. `phig config init` writes the complete,
commented version-1 file and never overwrites an existing file unless `--force`
is supplied.

## Location and precedence

The default is `$XDG_CONFIG_HOME/phig/config.toml` when `XDG_CONFIG_HOME` is
set, otherwise `~/.config/phig/config.toml` on both macOS and Linux. Phig does
not substitute macOS `Application Support` for this XDG contract.

```sh
phig config path
phig config init
phig config check
phig --config ./project-phig.toml config check
phig --no-config log
```

`--config PATH` loads exactly that file and fails when it is missing.
`PHIG_CONFIG=PATH` is its environment equivalent. `--no-config` overrides both
and ignores every file. Command-line options override configuration;
`--no-alt-screen` therefore always wins over `ui.alternate_screen = true`.
When the default file is absent, built-in defaults are used silently.
`config check` validates this same effective choice: absent defaults and
`--no-config` succeed, while a missing explicit `--config`/`PHIG_CONFIG` fails.

Parsing is strict at every table level. Unknown fields, unsupported enum values,
invalid colors, duplicate key assignments, unknown semantic actions, and a
version other than `1` exit with code 2 and name the source file and TOML
location. To recover from a broken file, run `phig --no-config`, fix the file,
and then run `phig config check`.

## Complete format

The distributed [`assets/config.example.toml`](../assets/config.example.toml)
is the canonical complete example. Main sections are:

- `[ui]`: contextual preview, alternate screen, mouse policy, date format,
  color, glyph, and clipboard policy. `color = "never"` strips colors and
  modifiers; `NO_COLOR` does the same in `auto` mode. `glyphs = "unicode"` or
  `"ascii"` is an explicit display override; `"auto"` uses Unicode except for
  `TERM=dumb`, where phig chooses ASCII. This is a predictable policy rather
  than terminal-capability detection. The zero-config default
  `clipboard = "osc52"` makes `y` copy the current semantic OID or path through
  the terminal. Set `clipboard = "off"` to emit no clipboard sequences; pressing
  `y` then reports that copying is disabled instead of implying success.
- `[diff]`: context lines, Git's `myers|minimal|patience|histogram` algorithm,
  and whitespace visibility/ignore policy.
- `[compare]`: the default `merge-base` or `exact` mode for comparisons started
  inside the TUI, plus an optional preferred base. Explicit `phig compare` is
  always merge-base; explicit `phig diff` is always exact.
- `[limits]`: bounded history pages, patch/blob bytes, and snapshot items.
- `[theme]`: named ANSI colors for accent, muted text, additions, removals,
  warnings, errors, and selection foreground/background. The calm default is
  accent-on-`reset`, preserving the terminal's native background; set a
  non-reset `selection_bg` to opt into block selection.
- `[keys]`: semantic action to key mappings such as `open = "enter"` and
  `view-refs = "ctrl+r"`.

Key names accept one character or `enter`, `esc`, `tab`, `backtab`,
`backspace`, arrows, page keys, home, and end, optionally prefixed by
`ctrl+`, `alt+`, or `shift+`. Every component is strict; unknown or duplicate
modifiers are errors. Assignments are remaps, not aliases: remapping `help`
disables `?`, and remapping `open` disables Enter until explicitly assigned.
Phig rejects two overrides that claim the same key. The documented semantic
navigation and view actions—including `file-picker` and `redraw`—are remappable;
internal overlay editing operations are intentionally not configuration keys.

Configuration never enables Git mutation, repository-controlled helpers,
network access, an embedded agent, or a daemon.
