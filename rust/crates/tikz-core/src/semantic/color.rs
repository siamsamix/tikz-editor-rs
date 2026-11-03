//! Colour resolution: the legacy `normalizeColor` subset.
//!
//! Port of `semantic/style/colors.ts` and the `NAMED_COLOR_HEX` table in
//! `semantic/style/constants.ts`. Supported forms:
//!
//! * `none` — no paint;
//! * a named colour (`blue`, `gray`, ...) — resolved to its hex value;
//! * `#rgb` / `#rrggbb`;
//! * mixes: `blue!20` (20% blue, 80% white) and `green!60!black`
//!   (60% first colour, 40% second). The percentage applies to the *first*
//!   colour and each channel is rounded, matching the legacy mix.
//!
//! ```
//! use tikz_core::scene::Paint;
//! use tikz_core::semantic::color::resolve_color;
//! assert_eq!(resolve_color("gray").unwrap(), Paint::Rgb(0x80, 0x80, 0x80));
//! assert_eq!(resolve_color("green!60!black").unwrap(), Paint::Rgb(0x00, 0x99, 0x00));
//! // 20% blue + 80% white: the ratio applies to the first colour.
//! assert_eq!(resolve_color("blue!20").unwrap(), Paint::Rgb(0xcc, 0xcc, 0xff));
//! assert_eq!(resolve_color("none").unwrap(), Paint::None);
//! ```

use crate::scene::Paint;

/// The named colours the legacy table defines, with their hex values.
const NAMED: [(&str, (u8, u8, u8)); 19] = [
    ("black", (0x00, 0x00, 0x00)),
    ("darkgray", (0x40, 0x40, 0x40)),
    ("white", (0xff, 0xff, 0xff)),
    ("gray", (0x80, 0x80, 0x80)),
    ("lightgray", (0xbf, 0xbf, 0xbf)),
    ("red", (0xff, 0x00, 0x00)),
    ("green", (0x00, 0xff, 0x00)),
    ("blue", (0x00, 0x00, 0xff)),
    ("cyan", (0x00, 0xff, 0xff)),
    ("magenta", (0xff, 0x00, 0xff)),
    ("yellow", (0xff, 0xff, 0x00)),
    ("lime", (0xbf, 0xff, 0x00)),
    ("olive", (0x80, 0x80, 0x00)),
    ("orange", (0xff, 0x80, 0x00)),
    ("pink", (0xff, 0xbf, 0xbf)),
    ("violet", (0x80, 0x00, 0x80)),
    ("purple", (0xbf, 0x00, 0x40)),
    ("teal", (0x00, 0x80, 0x80)),
    ("brown", (0xbf, 0x80, 0x40)),
];

/// Whether `name` is one of the legacy named colours.
pub fn is_named(name: &str) -> bool {
    lookup(name).is_some()
}

fn lookup(name: &str) -> Option<(u8, u8, u8)> {
    NAMED
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, rgb)| *rgb)
}

/// Resolve one colour expression (`none`, name, hex, or mix).
pub fn resolve_color(raw: &str) -> Result<Paint, String> {
    let text = raw.trim().to_ascii_lowercase();
    if text.is_empty() {
        return Err("empty colour".to_string());
    }
    if text == "none" {
        return Ok(Paint::None);
    }
    if let Some(rgb) = parse_hex(&text) {
        return Ok(Paint::Rgb(rgb.0, rgb.1, rgb.2));
    }
    let parts: Vec<&str> = text.split('!').collect();
    let base = lookup(parts[0]).ok_or_else(|| format!("unknown colour `{}`", parts[0].trim()))?;
    if parts.len() == 1 {
        return Ok(Paint::Rgb(base.0, base.1, base.2));
    }
    let percent: f64 = parts[1]
        .trim()
        .parse()
        .map_err(|_| format!("invalid colour percentage `{}`", parts[1].trim()))?;
    let second = match parts
        .get(2)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        None => (0xff, 0xff, 0xff),
        Some(name) => lookup(name).ok_or_else(|| format!("unknown colour `{name}`"))?,
    };
    let ratio = (percent / 100.0).clamp(0.0, 1.0);
    Ok(Paint::Rgb(
        mix(base.0, second.0, ratio),
        mix(base.1, second.1, ratio),
        mix(base.2, second.2, ratio),
    ))
}

/// Linear channel mix, rounded the way the legacy `rgbToHex` rounds.
fn mix(first: u8, second: u8, ratio: f64) -> u8 {
    (f64::from(first) * ratio + f64::from(second) * (1.0 - ratio))
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Parse `#rgb` / `#rrggbb`.
fn parse_hex(text: &str) -> Option<(u8, u8, u8)> {
    let digits = text.strip_prefix('#')?;
    let expand = |c: char| -> Option<u8> { u8::from_str_radix(&format!("{c}{c}"), 16).ok() };
    match digits.len() {
        3 => {
            let mut chars = digits.chars();
            let mut next = || chars.next().and_then(expand);
            Some((next()?, next()?, next()?))
        }
        6 => Some((
            u8::from_str_radix(&digits[0..2], 16).ok()?,
            u8::from_str_radix(&digits[2..4], 16).ok()?,
            u8::from_str_radix(&digits[4..6], 16).ok()?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_colors_match_the_legacy_table() {
        assert_eq!(
            resolve_color(" Purple ").unwrap(),
            Paint::Rgb(0xbf, 0x00, 0x40)
        );
        assert_eq!(resolve_color("lime").unwrap(), Paint::Rgb(0xbf, 0xff, 0x00));
        assert_eq!(
            resolve_color("orange").unwrap(),
            Paint::Rgb(0xff, 0x80, 0x00)
        );
    }

    #[test]
    fn mixes_apply_the_ratio_to_the_first_color() {
        // 20% blue + 80% white: the ratio applies to the first colour.
        assert_eq!(
            resolve_color("blue!20").unwrap(),
            Paint::Rgb(0xcc, 0xcc, 0xff)
        );
        assert_eq!(
            resolve_color("green!60!black").unwrap(),
            Paint::Rgb(0x00, 0x99, 0x00)
        );
        assert_eq!(
            resolve_color("black!0").unwrap(),
            Paint::Rgb(0xff, 0xff, 0xff)
        );
        assert_eq!(
            resolve_color("red!100").unwrap(),
            Paint::Rgb(0xff, 0x00, 0x00)
        );
    }

    #[test]
    fn hex_forms_and_none() {
        assert_eq!(resolve_color("#fff").unwrap(), Paint::Rgb(0xff, 0xff, 0xff));
        assert_eq!(
            resolve_color("#bf0040").unwrap(),
            Paint::Rgb(0xbf, 0x00, 0x40)
        );
        assert_eq!(resolve_color("none").unwrap(), Paint::None);
    }

    #[test]
    fn unknown_colors_are_errors_not_panics() {
        assert!(resolve_color("tealish")
            .unwrap_err()
            .contains("unknown colour"));
        assert!(resolve_color("blue!x").is_err());
        assert!(resolve_color("").is_err());
        assert!(resolve_color("green!50!notacolor").is_err());
    }
}
