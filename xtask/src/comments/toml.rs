use super::cursor::Cursor;

const MAX_EXTRA_CLOSING_QUOTES: usize = 2;

pub fn comment_lines(source: &str) -> Vec<usize> {
    let mut cursor = Cursor::new(source);
    let mut lines = Vec::new();
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(b"\"\"\"") || cursor.starts_with(b"'''") {
            cursor.advance(3);
            skip_multiline_string(&mut cursor, byte);
            continue;
        }
        cursor.bump();
        match byte {
            b'"' => cursor.skip_escaped_string(b'"'),
            b'\'' => cursor.skip_raw_string(b'\''),
            b'#' => {
                lines.push(cursor.line());
                cursor.skip_line();
            }
            _ => {}
        }
    }
    lines
}

fn skip_multiline_string(cursor: &mut Cursor, quote: u8) {
    let delimiter = [quote; 3];
    let supports_escapes = quote == b'"';
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(&delimiter) {
            cursor.advance(3);
            for _ in 0..MAX_EXTRA_CLOSING_QUOTES {
                if cursor.peek() != Some(quote) {
                    return;
                }
                cursor.bump();
            }
            return;
        }
        cursor.bump();
        if supports_escapes && byte == b'\\' {
            cursor.bump();
        }
    }
}
