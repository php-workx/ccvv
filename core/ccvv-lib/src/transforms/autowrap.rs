//! Stage 7: Auto-wrapper (backtick wrapping).
//!
//! Port of the Swift `addInlineCodeMarkersToPlainText` pipeline.
//! **Disabled by default** due to shell command substitution risk.
//! See §5.4 Stage 7 of the technical spec.

use regex::Regex;
use std::sync::LazyLock;

use super::whitespace::{is_code_fence_line, is_shell_command};
use super::{ContentType, RuleFired, Transform, TransformContext};

static TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S+").unwrap());
static FILENAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._-]+\.[A-Za-z0-9]{1,8}(:\d+)?$").unwrap());
static CAMEL_CASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]+[A-Z][A-Za-z0-9]*$").unwrap());
static SCREAMING_CASE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z][A-Z0-9_]{3,}$").unwrap());
static HOST_PORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9.-]+:\d{1,5}$").unwrap());
static FUNC_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9]*\([^)]*\)$").unwrap());
static DOT_ACCESS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9]+(\.[a-zA-Z][a-zA-Z0-9]+)+$").unwrap());

/// Characters that can appear as leading punctuation before a code token.
const LEADING_PUNCT: &[char] = &['(', '[', '{', '"', '\'', '\u{201C}', '\u{2018}'];

/// Characters that can appear as trailing punctuation after a code token.
const TRAILING_PUNCT: &[char] = &[
    '.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '\u{201D}', '\u{2019}',
];

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

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        // ShellBlock: wrap each command as a whole line
        if ctx.content_type == Some(ContentType::ShellBlock) {
            return self.wrap_shell_block(input, ctx);
        }

        let lines: Vec<&str> = input.split('\n').collect();
        let mut output: Vec<String> = Vec::new();
        let mut in_fence = false;
        let mut chars_changed = 0usize;

        for line in lines {
            if is_code_fence_line(line) {
                output.push(line.to_string());
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                output.push(line.to_string());
                continue;
            }
            // Wrap entire shell command lines in backticks rather than
            // wrapping individual tokens (paths, flags) separately.
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('`') && is_shell_command(trimmed) {
                let wrapped = format!("`{}`", trimmed);
                chars_changed += 2;
                output.push(wrapped);
                continue;
            }

            let wrapped = wrap_code_like_tokens_outside_backticks(line);
            if wrapped != line {
                chars_changed += wrapped.len().saturating_sub(line.len());
            }
            output.push(wrapped);
        }

        if chars_changed > 0 {
            ctx.rules_fired.push(RuleFired {
                stage: "auto_wrapper",
                description: format!("wrapped {} bytes of code-like tokens", chars_changed),
                chars_changed,
            });
        }

        output.join("\n")
    }
}

impl AutowrapTransform {
    /// ShellBlock mode: wrap each non-empty, non-comment line as a whole command.
    fn wrap_shell_block(&self, input: &str, ctx: &mut TransformContext) -> String {
        let lines: Vec<&str> = input.split('\n').collect();
        let mut output: Vec<String> = Vec::new();
        let mut chars_changed = 0usize;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                output.push(line.to_string());
                continue;
            }
            // Already wrapped — leave alone (idempotency)
            if trimmed.starts_with('`') && trimmed.ends_with('`') {
                output.push(line.to_string());
                continue;
            }
            let wrapped = format!("`{}`", trimmed);
            chars_changed += 2;
            output.push(wrapped);
        }

        if chars_changed > 0 {
            ctx.rules_fired.push(RuleFired {
                stage: "auto_wrapper",
                description: format!("wrapped {} bytes of code-like tokens", chars_changed),
                chars_changed,
            });
        }

        output.join("\n")
    }
}

/// Wrap code-like tokens in a line, respecting existing backtick spans.
fn wrap_code_like_tokens_outside_backticks(line: &str) -> String {
    let parts: Vec<&str> = line.split('`').collect();
    if parts.is_empty() {
        return line.to_string();
    }

    let mut rebuilt = String::new();
    for (index, segment) in parts.iter().enumerate() {
        if index % 2 == 0 {
            // Outside backticks — process tokens
            rebuilt.push_str(&wrap_code_like_tokens_in_segment(segment));
        } else {
            // Inside backticks — keep verbatim
            rebuilt.push_str(segment);
        }
        if index < parts.len() - 1 {
            rebuilt.push('`');
        }
    }
    rebuilt
}

/// Wrap code-like tokens within a segment (outside backticks).
fn wrap_code_like_tokens_in_segment(segment: &str) -> String {
    let mut result = segment.to_string();
    let matches: Vec<regex::Match> = TOKEN_RE.find_iter(segment).collect();

    // Process in reverse to preserve offsets
    for m in matches.into_iter().rev() {
        let token = m.as_str();
        let wrapped = wrap_token_if_code_like(token);
        if wrapped != token {
            result.replace_range(m.start()..m.end(), &wrapped);
        }
    }
    result
}

/// Attempt to wrap a single token if it looks like code.
fn wrap_token_if_code_like(token: &str) -> String {
    let (leading, rest) = split_leading(token);

    // Function call pattern: identifier(args) possibly followed by trailing punct.
    // Must check before split_trailing which would break the () pair.
    if let Some(open) = rest.find('(') {
        if let Some(rel_close) = rest[open..].find(')') {
            let close_idx = open + rel_close;
            let func_call = &rest[..=close_idx];
            let after = &rest[close_idx + 1..];
            if (after.is_empty() || after.chars().all(|c| TRAILING_PUNCT.contains(&c)))
                && FUNC_CALL_RE.is_match(func_call)
            {
                return format!("{}`{}`{}", leading, func_call, after);
            }
        }
    }

    let (core, trailing) = split_trailing(rest);

    if !should_wrap_code_token(core) {
        return token.to_string();
    }
    format!("{}`{}`{}", leading, core, trailing)
}

/// Split leading punctuation from a token.
fn split_leading(text: &str) -> (&str, &str) {
    let mut idx = 0;
    for ch in text.chars() {
        if LEADING_PUNCT.contains(&ch) {
            idx += ch.len_utf8();
        } else {
            break;
        }
    }
    (&text[..idx], &text[idx..])
}

/// Split trailing punctuation from a token.
fn split_trailing(text: &str) -> (&str, &str) {
    if text.is_empty() {
        return ("", "");
    }
    let mut end = text.len();
    for ch in text.chars().rev() {
        if TRAILING_PUNCT.contains(&ch) {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    (&text[..end], &text[end..])
}

/// Heuristic check: should this token be wrapped in backticks?
pub fn should_wrap_code_token(token: &str) -> bool {
    if token.is_empty() || token.len() < 2 || token.len() > 100 {
        return false;
    }
    if token.contains('`') || token.starts_with("http://") || token.starts_with("https://") {
        return false;
    }

    // CLI flags: --flag
    if token.starts_with("--") && token.len() > 2 && token.as_bytes()[2].is_ascii_alphanumeric() {
        return true;
    }
    if token.contains('_') {
        return true;
    }
    if looks_like_path(token) {
        return true;
    }
    if FILENAME_RE.is_match(token) {
        return true;
    }
    looks_like_code_identifier(token)
}

fn looks_like_path(token: &str) -> bool {
    token.contains('/')
        && (token.starts_with('/')
            || token.starts_with("./")
            || token.starts_with("../")
            || token.contains('.')
            || token.contains('-')
            || token.contains(':'))
}

fn looks_like_code_identifier(token: &str) -> bool {
    if CAMEL_CASE_RE.is_match(token) && token.chars().any(|c| c.is_ascii_lowercase()) {
        return true;
    }
    if SCREAMING_CASE_RE.is_match(token) {
        return true;
    }
    if HOST_PORT_RE.is_match(token) {
        return true;
    }
    if (token.starts_with('$') && token.len() > 1 && !token.contains(' '))
        || (token.ends_with('>') && token.contains('#'))
    {
        return true;
    }
    DOT_ACCESS_RE.is_match(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_flags_wrapped() {
        assert!(should_wrap_code_token("--flag"));
        assert!(should_wrap_code_token("--verbose"));
    }

    #[test]
    fn test_snake_case_wrapped() {
        assert!(should_wrap_code_token("some_var"));
        assert!(should_wrap_code_token("my_function_name"));
    }

    #[test]
    fn test_url_not_wrapped() {
        assert!(!should_wrap_code_token("https://example.com"));
        assert!(!should_wrap_code_token("http://localhost:3000"));
    }

    #[test]
    fn test_already_backticked_skipped() {
        let input = "Use `some_var` in your code";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        // some_var is already in backticks, should not double-wrap
        assert_eq!(result, "Use `some_var` in your code");
    }

    #[test]
    fn test_path_wrapped() {
        assert!(should_wrap_code_token("/usr/local/bin"));
        assert!(should_wrap_code_token("./config.yml"));
        assert!(should_wrap_code_token("../parent/file.txt"));
    }

    #[test]
    fn test_filename_wrapped() {
        assert!(should_wrap_code_token("config.yaml"));
        assert!(should_wrap_code_token("main.swift"));
    }

    #[test]
    fn test_camel_case_wrapped() {
        assert!(should_wrap_code_token("myFunction"));
        assert!(should_wrap_code_token("someValue"));
    }

    #[test]
    fn test_screaming_case_wrapped() {
        assert!(should_wrap_code_token("MAX_SIZE"));
        assert!(should_wrap_code_token("DEFAULT_VALUE"));
    }

    #[test]
    fn test_short_words_not_wrapped() {
        assert!(!should_wrap_code_token("a"));
        assert!(!should_wrap_code_token("the"));
        assert!(!should_wrap_code_token("hello"));
    }

    #[test]
    fn test_code_fence_skipped() {
        let input = "```\nsome_var = 1\n```";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "```\nsome_var = 1\n```");
    }

    #[test]
    fn test_idempotency() {
        let input = "Use --flag and some_var for config.yaml";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(first, second, "Auto-wrapper must be idempotent");
    }

    #[test]
    fn test_host_port_wrapped() {
        assert!(should_wrap_code_token("localhost:8080"));
        assert!(should_wrap_code_token("db.example.com:5432"));
        assert!(should_wrap_code_token("127.0.0.1:3000"));
    }

    #[test]
    fn test_shell_prompt_wrapped() {
        assert!(should_wrap_code_token("$HOME"));
        assert!(should_wrap_code_token("$PATH"));
    }

    #[test]
    fn test_dollar_sign_alone_not_wrapped() {
        assert!(!should_wrap_code_token("$"));
    }

    #[test]
    fn test_function_call_wrapped() {
        let input = "Call makeTemplateIcon() to create an icon";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Call `makeTemplateIcon()` to create an icon");
    }

    #[test]
    fn test_function_call_with_args_wrapped() {
        let input = "Use NSImage(systemSymbolName:) for icons";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Use `NSImage(systemSymbolName:)` for icons");
    }

    #[test]
    fn test_function_call_with_trailing_punct() {
        let input = "calls transform(), then returns";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "calls `transform()`, then returns");
    }

    #[test]
    fn test_function_call_idempotency() {
        let input = "Use performClean() and transform() here";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(first, second, "Function call wrapping must be idempotent");
    }

    #[test]
    fn test_dot_access_wrapped() {
        assert!(should_wrap_code_token("ccvv.cleanSuccess"));
        assert!(should_wrap_code_token("button.image"));
        assert!(should_wrap_code_token("result.summary"));
    }

    #[test]
    fn test_dot_access_short_components_not_matched() {
        // Single-char components should not match DOT_ACCESS_RE
        // (they may still match FILENAME_RE, which is fine)
        assert!(!DOT_ACCESS_RE.is_match("e.g"));
        assert!(!DOT_ACCESS_RE.is_match("i.e"));
    }

    #[test]
    fn test_dot_access_chained() {
        assert!(should_wrap_code_token("foo.bar.baz"));
    }

    #[test]
    fn test_prose_parens_not_wrapped() {
        // Regular prose with parenthetical should not be wrapped
        let input = "This is (a test) of the system";
        let mut ctx = TransformContext::default();
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "This is (a test) of the system");
    }

    #[test]
    fn test_shell_block_whole_line_wrapping() {
        let input = "rm -rf /tmp/foo\ncp -r ~/src /tmp/";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::ShellBlock),
            ..Default::default()
        };
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "`rm -rf /tmp/foo`\n`cp -r ~/src /tmp/`");
    }

    #[test]
    fn test_shell_block_preserves_comments() {
        let input = "# Install deps\ncargo build --release";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::ShellBlock),
            ..Default::default()
        };
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "# Install deps\n`cargo build --release`");
    }

    #[test]
    fn test_shell_block_idempotent() {
        let input = "`rm -rf /tmp/foo`\n`cp -r ~/src /tmp/`";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::ShellBlock),
            ..Default::default()
        };
        let transform = AutowrapTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input, "Shell block wrapping must be idempotent");
    }

}
