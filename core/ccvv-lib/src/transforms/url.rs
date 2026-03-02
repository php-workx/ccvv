//! Stage 6: URL cleaning.
//!
//! Scans text for URLs, strips tracking parameters, normalizes hosts.
//! See §5.4 Stage 6 of the technical spec.

use regex::Regex;
use std::sync::LazyLock;

use super::{RuleFired, Transform, TransformContext};

/// URL detection regex.
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https?://[^\s<>"')\]]+"#).unwrap());

/// Default deny list of tracking parameters.
const DEFAULT_DENY_PARAMS: &[&str] = &[
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

/// URL cleaning transform (Stage 6).
pub struct UrlTransform {
    strip_www: bool,
    strip_scheme: bool,
    deny_params: Vec<String>,
    deny_prefixes: Vec<String>,
}

impl UrlTransform {
    pub fn new() -> Self {
        UrlTransform {
            strip_www: true,
            strip_scheme: false,
            deny_params: DEFAULT_DENY_PARAMS.iter().map(|s| s.to_string()).collect(),
            deny_prefixes: DEFAULT_DENY_PREFIXES.iter().map(|s| s.to_string()).collect(),
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
        let mut total_changed = 0usize;

        // Track code fence state to skip URLs inside fences
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

            // Also skip URLs inside backtick spans
            let backtick_parts: Vec<&str> = line.split('`').collect();
            let processed_line = if backtick_parts.len() > 1 {
                // Rebuild line processing only even-indexed parts (outside backticks)
                let mut rebuilt = String::new();
                for (i, part) in backtick_parts.iter().enumerate() {
                    if i % 2 == 0 {
                        rebuilt.push_str(&self.clean_urls_in_text(part, &mut total_changed));
                    } else {
                        rebuilt.push_str(part);
                    }
                    if i < backtick_parts.len() - 1 {
                        rebuilt.push('`');
                    }
                }
                rebuilt
            } else {
                self.clean_urls_in_text(line, &mut total_changed)
            };

            processed_lines.push(processed_line);
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

        // Strip www. from host
        if self.strip_www {
            if let Some(host) = parsed.host_str() {
                if let Some(stripped) = host.strip_prefix("www.") {
                    if let Ok(mut u) = url::Url::parse(url_str) {
                        let _ = u.set_host(Some(stripped));
                        new_url = u;
                    }
                }
            }
        }

        // Filter query parameters
        let pairs: Vec<(String, String)> = new_url
            .query_pairs()
            .filter(|(key, _)| !self.should_strip_param(key))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        if pairs.is_empty() {
            new_url.set_query(None);
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
            new_url.set_query(Some(&query));
        }

        let mut result = new_url.to_string();

        // Strip scheme if configured
        if self.strip_scheme {
            if let Some(rest) = result.strip_prefix("https://") {
                result = rest.to_string();
            } else if let Some(rest) = result.strip_prefix("http://") {
                result = rest.to_string();
            }
        }

        // Remove trailing slash added by url crate on bare domains
        if result.ends_with('/') && !url_str.ends_with('/') {
            result.pop();
        }

        Some(result)
    }

    /// Check if a parameter should be stripped.
    fn should_strip_param(&self, key: &str) -> bool {
        // Exact match
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utm_stripping() {
        let input = "Visit https://example.com/page?utm_source=google&utm_medium=cpc&id=123 for more.";
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
        let input =
            "```\nhttps://example.com?utm_source=test\n```";
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
}
