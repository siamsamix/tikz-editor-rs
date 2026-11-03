//! Semantic layer: statements -> [`Scene`].
//!
//! This is the Rust counterpart of `semantic/evaluate.ts`, collapsed to one
//! pure driver. It owns the figure option list, the statement dispatch and
//! the per-statement style state; geometry lives in
//! [`crate::semantic::pathbuild`] and paint in [`crate::semantic::style`].
//!
//! Every statement the port does not support produces an issue. Errors stop
//! the document; warnings are reported in the UI while the rest of the
//! figure still renders — the canvas is never quietly wrong.

pub mod color;
pub mod expr;
pub mod pathbuild;
pub mod pathscan;
pub mod shapes;
pub mod style;
pub mod transform;

use crate::figure_scan::scan_tikz_figures;
use crate::ids::path_statement_id;
use crate::options::parse_option_list_raw;
use crate::scene::{Paint, Scene};
use crate::span::Span;

pub use style::{Issue, Severity};

use pathbuild::PathContext;
use style::StyleState;

/// Compile the whole source into a scene plus the issues found on the way.
///
/// Multi-figure documents are not supported yet: more than one `tikzpicture`
/// is an error, so the canvas never silently shows one figure of several.
pub fn compile_scene(source: &str) -> Result<(Scene, Vec<Issue>), Vec<Issue>> {
    let figures = scan_tikz_figures(source);
    let figure = match figures.as_slice() {
        [one] => one,
        [] => {
            return Err(vec![Issue::error(
                Span::new(0, source.len()),
                "no tikzpicture found",
            )])
        }
        [_, second, ..] => {
            return Err(vec![Issue::error(
                second.span,
                "multi-figure documents are not supported yet",
            )])
        }
    };

    let mut issues = Vec::new();
    let (mut cursor, end) = (figure.begin_span.to, figure.end_span.from);
    cursor = skip_trivia(source, cursor, end);
    let mut figure_state = StyleState::default();
    if source[cursor..end].starts_with('[') {
        let Some(close) = matching(source, cursor, end, b'[', b']') else {
            return Err(vec![Issue::error(
                Span::new(cursor, end),
                "unterminated figure options",
            )]);
        };
        let list = parse_option_list_raw(&source[cursor..=close], cursor);
        figure_state.apply(&list, &mut issues);
        cursor = close + 1;
    }

    let mut scene = Scene::default();
    for (index, (from, to, terminated)) in split_statements(source, cursor, end)
        .into_iter()
        .enumerate()
    {
        if !terminated {
            issues.push(Issue::error(
                Span::new(from, to),
                "statement is missing its terminating `;`",
            ));
            continue;
        }
        compile_statement(
            source,
            &figure_state,
            index,
            from,
            to,
            &mut scene,
            &mut issues,
        );
    }

    if issues.iter().any(|issue| issue.severity == Severity::Error) {
        Err(issues)
    } else {
        Ok((scene, issues))
    }
}

/// Compile one `;`-terminated statement (`to` excludes the `;`).
fn compile_statement(
    source: &str,
    figure_state: &StyleState,
    index: usize,
    from: usize,
    to: usize,
    scene: &mut Scene,
    issues: &mut Vec<Issue>,
) {
    let at = skip_trivia(source, from, to);
    if at >= to {
        return; // only whitespace/comments before the `;`
    }
    let (command, after) = read_command(source, at, to);
    let mut state = figure_state.clone();
    match command {
        "draw" | "path" => {}
        "filldraw" | "shadedraw" => {
            state.style.fill = fill_from_stroke(&state.style.stroke);
        }
        "fill" | "shade" => {
            state.style.fill = fill_from_stroke(&state.style.stroke);
            state.style.stroke = Paint::None;
            state.draw_explicit = false;
        }
        "clip" => {
            issues.push(Issue::warning(
                Span::new(at, after),
                "`\\clip` is ignored (clipping paths are not supported yet)",
            ));
            return;
        }
        other => {
            issues.push(Issue::warning(
                Span::new(at, to),
                format!("`\\{other}` statements are not supported yet"),
            ));
            return;
        }
    }
    let mut cursor = skip_trivia(source, after, to);
    if source[cursor..to].starts_with('[') {
        let Some(close) = matching(source, cursor, to, b'[', b']') else {
            issues.push(Issue::error(
                Span::new(cursor, to),
                "unterminated option list",
            ));
            return;
        };
        let list = parse_option_list_raw(&source[cursor..=close], cursor);
        state.apply(&list, issues);
        cursor = close + 1;
    }
    let mut ctx = PathContext {
        source,
        source_id: path_statement_id(index),
        state,
    };
    scene
        .elements
        .extend(pathbuild::build_path(&mut ctx, cursor, to, issues));
}

/// `\fill` with no colour paints the current colour; the port's base stroke
/// doubles as that "current colour" default.
fn fill_from_stroke(stroke: &Paint) -> Paint {
    match stroke {
        Paint::None => Paint::Black,
        other => *other,
    }
}

/// Read a `\name` command, returning its name without the backslash.
fn read_command(source: &str, at: usize, to: usize) -> (&str, usize) {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'\\') {
        return ("", at);
    }
    let mut end = at + 1;
    while end < to && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'*') {
        end += 1;
    }
    (&source[at + 1..end], end)
}

/// Split `[from, end)` at top-level `;` into `(from, to, terminated)`.
///
/// The final unterminated chunk is returned too (`terminated == false`), so a
/// missing `;` or trailing junk is diagnosed rather than ignored.
fn split_statements(src: &str, from: usize, end: usize) -> Vec<(usize, usize, bool)> {
    let bytes = src.as_bytes();
    let (mut out, mut start, mut depth, mut at) = (Vec::new(), from, 0i32, from);
    while at < end {
        match bytes[at] {
            b'%' => {
                while at < end && bytes[at] != b'\n' {
                    at += 1;
                }
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b';' if depth <= 0 => {
                out.push((start, at, true));
                start = at + 1;
            }
            _ => {}
        }
        at += 1;
    }
    if skip_trivia(src, start, end) < end {
        out.push((start, end, false));
    }
    out
}

fn skip_trivia(src: &str, mut at: usize, to: usize) -> usize {
    while at < to {
        let byte = src.as_bytes()[at];
        if byte.is_ascii_whitespace() {
            at += 1;
        } else if byte == b'%' {
            while at < to && src.as_bytes()[at] != b'\n' {
                at += 1;
            }
        } else {
            break;
        }
    }
    at
}
/// Index of the delimiter closing the one at `open` (comments are inert).
fn matching(src: &str, open: usize, end: usize, lo: u8, hi: u8) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut at = open;
    while at < end {
        match bytes[at] {
            b'%' => {
                while at < end && bytes[at] != b'\n' {
                    at += 1;
                }
                continue;
            }
            b if b == lo => depth += 1,
            b if b == hi => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{PathCmd, SceneElement};

    fn scene_of(source: &str) -> (Scene, Vec<Issue>) {
        compile_scene(source).unwrap_or_else(|issues| panic!("compile failed: {issues:?}"))
    }

    fn only_path(scene: &Scene) -> &crate::scene::ScenePath {
        match scene.elements.first() {
            Some(SceneElement::Path(path)) => path,
            other => panic!("expected a path, got {other:?}"),
        }
    }

    #[test]
    fn figure_options_transform_coordinates() {
        let (scene, issues) =
            scene_of("\\begin{tikzpicture}[scale=0.6]\\draw (0,0) -- (1,1);\\end{tikzpicture}");
        assert!(issues.is_empty(), "{issues:?}");
        let path = only_path(&scene);
        assert_eq!(
            path.cmds,
            vec![
                PathCmd::Move(crate::units::Vec2::new(0.0, 0.0)),
                PathCmd::Line(crate::units::Vec2::new(
                    0.6 * crate::units::CM_TO_PT,
                    0.6 * crate::units::CM_TO_PT
                )),
            ]
        );
    }

    #[test]
    fn path_options_style_the_stroke() {
        let (scene, _) = scene_of(
            "\\begin{tikzpicture}\\draw[very thick, dashed, blue] (0,0) -- (1,0);\\end{tikzpicture}",
        );
        let style = &only_path(&scene).style;
        assert_eq!(style.stroke_width, 1.2);
        assert_eq!(style.stroke, Paint::Rgb(0, 0, 255));
        assert_eq!(style.dash, Some(vec![3.0, 3.0]));
    }

    #[test]
    fn grid_expands_to_one_line_element_per_axis_step() {
        let (scene, issues) =
            scene_of("\\begin{tikzpicture}\\draw (0,0) grid (2,2);\\end{tikzpicture}");
        assert!(issues.is_empty(), "{issues:?}");
        // x = 0,1,2 vertical then y = 0,1,2 horizontal.
        assert_eq!(scene.len(), 6);
        assert!(scene.elements.iter().all(|e| e.source_id() == "path:0"));
        let step = crate::units::CM_TO_PT;
        let expected = vec![
            PathCmd::Move(crate::units::Vec2::new(0.0, 0.0)),
            PathCmd::Line(crate::units::Vec2::new(0.0, 2.0 * step)),
        ];
        assert_eq!(only_path(&scene).cmds, expected);
    }

    #[test]
    fn circle_and_ellipse_keep_their_legacy_element_kinds() {
        let (scene, issues) = scene_of(
            "\\begin{tikzpicture}\\draw (3,3) circle [radius=0.6];\\draw[purple] (2,1) ellipse (0.5 and 0.25);\\end{tikzpicture}",
        );
        assert!(issues.is_empty(), "{issues:?}");
        assert!(matches!(scene.elements[0], SceneElement::Path(_)));
        match &scene.elements[1] {
            SceneElement::Ellipse(ellipse) => {
                assert!((ellipse.rx - 0.5 * crate::units::CM_TO_PT).abs() < 1e-9);
                assert!((ellipse.ry - 0.25 * crate::units::CM_TO_PT).abs() < 1e-9);
                assert_eq!(ellipse.style.stroke, Paint::Rgb(0xbf, 0x00, 0x40));
            }
            other => panic!("expected an ellipse, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_statements_warn_but_keep_the_figure() {
        let (scene, issues) = scene_of(
            "\\begin{tikzpicture}\\node at (0,0) {A};\\draw (0,0) -- (1,1);\\end{tikzpicture}",
        );
        assert_eq!(scene.len(), 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }

    #[test]
    fn errors_are_returned_and_nothing_is_rendered() {
        let issues = compile_scene("\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}")
            .expect_err("named coordinates must fail");
        assert!(issues.iter().any(|issue| issue.severity == Severity::Error));
    }

    #[test]
    fn multiple_figures_are_reported() {
        let source = "\\begin{tikzpicture}\\draw (0,0)--(1,1);\\end{tikzpicture}\\begin{tikzpicture}\\draw (0,0)--(1,1);\\end{tikzpicture}";
        assert!(compile_scene(source).is_err());
    }
}
