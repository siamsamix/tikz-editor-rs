//! Port of `test/semantic/pgfmath.spec.ts` (evaluator half).

use super::rng::PgfRandom;
use super::{convert_quantity_to_length, evaluate, evaluate_with_rng, Quantity, QuantityKind};

fn scalar(input: &str) -> f64 {
    let q = evaluate_with_rng(input, &mut PgfRandom::new(1.0))
        .unwrap_or_else(|e| panic!("{input}: {e:?}"));
    q.value
}

fn length(input: &str) -> f64 {
    let q = evaluate_with_rng(input, &mut PgfRandom::new(1.0))
        .unwrap_or_else(|e| panic!("{input}: {e:?}"));
    assert_eq!(q.kind, QuantityKind::Length, "{input}");
    q.value
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} vs {expected}");
}

#[test]
fn unary_and_exponent_precedence() {
    assert_eq!(scalar("-2^2"), -4.0);
    assert_eq!(scalar("(-2)^2"), 4.0);
    assert_eq!(scalar("2^3^2"), 512.0);
}

#[test]
fn comparisons_logic_and_ternary() {
    assert_eq!(scalar("1 < 2 ? 7 : 9"), 7.0);
    assert_eq!(scalar("1 > 2 ? 7 : 9"), 9.0);
    assert_eq!(scalar("(1 > 2) || (3 == 3) ? 5 : 6"), 5.0);
    assert_eq!(scalar("(1 != 2) && (3 >= 3) && (2 <= 2)"), 1.0);
    assert_eq!(scalar("!0 ? 11 : 12"), 11.0);
}

#[test]
fn scalar_functions_and_constants() {
    close(scalar("sin(30)"), 0.5);
    close(scalar("cos(60) + tan(45)"), 1.5);
    close(scalar("atan2(1,1)"), 45.0);
    close(scalar("acos(0) + atan(1)"), 135.0);
    close(scalar("sqrt(9) + abs(-2) + ln(e)"), 6.0);
    assert_eq!(scalar("log10(100) + log2(8)"), 5.0);
    assert_eq!(
        scalar("round(1.5) + floor(1.9) + ceil(1.1) + int(-1.9)"),
        4.0
    );
    close(scalar("frac(1.25) + sign(-4) + sign(0) + sign(4)"), 0.25);
    close(scalar("deg(pi) + rad(180)"), 180.0 + std::f64::consts::PI);
    assert_eq!(scalar("scalar(5pt)"), 5.0);
    assert_eq!(scalar("pow(2,3) + mod(7,4)"), 11.0);
    assert_eq!(scalar("min(4,2,8) + max(1,6,3)"), 8.0);
    assert_eq!(scalar("Mod(-3,5)"), 2.0);
    assert_eq!(scalar("5!"), 120.0);
}

#[test]
fn round_halves_go_toward_positive_infinity_like_js() {
    assert_eq!(scalar("round(-1.5)"), -1.0);
    assert_eq!(scalar("round(2.5)"), 3.0);
}

#[test]
fn length_units_and_scalar_length_operations() {
    close(length("1cm + 2mm"), 34.1433070866);
    assert_eq!(length("2 * 5pt"), 10.0);
    assert_eq!(length("5pt * 2"), 10.0);
    assert_eq!(length("6pt / 2"), 3.0);
    assert_eq!(scalar("6pt / 3pt"), 2.0);
    assert_eq!(length("(2pt)^3"), 8.0);
    let degrees = 180.0 / std::f64::consts::PI;
    close(scalar("3.5r"), 3.5 * degrees);
    close(scalar("(3)r"), 3.0 * degrees);
    close(scalar("3 r"), 3.0 * degrees);
}

#[test]
fn context_units_override_the_fallbacks() {
    use super::{evaluate_with, EvalOptions};
    let options = EvalOptions {
        em_pt: Some(12.0),
        ex_pt: Some(5.0),
        ..EvalOptions::default()
    };
    assert_eq!(
        evaluate_with("2em", options).unwrap(),
        Quantity::length(24.0)
    );
    assert_eq!(evaluate("2em").unwrap(), Quantity::length(20.0));
    assert_eq!(evaluate("1ex").unwrap(), Quantity::length(4.3));
}

#[test]
fn quantity_conversion_to_length() {
    assert_eq!(
        convert_quantity_to_length(Quantity::length(12.0), "cm"),
        Some(12.0)
    );
    close(
        convert_quantity_to_length(Quantity::scalar(2.0), "cm").unwrap(),
        56.905511811,
    );
    assert_eq!(
        convert_quantity_to_length(Quantity::scalar(2.0), "pt"),
        Some(2.0)
    );
    assert_eq!(
        convert_quantity_to_length(Quantity::scalar(2.0), "zz"),
        None
    );
}

#[test]
fn seeded_random_functions_are_deterministic() {
    let with = |input: &str| {
        evaluate_with_rng(input, &mut PgfRandom::new(1.0))
            .unwrap()
            .value
    };
    close(with("rnd"), 0.69621);
    close(with("random()"), 0.69621);
    close(with("rand"), -0.30379);
    close(with("rand()"), -0.30379);
    for input in ["random(1,10)", "random(10,1)"] {
        assert!((1.0..=10.0).contains(&with(input)), "{input}");
    }
    let mut rng = PgfRandom::new(1.0);
    evaluate_with_rng("random(1,10)", &mut rng).unwrap();
    assert_eq!(rng.seed(), 69_621);
}

#[test]
fn ternary_and_logic_evaluate_both_sides_like_legacy() {
    let mut rng = PgfRandom::new(1.0);
    evaluate_with_rng("1 ? rnd : rnd", &mut rng).unwrap();
    assert_eq!(rng.seed(), 552_116_347);
    let mut rng = PgfRandom::new(1.0);
    evaluate_with_rng("1 || rnd", &mut rng).unwrap();
    assert_eq!(rng.seed(), 69_621);
}

#[test]
fn quoted_expressions_are_unsupported() {
    assert_eq!(
        evaluate("\"foo\"").unwrap_err().code.as_str(),
        "unsupported-syntax"
    );
}

#[test]
fn errors_carry_precise_codes() {
    let failures = [
        ("", "empty"),
        ("{1+2}{3}", "unsupported-syntax"),
        ("1 + @", "token"),
        (".", "token"),
        ("1..2", "unexpected-token"),
        ("1e+", "invalid-domain"),
        ("1 +", "unexpected-token"),
        ("(1+2", "unexpected-token"),
        ("(@)", "token"),
        ("1 ? 2", "unexpected-token"),
        ("1 ? @ : 2", "token"),
        ("0 ? 1 : @", "token"),
        ("1 || @", "token"),
        ("1 && @", "token"),
        ("1 < @", "token"),
        ("1 * @", "token"),
        ("2 ^ @", "token"),
        ("+@", "token"),
        ("-@", "token"),
        ("!@", "token"),
        ("foo", "unknown-function"),
        ("pi()", "unexpected-token"),
        ("sin(1,2)", "invalid-arity"),
        ("min()", "invalid-arity"),
        ("min(2pt)", "invalid-operation"),
        ("random(1,2,3)", "invalid-arity"),
        ("random(2pt)", "invalid-operation"),
        ("random(1,2pt)", "invalid-operation"),
        ("rnd(1)", "invalid-arity"),
        ("rand(1)", "invalid-arity"),
        ("1 / 0", "division-by-zero"),
        ("sqrt(-1)", "invalid-domain"),
        ("asin(2)", "invalid-domain"),
        ("exp(1000)", "invalid-domain"),
        ("171!", "invalid-domain"),
        ("2.5!", "invalid-domain"),
        ("2pt + 3", "invalid-operation"),
        ("2pt * 3pt", "invalid-operation"),
        ("3 / 2pt", "invalid-operation"),
        ("2pt ^ 0.5", "invalid-operation"),
        ("2pt ^ 2pt", "invalid-operation"),
        ("2pt!", "invalid-operation"),
        ("2pt r", "invalid-operation"),
        ("2pt < 3", "invalid-operation"),
        ("sin(2pt)", "invalid-operation"),
        ("atan2(1)", "invalid-arity"),
        ("atan2(1pt,2)", "invalid-operation"),
        ("atan2(1,2pt)", "invalid-operation"),
        ("mod(1,0)", "invalid-domain"),
        ("scalar()", "invalid-arity"),
        ("unknown(1)", "unknown-function"),
        ("foo(1 2)", "unexpected-token"),
    ];
    for (input, code) in failures {
        let error = evaluate_with_rng(input, &mut PgfRandom::new(1.0))
            .expect_err(&format!("{input} should fail"));
        assert_eq!(error.code.as_str(), code, "{input}");
        assert!(!error.message.is_empty(), "{input}");
    }
}

#[test]
fn random_without_a_runtime_is_unsupported() {
    for input in ["rnd", "rand", "rnd()", "rand()", "random()"] {
        assert_eq!(
            evaluate(input).unwrap_err().code.as_str(),
            "unsupported-random",
            "{input}"
        );
    }
}

#[test]
fn runtime_scope_supplies_and_restores_the_rng() {
    use super::runtime;
    {
        let _outer = runtime::install();
        assert!((evaluate("rnd").unwrap().value - 0.69621).abs() < 1e-12);
        {
            let _inner = runtime::install();
            assert!((evaluate("rnd").unwrap().value - 0.69621).abs() < 1e-12);
        }
        // The outer generator kept its advanced state.
        assert!(runtime::with_current_rng(|r| r.map(|r| r.seed())) == Some(69_621));
        assert!(evaluate("rnd").unwrap().value != 0.69621);
    }
    assert!(evaluate("rnd").is_err());
}

#[test]
fn hostile_input_never_panics() {
    for input in [
        "é",
        "1é",
        "((((",
        "))",
        "1e999",
        "9e307*9e307",
        "{{{",
        "\\",
        "1 ? 2 : ",
        "sin(",
        "-",
        "!",
    ] {
        let _ = evaluate_with_rng(input, &mut PgfRandom::new(1.0));
    }
}
