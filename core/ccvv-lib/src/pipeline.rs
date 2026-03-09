//! Pipeline orchestrator.
//!
//! Runs transform stages in canonical order with size guards,
//! sensitive content filtering, and inter-stage expansion checks.
//! See §5.2 of the technical spec.

use crate::classify::classify;
use crate::config::ResolvedConfig;
use crate::secrets::SecretFilter;
use crate::transforms::agent::AgentTransform;
use crate::transforms::autowrap::AutowrapTransform;
use crate::transforms::normalize::NormalizeTransform;
use crate::transforms::structural::StructuralTransform;
use crate::transforms::url::UrlTransform;
use crate::transforms::userrules::UserRulesTransform;
use crate::transforms::whitespace::WhitespaceTransform;
use crate::transforms::{Transform, TransformContext};

/// Default maximum input size: 1 MB.
const DEFAULT_MAX_INPUT_BYTES: usize = 1_048_576;

/// Default maximum output expansion ratio.
const DEFAULT_MAX_OUTPUT_RATIO: f64 = 2.0;

/// The transform pipeline. Stages are executed in registration order
/// (which must be canonical order per §5.2).
pub struct Pipeline {
    stages: Vec<Box<dyn Transform>>,
    max_input_bytes: usize,
    max_output_ratio: f64,
    sensitive_filter: SecretFilter,
    sensitive_filter_enabled: bool,
}

impl Pipeline {
    /// Create a new pipeline with the given stages and default limits.
    pub fn new(stages: Vec<Box<dyn Transform>>) -> Self {
        Pipeline {
            stages,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_output_ratio: DEFAULT_MAX_OUTPUT_RATIO,
            sensitive_filter: SecretFilter::new(),
            sensitive_filter_enabled: true,
        }
    }

    /// Build the canonical full pipeline from resolved config.
    pub fn from_resolved_config(config: &ResolvedConfig) -> Self {
        let mut stages: Vec<Box<dyn Transform>> = Vec::new();

        if config.settings.normalize_unicode {
            stages.push(Box::new(
                NormalizeTransform::new().with_em_dash_replacement(&config.em_dash_replacement),
            ));
        }
        if config.settings.whitespace_cleanup {
            stages.push(Box::new(WhitespaceTransform::new()));
        }
        if config.settings.agent_strip {
            stages.push(Box::new(AgentTransform::new()));
        }
        if config.settings.structural_detection {
            stages.push(Box::new(StructuralTransform::new()));
        }
        if config.settings.url_cleaning {
            stages.push(Box::new(
                UrlTransform::new()
                    .with_strip_scheme(config.settings.url_strip_scheme)
                    .with_domain_overrides(config.url_domain_overrides.clone())
                    .with_extra_deny_params(config.url_global_deny.clone()),
            ));
        }
        if config.settings.auto_wrapper {
            stages.push(Box::new(AutowrapTransform::new()));
        }
        if config.settings.user_rules && !config.compiled_rules.is_empty() {
            stages.push(Box::new(UserRulesTransform::with_rules(
                config.compiled_rules.clone(),
            )));
        }

        Pipeline::new(stages)
            .with_max_input_bytes(config.settings.max_input_bytes)
            .with_sensitive_filter(config.settings.sensitive_filter)
    }

    /// Set the maximum input size in bytes.
    pub fn with_max_input_bytes(mut self, max: usize) -> Self {
        self.max_input_bytes = max;
        self
    }

    /// Set the maximum output expansion ratio.
    pub fn with_max_output_ratio(mut self, ratio: f64) -> Self {
        self.max_output_ratio = ratio;
        self
    }

    /// Enable or disable the sensitive content filter.
    pub fn with_sensitive_filter(mut self, enabled: bool) -> Self {
        self.sensitive_filter_enabled = enabled;
        self
    }

    /// Run all stages. Returns cleaned text and context.
    ///
    /// If input exceeds `max_input_bytes`, returns input unchanged
    /// with `ctx.skipped_oversize = true`.
    ///
    /// If input matches sensitive patterns and filter is enabled,
    /// returns input unchanged with `ctx.skipped_sensitive = true`.
    pub fn run(&self, input: &str) -> (String, TransformContext) {
        let mut ctx = TransformContext {
            input_size_bytes: input.len(),
            ..Default::default()
        };

        // Size check
        if input.len() > self.max_input_bytes {
            ctx.skipped_oversize = true;
            return (input.to_string(), ctx);
        }

        // Sensitive content check
        if self.sensitive_filter_enabled && self.sensitive_filter.check(input) {
            ctx.skipped_sensitive = true;
            return (input.to_string(), ctx);
        }

        // Pre-scan: classify content type before stages run
        ctx.content_type = Some(classify(input));

        let original_len = input.len();
        let mut text = input.to_string();

        for stage in &self.stages {
            text = stage.apply(&text, &mut ctx);

            // Cumulative expansion check against original input size
            if original_len > 0 {
                let ratio = text.len() as f64 / original_len as f64;
                if ratio > self.max_output_ratio {
                    ctx.rules_fired.clear();
                    return (input.to_string(), ctx);
                }
            }
        }

        (text, ctx)
    }

    /// Run only named stages. Used by CLI `--strip-urls`, `--unwrap`, etc.
    pub fn run_selective(&self, input: &str, stage_names: &[&str]) -> (String, TransformContext) {
        let mut ctx = TransformContext {
            input_size_bytes: input.len(),
            ..Default::default()
        };

        // Size check
        if input.len() > self.max_input_bytes {
            ctx.skipped_oversize = true;
            return (input.to_string(), ctx);
        }

        // Sensitive content check
        if self.sensitive_filter_enabled && self.sensitive_filter.check(input) {
            ctx.skipped_sensitive = true;
            return (input.to_string(), ctx);
        }

        // Pre-scan: classify content type before stages run
        ctx.content_type = Some(classify(input));

        let original_len = input.len();
        let mut text = input.to_string();

        for stage in &self.stages {
            if stage_names.contains(&stage.name()) {
                text = stage.apply(&text, &mut ctx);

                // Cumulative expansion check against original input size
                if original_len > 0 {
                    let ratio = text.len() as f64 / original_len as f64;
                    if ratio > self.max_output_ratio {
                        ctx.rules_fired.clear();
                        return (input.to_string(), ctx);
                    }
                }
            }
        }

        (text, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A no-op transform for testing.
    struct NoopTransform;

    impl Transform for NoopTransform {
        fn name(&self) -> &'static str {
            "noop"
        }

        fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
            input.to_string()
        }
    }

    /// A transform that doubles the input (for testing expansion guard).
    struct DoublerTransform;

    impl Transform for DoublerTransform {
        fn name(&self) -> &'static str {
            "doubler"
        }

        fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
            format!("{}{}", input, input)
        }
    }

    #[test]
    fn test_empty_pipeline() {
        let pipeline = Pipeline::new(vec![]);
        let (result, ctx) = pipeline.run("hello");
        assert_eq!(result, "hello");
        assert!(!ctx.skipped_oversize);
        assert!(!ctx.skipped_sensitive);
    }

    #[test]
    fn test_noop_pipeline() {
        let pipeline = Pipeline::new(vec![Box::new(NoopTransform)]);
        let (result, _ctx) = pipeline.run("hello world");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_oversize_input() {
        let pipeline = Pipeline::new(vec![Box::new(NoopTransform)]).with_max_input_bytes(10);
        let (result, ctx) = pipeline.run("this is longer than 10 bytes");
        assert_eq!(result, "this is longer than 10 bytes");
        assert!(ctx.skipped_oversize);
    }

    #[test]
    fn test_sensitive_content_skipped() {
        let pipeline = Pipeline::new(vec![Box::new(NoopTransform)]);
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
        let (result, ctx) = pipeline.run(input);
        assert_eq!(result, input);
        assert!(ctx.skipped_sensitive);
    }

    #[test]
    fn test_sensitive_filter_disabled() {
        let pipeline = Pipeline::new(vec![Box::new(NoopTransform)]).with_sensitive_filter(false);
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
        let (_result, ctx) = pipeline.run(input);
        assert!(!ctx.skipped_sensitive);
    }

    #[test]
    fn test_expansion_guard() {
        let pipeline = Pipeline::new(vec![Box::new(DoublerTransform)]);
        let input = "hello";
        let (result, _ctx) = pipeline.run(input);
        // Doubler produces "hellohello" (10 chars) from "hello" (5 chars) = 2.0x
        // 2.0 is exactly at the limit, so it should pass
        assert_eq!(result, "hellohello");
    }

    #[test]
    fn test_expansion_guard_trips() {
        // Use a ratio of 1.5 so the doubler (2.0x) trips it
        let pipeline = Pipeline::new(vec![Box::new(DoublerTransform)]).with_max_output_ratio(1.5);
        let input = "hello";
        let (result, _ctx) = pipeline.run(input);
        // Should return original since expansion exceeded 1.5x
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_selective_run() {
        let pipeline = Pipeline::new(vec![Box::new(NoopTransform)]);
        let (result, _ctx) = pipeline.run_selective("hello", &["noop"]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_selective_run_skips_unselected() {
        let pipeline = Pipeline::new(vec![Box::new(DoublerTransform)]);
        // Request a stage that doesn't exist — doubler should be skipped
        let (result, _ctx) = pipeline.run_selective("hello", &["nonexistent"]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_cumulative_expansion_guard() {
        // Two doublers: stage1 produces 2x, stage2 produces 4x from original.
        // With max_output_ratio=3.0, the first doubler passes (2x <= 3.0)
        // but the second should trip it (4x > 3.0).
        let pipeline = Pipeline::new(vec![Box::new(DoublerTransform), Box::new(DoublerTransform)])
            .with_max_output_ratio(3.0);
        let input = "hello";
        let (result, _ctx) = pipeline.run(input);
        // Should return original since cumulative expansion (4x) exceeds 3.0x
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_input_size_tracked() {
        let pipeline = Pipeline::new(vec![]);
        let (_result, ctx) = pipeline.run("hello");
        assert_eq!(ctx.input_size_bytes, 5);
    }

    #[test]
    fn test_from_resolved_config_respects_disabled_stages() {
        let mut config = ResolvedConfig::default();
        config.settings.normalize_unicode = false;
        config.settings.whitespace_cleanup = false;

        let pipeline = Pipeline::from_resolved_config(&config);
        let (result, _ctx) = pipeline.run("hello");

        assert_eq!(result, "hello");
    }
}
