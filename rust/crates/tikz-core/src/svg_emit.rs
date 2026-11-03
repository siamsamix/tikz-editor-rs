//! SVG backend: [`Scene`] -> SVG 1.1 string.
//!
//! Port of the path/viewBox portion of `packages/core/src/svg/emit.ts` and
//! `svg/viewbox.ts`. Conventions (all verified against the committed
//! goldens):
//!
//! * viewBox = element bounds expanded by that element's stroke half-width,
//!   then padded by 12pt on every side; an empty scene falls back to
//!   `-12 -12 124 124`;
//! * the y-flip is `svg_y = 2*vb.y + vb.height - y`;
//! * attribute order is `stroke, fill, fill-rule, stroke-width, linecap,
//!   linejoin, opacities, dasharray, dashoffset` — one function emits them,
//!   so no path can drift (blueprint row 24);
//! * a `circle` op is written as `<path>` with two arcs, an `ellipse` op as
//!   an `<ellipse>` element (verified in `grid.svg`).

use crate::scene::{
    FontFamily, LineCap, LineJoin, PathCmd, PathStyle, Scene, SceneElement, SceneEllipse,
    ScenePath, SceneText, TextAlign,
};
use crate::svg_numbers::fmt_num;

/// Padding added around the drawing on each side, in pt.
const VIEWBOX_PAD: f64 = 12.0;

/// ViewBox in canvas units.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl ViewBox {
    fn of(scene: &Scene) -> ViewBox {
        let mut acc: Option<(f64, f64, f64, f64)> = None;
        for element in &scene.elements {
            let (lo, hi) = element_bounds(element);
            let half = match element {
                SceneElement::Path(path) => path.style.half_stroke(),
                SceneElement::Ellipse(ellipse) => ellipse.style.half_stroke(),
                SceneElement::Text(_) => 0.0,
            };
            let (x0, y0) = (lo.x - half, lo.y - half);
            let (x1, y1) = (hi.x + half, hi.y + half);
            acc = Some(match acc {
                None => (x0, y0, x1, y1),
                Some((ax0, ay0, ax1, ay1)) => (ax0.min(x0), ay0.min(y0), ax1.max(x1), ay1.max(y1)),
            });
        }
        let Some((x0, y0, x1, y1)) = acc else {
            return ViewBox {
                x: -12.0,
                y: -12.0,
                width: 124.0,
                height: 124.0,
            };
        };
        ViewBox {
            x: x0 - VIEWBOX_PAD,
            y: y0 - VIEWBOX_PAD,
            width: (x1 - x0) + 2.0 * VIEWBOX_PAD,
            height: (y1 - y0) + 2.0 * VIEWBOX_PAD,
        }
    }

    /// Constant of the y-flip: `svg_y = flip_k - y`.
    fn flip_k(&self) -> f64 {
        2.0 * self.y + self.height
    }
}

fn element_bounds(element: &SceneElement) -> (crate::units::Vec2, crate::units::Vec2) {
    match element {
        SceneElement::Path(path) => path_bbox(path),
        SceneElement::Ellipse(ellipse) => ellipse.bounds(),
        SceneElement::Text(text) => {
            if let Some((lo, hi)) = text.bbox {
                return (lo, hi);
            }
            let half_w = text.width / 2.0;
            let half_h = text.height / 2.0;
            (
                crate::units::Vec2::new(text.center.x - half_w, text.center.y - half_h),
                crate::units::Vec2::new(text.center.x + half_w, text.center.y + half_h),
            )
        }
    }
}

fn path_bbox(path: &ScenePath) -> (crate::units::Vec2, crate::units::Vec2) {
    if let Some(bounds) = path.bbox {
        return bounds;
    }
    let mut acc: Option<(crate::units::Vec2, crate::units::Vec2)> = None;
    let mut add = |p: crate::units::Vec2| match acc {
        None => acc = Some((p, p)),
        Some((lo, hi)) => {
            acc = Some((
                crate::units::Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
                crate::units::Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
            ))
        }
    };
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::Cubic { c1, c2, to, .. } => {
                add(c1);
                add(c2);
                add(to);
            }
            other => {
                if let Some(point) = other.end_point() {
                    add(point);
                }
            }
        }
    }
    acc.unwrap_or((
        crate::units::Vec2::new(0.0, 0.0),
        crate::units::Vec2::new(0.0, 0.0),
    ))
}

/// Emit the SVG document for `scene`.
///
/// ```
/// use tikz_core::scene::Scene;
/// use tikz_core::svg_emit::emit_svg;
/// assert!(emit_svg(&Scene::default()).contains(r#"viewBox="-12 -12 124 124""#));
/// ```
pub fn emit_svg(scene: &Scene) -> String {
    let vb = ViewBox::of(scene);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}" role="img" aria-label="TikZ SVG preview">"#,
        fmt_num(vb.x),
        fmt_num(vb.y),
        fmt_num(vb.width),
        fmt_num(vb.height)
    );
    // Collect all arrow tips used across paths, emit them as <marker> defs.
    let mut arrow_defs = String::new();
    let mut arrow_ids = std::collections::HashSet::new();
    for element in &scene.elements {
        if let SceneElement::Path(path) = element {
            for arrow in path
                .style
                .arrow_start
                .iter()
                .chain(path.style.arrow_end.iter())
            {
                let id = format!("arrow-{}", arrow.kind);
                if arrow_ids.insert(id.clone()) {
                    arrow_defs.push_str(&emit_arrow_marker(arrow, &id));
                }
            }
        }
    }
    if !arrow_defs.is_empty() {
        out.push_str(&format!("<defs>{arrow_defs}</defs>"));
    }
    for element in &scene.elements {
        match element {
            SceneElement::Path(path) => out.push_str(&emit_path(path, vb.flip_k())),
            SceneElement::Ellipse(ellipse) => out.push_str(&emit_ellipse(ellipse, vb.flip_k())),
            SceneElement::Text(text) => out.push_str(&emit_text(text, vb.flip_k())),
        }
    }
    out.push_str("</svg>");
    out
}

/// Emit an `<marker>` element for an arrow tip.
fn emit_arrow_marker(arrow: &crate::scene::ArrowTip, id: &str) -> String {
    let length = fmt_num(arrow.length);
    let width = fmt_num(arrow.width);
    let half_w = fmt_num(arrow.width / 2.0);
    // Arrow marker: a filled triangle pointing right, tip at (length, width/2),
    // base at (0, 0) and (0, width).
    format!(
        r#"<marker id="{id}" markerUnits="strokeWidth" markerWidth="{length}" markerHeight="{width}" refX="{length}" refY="{half_w}" orient="auto"><path d="M 0 0 L {length} {half_w} L 0 {width} Z" fill="currentColor" /></marker>"#,
    )
}

fn emit_path(path: &ScenePath, flip_k: f64) -> String {
    let mut d = String::new();
    for cmd in &path.cmds {
        if !d.is_empty() {
            d.push(' ');
        }
        match *cmd {
            PathCmd::Move(p) => {
                d.push_str(&format!("M {} {}", fmt_num(p.x), fmt_num(flip_k - p.y)));
            }
            PathCmd::Line(p) => {
                d.push_str(&format!("L {} {}", fmt_num(p.x), fmt_num(flip_k - p.y)));
            }
            PathCmd::Cubic { c1, c2, to } => {
                d.push_str(&format!(
                    "C {} {} {} {} {} {}",
                    fmt_num(c1.x),
                    fmt_num(flip_k - c1.y),
                    fmt_num(c2.x),
                    fmt_num(flip_k - c2.y),
                    fmt_num(to.x),
                    fmt_num(flip_k - to.y)
                ));
            }
            PathCmd::Arc {
                rx,
                ry,
                rotation,
                large,
                sweep,
                to,
            } => {
                // The y-flip reverses arc direction, so the sweep flag is
                // inverted on output (legacy `svg/emit.ts:920`).
                d.push_str(&format!(
                    "A {} {} {} {} {} {} {}",
                    fmt_num(rx),
                    fmt_num(ry),
                    fmt_num(rotation),
                    u8::from(large),
                    u8::from(!sweep),
                    fmt_num(to.x),
                    fmt_num(flip_k - to.y)
                ));
            }
            PathCmd::Close => d.push('Z'),
        }
    }
    let style_str = style_attrs(&path.style);
    let result = format!(
        r#"<path data-source-id="{}" d="{d}" {} {} {} />"#,
        escape_attr(&path.source_id),
        style_str,
        path.style
            .arrow_start
            .as_ref()
            .map(|a| format!("marker-start=\"url(#arrow-{})\"", a.kind))
            .unwrap_or_default(),
        path.style
            .arrow_end
            .as_ref()
            .map(|a| format!("marker-end=\"url(#arrow-{})\"", a.kind))
            .unwrap_or_default(),
    );
    result
}

fn emit_ellipse(ellipse: &SceneEllipse, flip_k: f64) -> String {
    format!(
        r#"<ellipse data-source-id="{}" cx="{}" cy="{}" rx="{}" ry="{}" {} />"#,
        escape_attr(&ellipse.source_id),
        fmt_num(ellipse.center.x),
        fmt_num(flip_k - ellipse.center.y),
        fmt_num(ellipse.rx),
        fmt_num(ellipse.ry),
        style_attrs(&ellipse.style)
    )
}

/// Emit a text element (`<text>` with `<tspan>` lines).
///
/// Matches the legacy emitter shape (`packages/core/src/svg/emit.ts`):
/// `x`/`y` are the anchor point (y up → SVG y-flip), `text-anchor` selects
/// horizontal alignment, `dominant-baseline="middle"` vertically centers on
/// `y`, and each line is a `<tspan>` with `1.15em` steps.
fn emit_text(text: &SceneText, flip_k: f64) -> String {
    let anchor = text.center.x;
    let text_anchor = match text.style.text_align {
        TextAlign::Left => "start",
        TextAlign::Center => "middle",
        TextAlign::Right => "end",
    };
    let font_family = match text.style.font_family {
        FontFamily::Serif => "Latin Modern Roman, CMU Serif, Times New Roman, serif",
        FontFamily::Sans => "Latin Modern Sans, CMU Sans Serif, Helvetica, Arial, sans-serif",
        FontFamily::Mono => "Latin Modern Mono, CMU Typewriter Text, Courier New, monospace",
    };
    let mut attrs = Vec::new();
    attrs.push(format!(
        r#"data-source-id="{}" x="{}" y="{}" font-family="{}" font-size="{:.4}" fill="{}" fill-opacity="{:.4}" text-anchor="{}" dominant-baseline="middle" xml:space="preserve""#,
        escape_attr(&text.source_id),
        fmt_num(anchor),
        fmt_num(flip_k - text.center.y),
        font_family,
        text.style.font_size,
        text.style.fill.as_attr(),
        text.style.fill_opacity,
        text_anchor
    ));
    if text.style.font_weight {
        attrs.push(r#"font-weight="700""#.to_string());
    }
    if text.style.font_style {
        attrs.push(r#"font-style="italic""#.to_string());
    }
    let mut lines = String::new();
    let n = text.lines.len() as f64;
    if n > 0.0 {
        let first_dy = -((n - 1.0) * 1.15) / 2.0;
        lines.push_str(&format!(
            r#"<tspan x="{}" y="{}" dy="{:.4}em">{}</tspan>"#,
            fmt_num(anchor),
            fmt_num(flip_k - text.center.y),
            first_dy,
            escape_text(&text.lines[0])
        ));
        for line in text.lines.iter().skip(1) {
            lines.push_str(&format!(
                r#"<tspan x="{}" dy="1.15em">{}</tspan>"#,
                fmt_num(anchor),
                escape_text(line)
            ));
        }
    }
    format!(r#"<text {}>{}</text>"#, attrs.join(" "), lines)
}

/// Escape text content: `& < >`.
fn escape_text(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Paint attributes in the one legacy order (blueprint row 24).
fn style_attrs(style: &PathStyle) -> String {
    let mut out = format!(
        r#"stroke="{}" fill="{}""#,
        style.stroke.as_attr(),
        style.fill.as_attr()
    );
    if style.even_odd {
        out.push_str(r#" fill-rule="evenodd""#);
    }
    out.push_str(" stroke-width=\"");
    out.push_str(&fmt_num(style.stroke_width));
    out.push_str("\" stroke-linecap=\"");
    out.push_str(match style.line_cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    });
    out.push_str("\" stroke-linejoin=\"");
    out.push_str(match style.line_join {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    });
    out.push_str("\" stroke-opacity=\"");
    out.push_str(&fmt_num(style.stroke_opacity));
    out.push_str("\" fill-opacity=\"");
    out.push_str(&fmt_num(style.fill_opacity));
    out.push('"');
    if let Some(dash) = &style.dash {
        let values: Vec<String> = dash.iter().map(|value| fmt_num(*value)).collect();
        out.push_str(&format!(r#" stroke-dasharray="{}""#, values.join(" ")));
        if style.dash_offset.abs() > 1e-6 {
            out.push_str(&format!(
                r#" stroke-dashoffset="{}""#,
                fmt_num(style.dash_offset)
            ));
        }
    }
    out
}

/// Attribute escaping: `& < >` plus the double quote (blueprint row 23).
fn escape_attr(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Paint;
    use crate::units::Vec2;

    fn styled(style: PathStyle, cmds: Vec<PathCmd>) -> Scene {
        Scene {
            elements: vec![SceneElement::Path(ScenePath {
                source_id: "path:0".into(),
                cmds,
                style,
                bbox: None,
            })],
        }
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> Scene {
        styled(
            PathStyle::default(),
            vec![
                PathCmd::Move(Vec2::new(a.0, a.1)),
                PathCmd::Line(Vec2::new(b.0, b.1)),
            ],
        )
    }

    #[test]
    fn viewbox_adds_pad_and_half_stroke() {
        let svg = emit_svg(&line((0.0, 0.0), (10.0, 20.0)));
        assert!(svg.contains(r#"viewBox="-12.2 -12.2 34.4 44.4""#), "{svg}");
    }

    #[test]
    fn y_flip_maps_bottom_to_top_edge() {
        let svg = emit_svg(&line((0.0, 0.0), (10.0, 20.0)));
        assert!(svg.contains(r#"d="M 0 20 L 10 0""#), "{svg}");
    }

    #[test]
    fn attribute_order_and_dash_follow_legacy() {
        let style = PathStyle {
            stroke: Paint::Rgb(255, 0, 0),
            stroke_width: 2.0,
            dash: Some(vec![3.0, 3.0]),
            ..PathStyle::default()
        };
        let svg = emit_svg(&styled(
            style,
            vec![
                PathCmd::Move(Vec2::new(0.0, 0.0)),
                PathCmd::Line(Vec2::new(1.0, 0.0)),
            ],
        ));
        let expected = r##"stroke="#ff0000" fill="none" stroke-width="2" stroke-linecap="butt" stroke-linejoin="miter" stroke-opacity="1" fill-opacity="1" stroke-dasharray="3 3""##;
        assert!(svg.contains(expected), "{svg}");
    }

    #[test]
    fn fill_rule_precedes_stroke_width() {
        let style = PathStyle {
            fill: Paint::Rgb(255, 255, 0),
            even_odd: true,
            ..PathStyle::default()
        };
        let svg = emit_svg(&styled(
            style,
            vec![PathCmd::Move(Vec2::new(0.0, 0.0)), PathCmd::Close],
        ));
        assert!(
            svg.contains(
                r##"stroke="black" fill="#ffff00" fill-rule="evenodd" stroke-width="0.4""##
            ),
            "{svg}"
        );
    }

    #[test]
    fn ellipse_is_its_own_element_with_flipped_y() {
        let scene = Scene {
            elements: vec![SceneElement::Ellipse(SceneEllipse {
                source_id: "path:7".into(),
                center: Vec2::new(56.9055, 10.0),
                rx: 14.2264,
                ry: 7.1132,
                style: PathStyle {
                    stroke: Paint::Rgb(191, 0, 64),
                    ..PathStyle::default()
                },
            })],
        };
        let svg = emit_svg(&scene);
        assert!(svg.contains(r##"<ellipse data-source-id="path:7" cx="56.9055" cy="10" rx="14.2264" ry="7.1132" stroke="#bf0040""##), "{svg}");
    }

    #[test]
    fn arc_flags_render_as_integers() {
        // Canvas-space sweep is counter-clockwise (y up); the emitter flips
        // the flag because the SVG is y-down (`svg/emit.ts:920`).
        let scene = styled(
            PathStyle::default(),
            vec![
                PathCmd::Move(Vec2::new(0.0, 0.0)),
                PathCmd::Arc {
                    rx: 21.3396,
                    ry: 21.3396,
                    rotation: 0.0,
                    large: false,
                    sweep: true,
                    to: Vec2::new(-42.6792, 0.0),
                },
            ],
        );
        assert!(
            emit_svg(&scene).contains("A 21.3396 21.3396 0 0 0 -42.6792 0"),
            "{}",
            emit_svg(&scene)
        );
    }

    #[test]
    fn attribute_escaping_covers_quote() {
        assert_eq!(escape_attr(r#"a"b<&>"#), "a&quot;b&lt;&amp;&gt;");
    }
}
