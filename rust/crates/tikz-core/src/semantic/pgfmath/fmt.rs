//! PGF math result formatting (blueprint row 25).
//!
//! Thresholds are copied verbatim from the legacy `formatPgfMathNumber`:
//! non-finite or `|v| ≤ 1e-12` → `"0"`; within `1e-9` of an integer → that
//! integer; otherwise `toFixed(12)` with trailing zeros stripped.
//!
//! ```
//! use tikz_core::semantic::pgfmath::fmt::format_number;
//! assert_eq!(format_number(1.0000000001), "1");
//! assert_eq!(format_number(1.2345000000001), "1.2345");
//! assert_eq!(format_number(f64::INFINITY), "0");
//! ```

/// Format a pgfmath value the way `\pgfmathresult` text appears.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() || value.abs() <= 1e-12 {
        return "0".to_string();
    }
    let rounded = js_round(value);
    if (value - rounded).abs() <= 1e-9 {
        return integer_string(rounded);
    }
    let fixed = to_fixed_12(value);
    let stripped = strip_trailing_zeros(&fixed);
    if stripped == "-0" {
        "0".to_string()
    } else {
        stripped
    }
}

/// `Math.round`: halves round toward `+∞` (Rust's `round` goes away from 0).
pub fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `String(n)` for an integral double.
fn integer_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.abs() < 1e21 {
        return format!("{value:.0}");
    }
    let exponential = format!("{:e}", value.abs());
    let signed = exponential.replacen('e', "e+", 1);
    if value < 0.0 {
        format!("-{signed}")
    } else {
        signed
    }
}

/// `Number.prototype.toFixed(12)`; exact ties round the magnitude up, where
/// Rust's formatter would round half to even.
fn to_fixed_12(value: f64) -> String {
    let magnitude = value.abs();
    let sign = if value < 0.0 { "-" } else { "" };
    let wide = format!("{magnitude:.40}");
    let Some((int_part, frac)) = wide.split_once('.') else {
        return format!("{sign}{magnitude:.12}");
    };
    let is_tie = frac.as_bytes()[12] == b'5' && frac[13..].bytes().all(|b| b == b'0');
    if !is_tie {
        return format!("{sign}{magnitude:.12}");
    }
    let mut digits: Vec<u8> = format!("{int_part}{}", &frac[..12]).into_bytes();
    for digit in digits.iter_mut().rev() {
        if *digit == b'9' {
            *digit = b'0';
        } else {
            *digit += 1;
            break;
        }
    }
    let carried = digits.iter().all(|&d| d == b'0');
    let text = String::from_utf8(digits).unwrap_or_default();
    let split = text.len() - 12;
    let prefix = if carried { "1" } else { "" };
    format!("{sign}{prefix}{}.{}", &text[..split], &text[split..])
}

/// `replace(/\.?0+$/, "")`.
fn strip_trailing_zeros(text: &str) -> String {
    let trimmed = text.trim_end_matches('0');
    if trimmed.len() == text.len() {
        return text.to_string();
    }
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_legacy_spec() {
        assert_eq!(format_number(f64::INFINITY), "0");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(1.0000000001), "1");
        assert_eq!(format_number(1.2345000000001), "1.2345");
    }

    #[test]
    fn integers_and_negatives() {
        assert_eq!(format_number(120.0), "120");
        assert_eq!(format_number(-3.0), "-3");
        assert_eq!(format_number(-0.5), "-0.5");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1.0 / 3.0), "0.333333333333");
    }

    #[test]
    fn huge_integers_use_js_exponent_form() {
        assert_eq!(format_number(1e21), "1e+21");
        assert_eq!(format_number(-1.5e22), "-1.5e+22");
        assert_eq!(format_number(1e20), "100000000000000000000");
    }

    #[test]
    fn exact_ties_round_up_like_to_fixed() {
        // 2^-13 = 0.0001220703125 exactly; toFixed(12) picks the larger n.
        assert_eq!(format_number(0.0001220703125), "0.000122070313");
        assert_eq!(js_round(-1.5), -1.0);
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-0.4), 0.0);
    }
}
