//! Stage 6: URL cleaning.
//!
//! Scans text for URLs, strips tracking parameters, normalizes hosts.
//! See §5.4 Stage 6 of the technical spec.

use super::{Transform, TransformContext};

/// URL cleaning transform (Stage 6).
pub struct UrlTransform {
    // TODO: compiled URL regex, deny list, domain overrides
}

impl UrlTransform {
    pub fn new() -> Self {
        UrlTransform {}
    }
}

impl Default for UrlTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for UrlTransform {
    fn name(&self) -> &'static str {
        "url_cleaning"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 6
        input.to_string()
    }
}
