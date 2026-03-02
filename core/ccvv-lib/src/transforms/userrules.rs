//! Stage 8: User-defined regex rules.
//!
//! Applies compiled user rules in declaration order with per-rule timeout.
//! See §5.4 Stage 8 of the technical spec.

use super::{Transform, TransformContext};

/// A compiled user-defined regex rule.
#[derive(Debug, Clone)]
pub struct CompiledUserRule {
    pub name: String,
    pub regex: regex::Regex,
    pub replacement: String,
}

/// User rules transform (Stage 8).
pub struct UserRulesTransform {
    // TODO: Vec<CompiledUserRule> from config
}

impl UserRulesTransform {
    pub fn new() -> Self {
        UserRulesTransform {}
    }
}

impl Default for UserRulesTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for UserRulesTransform {
    fn name(&self) -> &'static str {
        "user_rules"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 8
        input.to_string()
    }
}
