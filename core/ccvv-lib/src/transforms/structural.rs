//! Stage 5: Structural detection (JSON, table, code fence).
//!
//! Three sub-detectors tried in order: JSON, table, code fence.
//! See §5.4 Stage 5 of the technical spec.

use crate::table_extract::split_csv_line;

use super::{ContentType, RuleFired, Transform, TransformContext};

/// Structural detection transform (Stage 5).
pub struct StructuralTransform {
    max_output_ratio: f64,
}

impl StructuralTransform {
    pub fn new() -> Self {
        StructuralTransform {
            max_output_ratio: 2.0,
        }
    }
}

impl Default for StructuralTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for StructuralTransform {
    fn name(&self) -> &'static str {
        "structural_detection"
    }

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        // Skip structural detection for shell blocks and lists —
        // these should not be JSON-prettified, table-converted, or fence-wrapped
        if matches!(
            ctx.content_type,
            Some(ContentType::ShellBlock | ContentType::List)
        ) {
            return input.to_string();
        }

        let trimmed = input.trim();

        // Already fenced content — skip
        if trimmed.starts_with("```") && trimmed.ends_with("```") {
            return input.to_string();
        }

        // JSON detection: entire input must parse as valid JSON
        if let Some(result) = self.try_json(trimmed, ctx) {
            return result;
        }

        // Table detection
        if let Some(result) = self.try_table(trimmed, input, ctx) {
            return result;
        }

        // Code fence wrapping
        if let Some(result) = self.try_code_fence(trimmed, ctx) {
            return result;
        }

        input.to_string()
    }
}

impl StructuralTransform {
    /// Attempt JSON detection and prettification.
    fn try_json(&self, input: &str, ctx: &mut TransformContext) -> Option<String> {
        let value: serde_json::Value = serde_json::from_str(input).ok()?;

        // Only trigger on objects and arrays, not primitives
        if !value.is_object() && !value.is_array() {
            return None;
        }

        let pretty = serde_json::to_string_pretty(&value).ok()?;

        // Check expansion ratio
        if !input.is_empty() && pretty.len() as f64 / input.len() as f64 > self.max_output_ratio {
            return None;
        }

        ctx.content_type = Some(ContentType::Json);
        ctx.rules_fired.push(RuleFired {
            stage: "structural_detection",
            description: "prettified JSON".to_string(),
            chars_changed: pretty.len().abs_diff(input.len()),
        });

        Some(pretty)
    }

    /// Attempt table detection.
    fn try_table(&self, input: &str, original: &str, ctx: &mut TransformContext) -> Option<String> {
        let lines: Vec<&str> = input.lines().collect();
        if lines.len() < 2 {
            return None;
        }

        // Try each delimiter in order: tab, comma, semicolon
        for delimiter in ['\t', ',', ';'] {
            if let Some(table) = self.try_delimiter_table(&lines, delimiter) {
                // Check expansion ratio
                if !original.is_empty()
                    && table.len() as f64 / original.len() as f64 > self.max_output_ratio
                {
                    return None;
                }

                ctx.content_type = Some(ContentType::Table);
                ctx.rules_fired.push(RuleFired {
                    stage: "structural_detection",
                    description: format!(
                        "converted {}-delimited data to Markdown table",
                        match delimiter {
                            '\t' => "tab",
                            ',' => "comma",
                            ';' => "semicolon",
                            _ => "unknown",
                        }
                    ),
                    chars_changed: table.len().abs_diff(original.len()),
                });

                return Some(table);
            }
        }

        None
    }

    /// Try to parse lines as a delimiter-separated table.
    fn try_delimiter_table(&self, lines: &[&str], delimiter: char) -> Option<String> {
        let split_lines: Vec<Vec<String>> = lines
            .iter()
            .map(|line| split_csv_line(line, delimiter))
            .collect();

        if split_lines.is_empty() {
            return None;
        }

        // col_counts is the number of fields (columns) in each row
        let col_counts: Vec<usize> = split_lines.iter().map(|row| row.len()).collect();

        // Find the most common column count
        let mut freq = std::collections::HashMap::new();
        for &count in &col_counts {
            *freq.entry(count).or_insert(0) += 1;
        }
        let most_common = *freq
            .iter()
            .max_by_key(|(_, &f)| f)
            .map(|(count, _)| count)?;

        if most_common < 2 {
            return None;
        }

        // Check that >= 80% of lines match
        let matching = col_counts.iter().filter(|&&c| c == most_common).count();
        if matching * 100 / col_counts.len() < 80 {
            return None;
        }

        // Build Markdown table
        let rows: Vec<&Vec<String>> = split_lines
            .iter()
            .filter(|row| row.len() == most_common)
            .collect();

        if rows.is_empty() {
            return None;
        }

        // Header inference: first row is header if values are distinct and < 80% numeric
        let header = &rows[0];
        let is_header = {
            let unique: std::collections::HashSet<&str> =
                header.iter().map(|s| s.as_str()).collect();
            let distinct = unique.len() == header.len();
            let numeric_count = header
                .iter()
                .filter(|v| v.trim().parse::<f64>().is_ok())
                .count();
            distinct && (numeric_count * 100 / header.len().max(1) < 80)
        };

        let mut output = String::new();

        if is_header {
            // Use first row as header
            output.push_str("| ");
            output.push_str(
                &header
                    .iter()
                    .map(|s| s.trim())
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            output.push_str(" |\n");

            // Separator
            output.push_str("| ");
            output.push_str(&header.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));
            output.push_str(" |\n");

            // Data rows
            for row in rows.iter().skip(1) {
                output.push_str("| ");
                output.push_str(&row.iter().map(|s| s.trim()).collect::<Vec<_>>().join(" | "));
                output.push_str(" |\n");
            }
        } else {
            // Generate Col 1, Col 2, ... header
            let headers: Vec<String> = (1..=most_common).map(|i| format!("Col {}", i)).collect();
            output.push_str("| ");
            output.push_str(&headers.join(" | "));
            output.push_str(" |\n");

            output.push_str("| ");
            output.push_str(
                &headers
                    .iter()
                    .map(|_| "---")
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            output.push_str(" |\n");

            for row in &rows {
                output.push_str("| ");
                output.push_str(&row.iter().map(|s| s.trim()).collect::<Vec<_>>().join(" | "));
                output.push_str(" |\n");
            }
        }

        // Remove trailing newline
        if output.ends_with('\n') {
            output.pop();
        }

        Some(output)
    }

    /// Attempt code fence wrapping.
    ///
    /// Uses a multi-signal scoring system to avoid false positives on
    /// indented prose (common in CLI-copied text). Shebang is unambiguous
    /// and always triggers. Otherwise, requires ≥2 positive signals and
    /// no strong prose signal.
    fn try_code_fence(&self, input: &str, ctx: &mut TransformContext) -> Option<String> {
        let lines: Vec<&str> = input.lines().collect();
        if lines.len() <= 3 {
            return None;
        }

        // Skip if input already contains code fence lines (avoid double-wrapping)
        if lines.iter().any(|l| l.trim().starts_with("```")) {
            return None;
        }

        let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.is_empty()).collect();
        if non_empty.is_empty() {
            return None;
        }

        let has_shebang = input.starts_with("#!");
        let lang = detect_language(input);

        if !has_shebang && !passes_code_score(&non_empty, &lang) {
            return None;
        }

        let lang_hint = lang.unwrap_or("");

        ctx.content_type = Some(ContentType::Code);
        ctx.rules_fired.push(RuleFired {
            stage: "structural_detection",
            description: format!(
                "wrapped code block{}",
                if lang_hint.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", lang_hint)
                }
            ),
            chars_changed: 6 + lang_hint.len(), // ``` + lang + \n + ```
        });

        Some(format!("```{}\n{}\n```", lang_hint, input))
    }
}

/// Detect programming language from keyword frequency.
fn detect_language(input: &str) -> Option<&'static str> {
    let mut scores: Vec<(&str, usize)> = vec![
        ("python", 0),
        ("rust", 0),
        ("javascript", 0),
        ("swift", 0),
        ("go", 0),
        ("java", 0),
    ];

    let keywords: &[(&[&str], usize)] = &[
        // Python
        (
            &[
                "def ", "import ", "print(", "class ", "elif ", "self.", "from ",
            ],
            0,
        ),
        // Rust
        (
            &[
                "fn ",
                "let mut ",
                "impl ",
                "use std::",
                "pub fn ",
                "match ",
                "&self",
                "println!",
                "::",
            ],
            1,
        ),
        // JavaScript
        (
            &[
                "function ",
                "const ",
                "=> ",
                "console.",
                "require(",
                "module.exports",
            ],
            2,
        ),
        // Swift
        (
            &[
                "func ",
                "var ",
                "import Foundation",
                "guard let",
                "@objc",
                "NSObject",
            ],
            3,
        ),
        // Go
        (&["package ", "fmt.", "go ", ":= ", "interface{"], 4),
        // Java
        (
            &[
                "public class",
                "System.out",
                "private ",
                "protected ",
                "@Override",
            ],
            5,
        ),
    ];

    for (patterns, lang_idx) in keywords {
        for pattern in *patterns {
            let count = input.matches(pattern).count();
            scores[*lang_idx].1 += count;
        }
    }

    let best = scores.iter().max_by_key(|(_, score)| *score)?;
    if best.1 >= 2 {
        Some(best.0)
    } else {
        None
    }
}

/// Multi-signal scoring for code fence detection. Returns `true` when ≥2
/// positive signals are present and prose doesn't veto.
fn passes_code_score(non_empty: &[&&str], lang: &Option<&str>) -> bool {
    let mut score: i32 = 0;

    // +1: high indentation ratio
    let indented_count = non_empty
        .iter()
        .filter(|l| l.starts_with(' ') || l.starts_with('\t'))
        .count();
    if indented_count * 100 / non_empty.len() >= 40 {
        score += 1;
    }

    // +1: language keywords detected
    if lang.is_some() {
        score += 1;
    }

    // +1: syntax density ({, }, ; on ≥15% of lines)
    let syntax_lines = non_empty
        .iter()
        .filter(|l| l.contains('{') || l.contains('}') || l.contains(';'))
        .count();
    if syntax_lines * 100 / non_empty.len() >= 15 {
        score += 1;
    }

    // -2: prose signal (≥30% of lines end with sentence punctuation)
    let prose_lines = non_empty
        .iter()
        .filter(|l| {
            let t = l.trim();
            t.ends_with('.') || t.ends_with('?') || t.ends_with('!')
        })
        .count();
    if prose_lines * 100 / non_empty.len() >= 30 {
        score -= 2;
    }

    score >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_prettify() {
        let input = r#"{"key":"value","nested":{"a":1}}"#;
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.contains("\"key\": \"value\""));
        assert!(result.contains('\n'));
        assert_eq!(ctx.content_type, Some(ContentType::Json));
    }

    #[test]
    fn test_json_passthrough() {
        let input = "This is not JSON, just text.";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input);
    }

    #[test]
    fn test_json_primitive_passthrough() {
        let input = "42";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "42");
    }

    #[test]
    fn test_table_detection_tsv() {
        // Use longer column values so the Markdown table doesn't exceed 2x expansion
        let input = "First Name\tAge\tCity Location\nAlice Smith\t30\tNew York City\nBob Jones\t25\tLos Angeles\nCharlie Brown\t35\tSan Francisco\nDave Wilson\t28\tChicago Illinois";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(
            result.contains("| First Name"),
            "Expected table header, got: {}",
            result
        );
        assert!(result.contains("| ---"));
        assert!(result.contains("| Alice Smith"));
        assert_eq!(ctx.content_type, Some(ContentType::Table));
    }

    #[test]
    fn test_table_detection_csv() {
        // Use longer values to avoid 2x expansion guard
        let input = "Full Name,Age,City Location,Country\nAlice Smith,30,New York,USA\nBob Jones,25,Los Angeles,USA\nCharlie Brown,35,San Francisco,USA";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(
            result.contains("| Full Name"),
            "Expected table header, got: {}",
            result
        );
        assert!(result.contains("| Alice Smith"));
    }

    #[test]
    fn test_code_fence_wrapping() {
        let input = "    def foo():\n        print('hello')\n        x = 1\n        y = 2\n        return x + y";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(result.starts_with("```python"));
        assert!(result.ends_with("```"));
        assert_eq!(ctx.content_type, Some(ContentType::Code));
    }

    #[test]
    fn test_language_detection_rust() {
        let code = "fn main() {\n    let mut x = 5;\n    println!(\"{}\", x);\n    let y = 10;\n    x = y;\n}";
        let lang = detect_language(code);
        assert_eq!(lang, Some("rust"));
    }

    #[test]
    fn test_language_detection_python() {
        let code = "def hello():\n    print('world')\n    import os\n    return 42";
        let lang = detect_language(code);
        assert_eq!(lang, Some("python"));
    }

    #[test]
    fn test_already_fenced_skipped() {
        let input = "```rust\nfn main() {}\n```";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input);
    }

    #[test]
    fn test_try_delimiter_table_direct() {
        let lines = vec!["Name\tAge\tCity", "Alice\t30\tNYC", "Bob\t25\tLA"];
        let transform = StructuralTransform::new();
        let result = transform.try_delimiter_table(&lines, '\t');
        assert!(
            result.is_some(),
            "Expected table output from tab-delimited data"
        );
        let table = result.unwrap();
        assert!(table.contains("| Name"), "Table: {}", table);
    }

    #[test]
    fn test_csv_quoted_fields() {
        let fields = split_csv_line(r#"Alice,"New York, NY",30"#, ',');
        assert_eq!(fields, vec!["Alice", "New York, NY", "30"]);
    }

    #[test]
    fn test_idempotency_json() {
        let input = r#"{"a":1,"b":2}"#;
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(
            first, second,
            "Structural JSON transform must be idempotent"
        );
    }

    #[test]
    fn test_skip_for_shell_block() {
        let input = "    def foo():\n        print('hello')\n        x = 1\n        y = 2\n        return x + y";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::ShellBlock),
            ..Default::default()
        };
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input, "ShellBlock should skip structural detection");
    }

    #[test]
    fn test_skip_for_list() {
        let input = "    def foo():\n        print('hello')\n        x = 1\n        y = 2\n        return x + y";
        let mut ctx = TransformContext {
            content_type: Some(ContentType::List),
            ..Default::default()
        };
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, input, "List should skip structural detection");
    }

    #[test]
    fn test_indented_prose_not_fenced() {
        // Indented prose from CLI with "from" and "import" as English words
        // should NOT be fence-wrapped despite indentation + keyword matches
        let input = "  1. EKS endpoint made private.\n  The change broke kubectl from outside the VPC.\n\n  2. Lambda moved into the VPC.\n  This required importing VPC properties from the cluster.\n\n  3. CDK auto-exported those properties.\n  The merge conflict sealed the trap.";
        let mut ctx = TransformContext::default();
        let transform = StructuralTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert!(
            !result.starts_with("```"),
            "Indented prose should not be fence-wrapped, got: {}",
            &result[..result.len().min(80)]
        );
    }
}
