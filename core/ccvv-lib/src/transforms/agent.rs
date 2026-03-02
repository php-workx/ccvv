//! Stage 4: Agent artifact stripping.
//!
//! Removes ANSI escape sequences, the `\u{23FA}` recording dot at line start,
//! and zero-width characters. See §5.4 Stage 4 of the technical spec.

use super::{Transform, TransformContext};

/// Agent artifact stripping transform (Stage 4).
pub struct AgentTransform {
    // TODO: compiled ANSI regex
}

impl AgentTransform {
    pub fn new() -> Self {
        AgentTransform {}
    }
}

impl Default for AgentTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for AgentTransform {
    fn name(&self) -> &'static str {
        "agent_strip"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        // TODO: implement in Issue 3
        input.to_string()
    }
}
