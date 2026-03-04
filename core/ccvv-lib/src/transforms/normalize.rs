//! Stage 2: Unicode & encoding normalization.
//!
//! Character-by-character scan with lookup tables. No regex.
//! See §5.4 Stage 2 of the technical spec.

use unicode_normalization::UnicodeNormalization;

use super::{RuleFired, Transform, TransformContext};

/// Common mojibake patterns: (mojibake sequence, correct character).
/// These are Latin-1-as-UTF-8 double-encoding patterns.
const MOJIBAKE_TABLE: &[(&str, &str)] = &[
    ("\u{00C3}\u{00A9}", "\u{00E9}"),         // Ã© → é
    ("\u{00C3}\u{00BC}", "\u{00FC}"),         // Ã¼ → ü
    ("\u{00C3}\u{00B6}", "\u{00F6}"),         // Ã¶ → ö
    ("\u{00C3}\u{00A4}", "\u{00E4}"),         // Ã¤ → ä
    ("\u{00C3}\u{00A8}", "\u{00E8}"),         // Ã¨ → è
    ("\u{00C3}\u{00AA}", "\u{00EA}"),         // Ã© → ê
    ("\u{00C3}\u{00AB}", "\u{00EB}"),         // Ã« → ë
    ("\u{00C3}\u{00AF}", "\u{00EF}"),         // Ã¯ → ï
    ("\u{00C3}\u{00B4}", "\u{00F4}"),         // Ã´ → ô
    ("\u{00C3}\u{00BB}", "\u{00FB}"),         // Ã» → û
    ("\u{00C3}\u{00A7}", "\u{00E7}"),         // Ã§ → ç
    ("\u{00C3}\u{00A0}", "\u{00E0}"),         // Ã  → à
    ("\u{00C3}\u{00A2}", "\u{00E2}"),         // Ã¢ → â
    ("\u{00C3}\u{00AE}", "\u{00EE}"),         // Ã® → î
    ("\u{00C3}\u{00B1}", "\u{00F1}"),         // Ã± → ñ
    ("\u{00C3}\u{0089}", "\u{00C9}"),         // Ã‰ → É
    ("\u{00C3}\u{0080}", "\u{00C0}"),         // Ã€ → À
    ("\u{00C3}\u{009C}", "\u{00DC}"),         // Ãœ → Ü
    ("\u{00C3}\u{0096}", "\u{00D6}"),         // Ã– → Ö
    ("\u{00C3}\u{0084}", "\u{00C4}"),         // Ã„ → Ä
    ("\u{00C2}\u{00A0}", "\u{00A0}"),         // Â  → NBSP (then normalized to space below)
    ("\u{00C2}\u{00AB}", "\u{00AB}"),         // Â« → «
    ("\u{00C2}\u{00BB}", "\u{00BB}"),         // Â» → »
    ("\u{00C2}\u{00B0}", "\u{00B0}"),         // Â° → °
    ("\u{00C2}\u{00A3}", "\u{00A3}"),         // Â£ → £
    ("\u{00C2}\u{00A5}", "\u{00A5}"),         // Â¥ → ¥
    ("\u{00C2}\u{00A9}", "\u{00A9}"),         // Â© → ©
    ("\u{00C2}\u{00AE}", "\u{00AE}"),         // Â® → ®
    ("\u{00E2}\u{0080}\u{0099}", "\u{2019}"), // â€™ → ' (right single quote, will be normalized below)
    ("\u{00E2}\u{0080}\u{009C}", "\u{201C}"), // â€œ → " (left double quote, will be normalized below)
    ("\u{00E2}\u{0080}\u{009D}", "\u{201D}"), // â€ → " (right double quote, will be normalized below)
];

/// Unicode normalization transform (Stage 2).
pub struct NormalizeTransform {
    /// Em-dash replacement string. Empty string means preserve em-dash as-is.
    pub em_dash_replacement: String,
}

impl NormalizeTransform {
    pub fn new() -> Self {
        NormalizeTransform {
            em_dash_replacement: "--".to_string(),
        }
    }

    /// Create with custom em-dash replacement string.
    /// Use an empty string to preserve em-dashes unchanged.
    pub fn with_em_dash_replacement(mut self, replacement: &str) -> Self {
        self.em_dash_replacement = replacement.to_string();
        self
    }
}

impl Default for NormalizeTransform {
    fn default() -> Self {
        Self::new()
    }
}

impl Transform for NormalizeTransform {
    fn name(&self) -> &'static str {
        "normalize_unicode"
    }

    fn apply(&self, input: &str, ctx: &mut TransformContext) -> String {
        let original_len = input.len();

        // Step 1: Mojibake repair (paragraph-aware, threshold of 3)
        let mut result = repair_mojibake(input, ctx);

        // Step 2: Character-by-character normalization
        result = normalize_characters(&result, &self.em_dash_replacement);

        // Step 3: NFC normalization
        result = result.nfc().collect::<String>();

        let chars_changed = if result.len() != original_len {
            original_len.abs_diff(result.len())
        } else {
            // Check if content actually changed even if length is same
            if result != input {
                1
            } else {
                0
            }
        };

        if chars_changed > 0 {
            ctx.rules_fired.push(RuleFired {
                stage: "normalize_unicode",
                description: format!("normalized {} bytes of unicode content", chars_changed),
                chars_changed,
            });
        }

        result
    }
}

/// Repair mojibake patterns in text, paragraph by paragraph.
/// Only applies within a paragraph if >= 3 patterns match.
fn repair_mojibake(input: &str, ctx: &mut TransformContext) -> String {
    let paragraphs: Vec<&str> = input.split("\n\n").collect();
    let mut repaired_paragraphs: Vec<String> = Vec::new();
    let mut total_repairs = 0;

    for paragraph in &paragraphs {
        // Count matching patterns in this paragraph
        let match_count: usize = MOJIBAKE_TABLE
            .iter()
            .filter(|(pattern, _)| paragraph.contains(pattern))
            .count();

        if match_count >= 3 {
            let mut repaired = paragraph.to_string();
            for (pattern, replacement) in MOJIBAKE_TABLE {
                repaired = repaired.replace(pattern, replacement);
            }
            total_repairs += match_count;
            repaired_paragraphs.push(repaired);
        } else {
            repaired_paragraphs.push(paragraph.to_string());
        }
    }

    if total_repairs > 0 {
        ctx.rules_fired.push(RuleFired {
            stage: "normalize_unicode",
            description: format!("repaired {} mojibake sequences", total_repairs),
            chars_changed: total_repairs,
        });
    }

    repaired_paragraphs.join("\n\n")
}

/// Normalize individual characters.
fn normalize_characters(input: &str, em_dash_replacement: &str) -> String {
    let mut result = String::with_capacity(input.len());

    for ch in input.chars() {
        match ch {
            // Curly double quotes → straight
            '\u{201C}' | '\u{201D}' => result.push('"'),
            // Curly single quotes → straight
            '\u{2018}' | '\u{2019}' => result.push('\''),
            // Em-dash → configurable replacement
            '\u{2014}' => {
                if !em_dash_replacement.is_empty() {
                    result.push_str(em_dash_replacement);
                } else {
                    result.push(ch);
                }
            }
            // En-dash → hyphen
            '\u{2013}' => result.push('-'),
            // Non-breaking space → regular space
            '\u{00A0}' => result.push(' '),
            // Zero-width space
            '\u{200B}' => {}
            // Zero-width non-joiner
            '\u{200C}' => {}
            // Zero-width joiner
            '\u{200D}' => {}
            // Byte-order mark
            '\u{FEFF}' => {}
            // Everything else passes through
            _ => result.push(ch),
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_curly_quotes_to_straight() {
        let input = "\u{201C}Hello\u{201D} \u{2018}World\u{2019}";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "\"Hello\" 'World'");
    }

    #[test]
    fn test_em_dash_replacement() {
        let input = "Hello\u{2014}World";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Hello--World");
    }

    #[test]
    fn test_em_dash_preserved() {
        let input = "Hello\u{2014}World";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new().with_em_dash_replacement("");
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Hello\u{2014}World");
    }

    #[test]
    fn test_em_dash_custom_replacement() {
        let input = "Hello\u{2014}World";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new().with_em_dash_replacement("\u{2014}");
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Hello\u{2014}World");
    }

    #[test]
    fn test_en_dash_replacement() {
        let input = "pages 1\u{2013}10";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "pages 1-10");
    }

    #[test]
    fn test_zero_width_removal() {
        let input = "Hello\u{200B}World\u{200C}Test\u{200D}End\u{FEFF}!";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "HelloWorldTestEnd!");
    }

    #[test]
    fn test_nbsp_to_space() {
        let input = "Hello\u{00A0}World";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_nfc_normalization() {
        // e + combining acute accent should become é
        let input = "caf\u{0065}\u{0301}";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "caf\u{00E9}");
    }

    #[test]
    fn test_normal_text_unchanged() {
        let input = "Hello world. Normal text.";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let result = transform.apply(input, &mut ctx);
        assert_eq!(result, "Hello world. Normal text.");
    }

    #[test]
    fn test_idempotency() {
        let input = "\u{201C}Hello\u{201D}\u{2014}World\u{200B}!";
        let mut ctx = TransformContext::default();
        let transform = NormalizeTransform::new();
        let first = transform.apply(input, &mut ctx);
        let mut ctx2 = TransformContext::default();
        let second = transform.apply(&first, &mut ctx2);
        assert_eq!(first, second, "Normalize transform must be idempotent");
    }
}
