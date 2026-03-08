//! Stage 6: URL cleaning.
//!
//! Scans text for URLs, strips tracking parameters, normalizes hosts.
//! See §5.4 Stage 6 of the technical spec.

use regex::Regex;
use std::sync::LazyLock;

use super::{ContentType, RuleFired, Transform, TransformContext};

/// URL detection regex.
static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s<>"')\]]+"#).unwrap());

/// Default deny list of tracking parameters.
pub(crate) const DEFAULT_DENY_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "gclid",
    "fbclid",
    "mc_cid",
    "mc_eid",
    "si",
    "ref_src",
    "_ga",
    "_gl",
];

/// Glob-style deny patterns (matches prefix).
const DEFAULT_DENY_PREFIXES: &[&str] = &["utm_"];

use crate::config::DomainOverride;
use std::collections::HashMap;

/// URL cleaning transform (Stage 6).
pub struct UrlTransform {
    strip_www: bool,
    strip_scheme: bool,
    deny_params: Vec<String>,
    deny_prefixes: Vec<String>,
    domain_overrides: HashMap<String, DomainOverride>,
}

impl UrlTransform {
    pub fn new() -> Self {
        UrlTransform {
            strip_www: true,
            strip_scheme: false,
            deny_params: DEFAULT_DENY_PARAMS.iter().map(|s| s.to_string()).collect(),
            deny_prefixes: DEFAULT_DENY_PREFIXES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            domain_overrides: HashMap::new(),
        }
    }

    /// Set whether to strip `www.` from hosts.
    pub fn with_strip_www(mut self, strip: bool) -> Self {
        self.strip_www = strip;
        self
    }

    /// Set whether to strip the scheme (`https://`).
    pub fn with_strip_scheme(mut self, strip: bool) -> Self {
        self.strip_scheme = strip;
        self
    }

    /// Set per-domain parameter overrides.
    pub fn with_domain_overrides(mut self, overrides: HashMap<String, DomainOverride>) -> Self {
        self.domain_overrides = overrides;
        self
    }

    /// Append extra deny parameters (from config `url_global_deny`).
    pub fn with_extra_deny_params(mut self, extra: Vec<String>) -> Self {
        self.deny_params.extend(extra);
        self
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

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        // Skip URL cleaning for shell blocks — URLs are command arguments
        if ctx.content_type == Some(ContentType::ShellBlock) {
            return input.to_string();
        }

        let mut total_changed = 0usize;
        let mut in_fence = false;
        let lines: Vec<&str> = input.split('\n').collect();
        let mut processed_lines: Vec<String> = Vec::new();

        for line in &lines {
            if line.trim().starts_with("```") {
                in_fence = !in_fence;
                processed_lines.push(line.to_string());
                continue;
            }
            if in_fence {
                processed_lines.push(line.to_string());
                continue;
            }
            processed_lines.push(self.process_line_urls(line, &mut total_changed));
        }

        let result = processed_lines.join("\n");

        if total_changed > 0 {
            ctx.rules_fired.push(RuleFired {
                stage: "url_cleaning",
                description: format!("cleaned {} bytes from URLs", total_changed),
                chars_changed: total_changed,
            });
        }

        result
    }
}

impl UrlTransform {
    /// Process a single line, splitting by backticks to skip URLs in code spans.
    fn process_line_urls(&self, line: &str, total_changed: &mut usize) -> String {
        let backtick_parts: Vec<&str> = line.split('`').collect();
        if backtick_parts.len() <= 1 {
            return self.clean_urls_in_text(line, total_changed);
        }
        let mut rebuilt = String::new();
        for (i, part) in backtick_parts.iter().enumerate() {
            if i % 2 == 0 {
                rebuilt.push_str(&self.clean_urls_in_text(part, total_changed));
            } else {
                rebuilt.push_str(part);
            }
            if i < backtick_parts.len() - 1 {
                rebuilt.push('`');
            }
        }
        rebuilt
    }

    /// Clean URLs found in a text segment.
    fn clean_urls_in_text(&self, text: &str, total_changed: &mut usize) -> String {
        let mut result = text.to_string();

        // Process URLs in reverse order to preserve offsets
        let matches: Vec<(usize, usize, String)> = URL_RE
            .find_iter(text)
            .map(|m| (m.start(), m.end(), m.as_str().to_string()))
            .collect();

        for (start, end, url_str) in matches.into_iter().rev() {
            if let Some(cleaned) = self.clean_url(&url_str) {
                if cleaned != url_str {
                    *total_changed += url_str.len().saturating_sub(cleaned.len());
                    result.replace_range(start..end, &cleaned);
                }
            }
        }

        result
    }

    /// Clean a single URL: strip tracking params, normalize host.
    fn clean_url(&self, url_str: &str) -> Option<String> {
        let parsed = url::Url::parse(url_str).ok()?;

        let mut new_url = parsed.clone();
        self.strip_www_from_url(&mut new_url);
        let domain = new_url.host_str().map(|h| h.to_string());
        self.filter_query_params(&mut new_url, domain.as_deref());

        let mut result = new_url.to_string();
        if self.strip_scheme {
            result = strip_scheme(&result);
        }
        if result.ends_with('/') && !url_str.ends_with('/') {
            result.pop();
        }

        Some(result)
    }

    fn strip_www_from_url(&self, new_url: &mut url::Url) {
        if !self.strip_www {
            return;
        }
        if let Some(host) = new_url.host_str().map(|h| h.to_string()) {
            if let Some(stripped) = host.strip_prefix("www.") {
                let _ = new_url.set_host(Some(stripped));
            }
        }
    }

    fn filter_query_params(&self, url: &mut url::Url, domain: Option<&str>) {
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(key, _)| !self.should_strip_param(key, domain))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        if pairs.is_empty() {
            url.set_query(None);
        } else {
            let query = pairs
                .iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        k.clone()
                    } else {
                        format!("{}={}", k, v)
                    }
                })
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&query));
        }
    }

    /// Check if a parameter should be stripped, considering per-domain overrides.
    fn should_strip_param(&self, key: &str, domain: Option<&str>) -> bool {
        // Check per-domain overrides first
        if let Some(d) = domain {
            if let Some(overrides) = self.domain_overrides.get(d) {
                // Keep list takes priority — if param is in keep, never strip it
                if overrides.keep.iter().any(|k| k == key) {
                    return false;
                }
                // Domain-specific deny list
                if overrides.deny.iter().any(|dk| dk == key) {
                    return true;
                }
            }
        }

        // Global exact match
        if self.deny_params.iter().any(|p| p == key) {
            return true;
        }
        // Prefix match (glob patterns)
        if self
            .deny_prefixes
            .iter()
            .any(|prefix| key.starts_with(prefix.as_str()))
        {
            return true;
        }
        false
    }
}

fn strip_scheme(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("https://") {
        rest.to_string()
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest.to_string()
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utm_stripping() {
        let input =
            "Visit https://example.com/page?utm_source=google&utm_medium=cpc&id=123 for more.";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.contains("id=123"));
        assert!(!result.contains("utm_source"));
        assert!(!result.contains("utm_medium"));
    }

    #[test]
    fn test_www_stripping() {
        let input = "Check https://www.example.com/path";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.contains("https://example.com/path"));
        assert!(!result.contains("www."));
    }

    #[test]
    fn test_url_in_code_fence_skipped() {
        let input = "```\nhttps://example.com?utm_source=test\n```";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.contains("utm_source=test"));
    }

    #[test]
    fn test_fbclid_stripped() {
        let input = "https://example.com?fbclid=abc123&page=1";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(!result.contains("fbclid"));
        assert!(result.contains("page=1"));
    }

    #[test]
    fn test_all_params_stripped_cleans_query() {
        let input = "https://example.com?utm_source=google&utm_medium=cpc";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(!result.contains('?'));
    }

    #[test]
    fn test_scheme_stripping_opt_in() {
        let input = "https://example.com/path";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new().with_strip_scheme(true);
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "example.com/path");
    }

    #[test]
    fn test_no_url_passthrough() {
        let input = "Just regular text with no URLs.";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input);
        assert!(ctx.rules_fired.is_empty());
    }

    #[test]
    fn test_url_in_backticks_skipped() {
        let input = "See `https://example.com?utm_source=test` for details.";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.contains("utm_source=test"));
    }

    #[test]
    fn test_idempotency() {
        let input = "https://example.com/page?id=1&utm_source=google";
        let mut ctx = TransformContext::default();
        let transform = UrlTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(first, second, "URL transform must be idempotent");
    }

    #[test]
    fn test_domain_override_keep() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "example.com".to_string(),
            DomainOverride {
                keep: vec!["utm_source".to_string()],
                deny: vec![],
            },
        );
        let transform = UrlTransform::new().with_domain_overrides(overrides);
        let input = "https://example.com/page?utm_source=google&fbclid=abc";
        let mut ctx = TransformContext::default();
        let result = transform.apply(input, &mut ctx);
        assert!(
            result.contains("utm_source=google"),
            "keep list should preserve utm_source"
        );
        assert!(
            !result.contains("fbclid"),
            "fbclid should still be stripped"
        );
    }

    #[test]
    fn test_domain_override_deny() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "example.com".to_string(),
            DomainOverride {
                keep: vec![],
                deny: vec!["custom_param".to_string()],
            },
        );
        let transform = UrlTransform::new().with_domain_overrides(overrides);
        let input = "https://example.com/page?custom_param=value&id=1";
        let mut ctx = TransformContext::default();
        let result = transform.apply(input, &mut ctx);
        assert!(
            !result.contains("custom_param"),
            "domain-specific deny should strip"
        );
        assert!(result.contains("id=1"), "non-denied params preserved");
    }

    #[test]
    fn test_skip_for_shell_block() {
        let input = "curl https://example.com?utm_source=google";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::ShellBlock),
            ..Default::default()
        };
        let transform = UrlTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input, "ShellBlock should skip URL cleaning");
    }

    #[test]
    fn test_domain_override_no_match() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "other.com".to_string(),
            DomainOverride {
                keep: vec!["utm_source".to_string()],
                deny: vec![],
            },
        );
        let transform = UrlTransform::new().with_domain_overrides(overrides);
        let input = "https://example.com/page?utm_source=google";
        let mut ctx = TransformContext::default();
        let result = transform.apply(input, &mut ctx);
        assert!(
            !result.contains("utm_source"),
            "override for other.com shouldn't affect example.com"
        );
    }
}
