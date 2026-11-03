//! Capability registry: feature IDs, per-layer matrix, and layer registries.
//!
//! Port of `packages/core/src/capabilities/{feature-ids,matrix,registries}.ts`.
//! Drift between the matrix and the layer registries is guarded by the tests
//! in [`crate::capabilities::registries::drift_tests`].

pub mod feature_ids;
pub mod matrix;
pub mod registries;

pub use feature_ids::FEATURE_IDS;
pub use matrix::{capability_matrix, CapabilityRow, LayerStatus};
pub use registries::{
    all_registries, edit_feature_registry, parser_feature_registry, semantic_feature_registry,
    svg_feature_registry,
};
