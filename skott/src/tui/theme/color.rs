//! Theme color parsing: CSS Color Module Level 4 named colors (e.g.
//! `"black"`, `"tomato"`, `"rebeccapurple"` — the full list is standardized at
//! <https://www.w3.org/TR/css-color-4/#named-colors>) and RGB hex values
//! (`"#1a1a2e"`), used interchangeably anywhere a theme file expects a color.
//!
//! Parsing goes through the `csscolorparser` crate rather than a hand-rolled
//! name table, so the recognised names come from a maintained, standard
//! source instead of being invented by Kvist. Every resolved color is a fixed
//! truecolor RGB triple: a theme's colors always render the same regardless
//! of the terminal's own 16/256-color palette, which is what makes a theme
//! file fully explicit.

use ratatui::style::Color;

/// Parses one color value: a CSS named color or an RGB hex string.
///
/// Returns an error naming the offending value for a theme file's `path` and
/// `item` (e.g. `"panel.bg"`), so a typo is easy to locate.
pub fn parse_color(path: &str, item: &str, spec: &str) -> Result<Color, String> {
    let trimmed = spec.trim();
    let parsed = csscolorparser::parse(trimmed)
        .map_err(|error| format!("{path}: `{item}` has an invalid color `{spec}`: {error}"))?;
    let [r, g, b, a] = parsed.to_rgba8();
    if a != 255 {
        return Err(format!(
            "{path}: `{item}` color `{spec}` is not fully opaque (alpha {a}); theme colors must \
             be opaque so a declared background can never be hidden by transparency"
        ));
    }
    Ok(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_named_color() {
        assert_eq!(
            parse_color("t", "x", "rebeccapurple").unwrap(),
            Color::Rgb(102, 51, 153)
        );
    }

    #[test]
    fn accepts_hex_rgb() {
        assert_eq!(
            parse_color("t", "x", "#1a1a2e").unwrap(),
            Color::Rgb(26, 26, 46)
        );
    }

    #[test]
    fn rejects_unknown_names() {
        let err = parse_color("t", "x", "not-a-color").unwrap_err();
        assert!(err.contains("invalid color"), "{err}");
    }

    #[test]
    fn rejects_transparency() {
        let err = parse_color("t", "x", "rgba(0, 0, 0, 0.5)").unwrap_err();
        assert!(err.contains("opaque"), "{err}");
    }
}
