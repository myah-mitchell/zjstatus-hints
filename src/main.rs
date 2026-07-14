use ansi_term::{
    ANSIString, ANSIStrings,
    Colour::{Fixed, RGB},
    Style,
};
use std::collections::BTreeMap;
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
    overflow_str: String,
    hide_in_base_mode: bool,
    key_format: Option<String>,
    desc_format: Option<String>,
    discover_hints: bool,
    config: BTreeMap<String, String>,
}

register_plugin!(State);

const TO_NORMAL: Action = Action::SwitchToMode(InputMode::Normal);

const PLUGIN_SESSION_MANAGER: &str = "session-manager";
const PLUGIN_CONFIGURATION: &str = "configuration";
const PLUGIN_MANAGER: &str = "plugin-manager";
const PLUGIN_ABOUT: &str = "zellij:about";

const KEY_PATTERNS_NO_SEPARATOR: &[&str] = &["HJKL", "hjkl", "←↓↑→", "←→", "↓↑", "[]"];

const DEFAULT_MAX_LENGTH: usize = 0;
const DEFAULT_OVERFLOW_STR: &str = "...";
const DEFAULT_PIPE_NAME: &str = "zjstatus_hints";

const CONFIG_KEY_FORMAT: &str = "key_format";
const CONFIG_DESC_FORMAT: &str = "desc_format";
const CONFIG_KEY_ALIAS_PREFIX: &str = "key_alias_";
const CONFIG_LABEL_PREFIX: &str = "label_";
const CONFIG_DISCOVER_HINTS: &str = "discover_hints";

const DEFAULT_DISCOVER_HINTS: bool = true;

type ActionLabel = (Action, &'static str);
type ActionSequenceLabel = (&'static [Action], &'static str);

/// How each hint should be styled while rendering. Borrows from `State` so the
/// styling functions can fall back to the theme palette when no format string
/// is configured, and resolve `$alias` colors from the plugin configuration.
struct HintStyle<'a> {
    colors: &'a Styling,
    key_format: Option<&'a str>,
    desc_format: Option<&'a str>,
    discover: bool,
    config: &'a BTreeMap<String, String>,
}

const NORMAL_MODE_ACTIONS: &[ActionLabel] = &[
    (Action::SwitchToMode(InputMode::Pane), "pane"),
    (Action::SwitchToMode(InputMode::Tab), "tab"),
    (Action::SwitchToMode(InputMode::Resize), "resize"),
    (Action::SwitchToMode(InputMode::Move), "move"),
    (Action::SwitchToMode(InputMode::Scroll), "scroll"),
    (Action::SwitchToMode(InputMode::Search), "search"),
    (Action::SwitchToMode(InputMode::Session), "session"),
    (Action::Quit, "quit"),
];

const PANE_MODE_ACTION_SEQUENCES: &[ActionSequenceLabel] = &[
    (&[Action::NewPane(None, None, false), TO_NORMAL], "new"),
    (&[Action::CloseFocus, TO_NORMAL], "close"),
    (&[Action::ToggleFocusFullscreen, TO_NORMAL], "fullscreen"),
    (&[Action::ToggleFloatingPanes, TO_NORMAL], "float"),
    (&[Action::TogglePaneEmbedOrFloating, TO_NORMAL], "embed"),
    (
        &[
            Action::NewPane(Some(Direction::Right), None, false),
            TO_NORMAL,
        ],
        "split right",
    ),
    (
        &[
            Action::NewPane(Some(Direction::Down), None, false),
            TO_NORMAL,
        ],
        "split down",
    ),
];

const TAB_MODE_ACTION_SEQUENCES: &[ActionSequenceLabel] = &[
    (
        &[
            Action::NewTab(None, vec![], None, None, None, true),
            TO_NORMAL,
        ],
        "new",
    ),
    (&[Action::CloseTab, TO_NORMAL], "close"),
    (&[Action::BreakPane, TO_NORMAL], "break pane"),
    (&[Action::ToggleActiveSyncTab, TO_NORMAL], "sync"),
];

fn get_common_modifiers(mut key_bindings: Vec<&KeyWithModifier>) -> Vec<KeyModifier> {
    if key_bindings.is_empty() {
        return vec![];
    }
    let mut common_modifiers = key_bindings.pop().unwrap().key_modifiers.clone();
    for key in key_bindings {
        common_modifiers = common_modifiers
            .intersection(&key.key_modifiers)
            .cloned()
            .collect();
    }
    common_modifiers.into_iter().collect()
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
        // Retained so `$alias` colors and `label_<action>` overrides can be
        // resolved from the raw configuration.
        self.config = configuration;

        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::MessageAndLaunchOtherPlugins,
        ]);

        set_selectable(false);
        subscribe(&[EventType::ModeUpdate, EventType::SessionUpdate]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = !self.initialized;
        if let Event::ModeUpdate(mode_info) = event {
            if self.mode_info != mode_info {
                should_render = true;
            }
            self.mode_info = mode_info;
            self.base_mode_is_locked = self.mode_info.base_mode == Some(InputMode::Locked);
        };
        should_render
    }

    fn render(&mut self, _rows: usize, _cols: usize) {
        let mode_info = &self.mode_info;
        let output = if !(self.hide_in_base_mode && Some(mode_info.mode) == mode_info.base_mode) {
            let keymap = get_keymap_for_mode(mode_info);
            let ctx = HintStyle {
                colors: &mode_info.style.colors,
                key_format: self.key_format.as_deref(),
                desc_format: self.desc_format.as_deref(),
                discover: self.discover_hints,
                config: &self.config,
            };
            let parts = render_hints_for_mode(mode_info.mode, &keymap, &ctx);

            let ansi_strings = ANSIStrings(&parts);
            let formatted = format!(" {}", ansi_strings);

            let visible_len = calculate_visible_length(&formatted);
            if self.max_length > 0 && visible_len > self.max_length {
                truncate_ansi_string(&formatted, &self.overflow_str, self.max_length)
            } else {
                formatted.to_string()
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

fn calculate_visible_length(text: &str) -> usize {
    let mut parser = AnsiParser::new(text);
    let mut len = 0;

    while let Some(segment) = parser.next_segment() {
        if matches!(segment, AnsiSegment::VisibleChar(_)) {
            len += 1;
        }
    }

    len
}

fn truncate_ansi_string(text: &str, overflow_str: &str, max_len: usize) -> String {
    let visible_len = calculate_visible_length(text);
    let overflow_len = overflow_str.len();

    if visible_len <= max_len {
        return text.to_string();
    }

    if max_len <= overflow_len {
        return overflow_str.to_string();
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

fn format_modifier_string(modifiers: &[KeyModifier]) -> String {
    if modifiers.is_empty() {
        String::new()
    } else {
        modifiers
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join("-")
    }
}

fn format_key_display(
    key_bindings: &[KeyWithModifier],
    common_modifiers: &[KeyModifier],
    config: &BTreeMap<String, String>,
) -> Vec<String> {
    key_bindings
        .iter()
        .map(|key| {
            let unique_modifiers = key
                .key_modifiers
                .iter()
                .filter(|m| !common_modifiers.contains(m))
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            let bare = format_bare_key(&key.bare_key, config);
            if unique_modifiers.is_empty() {
                bare
            } else {
                format!("{} {}", unique_modifiers, bare)
            }
        })
        .collect()
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

fn get_key_separator(key_display: &[String]) -> &'static str {
    let key_string = key_display.join("");
    if KEY_PATTERNS_NO_SEPARATOR.contains(&&key_string[..]) {
        ""
    } else {
        "|"
    }
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
    let mut styled_parts = vec![];

    let common_modifiers = get_common_modifiers(key_bindings.iter().collect());
    let modifier_str = format_modifier_string(&common_modifiers);
    let key_display = format_key_display(key_bindings, &common_modifiers, config);
    let key_separator = get_key_separator(&key_display);

    styled_parts.push(Style::new().paint(" "));

    if !modifier_str.is_empty() {
        styled_parts.push(
            Style::new()
                .fg(contrasting_fg)
                .on(saturated_bg)
                .bold()
                .paint(format!(" {} + ", modifier_str)),
        );
    } else {
        styled_parts.push(Style::new().fg(contrasting_fg).on(saturated_bg).paint(" "));
    }

    for (idx, key) in key_display.iter().enumerate() {
        if idx > 0 && !key_separator.is_empty() {
            styled_parts.push(
                Style::new()
                    .fg(contrasting_fg)
                    .on(saturated_bg)
                    .paint(key_separator),
            );
        }
        styled_parts.push(
            Style::new()
                .fg(contrasting_fg)
                .on(saturated_bg)
                .bold()
                .paint(key.clone()),
        );
    }

    styled_parts.push(Style::new().fg(contrasting_fg).on(saturated_bg).paint(" "));

    styled_parts
}

/// Compose the plain-text representation of a key binding as it appears in a
/// hint, e.g. `Ctrl + p`, `h|j|k|l`, or `←↓↑→`. This is the value substituted
/// for the `{key}` placeholder when a custom `key_format` is configured; the
/// styling itself is left to the format string.
fn compose_key_text(key_bindings: &[KeyWithModifier], config: &BTreeMap<String, String>) -> String {
    if key_bindings.is_empty() {
        return String::new();
    }

    let common_modifiers = get_common_modifiers(key_bindings.iter().collect());
    let modifier_str = format_modifier_string(&common_modifiers);
    let key_display = format_key_display(key_bindings, &common_modifiers, config);
    let key_separator = get_key_separator(&key_display);
    let keys = key_display.join(key_separator);

    if modifier_str.is_empty() {
        keys
    } else {
        format!("{} + {}", modifier_str, keys)
    }
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

fn get_select_key(keymap: &[(KeyWithModifier, Vec<Action>)]) -> Vec<KeyWithModifier> {
    let to_normal_keys = find_keys_for_actions(keymap, &[TO_NORMAL], true);
    if to_normal_keys.contains(&KeyWithModifier::new(BareKey::Enter)) {
        vec![KeyWithModifier::new(BareKey::Enter)]
    } else {
        to_normal_keys.into_iter().take(1).collect()
    }
}

/// Render a single hint (key + description) into `parts`, recording the keys it
/// consumed in `used` so later discovery does not render them a second time.
fn add_hint(
    parts: &mut Vec<ANSIString<'static>>,
    keys: &[KeyWithModifier],
    description: &str,
    style: &HintStyle,
    used: &mut Vec<KeyWithModifier>,
) {
    if keys.is_empty() {
        return;
    }
    render_hint(parts, keys, description, style);
    used.extend(keys.iter().cloned());
}

/// Render a hint's styled segments, without tracking consumed keys.
fn render_hint(
    parts: &mut Vec<ANSIString<'static>>,
    keys: &[KeyWithModifier],
    description: &str,
    style: &HintStyle,
) {
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
    let mut parts = vec![];
    // Keys consumed by curated hints, so discovery does not repeat them.
    let mut used: Vec<KeyWithModifier> = vec![];
    let select_keys = get_select_key(keymap);

    match mode {
        InputMode::Normal => {
            for (action, label) in NORMAL_MODE_ACTIONS {
                let keys = find_keys_for_actions(keymap, &[action.clone()], true);
                add_hint(&mut parts, &keys, label, style, &mut used);
            }
        }
        InputMode::Pane => {
            for (actions, label) in PANE_MODE_ACTION_SEQUENCES {
                let keys = find_keys_for_actions(keymap, actions, false);
                if !keys.is_empty() {
                    add_hint(&mut parts, &keys, label, style, &mut used);
                }
            }

            let rename_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode(InputMode::RenamePane),
                    Action::PaneNameInput(vec![0]),
                ],
                false,
            );
            if !rename_keys.is_empty() {
                add_hint(&mut parts, &rename_keys, "rename", style, &mut used);
            }

            let focus_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::MoveFocus(Direction::Left)],
                    &[Action::MoveFocus(Direction::Down)],
                    &[Action::MoveFocus(Direction::Up)],
                    &[Action::MoveFocus(Direction::Right)],
                ],
            );
            add_hint(&mut parts, &focus_keys, "move", style, &mut used);
            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Tab => {
            for (actions, label) in TAB_MODE_ACTION_SEQUENCES {
                let keys = find_keys_for_actions(keymap, actions, false);
                if !keys.is_empty() {
                    add_hint(&mut parts, &keys, label, style, &mut used);
                }
            }

            let rename_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode(InputMode::RenameTab),
                    Action::TabNameInput(vec![0]),
                ],
                false,
            );
            if !rename_keys.is_empty() {
                add_hint(&mut parts, &rename_keys, "rename", style, &mut used);
            }

            let focus_keys_full = find_keys_for_action_groups(
                keymap,
                &[&[Action::GoToPreviousTab], &[Action::GoToNextTab]],
            );
            let focus_keys = if focus_keys_full.contains(&KeyWithModifier::new(BareKey::Left))
                && focus_keys_full.contains(&KeyWithModifier::new(BareKey::Right))
            {
                vec![
                    KeyWithModifier::new(BareKey::Left),
                    KeyWithModifier::new(BareKey::Right),
                ]
            } else {
                focus_keys_full
            };
            add_hint(&mut parts, &focus_keys, "move", style, &mut used);
            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Resize => {
            let resize_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize(Resize::Increase, None)],
                    &[Action::Resize(Resize::Decrease, None)],
                ],
            );
            add_hint(&mut parts, &resize_keys, "resize", style, &mut used);

            let increase_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize(Resize::Increase, Some(Direction::Left))],
                    &[Action::Resize(Resize::Increase, Some(Direction::Down))],
                    &[Action::Resize(Resize::Increase, Some(Direction::Up))],
                    &[Action::Resize(Resize::Increase, Some(Direction::Right))],
                ],
            );
            add_hint(&mut parts, &increase_keys, "increase", style, &mut used);

            let decrease_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::Resize(Resize::Decrease, Some(Direction::Left))],
                    &[Action::Resize(Resize::Decrease, Some(Direction::Down))],
                    &[Action::Resize(Resize::Decrease, Some(Direction::Up))],
                    &[Action::Resize(Resize::Decrease, Some(Direction::Right))],
                ],
            );
            add_hint(&mut parts, &decrease_keys, "decrease", style, &mut used);
            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Move => {
            let move_keys = find_keys_for_action_groups(
                keymap,
                &[
                    &[Action::MovePane(Some(Direction::Left))],
                    &[Action::MovePane(Some(Direction::Down))],
                    &[Action::MovePane(Some(Direction::Up))],
                    &[Action::MovePane(Some(Direction::Right))],
                ],
            );
            add_hint(&mut parts, &move_keys, "move", style, &mut used);
            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Scroll => {
            let search_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode(InputMode::EnterSearch),
                    Action::SearchInput(vec![0]),
                ],
                true,
            );
            add_hint(&mut parts, &search_keys, "search", style, &mut used);

            let scroll_keys =
                find_keys_for_action_groups(keymap, &[&[Action::ScrollDown], &[Action::ScrollUp]]);
            add_hint(&mut parts, &scroll_keys, "scroll", style, &mut used);

            let page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::PageScrollDown], &[Action::PageScrollUp]],
            );
            add_hint(&mut parts, &page_scroll_keys, "page", style, &mut used);

            let half_page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::HalfPageScrollDown], &[Action::HalfPageScrollUp]],
            );
            add_hint(
                &mut parts,
                &half_page_scroll_keys,
                "half page",
                style,
                &mut used,
            );

            let edit_keys =
                find_keys_for_actions(keymap, &[Action::EditScrollback, TO_NORMAL], false);
            if !edit_keys.is_empty() {
                add_hint(&mut parts, &edit_keys, "edit", style, &mut used);
            }
            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Search => {
            let search_keys = find_keys_for_actions(
                keymap,
                &[
                    Action::SwitchToMode(InputMode::EnterSearch),
                    Action::SearchInput(vec![0]),
                ],
                true,
            );
            add_hint(&mut parts, &search_keys, "search", style, &mut used);

            let scroll_keys =
                find_keys_for_action_groups(keymap, &[&[Action::ScrollDown], &[Action::ScrollUp]]);
            add_hint(&mut parts, &scroll_keys, "scroll", style, &mut used);

            let page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::PageScrollDown], &[Action::PageScrollUp]],
            );
            add_hint(&mut parts, &page_scroll_keys, "page", style, &mut used);

            let half_page_scroll_keys = find_keys_for_action_groups(
                keymap,
                &[&[Action::HalfPageScrollDown], &[Action::HalfPageScrollUp]],
            );
            add_hint(
                &mut parts,
                &half_page_scroll_keys,
                "half page",
                style,
                &mut used,
            );

            let down_keys =
                find_keys_for_actions(keymap, &[Action::Search(SearchDirection::Down)], true);
            add_hint(&mut parts, &down_keys, "down", style, &mut used);

            let up_keys =
                find_keys_for_actions(keymap, &[Action::Search(SearchDirection::Up)], true);
            add_hint(&mut parts, &up_keys, "up", style, &mut used);

            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        InputMode::Session => {
            let detach_keys = find_keys_for_actions(keymap, &[Action::Detach], true);
            add_hint(&mut parts, &detach_keys, "detach", style, &mut used);

            if let Some(manager_key) = plugin_key(keymap, PLUGIN_SESSION_MANAGER) {
                add_hint(&mut parts, &[manager_key], "manager", style, &mut used);
            }

            if let Some(config_key) = plugin_key(keymap, PLUGIN_CONFIGURATION) {
                add_hint(&mut parts, &[config_key], "config", style, &mut used);
            }

            if let Some(plugin_key_val) = plugin_key(keymap, PLUGIN_MANAGER) {
                add_hint(&mut parts, &[plugin_key_val], "plugins", style, &mut used);
            }

            if let Some(about_key) = plugin_key(keymap, PLUGIN_ABOUT) {
                add_hint(&mut parts, &[about_key], "about", style, &mut used);
            }

            add_hint(&mut parts, &select_keys, "select", style, &mut used);
        }
        _ => {
            let keys =
                find_keys_for_actions(keymap, &[Action::SwitchToMode(InputMode::Normal)], true);
            add_hint(&mut parts, &keys, "normal", style, &mut used);
        }
    }

    // Append every other enabled keybinding in this mode that the curated
    // hints above didn't already cover.
    if style.discover {
        add_discovered_hints(&mut parts, keymap, &used, style);
    }

    parts
}

/// Discover and render all enabled keybindings that the curated hints didn't
/// already show. Keys are grouped by their resolved label (so families like the
/// directional focus keys collapse into a single hint), and each label is
/// resolved from, in order: a `label_<action>` config override, the built-in
/// label table, or a name derived from the action itself.
fn add_discovered_hints(
    parts: &mut Vec<ANSIString<'static>>,
    keymap: &[(KeyWithModifier, Vec<Action>)],
    used: &[KeyWithModifier],
    style: &HintStyle,
) {
    let mut groups: Vec<(String, Vec<KeyWithModifier>)> = vec![];

    for (key, actions) in keymap {
        if used.contains(key) {
            continue;
        }
        let Some(primary) = actions.first() else {
            continue;
        };
        if is_hidden_action(primary) {
            continue;
        }
        let Some(label) = resolve_label(primary, style.config) else {
            continue;
        };

        if let Some(group) = groups.iter_mut().find(|(l, _)| *l == label) {
            group.1.push(key.clone());
        } else {
            groups.push((label, vec![key.clone()]));
        }
    }

    for (label, keys) in &groups {
        render_hint(parts, keys, label, style);
    }
}

/// Resolve the label for a discovered keybinding's primary action. A user
/// `label_<action>` override wins; an empty override hides the hint. Otherwise
/// the built-in table is consulted, falling back to a name derived from the
/// action's own signature so nothing is ever left unlabeled.
fn resolve_label(action: &Action, config: &BTreeMap<String, String>) -> Option<String> {
    let signature = action_signature(action);
    if let Some(label) = config.get(&format!("{}{}", CONFIG_LABEL_PREFIX, signature)) {
        return if label.is_empty() {
            None
        } else {
            Some(label.clone())
        };
    }
    if let Some(label) = builtin_label(action) {
        return Some(label.to_string());
    }
    Some(signature.replace('_', " "))
}

/// A stable, configuration-friendly identifier for an action, used both as the
/// `label_<name>` config key and as the derived fallback label. It is the
/// snake_case variant name, with the target mode appended for mode switches
/// (e.g. `switch_to_mode_locked`) since that distinction is meaningful.
fn action_signature(action: &Action) -> String {
    let debug = format!("{:?}", action);
    let variant = debug
        .split(|c| c == '(' || c == '{' || c == ' ')
        .next()
        .unwrap_or(&debug);
    let mut signature = to_snake_case(variant);
    if let Action::SwitchToMode(mode) | Action::SwitchModeForAllClients(mode) = action {
        signature.push('_');
        signature.push_str(&to_snake_case(&format!("{:?}", mode)));
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
        Action::Write(..)
            | Action::WriteChars(..)
            | Action::PaneNameInput(..)
            | Action::TabNameInput(..)
            | Action::SearchInput(..)
            | Action::RenameTab(..)
            | Action::RenameTerminalPane(..)
            | Action::RenamePluginPane(..)
            | Action::NoOp
            | Action::MouseEvent(..)
            | Action::ScrollUpAt(..)
            | Action::ScrollDownAt(..)
            | Action::CliPipe { .. }
            | Action::KeybindPipe { .. }
            | Action::DumpScreen(..)
            | Action::DumpLayout
            | Action::ListClients
            | Action::QueryTabNames
            | Action::Run(..)
            | Action::SkipConfirm(..)
            | Action::StackPanes(..)
            | Action::ChangeFloatingPaneCoordinates(..)
    )
}

/// The built-in action -> label table. Returns `None` for actions without a
/// curated label, in which case the caller derives one from the signature.
/// Expand this to give more actions friendly, human-readable labels.
fn builtin_label(action: &Action) -> Option<&'static str> {
    if let Action::SwitchToMode(mode) | Action::SwitchModeForAllClients(mode) = action {
        return switch_mode_label(mode);
    }

    Some(match action {
        Action::Quit => "quit",
        Action::Detach => "detach",

        // Panes
        Action::NewPane(None, ..) => "new",
        Action::NewPane(Some(Direction::Left), ..) => "split left",
        Action::NewPane(Some(Direction::Right), ..) => "split right",
        Action::NewPane(Some(Direction::Up), ..) => "split up",
        Action::NewPane(Some(Direction::Down), ..) => "split down",
        Action::CloseFocus => "close",
        Action::ToggleFocusFullscreen => "fullscreen",
        Action::ToggleFloatingPanes => "float",
        Action::TogglePaneEmbedOrFloating => "embed",
        Action::TogglePaneFrames => "frames",
        Action::TogglePanePinned => "pin",
        Action::MoveFocus(_) | Action::MoveFocusOrTab(_) => "move",
        Action::MovePane(_) => "move",
        Action::MovePaneBackwards => "move back",
        Action::FocusNextPane => "next",
        Action::FocusPreviousPane => "prev",
        Action::SwitchFocus => "focus",

        // Tabs
        Action::NewTab(..) => "new",
        Action::CloseTab => "close",
        Action::GoToNextTab => "next",
        Action::GoToPreviousTab => "prev",
        Action::GoToTab(_) | Action::GoToTabName(..) => "tab",
        Action::ToggleTab => "toggle",
        Action::ToggleActiveSyncTab => "sync",
        Action::MoveTab(_) => "move tab",
        Action::BreakPane => "break pane",
        Action::BreakPaneLeft => "break left",
        Action::BreakPaneRight => "break right",

        // Resize
        Action::Resize(Resize::Increase, None) | Action::Resize(Resize::Decrease, None) => "resize",
        Action::Resize(Resize::Increase, Some(_)) => "increase",
        Action::Resize(Resize::Decrease, Some(_)) => "decrease",

        // Scroll / search
        Action::ScrollUp | Action::ScrollDown => "scroll",
        Action::PageScrollUp | Action::PageScrollDown => "page",
        Action::HalfPageScrollUp | Action::HalfPageScrollDown => "half page",
        Action::ScrollToTop => "top",
        Action::ScrollToBottom => "bottom",
        Action::EditScrollback => "edit",
        Action::Search(SearchDirection::Down) => "down",
        Action::Search(SearchDirection::Up) => "up",
        Action::SearchToggleOption(_) => "toggle",

        // Misc
        Action::Copy => "copy",
        Action::ClearScreen => "clear",
        Action::ToggleMouseMode => "mouse",
        Action::PreviousSwapLayout => "prev layout",
        Action::NextSwapLayout => "next layout",
        Action::Confirm => "confirm",
        Action::Deny => "deny",
        Action::RenameSession(_) => "rename",
        Action::UndoRenamePane | Action::UndoRenameTab => "undo",

        _ => return None,
    })
}

/// Label for a `SwitchToMode` action, based on the target mode.
fn switch_mode_label(mode: &InputMode) -> Option<&'static str> {
    Some(match mode {
        InputMode::Normal => "normal",
        InputMode::Locked => "lock",
        InputMode::Pane => "pane",
        InputMode::Tab => "tab",
        InputMode::Resize => "resize",
        InputMode::Move => "move",
        InputMode::Scroll => "scroll",
        InputMode::Search | InputMode::EnterSearch => "search",
        InputMode::Session => "session",
        InputMode::RenameTab | InputMode::RenamePane => "rename",
        InputMode::Tmux => "tmux",
        InputMode::Prompt => "prompt",
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
