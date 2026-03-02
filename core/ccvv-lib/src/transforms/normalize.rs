//! Stage 2: Unicode & encoding normalization.
//!
//! Character-by-character scan with lookup tables. No regex.
//! See §5.4 Stage 2 of the technical spec.

use super::{Transform, TransformContext};

/// Unicode normalization transform (Stage 2).
pub struct NormalizeTransform {
    // TODO: configuration for em-dash replacement
}

impl NormalizeTransform {
    pub fn new() -> Self {
        NormalizeTransform {}
    }
}

impl Default for NormalizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for NormalizeTransform {
    fn name(&self) -> &'static str {
        "normalize_unicode"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 5
        input.to_string()
    }
}
