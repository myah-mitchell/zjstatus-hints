# zjstatus-hints

A [Zellij](https://github.com/zellij-org/zellij) plugin that displays context-aware key bindings for each Zellij mode. Extends the functionality of [zjstatus](https://github.com/dj95/zjstatus).

![2025-06-06_16-23-55_region](https://github.com/user-attachments/assets/cfb93423-f37c-410a-aca9-a49290312d0e)

https://github.com/user-attachments/assets/940a31a0-86de-469d-89e2-dab18a1aaca8

## Rationale

Zjstatus is an excellent plugin, but it lacks the ability to display keybinding hints for your current mode, as the built-in Zellij status-bar plugin allows. This plugin adds that functionality to zjstatus, so you can have the best of both worlds.

## Features

- Shows context-aware key bindings for each Zellij mode (Normal, Pane, Tab, Resize, Move, Scroll, Search, Session)
- Integrates seamlessly with zjstatus via named pipes

## Installation

First, install and configure [zjstatus](https://github.com/dj95/zjstatus). Then, add the zjstatus-hints plugin to your Zellij configuration:

```kdl
plugins {
    zjstatus-hints location="https://github.com/b0o/zjstatus-hints/releases/latest/download/zjstatus-hints.wasm" {
        // Maximum number of characters to display
        max_length 0 // 0 = unlimited
        // String to append when truncated
        overflow_str "..." // default
        // Name of the pipe for zjstatus integration
        pipe_name "zjstatus_hints" // default
        // Hide hints in base mode (a.k.a. default mode)
        // E.g. if you have set default_mode to "locked", then
        // you can hide hints in the locked mode by setting this to true
        hide_in_base_mode false // default

        // Show every enabled keybinding in each mode, not just the curated set.
        // Set to false to show only the curated hints. See "Discovered hints".
        discover_hints true // default
        // Override or set the label for a discovered action (snake_case name);
        // an empty value hides it. See "Discovered hints" below.
        label_switch_to_mode_locked "lock"

        // Optionally style the hints using zjstatus-style format strings.
        // When unset, the current Zellij theme palette is used automatically.
        // `{key}` and `{desc}` are replaced with the keybinding and its label.
        // See the "Styling" section below for the full syntax.
        key_format  "#[fg=$black,bg=$blue,bold] {key} "
        desc_format "#[fg=$fg,bg=$bg] {desc} "

        // `$name` colors resolve to the matching `color_<name>` option, exactly
        // like zjstatus color aliases.
        color_black "#1e1e2e"
        color_blue  "#89b4fa"
        color_fg    "#cdd6f4"
        color_bg    "#313244"

        // Optionally replace key names with symbols, e.g. show ENTER as ↵.
        // Uses the same per-line alias pattern (see "Key aliases" below).
        key_alias_enter "↵"
        key_alias_space "␣"
        key_alias_esc   "⎋"
    }
}

load_plugins {
    // Load at startup
    zjstatus-hints
}
```

Finally, configure zjstatus to display the hints in your default layout (`layouts/default.kdl`):

```kdl
layout {
    default_tab_template {
        children
        pane size=1 borderless=true {
            plugin location="zjstatus" {
                format_left   "{mode} {tabs}"

                // You can put `{pipe_zjstatus_hints}` inside of format_left, format_center, or format_right.
                // The pipe name should match the pipe_name configuration option from above, which is zjstatus_hints by default.
                // e.g. pipe_<pipe_name>
                format_right  "{pipe_zjstatus_hints}{datetime} " 

                // Note: this is necessary or else zjstatus won't render the pipe:
                pipe_zjstatus_hints_format "{output}"
            }
        }
    }
}
```

## Configuration

- `max_length`: Maximum number of characters to display (default: 0 = unlimited)
- `overflow_str`: String to append when truncated (default: "...")
- `pipe_name`: Name of the pipe for zjstatus integration (default: "zjstatus_hints")
- `hide_in_base_mode`: Hide hints in base mode (a.k.a. default mode) (default: false)
- `discover_hints`: Show **every** enabled keybinding in each mode, not just the curated set (default: true); see [Discovered hints](#discovered-hints)
- `key_format`: Format string for the keybinding portion of each hint (default: unset — use theme palette)
- `desc_format`: Format string for the description portion of each hint (default: unset — use theme palette)
- `color_<name>`: Define a color alias referenced as `$name` in the format strings above
- `key_alias_<name>`: Replace a key's name with a symbol (e.g. `key_alias_enter "↵"`); see [Key aliases](#key-aliases)
- `label_<action>`: Override or set the label for a discovered action (e.g. `label_new_pane "new"`); see [Discovered hints](#discovered-hints)

## Styling

Each hint is rendered in two parts: the **key** (e.g. `Ctrl + p`) and its
**description** (e.g. `pane`). By default both are styled from the active Zellij
theme palette so they blend in with the rest of your status bar.

To customize them, set `key_format` and/or `desc_format`. These use the same
format-string syntax as any other [zjstatus](https://github.com/dj95/zjstatus)
widget, so styling hints works just like styling `{mode}`, `{tabs}`, and friends:

- Wrap styling directives in `#[...]`; everything after a block is painted with
  that style until the next block.
- `{key}` and `{desc}` are placeholders substituted with the keybinding text and
  its label, respectively.
- If either option is unset (or empty), that part falls back to the theme palette,
  so you can restyle just the keys, just the descriptions, or both.

```kdl
key_format  "#[fg=$black,bg=$blue,bold] {key} "
desc_format "#[fg=#cdd6f4,bg=#313244,italic] {desc} "
```

### Directives

Inside `#[...]`, comma-separate any of the following:

- `fg=<color>` — foreground color
- `bg=<color>` — background color
- Effects: `bold`, `italic`, `underscore`, `blink`, `dim`, `strikethrough`, `reverse`, `hidden`

### Colors

`<color>` accepts the same forms as zjstatus:

- `#RRGGBB` — hex RGB (e.g. `#89b4fa`)
- A named color: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
  `white`, and their `bright_*` variants
- `0`–`255` (or `colour<N>`) — an ANSI 256 color index
- `$name` — a color alias, resolved from the matching `color_<name>` option

> Note: directives zjstatus supports but the hint renderer cannot express
> (e.g. `us=` underline colors and the fancy underline variants) are accepted
> and ignored, so a palette shared with zjstatus never errors.

### Key aliases

By default keys are rendered with their Zellij names (`ENTER`, `ESC`, `TAB`,
`SPACE`, `←`, …). Replace any of them with a symbol using `key_alias_<name>`
options — the same per-line alias pattern as `color_<name>`:

```kdl
key_alias_enter     "↵"
key_alias_space     "␣"
key_alias_esc       "⎋"
key_alias_tab       "⇥"
key_alias_backspace "⌫"
key_alias_left      "←"
```

- `<name>` is the lowercase key name. Aliases apply everywhere the key appears,
  including inside the `{key}` placeholder of a custom `key_format`.
- Keys without an alias keep their default representation, so you only need to
  set the ones you want to change.
- Recognized names: `enter` (`return`), `esc` (`escape`), `tab`, `space`,
  `backspace`, `delete` (`del`), `insert` (`ins`), `home`, `end`,
  `pageup` (`pgup`), `pagedown` (`pgdn`), `up`, `down`, `left`, `right`,
  `capslock`, `scrolllock`, `numlock`, `printscreen`, `pause`, `menu`,
  `f1`–`f12`, and any single character (e.g. `key_alias_x`).

## Discovered hints

By default the plugin shows **every keybinding that is actually enabled in the
current mode**, so the hints always reflect your real config — including custom
binds and the many default bindings the original curated list omitted (e.g. the
`Alt-*` quick keys and `lock` in Normal mode).

It works in two passes:

1. A curated set of common actions is rendered first, with hand-tuned labels and
   ordering (this is the polished default look).
2. Every *other* enabled keybinding is then discovered and appended. Keys that
   resolve to the same label are grouped into a single hint (so, e.g., the four
   directional focus keys collapse into one `move` hint).

Set `discover_hints false` to disable the second pass and show only the curated
set.

### Labels

Each discovered binding is labeled by resolving, in order:

1. A `label_<action>` config override, if set.
2. A built-in label for common actions (`new_pane` → "new", `move_focus` →
   "move", `toggle_pane_frames` → "frames", …).
3. A fallback derived from the action name itself (`next_swap_layout` →
   "next swap layout"), so nothing is ever left unlabeled.

`<action>` is the action's snake_case name; mode switches include the target
mode. Examples:

```kdl
label_switch_to_mode_locked "lock"    // relabel the lock keybinding
label_new_pane              "new"
label_move_focus            "focus"
label_go_to_tab             "tab"
label_next_swap_layout      "»"       // give a custom glyph
label_toggle_mouse_mode     ""        // empty string hides this hint
```

An empty value (`label_<action> ""`) hides that action's hint. To find an
action's name for a key you have bound, note the action from your Zellij config
and convert it to snake_case (e.g. `MoveTab` → `move_tab`).

## TODO

- [x] configurable colors/formatting
- [x] more advanced mode-specific configuration
- [ ] improved handling of long outputs
- [x] ability to enable/disable specific hints

## License

&copy; 2025 Maddison Cohodas

Adapted from the built-in Zellij status-bar plugin by Brooks J Rady.

MIT License
