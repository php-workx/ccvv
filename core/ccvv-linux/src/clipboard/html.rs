pub const MAX_HTML_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HtmlExtractError {
    Oversized,
    Malformed,
}

pub fn extract_plain_text_from_html(html: &str) -> Result<String, HtmlExtractError> {
    if html.len() > MAX_HTML_BYTES {
        return Err(HtmlExtractError::Oversized);
    }
    if html.contains('<') && !html.contains('>') {
        return Err(HtmlExtractError::Malformed);
    }

    let mut output = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    let mut preserve_whitespace = false;

    for character in html.chars() {
        match character {
            '<' if !in_tag => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                let normalized = tag.trim().to_ascii_lowercase();
                if normalized.starts_with("pre") || normalized.starts_with("code") {
                    preserve_whitespace = true;
                } else if normalized.starts_with("/pre") || normalized.starts_with("/code") {
                    preserve_whitespace = false;
                } else if normalized == "br"
                    || normalized == "/p"
                    || normalized == "p"
                    || normalized == "/div"
                    || normalized == "div"
                {
                    push_newline(&mut output);
                }
                in_tag = false;
            }
            _ if in_tag => tag.push(character),
            '\n' if preserve_whitespace => output.push('\n'),
            value if preserve_whitespace => output.push(value),
            value if value.is_whitespace() => push_space(&mut output),
            value => output.push(value),
        }
    }

    if in_tag {
        return Err(HtmlExtractError::Malformed);
    }

    Ok(html_unescape(output.trim()).trim().to_string())
}

fn push_space(output: &mut String) {
    if !output.ends_with([' ', '\n']) {
        output.push(' ');
    }
}

fn push_newline(output: &mut String) {
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
}

#[cfg(test)]
mod tests {
    use super::{extract_plain_text_from_html, HtmlExtractError, MAX_HTML_BYTES};

    #[test]
    fn test_extracts_plain_text_from_simple_html() {
        let plain = extract_plain_text_from_html("<p>Hello <strong>world</strong></p>").unwrap();

        assert_eq!(plain, "Hello world");
    }

    #[test]
    fn test_oversized_html_is_rejected() {
        let html = "a".repeat(MAX_HTML_BYTES + 1);

        assert_eq!(
            extract_plain_text_from_html(&html),
            Err(HtmlExtractError::Oversized)
        );
    }

    #[test]
    fn test_malformed_html_is_rejected() {
        assert_eq!(
            extract_plain_text_from_html("<div"),
            Err(HtmlExtractError::Malformed)
        );
    }

    #[test]
    fn test_code_and_pre_blocks_preserve_spacing() {
        let html = "<pre>fn main() {\n    println!(&quot;hi&quot;);\n}</pre>";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(plain, "fn main() {\n    println!(\"hi\");\n}");
    }
}
