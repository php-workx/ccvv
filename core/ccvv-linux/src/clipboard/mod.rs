#![allow(dead_code)]

use std::collections::HashMap;

use crate::clipboard::html::{extract_plain_text_from_html, HtmlExtractError};
use crate::clipboard::targets::{pick_text_target, HtmlPreference, TEXT_HTML};

pub mod html;
pub mod targets;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextCandidate {
    pub plain_text: String,
    pub html: Option<String>,
    pub source_mime: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquisitionError {
    NoTextTarget,
}

pub fn acquire_text_candidate(
    offers: &HashMap<String, Vec<u8>>,
    html_preference: HtmlPreference,
) -> Result<TextCandidate, AcquisitionError> {
    if matches!(html_preference, HtmlPreference::PreferHtml) {
        if let Some(html_bytes) = offers.get(TEXT_HTML) {
            let html = String::from_utf8_lossy(html_bytes).into_owned();
            match extract_plain_text_from_html(&html) {
                Ok(extracted) => {
                    return Ok(TextCandidate {
                        plain_text: extracted,
                        html: Some(html),
                        source_mime: TEXT_HTML.to_string(),
                    });
                }
                Err(HtmlExtractError::Oversized) | Err(HtmlExtractError::Malformed) => {}
            }
        }
    }

    let available_targets: Vec<&str> = offers.keys().map(String::as_str).collect();
    let target = pick_text_target(&available_targets).ok_or(AcquisitionError::NoTextTarget)?;
    let plain_text =
        String::from_utf8_lossy(offers.get(target).ok_or(AcquisitionError::NoTextTarget)?)
            .into_owned();

    Ok(TextCandidate {
        plain_text,
        html: None,
        source_mime: target.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{acquire_text_candidate, AcquisitionError};
    use crate::clipboard::html::MAX_HTML_BYTES;
    use crate::clipboard::targets::{HtmlPreference, TEXT_HTML, UTF8_STRING};

    #[test]
    fn test_html_is_preferred_when_extractable() {
        let mut offers = HashMap::new();
        offers.insert(
            TEXT_HTML.to_string(),
            b"<p>Hello <strong>world</strong></p>".to_vec(),
        );
        offers.insert(UTF8_STRING.to_string(), b"Hello world".to_vec());

        let candidate = acquire_text_candidate(&offers, HtmlPreference::PreferHtml).unwrap();

        assert_eq!(candidate.plain_text, "Hello world");
        assert_eq!(candidate.source_mime, TEXT_HTML);
        assert!(candidate.html.is_some());
    }

    #[test]
    fn test_oversized_html_falls_back_to_plain_text() {
        let mut offers = HashMap::new();
        offers.insert(TEXT_HTML.to_string(), vec![b'a'; MAX_HTML_BYTES + 1]);
        offers.insert(UTF8_STRING.to_string(), b"plain text".to_vec());

        let candidate = acquire_text_candidate(&offers, HtmlPreference::PreferHtml).unwrap();

        assert_eq!(candidate.plain_text, "plain text");
        assert_eq!(candidate.source_mime, UTF8_STRING);
        assert!(candidate.html.is_none());
    }

    #[test]
    fn test_no_text_target_errors() {
        let offers = HashMap::new();

        let error = acquire_text_candidate(&offers, HtmlPreference::PreferHtml).unwrap_err();

        assert_eq!(error, AcquisitionError::NoTextTarget);
    }
}
