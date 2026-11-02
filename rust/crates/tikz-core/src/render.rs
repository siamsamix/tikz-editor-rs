//! Source -> scene -> SVG facade.
//!
//! The Rust counterpart of `render/index.ts`, collapsed to one pure
//! synchronous entry point over [`crate::semantic`]. Errors stop the
//! document; warnings are reported while the rest of the figure still
//! renders, so unsupported TikZ is visible in the UI instead of silently
//! missing from the picture.
//!
//! ```
//! use tikz_core::render::render_tex;
//! let svg = render_tex("\\begin{tikzpicture}\\draw (0,0) -- (1,1);\\end{tikzpicture}").unwrap();
//! assert!(svg.starts_with("<svg "));
//! assert!(svg.contains(r#"d="M 0 28.4528 L 28.4528 0""#));
//! ```

use crate::scene::Scene;
use crate::semantic::style::Issue;
use crate::semantic::{compile_scene, expr};
use crate::svg_emit::emit_svg;

pub use crate::semantic::style::Issue as Diagnostic;

/// Result of a successful render: the SVG plus non-blocking issues.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    /// SVG 1.1 document.
    pub svg: String,
    /// Warnings collected while evaluating (never errors).
    pub warnings: Vec<Issue>,
}

/// Render `source` to SVG, failing on any error-severity issue.
pub fn render_tex(source: &str) -> Result<String, Vec<Diagnostic>> {
    compile_scene(source).map(|(scene, _)| emit_svg(&scene))
}

/// Render `source`, returning the SVG and any non-blocking warnings.
pub fn render_tex_with_warnings(source: &str) -> Result<Rendered, Vec<Diagnostic>> {
    compile_scene(source).map(|(scene, issues)| Rendered { svg: emit_svg(&scene), warnings: issues })
}

/// Evaluate `source` into a [`Scene`], failing on any error-severity issue.
pub fn compile(source: &str) -> Result<Scene, Vec<Diagnostic>> {
    compile_scene(source).map(|(scene, _)| scene)
}

/// Evaluate `source`, returning the scene and any non-blocking warnings.
pub fn compile_with_warnings(source: &str) -> Result<(Scene, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_scene(source)
}

/// Parse `1.5`, `2cm`, `3pt`, `4mm`, `1in`, `1bp` into pt.
///
/// ```
/// use tikz_core::render::dimension_pt;
/// assert_eq!(dimension_pt("2pt"), Ok(2.0));
/// assert!(dimension_pt("1em").is_err());
/// ```
pub fn dimension_pt(raw: &str) -> Result<f64, String> {
    expr::eval_len(raw, expr::Unit::Cm)
}

#[cfg(test)]
mod tests {
    use crate::scene::PathCmd;

    use super::*;

    #[test]
    fn warnings_do_not_block_rendering() {
        let rendered = render_tex_with_warnings(
            "\\begin{tikzpicture}\\draw (0,0) -- (1,1) node {A};\\end{tikzpicture}",
        )
        .expect("warnings must not fail the render");
        assert_eq!(rendered.warnings.len(), 1);
        assert!(rendered.svg.contains("<path "));
    }

    #[test]
    fn errors_report_the_span() {
        let issues = render_tex("\\begin{tikzpicture}\\draw (A) -- (B);\\end{tikzpicture}")
            .expect_err("named coordinates are unsupported");
        assert!(issues.iter().any(Issue::is_error));
        let (line, _col) = issues[0].line_col("x");
        assert_eq!(line, 1);
    }

    #[test]
    fn token_soup_never_panics() {
        let sources = [
            "\\begin{tikzpicture}",
            "\\begin{tikzpicture}\\draw;\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (;\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (1,1) -- (;\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw[\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (0,0) circle [radius=;\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (0,0) grid (1e9,1e9);\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (0,0) grid (0,0);\\end{tikzpicture}",
            "\\begin{tikzpicture}\\draw (0,0) -- (1,1) %comment",
        ];
        for source in sources {
            let _ = render_tex(source);
        }
    }

    #[test]
    fn integer_polylines_render() {
        let svg = render_tex("\\begin{tikzpicture}\\draw (0,0) -- (1,0) -- (1,1);\\end{tikzpicture}")
            .unwrap();
        assert!(svg.contains(r#"d="M 0 28.4528 L 28.4528 28.4528 L 28.4528 0""#), "{svg}");
    }

    #[test]
    fn scene_commands_carry_arcs_and_closes() {
        let scene = compile("\\begin{tikzpicture}\\draw (0,0) -- (1,0) -- cycle;\\end{tikzpicture}")
            .unwrap();
        let path = scene.paths().next().unwrap();
        assert!(matches!(path.cmds.last(), Some(PathCmd::Close)));
    }
}