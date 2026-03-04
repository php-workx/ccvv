//! Stage 3: Whitespace & line break cleanup.
//!
//! Port of the Swift `ccvv()` function from `mac/main.swift:11-209`.
//! See §5.4 Stage 3 of the technical spec.

use std::collections::HashMap;
use regex::Regex;
use std::sync::LazyLock;

use super::{Transform, TransformContext};

static MULTI_SPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {3,}").unwrap());
static BULLET_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[•◦▪]\s+").unwrap());
static NUMBERED_LIST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+[.)\]] ").unwrap());
static RECORDING_DOT_BULLET_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[◦▪]\s+").unwrap());

/// Data structure for paragraph line tracking.
#[derive(Debug, Clone)]
pub struct ParagraphLine {
    pub text: String,
    pub indent: usize,
}

/// Whitespace cleanup transform (Stage 3).
pub struct WhitespaceTransform;

impl WhitespaceTransform {
    pub fn new() -> Self {
        WhitespaceTransform
    }
}

impl Default for WhitespaceTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for WhitespaceTransform {
    fn name(&self) -> &'static str {
        "whitespace_cleanup"
    }

    fn apply(&self, input: &str, _ctx: &mut TransformContext) -> String {
        ccvv(input)
    }
}

/// Main text cleaning function — port of Swift `ccvv()`.
pub fn ccvv(input: &str) -> String {
    // Normalize line endings
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();

    // Detect terminal width: most common raw line length (for lines > 40 chars)
    let terminal_width = detect_terminal_width(&raw_lines);

    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph: Vec<ParagraphLine> = Vec::new();
    let mut code_block: Vec<String> = Vec::new();
    let mut in_code_fence = false;
    let mut code_fence_indent = String::new();
    let mut prev_raw_line_len: usize = 0;

    let flush_paragraph = |paragraph: &mut Vec<ParagraphLine>, blocks: &mut Vec<String>| {
        if paragraph.is_empty() {
            return;
        }
        let compacted = compact_paragraph(paragraph);
        if !compacted.is_empty() {
            blocks.push(compacted);
        }
        paragraph.clear();
    };

    let flush_code_block = |code_block: &mut Vec<String>, blocks: &mut Vec<String>| {
        if code_block.is_empty() {
            return;
        }
        blocks.push(code_block.join("\n"));
        code_block.clear();
    };

    for raw_line in &raw_lines {
        let mut line = raw_line.trim_end().to_string();

        // Collapse terminal padding and detect excessive leading whitespace
        let mut excessive_padding = false;
        if !in_code_fence {
            let ws = leading_whitespace(&line);
            let body = &line[ws.len()..];
            if !body.is_empty() {
                let collapsed = MULTI_SPACE_RE.replace_all(body, " ").to_string();
                if ws.len() > 20 {
                    excessive_padding = true;
                    line = collapsed;
                } else {
                    line = format!("{}{}", ws, collapsed);
                }
            }
        }

        if in_code_fence {
            if is_code_fence_line(&line) {
                code_block.push(canonical_fence_line(&line));
                flush_code_block(&mut code_block, &mut blocks);
                in_code_fence = false;
                code_fence_indent = String::new();
            } else {
                code_block.push(dedent(&line, &code_fence_indent));
            }
            continue;
        }

        if is_code_fence_line(&line) {
            flush_paragraph(&mut paragraph, &mut blocks);
            in_code_fence = true;
            code_fence_indent = leading_whitespace(&line);
            code_block = vec![canonical_fence_line(&line)];
            continue;
        }

        let mut indent = leading_indent_count(&line);
        let mut cleaned = line.trim_start().to_string();

        // Strip recording dot at line start
        if cleaned.starts_with('\u{23FA}') {
            cleaned = cleaned.trim_start_matches('\u{23FA}').to_string();
            cleaned = cleaned.trim_start().to_string();
        }

        // Adjust indent for sub-bullet markers
        if RECORDING_DOT_BULLET_RE.is_match(&cleaned) {
            indent += 2;
        }

        cleaned = normalize_bullet_marker(&cleaned);

        if cleaned.is_empty() || excessive_padding {
            flush_paragraph(&mut paragraph, &mut blocks);
        }

        if !cleaned.is_empty() {
            // Detect implicit paragraph break in terminal text
            if terminal_width > 0
                && !paragraph.is_empty()
                && prev_raw_line_len > 0
                && prev_raw_line_len < terminal_width * 85 / 100
            {
                if let Some(prev) = paragraph.last() {
                    if let Some(last_char) = prev.text.chars().last() {
                        if ".!?:".contains(last_char) && looks_like_paragraph_start(&cleaned) {
                            flush_paragraph(&mut paragraph, &mut blocks);
                        }
                    }
                }
            }

            paragraph.push(ParagraphLine {
                text: cleaned,
                indent,
            });
        }

        prev_raw_line_len = raw_line.trim_end().len();
    }

    flush_paragraph(&mut paragraph, &mut blocks);
    if !code_block.is_empty() {
        flush_code_block(&mut code_block, &mut blocks);
    }

    blocks.join("\n\n")
}

/// Detect terminal width from raw line lengths.
fn detect_terminal_width(raw_lines: &[&str]) -> usize {
    let raw_lengths: Vec<usize> = raw_lines
        .iter()
        .map(|l| l.chars().count())
        .filter(|&len| len > 40)
        .collect();

    let mut length_counts: HashMap<usize, usize> = HashMap::new();
    for &len in &raw_lengths {
        *length_counts.entry(len).or_insert(0) += 1;
    }

    length_counts
        .iter()
        .max_by_key(|(_, &count)| count)
        .and_then(|(&len, &count)| if count >= 3 { Some(len) } else { None })
        .unwrap_or(0)
}

/// Check if a line looks like the start of a new paragraph.
fn looks_like_paragraph_start(line: &str) -> bool {
    if is_list_item(line) {
        return true;
    }
    line.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Check if a line is a list item.
pub fn is_list_item(line: &str) -> bool {
    line.starts_with("- ")
        || line.starts_with("* ")
        || BULLET_RE.is_match(line)
        || NUMBERED_LIST_RE.is_match(line)
}

/// Normalize bullet markers to `- `.
pub fn normalize_bullet_marker(line: &str) -> String {
    BULLET_RE.replace(line, "- ").to_string()
}

/// Compact a paragraph by joining continuation lines.
pub fn compact_paragraph(lines: &[ParagraphLine]) -> String {
    let mut output_lines: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let base_indent = lines.iter().map(|l| l.indent).min().unwrap_or(0);
    let mut last_list_item_rel_indent: usize = 0;

    for entry in lines {
        let relative_indent = entry.indent.saturating_sub(base_indent);

        if is_list_item(&entry.text) {
            if !buffer.is_empty() {
                output_lines.push(buffer.clone());
                buffer.clear();
            }
            last_list_item_rel_indent = relative_indent;
            let indent_str = " ".repeat(relative_indent);
            output_lines.push(format!("{}{}", indent_str, entry.text));
        } else if !output_lines.is_empty()
            && is_list_item_with_optional_indent(output_lines.last().unwrap())
            && buffer.is_empty()
            && relative_indent > last_list_item_rel_indent
        {
            // Continuation of a list item
            let last = output_lines.last_mut().unwrap();
            last.push(' ');
            last.push_str(&entry.text);
        } else {
            // Regular paragraph continuation
            if buffer.is_empty() {
                buffer = entry.text.clone();
            } else {
                buffer.push(' ');
                buffer.push_str(&entry.text);
            }
        }
    }

    if !buffer.is_empty() {
        output_lines.push(buffer);
    }

    output_lines.join("\n")
}

/// Check if a line is a list item, ignoring leading whitespace.
fn is_list_item_with_optional_indent(line: &str) -> bool {
    let trimmed = line.trim_start();
    is_list_item(trimmed)
}

/// Check if a line is a code fence marker.
pub fn is_code_fence_line(line: &str) -> bool {
    line.trim().starts_with("```")
}

/// Canonicalize a code fence line (trim whitespace).
pub fn canonical_fence_line(line: &str) -> String {
    line.trim().to_string()
}

/// Get the leading whitespace of a line as a string.
pub fn leading_whitespace(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// Remove a prefix from a line (dedent).
pub fn dedent(line: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return line.to_string();
    }
    if let Some(rest) = line.strip_prefix(prefix) {
        rest.to_string()
    } else {
        line.to_string()
    }
}

/// Count leading indent (tabs count as 4 spaces).
pub fn leading_indent_count(line: &str) -> usize {
    let mut count = 0;
    for ch in line.chars() {
        if ch == '\t' {
            count += 4;
        } else if ch.is_whitespace() {
            count += 1;
        } else {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_paragraph_compaction() {
        let input = "This is a long\nparagraph that\nshould be joined.";
        let result = ccvv(input);
        assert_eq!(result, "This is a long paragraph that should be joined.");
    }

    #[test]
    fn test_code_fence_preserved() {
        let input = "Before.\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\nAfter.";
        let result = ccvv(input);
        assert!(result.contains("```rust"));
        assert!(result.contains("fn main()"));
        assert!(result.contains("```"));
        assert!(result.contains("Before."));
        assert!(result.contains("After."));
    }

    #[test]
    fn test_bullet_normalization() {
        let input = "• Item one\n◦ Item two\n▪ Item three";
        let result = ccvv(input);
        assert!(result.contains("- Item one"));
        assert!(result.contains("- Item two"));
        assert!(result.contains("- Item three"));
    }

    #[test]
    fn test_terminal_width_detection() {
        // Create lines with a common length to trigger terminal width detection
        let line = "a".repeat(80);
        let lines = vec![line.as_str(); 5];
        let width = detect_terminal_width(&lines);
        assert_eq!(width, 80);
    }

    #[test]
    fn test_excessive_leading_whitespace() {
        let input =
            "First paragraph.\n                         Second paragraph after excessive indent.";
        let result = ccvv(input);
        // Excessive indent (>20) should cause paragraph break
        assert!(result.contains("First paragraph."));
        assert!(result.contains("Second paragraph after excessive indent."));
        assert!(result.contains("\n\n"));
    }

    #[test]
    fn test_list_item_preservation() {
        let input = "- Item one\n- Item two\n- Item three";
        let result = ccvv(input);
        assert_eq!(result, "- Item one\n- Item two\n- Item three");
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(ccvv(""), "");
    }

    #[test]
    fn test_crlf_normalization() {
        let input = "Line one.\r\nLine two.\r\nLine three.";
        let result = ccvv(input);
        assert_eq!(result, "Line one. Line two. Line three.");
    }

    #[test]
    fn test_recording_dot_removal() {
        let input = "\u{23FA} Some text";
        let result = ccvv(input);
        assert_eq!(result, "Some text");
    }

    #[test]
    fn test_numbered_list() {
        let input = "1. First\n2. Second\n3. Third";
        let result = ccvv(input);
        assert_eq!(result, "1. First\n2. Second\n3. Third");
    }

    #[test]
    fn test_multiple_paragraphs() {
        let input = "First paragraph\nwith continuation.\n\nSecond paragraph\nwith continuation.";
        let result = ccvv(input);
        assert_eq!(
            result,
            "First paragraph with continuation.\n\nSecond paragraph with continuation."
        );
    }

    #[test]
    fn test_indented_code_fence() {
        let input = "  ```python\n  def foo():\n      pass\n  ```";
        let result = ccvv(input);
        assert!(result.contains("```python"));
        assert!(result.contains("def foo():"));
    }

    #[test]
    fn test_idempotency() {
        let input = "Hello world.\n\n- Item one\n- Item two\n\n```\ncode\n```";
        let first = ccvv(input);
        let second = ccvv(&first);
        assert_eq!(first, second, "Whitespace transform must be idempotent");
    }

    #[test]
    fn test_mixed_content() {
        let input = "# Heading\n\nSome text that spans\nmultiple lines.\n\n- List item\n  continuation\n\n```\ncode block\n```";
        let result = ccvv(input);
        assert!(result.contains("# Heading"));
        assert!(result.contains("Some text that spans multiple lines."));
        assert!(result.contains("- List item continuation"));
        assert!(result.contains("```\ncode block\n```"));
    }
}
