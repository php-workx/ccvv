//! Transform pipeline types and trait definition.
//!
//! Every pipeline stage implements the [`Transform`] trait. Stages are
//! ordered canonically (see §5.2 of the technical spec) and cannot be
//! reordered by configuration — only toggled on/off.

pub mod agent;
pub mod autowrap;
pub mod normalize;
pub mod structural;
pub mod url;
pub mod userrules;
pub mod whitespace;

/// Metadata that flows through the pipeline alongside the text.
/// Stages read and annotate this to communicate downstream.
#[derive(Debug, Clone, Default)]
pub struct TransformContext {
    pub content_type: Option<ContentType>,
    pub rules_fired: Vec<RuleFired>,
    pub profile: Option<String>,
    pub input_size_bytes: usize,
    pub skipped_sensitive: bool,
    pub skipped_oversize: bool,
}

/// Record of a single rule or heuristic that fired during transformation.
#[derive(Debug, Clone)]
pub struct RuleFired {
    pub stage: &'static str,
    pub description: String,
    pub chars_changed: usize,
}

/// Classification of clipboard content, used by stages to adjust behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Url,
    Code,
    Prose,
    Table,
    Json,
    Mixed,
}

/// Every pipeline stage implements this trait.
///
/// # Idempotency Contract
///
/// `apply(apply(input, ctx), ctx) == apply(input, ctx)` must hold
/// for all inputs and all configurations. See §5.7 of the technical spec.
pub trait Transform: Send + Sync {
    /// Human-readable name used for config toggles and diagnostics.
    fn name(&self) -> &'static str;

    /// Apply the transformation. Returns modified text.
    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String;
}
