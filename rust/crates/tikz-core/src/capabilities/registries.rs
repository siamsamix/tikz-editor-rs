//! Layer feature registries derived from the capability matrix.
//!
//! Port of `packages/core/src/capabilities/registries.ts`. Each registry lists
//! the feature IDs whose status for that layer is `Partial` or `Stable`.
//! A drift test enforces that these registries stay aligned with the matrix.

use crate::capabilities::feature_ids::FEATURE_IDS;
use crate::capabilities::matrix::{capability_matrix, CapabilityRow, LayerStatus};

type CapabilityLayer = &'static str;

fn features_supported_by(layer: CapabilityLayer) -> Vec<&'static str> {
    FEATURE_IDS
        .iter()
        .copied()
        .filter(|&id| {
            capability_matrix()
                .iter()
                .find(|(fid, _)| *fid == id)
                .map(|(_, row): &(&str, CapabilityRow)| status_for(layer, row).is_supported())
                .unwrap_or(false)
        })
        .collect()
}

fn status_for(layer: CapabilityLayer, row: &CapabilityRow) -> LayerStatus {
    match layer {
        "parser" => row.parser,
        "semantic" => row.semantic,
        "svg" => row.svg,
        "edit" => row.edit,
        _ => LayerStatus::None,
    }
}

/// Feature IDs supported by the parser layer.
pub fn parser_feature_registry() -> Vec<&'static str> {
    features_supported_by("parser")
}

/// Feature IDs supported by the semantic layer.
pub fn semantic_feature_registry() -> Vec<&'static str> {
    features_supported_by("semantic")
}

/// Feature IDs supported by the SVG layer.
pub fn svg_feature_registry() -> Vec<&'static str> {
    features_supported_by("svg")
}

/// Feature IDs supported by the edit layer.
pub fn edit_feature_registry() -> Vec<&'static str> {
    features_supported_by("edit")
}

/// All layer registries, keyed by layer name.
pub fn all_registries() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("parser", parser_feature_registry()),
        ("semantic", semantic_feature_registry()),
        ("svg", svg_feature_registry()),
        ("edit", edit_feature_registry()),
    ]
}

/// Sanity check: every ID in FEATURE_IDS must be a valid layer status key.
#[cfg(test)]
mod drift_tests {
    use super::*;

    #[test]
    fn every_feature_id_has_a_matrix_row() {
        let matrix: std::collections::HashSet<&str> =
            capability_matrix().iter().map(|(id, _)| *id).collect();
        for id in FEATURE_IDS {
            assert!(
                matrix.contains(id),
                "feature id {id} missing from capability matrix"
            );
        }
        // No extra rows beyond FEATURE_IDS.
        assert_eq!(
            matrix.len(),
            FEATURE_IDS.len(),
            "matrix has rows not in FEATURE_IDS"
        );
    }

    #[test]
    fn every_row_has_a_layer_status() {
        for (_id, row) in capability_matrix() {
            // All four layers must be set (non-default construction).
            let _ = row.parser;
            let _ = row.semantic;
            let _ = row.svg;
            let _ = row.edit;
        }
    }

    #[test]
    fn registries_align_with_matrix() {
        let matrix: std::collections::HashMap<&str, CapabilityRow> =
            capability_matrix().into_iter().collect();
        for (layer, registry) in all_registries() {
            let registry_set: std::collections::HashSet<&str> =
                registry.iter().copied().collect();
            for id in FEATURE_IDS {
                let row = matrix.get(id).unwrap();
                let supported = status_for(layer, row).is_supported();
                let in_registry = registry_set.contains(id);
                assert_eq!(
                    supported,
                    in_registry,
                    "{id} has {layer}={:?} but registry membership={in_registry}",
                    status_for(layer, row)
                );
            }
            // Registry must not contain unknown IDs.
            for id in &registry_set {
                assert!(
                    matrix.contains_key(id),
                    "unknown feature id {id} in {layer} registry"
                );
            }
        }
    }
}
