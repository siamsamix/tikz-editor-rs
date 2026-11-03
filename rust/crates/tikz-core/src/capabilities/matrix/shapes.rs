//! Node shape capability rows.

use super::{CapabilityRow, N, P, S};

pub fn rows() -> Vec<(&'static str, CapabilityRow)> {
    vec![
        (
            "shape_rectangle",
            CapabilityRow {
                parser: S,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: Some("rectangle keyword parses; node shapes deferred"),
            },
        ),
        (
            "shape_rounded_rectangle",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_chamfered_rectangle",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_cross_out",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_strike_out",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_circle",
            CapabilityRow {
                parser: S,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: Some("circle keyword parses; SceneEllipse not yet emitted"),
            },
        ),
        (
            "shape_magnifying_glass",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_circle_split",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_circle_solidus",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_ellipse_split",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_diamond_split",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_rectangle_split",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_ellipse",
            CapabilityRow {
                parser: S,
                semantic: P,
                svg: P,
                edit: N,
                fixtures: &["ellipse_keyword"],
                notes: Some(
                    "ellipse keyword parses; SceneEllipse emitted for [x radius and y radius]",
                ),
            },
        ),
        (
            "shape_diamond",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_trapezium",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_semicircle",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_isosceles_triangle",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_kite",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_dart",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_circular_sector",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_cylinder",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_regular_polygon",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_star",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_cloud",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_starburst",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_signal",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_tape",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_rectangle_callout",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_ellipse_callout",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_cloud_callout",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_single_arrow",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "shape_double_arrow",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "matrix_node",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "fit_node",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "keyword_ellipse",
            CapabilityRow {
                parser: S,
                semantic: P,
                svg: P,
                edit: N,
                fixtures: &["ellipse_keyword"],
                notes: Some("ellipse keyword parses; radii via options"),
            },
        ),
        (
            "keyword_arc",
            CapabilityRow {
                parser: S,
                semantic: P,
                svg: P,
                edit: N,
                fixtures: &["arc_keyword"],
                notes: Some("arc keyword parses; radii/angles via options"),
            },
        ),
        (
            "keyword_grid",
            CapabilityRow {
                parser: S,
                semantic: S,
                svg: S,
                edit: N,
                fixtures: &["grid_keyword"],
                notes: None,
            },
        ),
    ]
}
