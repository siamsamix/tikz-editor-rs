//! Evaluated scene: resolved geometry plus paint state, ready for SVG
//! emission, canvas painting or hit-testing.
//!
//! The legacy pipeline evaluates the AST into a scene graph of *elements*
//! (`path` / `ellipse` / text wrappers) before the SVG backend runs. This
//! module mirrors that: [`SceneElement`] keeps the element kind so the
//! emitter can reproduce legacy markup (a `circle` op becomes a `<path>` of
//! two arcs, an `ellipse` op becomes an `<ellipse>` element).
//!
//! Coordinates are TikZ canvas points with y up (see
//! [`crate::units::Vec2`]); the emitter owns the y-flip.

use crate::units::Vec2;

/// Paint applied to a stroke or fill.
///
/// The variant matters for parity: an unstyled path keeps the literal
/// keyword `black`, while a color that came from an option is emitted as a
/// resolved `#rrggbb` value (blueprint row 23 / golden `grid.svg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// Emitted as `none`.
    None,
    /// Emitted as the literal keyword `black` (the default stroke).
    Black,
    /// Emitted as `#rrggbb`.
    Rgb(u8, u8, u8),
}

impl Paint {
    /// The attribute value: `none`, `black` or `#rrggbb`.
    ///
    /// ```
    /// use tikz_core::scene::Paint;
    /// assert_eq!(Paint::None.as_attr(), "none");
    /// assert_eq!(Paint::Black.as_attr(), "black");
    /// assert_eq!(Paint::Rgb(0, 153, 0).as_attr(), "#009900");
    /// ```
    pub fn as_attr(self) -> String {
        match self {
            Paint::None => "none".to_string(),
            Paint::Black => "black".to_string(),
            Paint::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Paint state of one scene element.
#[derive(Debug, Clone, PartialEq)]
pub struct PathStyle {
    /// Stroke paint.
    pub stroke: Paint,
    /// Stroke width in pt.
    pub stroke_width: f64,
    /// `stroke-opacity`.
    pub stroke_opacity: f64,
    /// Fill paint.
    pub fill: Paint,
    /// `fill-opacity`.
    pub fill_opacity: f64,
    /// Emit `fill-rule="evenodd"` (legacy `even odd rule`).
    pub even_odd: bool,
    /// Dash pattern in pt, `None` for solid.
    pub dash: Option<Vec<f64>>,
    /// `stroke-dashoffset`; emitted only when `|offset| > 1e-6`.
    pub dash_offset: f64,
}

impl Default for PathStyle {
    fn default() -> Self {
        Self {
            stroke: Paint::Black,
            stroke_width: crate::units::DEFAULT_LINE_WIDTH_PT,
            stroke_opacity: 1.0,
            fill: Paint::None,
            fill_opacity: 1.0,
            even_odd: false,
            dash: None,
            dash_offset: 0.0,
        }
    }
}

impl PathStyle {
    /// Largest stroke half-width that expands the drawing bounds.
    pub fn half_stroke(&self) -> f64 {
        if self.stroke == Paint::None {
            0.0
        } else {
            self.stroke_width / 2.0
        }
    }
}

/// One path command in canvas space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    /// `M x y`
    Move(Vec2),
    /// `L x y`
    Line(Vec2),
    /// `C c1x c1y c2x c2y x y`
    Cubic {
        /// First control point.
        c1: Vec2,
        /// Second control point.
        c2: Vec2,
        /// End point.
        to: Vec2,
    },
    /// `A rx ry x-axis-rotation large-arc-flag sweep-flag x y`
    Arc {
        /// X radius.
        rx: f64,
        /// Y radius.
        ry: f64,
        /// X-axis rotation in degrees.
        rotation: f64,
        /// Large-arc flag.
        large: bool,
        /// Sweep flag.
        sweep: bool,
        /// End point.
        to: Vec2,
    },
    /// `Z`
    Close,
}

impl PathCmd {
    /// End point of the command; `None` for [`PathCmd::Close`].
    pub fn end_point(self) -> Option<Vec2> {
        match self {
            PathCmd::Move(p) | PathCmd::Line(p) => Some(p),
            PathCmd::Cubic { to, .. } | PathCmd::Arc { to, .. } => Some(to),
            PathCmd::Close => None,
        }
    }
}

/// A stroked/filled path.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenePath {
    /// Identity hook, e.g. `path:0` (`data-source-id` in the SVG).
    pub source_id: String,
    /// Commands in drawing order.
    pub cmds: Vec<PathCmd>,
    /// Paint state.
    pub style: PathStyle,
    /// Legacy-style conservative bounds of the path.
    ///
    /// The legacy evaluator tracks a bounding box per path instead of
    /// re-measuring the emit commands, and an `arc` contributes its whole
    /// ellipse box rather than the swept slice. `None` means "derive from
    /// the command points", which is exact for polylines.
    pub bbox: Option<(Vec2, Vec2)>,
}

/// A standalone `<ellipse>` element (legacy emits the `ellipse` op this way).
#[derive(Debug, Clone, PartialEq)]
pub struct SceneEllipse {
    /// Identity hook.
    pub source_id: String,
    /// Center in canvas space.
    pub center: Vec2,
    /// X radius in pt.
    pub rx: f64,
    /// Y radius in pt.
    pub ry: f64,
    /// Paint state.
    pub style: PathStyle,
}

impl SceneEllipse {
    /// Axis-aligned bounds of the ellipse outline.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        (
            Vec2::new(self.center.x - self.rx, self.center.y - self.ry),
            Vec2::new(self.center.x + self.rx, self.center.y + self.ry),
        )
    }
}

/// One drawable scene element in document order.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneElement {
    /// A path (line/cubic/arc chain).
    Path(ScenePath),
    /// A standalone ellipse element.
    Ellipse(SceneEllipse),
}

impl SceneElement {
    /// Identity hook shared by both element kinds.
    pub fn source_id(&self) -> &str {
        match self {
            SceneElement::Path(path) => &path.source_id,
            SceneElement::Ellipse(ellipse) => &ellipse.source_id,
        }
    }

    /// Paint state shared by both element kinds.
    pub fn style(&self) -> &PathStyle {
        match self {
            SceneElement::Path(path) => &path.style,
            SceneElement::Ellipse(ellipse) => &ellipse.style,
        }
    }
}

/// Everything one figure draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// Elements in document order.
    pub elements: Vec<SceneElement>,
}

impl Scene {
    /// Iterate the path elements (ellipses are not paths).
    pub fn paths(&self) -> impl Iterator<Item = &ScenePath> {
        self.elements.iter().filter_map(|element| match element {
            SceneElement::Path(path) => Some(path),
            SceneElement::Ellipse(_) => None,
        })
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Whether the scene draws nothing.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Axis-aligned bounds `(min, max)` of the drawn geometry, ignoring
    /// stroke width. Curve control points are included, matching the
    /// conservative legacy bbox.
    ///
    /// ```
    /// use tikz_core::scene::{PathCmd, PathStyle, Scene, SceneElement, ScenePath};
    /// use tikz_core::units::Vec2;
    /// let scene = Scene { elements: vec![SceneElement::Path(ScenePath {
    ///     source_id: "path:0".into(),
    ///     cmds: vec![PathCmd::Move(Vec2::new(1.0, 2.0)), PathCmd::Line(Vec2::new(3.0, -1.0))],
    ///     style: PathStyle::default(),
    ///     bbox: None,
    /// })] };
    /// assert_eq!(scene.bounds(), Some((Vec2::new(1.0, -1.0), Vec2::new(3.0, 2.0))));
    /// assert_eq!(Scene::default().bounds(), None);
    /// ```
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        let mut acc: Option<(Vec2, Vec2)> = None;
        let mut add = |p: Vec2| match acc {
            None => acc = Some((p, p)),
            Some((lo, hi)) => {
                acc = Some((
                    Vec2::new(lo.x.min(p.x), lo.y.min(p.y)),
                    Vec2::new(hi.x.max(p.x), hi.y.max(p.y)),
                ))
            }
        };
        for element in &self.elements {
            match element {
                SceneElement::Path(path) => {
                    if let Some((lo, hi)) = path.bbox {
                        add(lo);
                        add(hi);
                        continue;
                    }
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
                }
                SceneElement::Ellipse(ellipse) => {
                    let (lo, hi) = ellipse.bounds();
                    add(lo);
                    add(hi);
                }
            }
        }
        acc
    }

    /// Largest stroke half-width over every element.
    pub fn max_half_stroke(&self) -> f64 {
        self.elements.iter().map(|e| e.style().half_stroke()).fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(cmds: Vec<PathCmd>, style: PathStyle) -> SceneElement {
        SceneElement::Path(ScenePath { source_id: "path:0".into(), cmds, style, bbox: None })
    }

    #[test]
    fn paint_attr_matches_legacy_forms() {
        assert_eq!(Paint::Rgb(191, 0, 64).as_attr(), "#bf0040");
        assert_eq!(Paint::Black.as_attr(), "black");
    }

    #[test]
    fn bounds_include_curve_controls() {
        let scene = Scene {
            elements: vec![path(
                vec![
                    PathCmd::Move(Vec2::new(0.0, 0.0)),
                    PathCmd::Cubic {
                        c1: Vec2::new(1.0, 5.0),
                        c2: Vec2::new(2.0, -5.0),
                        to: Vec2::new(3.0, 0.0),
                    },
                ],
                PathStyle::default(),
            )],
        };
        assert_eq!(scene.bounds(), Some((Vec2::new(0.0, -5.0), Vec2::new(3.0, 5.0))));
    }

    #[test]
    fn ellipse_bounds_and_half_stroke() {
        let scene = Scene {
            elements: vec![SceneElement::Ellipse(SceneEllipse {
                source_id: "path:0".into(),
                center: Vec2::new(2.0, 3.0),
                rx: 1.0,
                ry: 0.5,
                style: PathStyle { stroke_width: 2.0, ..PathStyle::default() },
            })],
        };
        assert_eq!(scene.bounds(), Some((Vec2::new(1.0, 2.5), Vec2::new(3.0, 3.5))));
        assert_eq!(scene.max_half_stroke(), 1.0);
        assert_eq!(scene.paths().count(), 0);
    }

    #[test]
    fn half_stroke_ignores_none_stroke() {
        let invisible = PathStyle { stroke: Paint::None, stroke_width: 4.0, ..PathStyle::default() };
        assert_eq!(invisible.half_stroke(), 0.0);
        assert_eq!(PathStyle::default().half_stroke(), 0.2);
    }
}