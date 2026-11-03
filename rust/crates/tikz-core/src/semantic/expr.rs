//! Arithmetic evaluation for coordinate components, lengths and angles.
//!
//! TikZ writes dimensions as PGF math expressions (`2.5`, `1cm`, `2*0.5cm`,
//! `-1`, `(3+1)/2`). The legacy pipeline evaluates them with pgfmath; this
//! module implements the arithmetic subset the corpus needs — `+ - * /`,
//! parentheses, unary signs and an optional unit — on `f64` so results match
//! the JavaScript doubles exactly.
//!
//! ```
//! use tikz_core::semantic::expr::{eval_len, eval_number, Unit};
//! assert_eq!(eval_len("2cm", Unit::Cm), Ok(2.0 * 28.4527559055));
//! assert_eq!(eval_len("4pt", Unit::Cm), Ok(4.0));
//! assert_eq!(eval_len("(3+1)/2cm", Unit::Cm), Ok(2.0 * 28.4527559055));
//! assert_eq!(eval_number("30"), Ok(30.0));
//! assert!(eval_number("2cm").is_err());
//! ```

use crate::units::{CM_TO_PT, MM_TO_PT};

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
    fn factor(self) -> f64 {
        match self {
            Unit::Cm => CM_TO_PT,
            Unit::Pt => 1.0,
            Unit::Mm => MM_TO_PT,
            Unit::In => 72.27,
            Unit::Bp => 72.27 / 72.0,
        }
    }
}

/// Evaluate a length expression into points.
pub fn eval_len(raw: &str, default_unit: Unit) -> Result<f64, String> {
    let trimmed = raw.trim();
    let (expression, unit) = split_unit(trimmed)?;
    let value = eval_expression(expression)?;
    Ok(value * unit.unwrap_or(default_unit).factor())
}

/// Evaluate a unitless number expression (angles, scales, opacities).
pub fn eval_number(raw: &str) -> Result<f64, String> {
    let trimmed = raw.trim();
    let (expression, unit) = split_unit(trimmed)?;
    if unit.is_some() {
        return Err(format!("expected a number without a unit: `{trimmed}`"));
    }
    eval_expression(expression)
}

/// Split a trailing unit off an expression; `2*0.5cm` keeps its arithmetic.
fn split_unit(raw: &str) -> Result<(&str, Option<Unit>), String> {
    let trimmed = raw.trim();
    let end = trimmed
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(trimmed.len());
    let (expression, unit_text) = trimmed.split_at(end);
    let unit = match unit_text.trim().to_ascii_lowercase().as_str() {
        "" => None,
        "cm" => Some(Unit::Cm),
        "pt" => Some(Unit::Pt),
        "mm" => Some(Unit::Mm),
        "in" => Some(Unit::In),
        "bp" => Some(Unit::Bp),
        other => return Err(format!("unsupported unit `{other}` in `{trimmed}`")),
    };
    if expression.trim().is_empty() {
        return Err(format!("missing number in `{trimmed}`"));
    }
    Ok((expression, unit))
}

/// Evaluate an arithmetic expression with no unit suffix.
fn eval_expression(raw: &str) -> Result<f64, String> {
    let bytes = raw.as_bytes();
    let mut parser = Parser { bytes, at: 0 };
    let value = parser.sum()?;
    parser.skip_spaces();
    if parser.at != bytes.len() {
        return Err(format!("unexpected `{}` in `{raw}`", &raw[parser.at..]));
    }
    if !value.is_finite() {
        return Err(format!("non-finite result in `{raw}`"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_spaces(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip_spaces();
        if self.at < self.bytes.len() && self.bytes[self.at] == byte {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<f64, String> {
        let mut value = self.product()?;
        loop {
            if self.eat(b'+') {
                value += self.product()?;
            } else if self.eat(b'-') {
                value -= self.product()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn product(&mut self) -> Result<f64, String> {
        let mut value = self.unary()?;
        loop {
            if self.eat(b'*') {
                value *= self.unary()?;
            } else if self.eat(b'/') {
                let divisor = self.unary()?;
                if divisor == 0.0 {
                    return Err("division by zero".to_string());
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        if self.eat(b'-') {
            return Ok(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<f64, String> {
        self.skip_spaces();
        if self.eat(b'(') {
            let value = self.sum()?;
            if !self.eat(b')') {
                return Err("missing `)`".to_string());
            }
            return Ok(value);
        }
        self.number()
    }

    fn number(&mut self) -> Result<f64, String> {
        self.skip_spaces();
        let start = self.at;
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_digit()
                || self.bytes[self.at] == b'.'
                || (self.at == start
                    && (self.bytes[self.at] == b'-' || self.bytes[self.at] == b'+')))
        {
            self.at += 1;
        }
        if self.at == start {
            return Err(format!("expected a number at offset {start}"));
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| "not valid UTF-8".to_string())?;
        text.parse::<f64>()
            .map_err(|_| format!("not a number: `{text}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_convert_like_the_legacy_scale() {
        assert!((eval_len("1cm", Unit::Pt).unwrap() - CM_TO_PT).abs() < 1e-12);
        assert!((eval_len("1mm", Unit::Cm).unwrap() - MM_TO_PT).abs() < 1e-12);
        assert_eq!(eval_len("1in", Unit::Cm).unwrap(), 72.27);
        assert!((eval_len("72bp", Unit::Cm).unwrap() - 72.27).abs() < 1e-12);
    }

    #[test]
    fn arithmetic_and_parentheses() {
        assert_eq!(eval_number("(3+1)/2").unwrap(), 2.0);
        assert_eq!(eval_number("-1.5 * 2 + 1").unwrap(), -2.0);
        assert_eq!(eval_len("2*0.5cm", Unit::Pt).unwrap(), CM_TO_PT);
        assert_eq!(eval_number("0.6").unwrap(), 0.6);
    }

    #[test]
    fn errors_are_typed_and_never_panic() {
        assert!(eval_number("2cm").unwrap_err().contains("without a unit"));
        assert!(eval_len("1em", Unit::Cm)
            .unwrap_err()
            .contains("unsupported unit"));
        assert!(eval_number("sin(30)").is_err());
        assert!(eval_number("1/0").unwrap_err().contains("division by zero"));
        assert!(eval_number("(1").is_err());
        assert!(eval_number("").unwrap_err().contains("missing number"));
    }

    #[test]
    fn spaces_are_ignored() {
        assert_eq!(eval_number(" 1 + 2 ").unwrap(), 3.0);
        assert_eq!(eval_len(" 2 cm ", Unit::Pt).unwrap(), 2.0 * CM_TO_PT);
    }
}
