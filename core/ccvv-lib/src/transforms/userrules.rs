//! Stage 8: User-defined regex rules.
//!
//! Applies compiled user rules in declaration order with per-rule timeout.
//! See §5.4 Stage 8 of the technical spec.

use std::borrow::Cow;

use regex::Regex;
use std::time::{Duration, Instant};

use super::{RuleFired, Transform, TransformContext};

/// Maximum number of user rules allowed.
pub const MAX_USER_RULES: usize = 50;

/// Per-rule execution timeout.
const RULE_TIMEOUT: Duration = Duration::from_millis(50);

/// A compiled user-defined regex rule.
#[derive(Debug, Clone)]
pub struct CompiledUserRule {
    pub name: String,
    pub regex: Regex,
    pub replacement: String,
}

/// User rules transform (Stage 8).
pub struct UserRulesTransform {
    rules: Vec<CompiledUserRule>,
}

impl UserRulesTransform {
    /// Create a new user rules transform with no rules.
    pub fn new() -> Self {
        UserRulesTransform { rules: Vec::new() }
    }

    /// Create with a set of compiled rules.
    pub fn with_rules(rules: Vec<CompiledUserRule>) -> Self {
        UserRulesTransform {
            rules: rules.into_iter().take(MAX_USER_RULES).collect(),
        }
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

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        let mut text = input.to_string();

        for rule in &self.rules {
            let start = Instant::now();
            let result = rule.regex.replace_all(&text, rule.replacement.as_str());

            if start.elapsed() > RULE_TIMEOUT {
                ctx.rules_fired.push(RuleFired {
                    stage: "user_rules",
                    description: format!("rule '{}' timed out after {:?}", rule.name, RULE_TIMEOUT),
                    chars_changed: 0,
                });
                continue;
            }

            match result {
                Cow::Borrowed(_) => {}
                Cow::Owned(new_text) => {
                    let chars_changed = text.len().abs_diff(new_text.len());
                    ctx.rules_fired.push(RuleFired {
                        stage: "user_rules",
                        description: format!("rule '{}' fired", rule.name),
                        chars_changed,
                    });
                    text = new_text;
                }
            }
        }

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_group_replacement() {
        let rule = CompiledUserRule {
            name: "swap_words".to_string(),
            regex: Regex::new(r"(\w+)\s+(\w+)").unwrap(),
            replacement: "$2 $1".to_string(),
        };
        let transform = UserRulesTransform::with_rules(vec![rule]);
        let mut ctx = TransformContext::default();
        let result = transform.apply("hello world", &mut ctx);
        assert_eq!(result, "world hello");
        assert!(!ctx.rules_fired.is_empty());
    }

    #[test]
    fn test_no_rules_passthrough() {
        let transform = UserRulesTransform::new();
        let mut ctx = TransformContext::default();
        let result = transform.apply("hello world", &mut ctx);
        assert_eq!(result, "hello world");
        assert!(ctx.rules_fired.is_empty());
    }

    #[test]
    fn test_multiple_rules_in_order() {
        let rules = vec![
            CompiledUserRule {
                name: "remove_foo".to_string(),
                regex: Regex::new(r"foo").unwrap(),
                replacement: "bar".to_string(),
            },
            CompiledUserRule {
                name: "remove_bar".to_string(),
                regex: Regex::new(r"bar").unwrap(),
                replacement: "baz".to_string(),
            },
        ];
        let transform = UserRulesTransform::with_rules(rules);
        let mut ctx = TransformContext::default();
        let result = transform.apply("foo test", &mut ctx);
        // foo → bar → baz
        assert_eq!(result, "baz test");
        assert_eq!(ctx.rules_fired.len(), 2);
    }

    #[test]
    fn test_max_rules_enforced() {
        let rules: Vec<CompiledUserRule> = (0..60)
            .map(|i| CompiledUserRule {
                name: format!("rule_{}", i),
                regex: Regex::new(&format!("pattern_{}", i)).unwrap(),
                replacement: format!("replacement_{}", i),
            })
            .collect();
        let transform = UserRulesTransform::with_rules(rules);
        assert_eq!(transform.rules.len(), MAX_USER_RULES);
    }

    #[test]
    fn test_non_matching_rule_no_fire() {
        let rule = CompiledUserRule {
            name: "no_match".to_string(),
            regex: Regex::new(r"xyz").unwrap(),
            replacement: "abc".to_string(),
        };
        let transform = UserRulesTransform::with_rules(vec![rule]);
        let mut ctx = TransformContext::default();
        let result = transform.apply("hello world", &mut ctx);
        assert_eq!(result, "hello world");
        assert!(ctx.rules_fired.is_empty());
    }
}
