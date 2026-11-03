//! Decoration capability rows.

use super::{CapabilityRow, N};

pub fn rows() -> Vec<(&'static str, CapabilityRow)> {
    vec![
        (
            "decorate_operation",
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
            "decorate_option",
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
            "decoration_pathmorphing",
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
            "decoration_pathreplacing",
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
            "decoration_fractals",
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
            "decoration_shape_marks",
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
            "decoration_footprints",
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
            "decoration_shape_backgrounds",
            CapabilityRow {
                parser: N,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
    ]
}
