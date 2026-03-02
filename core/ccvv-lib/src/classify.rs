//! Content type classification heuristics.
//!
//! Determines whether clipboard content is URL, code, prose, table,
//! JSON, or mixed. Used by the pipeline and history system.
//! See §7 of the technical spec.

use crate::transforms::ContentType;

/// Classify the content type of the given text.
pub fn classify(text: &str) -> ContentType {
    let trimmed = text.trim();

    // JSON detection: entire content parses as JSON
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return ContentType::Json;
    }

    // Single URL check
    if trimmed.lines().count() == 1
        && (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.contains(' ')
    {
        return ContentType::Url;
    }

    // Table detection: check for consistent delimiter-separated values
    let lines: Vec<&str> = trimmed.lines().collect();
    if lines.len() >= 2 {
        for delimiter in ['\t', ',', ';'] {
            let counts: Vec<usize> = lines.iter().map(|l| l.matches(delimiter).count()).collect();
            if let Some(&first) = counts.first() {
                if first >= 1 && counts.iter().filter(|&&c| c == first).count() * 100 / counts.len() >= 80 {
                    return ContentType::Table;
                }
            }
        }
    }

    // Code detection: high indentation ratio or shebang
    if lines.len() > 3 {
        if trimmed.starts_with("#!") {
            return ContentType::Code;
        }
        let indented = lines.iter().filter(|l| !l.is_empty() && l.starts_with(|c: char| c == ' ' || c == '\t')).count();
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
    fn test_classify_code_shebang() {
        let code = "#!/bin/bash\necho hello\necho world\necho done";
        assert_eq!(classify(code), ContentType::Code);
    }
}
