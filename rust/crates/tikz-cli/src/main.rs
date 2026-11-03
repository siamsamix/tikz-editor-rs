#![forbid(unsafe_code)]
//! `tikz-cli`: headless renderer used for golden parity checks.
//!
//! ```text
//! tikz-cli render <input.tex> <output.svg>
//! tikz-cli parse --json <input.tex> <output.json>
//! tikz-cli bench <stage> <corpus_dir> [iterations]
//! ```
//!
//! Exit codes: `0` ok (warnings allowed, printed to stderr), `2` usage or
//! I/O error, `3` the document uses TikZ the port cannot render yet
//! (diagnostics on stderr, no SVG written).
//!
//! Bench stages: `legacy_baseline`, `safe-rust`, `soa-threaded`, `pgo-lto`.

use std::fs;
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
        [cmd, stage, corpus, iters] if cmd == "bench" => {
            let iterations: usize = iters.parse().unwrap_or(8);
            bench(stage, corpus, iterations)
        }
        [cmd, stage, corpus] if cmd == "bench" => bench(stage, corpus, 8),
        _ => {
            eprintln!(
                "usage: tikz-cli render <input.tex> <output.svg>\n       tikz-cli parse --json <input.tex> [output.json]\n       tikz-cli bench <stage> <corpus_dir> [iterations]"
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

/// Benchmark: render every `.tex` file in `corpus_dir` `iterations` times,
/// measuring total wall time, then print a `BENCH` line matching the legacy
/// `migration/golden-capture.mjs` output shape:
///
///     BENCH <stage> docs=<n> iterations=<n> renders=<n> wall_ms=<x> per_render_ms=<y>
///
/// `stage` is one of `legacy_baseline`, `safe-rust`, `soa-threaded`, `pgo-lto`
/// and is only a label recorded in the output — the actual build profile is
/// selected by how the binary was compiled (`cargo build --release` vs `--release`
/// with PGO/LTO, etc.).
fn bench(stage: &str, corpus_dir: &str, iterations: usize) -> ExitCode {
    let corpus: Vec<_> = match fs::read_dir(corpus_dir) {
        Ok(entries) => entries.collect(),
        Err(e) => {
            eprintln!("{corpus_dir}: {e}");
            return ExitCode::from(2);
        }
    };

    let mut sources: Vec<String> = Vec::new();
    for entry in corpus.into_iter().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tex") {
            match fs::read_to_string(&path) {
                Ok(text) => sources.push(text),
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    return ExitCode::from(2);
                }
            }
        }
    }
    sources.sort();

    if sources.is_empty() {
        eprintln!("{corpus_dir}: no .tex fixtures found");
        return ExitCode::from(2);
    }

    // Warm-up render (font loading, etc.).
    for source in &sources {
        let _ = render_tex_with_warnings(source);
    }

    let t0 = std::time::Instant::now();
    let mut renders = 0usize;
    for _ in 0..iterations {
        for source in &sources {
            match render_tex_with_warnings(source) {
                Ok(_) => renders += 1,
                Err(diagnostics) => {
                    for d in &diagnostics {
                        eprintln!("{}", describe_source(source, d));
                    }
                    return ExitCode::from(3);
                }
            }
        }
    }
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let per_render = wall_ms / renders as f64;

    println!(
        "BENCH {stage} docs={} iterations={} renders={} wall_ms={:.1} per_render_ms={:.2}",
        sources.len(),
        iterations,
        renders,
        wall_ms,
        per_render
    );

    ExitCode::SUCCESS
}

/// `line:col: message` for a diagnostic (1-based, counted in chars).
fn describe(source: &str, d: &Diagnostic) -> String {
    let (line, col) = d.line_col(source);
    format!("{line}:{col}: {}", d.message)
}

/// `line:col: message` for a diagnostic against an in-memory source string.
fn describe_source(source: &str, d: &Diagnostic) -> String {
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
