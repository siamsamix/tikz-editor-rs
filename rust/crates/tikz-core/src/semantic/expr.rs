//! Evaluation of coordinate components, lengths and angles.
//!
//! TikZ writes dimensions as PGF math expressions (`2.5`, `1cm`, `2*0.5cm`,
//! `-1`, `sin(30)*2`). This module is the thin typed front door over
//! [`crate::semantic::pgfmath`], the same evaluator `\pgfmathsetmacro` uses,
//! so every dimension and macro sees identical values — the Rust counterpart
//! of the legacy `parseLength` (`pgfmath` evaluate + `convertQuantityToLength`).
//!
//! ```
//! use tikz_core::semantic::expr::{eval_len, eval_number, Unit};
//! assert_eq!(eval_len("2cm", Unit::Cm), Ok(2.0 * 28.4527559055));
//! assert_eq!(eval_len("4pt", Unit::Cm), Ok(4.0));
//! assert_eq!(eval_len("(3+1)/2", Unit::Cm), Ok(2.0 * 28.4527559055));
//! assert_eq!(eval_number("30"), Ok(30.0));
//! assert_eq!(eval_number("2*sin(30)").map(|v| (v * 1e9).round()), Ok(1e9));
//! assert!(eval_number("2cm").is_err());
//! ```

use crate::semantic::pgfmath::{self, QuantityKind};

/// Default unit applied to a bare number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// `cm` (the TikZ coordinate default).
    Cm,
    /// `pt` (the TikZ length-option default).
    Pt,
    /// `mm`.
    Mm,
    /// `in`.
    In,
    /// `bp`.
    Bp,
}

impl Unit {
    fn symbol(self) -> &'static str {
        match self {
            Unit::Cm => "cm",
            Unit::Pt => "pt",
            Unit::Mm => "mm",
            Unit::In => "in",
            Unit::Bp => "bp",
        }
    }
}

/// Evaluate a length expression into points; bare numbers take `default_unit`.
pub fn eval_len(raw: &str, default_unit: Unit) -> Result<f64, String> {
    let quantity = pgfmath::evaluate(raw).map_err(|error| error.message)?;
    pgfmath::convert_quantity_to_length(quantity, default_unit.symbol())
        .ok_or_else(|| format!("unsupported unit `{}`", default_unit.symbol()))
}

/// Evaluate a unitless number expression (angles, scales, opacities).
pub fn eval_number(raw: &str) -> Result<f64, String> {
    let quantity = pgfmath::evaluate(raw).map_err(|error| error.message)?;
    match quantity.kind {
        QuantityKind::Scalar => Ok(quantity.value),
        QuantityKind::Length => Err(format!(
            "expected a number without a unit: `{}`",
            raw.trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{CM_TO_PT, MM_TO_PT};

    #[test]
    fn units_convert_like_the_legacy_scale() {
        assert!((eval_len("1cm", Unit::Pt).unwrap() - CM_TO_PT).abs() < 1e-12);
        assert!((eval_len("1mm", Unit::Cm).unwrap() - MM_TO_PT).abs() < 1e-12);
        assert_eq!(eval_len("1in", Unit::Cm).unwrap(), 72.27);
        assert!((eval_len("72bp", Unit::Cm).unwrap() - 72.27).abs() < 1e-12);
        assert!((eval_len("2", Unit::Mm).unwrap() - 2.0 * MM_TO_PT).abs() < 1e-12);
    }

    #[test]
    fn arithmetic_and_parentheses() {
        assert_eq!(eval_number("(3+1)/2").unwrap(), 2.0);
        assert_eq!(eval_number("-1.5 * 2 + 1").unwrap(), -2.0);
        assert_eq!(eval_len("2*0.5cm", Unit::Pt).unwrap(), CM_TO_PT);
        assert_eq!(eval_number("0.6").unwrap(), 0.6);
    }

    #[test]
    fn functions_and_context_units_are_available() {
        assert!((eval_number("2*sin(30)").unwrap() - 1.0).abs() < 1e-12);
        assert!((eval_number("sqrt(16)+pow(2,3)").unwrap() - 12.0).abs() < 1e-12);
        assert_eq!(eval_len("1em", Unit::Cm).unwrap(), 10.0);
        assert!(eval_len("1.5\\baselineskip", Unit::Pt).is_err());
    }

    #[test]
    fn mixed_scalar_and_length_follow_pgfmath_typing() {
        // `(3+1)/2cm` divides a scalar by a length, which legacy rejects too.
        assert!(eval_len("(3+1)/2cm", Unit::Cm).is_err());
        assert!(eval_len("1cm+2", Unit::Cm).is_err());
    }

    #[test]
    fn errors_are_typed_and_never_panic() {
        assert!(eval_number("2cm").unwrap_err().contains("without a unit"));
        assert!(eval_len("1zz", Unit::Cm)
            .unwrap_err()
            .contains("Unsupported unit"));
        assert!(eval_number("1/0").unwrap_err().contains("Division by zero"));
        assert!(eval_number("(1").is_err());
        assert!(eval_number("").unwrap_err().contains("empty"));
    }

    #[test]
    fn spaces_are_ignored() {
        assert_eq!(eval_number(" 1 + 2 ").unwrap(), 3.0);
        assert_eq!(eval_len(" 2 cm ", Unit::Pt).unwrap(), 2.0 * CM_TO_PT);
    }
}
