//! Beamer and render-pipeline capability rows.

use super::{CapabilityRow, N, NA, S};

pub fn rows() -> Vec<(&'static str, CapabilityRow)> {
    vec![
        (
            "beamer_document_scan",
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
            "beamer_document_inspector",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_list_templates",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_frame_shrink",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_frame_flow",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_titlepages",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_theme_colors",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_tcolorbox",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_slide_manager",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_direct_resize",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_builds",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_source_placeholders",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_hyperlinks",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_manual_bibliography",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_footnotes",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_speaker_notes",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_float_captions",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_frame_continuations",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "beamer_overlay_pictures",
            CapabilityRow {
                parser: NA,
                semantic: NA,
                svg: NA,
                edit: N,
                fixtures: &[],
                notes: None,
            },
        ),
        (
            "render_pipeline",
            CapabilityRow {
                parser: NA,
                semantic: N,
                svg: S,
                edit: N,
                fixtures: &["basic_draw", "circle_shape", "node_text"],
                notes: Some("parse → semantic → SVG emit pipeline works for slice-3 subset"),
            },
        ),
    ]
}
