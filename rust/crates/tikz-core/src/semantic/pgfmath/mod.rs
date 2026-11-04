//! PGF math (`\pgfmathparse`, `\pgfmathsetmacro`, `\pgfmathsetseed`).
//!
//! Port of `semantic/pgfmath/` (blueprint row 25). One evaluator serves every
//! consumer — dimensions, coordinates, angles, `\pgfmathsetmacro` — so they
//! all see the same values. Results are [`Quantity`] values that stay typed as
//! a plain scalar or a length (in pt), exactly like the legacy evaluator.
//!
//! ```
//! use tikz_core::semantic::pgfmath::{evaluate, fmt::format_number, QuantityKind};
//! let length = evaluate("1cm + 2mm").unwrap();
//! assert_eq!(length.kind, QuantityKind::Length);
//! assert_eq!(format_number(evaluate("2^3^2").unwrap().value), "512");
//! assert_eq!(evaluate("1 +").unwrap_err().code.as_str(), "unexpected-token");
//! ```

mod eval;
pub mod fmt;
mod funcs;
mod lex;
pub mod rng;
pub mod runtime;
pub mod state;

use crate::units::{CM_TO_PT, MM_TO_PT};
use rng::PgfRandom;

/// Whether a value is a plain number or a length in pt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuantityKind {
    /// Unitless number.
    Scalar,
    /// Length, stored in pt.
    Length,
}

/// A typed pgfmath value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quantity {
    /// Scalar or length.
    pub kind: QuantityKind,
    /// Value (pt for lengths).
    pub value: f64,
}

impl Quantity {
    /// A unitless value.
    pub fn scalar(value: f64) -> Self {
        Self {
            kind: QuantityKind::Scalar,
            value,
        }
    }

    /// A length in pt.
    pub fn length(value: f64) -> Self {
        Self {
            kind: QuantityKind::Length,
            value,
        }
    }
}

/// Stable failure categories (suffixes of the legacy diagnostic codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Blank expression.
    Empty,
    /// Unlexable character or malformed literal.
    Token,
    /// Grammar violation.
    UnexpectedToken,
    /// Arrays and quoted strings.
    UnsupportedSyntax,
    /// Division by (almost) zero.
    DivisionByZero,
    /// Wrong argument count.
    InvalidArity,
    /// Out-of-domain or non-finite result.
    InvalidDomain,
    /// Unknown function or identifier.
    UnknownFunction,
    /// Random function without an RNG runtime.
    UnsupportedRandom,
    /// Scalar/length type misuse.
    InvalidOperation,
}

impl ErrorCode {
    /// The legacy kebab-case code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Token => "token",
            Self::UnexpectedToken => "unexpected-token",
            Self::UnsupportedSyntax => "unsupported-syntax",
            Self::DivisionByZero => "division-by-zero",
            Self::InvalidArity => "invalid-arity",
            Self::InvalidDomain => "invalid-domain",
            Self::UnknownFunction => "unknown-function",
            Self::UnsupportedRandom => "unsupported-random",
            Self::InvalidOperation => "invalid-operation",
        }
    }
}

/// A failed evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgfMathError {
    /// Failure category.
    pub code: ErrorCode,
    /// Human-readable reason.
    pub message: String,
}

impl PgfMathError {
    pub(crate) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Result of an evaluation.
pub type EvalResult = Result<Quantity, PgfMathError>;

/// Optional inputs for [`evaluate_with`].
#[derive(Default)]
pub struct EvalOptions<'a> {
    /// Explicit RNG; otherwise the scoped [`runtime`] RNG (if any) is used.
    pub rng: Option<&'a mut PgfRandom>,
    /// `em` in pt (font dependent); legacy fallback is 10pt.
    pub em_pt: Option<f64>,
    /// `ex` in pt (font dependent); legacy fallback is 4.3pt.
    pub ex_pt: Option<f64>,
}

/// Evaluate with the scoped runtime RNG (random functions fail without one).
pub fn evaluate(input: &str) -> EvalResult {
    evaluate_with(input, EvalOptions::default())
}

/// Evaluate with an explicit RNG.
pub fn evaluate_with_rng(input: &str, rng: &mut PgfRandom) -> EvalResult {
    evaluate_with(
        input,
        EvalOptions {
            rng: Some(rng),
            ..EvalOptions::default()
        },
    )
}

/// Evaluate with full control over RNG and context units.
pub fn evaluate_with(input: &str, options: EvalOptions<'_>) -> EvalResult {
    let normalized = lex::normalize_input(input);
    if normalized.is_empty() {
        return Err(PgfMathError::new(ErrorCode::Empty, "Expression is empty."));
    }
    if normalized.contains(['{', '}', '"']) {
        return Err(PgfMathError::new(
            ErrorCode::UnsupportedSyntax,
            "Array and quoted-string pgfmath expressions are not supported yet.",
        ));
    }
    let EvalOptions { rng, em_pt, ex_pt } = options;
    match rng {
        Some(rng) => eval::run(&normalized, Some(rng), em_pt, ex_pt),
        None => runtime::with_current_rng(|rng| eval::run(&normalized, rng, em_pt, ex_pt)),
    }
}

/// Evaluate and format like `\pgfmathresult` (foreach `evaluate=` / step values).
///
/// ```
/// use tikz_core::semantic::pgfmath::evaluate_to_text;
/// assert_eq!(evaluate_to_text("1/4+0.5").unwrap(), "0.75");
/// assert_eq!(evaluate_to_text("2*3").unwrap(), "6");
/// ```
pub fn evaluate_to_text(input: &str) -> Result<String, PgfMathError> {
    evaluate(input).map(|quantity| fmt::format_number(quantity.value))
}

/// Factor converting one `unit` (lower-case) to pt.
pub fn unit_factor(unit: &str) -> Option<f64> {
    Some(match unit {
        "pt" => 1.0,
        "bp" | "px" => 72.27 / 72.0,
        "cm" => CM_TO_PT,
        "mm" => MM_TO_PT,
        "in" => 72.27,
        "ex" => 4.3,
        "em" => 10.0,
        _ => return None,
    })
}

/// Interpret `quantity` as a length: scalars take `default_unit` (`cm`/`pt`/…).
pub fn convert_quantity_to_length(quantity: Quantity, default_unit: &str) -> Option<f64> {
    match quantity.kind {
        QuantityKind::Length => Some(quantity.value),
        QuantityKind::Scalar => {
            unit_factor(&default_unit.to_ascii_lowercase()).map(|factor| quantity.value * factor)
        }
    }
}

#[cfg(test)]
mod tests;
