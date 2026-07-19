use ansi_term::{
    ANSIString, ANSIStrings,
    Colour::{Fixed, RGB},
    Style,
};
use std::collections::{BTreeMap, BTreeSet};
use unicode_width::UnicodeWidthChar;
use zellij_tile::prelude::actions::Action;
use zellij_tile::prelude::actions::SearchDirection;
use zellij_tile::prelude::*;
use zellij_tile_utils::palette_match;

mod format;

#[derive(Default)]
struct State {
    initialized: bool,
    pipe_name: String,
    mode_info: ModeInfo,
    base_mode_is_locked: bool,
    max_length: usize,
    auto_width: bool,
    reserve_columns: usize,
    /// Whether East Asian Ambiguous characters occupy two columns. Nerd Font
    /// glyphs are Ambiguous, and most terminals showing them render two.
    wide_ambiguous: bool,
    /// Terminal width in columns, learned from `PaneUpdate`. `None` until the
    /// first one arrives, when auto-fitting stays off rather than guessing.
    terminal_width: Option<usize>,
    overflow_str: String,
    hide_in_base_mode: bool,
    key_format: Option<String>,
    desc_format: Option<String>,
    hint_spacer: Option<String>,
    drop_indicator: Option<String>,
    discover_hints: bool,
    direction_keys: DirectionKeys,
    key_order: KeyOrder,
    hint_order: HintOrder,
    hint_precedence: HintPrecedence,
    hide_shared_hints: bool,
    config: BTreeMap<String, String>,
}

register_plugin!(State);

const TO_NORMAL: Action = Action::SwitchToMode {
    input_mode: InputMode::Normal,
};

/// Plugins with a curated Session-mode hint, as `(plugin name, id, label)`.
///
/// Every entry here is launched by a `LaunchOrFocusPlugin` binding, and those all
/// share one action signature — so without a curated id they would discover as a
/// single fused hint carrying every launcher key. Listing them gives each its own
/// concept id and a label worth reading.
const SESSION_PLUGINS: &[(&str, &str, &str)] = &[
    ("session-manager", "manager", "manager"),
    ("configuration", "config", "config"),
    ("plugin-manager", "plugins", "plugins"),
    ("zellij:about", "about", "about"),
    ("zellij:share", "share", "share"),
    ("zellij:layout-manager", "layout_manager", "layouts"),
];

/// The terminal's width, taken as the right edge of the widest visible pane.
///
/// Tiled panes tile the whole terminal, so the largest `pane_x + pane_columns`
/// among them is its width. Two kinds are skipped:
///
/// - **Floating** panes sit within the terminal but outside the tiling, so
///   their edge says nothing about its width.
/// - **Suppressed** panes are not on screen and stop tracking resizes. Their
///   geometry is whatever it was when they were hidden, and being stale it is
///   often the largest — which would pin the measurement to an old width and
///   silently stop the hints from ever being refitted.
fn terminal_width(manifest: &PaneManifest) -> Option<usize> {
    manifest
        .panes
        .values()
        .flatten()
        .filter(|pane| !pane.is_floating && !pane.is_suppressed)
        .map(|pane| pane.pane_x + pane.pane_columns)
        .max()
        .filter(|width| *width > 0)
}

/// SGR reset. Terminates a truncated styled run so its colours stop at the cut.
const ANSI_RESET: &str = "\u{1b}[0m";

const DEFAULT_MAX_LENGTH: usize = 0;
const DEFAULT_OVERFLOW_STR: &str = "...";
const DEFAULT_PIPE_NAME: &str = "zjstatus_hints";

const CONFIG_KEY_FORMAT: &str = "key_format";
const CONFIG_DESC_FORMAT: &str = "desc_format";
const CONFIG_HINT_SPACER: &str = "hint_spacer";
const CONFIG_KEY_ALIAS_PREFIX: &str = "key_alias_";
const CONFIG_MOD_ALIAS_PREFIX: &str = "mod_alias_";
const CONFIG_LABEL_PREFIX: &str = "label_";
const CONFIG_DISCOVER_HINTS: &str = "discover_hints";
const CONFIG_DIRECTION_KEYS: &str = "direction_keys";
const CONFIG_KEY_ORDER: &str = "key_order";
const CONFIG_HINT_ORDER: &str = "hint_order";
const CONFIG_DROP_INDICATOR: &str = "drop_indicator";
const CONFIG_HINT_PRECEDENCE: &str = "hint_precedence";
const CONFIG_AUTO_WIDTH: &str = "auto_width";
const CONFIG_RESERVE_COLUMNS: &str = "reserve_columns";
const CONFIG_AMBIGUOUS_WIDTH: &str = "ambiguous_width";
const CONFIG_HIDE_SHARED_HINTS: &str = "hide_shared_hints";

// The curated list alone is the readable default; discovery is comprehensive but
// long, and on a narrow bar the extra hints are the first to be dropped anyway.
const DEFAULT_DISCOVER_HINTS: bool = false;
const DEFAULT_HIDE_SHARED_HINTS: bool = true;

type ActionLabel = (Action, &'static str);
type ActionSequenceLabel = (&'static [Action], &'static str);

/// Which keys to show when a hint is bound to both the `hjkl` letters and the
/// arrow keys (e.g. move/focus). `Both` (the default) keeps the existing
/// behavior; `Arrows`/`Letters` drop the other family so the hint is shorter.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum DirectionKeys {
    #[default]
    Both,
    Arrows,
    Letters,
}

impl DirectionKeys {
    fn from_config(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "arrows" | "arrow" => Self::Arrows,
            "letters" | "hjkl" | "vim" => Self::Letters,
            _ => Self::Both,
        }
    }
}

/// Physical key order used to sort the keys within a single hint, so a hint
/// bound to several keys reads in the order they sit under your hands rather
/// than the order Zellij happens to report them.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum KeyOrder {
    #[default]
    Qwerty,
    Dvorak,
    Colemak,
    /// Ignore the keyboard entirely: digits `0-9`, then letters `a-z`.
    Alphabetical,
    /// Leave keys exactly as Zellij reports them.
    Unsorted,
}

impl KeyOrder {
    fn from_config(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "dvorak" => Self::Dvorak,
            "colemak" => Self::Colemak,
            "abcdef" | "alphabetical" | "alpha" | "abc" => Self::Alphabetical,
            "none" | "off" | "unsorted" => Self::Unsorted,
            _ => Self::Qwerty,
        }
    }

    /// The rows of this layout, top to bottom, each listed left to right and
    /// including the digit row. Position within these rows is the whole of the
    /// ordering — digits included, which is what puts `0` after `9`.
    fn rows(self) -> &'static [&'static str] {
        match self {
            Self::Qwerty => &[
                "1234567890-=",
                "qwertyuiop[]\\",
                "asdfghjkl;'",
                "zxcvbnm,./",
            ],
            Self::Dvorak => &[
                "1234567890[]",
                "',.pyfgcrl/=\\",
                "aoeuidhtns-",
                ";qjkxbmwvz",
            ],
            Self::Colemak => &[
                "1234567890-=",
                "qwfpgjluy;[]\\",
                "arstdhneio'",
                "zxcvbkm,./",
            ],
            Self::Alphabetical | Self::Unsorted => &[],
        }
    }
}

/// A user-supplied ordering for the hints within a mode, parsed from a
/// comma-separated `hint_order` list.
///
/// A `*` in the list marks where everything unlisted goes, so entries before it
/// are pinned to the front and entries after it to the back. Omitting `*` is the
/// same as ending with one: the list becomes the leading order and everything
/// else follows.
#[derive(Clone, Default)]
struct HintOrder {
    first: Vec<String>,
    last: Vec<String>,
}

impl HintOrder {
    fn from_config(value: &str) -> Self {
        let (mut first, mut last) = (vec![], vec![]);
        let mut after_wildcard = false;
        for entry in value.split(',') {
            match entry.trim() {
                "" => continue,
                "*" => after_wildcard = true,
                entry if after_wildcard => last.push(entry.to_lowercase()),
                entry => first.push(entry.to_lowercase()),
            }
        }
        Self { first, last }
    }

    fn is_empty(&self) -> bool {
        self.first.is_empty() && self.last.is_empty()
    }

    /// Which group a hint belongs to: `Leading`, `Middle`, or `Trailing`. This
    /// is what decides the order hints are given up in when space runs short.
    fn group(&self, hint: &Hint) -> HintGroup {
        if Self::position(&self.first, hint).is_some() {
            HintGroup::Leading
        } else if Self::position(&self.last, hint).is_some() {
            HintGroup::Trailing
        } else {
            HintGroup::Middle
        }
    }

    /// Sort key placing a hint in the leading, middle, or trailing group. Every
    /// unlisted hint shares one key, so a stable sort leaves them in the order
    /// the mode built them.
    fn rank(&self, hint: &Hint) -> (u8, usize) {
        if let Some(index) = Self::position(&self.first, hint) {
            return (0, index);
        }
        if let Some(index) = Self::position(&self.last, hint) {
            return (2, index);
        }
        (1, 0)
    }

    /// Entries name a hint by concept id or by the label it displays. Matching
    /// the label too is what keeps relabeled hints addressable — a hint fused by
    /// a shared label has the internal id `=swap layout`, which no one wants to
    /// type.
    fn position(entries: &[String], hint: &Hint) -> Option<usize> {
        let id = hint.id.to_lowercase();
        let label = hint.label.to_lowercase();
        entries
            .iter()
            .position(|entry| *entry == id || *entry == label)
    }
}

/// Which pinned group has precedence — is kept longest — when hints must be
/// given up. Named for zjstatus's `format_precedence`, and read the same way:
/// the groups in the order they are held onto.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum HintPrecedence {
    /// `"tl"` — the trailing group outlives the leading one.
    #[default]
    Trailing,
    /// `"lt"` — the leading group outlives the trailing one.
    Leading,
}

impl HintPrecedence {
    fn from_config(value: &str) -> Self {
        match value.to_lowercase().trim() {
            "lt" => Self::Leading,
            _ => Self::Trailing,
        }
    }
}

/// How each hint should be styled while rendering. Borrows from `State` so the
/// styling functions can fall back to the theme palette when no format string
/// is configured, and resolve `$alias` colors from the plugin configuration.
struct HintStyle<'a> {
    colors: &'a Styling,
    key_format: Option<&'a str>,
    desc_format: Option<&'a str>,
    /// Rendered between consecutive hints (never before the first or after the
    /// last). `None` when unset, leaving hints adjacent as before.
    spacer: Option<&'a str>,
    discover: bool,
    direction_keys: DirectionKeys,
    key_order: KeyOrder,
    /// The mode being rendered, lowercased, for mode-scoped `label_` lookups.
    mode: &'a str,
    /// User-chosen ordering for the hints within a mode; empty leaves the order
    /// each mode builds.
    hint_order: &'a HintOrder,
    /// Columns available for the hints, or `None` when unbounded. Whole hints
    /// are dropped to fit rather than the line being cut mid-hint.
    limit: Option<usize>,
    /// See `State::wide_ambiguous`.
    wide_ambiguous: bool,
    /// Rendered in place of hints dropped for lack of room. `None` leaves the
    /// gap unmarked.
    drop_indicator: Option<&'a str>,
    /// Which pinned group outlives the other when space runs short.
    precedence: HintPrecedence,
    /// Base-mode keybindings, used to suppress globals that every mode inherits.
    /// Empty when `hide_shared_hints` is off or when rendering the base mode.
    shared: &'a [(KeyWithModifier, Vec<Action>)],
    config: &'a BTreeMap<String, String>,
}

const NORMAL_MODE_ACTIONS: &[ActionLabel] = &[
    (
        Action::SwitchToMode {
            input_mode: InputMode::Pane,
        },
        "pane",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Tab,
        },
        "tab",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Resize,
        },
        "resize",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Move,
        },
        "move",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Scroll,
        },
        "scroll",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Search,
        },
        "search",
    ),
    (
        Action::SwitchToMode {
            input_mode: InputMode::Session,
        },
        "session",
    ),
    (Action::Quit, "quit"),
];

const PANE_MODE_ACTION_SEQUENCES: &[ActionSequenceLabel] = &[
    (
        &[
            Action::NewPane {
                direction: None,
                pane_name: None,
                start_suppressed: false,
            },
            TO_NORMAL,
        ],
        "new",
    ),
    (&[Action::CloseFocus, TO_NORMAL], "close"),
    (&[Action::ToggleFocusFullscreen, TO_NORMAL], "fullscreen"),
    (&[Action::ToggleFloatingPanes, TO_NORMAL], "float"),
    (&[Action::TogglePaneEmbedOrFloating, TO_NORMAL], "embed"),
    (
        &[
            Action::NewPane {
                direction: Some(Direction::Right),
                pane_name: None,
                start_suppressed: false,
            },
            TO_NORMAL,
        ],
        "split right",
    ),
    (
        &[
            Action::NewPane {
                direction: Some(Direction::Down),
                pane_name: None,
                start_suppressed: false,
            },
            TO_NORMAL,
        ],
        "split down",
    ),
];

const TAB_MODE_ACTION_SEQUENCES: &[ActionSequenceLabel] = &[
    (
        &[
            Action::NewTab {
                tiled_layout: None,
                floating_layouts: vec![],
                swap_tiled_layouts: None,
                swap_floating_layouts: None,
                tab_name: None,
                should_change_focus_to_new_tab: true,
                cwd: None,
                initial_panes: None,
                first_pane_unblock_condition: None,
            },
            TO_NORMAL,
        ],
        "new",
    ),
    (&[Action::CloseTab, TO_NORMAL], "close"),
    (&[Action::BreakPane, TO_NORMAL], "break pane"),
    (&[Action::ToggleActiveSyncTab, TO_NORMAL], "sync"),
];

impl State {
    /// How wide the hint line may be, or `None` for no limit.
    ///
    /// `max_length` is a hard cap the user set; auto-fitting derives one from
    /// the terminal. With both in play the smaller wins, so an explicit cap is
    /// never exceeded just because the window is wide.
    fn length_limit(&self) -> Option<usize> {
        let explicit = (self.max_length > 0).then_some(self.max_length);
        let auto = self
            .auto_width
            .then_some(self.terminal_width)
            .flatten()
            .map(|width| width.saturating_sub(self.reserve_columns));
        match (explicit, auto) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (limit, None) | (None, limit) => limit,
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.initialized = false;

        // TODO: configuration validation
        self.max_length = configuration
            .get("max_length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_LENGTH);
        self.overflow_str = configuration
            .get("overflow_str")
            .cloned()
            .unwrap_or_else(|| DEFAULT_OVERFLOW_STR.to_string());
        self.pipe_name = configuration
            .get("pipe_name")
            .cloned()
            .unwrap_or_else(|| DEFAULT_PIPE_NAME.to_string());
        self.hide_in_base_mode = configuration
            .get("hide_in_base_mode")
            .map(|s| s.to_lowercase().parse::<bool>().unwrap_or(false))
            .unwrap_or(false);
        // Fit the hints to the terminal, dropping the trailing ones as it
        // narrows instead of overflowing off the edge.
        self.auto_width = configuration
            .get(CONFIG_AUTO_WIDTH)
            .map(|s| s.to_lowercase().parse::<bool>().unwrap_or(true))
            .unwrap_or(true);
        // Columns to leave free for whatever else shares the bar — zjstatus's
        // `format_right`, typically, which the plugin cannot see.
        self.reserve_columns = configuration
            .get(CONFIG_RESERVE_COLUMNS)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        // Nerd Font glyphs are East Asian Ambiguous. A terminal set up to show
        // them draws two columns, so measuring them as one makes the plugin
        // think the line fits when it overflows.
        self.wide_ambiguous = configuration
            .get(CONFIG_AMBIGUOUS_WIDTH)
            .map(|s| s.trim() == "2")
            .unwrap_or(false);
        // Optional zjstatus-style format strings for the key and description
        // parts of each hint. When unset, the theme palette is used (see
        // `style_key_with_modifier` / `style_description`). An empty string is
        // treated as unset so the default styling still applies.
        self.key_format = configuration
            .get(CONFIG_KEY_FORMAT)
            .filter(|s| !s.is_empty())
            .cloned();
        self.desc_format = configuration
            .get(CONFIG_DESC_FORMAT)
            .filter(|s| !s.is_empty())
            .cloned();
        // Separator drawn between hints. Also a format string, so it can be a
        // styled glyph (`#[fg=$grey] | `) and not just whitespace.
        self.hint_spacer = configuration
            .get(CONFIG_HINT_SPACER)
            .filter(|s| !s.is_empty())
            .cloned();
        // Marks where hints were dropped to fit the window. Also a format
        // string, so it can be a styled glyph rather than bare text.
        self.drop_indicator = configuration
            .get(CONFIG_DROP_INDICATOR)
            .filter(|s| !s.is_empty())
            .cloned();
        // When enabled (the default), every enabled keybinding in a mode is
        // shown, not just the curated set. See `add_discovered_hints`.
        self.discover_hints = configuration
            .get(CONFIG_DISCOVER_HINTS)
            .map(|s| {
                s.to_lowercase()
                    .parse::<bool>()
                    .unwrap_or(DEFAULT_DISCOVER_HINTS)
            })
            .unwrap_or(DEFAULT_DISCOVER_HINTS);
        // When a hint is bound to both hjkl and the arrow keys, optionally show
        // only one family (`arrows` / `letters`); defaults to showing both.
        self.direction_keys = configuration
            .get(CONFIG_DIRECTION_KEYS)
            .map(|s| DirectionKeys::from_config(s))
            .unwrap_or_default();
        // Keyboard layout used to order the keys within a hint. Only affects
        // ordering, never which keys are shown.
        self.key_order = configuration
            .get(CONFIG_KEY_ORDER)
            .map(|s| KeyOrder::from_config(s))
            .unwrap_or_default();
        // Pin chosen hints to the start or end of each mode, around a `*` that
        // stands for everything left unlisted.
        self.hint_order = configuration
            .get(CONFIG_HINT_ORDER)
            .map(|s| HintOrder::from_config(s))
            .unwrap_or_default();
        // Which pinned group is held onto longest once the `*` is exhausted.
        self.hint_precedence = configuration
            .get(CONFIG_HINT_PRECEDENCE)
            .map(|s| HintPrecedence::from_config(s))
            .unwrap_or_default();
        // Suppress bindings every mode inherits from the base mode, so each mode
        // only advertises what is actually new in it.
        self.hide_shared_hints = configuration
            .get(CONFIG_HIDE_SHARED_HINTS)
            .map(|s| {
                s.to_lowercase()
                    .parse::<bool>()
                    .unwrap_or(DEFAULT_HIDE_SHARED_HINTS)
            })
            .unwrap_or(DEFAULT_HIDE_SHARED_HINTS);
        // Retained so `$alias` colors and `label_<action>` overrides can be
        // resolved from the raw configuration.
        self.config = configuration;

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::MessageAndLaunchOtherPlugins,
        ]);

        set_selectable(false);
        subscribe(&[
            EventType::ModeUpdate,
            EventType::SessionUpdate,
            EventType::PaneUpdate,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = !self.initialized;
        match event {
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
                self.base_mode_is_locked = self.mode_info.base_mode == Some(InputMode::Locked);
            }
            // The plugin runs headless, so its own `render` dimensions say
            // nothing about the status bar. Pane geometry is the only view of
            // the terminal's width it gets.
            Event::PaneUpdate(manifest) => {
                let width = terminal_width(&manifest);
                if width.is_some() && width != self.terminal_width {
                    self.terminal_width = width;
                    should_render = true;
                }
            }
            _ => {}
        };
        should_render
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        let mode_info = &self.mode_info;
        let output = if !(self.hide_in_base_mode && Some(mode_info.mode) == mode_info.base_mode) {
            let keymap = get_keymap_for_mode(mode_info);
            // Bindings the base mode already advertises. Every non-base mode
            // inherits these via Zellij's `shared_except` groups, so listing
            // them again in each mode is pure repetition.
            let base_mode = mode_info.base_mode.unwrap_or(InputMode::Normal);
            let shared = if self.hide_shared_hints && mode_info.mode != base_mode {
                mode_info.get_keybinds_for_mode(base_mode)
            } else {
                vec![]
            };
            let mode_key = format!("{:?}", mode_info.mode).to_lowercase();
            let limit = self.length_limit();
            let ctx = HintStyle {
                colors: &mode_info.style.colors,
                key_format: self.key_format.as_deref(),
                desc_format: self.desc_format.as_deref(),
                spacer: self.hint_spacer.as_deref(),
                discover: self.discover_hints,
                direction_keys: self.direction_keys,
                key_order: self.key_order,
                mode: &mode_key,
                hint_order: &self.hint_order,
                // `render` prefixes a space, so the hints themselves get one
                // column less than the limit.
                limit: limit.map(|limit| limit.saturating_sub(1)),
                wide_ambiguous: self.wide_ambiguous,
                drop_indicator: self.drop_indicator.as_deref(),
                precedence: self.hint_precedence,
                shared: &shared,
                config: &self.config,
            };
            let parts = render_hints_for_mode(mode_info.mode, &keymap, &ctx);

            let ansi_strings = ANSIStrings(&parts);
            let formatted = format!(" {}", ansi_strings);

            let visible_len = calculate_visible_length(&formatted, self.wide_ambiguous);
            match limit {
                Some(limit) if visible_len > limit => {
                    truncate_ansi_string(&formatted, &self.overflow_str, limit, self.wide_ambiguous)
                }
                _ => formatted.to_string(),
            }
        } else {
            String::new()
        };

        // HACK: Because we're not sure when zjstatus will be ready to receive messages,
        // we'll repeatedly send messages until the user has switched to a different mode,
        // at which point we'll assume that zjstatus has been initialized. The render function
        // does not seem to be called too frequently, so this should be fine.
        if !output.is_empty() && Some(mode_info.mode) != mode_info.base_mode {
            self.initialized = true;
        }

        pipe_message_to_plugin(MessageToPlugin::new("pipe").with_payload(format!(
            "zjstatus::pipe::pipe_{}::{}",
            self.pipe_name, output
        )));
        print!("{}", output);
    }
}

struct AnsiParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> AnsiParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.chars().peekable(),
        }
    }

    fn next_segment(&mut self) -> Option<AnsiSegment> {
        let ch = self.chars.next()?;

        if ch == '\x1b' {
            let mut escape_seq = String::from(ch);
            for escape_ch in self.chars.by_ref() {
                escape_seq.push(escape_ch);
                if escape_ch == 'm' {
                    break;
                }
            }
            Some(AnsiSegment::EscapeSequence(escape_seq))
        } else {
            Some(AnsiSegment::VisibleChar(ch))
        }
    }
}

enum AnsiSegment {
    EscapeSequence(String),
    VisibleChar(char),
}

/// Columns the text occupies on screen, ignoring escape sequences.
///
/// Counting characters is not the same as counting columns: a CJK ideograph or
/// an emoji takes two. `wide_ambiguous` decides the East Asian Ambiguous class,
/// which includes every Nerd Font glyph — narrow by the standard, but two
/// columns in a terminal actually configured to show them.
fn calculate_visible_length(text: &str, wide_ambiguous: bool) -> usize {
    let mut parser = AnsiParser::new(text);
    let mut len = 0;

    while let Some(segment) = parser.next_segment() {
        if let AnsiSegment::VisibleChar(ch) = segment {
            len += char_width(ch, wide_ambiguous);
        }
    }

    len
}

fn char_width(ch: char, wide_ambiguous: bool) -> usize {
    let width = if wide_ambiguous {
        ch.width_cjk()
    } else {
        ch.width()
    };
    // Control characters report no width; treat them as taking none.
    width.unwrap_or(0)
}

fn truncate_ansi_string(
    text: &str,
    overflow_str: &str,
    max_len: usize,
    wide_ambiguous: bool,
) -> String {
    let visible_len = calculate_visible_length(text, wide_ambiguous);
    // Width on screen, not bytes: an overflow marker like `…` is one column.
    let overflow_len = overflow_str.chars().count();

    if visible_len <= max_len {
        return text.to_string();
    }

    // Too narrow for even the marker, so show as much of it as fits rather than
    // overshooting the limit the caller asked for.
    if max_len <= overflow_len {
        return overflow_str.chars().take(max_len).collect();
    }

    let target_len = max_len - overflow_len;
    let mut result = String::new();
    let mut visible_count = 0;
    let mut parser = AnsiParser::new(text);

    while let Some(segment) = parser.next_segment() {
        match segment {
            AnsiSegment::EscapeSequence(seq) => {
                result.push_str(&seq);
            }
            AnsiSegment::VisibleChar(ch) => {
                if visible_count >= target_len {
                    break;
                }
                result.push(ch);
                visible_count += 1;
            }
        }
    }

    result.push_str(overflow_str);
    // `ANSIStrings` closes the styled run with a reset, which the loop above
    // discards by breaking early. Without one, whatever colour was active at the
    // cut keeps painting the rest of the status bar.
    if !result.ends_with(ANSI_RESET) {
        result.push_str(ANSI_RESET);
    }
    result
}

fn find_keys_for_actions(
    keymap: &[(KeyWithModifier, Vec<Action>)],
    target_actions: &[Action],
    exact_match: bool,
) -> Vec<KeyWithModifier> {
    keymap
        .iter()
        .filter_map(|(key, key_actions)| {
            if exact_match {
                let matching = key_actions
                    .iter()
                    .zip(target_actions)
                    .filter(|(a, b)| a.shallow_eq(b))
                    .count();
                if matching == key_actions.len() && matching == target_actions.len() {
                    Some(key.clone())
                } else {
                    None
                }
            } else if key_actions.iter().next() == target_actions.iter().next() {
                Some(key.clone())
            } else {
                None
            }
        })
        .collect()
}

fn find_keys_for_action_groups(
    keymap: &[(KeyWithModifier, Vec<Action>)],
    action_groups: &[&[Action]],
) -> Vec<KeyWithModifier> {
    action_groups
        .iter()
        .flat_map(|actions| find_keys_for_actions(keymap, actions, true))
        .collect()
}

/// A run of keys that share the exact same set of modifiers, so the modifier
/// can be factored out and shown once (e.g. `Ctrl+h` and `Ctrl+k` collapse to
/// `^h|k`). `modifiers` is in canonical order (Ctrl, Alt, Shift, Super) since
/// `key_modifiers` is a `BTreeSet`.
struct KeyGroup {
    modifiers: Vec<KeyModifier>,
    keys: Vec<String>,
}

/// Partition a hint's key bindings into groups sharing the same modifiers,
/// preserving first-seen order of both groups and keys. This is what lets
/// `Ctrl h|Alt <|Ctrl k|Alt l` collapse to `^h|k ⌥<|l`.
fn group_keys(
    key_bindings: &[KeyWithModifier],
    config: &BTreeMap<String, String>,
) -> Vec<KeyGroup> {
    let mut groups: Vec<KeyGroup> = vec![];
    for key in key_bindings {
        let modifiers: Vec<KeyModifier> = key.key_modifiers.iter().copied().collect();
        let display = format_bare_key(&key.bare_key, config);
        if let Some(group) = groups.iter_mut().find(|g| g.modifiers == modifiers) {
            group.keys.push(display);
        } else {
            groups.push(KeyGroup {
                modifiers,
                keys: vec![display],
            });
        }
    }
    groups
}

/// Render a modifier alias for display, e.g. `Ctrl` -> `^` when
/// `mod_alias_ctrl "^"` is configured. Without an alias, the modifier's normal
/// name is used (`Ctrl`, `Alt`, `Shift`, `Super`).
fn modifier_display(modifier: KeyModifier, config: &BTreeMap<String, String>) -> String {
    let name = modifier.to_string();
    config
        .get(&format!(
            "{}{}",
            CONFIG_MOD_ALIAS_PREFIX,
            name.to_lowercase()
        ))
        .cloned()
        .unwrap_or(name)
}

/// The prefix shown before a group's keys, including its trailing separator. A
/// word-like modifier (`Ctrl`) is followed by a space (`Ctrl h`); a symbol
/// alias (`^`) hugs the keys (`^h`). An empty modifier set yields no prefix.
fn group_prefix(modifiers: &[KeyModifier], config: &BTreeMap<String, String>) -> String {
    if modifiers.is_empty() {
        return String::new();
    }
    let joined: String = modifiers
        .iter()
        .map(|m| modifier_display(*m, config))
        .collect();
    if joined
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric())
    {
        format!("{} ", joined)
    } else {
        joined
    }
}

/// Render a bare key, substituting a configured alias symbol when one is set.
/// Without an alias this is exactly the key's normal Zellij representation
/// (e.g. `ENTER`, `ESC`, `←`), so behavior is unchanged unless configured.
fn format_bare_key(bare_key: &BareKey, config: &BTreeMap<String, String>) -> String {
    key_alias(bare_key, config).unwrap_or_else(|| bare_key.to_string())
}

/// Look up a configured symbol for `bare_key`. Aliases are set via
/// `key_alias_<name>` options (mirroring zjstatus's `color_<name>` aliases),
/// where `<name>` is a lowercase key name, e.g. `key_alias_enter "↵"`. Several
/// common keys accept a short synonym (e.g. `esc`/`escape`); the first name
/// with a configured value wins.
fn key_alias(bare_key: &BareKey, config: &BTreeMap<String, String>) -> Option<String> {
    key_alias_names(bare_key).into_iter().find_map(|name| {
        config
            .get(&format!("{}{}", CONFIG_KEY_ALIAS_PREFIX, name))
            .cloned()
    })
}

/// The accepted `key_alias_<name>` names for a bare key, in priority order.
fn key_alias_names(bare_key: &BareKey) -> Vec<String> {
    let names: &[&str] = match bare_key {
        BareKey::Enter => &["enter", "return"],
        BareKey::Esc => &["esc", "escape"],
        BareKey::Tab => &["tab"],
        BareKey::Char(' ') => &["space"],
        BareKey::Backspace => &["backspace"],
        BareKey::Delete => &["delete", "del"],
        BareKey::Insert => &["insert", "ins"],
        BareKey::Home => &["home"],
        BareKey::End => &["end"],
        BareKey::PageUp => &["pageup", "pgup"],
        BareKey::PageDown => &["pagedown", "pgdn"],
        BareKey::Up => &["up"],
        BareKey::Down => &["down"],
        BareKey::Left => &["left"],
        BareKey::Right => &["right"],
        BareKey::CapsLock => &["capslock"],
        BareKey::ScrollLock => &["scrolllock"],
        BareKey::NumLock => &["numlock"],
        BareKey::PrintScreen => &["printscreen"],
        BareKey::Pause => &["pause"],
        BareKey::Menu => &["menu"],
        BareKey::F(n) => return vec![format!("f{}", n)],
        // Any other character can be aliased by the character itself.
        BareKey::Char(c) => return vec![c.to_lowercase().to_string()],
    };
    names.iter().map(|s| s.to_string()).collect()
}

fn style_key_with_modifier(
    key_bindings: &[KeyWithModifier],
    palette: &Styling,
    config: &BTreeMap<String, String>,
) -> Vec<ANSIString<'static>> {
    if key_bindings.is_empty() {
        return vec![];
    }

    let saturated_bg = palette_match!(palette.ribbon_unselected.background);
    let contrasting_fg = palette_match!(palette.ribbon_unselected.base);
    let ribbon = || Style::new().fg(contrasting_fg).on(saturated_bg);

    let mut styled_parts = vec![Style::new().paint(" "), ribbon().paint(" ")];

    for (group_idx, group) in group_keys(key_bindings, config).iter().enumerate() {
        if group_idx > 0 {
            styled_parts.push(ribbon().paint(" "));
        }

        let prefix = group_prefix(&group.modifiers, config);
        if !prefix.is_empty() {
            styled_parts.push(ribbon().bold().paint(prefix));
        }

        for key in &group.keys {
            styled_parts.push(ribbon().bold().paint(key.clone()));
        }
    }

    styled_parts.push(ribbon().paint(" "));

    styled_parts
}

/// Compose the plain-text representation of a key binding as it appears in a
/// hint, e.g. `^p`, `h|j|k|l`, or `←↓↑→`. This is the value substituted for the
/// `{key}` placeholder when a custom `key_format` is configured; the styling
/// itself is left to the format string. Keys sharing a modifier are grouped so
/// the modifier is shown once per group (see `group_keys`).
fn compose_key_text(key_bindings: &[KeyWithModifier], config: &BTreeMap<String, String>) -> String {
    group_keys(key_bindings, config)
        .iter()
        .map(|group| {
            format!(
                "{}{}",
                group_prefix(&group.modifiers, config),
                group.keys.concat()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Named keys that are not letters, digits or arrows, in rough physical order
/// around the main block. Anything absent sorts after everything listed.
const NAMED_KEY_ORDER: &[BareKey] = &[
    BareKey::Esc,
    BareKey::Tab,
    BareKey::CapsLock,
    BareKey::Enter,
    BareKey::Backspace,
    BareKey::Insert,
    BareKey::Delete,
    BareKey::Home,
    BareKey::End,
    BareKey::PageUp,
    BareKey::PageDown,
    BareKey::PrintScreen,
    BareKey::ScrollLock,
    BareKey::Pause,
    BareKey::NumLock,
    BareKey::Menu,
];

/// Sort a hint's keys into a predictable reading order.
///
/// Modifiers lead the sort so that keys sharing one stay contiguous —
/// `group_keys` renders each modifier group as a single run (`^hjkl`), and
/// interleaving would shatter that into `^h h ^j j`.
fn sort_keys(keys: &mut [KeyWithModifier], order: KeyOrder) {
    if order == KeyOrder::Unsorted {
        return;
    }
    keys.sort_by_key(|key| {
        (
            modifier_rank(&key.key_modifiers),
            modifier_tiebreak(&key.key_modifiers),
            key_rank(&key.bare_key, order),
        )
    });
}

/// Which modifier group a key belongs to: unmodified first, then `Ctrl`,
/// `Super`, `Alt`, `Shift`. A key carrying several modifiers sorts with the
/// strongest one it has, so `Ctrl Shift p` groups under `Ctrl`.
///
/// Unmodified keys lead because they are the plainest way to reach an action,
/// and because a hint too wide to fit is cut from the right — so whatever sorts
/// first is what survives.
fn modifier_rank(modifiers: &BTreeSet<KeyModifier>) -> u8 {
    modifiers
        .iter()
        .map(|modifier| match modifier {
            KeyModifier::Ctrl => 1,
            KeyModifier::Super => 2,
            KeyModifier::Alt => 3,
            KeyModifier::Shift => 4,
        })
        .min()
        .unwrap_or(0)
}

/// Separates combinations that share a `modifier_rank` (`Ctrl` from
/// `Ctrl Shift`) so their grouping stays stable rather than depending on the
/// order Zellij reported them.
fn modifier_tiebreak(modifiers: &BTreeSet<KeyModifier>) -> (usize, u8) {
    (modifiers.len(), modifiers.iter().map(modifier_bit).sum())
}

fn modifier_bit(modifier: &KeyModifier) -> u8 {
    match modifier {
        KeyModifier::Ctrl => 1,
        KeyModifier::Super => 2,
        KeyModifier::Alt => 4,
        KeyModifier::Shift => 8,
    }
}

/// Where a key sits in the reading order, as `(category, row, column)`.
///
/// Categories keep unlike keys from interleaving, so letters gather before
/// punctuation rather than mixing at row boundaries — `opas\`, not `op\as`.
/// Within a category, position on the layout does the rest: function keys and
/// digits by number, letters and punctuation by row then column, arrows in
/// `hjkl` order rather than the `Left, Down, Up, Right` the enum declares.
fn key_rank(key: &BareKey, order: KeyOrder) -> (u8, u32, u32) {
    match key {
        BareKey::F(n) => (0, *n as u32, 0),
        BareKey::Char(c) => {
            let lowered = c.to_ascii_lowercase();
            let category = if lowered.is_ascii_digit() {
                1
            } else if lowered.is_ascii_alphabetic() {
                2
            } else {
                3
            };
            // No layout to consult: order by the character itself, which for
            // ASCII is `0-9` then `a-z`.
            if order == KeyOrder::Alphabetical {
                return (category, lowered as u32, 0);
            }
            for (row, keys) in order.rows().iter().enumerate() {
                if let Some(column) = keys.chars().position(|k| k == lowered) {
                    return (category, row as u32, column as u32);
                }
            }
            // Off-layout characters (space, non-ASCII) keep a stable order of
            // their own rather than landing at row 0 alongside the digits.
            (4, lowered as u32, 0)
        }
        BareKey::Left => (5, 0, 0),
        BareKey::Down => (5, 1, 0),
        BareKey::Up => (5, 2, 0),
        BareKey::Right => (5, 3, 0),
        other => {
            let position = NAMED_KEY_ORDER.iter().position(|k| k == other);
            (6, position.unwrap_or(NAMED_KEY_ORDER.len()) as u32, 0)
        }
    }
}

/// Drop one directional family (hjkl or arrows) when a hint is bound to both,
/// per the `direction_keys` setting. Only reduces when both families are
/// present, so a hint bound to just one is never emptied.
fn filter_direction_keys(
    key_bindings: &[KeyWithModifier],
    preference: DirectionKeys,
) -> Vec<KeyWithModifier> {
    if preference == DirectionKeys::Both {
        return key_bindings.to_vec();
    }

    let has_arrows = key_bindings.iter().any(|k| is_arrow_key(&k.bare_key));
    let has_letters = key_bindings.iter().any(|k| is_hjkl_key(&k.bare_key));
    if !(has_arrows && has_letters) {
        return key_bindings.to_vec();
    }

    key_bindings
        .iter()
        .filter(|k| match preference {
            DirectionKeys::Arrows => !is_hjkl_key(&k.bare_key),
            DirectionKeys::Letters => !is_arrow_key(&k.bare_key),
            DirectionKeys::Both => true,
        })
        .cloned()
        .collect()
}

fn is_arrow_key(bare_key: &BareKey) -> bool {
    matches!(
        bare_key,
        BareKey::Left | BareKey::Right | BareKey::Up | BareKey::Down
    )
}

fn is_hjkl_key(bare_key: &BareKey) -> bool {
    matches!(bare_key, BareKey::Char(c) if matches!(c.to_ascii_lowercase(), 'h' | 'j' | 'k' | 'l'))
}

fn style_description(description: &str, palette: &Styling) -> Vec<ANSIString<'static>> {
    let less_saturated_bg = palette_match!(palette.text_unselected.background);
    let contrasting_fg = palette_match!(palette.text_unselected.base);

    vec![Style::new()
        .fg(contrasting_fg)
        .on(less_saturated_bg)
        .paint(format!(" {} ", description))]
}

fn plugin_key(
    keymap: &[(KeyWithModifier, Vec<Action>)],
    plugin_name: &str,
) -> Option<KeyWithModifier> {
    keymap.iter().find_map(|(key, key_actions)| {
        if key_actions
            .iter()
            .any(|action| action.launches_plugin(plugin_name))
        {
            Some(key.clone())
        } else {
            None
        }
    })
}

/// Every key that leaves this mode for Normal.
///
/// Zellij binds Enter and Esc to the very same `SwitchToMode "Normal"`, so they
/// form one hint listing both keys rather than being split into a "select" and
/// an "exit" — a distinction Zellij does not make.
fn exit_keys(keymap: &[(KeyWithModifier, Vec<Action>)]) -> Vec<KeyWithModifier> {
    find_keys_for_actions(keymap, &[TO_NORMAL], true)
}

/// A hint queued for rendering: a concept `id`, the `label` shown for it, and
/// every key bound to it.
///
/// `id` — not `label` — is the merge key, and it is also the `label_<id>` config
/// key. Separating the two is what lets unrelated hints share a display label
/// without fusing (Pane's `new` is `new_pane`, Tab's is `new_tab`), and lets one
/// concept gather keys from several sources (a curated `focus` on `hjkl` and a
/// discovered `focus` on the arrows become one hint).
struct Hint {
    id: String,
    label: String,
    keys: Vec<KeyWithModifier>,
}

/// Queue a hint under concept `id`, recording the keys it consumed in `used` so
/// later discovery does not repeat them.
fn add_hint(
    hints: &mut Vec<Hint>,
    keys: &[KeyWithModifier],
    id: &str,
    label: &str,
    used: &mut Vec<KeyWithModifier>,
) {
    if keys.is_empty() {
        return;
    }
    merge_hint(hints, id, label, keys);
    used.extend(keys.iter().cloned());
}

/// Add `keys` to the hint with this `id`, creating it if it does not exist yet.
/// Keys already present are skipped so a merged hint never repeats a key. The
/// first source to create a hint sets its label; later merges only add keys.
fn merge_hint(hints: &mut Vec<Hint>, id: &str, label: &str, keys: &[KeyWithModifier]) {
    match hints.iter_mut().find(|hint| hint.id == id) {
        Some(hint) => {
            for key in keys {
                if !hint.keys.contains(key) {
                    hint.keys.push(key.clone());
                }
            }
        }
        None => hints.push(Hint {
            id: id.to_string(),
            label: label.to_string(),
            keys: keys.to_vec(),
        }),
    }
}

/// Render a hint's styled segments, without tracking consumed keys.
fn render_hint(
    parts: &mut Vec<ANSIString<'static>>,
    keys: &[KeyWithModifier],
    description: &str,
    style: &HintStyle,
) {
    // Optionally collapse hjkl/arrow duplicates down to a single family.
    let mut keys = filter_direction_keys(keys, style.direction_keys);
    sort_keys(&mut keys, style.key_order);
    let keys = keys.as_slice();

    // Key part: a custom `key_format` (a zjstatus-style format string with a
    // `{key}` placeholder) takes precedence over the theme-palette default.
    match style.key_format {
        Some(fmt) => {
            let key_text = compose_key_text(keys, style.config);
            parts.extend(format::render_template(
                fmt,
                &[("key", &key_text)],
                style.config,
            ));
        }
        None => parts.extend(style_key_with_modifier(keys, style.colors, style.config)),
    }

    // Description part: likewise overridable via `desc_format` with a `{desc}`
    // placeholder.
    match style.desc_format {
        Some(fmt) => {
            parts.extend(format::render_template(
                fmt,
                &[("desc", description)],
                style.config,
            ));
        }
        None => parts.extend(style_description(description, style.colors)),
    }
}

fn render_hints_for_mode(
    mode: InputMode,
    keymap: &[(KeyWithModifier, Vec<Action>)],
    style: &HintStyle,
) -> Vec<ANSIString<'static>> {
    let mut hints: Vec<Hint> = vec![];
    // Keys consumed by curated hints, so discovery does not repeat them.
    let mut used: Vec<KeyWithModifier> = vec![];
    let exit = exit_keys(keymap);

    match mode {
        InputMode::Normal => {
            for (action, label) in NORMAL_MODE_ACTIONS {
                let keys = find_keys_for_actions(keymap, &[action.clone()], true);
                add_curated_hint(&mut hints, &keys, action, label, style, &mut used);
            }
        }
        InputMode::Pane => {
            for (actions, label) in PANE_MODE_ACTION_SEQUENCES {
                let keys = find_keys_for_actions(keymap, actions, false);
                if let (false, Some(action)) = (keys.is_empty(), actions.first()) {
                    add_curated_hint(&mut hints, &keys, action, label, style, &mut used);
                }
            }

            let rename_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode {
                        input_mode: InputMode::RenamePane,
                    },
                    Action::PaneNameInput { input: vec![0] },
                ],
                false,
            );
            if !rename_keys.is_empty() {
                add_group_hint(
                    &mut hints,
                    &rename_keys,
                    "rename",
                    "rename",
                    style,
                    &mut used,
                );
            }

            let focus_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::MoveFocus {
                        direction: Direction::Left,
                    }],
                    &[Action::MoveFocus {
                        direction: Direction::Down,
                    }],
                    &[Action::MoveFocus {
                        direction: Direction::Up,
                    }],
                    &[Action::MoveFocus {
                        direction: Direction::Right,
                    }],
                ],
            );
            add_group_hint(&mut hints, &focus_keys, "focus", "focus", style, &mut used);
            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Tab => {
            for (actions, label) in TAB_MODE_ACTION_SEQUENCES {
                let keys = find_keys_for_actions(keymap, actions, false);
                if let (false, Some(action)) = (keys.is_empty(), actions.first()) {
                    add_curated_hint(&mut hints, &keys, action, label, style, &mut used);
                }
            }

            let rename_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode {
                        input_mode: InputMode::RenameTab,
                    },
                    Action::TabNameInput { input: vec![0] },
                ],
                false,
            );
            if !rename_keys.is_empty() {
                add_group_hint(
                    &mut hints,
                    &rename_keys,
                    "rename",
                    "rename",
                    style,
                    &mut used,
                );
            }

            // Every key bound to tab navigation, both families. Narrowing to one
            // is `direction_keys`' job, done at render time — deciding it here
            // would both ignore that setting and leave the keys it dropped
            // unclaimed, for discovery to resurface as separate hints.
            let focus_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::GoToPreviousTab], &[Action::GoToNextTab]],
            );
            add_group_hint(&mut hints, &focus_keys, "focus", "focus", style, &mut used);
            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Resize => {
            let resize_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize {
                        resize: Resize::Increase,
                        direction: None,
                    }],
                    &[Action::Resize {
                        resize: Resize::Decrease,
                        direction: None,
                    }],
                ],
            );
            add_group_hint(
                &mut hints,
                &resize_keys,
                "resize",
                "resize",
                style,
                &mut used,
            );

            let increase_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize {
                        resize: Resize::Increase,
                        direction: Some(Direction::Left),
                    }],
                    &[Action::Resize {
                        resize: Resize::Increase,
                        direction: Some(Direction::Down),
                    }],
                    &[Action::Resize {
                        resize: Resize::Increase,
                        direction: Some(Direction::Up),
                    }],
                    &[Action::Resize {
                        resize: Resize::Increase,
                        direction: Some(Direction::Right),
                    }],
                ],
            );
            add_group_hint(
                &mut hints,
                &increase_keys,
                "increase",
                "increase",
                style,
                &mut used,
            );

            let decrease_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize {
                        resize: Resize::Decrease,
                        direction: Some(Direction::Left),
                    }],
                    &[Action::Resize {
                        resize: Resize::Decrease,
                        direction: Some(Direction::Down),
                    }],
                    &[Action::Resize {
                        resize: Resize::Decrease,
                        direction: Some(Direction::Up),
                    }],
                    &[Action::Resize {
                        resize: Resize::Decrease,
                        direction: Some(Direction::Right),
                    }],
                ],
            );
            add_group_hint(
                &mut hints,
                &decrease_keys,
                "decrease",
                "decrease",
                style,
                &mut used,
            );
            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Move => {
            let move_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::MovePane {
                        direction: Some(Direction::Left),
                    }],
                    &[Action::MovePane {
                        direction: Some(Direction::Down),
                    }],
                    &[Action::MovePane {
                        direction: Some(Direction::Up),
                    }],
                    &[Action::MovePane {
                        direction: Some(Direction::Right),
                    }],
                ],
            );
            add_group_hint(
                &mut hints,
                &move_keys,
                "move_pane",
                "move",
                style,
                &mut used,
            );
            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Scroll => {
            let search_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode {
                        input_mode: InputMode::EnterSearch,
                    },
                    Action::SearchInput { input: vec![0] },
                ],
                true,
            );
            add_group_hint(
                &mut hints,
                &search_keys,
                "mode_search",
                "search",
                style,
                &mut used,
            );

            let scroll_keys =
                find_keys_for_action_groups(keymap, &[&[Action::ScrollDown], &[Action::ScrollUp]]);
            add_group_hint(
                &mut hints,
                &scroll_keys,
                "scroll",
                "scroll",
                style,
                &mut used,
            );

            let page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::PageScrollDown], &[Action::PageScrollUp]],
            );
            add_group_hint(
                &mut hints,
                &page_scroll_keys,
                "page",
                "page",
                style,
                &mut used,
            );

            let half_page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::HalfPageScrollDown], &[Action::HalfPageScrollUp]],
            );
            add_group_hint(
                &mut hints,
                &half_page_scroll_keys,
                "half_page",
                "half page",
                style,
                &mut used,
            );

            let edit_keys = find_keys_for_actions(
                keymap,
                &[Action::EditScrollback { ansi: false }, TO_NORMAL],
                false,
            );
            if !edit_keys.is_empty() {
                add_group_hint(&mut hints, &edit_keys, "edit", "edit", style, &mut used);
            }
            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Search => {
            let search_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode {
                        input_mode: InputMode::EnterSearch,
                    },
                    Action::SearchInput { input: vec![0] },
                ],
                true,
            );
            add_group_hint(
                &mut hints,
                &search_keys,
                "mode_search",
                "search",
                style,
                &mut used,
            );

            let scroll_keys =
                find_keys_for_action_groups(keymap, &[&[Action::ScrollDown], &[Action::ScrollUp]]);
            add_group_hint(
                &mut hints,
                &scroll_keys,
                "scroll",
                "scroll",
                style,
                &mut used,
            );

            let page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::PageScrollDown], &[Action::PageScrollUp]],
            );
            add_group_hint(
                &mut hints,
                &page_scroll_keys,
                "page",
                "page",
                style,
                &mut used,
            );

            let half_page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::HalfPageScrollDown], &[Action::HalfPageScrollUp]],
            );
            add_group_hint(
                &mut hints,
                &half_page_scroll_keys,
                "half_page",
                "half page",
                style,
                &mut used,
            );

            let down_keys = find_keys_for_actions(
                keymap,
                &[Action::Search {
                    direction: SearchDirection::Down,
                }],
                true,
            );
            add_group_hint(
                &mut hints,
                &down_keys,
                "search_down",
                "down",
                style,
                &mut used,
            );

            let up_keys = find_keys_for_actions(
                keymap,
                &[Action::Search {
                    direction: SearchDirection::Up,
                }],
                true,
            );
            add_group_hint(&mut hints, &up_keys, "search_up", "up", style, &mut used);

            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        InputMode::Session => {
            let detach_keys = find_keys_for_actions(keymap, &[Action::Detach], true);
            add_group_hint(
                &mut hints,
                &detach_keys,
                "detach",
                "detach",
                style,
                &mut used,
            );

            for (plugin, id, label) in SESSION_PLUGINS {
                if let Some(key) = plugin_key(keymap, plugin) {
                    add_group_hint(&mut hints, &[key], id, label, style, &mut used);
                }
            }

            add_group_hint(&mut hints, &exit, "mode_normal", "normal", style, &mut used);
        }
        _ => {
            let keys = find_keys_for_actions(
                keymap,
                &[Action::SwitchToMode {
                    input_mode: InputMode::Normal,
                }],
                true,
            );
            add_group_hint(&mut hints, &keys, "mode_normal", "normal", style, &mut used);
        }
    }

    // Append every other enabled keybinding in this mode that the curated
    // hints above didn't already cover.
    if style.discover {
        add_discovered_hints(&mut hints, keymap, &used, style);
    }

    // Stable, so hints the user didn't name keep the order this mode built them.
    if !style.hint_order.is_empty() {
        hints.sort_by_key(|hint| style.hint_order.rank(hint));
    }

    // Render each hint on its own, so whole hints can be dropped to fit rather
    // than the line being cut mid-hint.
    let mut pieces: Vec<HintPiece> = hints
        .iter()
        .map(|hint| {
            let mut parts = vec![];
            render_hint(&mut parts, &hint.keys, &hint.label, style);
            HintPiece {
                width: visible_width(&parts, style.wide_ambiguous),
                group: style.hint_order.group(hint),
                parts,
            }
        })
        .collect();

    let spacer: Vec<ANSIString<'static>> = style
        .spacer
        .map(|spacer| format::render_template(spacer, &[], style.config))
        .unwrap_or_default();

    let indicator: Vec<ANSIString<'static>> = style
        .drop_indicator
        .map(|indicator| format::render_template(indicator, &[], style.config))
        .unwrap_or_default();

    let gap = style.limit.and_then(|limit| {
        drop_to_fit(
            &mut pieces,
            visible_width(&spacer, style.wide_ambiguous),
            limit,
            visible_width(&indicator, style.wide_ambiguous),
            style.precedence,
        )
    });

    // The indicator stands in for the dropped run, spaced like a hint so it
    // reads as one.
    let mut rendered: Vec<Vec<ANSIString<'static>>> =
        pieces.into_iter().map(|piece| piece.parts).collect();
    if let Some(gap) = gap.filter(|_| !indicator.is_empty()) {
        rendered.insert(gap.min(rendered.len()), indicator);
    }

    let mut parts = vec![];
    for (idx, piece) in rendered.into_iter().enumerate() {
        if idx > 0 {
            parts.extend(spacer.iter().cloned());
        }
        parts.extend(piece);
    }
    parts
}

/// Where a hint sits relative to the `*` in `hint_order`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HintGroup {
    /// Pinned ahead of the `*`.
    Leading,
    /// Unpinned — the `*` itself.
    Middle,
    /// Pinned after the `*`.
    Trailing,
}

/// One rendered hint, kept separate so it can be dropped whole.
struct HintPiece {
    parts: Vec<ANSIString<'static>>,
    width: usize,
    group: HintGroup,
}

fn visible_width(parts: &[ANSIString<'static>], wide_ambiguous: bool) -> usize {
    calculate_visible_length(&format!("{}", ANSIStrings(parts)), wide_ambiguous)
}

/// Drop hints until the line fits, and report where the gap opened.
///
/// Hints go in a single contiguous run so one indicator can stand for all of
/// them. Unpinned hints — the `*` in `hint_order`, the ones the user never spoke
/// for — are given up first, from the right. When those run out the run keeps
/// growing *outward from that gap*: first the leading group from its inner edge,
/// then the trailing group from its inner edge. Dropping from the far ends
/// instead would open a second gap and need a second indicator.
///
/// Returns the index the indicator belongs at, or `None` if nothing was
/// dropped. A single hint wider than the whole limit survives and is cut by
/// `truncate_ansi_string` instead.
fn drop_to_fit(
    pieces: &mut Vec<HintPiece>,
    spacer_width: usize,
    limit: usize,
    indicator_width: usize,
    precedence: HintPrecedence,
) -> Option<usize> {
    let width = |pieces: &Vec<HintPiece>, extra: usize| {
        let hints: usize = pieces.iter().map(|piece| piece.width).sum();
        let count = pieces.len() + usize::from(extra > 0);
        hints + extra + spacer_width * count.saturating_sub(1)
    };

    let mut gap = None;
    while pieces.len() > 1 {
        // Once a hint has been dropped the indicator is part of the line, so it
        // has to be paid for out of the same budget.
        let extra = gap.map_or(0, |_| indicator_width);
        if width(pieces, extra) <= limit {
            break;
        }
        let Some(index) = next_to_drop(pieces, gap, precedence) else {
            break;
        };
        pieces.remove(index);
        gap = Some(index);
    }
    gap
}

/// The hint to give up next, as an index into `pieces`.
///
/// Always the one adjacent to the gap, so what is dropped stays a single run.
/// Unpinned hints go first; between the two pinned groups, `precedence` says
/// which is held onto longer.
fn next_to_drop(
    pieces: &[HintPiece],
    gap: Option<usize>,
    precedence: HintPrecedence,
) -> Option<usize> {
    // Only ever take from the inner edge of a group, the side facing the gap.
    let inner_left = |group: HintGroup| {
        pieces
            .iter()
            .rposition(|piece| piece.group == group)
            .filter(|index| gap.is_none_or(|gap| *index < gap))
    };
    let inner_right = |group: HintGroup| {
        pieces
            .iter()
            .position(|piece| piece.group == group)
            .filter(|index| gap.is_none_or(|gap| *index >= gap))
    };

    let leading = || inner_left(HintGroup::Leading);
    let trailing = || inner_right(HintGroup::Trailing);

    inner_left(HintGroup::Middle)
        .or_else(|| inner_right(HintGroup::Middle))
        .or_else(|| match precedence {
            // Keep the trailing group: spend the leading one first.
            HintPrecedence::Trailing => leading().or_else(trailing),
            HintPrecedence::Leading => trailing().or_else(leading),
        })
}

/// Discover every enabled keybinding the curated hints didn't already show, and
/// merge it into `hints`. Bindings are keyed by their resolved label (so
/// families like the directional focus keys collapse into a single hint, and a
/// label a curated hint already produced gains the extra keys instead of being
/// shown a second time). Labels resolve from, in order: a `label_<action>`
/// config override, the built-in label table, or a name derived from the action.
///
/// Bindings inherited from the base mode (Zellij's `shared_except` groups) are
/// skipped, so entering Pane mode doesn't re-list the global mode switches that
/// are already advertised in the base mode.
fn add_discovered_hints(
    hints: &mut Vec<Hint>,
    keymap: &[(KeyWithModifier, Vec<Action>)],
    used: &[KeyWithModifier],
    style: &HintStyle,
) {
    for (key, actions) in keymap {
        if used.contains(key) {
            continue;
        }
        // A binding's first action is often plumbing — a pipe to a plugin, a
        // rename input — with the part worth advertising coming after it. Judge
        // the binding by its first action that is worth a hint, so a key like
        // `MessagePlugin "autolock"; SwitchToMode "Normal"` still shows as the
        // unlock it visibly is, rather than vanishing entirely.
        let Some(primary) = actions.iter().find(|action| !is_hidden_action(action)) else {
            continue;
        };
        if is_shared_binding(key, primary, style.shared) {
            continue;
        }
        let Some((id, label)) = resolve_hint(primary, style) else {
            continue;
        };

        merge_hint(hints, &id, &label, std::slice::from_ref(key));
    }
}

/// Whether `key` is bound to the same action in the base mode, meaning it is a
/// global binding already shown there rather than one specific to this mode.
fn is_shared_binding(
    key: &KeyWithModifier,
    action: &Action,
    shared: &[(KeyWithModifier, Vec<Action>)],
) -> bool {
    shared.iter().any(|(shared_key, shared_actions)| {
        shared_key == key
            && shared_actions
                .first()
                .is_some_and(|shared_action| shared_action.shallow_eq(action))
    })
}

/// Resolve the label for a discovered keybinding's primary action. A user
/// `label_<action>` override wins; an empty override hides the hint. Otherwise
/// the built-in table is consulted, falling back to a name derived from the
/// action's own signature so nothing is ever left unlabeled.
fn resolve_hint(action: &Action, style: &HintStyle) -> Option<(String, String)> {
    let signature = action_signature(action);
    let (id, default) = match builtin_hint(action) {
        Some((id, label)) => (id.to_string(), label.to_string()),
        // No curated entry: the action names its own concept, and the label is
        // derived from that same name.
        None => (signature.clone(), signature.replace('_', " ")),
    };

    match label_override(&id, &signature, style.mode, style.config) {
        // An explicit label is its own merge id, so two actions given the same
        // label become one hint. See `merged_label_id`.
        Some(Some(label)) => Some((merged_label_id(&label), label)),
        // An empty override hides the hint.
        Some(None) => None,
        None => Some((id, default)),
    }
}

/// Look up a `label_<...>` override, accepting the hint's concept `id`
/// (`label_split_down`) or the raw action `signature` (`label_new_pane_down`),
/// each optionally scoped to the current mode (`label_locked_mode_normal`).
///
/// The outer `Option` distinguishes "no override set" from an override that is
/// present; the inner one is `None` for an empty value, meaning hide the hint.
fn label_override(
    id: &str,
    signature: &str,
    mode: &str,
    config: &BTreeMap<String, String>,
) -> Option<Option<String>> {
    // Most specific first: a label scoped to this mode beats a global one, and
    // the concept id beats the raw action signature. This is what lets
    // `label_locked_mode_normal "unlock"` retitle the unlock key without
    // touching every other mode's escape hatch.
    let value = [
        format!("{}{}_{}", CONFIG_LABEL_PREFIX, mode, id),
        format!("{}{}_{}", CONFIG_LABEL_PREFIX, mode, signature),
        format!("{}{}", CONFIG_LABEL_PREFIX, id),
        format!("{}{}", CONFIG_LABEL_PREFIX, signature),
    ]
    .iter()
    .find_map(|key| config.get(key))?;
    if value.is_empty() {
        Some(None)
    } else {
        Some(Some(value.clone()))
    }
}

/// Queue a curated hint, honoring a `label_<action>` override on `action`.
///
/// Curated hints carry hand-tuned labels rather than deriving them, so without
/// this they would silently ignore `label_<action>` entirely. An override that
/// hides the hint (an empty value) still marks its keys used, so discovery does
/// not resurface the same binding under the built-in label.
fn add_curated_hint(
    hints: &mut Vec<Hint>,
    keys: &[KeyWithModifier],
    action: &Action,
    default: &str,
    style: &HintStyle,
    used: &mut Vec<KeyWithModifier>,
) {
    let signature = action_signature(action);
    let id = builtin_hint(action)
        .map(|(id, _)| id.to_string())
        .unwrap_or_else(|| signature.clone());
    apply_label(hints, keys, &id, &signature, default, style, used);
}

/// Queue a curated hint that has no single backing action — one assembled from a
/// group of related bindings (the four resize directions), from alternatives
/// (`rename` matches whichever of two actions is bound), or discovered at
/// runtime (`select`, the plugin launchers). These declare their concept `id`
/// explicitly, since there is no action to derive it from.
fn add_group_hint(
    hints: &mut Vec<Hint>,
    keys: &[KeyWithModifier],
    id: &str,
    default: &str,
    style: &HintStyle,
    used: &mut Vec<KeyWithModifier>,
) {
    apply_label(hints, keys, id, id, default, style, used);
}

/// Queue a hint under `id`, applying a `label_<id>` / `label_<signature>`
/// override if one is set. An override that hides the hint still marks its keys
/// used, so discovery does not resurface it under the built-in label.
fn apply_label(
    hints: &mut Vec<Hint>,
    keys: &[KeyWithModifier],
    id: &str,
    signature: &str,
    default: &str,
    style: &HintStyle,
    used: &mut Vec<KeyWithModifier>,
) {
    match label_override(id, signature, style.mode, style.config) {
        Some(Some(label)) => add_hint(hints, keys, &merged_label_id(&label), &label, used),
        Some(None) => used.extend(keys.iter().cloned()),
        None => add_hint(hints, keys, id, default, used),
    }
}

/// The merge id for a hint whose label the user set explicitly.
///
/// Deliberately relabeling two hints to the same text fuses them into one hint —
/// that is how you combine separate bindings you consider a single concept, e.g.
/// giving both swap-layout actions the label "swap layout". Built-in labels do
/// *not* merge this way, so hints that merely ship with the same label (Pane's
/// `new` and Tab's `new`) stay separate.
fn merged_label_id(label: &str) -> String {
    // `=` never appears in a concept id, so this cannot collide with one.
    format!("={}", label)
}

/// A stable, configuration-friendly identifier for an action, used both as the
/// `label_<name>` config key and as the derived fallback label. It is the
/// snake_case variant name, with the target mode appended for mode switches
/// (e.g. `switch_to_mode_locked`) since that distinction is meaningful.
fn action_signature(action: &Action) -> String {
    let debug = format!("{:?}", action);
    let variant = debug.split(['(', '{', ' ']).next().unwrap_or(&debug);
    let mut signature = to_snake_case(variant);
    match action {
        Action::SwitchToMode { input_mode: mode }
        | Action::SwitchModeForAllClients { input_mode: mode } => {
            signature.push('_');
            signature.push_str(&to_snake_case(&format!("{:?}", mode)));
        }
        // Directional variants are distinct hints with distinct labels (`split
        // down` vs `new`), so they need distinct signatures to be addressable:
        // `label_new_pane_down` rather than one `label_new_pane` for all five.
        Action::NewPane {
            direction: Some(direction),
            ..
        }
        | Action::MovePane {
            direction: Some(direction),
        }
        | Action::MoveFocus { direction }
        | Action::MoveFocusOrTab { direction }
        | Action::MoveTab { direction } => {
            signature.push('_');
            signature.push_str(&to_snake_case(&format!("{:?}", direction)));
        }
        Action::Resize { resize, direction } => {
            signature.push('_');
            signature.push_str(&to_snake_case(&format!("{:?}", resize)));
            if let Some(direction) = direction {
                signature.push('_');
                signature.push_str(&to_snake_case(&format!("{:?}", direction)));
            }
        }
        _ => {}
    }
    signature
}

fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Actions that are never useful as hints (text input, mouse, programmatic /
/// CLI-only actions). These are skipped during discovery.
fn is_hidden_action(action: &Action) -> bool {
    matches!(
        action,
        Action::Write { .. }
            | Action::WriteChars { .. }
            | Action::PaneNameInput { .. }
            | Action::TabNameInput { .. }
            | Action::SearchInput { .. }
            | Action::RenameTab { .. }
            | Action::RenameTerminalPane { .. }
            | Action::RenamePluginPane { .. }
            | Action::NoOp
            | Action::MouseEvent { .. }
            | Action::ScrollUpAt { .. }
            | Action::ScrollDownAt { .. }
            | Action::CliPipe { .. }
            | Action::KeybindPipe { .. }
            | Action::DumpScreen { .. }
            | Action::DumpLayout
            | Action::ListClients
            | Action::QueryTabNames
            | Action::Run { .. }
            | Action::SkipConfirm { .. }
            | Action::StackPanes { .. }
            | Action::ChangeFloatingPaneCoordinates { .. }
    )
}

/// The built-in action -> (id, label) table. `id` is the concept the hint
/// belongs to — the merge key and the `label_<id>` config key — and `label` is
/// what gets displayed. Returns `None` for actions with no curated entry, in
/// which case the caller derives both from the action signature.
///
/// Ids must be unique per concept even where labels repeat: `new_pane` and
/// `new_tab` both display "new" but must never merge. Conversely, actions that
/// deliberately form one hint share an id (the four `MoveFocus` directions).
fn builtin_hint(action: &Action) -> Option<(&'static str, &'static str)> {
    if let Action::SwitchToMode { input_mode: mode }
    | Action::SwitchModeForAllClients { input_mode: mode } = action
    {
        return switch_mode_hint(mode);
    }

    Some(match action {
        Action::Quit => ("quit", "quit"),
        Action::Detach => ("detach", "detach"),

        // Panes
        Action::NewPane {
            direction: None, ..
        } => ("new_pane", "new"),
        Action::NewPane {
            direction: Some(Direction::Left),
            ..
        } => ("split_left", "split left"),
        Action::NewPane {
            direction: Some(Direction::Right),
            ..
        } => ("split_right", "split right"),
        Action::NewPane {
            direction: Some(Direction::Up),
            ..
        } => ("split_up", "split up"),
        Action::NewPane {
            direction: Some(Direction::Down),
            ..
        } => ("split_down", "split down"),
        Action::NewStackedPane { .. } => ("stacked_pane", "stacked"),
        Action::NewFloatingPane { .. } => ("floating_pane", "floating"),
        Action::NewInPlacePane { .. } => ("in_place_pane", "in place"),
        Action::CloseFocus => ("close_pane", "close"),
        Action::ToggleFocusFullscreen => ("fullscreen", "fullscreen"),
        Action::ToggleFloatingPanes => ("float", "float"),
        Action::TogglePaneEmbedOrFloating => ("embed", "embed"),
        Action::TogglePaneFrames => ("frames", "frames"),
        Action::TogglePanePinned => ("pin", "pin"),
        Action::TogglePaneInGroup => ("group_pane", "group"),
        Action::ToggleGroupMarking => ("group_marking", "mark"),
        // Directing focus is "focus"; only actions that relocate a pane are
        // "move". Distinct ids keep them apart from each other and from the
        // `SwitchToMode(Move)` hint, which means something else entirely.
        Action::MoveFocus { direction: _ } | Action::MoveFocusOrTab { direction: _ } => {
            ("focus", "focus")
        }
        Action::MovePane { direction: _ } => ("move_pane", "move"),
        Action::MovePaneBackwards => ("move_pane_back", "move back"),
        Action::FocusNextPane => ("next_pane", "next"),
        Action::FocusPreviousPane => ("prev_pane", "prev"),
        Action::SwitchFocus => ("toggle_focus", "toggle focus"),

        // Tabs
        Action::NewTab { .. } => ("new_tab", "new"),
        Action::CloseTab => ("close_tab", "close"),
        Action::GoToNextTab => ("next_tab", "next"),
        Action::GoToPreviousTab => ("prev_tab", "prev"),
        Action::GoToTab { index: _ } | Action::GoToTabName { .. } => ("go_to_tab", "tab"),
        Action::ToggleTab => ("toggle_tab", "toggle"),
        Action::ToggleActiveSyncTab => ("sync", "sync"),
        Action::MoveTab { direction: _ } => ("move_tab", "move tab"),
        Action::BreakPane => ("break_pane", "break pane"),
        Action::BreakPaneLeft => ("break_left", "break left"),
        Action::BreakPaneRight => ("break_right", "break right"),

        // Resize
        Action::Resize {
            resize: Resize::Increase,
            direction: None,
        }
        | Action::Resize {
            resize: Resize::Decrease,
            direction: None,
        } => ("resize", "resize"),
        Action::Resize {
            resize: Resize::Increase,
            direction: Some(_),
        } => ("increase", "increase"),
        Action::Resize {
            resize: Resize::Decrease,
            direction: Some(_),
        } => ("decrease", "decrease"),

        // Scroll / search
        Action::ScrollUp | Action::ScrollDown => ("scroll", "scroll"),
        Action::PageScrollUp | Action::PageScrollDown => ("page", "page"),
        Action::HalfPageScrollUp | Action::HalfPageScrollDown => ("half_page", "half page"),
        Action::ScrollToTop => ("top", "top"),
        Action::ScrollToBottom => ("bottom", "bottom"),
        Action::EditScrollback { .. } => ("edit", "edit"),
        Action::Search {
            direction: SearchDirection::Down,
        } => ("search_down", "down"),
        Action::Search {
            direction: SearchDirection::Up,
        } => ("search_up", "up"),
        Action::SearchToggleOption { option: _ } => ("search_toggle", "toggle"),

        // Misc
        Action::Copy => ("copy", "copy"),
        Action::ClearScreen => ("clear", "clear"),
        Action::ToggleMouseMode => ("mouse", "mouse"),
        Action::PreviousSwapLayout => ("prev_layout", "prev layout"),
        Action::NextSwapLayout => ("next_layout", "next layout"),
        Action::Confirm => ("confirm", "confirm"),
        Action::Deny => ("deny", "deny"),
        Action::RenameSession { name: _ } => ("rename_session", "rename"),
        Action::UndoRenamePane | Action::UndoRenameTab => ("undo_rename", "undo"),

        _ => return None,
    })
}

/// Id and label for a `SwitchToMode` action, based on the target mode. Ids are
/// `mode_`-prefixed so that, e.g., switching to Move mode never merges with the
/// `MovePane` hint that shares its label.
fn switch_mode_hint(mode: &InputMode) -> Option<(&'static str, &'static str)> {
    Some(match mode {
        InputMode::Normal => ("mode_normal", "normal"),
        InputMode::Locked => ("mode_locked", "lock"),
        InputMode::Pane => ("mode_pane", "pane"),
        InputMode::Tab => ("mode_tab", "tab"),
        InputMode::Resize => ("mode_resize", "resize"),
        InputMode::Move => ("mode_move", "move"),
        InputMode::Scroll => ("mode_scroll", "scroll"),
        InputMode::Search | InputMode::EnterSearch => ("mode_search", "search"),
        InputMode::Session => ("mode_session", "session"),
        InputMode::RenameTab | InputMode::RenamePane => ("mode_rename", "rename"),
        InputMode::Tmux => ("mode_tmux", "tmux"),
        InputMode::Prompt => ("mode_prompt", "prompt"),
    })
}

fn get_keymap_for_mode(mode_info: &ModeInfo) -> Vec<(KeyWithModifier, Vec<Action>)> {
    match mode_info.mode {
        InputMode::Normal => mode_info.get_keybinds_for_mode(InputMode::Normal),
        InputMode::Pane => mode_info.get_keybinds_for_mode(InputMode::Pane),
        InputMode::Tab => mode_info.get_keybinds_for_mode(InputMode::Tab),
        InputMode::Resize => mode_info.get_keybinds_for_mode(InputMode::Resize),
        InputMode::Move => mode_info.get_keybinds_for_mode(InputMode::Move),
        InputMode::Scroll => mode_info.get_keybinds_for_mode(InputMode::Scroll),
        InputMode::Search => mode_info.get_keybinds_for_mode(InputMode::Search),
        InputMode::Session => mode_info.get_keybinds_for_mode(InputMode::Session),
        _ => mode_info.get_mode_keybinds(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Sort a string of characters as if they were the keys of one hint, so an
    /// expected ordering can be written as a plain string literal.
    fn sorted(chars: &str, order: KeyOrder) -> String {
        let mut keys: Vec<KeyWithModifier> = chars
            .chars()
            .map(|c| KeyWithModifier::new(BareKey::Char(c)))
            .collect();
        sort_keys(&mut keys, order);
        keys.iter()
            .map(|key| match key.bare_key {
                BareKey::Char(c) => c,
                _ => '?',
            })
            .collect()
    }

    #[test]
    fn every_layout_covers_the_alphabet_and_digits_exactly_once() {
        for order in [KeyOrder::Qwerty, KeyOrder::Dvorak, KeyOrder::Colemak] {
            let all: String = order.rows().concat();
            for expected in "abcdefghijklmnopqrstuvwxyz0123456789".chars() {
                assert_eq!(
                    all.matches(expected).count(),
                    1,
                    "{:?} should place {:?} exactly once",
                    order.rows(),
                    expected
                );
            }
        }
    }

    #[test]
    fn digits_follow_the_digit_row_so_zero_comes_last() {
        assert_eq!(sorted("271543689", KeyOrder::Qwerty), "123456789");
        assert_eq!(sorted("0159", KeyOrder::Qwerty), "1590");
    }

    #[test]
    fn letters_sort_by_row_then_column() {
        assert_eq!(sorted("kh", KeyOrder::Qwerty), "hk");
        assert_eq!(sorted("lkjh", KeyOrder::Qwerty), "hjkl");
        // Rows run top to bottom: q above a above z.
        assert_eq!(sorted("zaq", KeyOrder::Qwerty), "qaz");
    }

    #[test]
    fn letters_group_ahead_of_punctuation_rather_than_interleaving() {
        assert_eq!(sorted("op\\as", KeyOrder::Qwerty), "opas\\");
    }

    #[test]
    fn digits_letters_and_punctuation_stay_in_separate_groups() {
        assert_eq!(sorted("a1/", KeyOrder::Qwerty), "1a/");
        assert_eq!(sorted("/1a", KeyOrder::Qwerty), "1a/");
    }

    #[test]
    fn layout_changes_where_letters_land() {
        // hjkl only reads in order on the layout it was designed for. Dvorak
        // scatters it across all three rows — l on the top, h on the home row,
        // j and k down on the bottom — so the vim ordering is lost.
        assert_eq!(sorted("hjkl", KeyOrder::Qwerty), "hjkl");
        assert_eq!(sorted("hjkl", KeyOrder::Dvorak), "lhjk");
        assert_eq!(sorted("arst", KeyOrder::Colemak), "arst");
    }

    #[test]
    fn alphabetical_ignores_the_keyboard_layout() {
        assert_eq!(sorted("dbca", KeyOrder::Alphabetical), "abcd");
        // The layout modes would order these by position instead.
        assert_eq!(sorted("lkjh", KeyOrder::Alphabetical), "hjkl");
        assert_eq!(sorted("zaq", KeyOrder::Alphabetical), "aqz");
        assert_eq!(sorted("qaz", KeyOrder::Qwerty), "qaz");
    }

    #[test]
    fn alphabetical_puts_zero_first_unlike_the_digit_row() {
        // Plain ascending order, so `0` leads rather than trailing `9`.
        assert_eq!(sorted("0159", KeyOrder::Alphabetical), "0159");
        assert_eq!(sorted("0159", KeyOrder::Qwerty), "1590");
    }

    #[test]
    fn alphabetical_still_groups_digits_letters_and_punctuation() {
        assert_eq!(sorted("/a1", KeyOrder::Alphabetical), "1a/");
    }

    #[test]
    fn unsorted_preserves_the_order_zellij_reported() {
        assert_eq!(sorted("271543689", KeyOrder::Unsorted), "271543689");
        assert_eq!(sorted("op\\as", KeyOrder::Unsorted), "op\\as");
    }

    #[test]
    fn arrows_follow_hjkl_order_not_declaration_order() {
        let mut keys: Vec<KeyWithModifier> =
            [BareKey::Right, BareKey::Up, BareKey::Left, BareKey::Down]
                .into_iter()
                .map(KeyWithModifier::new)
                .collect();
        sort_keys(&mut keys, KeyOrder::Qwerty);
        let bare: Vec<BareKey> = keys.iter().map(|k| k.bare_key.clone()).collect();
        assert_eq!(
            bare,
            vec![BareKey::Left, BareKey::Down, BareKey::Up, BareKey::Right]
        );
    }

    #[test]
    fn function_keys_sort_numerically_not_lexically() {
        let mut keys: Vec<KeyWithModifier> = [10u8, 2, 1]
            .into_iter()
            .map(|n| KeyWithModifier::new(BareKey::F(n)))
            .collect();
        sort_keys(&mut keys, KeyOrder::Qwerty);
        let bare: Vec<BareKey> = keys.iter().map(|k| k.bare_key.clone()).collect();
        assert_eq!(bare, vec![BareKey::F(1), BareKey::F(2), BareKey::F(10)]);
    }

    #[test]
    fn modifier_groups_run_unmodified_then_ctrl_super_alt_shift() {
        let plain = KeyWithModifier::new(BareKey::Char('a'));
        let mut keys = vec![
            plain.clone().with_shift_modifier(),
            plain.clone().with_alt_modifier(),
            plain.clone(),
            plain.clone().with_super_modifier(),
            plain.clone().with_ctrl_modifier(),
        ];
        sort_keys(&mut keys, KeyOrder::Qwerty);
        // Assert the modifiers themselves, not just that the ranks ascend —
        // ascending ranks would hold for any ordering.
        let groups: Vec<Vec<KeyModifier>> = keys
            .iter()
            .map(|key| key.key_modifiers.iter().copied().collect())
            .collect();
        assert_eq!(
            groups,
            vec![
                vec![],
                vec![KeyModifier::Ctrl],
                vec![KeyModifier::Super],
                vec![KeyModifier::Alt],
                vec![KeyModifier::Shift],
            ]
        );
    }

    #[test]
    fn a_key_sorts_with_the_strongest_modifier_it_carries() {
        let plain = KeyWithModifier::new(BareKey::Char('p'));
        let ctrl_shift = plain.clone().with_ctrl_modifier().with_shift_modifier();
        assert_eq!(modifier_rank(&ctrl_shift.key_modifiers), 1);
        // Same group as plain Ctrl, but still ordered after it.
        let ctrl = plain.with_ctrl_modifier();
        assert_eq!(modifier_rank(&ctrl.key_modifiers), 1);
        assert!(
            modifier_tiebreak(&ctrl.key_modifiers) < modifier_tiebreak(&ctrl_shift.key_modifiers)
        );
    }

    /// Build hints with the given ids (label mirrors the id unless it contains
    /// `/`, written as `id/label`), order them, and read back the resulting ids.
    fn ordered(ids: &[&str], config: &str) -> Vec<String> {
        let mut hints: Vec<Hint> = ids
            .iter()
            .map(|spec| {
                let (id, label) = spec.split_once('/').unwrap_or((spec, spec));
                Hint {
                    id: id.to_string(),
                    label: label.to_string(),
                    keys: vec![],
                }
            })
            .collect();
        let order = HintOrder::from_config(config);
        if !order.is_empty() {
            hints.sort_by_key(|hint| order.rank(hint));
        }
        hints.into_iter().map(|hint| hint.id).collect()
    }

    #[test]
    fn wildcard_splits_pinned_front_from_pinned_back() {
        assert_eq!(
            ordered(&["a", "b", "c", "d"], "d, *, a"),
            vec!["d", "b", "c", "a"]
        );
    }

    #[test]
    fn entries_before_the_wildcard_lead_in_the_order_given() {
        assert_eq!(ordered(&["a", "b", "c"], "c, b, *"), vec!["c", "b", "a"]);
    }

    #[test]
    fn entries_after_the_wildcard_trail_in_the_order_given() {
        assert_eq!(ordered(&["a", "b", "c"], "*, b, a"), vec!["c", "b", "a"]);
    }

    #[test]
    fn a_list_without_a_wildcard_leads_and_the_rest_follow() {
        assert_eq!(ordered(&["a", "b", "c"], "c, a"), vec!["c", "a", "b"]);
    }

    #[test]
    fn unlisted_hints_keep_the_order_the_mode_built_them() {
        assert_eq!(
            ordered(&["a", "b", "c", "d", "e"], "e, *"),
            vec!["e", "a", "b", "c", "d"]
        );
    }

    #[test]
    fn an_empty_or_absent_order_changes_nothing() {
        assert_eq!(ordered(&["b", "a"], ""), vec!["b", "a"]);
        assert_eq!(ordered(&["b", "a"], "  ,  "), vec!["b", "a"]);
    }

    #[test]
    fn naming_an_absent_hint_is_harmless() {
        assert_eq!(ordered(&["a", "b"], "nope, *, alsonope"), vec!["a", "b"]);
    }

    #[test]
    fn hints_can_be_named_by_label_as_well_as_id() {
        // A hint fused by a shared label carries the internal id `=swap layout`;
        // naming it by the label is what makes it addressable.
        assert_eq!(
            ordered(&["a", "=swap layout/swap layout"], "swap layout, *"),
            vec!["=swap layout", "a"]
        );
    }

    #[test]
    fn matching_ignores_case_and_surrounding_space() {
        assert_eq!(ordered(&["a", "Quit"], "  QUIT  , *"), vec!["Quit", "a"]);
    }

    /// Render a mode from a synthetic keymap and return the visible text, so a
    /// whole hint line can be asserted as a plain string.
    fn rendered(
        mode: InputMode,
        keymap: &[(KeyWithModifier, Vec<Action>)],
        config: &[(&str, &str)],
    ) -> String {
        let colors = Styling::default();
        let config = label_config(config);
        let hint_order = HintOrder::from_config(config.get("hint_order").map_or("", |v| v));
        let mode_key = format!("{:?}", mode).to_lowercase();
        let style = HintStyle {
            colors: &colors,
            key_format: Some("{key} "),
            desc_format: Some("{desc}"),
            spacer: Some("|"),
            discover: config.get("discover_hints").map_or(true, |v| v == "true"),
            direction_keys: DirectionKeys::from_config(
                config.get("direction_keys").map_or("both", |v| v),
            ),
            key_order: KeyOrder::default(),
            mode: &mode_key,
            hint_order: &hint_order,
            limit: config.get("limit").and_then(|v| v.parse::<usize>().ok()),
            drop_indicator: config.get("drop_indicator").map(|v| v.as_str()),
            precedence: HintPrecedence::from_config(
                config.get("hint_precedence").map_or("", |v| v),
            ),
            wide_ambiguous: config
                .get("ambiguous_width")
                .map(|v| v == "2")
                .unwrap_or(false),
            shared: &[],
            config: &config,
        };
        let parts = render_hints_for_mode(mode, keymap, &style);
        let text = format!("{}", ANSIStrings(&parts));
        let mut parser = AnsiParser::new(&text);
        let mut visible = String::new();
        while let Some(segment) = parser.next_segment() {
            if let AnsiSegment::VisibleChar(ch) = segment {
                visible.push(ch);
            }
        }
        visible
    }

    fn key(bare: BareKey) -> KeyWithModifier {
        KeyWithModifier::new(bare)
    }

    fn ctrl(c: char) -> KeyWithModifier {
        KeyWithModifier::new(BareKey::Char(c)).with_ctrl_modifier()
    }

    /// `MessagePlugin` in KDL, which parses to this plumbing action.
    fn message_plugin() -> Action {
        Action::KeybindPipe {
            name: None,
            payload: Some("enable".to_string()),
            args: None,
            plugin: Some("autolock".to_string()),
            plugin_id: None,
            configuration: None,
            launch_new: false,
            skip_cache: false,
            floating: None,
            in_place: None,
            cwd: None,
            pane_title: None,
        }
    }

    #[test]
    fn a_binding_led_by_a_plumbing_action_still_shows_what_it_does() {
        // Locked mode's only binding is `MessagePlugin ...; SwitchToMode
        // "Normal"`. Judging it by its first action alone hid the mode entirely.
        let keymap = vec![(ctrl('g'), vec![message_plugin(), TO_NORMAL])];
        assert_eq!(rendered(InputMode::Locked, &keymap, &[]), "Ctrl g normal");
        assert_eq!(
            rendered(
                InputMode::Locked,
                &keymap,
                &[("label_locked_mode_normal", "unlock")]
            ),
            "Ctrl g unlock"
        );
    }

    #[test]
    fn a_binding_of_pure_plumbing_stays_hidden() {
        let keymap = vec![(ctrl('g'), vec![message_plugin()])];
        assert_eq!(rendered(InputMode::Locked, &keymap, &[]), "");
    }

    /// Tab navigation as Zellij binds it by default: both families, both ways.
    fn tab_nav_keymap() -> Vec<(KeyWithModifier, Vec<Action>)> {
        vec![
            (key(BareKey::Char('h')), vec![Action::GoToPreviousTab]),
            (key(BareKey::Left), vec![Action::GoToPreviousTab]),
            (key(BareKey::Char('l')), vec![Action::GoToNextTab]),
            (key(BareKey::Right), vec![Action::GoToNextTab]),
        ]
    }

    #[test]
    fn tab_focus_honors_direction_keys_and_claims_every_key_it_shows() {
        // With "letters", the arrows drop out and no stray next/prev hints are
        // left behind for discovery to resurface.
        assert_eq!(
            rendered(
                InputMode::Tab,
                &tab_nav_keymap(),
                &[("direction_keys", "letters")]
            ),
            "hl focus"
        );
        assert_eq!(
            rendered(
                InputMode::Tab,
                &tab_nav_keymap(),
                &[("direction_keys", "arrows")]
            ),
            "←→ focus"
        );
    }

    #[test]
    fn tab_focus_shows_both_families_by_default() {
        assert_eq!(
            rendered(InputMode::Tab, &tab_nav_keymap(), &[]),
            "hl←→ focus"
        );
    }

    #[test]
    fn every_key_that_leaves_a_mode_forms_one_hint() {
        // Enter and Esc are bound to the identical action, so they are one hint
        // rather than a "select" and an "exit" for the same thing.
        let keymap = vec![
            (key(BareKey::Enter), vec![TO_NORMAL]),
            (key(BareKey::Esc), vec![TO_NORMAL]),
        ];
        assert_eq!(rendered(InputMode::Pane, &keymap, &[]), "ESCENTER normal");
        assert_eq!(rendered(InputMode::Tab, &keymap, &[]), "ESCENTER normal");
    }

    #[test]
    fn a_mode_keeps_its_exit_hint_without_discovery() {
        // The curated list has to carry the escape hatch itself, since discovery
        // is off by default.
        let keymap = vec![
            (key(BareKey::Char('x')), vec![Action::CloseFocus, TO_NORMAL]),
            (key(BareKey::Esc), vec![TO_NORMAL]),
        ];
        assert_eq!(
            rendered(InputMode::Pane, &keymap, &[("discover_hints", "false")]),
            "x close|ESC normal"
        );
    }

    #[test]
    fn a_mode_scoped_label_reaches_the_rendered_line() {
        let config = &[
            ("label_mode_normal", "exit"),
            ("label_locked_mode_normal", "unlock"),
        ];
        // Locked binds only the one escape hatch.
        let locked = vec![(key(BareKey::Esc), vec![TO_NORMAL])];
        assert_eq!(rendered(InputMode::Locked, &locked, config), "ESC unlock");
        // Pane binds Enter and Esc to the same action, so they form one hint
        // carrying both keys, under the global label.
        let pane = vec![
            (key(BareKey::Enter), vec![TO_NORMAL]),
            (key(BareKey::Esc), vec![TO_NORMAL]),
        ];
        assert_eq!(rendered(InputMode::Pane, &pane, config), "ESCENTER exit");
    }

    /// A styled line of the shape the plugin actually emits: coloured segments
    /// closed by the reset `ANSIStrings` appends.
    fn styled_line() -> String {
        let parts = vec![
            Style::new().on(Fixed(1)).paint("aaaa"),
            Style::new().on(Fixed(2)).paint("bbbb"),
        ];
        format!("{}", ANSIStrings(&parts))
    }

    #[test]
    fn an_untruncated_line_already_ends_reset() {
        assert!(styled_line().ends_with(ANSI_RESET));
    }

    #[test]
    fn truncating_closes_the_styled_run_so_colour_stops_at_the_cut() {
        let truncated = truncate_ansi_string(&styled_line(), "...", 6, false);
        assert!(
            truncated.ends_with(ANSI_RESET),
            "colour would bleed past the cut: {:?}",
            truncated
        );
    }

    #[test]
    fn truncating_keeps_the_visible_width_within_the_limit() {
        for limit in 1..=10 {
            let truncated = truncate_ansi_string(&styled_line(), "...", limit, false);
            assert!(
                calculate_visible_length(&truncated, false) <= limit,
                "limit {} produced {:?}",
                limit,
                truncated
            );
        }
    }

    #[test]
    fn a_line_that_fits_is_left_exactly_as_it_was() {
        let line = styled_line();
        assert_eq!(truncate_ansi_string(&line, "...", 100, false), line);
    }

    /// Four one-key hints, each rendering as `<key> <label>` — 6 columns wide
    /// with the test harness's formats, plus a 1-column spacer between them.
    fn four_hints() -> Vec<(KeyWithModifier, Vec<Action>)> {
        vec![
            (key(BareKey::Char('a')), vec![Action::CloseFocus]),
            (key(BareKey::Char('b')), vec![Action::ToggleFocusFullscreen]),
            (key(BareKey::Char('c')), vec![Action::TogglePaneFrames]),
            (key(BareKey::Char('d')), vec![Action::TogglePanePinned]),
        ]
    }

    #[test]
    fn hints_are_dropped_from_the_right_to_fit() {
        let all = rendered(InputMode::Pane, &four_hints(), &[]);
        assert_eq!(all, "a close|b fullscreen|c frames|d pin");
        // Enough room for the first two only.
        let fitted = rendered(InputMode::Pane, &four_hints(), &[("limit", "20")]);
        assert_eq!(fitted, "a close|b fullscreen");
    }

    #[test]
    fn unpinned_hints_yield_before_hints_pinned_to_the_end() {
        // `pin` is pinned last, so the unpinned middle drops around it.
        let fitted = rendered(
            InputMode::Pane,
            &four_hints(),
            &[("hint_order", "*, pin"), ("limit", "20")],
        );
        assert_eq!(fitted, "a close|d pin");
    }

    #[test]
    fn hints_pinned_to_the_front_are_kept_too() {
        let fitted = rendered(
            InputMode::Pane,
            &four_hints(),
            &[("hint_order", "frames, *, pin"), ("limit", "20")],
        );
        assert_eq!(fitted, "c frames|d pin");
    }

    #[test]
    fn the_leading_group_is_given_up_before_the_trailing_one() {
        // `close, fullscreen, *, pin`: once the `*` is gone the leading group is
        // consumed from its inner edge outward, so the trailing `pin` — the hint
        // most deliberately placed — is the last one standing.
        let order = ("hint_order", "close, fullscreen, *, pin");
        assert_eq!(
            rendered(InputMode::Pane, &four_hints(), &[order, ("limit", "26")]),
            "a close|b fullscreen|d pin"
        );
        assert_eq!(
            rendered(InputMode::Pane, &four_hints(), &[order, ("limit", "20")]),
            "a close|d pin"
        );
        assert_eq!(
            rendered(InputMode::Pane, &four_hints(), &[order, ("limit", "8")]),
            "d pin"
        );
    }

    #[test]
    fn precedence_lt_keeps_the_leading_group_instead() {
        let order = ("hint_order", "close, fullscreen, *, pin");
        let lt = ("hint_precedence", "lt");
        // The trailing `pin` is spent first now, so the leading pair outlives it.
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[order, lt, ("limit", "22")]
            ),
            "a close|b fullscreen"
        );
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[order, lt, ("limit", "10")]
            ),
            "a close"
        );
    }

    #[test]
    fn precedence_defaults_to_keeping_the_trailing_group() {
        let order = ("hint_order", "close, fullscreen, *, pin");
        let explicit = ("hint_precedence", "tl");
        let limit = ("limit", "10");
        assert_eq!(
            rendered(InputMode::Pane, &four_hints(), &[order, explicit, limit]),
            rendered(InputMode::Pane, &four_hints(), &[order, limit])
        );
        assert_eq!(
            rendered(InputMode::Pane, &four_hints(), &[order, limit]),
            "d pin"
        );
    }

    #[test]
    fn precedence_still_leaves_the_dropped_hints_in_one_run() {
        // Whichever group is spent first, the gap stays contiguous.
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[
                    ("hint_order", "close, fullscreen, *, pin"),
                    ("hint_precedence", "lt"),
                    ("drop_indicator", "…"),
                    ("limit", "22")
                ]
            ),
            "a close|b fullscreen|…"
        );
    }

    #[test]
    fn dropped_hints_stay_one_run_so_a_single_indicator_covers_them() {
        // Each drop extends the same gap rather than opening a new one, so the
        // indicator never needs a twin.
        let order = ("hint_order", "close, fullscreen, *, pin");
        let indicator = ("drop_indicator", "…");
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[order, indicator, ("limit", "30")]
            ),
            "a close|b fullscreen|…|d pin"
        );
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[order, indicator, ("limit", "20")]
            ),
            "a close|…|d pin"
        );
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[order, indicator, ("limit", "10")]
            ),
            "…|d pin"
        );
    }

    #[test]
    fn the_indicator_marks_where_hints_were_dropped() {
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[("drop_indicator", "…"), ("limit", "22")]
            ),
            "a close|b fullscreen|…"
        );
    }

    #[test]
    fn no_indicator_appears_when_everything_fits() {
        assert_eq!(
            rendered(
                InputMode::Pane,
                &four_hints(),
                &[("drop_indicator", "…"), ("limit", "100")]
            ),
            "a close|b fullscreen|c frames|d pin"
        );
    }

    #[test]
    fn the_indicator_is_paid_for_out_of_the_same_budget() {
        // A wide indicator costs room of its own, so it forces a further drop
        // rather than pushing the line over the limit.
        let wide = rendered(
            InputMode::Pane,
            &four_hints(),
            &[("drop_indicator", "<more>"), ("limit", "22")],
        );
        assert!(
            calculate_visible_length(&wide, false) <= 22,
            "over the limit: {:?}",
            wide
        );
        assert!(wide.contains("<more>"), "indicator missing: {:?}", wide);
    }

    #[test]
    fn one_hint_always_survives_for_truncation_to_handle() {
        let fitted = rendered(InputMode::Pane, &four_hints(), &[("limit", "2")]);
        assert_eq!(fitted, "a close");
    }

    #[test]
    fn width_counts_columns_not_characters() {
        // A CJK ideograph is two columns in any terminal.
        assert_eq!(calculate_visible_length("ab", false), 2);
        assert_eq!(calculate_visible_length("字", false), 2);
        assert_eq!(calculate_visible_length("a字b", false), 4);
    }

    #[test]
    fn nerd_font_glyphs_are_ambiguous_width() {
        // U+F060 is the arrow glyph used for `key_alias_left`. It is East Asian
        // Ambiguous: one column by the standard, two in a terminal set up for
        // Nerd Fonts.
        assert_eq!(calculate_visible_length("\u{f060}", false), 1);
        assert_eq!(calculate_visible_length("\u{f060}", true), 2);
    }

    #[test]
    fn escape_sequences_take_no_columns() {
        assert_eq!(calculate_visible_length(&styled_line(), false), 8);
        assert_eq!(calculate_visible_length(&styled_line(), true), 8);
    }

    #[test]
    fn wide_glyphs_are_measured_when_fitting_hints() {
        // Two hints whose labels are wide glyphs. Counting characters would call
        // this 4 columns of label; it is really 8.
        let keymap = vec![
            (key(BareKey::Char('a')), vec![Action::CloseFocus]),
            (key(BareKey::Char('b')), vec![Action::ToggleFocusFullscreen]),
        ];
        // Distinct labels, or sharing one would deliberately fuse them.
        let labels: Vec<(&str, &str)> =
            vec![("label_close_pane", "字左"), ("label_fullscreen", "字右")];
        // Each hint is "a " plus a 4-column label = 6; with the 1-column spacer
        // the pair is 13 columns, though only 9 characters.
        assert_eq!(rendered(InputMode::Pane, &keymap, &labels), "a 字左|b 字右");
        // 12 columns cannot hold both, even though 9 characters would fit.
        let mut fitted = labels.clone();
        fitted.push(("limit", "12"));
        assert_eq!(rendered(InputMode::Pane, &keymap, &fitted), "a 字左");
    }

    fn pane(x: usize, columns: usize, floating: bool) -> PaneInfo {
        PaneInfo {
            pane_x: x,
            pane_columns: columns,
            is_floating: floating,
            ..Default::default()
        }
    }

    fn suppressed_pane(x: usize, columns: usize) -> PaneInfo {
        PaneInfo {
            pane_x: x,
            pane_columns: columns,
            is_suppressed: true,
            ..Default::default()
        }
    }

    fn manifest(panes: Vec<PaneInfo>) -> PaneManifest {
        let mut map = HashMap::new();
        map.insert(0, panes);
        PaneManifest { panes: map }
    }

    #[test]
    fn terminal_width_is_the_right_edge_of_the_widest_pane() {
        // Two panes side by side across an 80-column terminal.
        let panes = vec![pane(0, 40, false), pane(40, 40, false)];
        assert_eq!(terminal_width(&manifest(panes)), Some(80));
    }

    #[test]
    fn floating_panes_do_not_define_the_terminal_width() {
        let panes = vec![pane(0, 80, false), pane(10, 30, true)];
        assert_eq!(terminal_width(&manifest(panes)), Some(80));
    }

    #[test]
    fn suppressed_panes_do_not_define_the_terminal_width() {
        // Suppressed panes stop tracking resizes, so their stale geometry is
        // often the widest. Trusting it pins the width to an old value and the
        // hints are never refitted.
        let panes = vec![
            pane(0, 103, false),
            suppressed_pane(53, 106),
            suppressed_pane(53, 106),
        ];
        assert_eq!(terminal_width(&manifest(panes)), Some(103));
    }

    #[test]
    fn width_is_unknown_when_there_is_nothing_to_measure() {
        assert_eq!(terminal_width(&manifest(vec![])), None);
        assert_eq!(terminal_width(&manifest(vec![pane(0, 0, false)])), None);
    }

    fn limits(
        max_length: usize,
        auto: bool,
        reserve: usize,
        width: Option<usize>,
    ) -> Option<usize> {
        State {
            max_length,
            auto_width: auto,
            reserve_columns: reserve,
            terminal_width: width,
            ..Default::default()
        }
        .length_limit()
    }

    #[test]
    fn auto_width_fits_the_hints_to_the_terminal() {
        assert_eq!(limits(0, true, 0, Some(80)), Some(80));
    }

    #[test]
    fn reserved_columns_are_kept_free_for_the_rest_of_the_bar() {
        assert_eq!(limits(0, true, 30, Some(80)), Some(50));
        // A reserve wider than the terminal floors at zero rather than wrapping.
        assert_eq!(limits(0, true, 200, Some(80)), Some(0));
    }

    #[test]
    fn an_explicit_max_length_is_never_exceeded_on_a_wide_terminal() {
        assert_eq!(limits(40, true, 0, Some(200)), Some(40));
    }

    #[test]
    fn auto_width_still_narrows_below_an_explicit_max_length() {
        assert_eq!(limits(100, true, 0, Some(60)), Some(60));
    }

    #[test]
    fn nothing_is_capped_before_the_first_pane_update() {
        // Auto-fitting waits for a real width rather than guessing one.
        assert_eq!(limits(0, true, 0, None), None);
        assert_eq!(limits(40, true, 0, None), Some(40));
    }

    #[test]
    fn auto_width_off_leaves_only_the_explicit_cap() {
        assert_eq!(limits(0, false, 0, Some(80)), None);
        assert_eq!(limits(40, false, 0, Some(80)), Some(40));
    }

    fn label_config(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_mode_scoped_label_beats_the_global_one() {
        let config = label_config(&[
            ("label_mode_normal", "exit"),
            ("label_locked_mode_normal", "unlock"),
        ]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "locked", &config),
            Some(Some("unlock".to_string()))
        );
        // Every other mode still gets the global label.
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "pane", &config),
            Some(Some("exit".to_string()))
        );
    }

    #[test]
    fn a_mode_scoped_label_works_by_action_signature_too() {
        let config = label_config(&[("label_locked_switch_to_mode_normal", "unlock")]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "locked", &config),
            Some(Some("unlock".to_string()))
        );
    }

    #[test]
    fn concept_id_wins_over_signature_at_the_same_scope() {
        let config = label_config(&[
            ("label_mode_normal", "by id"),
            ("label_switch_to_mode_normal", "by signature"),
        ]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "pane", &config),
            Some(Some("by id".to_string()))
        );
    }

    #[test]
    fn a_mode_scoped_empty_label_hides_the_hint_in_that_mode_only() {
        let config = label_config(&[("label_pane_mode_normal", "")]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "pane", &config),
            Some(None)
        );
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "tab", &config),
            None
        );
    }

    #[test]
    fn a_mode_scope_can_reinstate_a_hint_hidden_globally() {
        let config = label_config(&[
            ("label_mode_normal", ""),
            ("label_locked_mode_normal", "unlock"),
        ]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "locked", &config),
            Some(Some("unlock".to_string()))
        );
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "tab", &config),
            Some(None)
        );
    }

    #[test]
    fn no_matching_label_leaves_the_builtin_in_place() {
        let config = label_config(&[("label_quit", "bye")]);
        assert_eq!(
            label_override("mode_normal", "switch_to_mode_normal", "locked", &config),
            None
        );
    }

    #[test]
    fn keys_sharing_a_modifier_stay_in_one_contiguous_run() {
        let mut keys = vec![
            KeyWithModifier::new(BareKey::Char('j')),
            KeyWithModifier::new(BareKey::Char('h')).with_ctrl_modifier(),
            KeyWithModifier::new(BareKey::Char('h')),
            KeyWithModifier::new(BareKey::Char('j')).with_ctrl_modifier(),
        ];
        sort_keys(&mut keys, KeyOrder::Qwerty);
        let rendered: Vec<String> = keys
            .iter()
            .map(|k| {
                let prefix = if k.key_modifiers.is_empty() { "" } else { "^" };
                match k.bare_key {
                    BareKey::Char(c) => format!("{}{}", prefix, c),
                    _ => "?".to_string(),
                }
            })
            .collect();
        assert_eq!(rendered, vec!["h", "j", "^h", "^j"]);
    }
}
