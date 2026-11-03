//! Capability matrix: per-feature, per-layer support status.
//!
//! Port of `packages/core/src/capabilities/matrix.ts`. Every feature ID in
//! [`crate::capabilities::feature_ids::FEATURE_IDS`] must appear exactly once
//! here. Statuses are **honest about the current port state** (slice-3); only
//! what is actually implemented is `Partial`/`Stable`, everything else is
//! `None`. The drift test enforces that the layer registries stay aligned with
//! this matrix.
//!
//! The row data lives in per-category submodules so no single file exceeds the
//! port's 400 LOC module budget.

/// Shortcut constructors shared by category submodules.
pub(crate) const N: LayerStatus = LayerStatus::None;
pub(crate) const P: LayerStatus = LayerStatus::Partial;
pub(crate) const S: LayerStatus = LayerStatus::Stable;
pub(crate) const NA: LayerStatus = LayerStatus::NotApplicable;

mod beamer;
mod decorations;
mod paths;
mod shapes;
mod svg_text;

use crate::capabilities::feature_ids::FEATURE_IDS;

/// Support level for one pipeline layer of one feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayerStatus {
    /// Not implemented at all.
    #[default]
    None,
    /// Implemented but incomplete or subset.
    Partial,
    /// Fully supported at parity with the legacy implementation.
    Stable,
    /// Concept does not apply to this layer.
    NotApplicable,
}

impl LayerStatus {
    /// Whether the registry for this layer should include the feature.
    pub fn is_supported(self) -> bool {
        matches!(self, LayerStatus::Partial | LayerStatus::Stable)
    }
}

/// One row of the capability matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRow {
    pub parser: LayerStatus,
    pub semantic: LayerStatus,
    pub svg: LayerStatus,
    pub edit: LayerStatus,
    pub fixtures: &'static [&'static str],
    pub notes: Option<&'static str>,
}

/// Assemble the full matrix from every category submodule, in `FEATURE_IDS`
/// order. The drift test verifies completeness and uniqueness.
pub fn capability_matrix() -> Vec<(&'static str, CapabilityRow)> {
    let mut rows: Vec<(&'static str, CapabilityRow)> = Vec::new();
    rows.extend(paths::rows());
    rows.extend(shapes::rows());
    rows.extend(decorations::rows());
    rows.extend(svg_text::rows());
    rows.extend(beamer::rows());

    let ids: Vec<&str> = rows.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids.len(),
        FEATURE_IDS.len(),
        "matrix row count must match FEATURE_IDS count"
    );
    assert!(
        ids.windows(2).all(|w| w[0] != w[1]),
        "matrix rows must be unique"
    );
    for id in FEATURE_IDS {
        assert!(
            rows.iter().any(|(fid, _)| *fid == id),
            "feature id {id} missing from capability matrix"
        );
    }
    rows
}
