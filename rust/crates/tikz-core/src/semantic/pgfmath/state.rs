//! `\pgfmath…` statements and the macro bindings they write.
//!
//! Port of `applyPgfMathStatement` (`semantic/evaluate.ts`) plus the part of
//! `macros/expand.ts` those statements need: control sequences bound by
//! `\pgfmathsetmacro` / `\pgfmathparse` (`\pgfmathresult`) are substituted
//! into later statements before they are evaluated.
//!
//! ```
//! use tikz_core::semantic::pgfmath::state::{apply_statement, Arg, MacroBindings};
//! use tikz_core::span::Span;
//! let mut bindings = MacroBindings::default();
//! let mut issues = Vec::new();
//! let args = [Arg { raw: "\\w", span: Span::new(0, 2) }, Arg { raw: "2+3", span: Span::new(3, 6) }];
//! apply_statement(&mut bindings, "pgfmathsetmacro", &args, Span::new(0, 7), &mut issues);
//! assert!(issues.is_empty());
//! assert_eq!(bindings.expand("(\\w,0)"), "(5,0)");
//! ```

use std::collections::BTreeMap;

use super::{evaluate, fmt::format_number, runtime};
use crate::semantic::style::Issue;
use crate::span::Span;

/// Legacy `DEFAULT_MACRO_EXPANSION_MAX_DEPTH`.
const MAX_EXPANSION_DEPTH: usize = 100;

/// Control-sequence → replacement text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MacroBindings {
    values: BTreeMap<String, String>,
}

/// One brace-group argument of a `\pgfmath…` command.
#[derive(Debug, Clone, Copy)]
pub struct Arg<'a> {
    /// Text inside the braces.
    pub raw: &'a str,
    /// Source span of that text.
    pub span: Span,
}

impl MacroBindings {
    /// Whether no macro is bound (expansion is then the identity).
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Bind `name` (including its backslash).
    pub fn set(&mut self, name: &str, value: String) {
        self.values.insert(name.to_string(), value);
    }

    /// Replacement text for `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Substitute bound control sequences until a fixed point (or a repeat).
    pub fn expand(&self, input: &str) -> String {
        if input.is_empty() || self.is_empty() {
            return input.to_string();
        }
        let mut current = input.to_string();
        let mut seen = vec![current.clone()];
        for _ in 0..MAX_EXPANSION_DEPTH {
            let next = self.substitute_once(&current);
            if next == current || seen.contains(&next) {
                return next;
            }
            seen.push(next.clone());
            current = next;
        }
        current
    }

    fn substitute_once(&self, input: &str) -> String {
        let bytes = input.as_bytes();
        let mut output = String::with_capacity(input.len());
        let (mut cursor, mut at) = (0, 0);
        while at < bytes.len() {
            if bytes[at] != b'\\' {
                at += 1;
                continue;
            }
            let mut end = at + 1;
            while end < bytes.len() && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'@') {
                end += 1;
            }
            if let Some(value) = (end > at + 1).then(|| self.get(&input[at..end])).flatten() {
                output.push_str(&input[cursor..at]);
                output.push_str(value);
                // Keep the replacement from fusing with a following character.
                let next = input[end..].chars().next();
                let last = value.chars().last();
                if last.is_some_and(|c| c.is_ascii_alphanumeric())
                    && next.is_some_and(|c| c.is_ascii_alphanumeric())
                {
                    output.push_str("{}");
                }
                cursor = end;
            }
            at = end.max(at + 1);
        }
        output.push_str(&input[cursor..]);
        output
    }
}

/// `\name` made only of letters/`@`, or `None`.
fn control_sequence_name(raw: &str) -> Option<&str> {
    let name = raw.trim();
    let letters = name.strip_prefix('\\')?;
    (!letters.is_empty()
        && letters
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'@'))
    .then_some(name)
}

/// Evaluate one expression argument, reporting failures as legacy warnings.
fn evaluate_arg(
    bindings: &MacroBindings,
    command: &str,
    arg: Option<Arg<'_>>,
    statement: Span,
    issues: &mut Vec<Issue>,
) -> Option<f64> {
    let (raw, span) = arg.map_or(("", statement), |a| (a.raw, a.span));
    match evaluate(&bindings.expand(raw)) {
        Ok(quantity) => Some(quantity.value),
        Err(error) => {
            issues.push(
                Issue::warning(span, format!("\\{command} failed: {}", error.message))
                    .with_code(format!("invalid-{command}:{}", error.code.as_str())),
            );
            None
        }
    }
}

/// Apply `\pgfmathsetseed`, `\pgfmathparse` or `\pgfmathsetmacro`.
///
/// `command` has no backslash. Failures are warnings; the statement then has
/// no effect, exactly like the legacy evaluator.
pub fn apply_statement(
    bindings: &mut MacroBindings,
    command: &str,
    args: &[Arg<'_>],
    statement: Span,
    issues: &mut Vec<Issue>,
) {
    match command {
        "pgfmathsetseed" => {
            if let Some(value) =
                evaluate_arg(bindings, command, args.first().copied(), statement, issues)
            {
                runtime::set_seed(value.trunc());
            }
        }
        "pgfmathparse" => {
            if let Some(value) =
                evaluate_arg(bindings, command, args.first().copied(), statement, issues)
            {
                bindings.set("\\pgfmathresult", format_number(value));
            }
        }
        _ => {
            let target_arg = args.first().copied();
            let Some(target) = target_arg.and_then(|a| control_sequence_name(a.raw)) else {
                let span = target_arg.map_or(statement, |a| a.span);
                issues.push(
                    Issue::warning(
                        span,
                        "\\pgfmathsetmacro requires a control-sequence macro target.",
                    )
                    .with_code("invalid-pgfmathsetmacro-target"),
                );
                return;
            };
            let target = target.to_string();
            if let Some(value) =
                evaluate_arg(bindings, command, args.get(1).copied(), statement, issues)
            {
                bindings.set(&target, format_number(value));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg(raw: &str) -> Arg<'_> {
        Arg {
            raw,
            span: Span::new(0, raw.len()),
        }
    }

    #[test]
    fn setmacro_binds_formatted_values_and_expands_chained_macros() {
        let mut b = MacroBindings::default();
        let mut issues = Vec::new();
        let at = Span::new(0, 0);
        apply_statement(
            &mut b,
            "pgfmathsetmacro",
            &[arg("\\rm"), arg("3")],
            at,
            &mut issues,
        );
        apply_statement(
            &mut b,
            "pgfmathsetmacro",
            &[arg("\\rc"), arg("\\rm *2/3")],
            at,
            &mut issues,
        );
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(b.get("\\rc"), Some("2"));
        assert_eq!(b.expand("(\\rm cm,\\rc cm)"), "(3 cm,2 cm)");
    }

    #[test]
    fn parse_writes_pgfmathresult() {
        let mut b = MacroBindings::default();
        let mut issues = Vec::new();
        apply_statement(
            &mut b,
            "pgfmathparse",
            &[arg("1<2 ? 9 : 4")],
            Span::new(0, 0),
            &mut issues,
        );
        assert_eq!(b.get("\\pgfmathresult"), Some("9"));
        assert_eq!(b.expand("\\pgfmathresult/\\m"), "9/\\m");
    }

    #[test]
    fn malformed_commands_warn_with_legacy_codes() {
        let mut b = MacroBindings::default();
        let mut issues = Vec::new();
        let at = Span::new(0, 0);
        apply_statement(&mut b, "pgfmathsetseed", &[arg("1+")], at, &mut issues);
        apply_statement(&mut b, "pgfmathparse", &[arg("1+")], at, &mut issues);
        apply_statement(
            &mut b,
            "pgfmathsetmacro",
            &[arg("bad-target"), arg("1")],
            at,
            &mut issues,
        );
        apply_statement(
            &mut b,
            "pgfmathsetmacro",
            &[arg("\\bad"), arg("1+")],
            at,
            &mut issues,
        );
        let codes: Vec<_> = issues.iter().filter_map(|i| i.code.as_deref()).collect();
        assert_eq!(
            codes,
            [
                "invalid-pgfmathsetseed:unexpected-token",
                "invalid-pgfmathparse:unexpected-token",
                "invalid-pgfmathsetmacro-target",
                "invalid-pgfmathsetmacro:unexpected-token",
            ]
        );
        assert!(b.is_empty());
    }

    #[test]
    fn expansion_is_unicode_safe_and_leaves_unbound_names() {
        let mut b = MacroBindings::default();
        b.set("\\a", "1".to_string());
        assert_eq!(b.expand("é\\a ¬ \\b \\"), "é1 ¬ \\b \\");
    }

    #[test]
    fn digits_after_a_macro_do_not_fuse() {
        let mut b = MacroBindings::default();
        b.set("\\a", "3".to_string());
        assert_eq!(b.expand("\\a5"), "3{}5");
    }
}
