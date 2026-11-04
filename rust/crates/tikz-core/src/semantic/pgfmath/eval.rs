//! Tokenizer and recursive-descent parser for PGF math expressions.
//!
//! Port of `semantic/pgfmath/evaluator.ts`. Error codes, evaluation order
//! (ternary and `&&`/`||` evaluate both sides, so the RNG advances like the
//! legacy evaluator) and the scalar/length typing rules are kept verbatim.

use super::fmt::js_round;
use super::funcs::{call_function, factorial_of};
use super::lex::{tokenize, Token};
use super::rng::PgfRandom;
use super::{unit_factor, ErrorCode, PgfMathError, Quantity};

type Eval<T> = Result<T, PgfMathError>;

fn fail<T>(code: ErrorCode, message: impl Into<String>) -> Eval<T> {
    Err(PgfMathError::new(code, message))
}

/// Evaluate an already-normalised, brace-free expression.
pub(super) fn run(
    input: &str,
    rng: Option<&mut PgfRandom>,
    em_pt: Option<f64>,
    ex_pt: Option<f64>,
) -> Eval<Quantity> {
    let tokens = tokenize(input)?;
    let mut parser = Parser {
        tokens,
        index: 0,
        rng,
        em_pt,
        ex_pt,
    };
    parser.parse()
}

struct Parser<'r> {
    tokens: Vec<Token>,
    index: usize,
    rng: Option<&'r mut PgfRandom>,
    em_pt: Option<f64>,
    ex_pt: Option<f64>,
}

fn truthy(q: Quantity) -> bool {
    q.value.abs() > 1e-12
}

fn flag(on: bool) -> Quantity {
    Quantity::scalar(if on { 1.0 } else { 0.0 })
}

impl Parser<'_> {
    fn parse(&mut self) -> Eval<Quantity> {
        let expression = self.ternary()?;
        if self.index < self.tokens.len() {
            return fail(
                ErrorCode::UnexpectedToken,
                "Unexpected trailing tokens in expression.",
            );
        }
        if !expression.value.is_finite() {
            return fail(
                ErrorCode::InvalidDomain,
                "Expression evaluated to a non-finite value.",
            );
        }
        Ok(expression)
    }

    fn ternary(&mut self) -> Eval<Quantity> {
        let condition = self.logical_or()?;
        if !self.match_op("?") {
            return Ok(condition);
        }
        let when_true = self.ternary()?;
        if !self.match_op(":") {
            return fail(
                ErrorCode::UnexpectedToken,
                "Ternary expression is missing ':'.",
            );
        }
        let when_false = self.ternary()?;
        Ok(if truthy(condition) {
            when_true
        } else {
            when_false
        })
    }

    fn logical_or(&mut self) -> Eval<Quantity> {
        let mut left = self.logical_and()?;
        while self.match_op("||") {
            let right = self.logical_and()?;
            left = flag(truthy(left) || truthy(right));
        }
        Ok(left)
    }

    fn logical_and(&mut self) -> Eval<Quantity> {
        let mut left = self.comparison()?;
        while self.match_op("&&") {
            let right = self.comparison()?;
            left = flag(truthy(left) && truthy(right));
        }
        Ok(left)
    }

    fn comparison(&mut self) -> Eval<Quantity> {
        let mut left = self.add_sub()?;
        while let Some(op) = self.peek_op(&["==", "!=", "<", "<=", ">", ">="]) {
            self.index += 1;
            let right = self.add_sub()?;
            if left.kind != right.kind {
                return fail(
                    ErrorCode::InvalidOperation,
                    "Cannot compare scalar and length values.",
                );
            }
            let (l, r) = (left.value, right.value);
            left = flag(match op {
                "==" => (l - r).abs() <= 1e-9,
                "!=" => (l - r).abs() > 1e-9,
                "<" => l < r,
                "<=" => l <= r,
                ">" => l > r,
                _ => l >= r,
            });
        }
        Ok(left)
    }

    fn add_sub(&mut self) -> Eval<Quantity> {
        let mut left = self.mul_div()?;
        while let Some(op) = self.peek_op(&["+", "-"]) {
            self.index += 1;
            let right = self.mul_div()?;
            if left.kind != right.kind {
                return fail(
                    ErrorCode::InvalidOperation,
                    "Cannot add or subtract scalar and length values.",
                );
            }
            let value = if op == "+" {
                left.value + right.value
            } else {
                left.value - right.value
            };
            left = Quantity {
                kind: left.kind,
                value,
            };
        }
        Ok(left)
    }

    fn mul_div(&mut self) -> Eval<Quantity> {
        use super::QuantityKind::{Length, Scalar};
        let mut left = self.unary()?;
        while let Some(op) = self.peek_op(&["*", "/"]) {
            self.index += 1;
            let right = self.unary()?;
            if op == "*" {
                left = match (left.kind, right.kind) {
                    (Scalar, Scalar) => Quantity::scalar(left.value * right.value),
                    (Length, Length) => {
                        return fail(
                            ErrorCode::InvalidOperation,
                            "Cannot multiply two length values.",
                        )
                    }
                    _ => Quantity::length(left.value * right.value),
                };
                continue;
            }
            if right.value.abs() <= 1e-12 {
                return fail(ErrorCode::DivisionByZero, "Division by zero.");
            }
            left = match (left.kind, right.kind) {
                (Scalar, Scalar) | (Length, Length) => Quantity::scalar(left.value / right.value),
                (Length, Scalar) => Quantity::length(left.value / right.value),
                (Scalar, Length) => {
                    return fail(
                        ErrorCode::InvalidOperation,
                        "Cannot divide scalar by length.",
                    )
                }
            };
        }
        Ok(left)
    }

    fn power(&mut self) -> Eval<Quantity> {
        let left = self.postfix()?;
        if !self.match_op("^") {
            return Ok(left);
        }
        let right = self.unary()?;
        if right.kind != super::QuantityKind::Scalar {
            return fail(
                ErrorCode::InvalidOperation,
                "Exponent must be a scalar value.",
            );
        }
        let value = left.value.powf(right.value);
        if left.kind == super::QuantityKind::Scalar {
            return Ok(Quantity::scalar(value));
        }
        if (right.value - js_round(right.value)).abs() > 1e-9 {
            return fail(
                ErrorCode::InvalidOperation,
                "Length exponent must be an integer scalar.",
            );
        }
        Ok(Quantity::length(value))
    }

    fn unary(&mut self) -> Eval<Quantity> {
        if self.match_op("+") {
            return self.unary();
        }
        if self.match_op("-") {
            let value = self.unary()?;
            return Ok(Quantity {
                kind: value.kind,
                value: -value.value,
            });
        }
        if self.match_op("!") {
            let value = self.unary()?;
            return Ok(flag(!truthy(value)));
        }
        self.power()
    }

    fn postfix(&mut self) -> Eval<Quantity> {
        let mut current = self.primary()?;
        loop {
            if self.match_op("!") {
                if current.kind != super::QuantityKind::Scalar {
                    return fail(
                        ErrorCode::InvalidOperation,
                        "Factorial requires a scalar argument.",
                    );
                }
                let Some(factorial) = factorial_of(current.value) else {
                    return fail(
                        ErrorCode::InvalidDomain,
                        "Factorial requires a non-negative integer.",
                    );
                };
                current = Quantity::scalar(factorial);
                continue;
            }
            let is_radian_suffix = matches!(self.tokens.get(self.index), Some(Token::Ident(name)) if name.eq_ignore_ascii_case("r"));
            if !is_radian_suffix {
                return Ok(current);
            }
            self.index += 1;
            if current.kind != super::QuantityKind::Scalar {
                return fail(
                    ErrorCode::InvalidOperation,
                    "The postfix 'r' operator requires a scalar value.",
                );
            }
            current = Quantity::scalar(current.value * 180.0 / std::f64::consts::PI);
        }
    }

    fn primary(&mut self) -> Eval<Quantity> {
        let Some(token) = self.tokens.get(self.index).cloned() else {
            return fail(ErrorCode::UnexpectedToken, "Unexpected end of expression.");
        };
        match token {
            Token::Number { value, unit } => {
                self.index += 1;
                self.number_with_unit(value, unit)
            }
            Token::Op("(") => {
                self.index += 1;
                let nested = self.ternary()?;
                if !self.match_op(")") {
                    return fail(
                        ErrorCode::UnexpectedToken,
                        "Missing closing ')' in expression.",
                    );
                }
                Ok(nested)
            }
            Token::Ident(ident) => {
                self.index += 1;
                self.identifier(&ident)
            }
            Token::Op(_) => fail(ErrorCode::Token, "Unexpected token in expression."),
        }
    }

    fn number_with_unit(&self, value: f64, unit: Option<String>) -> Eval<Quantity> {
        let Some(unit) = unit else {
            return Ok(Quantity::scalar(value));
        };
        let key = unit.to_ascii_lowercase();
        if key == "r" {
            return Ok(Quantity::scalar(value * 180.0 / std::f64::consts::PI));
        }
        let context = match key.as_str() {
            "em" => self.em_pt,
            "ex" => self.ex_pt,
            _ => None,
        };
        match context.or_else(|| unit_factor(&key)) {
            Some(factor) => Ok(Quantity::length(value * factor)),
            None => fail(
                ErrorCode::InvalidDomain,
                format!("Unsupported unit '{unit}'."),
            ),
        }
    }

    fn identifier(&mut self, ident: &str) -> Eval<Quantity> {
        let lower = ident.to_ascii_lowercase();
        match lower.as_str() {
            "pi" => return Ok(Quantity::scalar(std::f64::consts::PI)),
            "e" => return Ok(Quantity::scalar(std::f64::consts::E)),
            "true" => return Ok(Quantity::scalar(1.0)),
            "false" => return Ok(Quantity::scalar(0.0)),
            _ => {}
        }
        if !self.match_op("(") {
            if lower == "rnd" || lower == "rand" {
                return call_function(ident, &[], self.rng.as_deref_mut());
            }
            return fail(
                ErrorCode::UnknownFunction,
                format!("Unknown identifier '{ident}'."),
            );
        }
        let mut args = Vec::new();
        if !self.match_op(")") {
            loop {
                args.push(self.ternary()?);
                if self.match_op(")") {
                    break;
                }
                if !self.match_op(",") {
                    return fail(
                        ErrorCode::UnexpectedToken,
                        format!("Function '{ident}' expects comma-separated arguments."),
                    );
                }
            }
        }
        call_function(ident, &args, self.rng.as_deref_mut())
    }

    fn match_op(&mut self, expected: &str) -> bool {
        if matches!(self.tokens.get(self.index), Some(Token::Op(op)) if *op == expected) {
            self.index += 1;
            return true;
        }
        false
    }

    fn peek_op(&self, expected: &[&'static str]) -> Option<&'static str> {
        match self.tokens.get(self.index) {
            Some(Token::Op(op)) => expected.iter().copied().find(|e| e == op),
            _ => None,
        }
    }
}
