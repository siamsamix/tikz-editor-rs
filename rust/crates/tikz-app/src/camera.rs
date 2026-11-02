//! Canvas camera: pan, zoom and the world <-> screen mapping.
//!
//! The scene lives in TikZ canvas points with y up (see
//! [`tikz_core::units::Vec2`]); egui paints in screen points with y down.
//! [`Camera`] owns exactly that conversion, so canvases never duplicate a
//! flip.

use egui::{Pos2, Rect, Vec2 as EVec2};
use tikz_core::units::Vec2;

/// Minimum and maximum zoom (screen points per TikZ point).
const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 4000.0;

/// Padding kept around fitted content, in screen points.
const FIT_PADDING: f32 = 24.0;

/// Pan/zoom state of the drawing canvas.
///
/// ```
/// use egui::{Pos2, Rect, vec2};
/// use tikz_core::units::Vec2;
/// use tikz_app::camera::Camera;
///
/// let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
/// let camera = Camera::default();
/// let world = Vec2::new(12.0, -4.0);
/// let screen = camera.world_to_screen(world, rect);
/// let back = camera.screen_to_world(screen, rect);
/// assert!((back.x - world.x).abs() < 1e-9 && (back.y - world.y).abs() < 1e-9);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// World point shown at the center of the canvas.
    pub center: Vec2,
    /// Screen points per world point.
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { center: Vec2::new(0.0, 0.0), zoom: 6.0 }
    }
}

impl Camera {
    /// Map a point in TikZ canvas space to canvas screen coordinates.
    pub fn world_to_screen(&self, point: Vec2, rect: Rect) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((point.x - self.center.x) * self.zoom) as f32,
            c.y - ((point.y - self.center.y) * self.zoom) as f32,
        )
    }

    /// Map a canvas screen position back into TikZ canvas space.
    pub fn screen_to_world(&self, pos: Pos2, rect: Rect) -> Vec2 {
        let c = rect.center();
        Vec2::new(
            self.center.x + (pos.x - c.x) as f64 / self.zoom,
            self.center.y - (pos.y - c.y) as f64 / self.zoom,
        )
    }

    /// Drag the camera by a screen-space delta.
    pub fn pan(&mut self, delta: EVec2) {
        self.center.x -= delta.x as f64 / self.zoom;
        self.center.y += delta.y as f64 / self.zoom;
    }

    /// Multiply the zoom by `factor`, keeping `anchor` (screen) fixed.
    ///
    /// ```
    /// use egui::{Pos2, Rect, vec2};
    /// use tikz_core::units::Vec2;
    /// use tikz_app::camera::Camera;
    ///
    /// let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    /// let mut camera = Camera { center: Vec2::new(5.0, 5.0), zoom: 4.0 };
    /// let anchor = Pos2::new(120.0, 90.0);
    /// let before = camera.screen_to_world(anchor, rect);
    /// camera.zoom_at(2.0, anchor, rect);
    /// let after = camera.screen_to_world(anchor, rect);
    /// assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
    /// assert!((camera.zoom - 8.0).abs() < 1e-9);
    /// ```
    pub fn zoom_at(&mut self, factor: f64, anchor: Pos2, rect: Rect) {
        let world = self.screen_to_world(anchor, rect);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let c = rect.center();
        self.center.x = world.x - (anchor.x - c.x) as f64 / self.zoom;
        self.center.y = world.y + (anchor.y - c.y) as f64 / self.zoom;
    }

    /// Frame a world-space bounding box with padding.
    ///
    /// ```
    /// use egui::{Pos2, Rect, vec2};
    /// use tikz_core::units::Vec2;
    /// use tikz_app::camera::Camera;
    ///
    /// let rect = Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0));
    /// let mut camera = Camera::default();
    /// let (lo, hi) = (Vec2::new(-10.0, -5.0), Vec2::new(30.0, 15.0));
    /// camera.fit((lo, hi), rect);
    /// assert!((camera.center.x - 10.0).abs() < 1e-9);
    /// assert!((camera.center.y - 5.0).abs() < 1e-9);
    /// assert!(camera.zoom > 0.0);
    /// ```
    pub fn fit(&mut self, bounds: (Vec2, Vec2), rect: Rect) {
        let (lo, hi) = bounds;
        self.center = Vec2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let usable_w = (rect.width() - 2.0 * FIT_PADDING).max(1.0) as f64;
        let usable_h = (rect.height() - 2.0 * FIT_PADDING).max(1.0) as f64;
        let span_x = (hi.x - lo.x).max(1e-6);
        let span_y = (hi.y - lo.y).max(1e-6);
        self.zoom = (usable_w / span_x).min(usable_h / span_y).clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), vec2(640.0, 480.0))
    }

    #[test]
    fn roundtrip_is_exact_enough_for_picking() {
        let camera = Camera { center: Vec2::new(-3.5, 7.25), zoom: 11.75 };
        for (x, y) in [(0.0, 0.0), (100.0, -50.0), (-12.5, 3.0)] {
            let back = camera.screen_to_world(camera.world_to_screen(Vec2::new(x, y), rect()), rect());
            assert!((back.x - x).abs() < 1e-6, "{back:?}");
            assert!((back.y - y).abs() < 1e-6, "{back:?}");
        }
    }

    #[test]
    fn y_axis_points_up_on_screen() {
        let camera = Camera::default();
        let low = camera.world_to_screen(Vec2::new(0.0, 0.0), rect());
        let high = camera.world_to_screen(Vec2::new(0.0, 10.0), rect());
        assert!(high.y < low.y);
    }

    #[test]
    fn pan_moves_content_with_the_pointer() {
        let mut camera = Camera::default();
        let before = camera.world_to_screen(Vec2::new(0.0, 0.0), rect());
        camera.pan(vec2(25.0, -10.0));
        let after = camera.world_to_screen(Vec2::new(0.0, 0.0), rect());
        assert!((after.x - before.x - 25.0).abs() < 1e-4);
        assert!((after.y - before.y + 10.0).abs() < 1e-4);
    }

    #[test]
    fn fit_clamps_zoom_for_degenerate_bounds() {
        let mut camera = Camera::default();
        camera.fit((Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0)), rect());
        assert!(camera.zoom <= MAX_ZOOM && camera.zoom >= MIN_ZOOM);
        assert_eq!(camera.center, Vec2::new(0.0, 0.0));
    }
}