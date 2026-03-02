//! Stage 4: Agent artifact stripping.
//!
//! Removes ANSI escape sequences, the `\u{23FA}` recording dot at line start,
//! and zero-width characters. See §5.4 Stage 4 of the technical spec.

use regex::Regex;
use std::sync::LazyLock;

use super::{Transform, TransformContext, RuleFired};

/// ANSI escape sequence regex, compiled once.
static ANSI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1B\[[0-9;]*[A-Za-z]").unwrap());

/// Zero-width characters to remove.
const ZERO_WIDTH_CHARS: &[char] = &[
    '\u{200B}', // Zero-width space
    '\u{200C}', // Zero-width non-joiner
    '\u{200D}', // Zero-width joiner
    '\u{FEFF}', // Byte-order mark
];

/// Agent artifact stripping transform (Stage 4).
pub struct AgentTransform;

impl AgentTransform {
    pub fn new() -> Self {
        AgentTransform
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

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        let original_len = input.len();
        let mut result = input.to_string();

        // Strip ANSI escape sequences
        let after_ansi = ANSI_RE.replace_all(&result, "").to_string();
        if after_ansi.len() != result.len() {
            result = after_ansi;
        }

        // Strip recording dot (⏺ U+23FA) at line starts
        let lines: Vec<&str> = result.split('\n').collect();
        let processed: Vec<String> = lines
            .iter()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with('\u{23FA}') {
                    let after_dot = trimmed.trim_start_matches('\u{23FA}');
                    let ws = &line[..line.len() - trimmed.len()];
                    format!("{}{}", ws, after_dot.trim_start())
                } else {
                    line.to_string()
                }
            })
            .collect();
        result = processed.join("\n");

        // Remove zero-width characters
        result = result.replace(ZERO_WIDTH_CHARS, "");

        let chars_changed = if result.len() != original_len {
            original_len.saturating_sub(result.len())
        } else {
            0
        };

        if chars_changed > 0 {
            ctx.rules_fired.push(RuleFired {
                stage: "agent_strip",
                description: format!("removed {} bytes of agent artifacts", chars_changed),
                chars_changed,
            });
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ansi_stripping() {
        let input = "\x1B[31mRed text\x1B[0m and \x1B[1;32mbold green\x1B[0m";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Red text and bold green");
        assert!(!ctx.rules_fired.is_empty());
    }

    #[test]
    fn test_recording_dot_removal() {
        let input = "\u{23FA} Some text\n\u{23FA}  More text";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Some text\nMore text");
    }

    #[test]
    fn test_zero_width_removal() {
        let input = "Hello\u{200B}World\u{FEFF}Test\u{200C}End\u{200D}!";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "HelloWorldTestEnd!");
    }

    #[test]
    fn test_no_artifacts_passthrough() {
        let input = "Normal text without any artifacts.";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Normal text without any artifacts.");
        assert!(ctx.rules_fired.is_empty());
    }

    #[test]
    fn test_cursor_sequences_removed() {
        let input = "\x1B[2J\x1B[HWelcome\x1B[K";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Welcome");
    }

    #[test]
    fn test_idempotency() {
        let input = "\x1B[31m\u{23FA} Hello\u{200B}\x1B[0m";
        let mut ctx = TransformContext::default();
        let transform = AgentTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(first, second, "Agent transform must be idempotent");
    }
}
