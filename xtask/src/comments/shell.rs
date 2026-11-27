use super::cursor::Cursor;

struct Heredoc {
    delimiter: Vec<u8>,
    strips_tabs: bool,
}

pub fn posix_comment_lines(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut cursor = Cursor::new(source);
    let mut lines = Vec::new();
    let mut pending_heredocs = Vec::new();
    let mut arithmetic_parens = 0_usize;
    if cursor.starts_with(b"#!") {
        cursor.skip_line();
    }
    while let Some(byte) = cursor.peek() {
        if byte == b'#' && starts_word(bytes, cursor.position(), b";|&()") {
            lines.push(cursor.line());
            cursor.skip_line();
            continue;
        }
        if cursor.starts_with(b"$((")
            || (arithmetic_parens == 0
                && cursor.starts_with(b"((")
                && starts_word(bytes, cursor.position(), b";|&()"))
        {
            cursor.advance(if byte == b'$' { 3 } else { 2 });
            arithmetic_parens += 2;
            continue;
        }
        if arithmetic_parens == 0 && cursor.starts_with(b"<<<") {
            cursor.advance(3);
            continue;
        }
        if arithmetic_parens == 0 && cursor.starts_with(b"<<") {
            cursor.advance(2);
            pending_heredocs.extend(read_heredoc(&mut cursor));
            continue;
        }
        if cursor.starts_with(b"$'") {
            cursor.advance(2);
            skip_quoted(&mut cursor, b'\'', b'\\');
            continue;
        }
        cursor.bump();
        match byte {
            b'\'' => cursor.skip_past(b"'"),
            b'"' => skip_quoted(&mut cursor, b'"', b'\\'),
            b'\\' => {
                cursor.bump();
            }
            b'(' if arithmetic_parens > 0 => arithmetic_parens += 1,
            b')' if arithmetic_parens > 0 => arithmetic_parens -= 1,
            b'\n' => {
                for heredoc in pending_heredocs.drain(..) {
                    skip_heredoc_body(&mut cursor, source, &heredoc);
                }
            }
            _ => {}
        }
    }
    lines
}

pub fn powershell_comment_lines(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut cursor = Cursor::new(source);
    let mut lines = Vec::new();
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(b"<#") {
            lines.push(cursor.line());
            cursor.advance(2);
            cursor.skip_past(b"#>");
            continue;
        }
        if byte == b'#' && starts_word(bytes, cursor.position(), b";|(){}=,") {
            lines.push(cursor.line());
            cursor.skip_line();
            continue;
        }
        if let Some(terminator) = here_string_terminator(bytes, cursor.position()) {
            cursor.skip_line();
            skip_here_string(&mut cursor, source, terminator);
            continue;
        }
        cursor.bump();
        match byte {
            b'\'' => skip_doubling_quoted(&mut cursor, b'\'', None),
            b'"' => skip_doubling_quoted(&mut cursor, b'"', Some(b'`')),
            b'`' => {
                cursor.bump();
            }
            _ => {}
        }
    }
    lines
}

fn starts_word(bytes: &[u8], position: usize, separators: &[u8]) -> bool {
    position.checked_sub(1).is_none_or(|previous| {
        bytes[previous].is_ascii_whitespace() || separators.contains(&bytes[previous])
    })
}

fn skip_quoted(cursor: &mut Cursor, quote: u8, escape: u8) {
    while let Some(byte) = cursor.bump() {
        if byte == escape {
            cursor.bump();
        } else if byte == quote {
            return;
        }
    }
}

fn skip_doubling_quoted(cursor: &mut Cursor, quote: u8, escape: Option<u8>) {
    while let Some(byte) = cursor.bump() {
        if Some(byte) == escape {
            cursor.bump();
        } else if byte == quote {
            if cursor.peek() != Some(quote) {
                return;
            }
            cursor.bump();
        }
    }
}

fn read_heredoc(cursor: &mut Cursor) -> Option<Heredoc> {
    let strips_tabs = cursor.peek() == Some(b'-');
    if strips_tabs {
        cursor.bump();
    }
    while matches!(cursor.peek(), Some(b' ' | b'\t')) {
        cursor.bump();
    }
    if !cursor.peek().is_some_and(|byte| {
        byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'\\' | b'\'' | b'"')
    }) {
        return None;
    }
    let mut delimiter = Vec::new();
    while let Some(byte) = cursor.peek() {
        if byte.is_ascii_whitespace() || b";|&<>()".contains(&byte) {
            break;
        }
        cursor.bump();
        match byte {
            b'\\' => delimiter.extend(cursor.bump()),
            b'\'' | b'"' => take_quoted_delimiter_part(cursor, byte, &mut delimiter),
            _ => delimiter.push(byte),
        }
    }
    (!delimiter.is_empty()).then_some(Heredoc {
        delimiter,
        strips_tabs,
    })
}

fn take_quoted_delimiter_part(cursor: &mut Cursor, quote: u8, delimiter: &mut Vec<u8>) {
    while let Some(byte) = cursor.peek() {
        if byte == b'\n' {
            return;
        }
        cursor.bump();
        if byte == quote {
            return;
        }
        let escapes_next = quote == b'"'
            && byte == b'\\'
            && cursor
                .peek()
                .is_some_and(|next| matches!(next, b'"' | b'\\' | b'$' | b'`'));
        if escapes_next {
            delimiter.extend(cursor.bump());
        } else {
            delimiter.push(byte);
        }
    }
}

fn skip_heredoc_body(cursor: &mut Cursor, source: &str, heredoc: &Heredoc) {
    while cursor.peek().is_some() {
        let line = take_line(cursor, source);
        let candidate = if heredoc.strips_tabs {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if candidate.as_bytes() == heredoc.delimiter {
            return;
        }
    }
}

fn here_string_terminator(bytes: &[u8], position: usize) -> Option<&'static str> {
    let terminator = match bytes.get(position..position + 2)? {
        b"@'" => "'@",
        b"@\"" => "\"@",
        _ => return None,
    };
    let rest = &bytes[position + 2..];
    let line_end = rest
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(rest.len());
    rest[..line_end]
        .iter()
        .all(u8::is_ascii_whitespace)
        .then_some(terminator)
}

fn skip_here_string(cursor: &mut Cursor, source: &str, terminator: &str) {
    if cursor.peek() == Some(b'\n') {
        cursor.bump();
    }
    while cursor.peek().is_some() {
        let start = cursor.position();
        if source[start..].starts_with(terminator) {
            cursor.advance(terminator.len());
            return;
        }
        take_line(cursor, source);
    }
}

fn take_line<'a>(cursor: &mut Cursor, source: &'a str) -> &'a str {
    let start = cursor.position();
    cursor.skip_line();
    let line = &source[start..cursor.position()];
    cursor.bump();
    line.strip_suffix('\r').unwrap_or(line)
}
