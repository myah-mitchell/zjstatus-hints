//! Parsing of zjstatus-style format strings.
//!
//! This mirrors the styling syntax used by [zjstatus](https://github.com/dj95/zjstatus)
//! so that hint styles can be configured in exactly the same way as any other
//! zjstatus widget. A format string is a sequence of literal text and `#[...]`
//! styling directives, e.g.:
//!
//! ```text
//! #[fg=$black,bg=$blue,bold] {key} #[fg=#cdd6f4,bg=#1e1e2e] {desc}
//! ```
//!
//! Everything after a `#[...]` block (up to the next block) is painted with the
//! style described by that block. Supported directives, matching zjstatus:
//!
//! - `fg=<color>` / `bg=<color>`: foreground / background color
//! - Effects: `bold`, `italic`/`italics`, `underscore`, `blink`, `hidden`,
//!   `dim`, `strikethrough`, `reverse`
//!
//! Colors accept the same forms as zjstatus:
//!
//! - `$alias`: looks up `color_<alias>` from the plugin configuration
//! - `#RRGGBB`: hex RGB
//! - a named color (`red`, `bright_black`, ...)
//! - `colour<N>` / `color<N>` or a bare `0`-`255`: an ANSI 256 color index
//!
//! Effects and color forms that the underlying `ansi_term` renderer cannot
//! express (e.g. `us=` underline colors, curly/dotted underlines) are parsed
//! but ignored, so a config shared with zjstatus never errors.

use ansi_term::{
    ANSIString,
    Colour::{self, Fixed, RGB},
    Style,
};
use std::collections::BTreeMap;

/// Render a format-string `template` into styled segments, substituting each
/// `{name}` placeholder in `values` with its plain-text value beforehand.
///
/// `config` is the plugin configuration, used to resolve `$alias` colors via
/// `color_<alias>` keys (the same mechanism zjstatus uses).
pub fn render_template(
    template: &str,
    values: &[(&str, &str)],
    config: &BTreeMap<String, String>,
) -> Vec<ANSIString<'static>> {
    let mut substituted = template.to_string();
    for (name, value) in values {
        substituted = substituted.replace(&format!("{{{}}}", name), value);
    }
    parse_format(&substituted, config)
}

/// Parse a format string into styled segments. Text before the first `#[...]`
/// block is emitted unstyled; each block sets the style for the text following
/// it, up to the next block.
fn parse_format(template: &str, config: &BTreeMap<String, String>) -> Vec<ANSIString<'static>> {
    let mut parts: Vec<ANSIString<'static>> = vec![];

    for (idx, segment) in template.split("#[").enumerate() {
        if idx == 0 {
            // Text preceding the first styling block has no directives.
            if !segment.is_empty() {
                parts.push(Style::default().paint(segment.to_string()));
            }
            continue;
        }

        // `segment` is `<directives>]<text>`. A missing `]` means the block has
        // no trailing text (just directives), which we simply drop.
        if let Some((directives, text)) = segment.split_once(']') {
            let style = parse_style(directives, config);
            if !text.is_empty() {
                parts.push(style.paint(text.to_string()));
            }
        }
    }

    parts
}

/// Parse the comma-separated directives inside a `#[...]` block into a style.
fn parse_style(directives: &str, config: &BTreeMap<String, String>) -> Style {
    let mut style = Style::new();

    for directive in directives.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }

        if let Some(color) = directive.strip_prefix("fg=") {
            if let Some(color) = parse_color(color, config) {
                style = style.fg(color);
            }
        } else if let Some(color) = directive.strip_prefix("bg=") {
            if let Some(color) = parse_color(color, config) {
                style = style.on(color);
            }
        } else if directive.starts_with("us=") {
            // Underline color: parsed for compatibility but unsupported by the
            // renderer, so intentionally ignored.
        } else {
            style = apply_effect(style, directive);
        }
    }

    style
}

/// Apply a single text-effect directive to `style`, ignoring unknown effects.
fn apply_effect(style: Style, effect: &str) -> Style {
    match effect {
        "bold" => style.bold(),
        "italic" | "italics" => style.italic(),
        // zjstatus spells underline "underscore"; accept "underline" too.
        "underscore" | "underline" | "double-underscore" | "curly-underscore"
        | "dotted-underscore" | "dashed-underscore" => style.underline(),
        "blink" => style.blink(),
        "hidden" => style.hidden(),
        "dim" => style.dimmed(),
        "strikethrough" => style.strikethrough(),
        "reverse" => style.reverse(),
        _ => style,
    }
}

/// Parse a color string using the same rules as zjstatus.
fn parse_color(color: &str, config: &BTreeMap<String, String>) -> Option<Colour> {
    let color = color.trim();

    // `$alias` resolves to the `color_<alias>` configuration value.
    let color = if let Some(alias) = color.strip_prefix('$') {
        config.get(&format!("color_{}", alias))?.as_str()
    } else {
        color
    };

    if let Some(hex) = color.strip_prefix('#') {
        return hex_to_rgb(hex);
    }

    if let Some(named) = color_by_name(color) {
        return Some(named);
    }

    let index = color
        .strip_prefix("colour")
        .or_else(|| color.strip_prefix("color"))
        .unwrap_or(color);
    if let Ok(n) = index.parse::<u8>() {
        return Some(Fixed(n));
    }

    None
}

/// Parse a `RRGGBB` hex string (without a leading `#`) into an RGB color.
fn hex_to_rgb(hex: &str) -> Option<Colour> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(RGB(r, g, b))
}

/// Map a named color to an `ansi_term` color, matching zjstatus's names.
fn color_by_name(name: &str) -> Option<Colour> {
    let color = match name {
        "black" => Colour::Black,
        "red" => Colour::Red,
        "green" => Colour::Green,
        "yellow" => Colour::Yellow,
        "blue" => Colour::Blue,
        // ansi_term names ANSI magenta "Purple".
        "magenta" | "purple" => Colour::Purple,
        "cyan" => Colour::Cyan,
        "white" => Colour::White,
        "bright_black" | "bright-black" => Fixed(8),
        "bright_red" | "bright-red" => Fixed(9),
        "bright_green" | "bright-green" => Fixed(10),
        "bright_yellow" | "bright-yellow" => Fixed(11),
        "bright_blue" | "bright-blue" => Fixed(12),
        "bright_magenta" | "bright-magenta" => Fixed(13),
        "bright_cyan" | "bright-cyan" => Fixed(14),
        "bright_white" | "bright-white" => Fixed(15),
        _ => return None,
    };
    Some(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_hex_and_named_and_indexed_colors() {
        let config = cfg(&[]);
        assert_eq!(parse_color("#010203", &config), Some(RGB(1, 2, 3)));
        assert_eq!(parse_color("red", &config), Some(Colour::Red));
        assert_eq!(parse_color("magenta", &config), Some(Colour::Purple));
        assert_eq!(parse_color("bright_black", &config), Some(Fixed(8)));
        assert_eq!(parse_color("5", &config), Some(Fixed(5)));
        assert_eq!(parse_color("colour200", &config), Some(Fixed(200)));
        assert_eq!(parse_color("nonsense", &config), None);
        assert_eq!(parse_color("#12", &config), None);
    }

    #[test]
    fn resolves_color_aliases() {
        let config = cfg(&[("color_accent", "#89b4fa")]);
        assert_eq!(parse_color("$accent", &config), Some(RGB(0x89, 0xb4, 0xfa)));
        assert_eq!(parse_color("$missing", &config), None);
    }

    #[test]
    fn template_substitutes_placeholders_and_styles() {
        let config = cfg(&[]);
        let parts = render_template(
            "#[fg=red,bold] {key} #[fg=white] {desc} ",
            &[("key", "Ctrl + p"), ("desc", "pane")],
            &config,
        );
        // Rendered ANSI should contain the substituted, styled text.
        let rendered = ansi_term::ANSIStrings(&parts).to_string();
        assert!(rendered.contains("Ctrl + p"));
        assert!(rendered.contains("pane"));
        // Bold (SGR 1) and red foreground (SGR 31) should appear.
        assert!(rendered.contains("1;31") || rendered.contains("31;1"));
    }

    #[test]
    fn leading_literal_text_is_unstyled() {
        let config = cfg(&[]);
        let parts = render_template("x#[fg=red]y", &[], &config);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].to_string(), "x");
    }
}
