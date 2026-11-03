#![forbid(unsafe_code)]
//! `tikz-cli`: headless renderer used for golden parity checks.
//!
//! ```text
//! tikz-cli render <input.tex> <output.svg>
//! tikz-cli parse --json <input.tex> <output.json>
//! ```
//!
//! Exit codes: `0` ok (warnings allowed, printed to stderr), `2` usage or
//! I/O error, `3` the document uses TikZ the port cannot render yet
//! (diagnostics on stderr, no SVG written).

use std::process::ExitCode;
use tikz_core::parser::parse_tex;
use tikz_core::render::{render_tex_with_warnings, Diagnostic};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, input, output] if cmd == "render" => render(input, output),
        [cmd, flag, input, output] if cmd == "parse" && flag == "--json" => {
            parse_json(input, output)
        }
        [cmd, input, output] if cmd == "parse" => parse_json(input, output),
        _ => {
            eprintln!(
                "usage: tikz-cli render <input.tex> <output.svg>\n       tikz-cli parse --json <input.tex> [output.json]"
            );
            ExitCode::from(2)
        }
    }
}

fn render(input: &str, output: &str) -> ExitCode {
    let source = match std::fs::read_to_string(input) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::from(2);
        }
    };
    match render_tex_with_warnings(&source) {
        Ok(rendered) => {
            for warning in &rendered.warnings {
                let (line, col) = warning.line_col(&source);
                eprintln!("{input}:{line}:{col}: warning: {}", warning.message);
            }
            match std::fs::write(output, rendered.svg) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{output}: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Err(diagnostics) => {
            for d in &diagnostics {
                eprintln!("{input}:{}", describe(&source, d));
            }
            ExitCode::from(3)
        }
    }
}

fn parse_json(input: &str, output: &str) -> ExitCode {
    let source = match std::fs::read_to_string(input) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::from(2);
        }
    };
    let document = parse_tex(&source);
    let json = match serde_json::to_string_pretty(&document) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("serialize: {e}");
            return ExitCode::from(2);
        }
    };
    match std::fs::write(output, json) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{output}: {e}");
            ExitCode::from(2)
        }
    }
}

/// `line:col: message` for a diagnostic (1-based, counted in chars).
fn describe(source: &str, d: &Diagnostic) -> String {
    let (line, col) = d.line_col(source);
    format!("{line}:{col}: {}", d.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tikz_core::span::Span;

    #[test]
    fn describe_reports_line_and_column_without_the_message_body() {
        let d = Diagnostic::error(Span::new(6, 8), "bad");
        assert_eq!(describe("ab\ncd\nef", &d), "3:1: bad");
        let d = Diagnostic::warning(Span::new(4, 5), "bad");
        assert_eq!(describe("ab\ncd\nef", &d), "2:2: bad");
    }

    #[test]
    fn warnings_render_and_errors_do_not() {
        let with_warning = "\\begin{tikzpicture}\\draw (0,0) -- (1,1) node {A};\\end{tikzpicture}";
        assert!(render_tex_with_warnings(with_warning).is_ok());
        let with_error = "\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}";
        assert!(render_tex_with_warnings(with_error).is_err());
    }
}
