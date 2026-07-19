# zjstatus-hints

A [Zellij](https://github.com/zellij-org/zellij) plugin that displays context-aware key bindings for each Zellij mode. Extends the functionality of [zjstatus](https://github.com/dj95/zjstatus).

![2025-06-06_16-23-55_region](https://github.com/user-attachments/assets/cfb93423-f37c-410a-aca9-a49290312d0e)

https://github.com/user-attachments/assets/940a31a0-86de-469d-89e2-dab18a1aaca8

## Rationale

Zjstatus is an excellent plugin, but it lacks the ability to display keybinding hints for your current mode, as the built-in Zellij status-bar plugin allows. This plugin adds that functionality to zjstatus, so you can have the best of both worlds.

## Features

- Shows a [curated list](#the-curated-list) of the key bindings that matter in
  your current mode, hand-labelled and ordered
- Optionally [discovers](#discovered-hints) **every** other binding your config
  enables, so nothing is hidden from you
- Integrates seamlessly with zjstatus via named pipes
- Styled with zjstatus's own format strings, or the active Zellij theme palette
  when you set nothing
- Fits the status bar: whole hints are dropped as the window narrows, in an
  order you control, with an optional indicator marking what was left out
- Relabel, hide, reorder, or merge any hint — globally or in a single mode
- Key and modifier aliases, so `Ctrl Left` can read `^←`

## Installation

First, install and configure [zjstatus](https://github.com/dj95/zjstatus). Then, add the zjstatus-hints plugin to your Zellij configuration:

```kdl
plugins {
    zjstatus-hints location="https://github.com/myah-mitchell/zjstatus-hints/releases/latest/download/zjstatus-hints.wasm" {
        // Hard cap on the width of the hint line, in columns
        max_length 0 // 0 = unlimited
        // Fit the hints to the terminal, dropping trailing hints as it
        // narrows. Reserve columns for whatever shares the bar.
        auto_width true // default
        reserve_columns 0 // default
        // Set to 2 if your terminal draws Nerd Font glyphs double-width.
        ambiguous_width 1 // default
        // Appended when a single hint is too wide to fit even alone
        overflow_str "..." // default
        // Name of the pipe for zjstatus integration
        pipe_name "zjstatus_hints" // default
        // Hide hints in base mode (a.k.a. default mode)
        // E.g. if you have set default_mode to "locked", then
        // you can hide hints in the locked mode by setting this to true
        hide_in_base_mode false // default

        // Also show every other keybinding your config enables, beyond the
        // curated list. See "Discovered hints".
        discover_hints false // default
        // Hide bindings that every mode inherits from the base mode, so each
        // mode only advertises what is new in it. See "Discovered hints".
        hide_shared_hints true // default
        // When a hint is bound to both hjkl and the arrows, show only one
        // family: "both" (default), "arrows", or "letters".
        direction_keys "both" // default
        // How keys within a hint are ordered: a keyboard layout
        // ("qwerty", "dvorak", "colemak"), "abcdef", or "none".
        key_order "qwerty" // default
        // Pin hints to the start or end of each mode; "*" is everything
        // else. Unset leaves the order each mode builds.
        hint_order "*, exit"
        // Override or hide any hint's label by its concept id; an empty
        // value hides it. See "Labels" below for the full list of ids.
        label_mode_locked "lock"
        label_split_down  "split ↓"

        // Optionally style the hints using zjstatus-style format strings.
        // When unset, the current Zellij theme palette is used automatically.
        // `{key}` and `{desc}` are replaced with the keybinding and its label.
        // See the "Styling" section below for the full syntax.
        key_format  "#[fg=$black,bg=$blue,bold] {key} "
        desc_format "#[fg=$fg,bg=$bg] {desc} "
        // Drawn between hints (never before the first or after the last).
        // Also a format string, so it can be a styled glyph, not just space.
        hint_spacer "#[fg=$fg] │ "
        // Shown where hints were dropped because the window is too narrow.
        drop_indicator "#[fg=$fg]…"
        // Which pinned group outlives the other: "tl" or "lt".
        hint_precedence "tl" // default

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

        // Likewise for modifiers: show Ctrl as ^ and Alt as ⌥.
        mod_alias_ctrl "^"
        mod_alias_alt  "⌥"
    }
}

load_plugins {
    // Load at startup
    zjstatus-hints
}
```

### Release channels

Two channels are published:

| Channel | Contents |
|---|---|
| `latest` | Tagged releases. What the URL above resolves to. |
| `nightly` | Rebuilt from `main` every night, tests green. Prerelease. |

Zellij caches remote plugins by URL, so pointing the config at the nightly URL
will keep serving whatever it downloaded first. To track nightlies, fetch into
the plugin path instead and start a new session:

```sh
make nightly   # or: make latest
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

- `max_length`: Hard cap on the hint line's width in columns (default: 0 = unlimited); see [Fitting the bar](#fitting-the-bar)
- `auto_width`: Fit the hints to the terminal width (default: `true`); see [Fitting the bar](#fitting-the-bar)
- `reserve_columns`: Columns to leave free for the rest of the status bar (default: 0)
- `ambiguous_width`: Columns an East Asian Ambiguous character occupies — `1` or `2` (default: `1`); see [Fitting the bar](#fitting-the-bar)
- `overflow_str`: Appended when a lone hint is too wide to fit and must be cut mid-hint (default: "..."); see [What gets dropped](#what-gets-dropped)
- `pipe_name`: Name of the pipe for zjstatus integration (default: "zjstatus_hints")
- `hide_in_base_mode`: Hide hints in base mode (a.k.a. default mode) (default: false)
- `discover_hints`: Also show every enabled keybinding beyond the [curated list](#the-curated-list) (default: false); see [Discovered hints](#discovered-hints)
- `hide_shared_hints`: Hide bindings inherited from the base mode so each mode only shows what is new in it (default: true); see [Shared bindings](#shared-bindings)
- `direction_keys`: Which keys to show when a hint is bound to both `hjkl` and the arrows — `both`, `arrows`, or `letters` (default: `both`); see [Direction keys](#direction-keys)
- `key_order`: How keys within a hint are ordered — `qwerty`, `dvorak`, `colemak`, `abcdef`, or `none` (default: `qwerty`); see [Key order](#key-order)
- `hint_order`: Comma-separated hint ids pinned to the start or end of each mode, around a `*` for everything else (default: unset); see [Hint order](#hint-order)
- `key_format`: Format string for the keybinding portion of each hint (default: unset — use theme palette)
- `desc_format`: Format string for the description portion of each hint (default: unset — use theme palette)
- `hint_spacer`: Format string drawn between consecutive hints (default: unset — hints sit adjacent); see [Spacing](#spacing)
- `drop_indicator`: Format string marking where hints were dropped to fit the window (default: unset — the gap is unmarked); see [What gets dropped](#what-gets-dropped)
- `hint_precedence`: Which pinned group is kept longest — `tl` or `lt` (default: `tl`); see [What gets dropped](#what-gets-dropped)
- `color_<name>`: Define a color alias referenced as `$name` in the format strings above
- `key_alias_<name>`: Replace a key's name with a symbol (e.g. `key_alias_enter "↵"`); see [Key aliases](#key-aliases)
- `mod_alias_<name>`: Replace a modifier's name with a symbol (e.g. `mod_alias_ctrl "^"`); see [Modifier aliases](#modifier-aliases)
- `label_<id>`: Override or hide the label of any hint, curated or discovered (e.g. `label_split_down "split ↓"`); see [Labels](#labels)
- `label_<mode>_<id>`: The same, scoped to one mode (e.g. `label_locked_mode_normal "unlock"`); see [Per-mode labels](#per-mode-labels)

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

### Spacing

By default hints sit directly against one another. `hint_spacer` inserts a
separator **between** consecutive hints — never before the first or after the
last, so it never leaks into the edges of the piped output:

```kdl
hint_spacer "  "             // just widen the gap
hint_spacer "#[fg=$grey] │ " // a styled divider
```

It is parsed as a format string like `key_format` and `desc_format` (it just has
no placeholders), so `$name` color aliases and all the directives below work in
it. Note that the default key styling already emits one leading space per hint,
so the spacer adds to that gap rather than replacing it.

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

### Modifier aliases

Modifiers work the same way via `mod_alias_<name>`, where `<name>` is one of
`ctrl`, `alt`, `shift`, or `super`:

```kdl
mod_alias_ctrl  "^"
mod_alias_alt   "⌥"
mod_alias_shift "⇧"
mod_alias_super "⌘"
```

Keys sharing a modifier are grouped into a single run, so a set of bindings like
`Ctrl h`, `Alt <`, `Ctrl k`, `Alt l` renders as `^hk ⌥<l` rather than repeating
the modifier four times.

How the modifier joins its keys depends on the alias: a word (`Ctrl`) is
followed by a space (`Ctrl hk`), while a symbol (`^`) hugs them (`^hk`). This is
decided by the last character, so you get the natural spacing either way without
configuring it.

### A complete alias set

A full set using only standard Unicode — no Nerd Font required, and no private-use
codepoints, so it renders in most terminal fonts:

```kdl
// Keys
key_alias_enter     "⏎"
key_alias_esc       "⎋"
key_alias_tab       "⇥"
key_alias_space     "␣"
key_alias_backspace "⌫"
key_alias_delete    "⌦"
key_alias_insert    "⎀"
key_alias_left      "←"
key_alias_down      "↓"
key_alias_up        "↑"
key_alias_right     "→"
key_alias_home      "⇱"
key_alias_end       "⇲"
key_alias_pageup    "⇞"
key_alias_pagedown  "⇟"

// Modifiers
mod_alias_ctrl  "^"
mod_alias_alt   "⌥"
mod_alias_shift "⇧"
mod_alias_super "⌘"
```

Turning `Ctrl Left` into `^←` and `Alt Shift PgDn` into `⌥⇧⇟`.

Nerd Fonts offer alternatives for several of these — arrows, Home/End, Page
Up/Down and Delete all have glyphs in the private-use area. They look sharper if
you have the font, but they render as tofu for anyone who doesn't, so the set
above is the safer default.

> **Width note:** the four arrows and `⇧` are East Asian **Ambiguous**, as are
> all Nerd Font glyphs — see [Glyph width](#glyph-width). If your terminal draws
> them double-width, set `ambiguous_width 2` or the hints will overflow slightly.
> The rest of the set above is unambiguously one column.

## Keys and ordering

### Direction keys

Actions like focus and move are typically bound to both the `hjkl` letters and
the arrow keys, which makes for a long hint. `direction_keys` picks one family:

```kdl
direction_keys "both"    // default — show everything that is bound
direction_keys "arrows"  // drop hjkl, keep ←↓↑→
direction_keys "letters" // drop the arrows, keep hjkl
```

The reduction only applies when **both** families are actually present on a
hint, so a binding that only has arrows (or only letters) is never emptied.

Accepted spellings: `arrows`/`arrow`, and `letters`/`hjkl`/`vim`. Anything
unrecognized falls back to `both`.

### Key order

Zellij reports a hint's keys in an arbitrary order — tab selection can arrive as
`271543689`. They are sorted into a predictable reading order by default;
`key_order` says which keyboard that order follows:

```kdl
key_order "qwerty"  // default
key_order "dvorak"
key_order "colemak"
key_order "abcdef"  // ignore the keyboard: 0-9 then a-z
key_order "none"    // leave keys as Zellij reports them
```

Within a hint, keys sort by kind so unlike keys never interleave:

| Kind | Order |
| --- | --- |
| Function keys | by number — `F1 F2 F3` |
| Digits | along the digit row — `1234567890`, so `0` comes last |
| Letters | by layout row, then left to right |
| Punctuation | likewise, but after all letters |
| Arrows | `hjkl` order — `←↓↑→` |
| Everything else | roughly physical, starting `Esc Tab Enter` |

Letters and punctuation are separate groups so they don't interleave at row
boundaries: `o p a s \` sorts to `opas\`, not `op\as`.

Modifiers sort ahead of the key itself, so keys sharing one stay in a single
run — a hint bound to both `hjkl` and `Ctrl hjkl` renders as `hjkl ^hjkl`, not
as an interleaved `h ^h j ^j`. Groups run unmodified first, then `Ctrl`,
`Super`, `Alt`, `Shift`; a key with several modifiers sorts with the strongest
it carries, so `Ctrl Shift p` lands in the `Ctrl` group.

Unmodified keys lead because they are the plainest way to reach the action, and
because a hint too wide to fit is [cut from the right](#what-gets-dropped) —
so whatever sorts first is what survives.

This only changes ordering. Which keys appear is decided by `direction_keys`
and the label settings, and is unaffected.

Note that letters sort by **physical position**, so `hjkl` reads in that order
on QWERTY but not on Dvorak or Colemak, where those letters sit elsewhere. If
you use a non-QWERTY layout but keep vim-style bindings on their QWERTY
positions, `key_order "qwerty"` is likely what you want.

#### Alphabetical

`key_order "abcdef"` ignores the keyboard entirely and sorts by the character
itself — digits `0`–`9`, then letters `a`–`z`:

```kdl
key_order "abcdef"
```

Useful when the hint is a list to scan rather than keys to reach for, and it is
layout-independent, so it reads the same on any keyboard.

Two differences from the layout modes:

- `0` comes **first**, not last — this is plain ascending order, not a walk
  along the digit row.
- `hjkl` happens to read in order here too, since `h < j < k < l`
  alphabetically, though for an unrelated reason.

Digits, letters and punctuation are still kept in separate groups, and arrows,
function keys and named keys are unaffected — those never had a layout position
to sort by.

Accepted spellings: `abcdef`, `alphabetical`, `alpha`, `abc`.

### Hint order

By default each mode shows its [curated list](#the-curated-list) first, in the
order the plugin builds it, followed by anything
[discovery](#discovered-hints) turned up.
`hint_order` overrides that with a comma-separated list of hint ids:

```kdl
hint_order "*, exit"          // exit always last
hint_order "new, close, *"    // these two first, rest unchanged
hint_order "new, *, quit"     // pin both ends
hint_order "pane, tab, move"  // explicit order, rest follow
```

The `*` marks where every hint you didn't name goes. Entries before it lead,
entries after it trail, and unlisted hints keep the order the mode built them.
Leaving `*` out is the same as ending with one.

The list applies to **every mode**, so an id that mode doesn't have is simply
ignored — `"*, exit"` puts `exit` last wherever it appears and changes nothing
elsewhere. Naming an id that doesn't exist at all is equally harmless, so a
typo degrades to no effect rather than an error.

Entries match a hint's **id or its label**, case-insensitively. Labels matter
for hints you have fused by giving them a shared label: those carry an internal
id like `=swap layout`, and naming them by label avoids having to write that.

`hint_order` does double duty: what you pin here is also what survives longest
when the window is too narrow to show everything. See
[What gets dropped](#what-gets-dropped).

This orders hints; [`key_order`](#key-order) orders the keys inside one.

## Fitting the bar

The hint line grows with the number of bindings a mode has, and can easily
exceed the terminal. By default the plugin fits it to the available width,
**dropping whole hints** rather than cutting one in half — you choose which
ones go, and they can be replaced by an indicator:

```kdl
auto_width      true // default — fit to the terminal
reserve_columns 0    // columns to leave for the rest of the bar
max_length      0    // hard cap; 0 = none
```

Width is learned from Zellij's pane geometry, since the plugin runs headless and
has no view of the status bar itself. It measures the right edge of the widest
visible tiled pane; floating and suppressed panes are ignored, as neither tracks
the terminal's real width. Until the first pane update arrives no width is
assumed, so nothing is cut on a guess.

`reserve_columns` is what keeps room for anything sharing the bar. The plugin
cannot see zjstatus's `format_right`, so if you have one, reserve roughly its
width:

```kdl
format_left  "{pipe_zjstatus_hints}"
format_right "{command_user}@{command_host}:{session}" // ≈30 columns
```

```kdl
reserve_columns 32
```

`max_length` acts as a hard cap alongside this. With both set the smaller wins,
so an explicit cap is never exceeded on a wide terminal. Set `auto_width false`
to ignore the terminal entirely and use the fixed cap alone.

### Glyph width

Fitting depends on measuring how many **columns** the hints occupy, which is not
the same as counting characters — a CJK ideograph or an emoji takes two.

Nerd Font glyphs are East Asian **Ambiguous**: one column by the Unicode
standard, but two in a terminal actually set up to display them. If you use them
in `key_alias_*` or `mod_alias_*` and the hints overflow slightly — fitting looks
right on wide windows and breaks near the edge — set:

```kdl
ambiguous_width 2
```

Getting this wrong under-counts every such glyph, so the plugin believes the
line fits while the terminal wraps or clips it.

### What gets dropped

[`hint_order`](#hint-order) decides what survives. Unpinned hints — the `*`, the
ones you never spoke for — are given up first, starting with the rightmost:

```kdl
hint_order "*, exit" // exit outlives the hints in the "*"
```

So on a narrowing window the middle thins out while `exit` stays put, rather
than `exit` being the first thing over the edge.

When the `*` runs out, dropping continues **outward from that same gap**: the
leading group is consumed from its inner edge, then the trailing group from
its inner edge. Hints given up therefore always form one contiguous run, so a
single indicator can stand for all of them:

```
new close split float frames pin exit   ← everything fits
new close split … exit                  ← the "*" thinning out
new … exit                              ← leading group being consumed
… exit                                  ← trailing group is last to go
```

Dropping from the far ends instead would open a second gap and need a second
indicator.

A single hint wider than the whole bar survives this and is cut with
`overflow_str` as a last resort.

#### Which pinned group goes first

`hint_precedence` names the pinned groups in the order they are held onto,
reading like zjstatus's `format_precedence`:

```kdl
hint_precedence "tl" // default — trailing kept, leading spent first
hint_precedence "lt" // leading kept, trailing spent first
```

With the default, a hint pinned to the end outlives one pinned to the front:

```kdl
hint_order      "new, *, exit"
hint_precedence "tl"
```

```
new … exit   ← the "*" is gone
… exit       ← "new" spent
exit         ← trailing group is last
```

Reverse it with `"lt"` if the hints you pin to the front are the ones you most
want kept. Either way the dropped hints stay one contiguous run, so a single
`drop_indicator` still covers them. The `*` is always spent first, whichever
precedence is set.

### Marking the gap

`drop_indicator` is rendered in place of the dropped run:

```kdl
drop_indicator "#[fg=$grey]…"
```

It is a format string like `hint_spacer`, so it can be styled, and it is spaced
like a hint so it reads as one. It costs columns of its own, which come out of
the same budget — a wide indicator forces one more hint to drop rather than
pushing the line over the limit.

Left unset, hints are dropped silently.

## The curated list

Hints come from two places, and most of this document refers to both. By default
you see only the first: the **curated list**, a small table built into the
plugin naming, for each of the main modes, the actions people reach for most —
in a deliberate order, with short hand-written labels.

It exists because a faithful list of your keybindings is not automatically a
readable one. Left to itself the plugin would show bindings in whatever order
Zellij reports them, labelled from action names — `switch_to_mode_pane` reading
as "switch to mode pane". The curated list is what makes Normal mode open as
`pane  tab  resize  move  scroll  session  quit` instead.

What it covers:

- **Normal, Pane, Tab, Resize, Move, Scroll, Search and Session** have entries.
  Other modes — Locked, Tmux, RenameTab and friends — have none, so they show
  only their escape hatch back to Normal unless
  [discovery](#discovered-hints) is on.
- **Only bindings you actually have.** An entry whose action is not bound in
  your config is skipped, so unbinding something removes its hint rather than
  leaving a dead one.
- **Grouped concepts that no single action describes** — the four resize
  directions as one `resize`, or Session mode's plugin launchers.
- **The way out of each mode.** Every key bound to `SwitchToMode "Normal"` —
  usually Enter and Esc both — forms a single `mode_normal` hint, so a mode
  always shows how to leave it even with discovery off.

Nothing about it is fixed: every entry is a normal hint with an
[id](#ids), so it can be relabelled with `label_<id>`, moved with
[`hint_order`](#hint-order), or hidden with an empty label.

## Discovered hints

The second source is **discovery**, off by default and enabled with:

```kdl
discover_hints true
```

With it on, every *other* keybinding enabled in the current mode is found and
appended after the curated list. Nothing your config binds is left out — custom
binds appear, and so do the many defaults the curated list omits (the `Alt-*`
quick keys, `lock` in Normal mode), along with modes like Locked and Tmux that
have no curated entries at all.

Keys resolving to the same [id](#labels) become a single hint, so the four
directional focus keys collapse into one `focus` rather than four entries.

Hints are collected before anything is drawn, so the two sources merge rather
than duplicate: a curated `focus` on `hjkl` and a discovered `focus` on the
arrows become one hint carrying all eight keys.

### Which to use

The curated list is the shorter, more readable default: a handful of hints per
mode, worded for scanning. Discovery is the complete one — it will not let a
binding go unmentioned, but a mode can easily produce more hints than a status
bar has room for, at which point [fitting](#fitting-the-bar) starts dropping
them again.

Turn it on if you want the hints to be an exhaustive reference for your config,
or while you are learning a keymap you just changed. Leave it off if you want a
compact bar that names the things you reach for.

Everything else works the same either way: discovered hints have
[ids](#ids) exactly like curated ones, so they relabel, reorder and hide
identically.

### Shared bindings

Zellij's default config binds the mode switches (`pane`, `tab`, `session`, …) in
every non-locked mode via `shared_except` groups. Discovering those in each mode
means every mode re-lists the same globals, which is mostly noise.

`hide_shared_hints` (default `true`) suppresses any discovered binding whose key
maps to the same action in the base mode, so each mode shows only what is new in
it. It has no effect while you are *in* the base mode — Normal still lists the
globals — and it respects a non-Normal `default_mode` such as `locked`.

Set it to `false` to have every mode list everything it accepts.

## Labels

Every hint is two things: the **keys** it shows, and the **label** — the words
printed next to them. This section is about changing those words.

### Ids and labels

Each hint has both a label and an **id**:

- The **label** is what you see — `split down`, `focus`, `exit`.
- The **id** is the hint's permanent name, which is never displayed —
  `split_down`, `focus`, `mode_normal`.

Most ids look like their label with underscores, which makes them easy to guess,
but the two do different jobs and keeping them apart is what makes everything
here work.

**The id is the config key.** `label_split_down "↓"` means *find the hint whose
id is `split_down`, and change its label to `↓`*. You can relabel a hint as often
as you like and its id never moves, so your config keeps working.

**The id is also the merge key.** Two bindings that resolve to the same id become
one hint showing all of their keys. This is why ids exist at all, rather than
just matching on the displayed text:

- Zellij binds focus-left, focus-down, focus-up and focus-right as four separate
  actions. All four resolve to the id `focus`, so they merge into a single
  `hjkl focus` hint instead of cluttering the bar with four.
- Pane mode's "new pane" and Tab mode's "new tab" both *display* `new`. Their ids
  differ (`new_pane`, `new_tab`), so they stay separate — and
  `label_new_tab "＋"` changes only the tab one.

Matching on displayed text would get the first case right and the second wrong.
Ids get both.

### Changing a label

Name the id, give it the text you want:

```kdl
label_split_down  "split ↓"
label_focus       "◆"
label_mode_locked "lock"
label_next_layout "»"
label_mouse       ""      // empty string hides this hint entirely
```

This works for **every** hint, whether it came from the
[curated list](#the-curated-list) or from [discovery](#discovered-hints).

### Finding a hint's id

Two cases:

1. **It is in the table below.** Those are the hints the plugin has hand-picked
   ids and labels for.
2. **It is not.** Then the id is the Zellij action's name in `snake_case`, and
   the label is that same name with the underscores turned into spaces. So a
   hint reading `next swap layout` has the id `next_swap_layout`, and
   `label_next_swap_layout "»"` renames it.

That second rule is why nothing is ever unlabelled and every hint is
addressable, including bindings this plugin has never heard of.

### Ids

The hints with hand-picked ids, and the label each shows unless you change it.
Rows are grouped roughly by the mode they turn up in. Where the label column says
"same", the label is the id with spaces instead of underscores.

| Id | Default label |
|---|---|
| `mode_normal`, `mode_locked`, `mode_pane`, `mode_tab`, `mode_resize`, `mode_move`, `mode_scroll`, `mode_search`, `mode_session`, `mode_rename`, `mode_tmux`, `mode_prompt` | the mode name (`lock` for locked) |
| `new_pane`, `close_pane`, `split_left`, `split_right`, `split_up`, `split_down` | `new`, `close`, `split …` |
| `stacked_pane`, `floating_pane`, `in_place_pane` | `stacked`, `floating`, `in place` |
| `fullscreen`, `float`, `embed`, `frames`, `pin` | same |
| `group_pane`, `group_marking` | `group`, `mark` |
| `focus`, `move_pane`, `move_pane_back`, `next_pane`, `prev_pane`, `toggle_focus` | `focus`, `move`, `move back`, `next`, `prev`, `toggle focus` |
| `new_tab`, `close_tab`, `next_tab`, `prev_tab`, `go_to_tab`, `toggle_tab`, `sync`, `move_tab` | `new`, `close`, `next`, `prev`, `tab`, `toggle`, `sync`, `move tab` |
| `break_pane`, `break_left`, `break_right` | `break …` |
| `resize`, `increase`, `decrease` | same |
| `scroll`, `page`, `half_page`, `top`, `bottom`, `edit` | same (`half page`) |
| `search_down`, `search_up`, `search_toggle` | `down`, `up`, `toggle` |
| `copy`, `clear`, `mouse`, `prev_layout`, `next_layout`, `confirm`, `deny`, `quit`, `detach`, `undo_rename`, `rename_session` | same (`prev layout`, `next layout`, `undo`, `rename`) |
| `rename`, `manager`, `config`, `plugins`, `about`, `share` | same |
| `layout_manager` | `layouts` |

Anything not listed falls back to the rule above: the action's own snake_case
name serves as both id and label.

### By action name

A hint can also be addressed by the **Zellij action** behind it, using that
action's snake_case name with any direction appended. These two are the same
hint:

```kdl
label_split_down    "↓"   // by id
label_new_pane_down "↓"   // by the action, NewPane "Down"
```

More examples: `label_move_focus_left`, `label_resize_increase_left`,
`label_switch_to_mode_locked`.

Ids are usually the better choice — they are shorter, and some hints are built
from several actions at once (`resize` covers four, `mode_normal` gathers every
key that leaves the mode), so they have no single action name to use. Reach for
the action form when you already know the Zellij action and would rather not
look one up.

If both are set for the same hint, the id wins.

### Per-mode labels

Prefixing a mode name scopes a label to that mode alone:

```kdl
label_mode_normal        "exit"   // every mode
label_locked_mode_normal "unlock" // …except Locked, which says "unlock"
```

Useful where one action means different things in different modes. Leaving
Locked is an unlock; leaving Pane mode is just an exit.

Mode names are the lowercased Zellij modes — `normal`, `locked`, `pane`, `tab`,
`resize`, `move`, `scroll`, `search`, `session`, `renametab`, `renamepane`,
`tmux`, `prompt` — and the suffix is an id or an action name, both of which work
scoped:

```kdl
label_locked_switch_to_mode_normal "unlock" // same thing, by action
```

Lookup runs most specific first, so a mode-scoped label always beats a global
one:

1. `label_<mode>_<id>`
2. `label_<mode>_<action>`
3. `label_<id>`
4. `label_<action>`

An empty value still hides, at whichever scope you set it — so
`label_pane_mode_normal ""` drops the hint in Pane mode and leaves it elsewhere.
The reverse works too: hide globally with `label_mode_normal ""`, then bring it
back in one mode with `label_locked_mode_normal "unlock"`.

### Combining hints

Hints normally merge only when they share an [id](#ids-and-labels). There is one
deliberate exception: giving two hints the **same label yourself** merges them
too, gathering all their keys into one hint.

```kdl
label_next_layout "swap layout"
label_prev_layout "swap layout"   // one hint, both keys
```

Next-layout and previous-layout are separate actions with separate ids, so by
default they are two hints. Labelling both `swap layout` says *treat these as one
thing*, and they collapse into a single hint carrying both keys.

This applies only to labels **you** set. Hints that merely ship with the same
built-in label — Pane's `new` and Tab's `new` — stay separate, so a merge is
always something you asked for rather than an accident of the default table.

One consequence worth knowing: a merged hint's id becomes the label you chose,
so [`hint_order`](#hint-order) should refer to it by that label.

## TODO

- [x] configurable colors/formatting
- [x] more advanced mode-specific configuration
- [x] improved handling of long outputs
- [x] ability to enable/disable specific hints
- [ ] replace `ansi_term`, which has been unmaintained since 2021
      (RUSTSEC-2021-0139). It is a direct dependency and the only advisory
      warning this project owns — the rest come in through `zellij-tile`.
      `nu-ansi-term` is a maintained fork of the same API; `anstyle` is the
      more modern choice but a larger change.
- [x] reconsider the `select` hint — dropped. It labelled Enter separately in
      seven modes, but Enter and Esc are bound to the identical
      `SwitchToMode "Normal"`, so it spent a hint on a distinction Zellij does
      not make. Both keys now form one `mode_normal` hint.

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md)
for the development loop, and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for
expected conduct. Security problems should go through
[SECURITY.md](SECURITY.md) rather than a public issue.

How this repository builds, tests and releases itself is described in
[docs/AUTOMATION.md](docs/AUTOMATION.md).

## License

&copy; 2026 Myah Mitchell
&copy; 2025 Maddison Cohodas

A fork of [zjstatus-hints](https://github.com/b0o/zjstatus-hints) by Maddison
Cohodas, itself adapted from the built-in Zellij status-bar plugin by
Brooks J Rady.

MIT License
