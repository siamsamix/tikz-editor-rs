//! Low-level scanning and parsing helpers for path statements.
//!
//! Split out of [`super::pathbuild`] to keep both modules inside the port's
//! `<= 400` LOC budget: this module owns text scanning, length/coordinate
//! parsing and the radius/arc geometry helpers, while `pathbuild` owns the
//! statement state machine and the shape emitters.

use crate::semantic::expr::{eval_len, eval_number, Unit};
use crate::semantic::style::Issue;
use crate::semantic::transform::Transform;
use crate::span::Span;
use crate::units::Vec2;

/// A length plus whether the source carried an explicit unit.
pub type Length = (f64, bool);

/// Arc parameters in local (untransformed) units.
#[derive(Debug, Clone, Copy)]
pub struct ArcParams {
    /// Start angle in degrees.
    pub start_angle: f64,
    /// End angle in degrees.
    pub end_angle: f64,
    /// X radius with its explicit-unit flag.
    pub rx: Length,
    /// Y radius with its explicit-unit flag.
    pub ry: Length,
}

/// Multiples of `step` inside `[from, to]`, inclusive (bounded).
pub fn steps(from: f64, to: f64, step: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if step <= 0.0 || !step.is_finite() {
        return out;
    }
    let mut index = (from / step).ceil();
    let last = (to / step).floor();
    while index <= last && out.len() < 4096 {
        out.push(index * step);
        index += 1.0;
    }
    out
}

pub fn parse_arc_shorthand(inner: &str) -> Result<ArcParams, String> {
    let parts: Vec<&str> = inner.split(':').collect();
    if parts.len() != 3 {
        return Err(format!("expected `arc (start:end:radius)`, got `({inner})`"));
    }
    let start_angle = eval_number(parts[0])?;
    let end_angle = eval_number(parts[1])?;
    let radius = parts[2].trim();
    if let Some((rx, ry)) = radius.split_once(" and ") {
        Ok(ArcParams { start_angle, end_angle, rx: parse_length(rx)?, ry: parse_length(ry)? })
    } else {
        let value = parse_length(radius)?;
        Ok(ArcParams { start_angle, end_angle, rx: value, ry: value })
    }
}

/// Parse a length, reporting whether it carried an explicit unit.
pub fn parse_length(raw: &str) -> Result<Length, String> {
    let trimmed = raw.trim().trim_start_matches('{').trim_end_matches('}').trim();
    let explicit = trimmed.chars().any(|c| c.is_ascii_alphabetic());
    Ok((eval_len(trimmed, Unit::Cm)?, explicit))
}

/// Parse the radii of an `ellipse (rx and ry)` group.
pub fn parse_radii(inner: &str) -> Result<(Length, Length), String> {
    let (rx, ry) = inner
        .split_once(" and ")
        .ok_or_else(|| format!("expected `ellipse (rx and ry)`, got `({inner})`"))?;
    Ok((parse_length(rx)?, parse_length(ry)?))
}

/// Parse `(x,y)` or `(angle:radius)`.
pub fn parse_coordinate(inner: &str) -> Result<Vec2, String> {
    let text = inner.trim();
    if let Some((angle, radius)) = text.split_once(':') {
        let angle = eval_number(angle)?;
        let radius = eval_len(radius, Unit::Cm)?;
        let radians = angle.to_radians();
        return Ok(Vec2::new(radius * radians.cos(), radius * radians.sin()));
    }
    if let Some((x, y)) = text.split_once(',') {
        return Ok(Vec2::new(eval_len(x, Unit::Cm)?, eval_len(y, Unit::Cm)?));
    }
    let name = text.trim();
    if name.is_empty() {
        return Err("empty coordinate".to_string());
    }
    if name.starts_with('$') {
        return Err("calc coordinates are not supported yet".to_string());
    }
    Err(format!("named coordinate `{name}` needs nodes, which are not supported yet"))
}

pub fn parse_point(inner: &str) -> Result<Vec2, String> {
    let text = inner.trim();
    let (x, y) = text
        .split_once(',')
        .ok_or_else(|| format!("expected `(x,y)`, got `({inner})`"))?;
    Ok(Vec2::new(eval_len(x, Unit::Cm)?, eval_len(y, Unit::Cm)?))
}

/// Scale radii by the transform; dimensionless radii follow the frame.
pub fn scale_radii_pair(transform: &Transform, rx: Length, ry: Length) -> (f64, f64) {
    let scaled_x = if rx.1 {
        rx.0
    } else {
        transform.apply_vector(Vec2::new(rx.0, 0.0)).x.abs()
    };
    let scaled_y = if ry.1 {
        ry.0
    } else {
        transform.apply_vector(Vec2::new(0.0, ry.0)).y.abs()
    };
    (scaled_x, scaled_y)
}

/// Ellipse axes of a transformed radius basis (legacy `ellipseGeometryFromBasis`).
pub fn ellipse_from_basis(transform: &Transform, rx: f64, ry: f64) -> (f64, f64, f64) {
    let bx = transform.apply_vector(Vec2::new(rx, 0.0));
    let by = transform.apply_vector(Vec2::new(0.0, ry));
    let s11 = bx.x * bx.x + by.x * by.x;
    let s12 = bx.x * bx.y + by.x * by.y;
    let s22 = bx.y * bx.y + by.y * by.y;
    let trace_half = (s11 + s22) / 2.0;
    let discriminant = (trace_half * trace_half - (s11 * s22 - s12 * s12)).max(0.0).sqrt();
    let major = (trace_half + discriminant).max(0.0).sqrt();
    let minor = (trace_half - discriminant).max(0.0).sqrt();
    let rotation = if (major - minor).abs() <= 1e-9 {
        0.0
    } else {
        (0.5 * (2.0 * s12).atan2(s11 - s22)).to_degrees()
    };
    (
        if major > 1e-9 { major } else { bx.x.hypot(bx.y) },
        if minor > 1e-9 { minor } else { by.x.hypot(by.y) },
        if rotation.abs() <= 1e-9 { 0.0 } else { rotation },
    )
}

/// Skip `[...]` / `{...}` groups after a keyword we do not model.
pub fn skip_optional_groups(src: &str, from: usize, to: usize, issues: &mut Vec<Issue>) -> usize {
    let mut at = from;
    loop {
        at = skip_trivia(src, at, to);
        match src.as_bytes().get(at) {
            Some(b'[') => match matching(src, at, to, b'[', b']') {
                Some(close) => at = close + 1,
                None => {
                    issues.push(Issue::error(Span::new(at, to), "unterminated option list"));
                    return to;
                }
            },
            Some(b'{') => match matching(src, at, to, b'{', b'}') {
                Some(close) => at = close + 1,
                None => {
                    issues.push(Issue::error(Span::new(at, to), "unterminated group"));
                    return to;
                }
            },
            _ => return at,
        }
    }
}

pub fn skip_trivia(src: &str, mut at: usize, to: usize) -> usize {
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

/// Read a bare word (`rectangle`, `cycle`, ...) starting at `at`.
pub fn read_word(src: &str, at: usize, to: usize) -> (&str, usize) {
    let bytes = src.as_bytes();
    let mut end = at;
    while end < to && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'_') {
        end += 1;
    }
    (&src[at..end], end)
}

/// Index of the delimiter closing the one at `open` (comments are inert).
pub fn matching(src: &str, open: usize, end: usize, lo: u8, hi: u8) -> Option<usize> {
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

    #[test]
    fn step_enumeration_is_inclusive_and_bounded() {
        let values = steps(-11.3811, 110.9657, crate::units::CM_TO_PT);
        assert_eq!(values.len(), 4);
        assert!((values[0] - 0.0).abs() < 1e-9);
        assert!((values[3] - 3.0 * crate::units::CM_TO_PT).abs() < 1e-6);
        assert!(steps(0.0, 0.0, 0.0).is_empty());
        assert_eq!(steps(0.0, 10.0, 2.0), vec![0.0, 2.0, 4.0, 6.0, 8.0, 10.0]);
    }

    #[test]
    fn coordinate_forms_evaluate() {
        assert_eq!(
            parse_coordinate("1,2").unwrap(),
            Vec2::new(crate::units::CM_TO_PT, 2.0 * crate::units::CM_TO_PT)
        );
        let polar = parse_coordinate("30:2").unwrap();
        assert!((polar.x - 2.0 * crate::units::CM_TO_PT * 30f64.to_radians().cos()).abs() < 1e-9);
        assert!(parse_coordinate("A").unwrap_err().contains("named coordinate"));
        assert!(parse_coordinate("$1+1$").unwrap_err().contains("calc"));
    }

    #[test]
    fn ellipse_basis_of_a_uniform_scale_is_axis_aligned() {
        let mut transform = Transform::default();
        transform.scale(0.6, 0.6).unwrap();
        let (rx, ry, rotation) = ellipse_from_basis(&transform, 10.0, 5.0);
        assert!((rx - 6.0).abs() < 1e-9);
        assert!((ry - 3.0).abs() < 1e-9);
        assert_eq!(rotation, 0.0);
    }

}
