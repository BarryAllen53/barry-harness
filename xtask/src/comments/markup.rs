use super::{css, cursor::Cursor, script};

const JAVASCRIPT_TYPES: [&str; 8] = [
    "",
    "module",
    "text/javascript",
    "application/javascript",
    "text/ecmascript",
    "application/ecmascript",
    "application/x-javascript",
    "text/x-javascript",
];

type Scanner = fn(&str) -> Vec<usize>;

struct RawTextElement {
    closing_tag: &'static [u8],
    scanner: Option<Scanner>,
}

enum TagEnd {
    Open,
    SelfClosing,
}

struct OpeningTag<'a> {
    end: TagEnd,
    type_attribute: Option<&'a str>,
}

pub fn comment_lines(source: &str) -> Vec<usize> {
    let mut cursor = Cursor::new(source);
    let mut lines = Vec::new();
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(b"<!--") {
            lines.push(cursor.line());
            cursor.advance(4);
            cursor.skip_past(b"-->");
            continue;
        }
        if cursor.starts_with(b"<![CDATA[") {
            cursor.skip_past(b"]]>");
            continue;
        }
        cursor.bump();
        if byte != b'<' {
            continue;
        }
        let name = read_name(&mut cursor, source);
        if name.is_empty() {
            if matches!(cursor.peek(), Some(b'/' | b'!' | b'?')) {
                cursor.skip_past(b">");
            }
            continue;
        }
        let tag = read_opening_tag(&mut cursor, source, &mut lines);
        if let (TagEnd::Open, Some(element)) = (tag.end, raw_text_element(name, tag.type_attribute))
        {
            scan_raw_text(&mut cursor, source, &element, &mut lines);
        }
    }
    lines
}

fn read_name<'a>(cursor: &mut Cursor, source: &'a str) -> &'a str {
    let start = cursor.position();
    if !cursor.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
        return "";
    }
    while cursor.peek().is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'_' | b'.')
    }) {
        cursor.bump();
    }
    &source[start..cursor.position()]
}

fn read_opening_tag<'a>(
    cursor: &mut Cursor,
    source: &'a str,
    lines: &mut Vec<usize>,
) -> OpeningTag<'a> {
    let mut type_attribute = None;
    loop {
        while cursor.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            cursor.bump();
        }
        if cursor.starts_with(b"/>") {
            cursor.advance(2);
            return OpeningTag {
                end: TagEnd::SelfClosing,
                type_attribute,
            };
        }
        match cursor.peek() {
            None => {
                return OpeningTag {
                    end: TagEnd::Open,
                    type_attribute,
                };
            }
            Some(b'>') => {
                cursor.bump();
                return OpeningTag {
                    end: TagEnd::Open,
                    type_attribute,
                };
            }
            Some(_) => {}
        }
        let name_start = cursor.position();
        while cursor
            .peek()
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'>' | b'/'))
        {
            cursor.bump();
        }
        let name = &source[name_start..cursor.position()];
        if name.is_empty() {
            cursor.bump();
            continue;
        }
        while cursor.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            cursor.bump();
        }
        if cursor.peek() != Some(b'=') {
            continue;
        }
        cursor.bump();
        while cursor.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            cursor.bump();
        }
        let value_line = cursor.line();
        let value = read_attribute_value(cursor, source);
        if name.eq_ignore_ascii_case("type") {
            type_attribute = Some(value);
        }
        if let Some(scanner) = attribute_scanner(name) {
            lines.extend(scanner(value).into_iter().map(|line| line + value_line - 1));
        }
    }
}

fn read_attribute_value<'a>(cursor: &mut Cursor, source: &'a str) -> &'a str {
    if let Some(quote @ (b'"' | b'\'')) = cursor.peek() {
        cursor.bump();
        let start = cursor.position();
        while cursor.peek().is_some_and(|byte| byte != quote) {
            cursor.bump();
        }
        let value = &source[start..cursor.position()];
        cursor.bump();
        return value;
    }
    let start = cursor.position();
    while cursor
        .peek()
        .is_some_and(|byte| !byte.is_ascii_whitespace() && byte != b'>')
    {
        cursor.bump();
    }
    &source[start..cursor.position()]
}

fn attribute_scanner(name: &str) -> Option<Scanner> {
    if name.eq_ignore_ascii_case("style") {
        Some(css::comment_lines)
    } else if name.len() > 2 && name.as_bytes()[..2].eq_ignore_ascii_case(b"on") {
        Some(script::comment_lines)
    } else {
        None
    }
}

fn raw_text_element(tag_name: &str, type_attribute: Option<&str>) -> Option<RawTextElement> {
    let lowercase_name = tag_name.to_ascii_lowercase();
    let (closing_tag, scanner): (&'static [u8], Option<Scanner>) = match lowercase_name.as_str() {
        "script" if is_javascript_type(type_attribute) => {
            (b"</script", Some(script::comment_lines))
        }
        "script" => (b"</script", None),
        "style" => (b"</style", Some(css::comment_lines)),
        "textarea" => (b"</textarea", None),
        "title" => (b"</title", None),
        _ => return None,
    };
    Some(RawTextElement {
        closing_tag,
        scanner,
    })
}

fn is_javascript_type(type_attribute: Option<&str>) -> bool {
    type_attribute.is_none_or(|value| {
        let essence = value.split(';').next().unwrap_or_default().trim();
        JAVASCRIPT_TYPES
            .iter()
            .any(|known| known.eq_ignore_ascii_case(essence))
    })
}

fn scan_raw_text(
    cursor: &mut Cursor,
    source: &str,
    element: &RawTextElement,
    lines: &mut Vec<usize>,
) {
    let start = cursor.position();
    let end = source.as_bytes()[start..]
        .windows(element.closing_tag.len())
        .position(|window| window.eq_ignore_ascii_case(element.closing_tag))
        .map_or(source.len(), |offset| start + offset);
    if let Some(scanner) = element.scanner {
        let line_offset = cursor.line() - 1;
        lines.extend(
            scanner(&source[start..end])
                .into_iter()
                .map(|line| line + line_offset),
        );
    }
    cursor.advance(end - start);
}
