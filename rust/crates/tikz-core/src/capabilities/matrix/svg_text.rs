//! SVG text and text-feature capability rows.

use super::{CapabilityRow, N, NA, S};

pub fn rows() -> Vec<(&'static str, CapabilityRow)> {
    vec![
        (
            "svg_path",
            CapabilityRow {
                parser: NA,
                semantic: S,
                svg: S,
                edit: N,
                fixtures: &["basic_draw"],
                notes: None,
            },
        ),
        (
            "svg_circle",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: Some("circle drawn as path; SceneEllipse not yet emitted"),
            },
        ),
        (
            "svg_text",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: Some("text engine deferred to Phase 4"),
            },
        ),
        (
            "text_includegraphics",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "text_tabular",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "text_math_fonts",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "text_dirac_notation",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_scientific_notation",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_preformatted",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "text_transform_boxes",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: N,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
    ]
}
