//! Content type classification heuristics.
//!
//! Determines whether clipboard content is URL, code, prose, table,
//! JSON, or mixed. Used by the pipeline and history system.
//! See §7 of the technical spec.

use crate::table_extract::extract_table;
use crate::transforms::ContentType;
use crate::transforms::whitespace::{is_list_item, is_shell_command};

/// Classify the content type of the given text.
pub fn classify(text: &str) -> ContentType {
    let trimmed = text.trim();

    // JSON detection: entire content parses as JSON
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return ContentType::Json;
    }

    // Table detection (robust): terminal/pipe/markdown/delimiter formats
    if extract_table(trimmed).detected {
        return ContentType::Table;
    }

    // Single URL check
    if !trimmed.contains('\n')
        && (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.contains(' ')
    {
        return ContentType::Url;
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let non_empty: Vec<&str> = lines.iter().filter(|l| !l.is_empty()).copied().collect();

    // ShellBlock detection: ALL non-empty lines are shell-like
    if !non_empty.is_empty() && non_empty.iter().all(|l| is_shell_line(l)) {
        return ContentType::ShellBlock;
    }

    // Code detection: shebang is unambiguous; otherwise require indentation
    // AND no strong prose signal (indented prose from CLIs is common)
    if lines.len() > 3 {
        if trimmed.starts_with("#!") {
            return ContentType::Code;
        }
        let indented = lines
            .iter()
            .filter(|l| !l.is_empty() && l.starts_with([' ', '\t']))
            .count();
        if !non_empty.is_empty() && indented * 100 / non_empty.len() >= 40 {
            let prose_lines = non_empty
                .iter()
                .filter(|l| {
                    let t = l.trim();
                    t.ends_with('.') || t.ends_with('?') || t.ends_with('!')
                })
                .count();
            if prose_lines * 100 / non_empty.len() < 30 {
                return ContentType::Code;
            }
        }
    }

    // List detection: ≥60% of non-empty lines are list items, ≥2 non-empty lines
    if non_empty.len() >= 2 {
        let list_count = non_empty.iter().filter(|l| is_list_item(l.trim())).count();
        if list_count * 100 / non_empty.len() >= 60 {
            return ContentType::List;
        }
    }

    // Default
    if lines.len() <= 2 {
        ContentType::Prose
    } else {
        ContentType::Mixed
    }
}

/// Check if a single line is shell-like: a shell command, comment, shebang,
/// continuation, or prompt.
fn is_shell_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Shell comment (# followed by space) or shebang (#!).
    // Excludes markdown headings (## ...) and horizontal rules (---).
    if trimmed.starts_with("#!") || trimmed.starts_with("# ") {
        return true;
    }
    // Continuation line (ends with \)
    if trimmed.ends_with('\\') {
        return true;
    }
    // Prompt prefix ($ command)
    if trimmed.starts_with("$ ") {
        return true;
    }
    // Standalone flags (--flag or -f) as continuation of a prior command.
    // Excludes markdown horizontal rules (---) and PEM markers (-----BEGIN).
    if (trimmed.starts_with("--") && trimmed.len() > 2 && trimmed.as_bytes()[2].is_ascii_alphanumeric())
        || (trimmed.starts_with('-')
            && trimmed.len() > 1
            && trimmed.as_bytes()[1] != b' '
            && trimmed.as_bytes()[1] != b'-')
    {
        return true;
    }
    is_shell_command(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_json() {
        assert_eq!(classify(r#"{"key": "value"}"#), ContentType::Json);
    }

    #[test]
    fn test_classify_url() {
        assert_eq!(classify("https://example.com/path?q=1"), ContentType::Url);
    }

    #[test]
    fn test_classify_prose() {
        assert_eq!(classify("Hello world."), ContentType::Prose);
    }

    #[test]
    fn test_classify_table() {
        let tsv = "Name\tAge\nAlice\t30\nBob\t25";
        assert_eq!(classify(tsv), ContentType::Table);
    }

    #[test]
    fn test_classify_table_aligned_pipe_block() {
        let block = "│ Memory snapshot    │ @modal.enter snapshot may become  │ [M] Periodic forced re-snapshot │\n\
                     │ drift              │ stale, causing subtle bugs after  │ Test snapshot restore            │\n\
                     │                    │ Modal infra updates               │                                  │";
        assert_eq!(classify(block), ContentType::Table);
    }

    #[test]
    fn test_classify_multiline_url_like_not_url() {
        // Multi-line input starting with http:// should NOT be classified as Url
        let input = "https://example.com/path\nsome other line";
        assert_ne!(classify(input), ContentType::Url);
    }

    #[test]
    fn test_classify_code_shebang() {
        let code = "#!/bin/bash\necho hello\necho world\necho done";
        assert_eq!(classify(code), ContentType::Code);
    }

    #[test]
    fn test_classify_shell_block_multiple_commands() {
        let input = "rm -rf /tmp/foo\ncp -r ~/src /tmp/\nmkdir -p /tmp/out";
        assert_eq!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_shell_block_with_comments() {
        let input = "# Install deps\nbrew install --cask jq\n# Build\ncargo build --release";
        assert_eq!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_shell_block_with_continuations() {
        let input = "aws rds wait db-instance-available \\\n  --db-instance-identifier stagingdb \\\n  --region eu-central-1";
        assert_eq!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_shell_block_single_command() {
        let input = "cargo build --release";
        assert_eq!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_mixed_prose_with_commands_not_shell_block() {
        let input = "The running app has a lock. You need to:\n\n1. Quit ccvv\n2. Then reinstall:\n\nrm -rf /Applications/ccvv.app\ncp -r ~/build/ccvv.app /Applications/";
        assert_ne!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_list_pure() {
        let input = "- Item one\n- Item two\n- Item three\n- Item four";
        assert_eq!(classify(input), ContentType::List);
    }

    #[test]
    fn test_classify_list_numbered() {
        let input = "1. First step\n2. Second step\n3. Third step";
        assert_eq!(classify(input), ContentType::List);
    }

    #[test]
    fn test_classify_list_with_non_list_minority() {
        // 3 out of 4 non-empty lines are list items = 75% >= 60%
        let input = "Shopping list:\n- Apples\n- Bananas\n- Oranges";
        assert_eq!(classify(input), ContentType::List);
    }

    #[test]
    fn test_classify_prose_with_one_list_item_not_list() {
        let input = "This is a paragraph about things.\n- Just one item";
        assert_ne!(classify(input), ContentType::List);
    }

    #[test]
    fn test_classify_markdown_headings_not_shell_block() {
        let input = "# Heading\n## Subheading\n### Third level";
        assert_ne!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_horizontal_rule_not_shell_block() {
        let input = "Some text\n---\nMore text";
        assert_ne!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_pem_block_not_shell_block() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        assert_ne!(classify(input), ContentType::ShellBlock);
    }

    #[test]
    fn test_classify_indented_prose_not_code() {
        // Indented text from CLI output — prose with sentence endings should NOT be Code
        let input = "  1. EKS endpoint made private.\n  The one-line change broke kubectl.\n\n  2. Lambda moved into the VPC.\n  This required referencing VPC properties.";
        assert_ne!(classify(input), ContentType::Code);
    }
}
