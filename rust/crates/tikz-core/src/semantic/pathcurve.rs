//! `to`/bend curves and `.. controls .. and ..` cubics.
//!
//! Port of `packages/core/src/semantic/path/to-operation.ts` (`appendToCurve`)
//! and `curves.ts` (`parseBezierFromItems` / `appendSinCosSegment`).
//!
//! PGF `topaths` fixed-point math (blueprint row 26 / TODO trap T9):
//!
//! ```text
//! divisor = trunc(round(largest / length * 65536) / 255)
//! pgfDistance = 16 * trunc(16 * round(largest * 65536) / divisor) / 65536
//! baseDistance = pgfDistance * 0.3915
//! ```
//!
//! Control points are placed at `out`/`in` angles, `baseDistance * looseness`
//! from the endpoints, mapped through the statement's CTM linear part. The
//! curve's `out`/`in` angles and the bend's turn are read in the transformed
//! frame, so `rotate=90` turns `out=0` upward and a bend flattens under
//! `xscale=2`.

use crate::semantic::transform::Transform;
use crate::units::Vec2;

/// The resolved `to` curve options, computed from the option list following
/// a `to`/`edge` operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToCurve {
    /// Out angle in degrees, measured in the local (untransformed) frame.
    pub out: f64,
    /// In angle in degrees, measured in the local frame.
    pub in_angle: f64,
    /// Out looseness multiplier (default 1).
    pub out_looseness: f64,
    /// In looseness multiplier (default 1).
    pub in_looseness: f64,
    /// Base heading of the chord in degrees (local frame).
    pub base_heading: f64,
    /// Signed bend angle from `bend left`/`bend right` (degrees).
    pub bend_signed_angle: f64,
}

impl Default for ToCurve {
    fn default() -> Self {
        Self {
            out: 45.0,
            in_angle: 135.0,
            out_looseness: 1.0,
            in_looseness: 1.0,
            base_heading: 0.0,
            bend_signed_angle: 0.0,
        }
    }
}

/// PGF fixed-point base distance for a chord `delta`.
///
/// ```text
/// divisor = trunc(round(largest / length * 65536) / 255)
/// pgfDistance = 16 * trunc(16 * round(largest * 65536) / divisor) / 65536
/// baseDistance = pgfDistance * 0.3915
/// ```
pub fn pgf_base_distance(delta: Vec2) -> f64 {
    let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
    let largest = delta.x.abs().max(delta.y.abs());
    if length == 0.0 {
        return 0.0;
    }
    let divisor = ((largest / length * 65536.0).round() / 255.0).trunc();
    if divisor == 0.0 {
        return 0.0;
    }
    let pgf_distance = 16.0 * ((16.0 * largest * 65536.0).round() / divisor).trunc() / 65536.0;
    pgf_distance * 0.3915
}

/// Direction of a `bend left`/`bend right` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BendDir {
    Left,
    Right,
}

/// Options that feed `to`/`edge` curve resolution, mirroring legacy
/// `extractToCurveOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToCurveOptions {
    /// `out` angle in degrees; `None` means the default `45`.
    pub out: Option<f64>,
    /// `in` angle in degrees; `None` means the default `135`.
    pub in_angle: Option<f64>,
    /// `looseness` multiplier applied to both distances; `None` means `1`.
    pub looseness: Option<f64>,
    /// `out looseness` multiplier; `None` means `looseness` / `1`.
    pub out_looseness: Option<f64>,
    /// `in looseness` multiplier; `None` means `looseness` / `1`.
    pub in_looseness: Option<f64>,
    /// Signed bend angle from `bend left`/`bend right` (`+` left, `-` right).
    pub bend_signed: Option<f64>,
    /// Base heading of the chord in degrees (local frame).
    pub base_heading: f64,
    /// Whether angles are relative to the chord heading (set by `relative`/bend).
    pub relative: bool,
    /// Whether `out`/`in` were given explicitly.
    pub has_explicit_in_or_out: bool,
    /// Whether `bend left`/`bend right` was present.
    pub has_bend_direction: bool,
    /// Direction of the bend when present.
    pub bend_direction: Option<BendDir>,
}

/// Resolve the `to` curve options from the option list following `to`/`edge`.
///
/// Mirrors legacy `extractToCurveOptions`: `out`/`in` default to `45`/`135`;
/// `bend left`/`bend right` override them relative to the chord heading
/// (`out = +/-bendAngle`, `in = 180 - out`); `relative` adds `baseHeading`.
pub fn resolve_to_curve(options: ToCurveOptions) -> ToCurve {
    let ToCurveOptions {
        out,
        in_angle,
        looseness,
        out_looseness,
        in_looseness,
        bend_signed,
        base_heading,
        relative,
        has_explicit_in_or_out,
        has_bend_direction,
        bend_direction,
    } = options;
    let mut out = out.unwrap_or(45.0);
    let mut in_angle = in_angle.unwrap_or(135.0);
    let shared = looseness.unwrap_or(1.0);
    let out_looseness = out_looseness.unwrap_or(shared);
    let in_looseness = in_looseness.unwrap_or(shared);
    let mut bend_signed_angle = bend_signed.unwrap_or(0.0);

    if has_bend_direction {
        // bend left/right set out/in from the bend angle, relative to chord.
        out = bend_signed.unwrap_or(0.0);
        in_angle = 180.0 - out;
        out += base_heading;
        in_angle += base_heading;
        match bend_direction {
            Some(BendDir::Left) => bend_signed_angle = bend_signed_angle.abs(),
            _ => bend_signed_angle = -bend_signed_angle.abs(),
        }
    } else if relative {
        out += base_heading;
        in_angle += base_heading;
        if !has_explicit_in_or_out {
            bend_signed_angle = normalize_signed_degrees(out - base_heading);
        }
    } else if !has_explicit_in_or_out {
        // Plain `to` with no options: relative out/in from the chord.
        out += base_heading;
        in_angle += base_heading;
        bend_signed_angle = normalize_signed_degrees(out - base_heading);
    }

    ToCurve {
        out,
        in_angle,
        out_looseness,
        in_looseness,
        base_heading,
        bend_signed_angle,
    }
}

/// Compute the two cubic control points for a `to` curve from `from` to `to`.
///
/// Angles are applied in the local frame and mapped through `transform`'s
/// linear part (the CTM). Returns `(c1, c2, end)` in canvas space.
pub fn to_curve_controls(
    from: Vec2,
    to: Vec2,
    transform: &Transform,
    curve: &ToCurve,
) -> Option<(Vec2, Vec2, Vec2)> {
    let delta = transform.inverse_apply_vector(to - from)?;
    let base = pgf_base_distance(delta);
    if base == 0.0 {
        return None;
    }
    let mut out_distance = base * curve.out_looseness;
    let mut in_distance = base * curve.in_looseness;
    out_distance = out_distance.clamp(0.0, f64::INFINITY);
    in_distance = in_distance.clamp(0.0, f64::INFINITY);

    let out_radians = curve.out.to_radians();
    let in_radians = curve.in_angle.to_radians();
    let out_vector = transform.apply_vector(Vec2::new(
        out_distance * out_radians.cos(),
        out_distance * out_radians.sin(),
    ));
    let in_vector = transform.apply_vector(Vec2::new(
        in_distance * in_radians.cos(),
        in_distance * in_radians.sin(),
    ));
    let c1 = from + out_vector;
    let c2 = to + in_vector;
    Some((c1, c2, to))
}

/// Normalize a signed degree value into `(-180, 180]`.
fn normalize_signed_degrees(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let wrapped = ((value + 180.0) % 360.0 + 360.0) % 360.0 - 180.0;
    if wrapped.abs() < 1e-9 {
        0.0
    } else {
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::transform::Transform;

    #[test]
    fn pgf_base_distance_matches_fixed_point() {
        // (3cm, 1cm) chord: largest=3cm, length=hypot.
        let cm = crate::units::CM_TO_PT;
        let delta = Vec2::new(3.0 * cm, 1.0 * cm);
        let base = pgf_base_distance(delta);
        // divisor = trunc(round(3/hypot * 65536) / 255); pgfDistance; *0.3915
        let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
        let largest = delta.x.abs().max(delta.y.abs());
        let divisor = ((largest / length * 65536.0).round() / 255.0).trunc();
        let pgf = 16.0 * ((16.0 * largest * 65536.0).round() / divisor).trunc() / 65536.0;
        assert!(
            (base - pgf * 0.3915).abs() < 1e-6,
            "{base} != {}",
            pgf * 0.3915
        );
    }

    #[test]
    fn bend_right_turns_clockwise() {
        let t = Transform::default();
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(3.0 * crate::units::CM_TO_PT, 1.0 * crate::units::CM_TO_PT);
        let delta = to - from;
        let base_heading = delta.y.atan2(delta.x).to_degrees();
        let curve = resolve_to_curve(ToCurveOptions {
            out: None,
            in_angle: None,
            looseness: None,
            out_looseness: None,
            in_looseness: None,
            bend_signed: Some(-30.0), // bend right = -30
            base_heading,
            relative: true,
            has_explicit_in_or_out: false,
            has_bend_direction: true,
            bend_direction: Some(BendDir::Right),
        });
        let (c1, c2, end) = to_curve_controls(from, to, &t, &curve).unwrap();
        assert_eq!(end, to);
        // c1 should lie "below" the chord (negative y) for a right bend.
        assert!(c1.y < from.y, "{c1:?} should be below {from:?}");
        assert!(c2.y < to.y, "{c2:?} should be below {to:?}");
    }

    #[test]
    fn bend_right_matches_legacy_golden_curve() {
        // \draw[->] (a) to[bend right=30] (b); chord from (0,0) to (3cm,1cm).
        // PGF fixed-point base distance and bend placement; control points lie to the
        // right/below the chord for `bend right`.
        let t = Transform::default();
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(3.0 * crate::units::CM_TO_PT, 1.0 * crate::units::CM_TO_PT);
        let delta = to - from;
        let base_heading = delta.y.atan2(delta.x).to_degrees();
        let curve = resolve_to_curve(ToCurveOptions {
            out: None,
            in_angle: None,
            looseness: None,
            out_looseness: None,
            in_looseness: None,
            bend_signed: Some(-30.0),
            base_heading,
            relative: true,
            has_explicit_in_or_out: false,
            has_bend_direction: true,
            bend_direction: Some(BendDir::Right),
        });
        // Relative: out = -30 + base_heading, in = 210 + base_heading.
        assert!(
            (curve.out - (base_heading - 30.0)).abs() < 1e-9,
            "out={}",
            curve.out
        );
        assert!(
            (curve.in_angle - (base_heading + 210.0)).abs() < 1e-9,
            "in={}",
            curve.in_angle
        );
        assert!(
            (curve.bend_signed_angle - (-30.0)).abs() < 1e-9,
            "bend={}",
            curve.bend_signed_angle
        );
        let (c1, c2, end) = to_curve_controls(from, to, &t, &curve).unwrap();
        assert_eq!(end, to);
        // A right bend pulls both controls below the chord line (negative y side).
        assert!(c1.y < from.y, "{c1:?} should be below {from:?}");
        assert!(c2.y < to.y, "{c2:?} should be below {to:?}");
        // Controls lie inside the chord's bounding box horizontally.
        assert!(c1.x > from.x && c1.x < to.x, "{c1:?} outside chord x-range");
        assert!(c2.x > from.x && c2.x < to.x, "{c2:?} outside chord x-range");
    }

    #[test]
    fn default_to_curve_uses_forty_five() {
        let t = Transform::default();
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(10.0, 0.0);
        let curve = resolve_to_curve(ToCurveOptions::default());
        assert_eq!(curve.out, 45.0);
        assert_eq!(curve.in_angle, 135.0);
        let (c1, c2, _end) = to_curve_controls(from, to, &t, &curve).unwrap();
        assert!(c1.y > 0.0);
        assert!(c2.y > 0.0);
    }
}
