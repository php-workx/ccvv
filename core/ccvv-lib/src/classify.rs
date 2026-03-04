//! Content type classification heuristics.
//!
//! Determines whether clipboard content is URL, code, prose, table,
//! JSON, or mixed. Used by the pipeline and history system.
//! See §7 of the technical spec.

use crate::table_extract::extract_table;
use crate::transforms::ContentType;

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

    // Code detection: high indentation ratio or shebang
    if lines.len() > 3 {
        if trimmed.starts_with("#!") {
            return ContentType::Code;
        }
        let indented = lines
            .iter()
            .filter(|l| !l.is_empty() && l.starts_with([' ', '\t']))
            .count();
        let non_empty = lines.iter().filter(|l| !l.is_empty()).count();
        if non_empty > 0 && indented * 100 / non_empty >= 40 {
            return ContentType::Code;
        }
    }

    // Default
    if lines.len() <= 2 {
        ContentType::Prose
    } else {
        ContentType::Mixed
    }
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
}
