//! Stage 3: Whitespace & line break cleanup.
//!
//! Port of the Swift `ccvv()` function from `mac/main.swift:11-209`.
//! See §5.4 Stage 3 of the technical spec.

use regex::Regex;
use std::sync::LazyLock;

use super::{ContentType, Transform, TransformContext};

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
    /// Character count of the original raw line (before cleaning/trimming).
    pub raw_len: usize,
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

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        match ctx.content_type {
            Some(ContentType::ShellBlock) => ccvv_shell_block(input),
            _ => ccvv(input),
        }
    }
}

/// Main text cleaning function — port of Swift `ccvv()`.
pub fn ccvv(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();
    let terminal_width = detect_terminal_width(&raw_lines);

    let mut blocks: Vec<String> = Vec::new();
    let mut paragraph: Vec<ParagraphLine> = Vec::new();
    let mut code_block: Vec<String> = Vec::new();
    let mut in_code_fence = false;
    let mut code_fence_indent = String::new();

    for raw_line in &raw_lines {
        let (line, excessive_padding) = collapse_padding(raw_line, in_code_fence);

        if in_code_fence {
            handle_inside_fence(
                &line,
                &mut code_block,
                &mut blocks,
                &mut in_code_fence,
                &mut code_fence_indent,
            );
            continue;
        }

        if is_code_fence_line(&line) {
            flush_paragraph(&mut paragraph, &mut blocks, terminal_width);
            in_code_fence = true;
            code_fence_indent = leading_whitespace(&line);
            code_block = vec![canonical_fence_line(&line)];
            continue;
        }

        process_prose_line(
            raw_line,
            &line,
            excessive_padding,
            &mut paragraph,
            &mut blocks,
            terminal_width,
        );
    }

    flush_paragraph(&mut paragraph, &mut blocks, terminal_width);
    if !code_block.is_empty() {
        flush_code_block(&mut code_block, &mut blocks);
    }

    blocks.join("\n\n")
}

fn flush_paragraph(
    paragraph: &mut Vec<ParagraphLine>,
    blocks: &mut Vec<String>,
    terminal_width: usize,
) {
    if paragraph.is_empty() {
        return;
    }
    let compacted = compact_paragraph_with_width(paragraph, terminal_width);
    if !compacted.is_empty() {
        blocks.push(compacted);
    }
    paragraph.clear();
}

fn flush_code_block(code_block: &mut Vec<String>, blocks: &mut Vec<String>) {
    if code_block.is_empty() {
        return;
    }
    blocks.push(code_block.join("\n"));
    code_block.clear();
}

/// Handle a line while inside a code fence: either close the fence or accumulate.
fn handle_inside_fence(
    line: &str,
    code_block: &mut Vec<String>,
    blocks: &mut Vec<String>,
    in_code_fence: &mut bool,
    code_fence_indent: &mut String,
) {
    if is_code_fence_line(line) {
        code_block.push(canonical_fence_line(line));
        flush_code_block(code_block, blocks);
        *in_code_fence = false;
        *code_fence_indent = String::new();
    } else {
        code_block.push(dedent(line, code_fence_indent));
    }
}

/// Process a prose line: clean markers, accumulate into paragraph.
fn process_prose_line(
    raw_line: &str,
    line: &str,
    excessive_padding: bool,
    paragraph: &mut Vec<ParagraphLine>,
    blocks: &mut Vec<String>,
    terminal_width: usize,
) {
    let (cleaned, indent) = clean_line_markers(line);

    if cleaned.is_empty() || excessive_padding {
        flush_paragraph(paragraph, blocks, terminal_width);
    }

    if !cleaned.is_empty() {
        paragraph.push(ParagraphLine {
            text: cleaned,
            indent,
            raw_len: raw_line.trim_end().chars().count(),
        });
    }
}

/// Collapse terminal padding and detect excessive leading whitespace.
fn collapse_padding(raw_line: &str, in_code_fence: bool) -> (String, bool) {
    if in_code_fence {
        return (raw_line.to_string(), false);
    }

    let line = raw_line.trim_end().to_string();
    let mut excessive_padding = false;

    let ws = leading_whitespace(&line);
    let body = &line[ws.len()..];
    if body.is_empty() {
        return (line, false);
    }

    let collapsed = MULTI_SPACE_RE.replace_all(body, " ").to_string();
    let result = if ws.len() > 20 {
        excessive_padding = true;
        collapsed
    } else {
        format!("{}{}", ws, collapsed)
    };

    (result, excessive_padding)
}

/// Strip recording dot, normalize bullets, compute indent.
pub(crate) fn clean_line_markers(line: &str) -> (String, usize) {
    let mut indent = leading_indent_count(line);
    let mut cleaned = line.trim_start().to_string();

    if cleaned.starts_with('\u{23FA}') {
        cleaned = cleaned
            .trim_start_matches('\u{23FA}')
            .trim_start()
            .to_string();
    }

    if RECORDING_DOT_BULLET_RE.is_match(&cleaned) {
        indent += 2;
    }

    cleaned = normalize_bullet_marker(&cleaned);
    (cleaned, indent)
}

/// Detect terminal width from raw line lengths.
///
/// Word-wrapped text produces lines of varying lengths (e.g., 78, 80, 82, 84)
/// because words break at different points. We bucket nearby lengths (±2 chars)
/// and look for a cluster of ≥3 lines, returning the maximum length in that
/// cluster as the terminal width.
fn detect_terminal_width(raw_lines: &[&str]) -> usize {
    let mut raw_lengths: Vec<usize> = raw_lines
        .iter()
        .map(|l| l.trim_end().chars().count())
        .filter(|&len| len > 40)
        .collect();

    if raw_lengths.len() < 3 {
        return 0;
    }

    raw_lengths.sort_unstable();

    // Sliding window: find the largest cluster of lengths within a 4-char range
    let mut best_count = 0;
    let mut best_max = 0;

    for (i, &len) in raw_lengths.iter().enumerate() {
        // Count how many lengths fall within [len, len+4]
        let count = raw_lengths[i..]
            .iter()
            .take_while(|&&l| l <= len + 4)
            .count();
        if count > best_count || (count == best_count && len > best_max) {
            best_count = count;
            // Use the maximum length in this cluster as the terminal width
            best_max = raw_lengths[i..i + count]
                .iter()
                .copied()
                .max()
                .unwrap_or(len);
        }
    }

    if best_count >= 3 {
        best_max
    } else {
        0
    }
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

/// Join shell continuation lines (lines ending with `\`) into single lines.
pub(crate) fn join_shell_continuations(lines: &[ParagraphLine]) -> Vec<ParagraphLine> {
    let mut result: Vec<ParagraphLine> = Vec::new();
    let mut acc: Option<ParagraphLine> = None;

    for entry in lines {
        if let Some(ref mut a) = acc {
            // Continuing a backslash-joined sequence
            let trimmed = entry.text.trim_start();
            if let Some(stripped) = trimmed.strip_suffix('\\') {
                a.text.push(' ');
                a.text.push_str(stripped.trim_end());
            } else {
                a.text.push(' ');
                a.text.push_str(trimmed);
                result.push(a.clone());
                acc = None;
            }
        } else if entry.text.ends_with('\\') {
            // Start a new backslash continuation
            acc = Some(ParagraphLine {
                text: entry.text[..entry.text.len() - 1].trim_end().to_string(),
                indent: entry.indent,
                raw_len: entry.raw_len,
            });
        } else {
            result.push(entry.clone());
        }
    }

    if let Some(a) = acc {
        result.push(a);
    }
    result
}

/// Compact a paragraph by joining continuation lines (without terminal width info).
pub fn compact_paragraph(lines: &[ParagraphLine]) -> String {
    compact_paragraph_with_width(lines, 0)
}

/// Compact a paragraph by joining continuation lines.
///
/// When `terminal_width > 0`, applies a reverse word-wrap heuristic in the
/// regular-prose branch: if the previous raw line was short enough that the
/// first word of the current line *could* have fit, the line break was
/// intentional and should be preserved (as a `\n` within the block, not a
/// `\n\n` block separator).
pub fn compact_paragraph_with_width(lines: &[ParagraphLine], terminal_width: usize) -> String {
    let joined = join_shell_continuations(lines);
    let lines = &joined;
    let mut state = CompactState::new(lines);

    for entry in lines {
        let relative_indent = entry.indent.saturating_sub(state.base_indent);
        state.process_line(entry, relative_indent, terminal_width);
    }

    state.finish()
}

/// Mutable state for paragraph compaction, extracted to reduce cognitive
/// complexity of the main loop.
struct CompactState {
    output_lines: Vec<String>,
    buffer: String,
    last_buffer_raw_len: usize,
    base_indent: usize,
    last_list_item_rel_indent: usize,
    /// When a continuation at indent C joins a list item at indent L (C > L),
    /// subsequent list items at indent C are promoted to indent L.
    promoted_indents: std::collections::HashMap<usize, usize>,
}

impl CompactState {
    fn new(lines: &[ParagraphLine]) -> Self {
        Self {
            output_lines: Vec::new(),
            buffer: String::new(),
            last_buffer_raw_len: 0,
            base_indent: lines.iter().map(|l| l.indent).min().unwrap_or(0),
            last_list_item_rel_indent: 0,
            promoted_indents: std::collections::HashMap::new(),
        }
    }

    fn process_line(
        &mut self,
        entry: &ParagraphLine,
        relative_indent: usize,
        terminal_width: usize,
    ) {
        if is_list_item(&entry.text) {
            let effective_indent = self
                .promoted_indents
                .get(&relative_indent)
                .copied()
                .unwrap_or(relative_indent);
            self.flush_buffer();
            self.last_list_item_rel_indent = effective_indent;
            let indent_str = " ".repeat(effective_indent);
            self.output_lines
                .push(format!("{}{}", indent_str, entry.text));
        } else if is_shell_command(&entry.text) {
            self.flush_buffer();
            self.output_lines.push(entry.text.clone());
        } else if self.is_list_continuation(relative_indent) {
            if relative_indent > self.last_list_item_rel_indent {
                self.promoted_indents
                    .insert(relative_indent, self.last_list_item_rel_indent);
            }
            let last = self.output_lines.last_mut().unwrap();
            last.push(' ');
            last.push_str(&entry.text);
        } else {
            self.append_prose(entry, terminal_width);
        }
    }

    fn is_list_continuation(&self, relative_indent: usize) -> bool {
        !self.output_lines.is_empty()
            && is_list_item_with_optional_indent(self.output_lines.last().unwrap())
            && self.buffer.is_empty()
            && relative_indent >= self.last_list_item_rel_indent
    }

    fn append_prose(&mut self, entry: &ParagraphLine, terminal_width: usize) {
        if !self.buffer.is_empty()
            && (self.buffer.ends_with(':')
                || should_keep_break(terminal_width, self.last_buffer_raw_len, &entry.text))
        {
            self.output_lines.push(self.buffer.clone());
            self.buffer.clear();
        }
        if self.buffer.is_empty() {
            self.buffer = entry.text.clone();
        } else {
            self.buffer.push(' ');
            self.buffer.push_str(&entry.text);
        }
        self.last_buffer_raw_len = entry.raw_len;
    }

    fn flush_buffer(&mut self) {
        if !self.buffer.is_empty() {
            self.output_lines.push(self.buffer.clone());
            self.buffer.clear();
        }
        self.last_buffer_raw_len = 0;
    }

    fn finish(mut self) -> String {
        if !self.buffer.is_empty() {
            self.output_lines.push(self.buffer);
        }
        self.output_lines.join("\n")
    }
}

/// Reverse word-wrap heuristic: if the previous raw line was short enough
/// that the first word of the current line COULD have fit, the break was
/// intentional. Returns `true` when the break should be preserved.
fn should_keep_break(terminal_width: usize, prev_raw_len: usize, current_text: &str) -> bool {
    if terminal_width == 0 || prev_raw_len == 0 {
        return false;
    }
    let first_word_len = current_text
        .split_whitespace()
        .next()
        .map(|w| w.chars().count())
        .unwrap_or(0);
    // Strict < (not <=): when the sum exactly equals terminal_width, the
    // word barely fits — treat as soft wrap, not intentional break.
    first_word_len > 0 && prev_raw_len + 1 + first_word_len < terminal_width
}

/// Check if a line is a list item, ignoring leading whitespace.
fn is_list_item_with_optional_indent(line: &str) -> bool {
    let trimmed = line.trim_start();
    is_list_item(trimmed)
}

/// Known shell commands for standalone-line detection.
const SHELL_COMMANDS: &[&str] = &[
    "rm",
    "cp",
    "mv",
    "mkdir",
    "chmod",
    "ln",
    "touch",
    "cd",
    "ls",
    "open",
    "brew",
    "npm",
    "yarn",
    "pip",
    "pip3",
    "cargo",
    "go",
    "make",
    "git",
    "docker",
    "curl",
    "wget",
    "ssh",
    "scp",
    "tar",
    "sudo",
    "python",
    "python3",
    "node",
    "cat",
    "echo",
    "export",
    "source",
    "xcrun",
    "aws",
    "gcloud",
    "az",
    "kubectl",
    "terraform",
    "helm",
];

/// Check if a line looks like a standalone shell command.
///
/// Recognises both plain commands (`rm -rf /tmp/foo`) and backtick-wrapped
/// commands (`` `rm -rf /tmp/foo` ``) so that idempotent re-processing keeps
/// them on separate lines.
pub fn is_shell_command(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Handle backtick-wrapped commands for idempotency
    let inner = if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() > 2 {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let first = match inner.split_whitespace().next() {
        Some(w) => w,
        None => return false,
    };

    // Handle sudo prefix
    let cmd = if first == "sudo" {
        inner.split_whitespace().nth(1).unwrap_or("")
    } else {
        first
    };

    // Lines starting with ./ (running a local script)
    if cmd.starts_with("./") {
        return true;
    }

    if !SHELL_COMMANDS.contains(&cmd) {
        return false;
    }

    // Require at least one flag or path argument to distinguish from prose
    // like "open the file" or "make the changes".
    let word_count = inner.split_whitespace().count();
    if word_count <= 1 {
        return false;
    }

    inner.split_whitespace().skip(1).any(|w| {
        (w.starts_with('-') && w.len() > 1)
            || w.contains('/')
            || w.starts_with('~')
            || w.starts_with('$')
    })
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

/// Shell block mode: normalize line endings, join continuations, strip markers.
/// No paragraph compaction — each logical command stays on its own line.
fn ccvv_shell_block(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();

    let entries: Vec<ParagraphLine> = raw_lines
        .iter()
        .map(|line| {
            let (cleaned, indent) = clean_line_markers(line);
            ParagraphLine {
                text: cleaned,
                indent,
                raw_len: line.trim_end().chars().count(),
            }
        })
        .collect();

    let joined = join_shell_continuations(&entries);
    let mut result = String::new();
    for (i, entry) in joined.iter().enumerate() {
        if i > 0 {
            result.push('\n');
        }
        result.push_str(&entry.text);
    }
    result.trim().to_string()
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

    #[test]
    fn test_terminal_wrapped_list_continuation() {
        // Terminal-wrapped text: list item and continuation share the same indent
        let input = "  - Line 1242 (precomputed path): candidate.cleanedText compares\n  cleaned against rawText";
        let result = ccvv(input);
        assert_eq!(
            result,
            "- Line 1242 (precomputed path): candidate.cleanedText compares cleaned against rawText"
        );
    }

    #[test]
    fn test_terminal_wrapped_multiple_continuations() {
        let input =
            "  - Long list item that wraps\n  at the terminal boundary and\n  continues further";
        let result = ccvv(input);
        assert_eq!(
            result,
            "- Long list item that wraps at the terminal boundary and continues further"
        );
    }

    #[test]
    fn test_wrapped_flat_list_promoted() {
        // Agent output: first bullet at indent 0, rest at indent 2 with wrapped continuations.
        // All bullets are logically at the same level — the indent is a wrapping artifact.
        let input = "\
- Add LLM judge for command classifications that catches false\n\
  positives and missed risks\n\
  - Add shadow mode for risk-free evaluation\n\
  - Add judge accuracy dashboard with agreement rates, upgrade/downgrade\n\
  counts, and latency\n\
  - Add per-event judge column showing agreement with\n\
  confidence scores";
        let result = ccvv(input);
        assert_eq!(
            result,
            "\
- Add LLM judge for command classifications that catches false positives and missed risks\n\
- Add shadow mode for risk-free evaluation\n\
- Add judge accuracy dashboard with agreement rates, upgrade/downgrade counts, and latency\n\
- Add per-event judge column showing agreement with confidence scores"
        );
    }

    #[test]
    fn test_genuine_nested_list_preserved() {
        // Genuine nesting (no continuation before sub-bullets) should be preserved.
        let input = "- Top level item\n  - Nested item 1\n  - Nested item 2";
        let result = ccvv(input);
        assert_eq!(
            result,
            "- Top level item\n  - Nested item 1\n  - Nested item 2"
        );
    }

    #[test]
    fn test_shell_block_no_compaction() {
        let input = "rm -rf /tmp/foo\ncp -r ~/src /tmp/\nmkdir -p /tmp/out";
        let result = ccvv_shell_block(input);
        assert_eq!(
            result,
            "rm -rf /tmp/foo\ncp -r ~/src /tmp/\nmkdir -p /tmp/out"
        );
    }

    #[test]
    fn test_shell_block_joins_continuations() {
        let input =
            "aws rds wait \\\n  --db-instance-identifier staging \\\n  --region eu-central-1";
        let result = ccvv_shell_block(input);
        assert_eq!(
            result,
            "aws rds wait --db-instance-identifier staging --region eu-central-1"
        );
    }

    #[test]
    fn test_shell_block_strips_recording_dot() {
        let input = "\u{23FA} cargo build --release";
        let result = ccvv_shell_block(input);
        assert_eq!(result, "cargo build --release");
    }
}
