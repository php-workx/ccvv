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
                let tag_name = normalized
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('/');
                if tag_name == "pre" || tag_name == "code" {
                    preserve_whitespace = true;
                } else if tag_name == "/pre" || tag_name == "/code" {
                    preserve_whitespace = false;
                } else if matches!(tag_name, "br" | "/p" | "p" | "/div" | "div") {
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
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '&' {
            output.push(ch);
            continue;
        }

        // Collect entity up to ';'
        let mut entity = String::new();
        let mut found_semi = false;
        for _ in 0..10 {
            match chars.peek() {
                Some(&';') => {
                    chars.next();
                    found_semi = true;
                    break;
                }
                Some(_) => entity.push(chars.next().unwrap()),
                None => break,
            }
        }

        if !found_semi {
            output.push('&');
            output.push_str(&entity);
            continue;
        }

        match entity.as_str() {
            "nbsp" => output.push(' '),
            "lt" => output.push('<'),
            "gt" => output.push('>'),
            "amp" => output.push('&'),
            "quot" => output.push('"'),
            "apos" => output.push('\''),
            _ if entity.starts_with('#') => {
                let code_point = if entity.starts_with("#x") || entity.starts_with("#X") {
                    u32::from_str_radix(&entity[2..], 16).ok()
                } else {
                    entity[1..].parse::<u32>().ok()
                };
                match code_point.and_then(char::from_u32) {
                    Some(decoded) => output.push(decoded),
                    None => {
                        output.push('&');
                        output.push_str(&entity);
                        output.push(';');
                    }
                }
            }
            _ => {
                output.push('&');
                output.push_str(&entity);
                output.push(';');
            }
        }
    }

    output
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

    #[test]
    fn test_extracts_entities_and_block_boundaries() {
        let html = "<div class=\"lead\">Tom &amp; Jerry</div><p>3 &lt; 5&nbsp;times</p><br/><div>&#x1F642;</div>";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(plain, "Tom & Jerry\n3 < 5 times\n🙂");
    }

    #[test]
    fn test_pre_block_keeps_spacing_between_surrounding_blocks() {
        let html =
            "<div>Before</div><pre>fn main() {\n    println!(&quot;hi&quot;);\n}</pre><div>After</div>";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(
            plain,
            "Before\nfn main() {\n    println!(\"hi\");\n}\nAfter"
        );
    }
}
