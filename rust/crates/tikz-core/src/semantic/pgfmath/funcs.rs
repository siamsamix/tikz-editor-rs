//! PGF math function table (scalar functions, numeric helpers).

use super::fmt::js_round;
use super::rng::PgfRandom;
use super::{ErrorCode, PgfMathError, Quantity, QuantityKind};

type Eval<T> = Result<T, PgfMathError>;

fn fail<T>(code: ErrorCode, message: impl Into<String>) -> Eval<T> {
    Err(PgfMathError::new(code, message))
}

pub(super) fn require_scalar(quantity: Option<Quantity>, function: &str) -> Eval<f64> {
    match quantity {
        None => fail(
            ErrorCode::InvalidArity,
            format!("{function}() is missing an argument."),
        ),
        Some(q) if q.kind != QuantityKind::Scalar => fail(
            ErrorCode::InvalidOperation,
            format!("{function}() expects scalar arguments."),
        ),
        Some(q) => Ok(q.value),
    }
}

pub(super) fn unary(name: &str, args: &[Quantity], f: fn(f64) -> f64) -> Eval<Quantity> {
    let [arg] = args else {
        return fail(
            ErrorCode::InvalidArity,
            format!("{name}() expects exactly one argument."),
        );
    };
    finite(name, f(require_scalar(Some(*arg), name)?))
}

pub(super) fn binary(name: &str, args: &[Quantity], f: impl Fn(f64, f64) -> f64) -> Eval<Quantity> {
    let [left, right] = args else {
        return fail(
            ErrorCode::InvalidArity,
            format!("{name}() expects exactly two arguments."),
        );
    };
    let l = require_scalar(Some(*left), name)?;
    let r = require_scalar(Some(*right), name)?;
    finite(name, f(l, r))
}

fn finite(name: &str, value: f64) -> Eval<Quantity> {
    if value.is_finite() {
        Ok(Quantity::scalar(value))
    } else {
        fail(
            ErrorCode::InvalidDomain,
            format!("{name}() produced a non-finite result."),
        )
    }
}

pub(super) fn unary_function(key: &str) -> Option<fn(f64) -> f64> {
    use std::f64::consts::PI;
    // The `(v * PI) / 180` shapes are deliberate: they match the legacy
    // JavaScript rounding exactly (`to_radians` multiplies by `PI / 180`).
    let f: fn(f64) -> f64 = match key {
        "sin" => |v| (v * PI / 180.0).sin(),
        "cos" => |v| (v * PI / 180.0).cos(),
        "tan" => |v| (v * PI / 180.0).tan(),
        "asin" => |v| v.asin() * 180.0 / PI,
        "acos" => |v| v.acos() * 180.0 / PI,
        "atan" => |v| v.atan() * 180.0 / PI,
        "sqrt" => |v| if v < 0.0 { f64::NAN } else { v.sqrt() },
        "abs" => f64::abs,
        "exp" => f64::exp,
        "ln" => |v| if v <= 0.0 { f64::NAN } else { v.ln() },
        "log10" => |v| if v <= 0.0 { f64::NAN } else { v.log10() },
        "log2" => |v| if v <= 0.0 { f64::NAN } else { v.log2() },
        "round" => js_round,
        "floor" => f64::floor,
        "ceil" => f64::ceil,
        "int" => f64::trunc,
        "frac" => |v| v - v.trunc(),
        "sign" => |v| {
            if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }
        },
        "deg" => |v| v * 180.0 / PI,
        "rad" => |v| v * PI / 180.0,
        _ => return None,
    };
    Some(f)
}

pub(super) fn factorial_of(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    let rounded = js_round(value);
    if (value - rounded).abs() > 1e-9 || !(0.0..=170.0).contains(&rounded) {
        return None;
    }
    Some((2..=rounded as u32).fold(1.0, |acc, n| acc * f64::from(n)))
}

fn random_function(name: &str, args: &[Quantity], rng: Option<&mut PgfRandom>) -> Eval<Quantity> {
    let key = name.to_ascii_lowercase();
    if key == "rnd" || key == "rand" {
        if !args.is_empty() {
            return fail(
                ErrorCode::InvalidArity,
                format!("{key}() does not accept arguments."),
            );
        }
        let rng = require_rng(rng)?;
        return Ok(Quantity::scalar(if key == "rnd" {
            rng.rnd()
        } else {
            rng.rand()
        }));
    }
    let rng = require_rng(rng)?;
    match args {
        [] => Ok(Quantity::scalar(rng.rnd())),
        [upper] => {
            let upper = require_scalar(Some(*upper), "random")?;
            Ok(Quantity::scalar(rng.random_integer(1.0, upper)))
        }
        [lower, upper] => {
            let lower = require_scalar(Some(*lower), "random")?;
            let upper = require_scalar(Some(*upper), "random")?;
            Ok(Quantity::scalar(rng.random_integer(lower, upper)))
        }
        _ => fail(
            ErrorCode::InvalidArity,
            "random() supports 0, 1, or 2 arguments.",
        ),
    }
}

fn require_rng(rng: Option<&mut PgfRandom>) -> Eval<&mut PgfRandom> {
    match rng {
        Some(rng) => Ok(rng),
        None => fail(
            ErrorCode::UnsupportedRandom,
            "Random functions require an active pgfmath RNG runtime.",
        ),
    }
}

pub(super) fn call_function(
    name: &str,
    args: &[Quantity],
    rng: Option<&mut PgfRandom>,
) -> Eval<Quantity> {
    let key = name.to_ascii_lowercase();
    match key.as_str() {
        "rnd" | "rand" | "random" => random_function(name, args, rng),
        "min" | "max" => {
            if args.is_empty() {
                return fail(
                    ErrorCode::InvalidArity,
                    format!("{name}() expects at least one argument."),
                );
            }
            let mut best: Option<f64> = None;
            for arg in args {
                let v = require_scalar(Some(*arg), name)?;
                best = Some(match best {
                    None => v,
                    Some(b) if key == "min" => b.min(v),
                    Some(b) => b.max(v),
                });
            }
            Ok(Quantity::scalar(best.unwrap_or(0.0)))
        }
        "scalar" => match args {
            [only] => Ok(Quantity::scalar(only.value)),
            _ => fail(
                ErrorCode::InvalidArity,
                "scalar() expects exactly one argument.",
            ),
        },
        "pow" => binary(name, args, f64::powf),
        "atan2" => binary(name, args, |y, x| y.atan2(x) * 180.0 / std::f64::consts::PI),
        "mod" => {
            let positive = name == "Mod";
            binary(name, args, move |l, r| {
                if r.abs() <= 1e-12 {
                    return f64::NAN;
                }
                let remainder = l % r;
                if !positive {
                    return remainder;
                }
                let divisor = r.abs();
                ((remainder % divisor) + divisor) % divisor
            })
        }
        _ => match unary_function(&key) {
            Some(f) => unary(name, args, f),
            None => fail(
                ErrorCode::UnknownFunction,
                format!("Unsupported function '{name}'."),
            ),
        },
    }
}
