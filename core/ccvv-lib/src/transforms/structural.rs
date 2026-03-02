//! Stage 5: Structural detection (JSON, table, code fence).
//!
//! Three sub-detectors tried in order: JSON, table, code fence.
//! See §5.4 Stage 5 of the technical spec.

use super::{Transform, TransformContext};

/// Structural detection transform (Stage 5).
pub struct StructuralTransform;

impl StructuralTransform {
    pub fn new() -> Self {
        StructuralTransform
    }
}

impl Default for StructuralTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for StructuralTransform {
    fn name(&self) -> &'static str {
        "structural_detection"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 7
        input.to_string()
    }
}
