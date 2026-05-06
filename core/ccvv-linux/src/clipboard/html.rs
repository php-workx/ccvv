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
    // Depth of nested inline-code-like tags (<code>, <kbd>, <samp>, <tt>) that
    // are NOT inside a <pre>; while > 0, the surrounding output is wrapped
    // in backticks so the resulting plain text keeps the inline-code hint.
    // <pre> still owns whitespace preservation; <pre><code> does not double-wrap.
    let mut inline_code_depth: u32 = 0;
    // Depth of <pre> blocks. While > 0 we preserve whitespace and skip the
    // inline backtick wrapping (block-level code preserves layout instead).
    let mut pre_depth: u32 = 0;

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
                match tag_name {
                    "pre" => {
                        pre_depth = pre_depth.saturating_add(1);
                        preserve_whitespace = true;
                    }
                    "/pre" => {
                        pre_depth = pre_depth.saturating_sub(1);
                        if pre_depth == 0 {
                            preserve_whitespace = false;
                        }
                    }
                    "code" | "kbd" | "samp" | "tt" => {
                        if pre_depth == 0 {
                            inline_code_depth = inline_code_depth.saturating_add(1);
                            if inline_code_depth == 1 {
                                output.push('`');
                            }
                        }
                    }
                    "/code" | "/kbd" | "/samp" | "/tt" => {
                        if pre_depth == 0 && inline_code_depth > 0 {
                            inline_code_depth -= 1;
                            if inline_code_depth == 0 {
                                output.push('`');
                            }
                        }
                    }
                    "br" | "/p" | "p" | "/div" | "div" => {
                        push_newline(&mut output);
                    }
                    _ => {}
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

enum EntityDecode {
    Decoded(char),
    Literal(String),
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

        match collect_entity(&mut chars) {
            None => output.push('&'),
            Some(entity) => match decode_entity(&entity) {
                EntityDecode::Decoded(decoded) => output.push(decoded),
                EntityDecode::Literal(literal) => output.push_str(&literal),
            },
        }
    }

    output
}

fn collect_entity(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut entity = String::new();
    for _ in 0..10 {
        match chars.peek() {
            Some(&';') => {
                chars.next();
                return Some(entity);
            }
            Some(_) => entity.push(chars.next().expect("peeked entity character missing")),
            None => return None,
        }
    }
    None
}

fn decode_entity(entity: &str) -> EntityDecode {
    match entity {
        "nbsp" => EntityDecode::Decoded(' '),
        "lt" => EntityDecode::Decoded('<'),
        "gt" => EntityDecode::Decoded('>'),
        "amp" => EntityDecode::Decoded('&'),
        "quot" => EntityDecode::Decoded('"'),
        "apos" => EntityDecode::Decoded('\''),
        _ if entity.starts_with('#') => decode_numeric_entity(entity)
            .map(EntityDecode::Decoded)
            .unwrap_or_else(|| EntityDecode::Literal(format!("&{entity};"))),
        _ => EntityDecode::Literal(format!("&{entity};")),
    }
}

fn decode_numeric_entity(entity: &str) -> Option<char> {
    let code_point = if entity.starts_with("#x") || entity.starts_with("#X") {
        u32::from_str_radix(&entity[2..], 16).ok()
    } else {
        entity[1..].parse::<u32>().ok()
    };
    code_point.and_then(char::from_u32)
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

    #[test]
    fn test_inline_code_tag_wraps_in_backticks() {
        let html = "Use <code>cargo build</code> to compile.";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(plain, "Use `cargo build` to compile.");
    }

    #[test]
    fn test_inline_kbd_samp_tt_each_wrap_in_backticks() {
        let html = "Press <kbd>Ctrl+C</kbd>; output <samp>OK</samp>; var <tt>FOO</tt>.";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(plain, "Press `Ctrl+C`; output `OK`; var `FOO`.");
    }

    #[test]
    fn test_pre_code_does_not_double_wrap_in_backticks() {
        // Block-level <pre><code> preserves whitespace and must NOT add inline
        // backticks; the spec keeps backticks for inline use only.
        let html = "<pre><code>fn main() {\n    foo();\n}</code></pre>";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert!(
            !plain.contains('`'),
            "pre>code should not be wrapped in inline backticks; got: {plain:?}"
        );
        assert!(plain.contains("fn main() {"));
        assert!(plain.contains("    foo();"));
    }

    #[test]
    fn test_nested_inline_code_wraps_once() {
        // Nested <code><kbd>x</kbd></code> should still produce a single backtick
        // pair around the whole run, not two pairs.
        let html = "<code><kbd>Ctrl+C</kbd></code>";
        let plain = extract_plain_text_from_html(html).unwrap();

        assert_eq!(plain, "`Ctrl+C`");
    }
}
