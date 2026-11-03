//! Coordinate transforms accumulated from option lists.
//!
//! The legacy evaluator threads a matrix through style application; this
//! port keeps the three operations the corpus uses — scale, rotation about a
//! pivot and a shift — and applies them in the legacy order: rotate and
//! translate inside the scaled frame, i.e. `p' = S · (pivot + R·(p − pivot)
//! + shift)`.
//!
//! Verified against the goldens: `[scale=0.6]` on the picture turns
//! `(1,1)cm` into `0.6cm`, and `[xshift=5cm]` under that scale moves the
//! origin by `3cm` rather than `5cm`.
//!
//! ```
//! use tikz_core::semantic::transform::Transform;
//! use tikz_core::units::Vec2;
//!
//! let mut t = Transform::default();
//! t.scale(0.6, 0.6).expect("scale");
//! t.shift(Vec2::new(5.0 * 28.4527559055, 0.0));
//! assert!((t.apply(Vec2::new(0.0, 0.0)).x - 3.0 * 28.4527559055).abs() < 1e-9);
//! ```

use crate::units::Vec2;

/// Scale, rotation and shift applied to every coordinate of a statement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// X scale factor.
    pub scale_x: f64,
    /// Y scale factor.
    pub scale_y: f64,
    /// Rotation in degrees, counter-clockwise in TikZ canvas space.
    pub rotate_deg: f64,
    /// Rotation pivot in canvas space.
    pub pivot: Vec2,
    /// Linear shift in canvas space (applied inside the scaled frame).
    pub shift: Vec2,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            scale_x: 1.0,
            scale_y: 1.0,
            rotate_deg: 0.0,
            pivot: Vec2::new(0.0, 0.0),
            shift: Vec2::new(0.0, 0.0),
        }
    }
}

impl Transform {
    /// Multiply the scale factors.
    pub fn scale(&mut self, x: f64, y: f64) -> Result<(), String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("non-finite scale".to_string());
        }
        self.scale_x *= x;
        self.scale_y *= y;
        Ok(())
    }

    /// Add a rotation about `pivot` (the pivot is a canvas point).
    pub fn rotate(&mut self, degrees: f64, pivot: Vec2) -> Result<(), String> {
        if !degrees.is_finite() {
            return Err("non-finite rotation".to_string());
        }
        if self.rotate_deg == 0.0 {
            self.pivot = pivot;
        }
        self.rotate_deg += degrees;
        Ok(())
    }

    /// Add a shift.
    pub fn shift(&mut self, delta: Vec2) {
        self.shift = self.shift + delta;
    }

    /// Apply the transform to a coordinate in canvas space.
    ///
    /// ```
    /// use tikz_core::semantic::transform::Transform;
    /// use tikz_core::units::Vec2;
    /// let mut t = Transform::default();
    /// t.rotate(90.0, Vec2::new(0.0, 0.0)).expect("rotate");
    /// let p = t.apply(Vec2::new(1.0, 0.0));
    /// assert!(p.x.abs() < 1e-9 && (p.y - 1.0).abs() < 1e-9);
    /// ```
    pub fn apply(&self, point: Vec2) -> Vec2 {
        let radians = self.rotate_deg.to_radians();
        let (sin, cos) = radians.sin_cos();
        let dx = point.x - self.pivot.x;
        let dy = point.y - self.pivot.y;
        let rotated = Vec2::new(
            self.pivot.x + dx * cos - dy * sin,
            self.pivot.y + dx * sin + dy * cos,
        );
        let local = rotated + self.shift;
        Vec2::new(local.x * self.scale_x, local.y * self.scale_y)
    }

    /// Apply the transform's linear part to a direction (no shift, no pivot).
    ///
    /// ```
    /// use tikz_core::semantic::transform::Transform;
    /// use tikz_core::units::Vec2;
    /// let mut t = Transform::default();
    /// t.scale(2.0, 3.0).expect("scale");
    /// assert_eq!(t.apply_vector(Vec2::new(1.0, 1.0)), Vec2::new(2.0, 3.0));
    /// ```
    pub fn apply_vector(&self, vector: Vec2) -> Vec2 {
        let radians = self.rotate_deg.to_radians();
        let (sin, cos) = radians.sin_cos();
        Vec2::new(
            (vector.x * cos - vector.y * sin) * self.scale_x,
            (vector.x * sin + vector.y * cos) * self.scale_y,
        )
    }

    /// Linear part of the transform as `(a, b, c, d)` mapping a local vector
    /// to a world vector: `x' = a*x + c*y`, `y' = b*x + d*y`.
    ///
    /// This is the CTM linear part that PGF's `topaths` reads for `to`
    /// curves; a singular transform falls back to the identity.
    pub fn linear(&self) -> (f64, f64, f64, f64) {
        let radians = self.rotate_deg.to_radians();
        let (sin, cos) = radians.sin_cos();
        (
            self.scale_x * cos,
            -self.scale_x * sin,
            self.scale_y * sin,
            self.scale_y * cos,
        )
    }

    /// Apply the inverse linear part to a world vector, yielding a local
    /// vector. Returns `None` when the transform is singular.
    pub fn inverse_apply_vector(&self, vector: Vec2) -> Option<Vec2> {
        let (a, b, c, d) = self.linear();
        let det = a * d - b * c;
        if det.abs() <= 1e-12 || !det.is_finite() {
            return None;
        }
        let inv = 1.0 / det;
        Some(Vec2::new(
            (d * vector.x - c * vector.y) * inv,
            (a * vector.y - b * vector.x) * inv,
        ))
    }

    /// Whether the transform changes any geometry.
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_multiplies_increments_too() {
        let mut t = Transform::default();
        t.scale(0.6, 0.6).unwrap();
        let p = t.apply(Vec2::new(2.0 * crate::units::CM_TO_PT, 0.0));
        assert!((p.x - 1.2 * crate::units::CM_TO_PT).abs() < 1e-9);
    }

    #[test]
    fn rotate_around_a_pivot_matches_the_golden() {
        // grid.tex: \draw[rotate around={45:(3,1)}] (2.5,1) rectangle ...
        let mut t = Transform::default();
        t.rotate(45.0, Vec2::new(3.0, 1.0)).unwrap();
        let p = t.apply(Vec2::new(2.5, 1.0));
        assert!((p.x - 2.6464).abs() < 5e-4, "{p:?}");
        assert!((p.y - 0.6464).abs() < 5e-4, "{p:?}");
    }

    #[test]
    fn vector_transform_ignores_shift() {
        let mut t = Transform::default();
        t.shift(Vec2::new(10.0, 10.0));
        assert_eq!(t.apply_vector(Vec2::new(1.0, 0.0)), Vec2::new(1.0, 0.0));
    }
}
