use super::cursor::Cursor;

const URL_FUNCTION: &[u8] = b"url(";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Css,
    Scss,
}

pub fn comment_lines(source: &str) -> Vec<usize> {
    scan(source, Dialect::Css)
}

pub fn scss_comment_lines(source: &str) -> Vec<usize> {
    scan(source, Dialect::Scss)
}

fn scan(source: &str, dialect: Dialect) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut cursor = Cursor::new(source);
    let mut lines = Vec::new();
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(b"/*") {
            lines.push(cursor.line());
            cursor.advance(2);
            cursor.skip_past(b"*/");
            continue;
        }
        if dialect == Dialect::Scss && cursor.starts_with(b"//") {
            lines.push(cursor.line());
            cursor.skip_line();
            continue;
        }
        if starts_url_function(bytes, cursor.position()) {
            cursor.advance(URL_FUNCTION.len());
            skip_unquoted_url(&mut cursor);
            continue;
        }
        cursor.bump();
        match byte {
            b'"' | b'\'' => cursor.skip_escaped_string(byte),
            b'\\' => {
                cursor.bump();
            }
            _ => {}
        }
    }
    lines
}

fn starts_url_function(bytes: &[u8], position: usize) -> bool {
    let follows_identifier = position.checked_sub(1).is_some_and(|previous| {
        bytes[previous].is_ascii_alphanumeric() || matches!(bytes[previous], b'-' | b'_')
    });
    !follows_identifier
        && bytes
            .get(position..position + URL_FUNCTION.len())
            .is_some_and(|window| window.eq_ignore_ascii_case(URL_FUNCTION))
}

fn skip_unquoted_url(cursor: &mut Cursor) {
    while cursor.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
        cursor.bump();
    }
    if matches!(cursor.peek(), Some(b'"' | b'\'')) {
        return;
    }
    while let Some(byte) = cursor.bump() {
        match byte {
            b'\\' => {
                cursor.bump();
            }
            b')' => return,
            _ => {}
        }
    }
}
