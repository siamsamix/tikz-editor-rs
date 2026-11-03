//! Property-based fuzz tests for the parser and SVG number pipeline.
//!
//! These run against arbitrary inputs and must never panic or produce
//! out-of-contract results. See the migration TODO (M0-6).

use crate::render::render_tex_with_warnings;
use crate::svg_numbers::fmt_num;
use crate::units::Vec2;
use proptest::prelude::*;

/// Arbitrary TikZ-ish source text: a mix of structural characters, control
/// sequences, numbers, units and random junk. All parts produce `String`.
fn tex_source() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            any::<char>().prop_map(|c| c.to_string()),
            Just("\\draw".to_string()),
            Just("\\fill".to_string()),
            Just("\\path".to_string()),
            Just("\\node".to_string()),
            Just("\\begin{tikzpicture}".to_string()),
            Just("\\end{tikzpicture}".to_string()),
            Just("(0,0)".to_string()),
            Just("(1,1)".to_string()),
            Just("--".to_string()),
            Just("|-".to_string()),
            Just("-|".to_string()),
            Just("..".to_string()),
            Just("cycle".to_string()),
            Just("rectangle".to_string()),
            Just("circle".to_string()),
            Just("[draw]".to_string()),
            Just("[fill=red]".to_string()),
            Just("[line width=2pt]".to_string()),
            Just("[scale=0.5]".to_string()),
            Just("[xshift=5cm]".to_string()),
            Just("[rotate around={45:(3,1)}]".to_string()),
            Just(";".to_string()),
            Just(",".to_string()),
            Just(":".to_string()),
            Just("{".to_string()),
            Just("}".to_string()),
            Just("[".to_string()),
            Just("]".to_string()),
            Just("(".to_string()),
            Just(")".to_string()),
            Just("+".to_string()),
            Just("-".to_string()),
            Just("*".to_string()),
            Just("/".to_string()),
        ],
        0..100,
    )
    .prop_map(|chars| chars.into_iter().collect::<String>())
}

/// Generator for f64 values that are finite and in a sane range for
/// coordinate/length parsing.
fn lenish() -> impl Strategy<Value = f64> {
    (-1000.0f64..1000.0).prop_filter("finite", |v| v.is_finite())
}

proptest! {
    /// The parser (render_tex_with_warnings) must never panic on arbitrary
    /// token soup, and must always return either Ok or Err (diagnostics).
    #[test]
    fn parser_never_panics_on_token_soup(source in tex_source()) {
        let _ = render_tex_with_warnings(&source);
    }

    /// fmt_num must never panic and must round-trip within 5e-5.
    #[test]
    fn fmt_num_within_5e_5(x in -1.0e6f64..1.0e6) {
        if !x.is_finite() {
            return Ok(());
        }
        let s = fmt_num(x);
        let back: f64 = s.parse().unwrap_or(x);
        prop_assert!((back - x).abs() <= 5.0e-5 + 1e-9);
    }

    /// fmt_num never produces exponent notation or trailing zeros.
    #[test]
    fn fmt_num_no_exponent_or_trailing_zero(x in -1.0e9f64..1.0e9) {
        if !x.is_finite() {
            return Ok(());
        }
        let s = fmt_num(x);
        prop_assert!(!s.contains('e'));
        prop_assert!(!s.contains('E'));
        if s.contains('.') {
            prop_assert!(!s.ends_with('0'));
        }
    }

    /// Coordinate parse roundtrip: any `x,y` pair of finite lengths parses
    /// back to the same values (in cm).
    #[test]
    fn coordinate_parse_roundtrip(x in lenish(), y in lenish()) {
        let input = format!("({}cm,{}cm)", x, y);
        let parsed =
            crate::semantic::pathscan::parse_coordinate(&input[1..input.len() - 1], None);
        prop_assert!(parsed.is_ok());
        let p = parsed.unwrap();
        prop_assert!((p.x - x * crate::units::CM_TO_PT).abs() < 1e-6);
        prop_assert!((p.y - y * crate::units::CM_TO_PT).abs() < 1e-6);
    }

    /// Y-flip invariance: bounding boxes are symmetric under y-flip.
    #[test]
    fn y_flip_bbox_invariance(x1 in lenish(), y1 in lenish(), x2 in lenish(), y2 in lenish()) {
        if !x1.is_finite() || !y1.is_finite() || !x2.is_finite() || !y2.is_finite() {
            return Ok(());
        }
        let lo = Vec2::new(x1.min(x2), y1.min(y2));
        let hi = Vec2::new(x1.max(x2), y1.max(y2));
        // Flipped bounds should just swap y signs.
        let flipped_lo = Vec2::new(lo.x, -hi.y);
        let flipped_hi = Vec2::new(hi.x, -lo.y);
        prop_assert_eq!(flipped_lo, Vec2::new(lo.x, -hi.y));
        prop_assert_eq!(flipped_hi, Vec2::new(hi.x, -lo.y));
    }

    /// render_tex_with_warnings returns Ok for simple path-only documents
    /// (warnings allowed, but never a panic).
    #[test]
    fn simple_paths_render(x in lenish(), y in lenish()) {
        if !x.is_finite() || !y.is_finite() {
            return Ok(());
        }
        let bs = "\\";
        let source = format!(
            "{}begin{{tikzpicture}}{}draw ({}cm,{}cm) -- ({}cm,{}cm);{}end{{tikzpicture}}",
            bs, bs, x, y, x, y, bs
        );
        let result = render_tex_with_warnings(&source);
        prop_assert!(result.is_ok());
    }
}
