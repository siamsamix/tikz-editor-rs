//! Geometric picking: distance-to-stroke hit testing for the egui shell.
//!
//! The legacy editor relies on the browser's SVG `pointer-events` machinery
//! (blueprint row 6); the Rust port replaces it with an explicit geometric
//! test so the canvas does not need an SVG DOM. Curves and arcs are sampled
//! into short segments, which is accurate enough for a pick radius of a few
//! points and keeps the code allocation-free per query.
//!
//! ```
//! use tikz_core::hit_test::hit_test;
//! use tikz_core::scene::{PathCmd, PathStyle, Scene, SceneElement, ScenePath};
//! use tikz_core::units::Vec2;
//!
//! let scene = Scene { elements: vec![SceneElement::Path(ScenePath {
//!     source_id: "path:0".into(),
//!     cmds: vec![PathCmd::Move(Vec2::new(0.0, 0.0)), PathCmd::Line(Vec2::new(10.0, 0.0))],
//!     style: PathStyle::default(),
//!     bbox: None,
//! })] };
//! assert_eq!(hit_test(&scene, Vec2::new(5.0, 0.2), 0.0), Some(0));
//! assert_eq!(hit_test(&scene, Vec2::new(5.0, 2.0), 0.0), None);
//! ```

use crate::scene::{Paint, PathCmd, Scene, SceneElement, SceneEllipse, ScenePath};
use crate::units::Vec2;

/// Samples per cubic segment when flattening for picking.
const CUBIC_SAMPLES: usize = 12;
/// Samples per arc command when flattening for picking.
const ARC_SAMPLES: usize = 24;
/// Samples used to approximate a standalone ellipse outline.
const ELLIPSE_SAMPLES: usize = 72;

/// Euclidean distance from `p` to the segment `a..b`.
///
/// ```
/// use tikz_core::hit_test::distance_to_segment;
/// use tikz_core::units::Vec2;
/// let a = Vec2::new(0.0, 0.0);
/// let b = Vec2::new(10.0, 0.0);
/// assert_eq!(distance_to_segment(Vec2::new(5.0, 3.0), a, b), 3.0);
/// assert_eq!(distance_to_segment(Vec2::new(-2.0, 0.0), a, b), 2.0);
/// ```
pub fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 <= f64::EPSILON {
        return ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt();
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    let q = Vec2::new(a.x + t * dx, a.y + t * dy);
    ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt()
}

/// Distance from `p` to the polyline through `points`.
fn distance_to_polyline(p: Vec2, points: &[Vec2]) -> f64 {
    let mut best = f64::INFINITY;
    if points.len() == 1 {
        return ((p.x - points[0].x).powi(2) + (p.y - points[0].y).powi(2)).sqrt();
    }
    for pair in points.windows(2) {
        best = best.min(distance_to_segment(p, pair[0], pair[1]));
    }
    best
}

/// Flatten a command list into polylines (one per subpath).
///
/// ```
/// use tikz_core::hit_test::flatten;
/// use tikz_core::scene::PathCmd;
/// use tikz_core::units::Vec2;
/// let subpaths = flatten(&[PathCmd::Move(Vec2::new(0.0, 0.0)), PathCmd::Line(Vec2::new(1.0, 0.0))]);
/// assert_eq!(subpaths, vec![vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)]]);
/// ```
pub fn flatten(cmds: &[PathCmd]) -> Vec<Vec<Vec2>> {
    let mut subpaths: Vec<Vec<Vec2>> = Vec::new();
    let mut cursor: Option<Vec2> = None;
    for cmd in cmds {
        match *cmd {
            PathCmd::Move(p) => {
                subpaths.push(vec![p]);
                cursor = Some(p);
            }
            PathCmd::Line(p) => {
                if let Some(subpath) = subpaths.last_mut() {
                    subpath.push(p);
                } else {
                    subpaths.push(vec![p]);
                }
                cursor = Some(p);
            }
            PathCmd::Cubic { c1, c2, to } => {
                let from = cursor.unwrap_or(c1);
                ensure_subpath(&mut subpaths, from);
                let samples = (1..=CUBIC_SAMPLES)
                    .map(|step| cubic_point(from, c1, c2, to, step as f64 / CUBIC_SAMPLES as f64));
                subpaths.last_mut().expect("subpath exists").extend(samples);
                cursor = Some(to);
            }
            PathCmd::Arc {
                rx,
                ry,
                rotation,
                large,
                sweep,
                to,
            } => {
                let from = cursor.unwrap_or(to);
                let points = arc_points(from, rx, ry, rotation, large, sweep, to);
                ensure_subpath(&mut subpaths, from);
                subpaths
                    .last_mut()
                    .expect("subpath exists")
                    .extend(points.into_iter().skip(1));
                cursor = Some(to);
            }
            PathCmd::Close => {
                if let Some(subpath) = subpaths.last_mut() {
                    if let Some(first) = subpath.first().copied() {
                        subpath.push(first);
                    }
                }
            }
        }
    }
    subpaths
}

/// Make sure the last subpath exists, starting it at `from` when needed.
fn ensure_subpath(subpaths: &mut Vec<Vec<Vec2>>, from: Vec2) {
    if subpaths.is_empty() {
        subpaths.push(vec![from]);
    }
}

fn cubic_point(a: Vec2, b: Vec2, c: Vec2, d: Vec2, t: f64) -> Vec2 {
    let u = 1.0 - t;
    let (u2, t2) = (u * u, t * t);
    Vec2::new(
        u2 * u * a.x + 3.0 * u2 * t * b.x + 3.0 * u * t2 * c.x + t2 * t * d.x,
        u2 * u * a.y + 3.0 * u2 * t * b.y + 3.0 * u * t2 * c.y + t2 * t * d.y,
    )
}

/// Sample an SVG-style arc (canvas coordinates) into points.
///
/// Canvas space has y up while SVG arcs are defined with y down, so the
/// sweep flag is inverted here.
fn arc_points(
    from: Vec2,
    rx: f64,
    ry: f64,
    rotation: f64,
    large: bool,
    sweep: bool,
    to: Vec2,
) -> Vec<Vec2> {
    let sweep = !sweep;
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx <= f64::EPSILON || ry <= f64::EPSILON {
        return vec![from, to];
    }
    let phi = rotation.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx2 = (from.x - to.x) / 2.0;
    let dy2 = (from.y - to.y) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let numerator = (rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p).max(0.0);
    let denominator = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coefficient = if denominator <= f64::EPSILON {
        0.0
    } else {
        let sign = if large != sweep { 1.0 } else { -1.0 };
        sign * (numerator / denominator).sqrt()
    };
    let cxp = coefficient * rx * y1p / ry;
    let cyp = -coefficient * ry * x1p / rx;
    let center = Vec2::new(
        cos_phi * cxp - sin_phi * cyp + (from.x + to.x) / 2.0,
        sin_phi * cxp + cos_phi * cyp + (from.y + to.y) / 2.0,
    );
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = uy.atan2(ux);
    let mut delta = angle_between(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    (0..=ARC_SAMPLES)
        .map(|index| {
            let theta = theta1 + delta * (index as f64 / ARC_SAMPLES as f64);
            let (sin_t, cos_t) = theta.sin_cos();
            Vec2::new(
                center.x + cos_phi * rx * cos_t - sin_phi * ry * sin_t,
                center.y + sin_phi * rx * cos_t + cos_phi * ry * sin_t,
            )
        })
        .collect()
}

fn angle_between(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let dot = ux * vx + uy * vy;
    let cross = ux * vy - uy * vx;
    cross.atan2(dot)
}

/// Sample an ellipse outline into a closed polyline.
///
/// ```
/// use tikz_core::hit_test::ellipse_points;
/// use tikz_core::scene::{PathStyle, SceneEllipse};
/// use tikz_core::units::Vec2;
/// let ellipse = SceneEllipse {
///     source_id: "path:0".into(),
///     center: Vec2::new(0.0, 0.0),
///     rx: 2.0,
///     ry: 1.0,
///     style: PathStyle::default(),
/// };
/// let points = ellipse_points(&ellipse);
/// assert!(points.iter().all(|p| (p.x / 2.0).powi(2) + (p.y / 1.0).powi(2) - 1.0 < 1e-9));
/// ```
pub fn ellipse_points(ellipse: &SceneEllipse) -> Vec<Vec2> {
    (0..=ELLIPSE_SAMPLES)
        .map(|index| {
            let theta = std::f64::consts::TAU * (index as f64 / ELLIPSE_SAMPLES as f64);
            let (sin_t, cos_t) = theta.sin_cos();
            Vec2::new(
                ellipse.center.x + ellipse.rx * cos_t,
                ellipse.center.y + ellipse.ry * sin_t,
            )
        })
        .collect()
}

/// Flatten one scene element into polylines, for picking or painting.
///
/// ```
/// use tikz_core::hit_test::flatten_element;
/// use tikz_core::scene::{PathCmd, PathStyle, SceneElement, ScenePath};
/// use tikz_core::units::Vec2;
/// let element = SceneElement::Path(ScenePath {
///     source_id: "path:0".into(),
///     cmds: vec![PathCmd::Move(Vec2::new(0.0, 0.0)), PathCmd::Line(Vec2::new(1.0, 1.0))],
///     style: PathStyle::default(),
///     bbox: None,
/// });
/// assert_eq!(flatten_element(&element).len(), 1);
/// ```
pub fn flatten_element(element: &SceneElement) -> Vec<Vec<Vec2>> {
    match element {
        SceneElement::Path(path) => flatten(&path.cmds),
        SceneElement::Ellipse(ellipse) => vec![ellipse_points(ellipse)],
        SceneElement::Text(_) => vec![],
    }
}

/// Distance from `p` to an element's stroke center line.
pub fn distance_to_element(p: Vec2, element: &SceneElement) -> f64 {
    flatten_element(element)
        .iter()
        .map(|subpath| distance_to_polyline(p, subpath))
        .fold(f64::INFINITY, f64::min)
}

/// Distance from `p` to a path's stroke center line.
pub fn distance_to_path(p: Vec2, path: &ScenePath) -> f64 {
    flatten(&path.cmds)
        .iter()
        .map(|subpath| distance_to_polyline(p, subpath))
        .fold(f64::INFINITY, f64::min)
}

/// Distance from `p` to an ellipse outline.
pub fn distance_to_ellipse(p: Vec2, ellipse: &SceneEllipse) -> f64 {
    distance_to_polyline(p, &ellipse_points(ellipse))
}

/// Index of the topmost element whose stroke covers `point`, if any.
///
/// `tolerance` is an extra pick radius in the same units as the scene (pt);
/// the stroke half-width is added automatically. Elements are tested in
/// reverse document order so the last drawn shape wins. Elements with no
/// stroke are not pickable.
///
/// ```
/// use tikz_core::hit_test::hit_test;
/// use tikz_core::scene::{PathCmd, PathStyle, Scene, SceneElement, ScenePath};
/// use tikz_core::units::Vec2;
///
/// let path = |id: &str, x: f64| SceneElement::Path(ScenePath {
///     source_id: id.into(),
///     cmds: vec![PathCmd::Move(Vec2::new(x, 0.0)), PathCmd::Line(Vec2::new(x, 5.0))],
///     style: PathStyle { stroke_width: 1.0, ..PathStyle::default() },
///     bbox: None,
/// });
/// let scene = Scene { elements: vec![path("a", 0.0), path("b", 0.0)] };
/// assert_eq!(hit_test(&scene, Vec2::new(0.0, 2.5), 0.0), Some(1));
/// ```
pub fn hit_test(scene: &Scene, point: Vec2, tolerance: f64) -> Option<usize> {
    scene
        .elements
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, element)| {
            let (stroke_width, drawable) = match element {
                SceneElement::Path(path) => {
                    (path.style.stroke_width, path.style.stroke != Paint::None)
                }
                SceneElement::Ellipse(ellipse) => (
                    ellipse.style.stroke_width,
                    ellipse.style.stroke != Paint::None,
                ),
                SceneElement::Text(_) => return None,
            };
            if !drawable {
                return None;
            }
            let distance = distance_to_element(point, element);
            (distance <= stroke_width / 2.0 + tolerance).then_some(index)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Paint, PathStyle};

    fn path(cmds: Vec<PathCmd>, width: f64) -> SceneElement {
        SceneElement::Path(ScenePath {
            source_id: "path:0".into(),
            cmds,
            style: PathStyle {
                stroke_width: width,
                ..PathStyle::default()
            },
            bbox: None,
        })
    }

    #[test]
    fn segment_distance_handles_endpoints_and_degenerate_input() {
        let a = Vec2::new(0.0, 0.0);
        let b = Vec2::new(4.0, 3.0);
        assert_eq!(distance_to_segment(Vec2::new(2.0, 1.5), a, b), 0.0);
        assert!((distance_to_segment(Vec2::new(4.0, 0.0), a, b) - 2.4).abs() < 1e-12);
        assert_eq!(distance_to_segment(Vec2::new(1.0, 1.0), a, a), 2f64.sqrt());
    }

    #[test]
    fn subpaths_are_flattened_separately() {
        let subpaths = flatten(&[
            PathCmd::Move(Vec2::new(0.0, 0.0)),
            PathCmd::Line(Vec2::new(10.0, 0.0)),
            PathCmd::Move(Vec2::new(20.0, 0.0)),
            PathCmd::Line(Vec2::new(30.0, 0.0)),
        ]);
        assert_eq!(subpaths.len(), 2);
        assert_eq!(subpaths[0], vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]);
    }

    #[test]
    fn circle_path_samples_the_full_ring() {
        let radius = 10.0;
        let cmds = vec![
            PathCmd::Move(Vec2::new(0.0, 0.0)),
            PathCmd::Move(Vec2::new(radius, 0.0)),
            PathCmd::Arc {
                rx: radius,
                ry: radius,
                rotation: 0.0,
                large: false,
                sweep: false,
                to: Vec2::new(-radius, 0.0),
            },
            PathCmd::Arc {
                rx: radius,
                ry: radius,
                rotation: 0.0,
                large: false,
                sweep: false,
                to: Vec2::new(radius, 0.0),
            },
            PathCmd::Close,
        ];
        let points: Vec<Vec2> = flatten(&cmds).into_iter().flatten().collect();
        // Every sampled point sits on the ring, and both the top and bottom
        // of the circle are covered.
        for point in &points {
            let r = (point.x * point.x + point.y * point.y).sqrt();
            assert!((r - radius).abs() < 1e-9 || r < 1e-9, "{point:?} at r={r}");
        }
        assert!(points.iter().any(|p| p.y > radius - 1e-6), "no top sample");
        assert!(
            points.iter().any(|p| p.y < -radius + 1e-6),
            "no bottom sample"
        );
    }

    #[test]
    fn stroke_width_and_tolerance_widen_the_pick_radius() {
        let scene = Scene {
            elements: vec![path(
                vec![
                    PathCmd::Move(Vec2::new(0.0, 0.0)),
                    PathCmd::Line(Vec2::new(0.0, 10.0)),
                ],
                2.0,
            )],
        };
        assert_eq!(hit_test(&scene, Vec2::new(1.2, 5.0), 0.0), None);
        assert_eq!(hit_test(&scene, Vec2::new(0.8, 5.0), 0.0), Some(0));
        assert_eq!(hit_test(&scene, Vec2::new(1.2, 5.0), 0.3), Some(0));
    }

    #[test]
    fn later_elements_win_and_strokeless_ones_are_skipped() {
        let invisible = SceneElement::Path(ScenePath {
            source_id: "path:1".into(),
            cmds: vec![
                PathCmd::Move(Vec2::new(0.0, 0.0)),
                PathCmd::Line(Vec2::new(0.0, 10.0)),
            ],
            style: PathStyle {
                stroke: Paint::None,
                ..PathStyle::default()
            },
            bbox: None,
        });
        let scene = Scene {
            elements: vec![
                path(
                    vec![
                        PathCmd::Move(Vec2::new(0.0, 0.0)),
                        PathCmd::Line(Vec2::new(0.0, 10.0)),
                    ],
                    0.4,
                ),
                invisible,
            ],
        };
        assert_eq!(hit_test(&scene, Vec2::new(0.0, 5.0), 0.2), Some(0));
    }

    #[test]
    fn ellipse_outline_is_pickable() {
        let scene = Scene {
            elements: vec![SceneElement::Ellipse(SceneEllipse {
                source_id: "path:0".into(),
                center: Vec2::new(0.0, 0.0),
                rx: 10.0,
                ry: 5.0,
                style: PathStyle::default(),
            })],
        };
        assert_eq!(hit_test(&scene, Vec2::new(10.0, 0.0), 0.2), Some(0));
        assert_eq!(hit_test(&scene, Vec2::new(0.0, 0.0), 0.2), None);
    }

    #[test]
    fn empty_scene_never_hits() {
        assert_eq!(
            hit_test(&Scene::default(), Vec2::new(0.0, 0.0), 100.0),
            None
        );
    }
}
