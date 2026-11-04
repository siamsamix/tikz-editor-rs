//! Normalisation and tokenizer for PGF math expressions.

use super::{ErrorCode, PgfMathError};

type Eval<T> = Result<T, PgfMathError>;

#[derive(Debug, Clone)]
pub(super) enum Token {
    Number { value: f64, unit: Option<String> },
    Ident(String),
    Op(&'static str),
}

fn fail<T>(code: ErrorCode, message: impl Into<String>) -> Eval<T> {
    Err(PgfMathError::new(code, message))
}

/// Strip whitespace and redundant outer `{…}` pairs (legacy `normalizeQuantityInput`).
pub(super) fn normalize_input(input: &str) -> String {
    let mut normalized = input.trim().to_string();
    while normalized.chars().count() >= 2
        && normalized.starts_with('{')
        && normalized.ends_with('}')
    {
        match unwrap_single_outer_braces(&normalized) {
            Some(inner) => normalized = inner.trim().to_string(),
            None => break,
        }
    }
    normalized
}

fn unwrap_single_outer_braces(raw: &str) -> Option<String> {
    let chars: Vec<char> = raw.chars().collect();
    let mut depth = 0i32;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 1,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 || (depth == 0 && index != chars.len() - 1) {
                    return None;
                }
            }
            _ => {}
        }
        index += 1;
    }
    (depth == 0).then(|| chars[1..chars.len() - 1].iter().collect())
}

fn two_char_op(pair: &str) -> Option<&'static str> {
    ["&&", "||", "==", "!=", "<=", ">="]
        .into_iter()
        .find(|op| *op == pair)
}

fn one_char_op(c: char) -> Option<&'static str> {
    Some(match c {
        '+' => "+",
        '-' => "-",
        '*' => "*",
        '/' => "/",
        '^' => "^",
        '(' => "(",
        ')' => ")",
        '?' => "?",
        ',' => ",",
        ':' => ":",
        '!' => "!",
        '<' => "<",
        '>' => ">",
        _ => return None,
    })
}

pub(super) fn tokenize(input: &str) -> Eval<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if i + 1 < chars.len() {
            let pair: String = chars[i..i + 2].iter().collect();
            if let Some(op) = two_char_op(&pair) {
                tokens.push(Token::Op(op));
                i += 2;
                continue;
            }
        }
        if let Some(op) = one_char_op(c) {
            tokens.push(Token::Op(op));
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            i = read_number(&chars, i, &mut tokens)?;
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
            continue;
        }
        return fail(
            ErrorCode::Token,
            format!("Unexpected token '{c}' in expression."),
        );
    }
    Ok(tokens)
}

/// Read a number (plus optional exponent and unit); returns the next index.
fn read_number(chars: &[char], start: usize, tokens: &mut Vec<Token>) -> Eval<usize> {
    let digits = |mut at: usize| {
        while at < chars.len() && chars[at].is_ascii_digit() {
            at += 1;
        }
        at
    };
    let mut i = start;
    if chars[i] == '.' {
        let after = digits(i + 1);
        if after == i + 1 {
            return fail(ErrorCode::Token, "Invalid decimal number.");
        }
        i = after;
    } else {
        i = digits(i);
        if i < chars.len() && chars[i] == '.' {
            i = digits(i + 1);
        }
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut at = i + 1;
        if at < chars.len() && (chars[at] == '+' || chars[at] == '-') {
            at += 1;
        }
        let after = digits(at);
        if after != at {
            i = after;
        }
    }
    let text: String = chars[start..i].iter().collect();
    let value = text
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map_or_else(|| fail(ErrorCode::Token, "Invalid numeric literal."), Ok)?;

    let mut cursor = i;
    while cursor < chars.len() && chars[cursor].is_whitespace() {
        cursor += 1;
    }
    let unit_start = cursor;
    while cursor < chars.len() && chars[cursor].is_ascii_alphabetic() {
        cursor += 1;
    }
    let unit = (cursor > unit_start).then(|| chars[unit_start..cursor].iter().collect::<String>());
    if unit.is_some() {
        i = cursor;
    }
    tokens.push(Token::Number { value, unit });
    Ok(i)
}
