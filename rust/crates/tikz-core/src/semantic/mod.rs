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
pub mod pathcurve;
pub mod pathscan;
pub mod shapes;
pub mod style;
pub mod transform;

use crate::ids::path_statement_id;
use crate::options::parse_option_list_raw;
use crate::parser::{convert_parsed_option_list, parse_tex, StatementKind};
use crate::scene::{Paint, Scene, SceneElement, SceneText};
use crate::semantic::pathscan::NamedCoordinates;
use crate::span::Span;
use crate::units::Vec2;

pub use style::{Issue, Severity};

use pathbuild::PathContext;
use style::StyleState;

/// Compile the whole source into a scene plus the issues found on the way.
///
/// Every `tikzpicture` environment in the document is compiled into one
/// merged [`Scene`]. Each figure gets its own figure-level option state and
/// its own named-coordinate scope, so options and `(name)` definitions never
/// leak between figures. Statement ids are drawn from a global counter so
/// `data-source-id`s stay unique across figures (for a single figure they are
/// the legacy `path:N`, `node:N` ids).
///
/// Errors stop the document: if any figure produces an error-severity issue,
/// the whole result is `Err`, but all figures are still evaluated so the UI
/// can show whatever is renderable.
pub fn compile_scene(source: &str) -> Result<(Scene, Vec<Issue>), Vec<Issue>> {
    let document = parse_tex(source);
    if document.figures.is_empty() {
        return Err(document
            .diagnostics
            .into_iter()
            .map(convert_parsed_diagnostic)
            .collect());
    }

    let mut issues: Vec<Issue> = document
        .diagnostics
        .into_iter()
        .map(convert_parsed_diagnostic)
        .collect();
    let mut scene = Scene::default();
    // Global statement counter: keeps `data-source-id`s unique across figures
    // and byte-identical to the legacy `path:N` scheme for a single figure.
    let mut next_statement_index = 0usize;

    for figure in &document.figures {
        let mut figure_state = StyleState::default();
        let mut named = NamedCoordinates::default();

        // Figure-level options.
        if let Some(options) = &figure.options {
            let list = convert_parsed_option_list(options.clone());
            figure_state.apply(&list, &mut issues);
        }

        for stmt in &figure.statements {
            let index = next_statement_index;
            next_statement_index += 1;

            match &stmt.kind {
                StatementKind::Node => {
                    let (_command, after) = read_command(source, stmt.span.from, stmt.span.to);
                    compile_node_statement(
                        source,
                        &figure_state,
                        &mut named,
                        index,
                        after,
                        stmt.span.to,
                        &mut scene,
                        &mut issues,
                    );
                }
                StatementKind::Path => match stmt.command.as_deref() {
                    Some("coordinate") => {
                        let (_command, after) = read_command(source, stmt.span.from, stmt.span.to);
                        compile_coordinate_statement(
                            source,
                            &figure_state,
                            &mut named,
                            index,
                            after,
                            stmt.span.to,
                            &mut issues,
                        );
                    }
                    Some(command) => compile_statement_from_ast(
                        source,
                        &mut figure_state,
                        &mut named,
                        index,
                        command,
                        &stmt.items,
                        &mut scene,
                        &mut issues,
                    ),
                    None => {
                        issues.push(Issue::error(
                            stmt.span,
                            "statement is missing its terminating `;`",
                        ));
                    }
                },
                StatementKind::Unknown => {
                    // Diagnostic already pushed during parsing (see parse_statement).
                }
                StatementKind::Coordinate => {
                    let (_command, after) = read_command(source, stmt.span.from, stmt.span.to);
                    compile_coordinate_statement(
                        source,
                        &figure_state,
                        &mut named,
                        index,
                        after,
                        stmt.span.to,
                        &mut issues,
                    );
                }
                StatementKind::Scope
                | StatementKind::Foreach
                | StatementKind::TikzStyle
                | StatementKind::TikzSet
                | StatementKind::TikzLibrary
                | StatementKind::Pgfkeys
                | StatementKind::PgfMath
                | StatementKind::MacroDefinition
                | StatementKind::MacroAlias
                | StatementKind::MacroCommandDefinition
                | StatementKind::Colorlet
                | StatementKind::DefineColor => {
                    issues.push(Issue::warning(
                        stmt.span,
                        format!(
                            "`\\{}` statements are not supported yet",
                            stmt.command.as_deref().unwrap_or(""),
                        ),
                    ));
                }
            }
        }
    }

    if issues.iter().any(|issue| issue.severity == Severity::Error) {
        Err(issues)
    } else {
        Ok((scene, issues))
    }
}

/// Compile one `;`-terminated path statement from its [`ParsedStatement`] AST
/// items, replacing the legacy text scanner.
#[allow(clippy::too_many_arguments)]
fn compile_statement_from_ast(
    source: &str,
    figure_state: &mut StyleState,
    named: &mut NamedCoordinates,
    index: usize,
    command: &str,
    items: &[crate::parser::ParsedPathItem],
    scene: &mut Scene,
    issues: &mut Vec<Issue>,
) {
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
                Span::new(
                    items.first().map(|i| i.span.from).unwrap_or(0),
                    items.last().map(|i| i.span.to).unwrap_or(0),
                ),
                "`\\clip` is ignored (clipping paths are not supported yet)",
            ));
            return;
        }
        "tikzset" | "tikzstyle" => {
            figure_state.style_definitions = state.style_definitions.clone();
            return;
        }
        other => {
            issues.push(Issue::warning(
                Span::new(
                    items.first().map(|i| i.span.from).unwrap_or(0),
                    items.last().map(|i| i.span.to).unwrap_or(0),
                ),
                format!("`\\{other}` statements are not supported yet"),
            ));
            return;
        }
    }

    // Parse command name + items into PathItem AST.
    let span = items
        .first()
        .map(|i| i.span)
        .unwrap_or_else(|| Span::new(0, 0));
    let stmt = crate::parser::ParsedStatement {
        kind: StatementKind::Path,
        id: path_statement_id(index),
        span,
        command: Some(command.to_string()),
        items: items.to_vec(),
    };
    let (_command, path_items) = crate::parser::parsed_statement_to_ast(source, &stmt);

    let mut ctx = PathContext {
        source_id: path_statement_id(index),
        state,
        named,
    };
    scene
        .elements
        .extend(pathbuild::build_path(&mut ctx, &path_items, issues));
}

/// Compile one `\node … ;` statement into a [`SceneText`] element.
///
/// Supported subset: `[options]`, a leading named coordinate `(name)` or a
/// placement `(x,y)`, an optional `at (x,y)`, and a trailing `{text}` group.
/// A leading bare `(name)` registers that name at the resolved placement so
/// later path statements can refer to it (`\draw (name) -- ...`). Unsupported
/// constructs inside a node produce issues but never drop the statement
/// silently.
#[allow(clippy::too_many_arguments)]
fn compile_node_statement(
    source: &str,
    figure_state: &StyleState,
    named: &mut NamedCoordinates,
    index: usize,
    from: usize,
    to: usize,
    scene: &mut Scene,
    issues: &mut Vec<Issue>,
) {
    // Skip non-char-boundary bytes (invalid UTF-8 from fuzz input).
    let mut cursor = skip_trivia(source, from, to);
    while cursor < to && !source.is_char_boundary(cursor) {
        cursor += 1;
    }
    let mut state = figure_state.clone();

    // Options `[…]`.
    if cursor < to && source[cursor..].starts_with('[') {
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

    // Read placement: optional leading `(name)` definition, then optional
    // `(x,y)` / `at (x,y)`. The leading bare name, if any, is registered at the
    // resolved placement after the loop.
    let mut center = Vec2::new(0.0, 0.0);
    let mut center_set = false;
    let mut defined_name: Option<String> = None;
    while cursor < to {
        cursor = skip_trivia(source, cursor, to);
        let byte = source.as_bytes()[cursor];
        if byte == b'{' {
            // The trailing `{text}` group is handled below; stop scanning here.
            break;
        }
        if byte == b'(' {
            let Some(close) = matching(source, cursor, to, b'(', b')') else {
                issues.push(Issue::error(
                    Span::new(cursor, to),
                    "unterminated coordinate",
                ));
                return;
            };
            let inner = &source[cursor + 1..close];
            let trimmed = inner.trim();
            if pathscan::is_bare_name(trimmed) && defined_name.is_none() {
                // Leading `(name)`: a coordinate definition, resolved at the
                // placement below once the placement point is known.
                defined_name = Some(trimmed.to_string());
            } else if let Ok(point) = pathscan::parse_coordinate(inner, Some(named)) {
                if !center_set {
                    center = point;
                    center_set = true;
                } else {
                    center = point; // `at` overrides an earlier placement.
                }
            }
            cursor = close + 1;
            continue;
        }
        let (word, after) = pathscan::read_word(source, cursor, to);
        if word.is_empty() {
            cursor = (cursor + 1).min(to);
            continue;
        }
        if word == "at" {
            cursor = after;
            cursor = skip_trivia(source, cursor, to);
            if source.as_bytes().get(cursor) == Some(&b'(') {
                let Some(close) = matching(source, cursor, to, b'(', b')') else {
                    issues.push(Issue::error(
                        Span::new(cursor, to),
                        "unterminated coordinate",
                    ));
                    return;
                };
                let inner = &source[cursor + 1..close];
                if let Ok(point) = pathscan::parse_coordinate(inner, Some(named)) {
                    center = point;
                    center_set = true;
                }
                cursor = close + 1;
            }
            continue;
        }
        // Anything else ends placement scanning; the rest is the text group.
        break;
    }

    // Register a leading `(name)` definition at the resolved placement point.
    if let Some(name) = defined_name.take() {
        named.insert(&name, center);
    }

    // Trailing `{text}` group.
    let mut text = String::new();
    cursor = skip_trivia(source, cursor, to);
    if source.as_bytes().get(cursor) == Some(&b'{') {
        let Some(close) = matching(source, cursor, to, b'{', b'}') else {
            issues.push(Issue::error(
                Span::new(cursor, to),
                "unterminated node text",
            ));
            return;
        };
        text = source[cursor + 1..close].to_string();
    }

    // Build the text element.
    let text_style = state.text_style();
    let lines: Vec<String> = text
        .split('\\')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let width = state.text_width(&text);
    let height = state.text_height(&lines);
    let half_w = width / 2.0;
    let half_h = height / 2.0;
    let bbox = Some((
        Vec2::new(center.x - half_w, center.y - half_h),
        Vec2::new(center.x + half_w, center.y + half_h),
    ));
    scene.elements.push(SceneElement::Text(SceneText {
        source_id: format!("node:{index}"),
        center,
        lines,
        style: text_style,
        bbox,
        width,
        height,
    }));
}

/// Compile one `\coordinate (name) at (x,y);` statement: it defines a named
/// coordinate and produces no drawn element.
#[allow(clippy::too_many_arguments)]
fn compile_coordinate_statement(
    source: &str,
    figure_state: &StyleState,
    named: &mut NamedCoordinates,
    _index: usize,
    from: usize,
    to: usize,
    issues: &mut Vec<Issue>,
) {
    let mut cursor = skip_trivia(source, from, to);
    let mut state = figure_state.clone();

    // Options `[…]` (rare but legal: `\coordinate[...] (name) at ...`).
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

    let mut center = Vec2::new(0.0, 0.0);
    let mut center_set = false;
    let mut defined_name: Option<String> = None;
    while cursor < to {
        cursor = skip_trivia(source, cursor, to);
        let byte = source.as_bytes()[cursor];
        if byte == b'(' {
            let Some(close) = matching(source, cursor, to, b'(', b')') else {
                issues.push(Issue::error(
                    Span::new(cursor, to),
                    "unterminated coordinate",
                ));
                return;
            };
            let inner = &source[cursor + 1..close];
            if pathscan::is_bare_name(inner.trim()) && defined_name.is_none() {
                defined_name = Some(inner.trim().to_string());
            } else if let Ok(point) = pathscan::parse_coordinate(inner, Some(named)) {
                center = point;
                center_set = true;
            }
            cursor = close + 1;
            continue;
        }
        let (word, after) = pathscan::read_word(source, cursor, to);
        if word.is_empty() {
            cursor = (cursor + 1).min(to);
            continue;
        }
        if word == "at" {
            cursor = after;
            cursor = skip_trivia(source, cursor, to);
            if source.as_bytes().get(cursor) == Some(&b'(') {
                let Some(close) = matching(source, cursor, to, b'(', b')') else {
                    issues.push(Issue::error(
                        Span::new(cursor, to),
                        "unterminated coordinate",
                    ));
                    return;
                };
                let inner = &source[cursor + 1..close];
                if let Ok(point) = pathscan::parse_coordinate(inner, Some(named)) {
                    center = point;
                    center_set = true;
                }
                cursor = close + 1;
            }
            continue;
        }
        break;
    }

    if let Some(name) = defined_name.take() {
        named.insert(&name, center);
    } else if center_set {
        issues.push(Issue::warning(
            Span::new(from, to),
            "`\\coordinate` without a name does nothing",
        ));
    }
}

/// Convert a parser [`ParsedDiagnostic`] into a semantic [`Issue`].
fn convert_parsed_diagnostic(d: crate::parser::ParsedDiagnostic) -> Issue {
    match d.severity.as_str() {
        "error" => Issue::error(d.span, d.message),
        _ => Issue::warning(d.span, d.message),
    }
}

/// `\fill` with no colour paints the current colour; the port's base stroke
/// doubles as that "current colour" default.
fn fill_from_stroke(stroke: &Paint) -> Paint {
    match stroke {
        Paint::None => Paint::Black,
        other => *other,
    }
}

/// Skip ASCII whitespace and `%` comments (single-pass, used by the
/// statement scanners).
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
        eprintln!("DEBUG ellipse test");
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
            "\\begin{tikzpicture}\\unknowncmd at (0,0);\\draw (0,0) -- (1,1);\\end{tikzpicture}",
        );
        assert_eq!(scene.len(), 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }

    #[test]
    fn node_statement_produces_text_element() {
        let (scene, issues) =
            scene_of("\\begin{tikzpicture}\\node[draw] at (1,2) {Hello};\\end{tikzpicture}");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(scene.len(), 1);
        match &scene.elements[0] {
            SceneElement::Text(text) => {
                assert_eq!(text.source_id, "node:0");
                assert_eq!(text.lines, vec!["Hello"]);
                assert!((text.center.x - 28.4527559055).abs() < 1e-9);
                assert!((text.center.y - 2.0 * crate::units::CM_TO_PT).abs() < 1e-9);
            }
            other => panic!("expected a text element, got {other:?}"),
        }
    }

    #[test]
    fn node_text_splits_on_backslash() {
        let (scene, issues) =
            scene_of("\\begin{tikzpicture}\\node {first line\\\\second line};\\end{tikzpicture}");
        assert!(issues.is_empty(), "{issues:?}");
        match &scene.elements[0] {
            SceneElement::Text(text) => {
                assert_eq!(text.lines, vec!["first line", "second line"]);
            }
            other => panic!("expected a text element, got {other:?}"),
        }
    }

    #[test]
    fn named_coordinate_node_renders_at_origin() {
        let (scene, issues) =
            scene_of("\\begin{tikzpicture}\\node (A) at (0,0) {A};\\end{tikzpicture}");
        assert!(issues.is_empty(), "{issues:?}");
        match &scene.elements[0] {
            SceneElement::Text(text) => {
                assert_eq!(text.lines, vec!["A"]);
                assert!((text.center.x).abs() < 1e-9);
                assert!((text.center.y).abs() < 1e-9);
            }
            other => panic!("expected a text element, got {other:?}"),
        }
    }

    #[test]
    fn defined_named_coordinate_resolves_on_paths() {
        let cm = crate::units::CM_TO_PT;
        let (scene, issues) = scene_of(
            "\\begin{tikzpicture}\\coordinate (A) at (1,2);\\draw[->] (0,0) -- (A);\\end{tikzpicture}",
        );
        assert!(issues.is_empty(), "{issues:?}");
        let path = only_path(&scene);
        assert_eq!(
            path.cmds,
            vec![
                PathCmd::Move(crate::units::Vec2::new(0.0, 0.0)),
                PathCmd::Line(crate::units::Vec2::new(1.0 * cm, 2.0 * cm)),
            ]
        );
    }

    #[test]
    fn node_defined_name_resolves_later() {
        let cm = crate::units::CM_TO_PT;
        let (scene, issues) = scene_of(
            "\\begin{tikzpicture}\\node (B) at (3,1) {B};\\draw[dashed] (0,0) -- (B);\\end{tikzpicture}",
        );
        assert!(issues.is_empty(), "{issues:?}");
        let path = scene.paths().next().expect("expected a path element");
        assert_eq!(
            path.cmds,
            vec![
                PathCmd::Move(crate::units::Vec2::new(0.0, 0.0)),
                PathCmd::Line(crate::units::Vec2::new(3.0 * cm, 1.0 * cm)),
            ]
        );
        assert_eq!(path.style.dash, Some(vec![3.0, 3.0]));
    }

    #[test]
    fn unknown_named_coordinate_is_an_error() {
        let issues = compile_scene("\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}")
            .expect_err("undefined named coordinates must fail");
        assert!(issues.iter().any(|issue| issue.severity == Severity::Error));
    }

    #[test]
    fn multiple_figures_render_into_one_scene() {
        let source =
        "\\begin{tikzpicture}\\draw (0,0)--(1,1);\\end{tikzpicture}\\begin{tikzpicture}\\draw (0,0)--(1,1);\\end{tikzpicture}";
        let (scene, issues) = compile_scene(source).expect("multi-figure documents must compile");
        // One path element per figure, in document order.
        assert_eq!(scene.len(), 2);
        assert!(issues.is_empty(), "{issues:?}");
        // Source ids stay unique across figures via the global statement counter.
        let mut paths = scene.paths();
        assert_eq!(paths.next().unwrap().source_id, "path:0");
        assert_eq!(paths.next().unwrap().source_id, "path:1");
    }

    #[test]
    fn figure_options_do_not_leak_between_figures() {
        let source =
        "\\begin{tikzpicture}[red]\\draw (0,0)--(1,0);\\end{tikzpicture}\\begin{tikzpicture}\\draw (0,0)--(1,0);\\end{tikzpicture}";
        let (scene, issues) = compile_scene(source).expect("multi-figure documents must compile");
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(scene.len(), 2);
        let mut paths = scene.paths();
        let first = paths.next().unwrap();
        let second = paths.next().unwrap();
        assert_eq!(
            first.style.stroke,
            crate::scene::Paint::Rgb(0xff, 0x00, 0x00)
        );
        assert_eq!(second.style.stroke, crate::scene::Paint::Black);
    }
}
