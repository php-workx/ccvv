//! Stage 7: Auto-wrapper (backtick wrapping).
//!
//! Port of the Swift `addInlineCodeMarkersToPlainText` pipeline.
//! **Disabled by default** due to shell command substitution risk.
//! See §5.4 Stage 7 of the technical spec.

use super::{Transform, TransformContext};

/// Auto-wrapper transform (Stage 7). Disabled by default.
pub struct AutowrapTransform;

impl AutowrapTransform {
    pub fn new() -> Self {
        AutowrapTransform
    }
}

impl Default for AutowrapTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for AutowrapTransform {
    fn name(&self) -> &'static str {
        "auto_wrapper"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 4
        input.to_string()
    }
}
