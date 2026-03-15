pub const TEXT_HTML: &str = "text/html";
pub const UTF8_PLAIN: &str = "text/plain;charset=utf-8";
pub const UTF8_STRING: &str = "UTF8_STRING";
pub const TEXT_PLAIN: &str = "text/plain";
pub const STRING: &str = "STRING";
pub const TEXT: &str = "TEXT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HtmlPreference {
    PreferHtml,
    PlainTextOnly,
}

pub fn pick_text_target(targets: &[&str]) -> Option<&'static str> {
    [UTF8_PLAIN, UTF8_STRING, TEXT_PLAIN, TEXT, STRING]
        .into_iter()
        .find(|candidate| targets.iter().any(|target| target == candidate))
}

#[cfg(test)]
mod tests {
    use super::{pick_text_target, STRING, TEXT, TEXT_PLAIN, UTF8_PLAIN, UTF8_STRING};

    #[test]
    fn test_prefers_utf8_plain_targets_first() {
        let target = pick_text_target(&[STRING, TEXT, UTF8_STRING, UTF8_PLAIN]).unwrap();

        assert_eq!(target, UTF8_PLAIN);
    }

    #[test]
    fn test_returns_none_when_no_text_target_exists() {
        assert_eq!(pick_text_target(&["image/png", "text/uri-list"]), None);
    }

    #[test]
    fn test_prefers_text_plain_before_legacy_targets() {
        let target = pick_text_target(&[STRING, TEXT, TEXT_PLAIN]).unwrap();

        assert_eq!(target, TEXT_PLAIN);
    }
}
