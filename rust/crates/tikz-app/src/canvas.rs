//! Drawing canvas: paints a [`Scene`] and owns pointer interaction.
//!
//! The canvas replaces the browser SVG hit-testing machinery (blueprint
//! row 6) with [`tikz_core::hit_test`] plus a pan/zoom [`Camera`]. It never
//! mutates the document; the shell decides what a click means.

use egui::{Color32, Pos2, Sense, Stroke, StrokeKind};
use tikz_core::hit_test::{flatten_element, hit_test};
use tikz_core::scene::{Paint, Scene, SceneElement};
use tikz_core::units::Vec2;

use crate::camera::Camera;

/// Extra pick radius in screen points, on top of half the stroke width.
const PICK_PAD_PT: f64 = 5.0;

/// Fallback stroke colour for the legacy keyword `black`.
const BLACK_STROKE: Color32 = Color32::from_rgb(0, 0, 0);
/// Selection accent.
const SELECTED_COLOR: Color32 = Color32::from_rgb(24, 108, 214);
/// Hover accent.
const HOVER_COLOR: Color32 = Color32::from_rgb(222, 123, 24);
/// Canvas background.
const CANVAS_BG: Color32 = Color32::from_rgb(250, 250, 249);

/// What the canvas observed this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CanvasOutcome {
    /// Pointer position in TikZ canvas points.
    pub cursor_world: Option<Vec2>,
    /// Path index under the pointer, if any.
    pub hovered: Option<usize>,
    /// Path index picked by a click.
    pub clicked: Option<usize>,
    /// A click landed on empty canvas.
    pub clicked_empty: bool,
}

/// Paint `scene` and process pan/zoom/click interaction.
///
/// `selection` only changes the paint style; the caller owns the selected
/// index and applies [`CanvasOutcome::clicked`] / `clicked_empty`.
pub fn show(
    ui: &mut egui::Ui,
    camera: &mut Camera,
    scene: Option<&Scene>,
    selection: Option<usize>,
) -> CanvasOutcome {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    let painter = painter.with_clip_rect(rect);

    painter.rect_filled(rect, 0.0, CANVAS_BG);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, Color32::from_gray(205)), StrokeKind::Inside);

    if response.dragged() {
        camera.pan(response.drag_delta());
    }
    if let Some(anchor) = response.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            camera.zoom_at((f64::from(scroll) * 0.002).exp(), anchor, rect);
        }
        let pinch = ui.input(|i| i.zoom_delta());
        if (pinch - 1.0).abs() > 1e-3 {
            camera.zoom_at(f64::from(pinch), anchor, rect);
        }
    }

    let mut outcome = CanvasOutcome {
        cursor_world: response.hover_pos().map(|p| camera.screen_to_world(p, rect)),
        ..Default::default()
    };

    if let (Some(scene), Some(pointer)) = (scene, response.hover_pos()) {
        let world = camera.screen_to_world(pointer, rect);
        outcome.hovered = hit_test(scene, world, PICK_PAD_PT / camera.zoom);
    }

    if let Some(scene) = scene {
        paint_scene(&painter, camera, rect, scene, selection, outcome.hovered);
    } else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "no scene — fix the diagnostics below",
            egui::FontId::proportional(15.0),
            Color32::from_gray(130),
        );
    }

    if response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let hit = scene.and_then(|scene| {
                let world = camera.screen_to_world(pointer, rect);
                hit_test(scene, world, PICK_PAD_PT / camera.zoom)
            });
            match hit {
                Some(index) => outcome.clicked = Some(index),
                None => outcome.clicked_empty = true,
            }
        }
    }

    outcome
}

fn paint_scene(
    painter: &egui::Painter,
    camera: &Camera,
    rect: egui::Rect,
    scene: &Scene,
    selection: Option<usize>,
    hovered: Option<usize>,
) {
    for (index, element) in scene.elements.iter().enumerate() {
        paint_element(painter, camera, rect, element, 0.0);
        let accent = if Some(index) == selection {
            Some(SELECTED_COLOR)
        } else if Some(index) == hovered {
            Some(HOVER_COLOR)
        } else {
            None
        };
        if let Some(color) = accent {
            paint_highlight(painter, camera, rect, element, color);
        }
    }
}

/// Fill then stroke one element with its own paint state.
fn paint_element(
    painter: &egui::Painter,
    camera: &Camera,
    rect: egui::Rect,
    element: &SceneElement,
    extra_width: f32,
) {
    let style = element.style();
    let subpaths = flatten_element(element);
    if let Some(fill) = paint_color(style.fill) {
        let fill = fill.gamma_multiply(style.fill_opacity as f32);
        for subpath in &subpaths {
            let points: Vec<Pos2> = subpath.iter().map(|p| camera.world_to_screen(*p, rect)).collect();
            if points.len() >= 3 {
                // epaint fills convex polygons only; every shape this slice
                // emits (rectangle, circle, ellipse) is convex.
                painter.add(egui::Shape::convex_polygon(points, fill, Stroke::NONE));
            }
        }
    }
    if let Some(stroke_color) = paint_color(style.stroke) {
        let width = ((style.stroke_width * camera.zoom) as f32).max(1.0) + extra_width;
        let stroke = Stroke::new(width, stroke_color.gamma_multiply(style.stroke_opacity as f32));
        for subpath in &subpaths {
            let points: Vec<Pos2> = subpath.iter().map(|p| camera.world_to_screen(*p, rect)).collect();
            for pair in points.windows(2) {
                painter.line_segment([pair[0], pair[1]], stroke);
            }
            if points.len() == 1 {
                painter.circle_filled(points[0], (width / 2.0).max(0.5), stroke.color);
            }
        }
    }
}

/// Accent halo around a selected or hovered element.
fn paint_highlight(
    painter: &egui::Painter,
    camera: &Camera,
    rect: egui::Rect,
    element: &SceneElement,
    color: Color32,
) {
    let style = element.style();
    let subpaths = flatten_element(element);
    let width = ((style.stroke_width * camera.zoom) as f32).max(1.0) + 6.0;
    let stroke = Stroke::new(width, color.gamma_multiply(0.25));
    for subpath in &subpaths {
        let points: Vec<Pos2> = subpath.iter().map(|p| camera.world_to_screen(*p, rect)).collect();
        for pair in points.windows(2) {
            painter.line_segment([pair[0], pair[1]], stroke);
        }
    }
}

/// Map scene paint to a canvas colour; `None` means "do not paint".
fn paint_color(paint: Paint) -> Option<Color32> {
    match paint {
        Paint::None => None,
        Paint::Black => Some(BLACK_STROKE),
        Paint::Rgb(r, g, b) => Some(Color32::from_rgb(r, g, b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_outcome_has_no_interaction() {
        let outcome = CanvasOutcome::default();
        assert_eq!(outcome.cursor_world, None);
        assert_eq!(outcome.hovered, None);
        assert_eq!(outcome.clicked, None);
        assert!(!outcome.clicked_empty);
    }
}