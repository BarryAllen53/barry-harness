use std::mem;

use super::cursor::Cursor;

const KEYWORDS_BEFORE_EXPRESSION: [&[u8]; 14] = [
    b"return",
    b"typeof",
    b"instanceof",
    b"in",
    b"of",
    b"new",
    b"delete",
    b"void",
    b"throw",
    b"case",
    b"do",
    b"else",
    b"yield",
    b"await",
];

const CONTROL_KEYWORDS: [&[u8]; 4] = [b"if", b"while", b"for", b"with"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Plain,
    Jsx,
}

#[derive(Clone, Copy)]
enum Slash {
    StartsRegex,
    Divides,
}

enum TemplateEnd {
    Closed,
    Interpolation,
}

#[derive(Clone, Copy)]
enum Tag {
    Opening,
    Closing,
}

#[derive(Clone, Copy)]
struct JsxPosition {
    open_elements: usize,
    tag: Option<Tag>,
}

#[derive(Clone, Copy)]
enum JsxEnd {
    Closed,
    Interpolation(JsxPosition),
}

enum Frame {
    Template { braces: usize },
    Jsx { braces: usize, resume: JsxPosition },
}

struct Scanner<'a> {
    bytes: &'a [u8],
    cursor: Cursor<'a>,
    dialect: Dialect,
    lines: Vec<usize>,
    frames: Vec<Frame>,
    control_parens: Vec<bool>,
    slash: Slash,
    follows_control_keyword: bool,
}

pub fn comment_lines(source: &str) -> Vec<usize> {
    Scanner::new(source, Dialect::Plain).run()
}

pub fn jsx_comment_lines(source: &str) -> Vec<usize> {
    Scanner::new(source, Dialect::Jsx).run()
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str, dialect: Dialect) -> Self {
        Self {
            bytes: source.as_bytes(),
            cursor: Cursor::new(source),
            dialect,
            lines: Vec::new(),
            frames: Vec::new(),
            control_parens: Vec::new(),
            slash: Slash::StartsRegex,
            follows_control_keyword: false,
        }
    }

    fn run(mut self) -> Vec<usize> {
        while let Some(byte) = self.cursor.peek() {
            if self.skip_comment() {
                continue;
            }
            if is_identifier_byte(byte) {
                self.scan_word();
                continue;
            }
            if byte.is_ascii_whitespace() {
                self.cursor.bump();
                continue;
            }
            let follows_control_keyword = mem::take(&mut self.follows_control_keyword);
            if self.allows_postfix_operator() && self.skip_postfix_operator(byte) {
                self.slash = Slash::Divides;
                continue;
            }
            if self.starts_jsx(byte) {
                self.cursor.bump();
                let end = self.scan_jsx(JsxPosition {
                    open_elements: 0,
                    tag: Some(Tag::Opening),
                });
                self.slash = self.leave_jsx(end);
                continue;
            }
            self.cursor.bump();
            self.slash = match byte {
                b'"' | b'\'' => {
                    self.cursor.skip_escaped_string(byte);
                    Slash::Divides
                }
                b'`' => self.enter_template(),
                b'(' => {
                    self.control_parens.push(follows_control_keyword);
                    Slash::StartsRegex
                }
                b')' if self.control_parens.pop() == Some(true) => Slash::StartsRegex,
                b')' | b']' => Slash::Divides,
                b'{' => {
                    if let Some(Frame::Template { braces } | Frame::Jsx { braces, .. }) =
                        self.frames.last_mut()
                    {
                        *braces += 1;
                    }
                    Slash::StartsRegex
                }
                b'}' => self.close_brace(),
                b'/' => match self.slash {
                    Slash::StartsRegex => {
                        skip_regex(&mut self.cursor);
                        Slash::Divides
                    }
                    Slash::Divides => Slash::StartsRegex,
                },
                _ => Slash::StartsRegex,
            };
        }
        self.lines
    }

    fn skip_comment(&mut self) -> bool {
        if self.cursor.starts_with(b"//") {
            self.lines.push(self.cursor.line());
            self.cursor.skip_line();
            return true;
        }
        if self.cursor.starts_with(b"/*") {
            self.lines.push(self.cursor.line());
            self.cursor.advance(2);
            self.cursor.skip_past(b"*/");
            return true;
        }
        false
    }

    fn scan_word(&mut self) {
        let start = self.cursor.position();
        while self.cursor.peek().is_some_and(is_identifier_byte) {
            self.cursor.bump();
        }
        let word = &self.bytes[start..self.cursor.position()];
        let is_keyword = !is_member_access(&self.bytes[..start]);
        self.slash = if is_keyword && KEYWORDS_BEFORE_EXPRESSION.contains(&word) {
            Slash::StartsRegex
        } else {
            Slash::Divides
        };
        self.follows_control_keyword = is_keyword && CONTROL_KEYWORDS.contains(&word);
    }

    fn allows_postfix_operator(&self) -> bool {
        matches!(self.slash, Slash::Divides)
            && self.bytes[..self.cursor.position()]
                .iter()
                .rev()
                .find(|&&byte| !matches!(byte, b' ' | b'\t'))
                != Some(&b'\n')
    }

    fn skip_postfix_operator(&mut self, byte: u8) -> bool {
        if self.cursor.starts_with(b"++") || self.cursor.starts_with(b"--") {
            self.cursor.advance(2);
            return true;
        }
        if byte == b'!' && !self.cursor.starts_with(b"!=") {
            self.cursor.bump();
            return true;
        }
        false
    }

    fn starts_jsx(&self, byte: u8) -> bool {
        self.dialect == Dialect::Jsx
            && byte == b'<'
            && matches!(self.slash, Slash::StartsRegex)
            && starts_jsx_element(&self.bytes[self.cursor.position() + 1..])
    }

    fn enter_template(&mut self) -> Slash {
        match skip_template(&mut self.cursor) {
            TemplateEnd::Closed => Slash::Divides,
            TemplateEnd::Interpolation => {
                self.frames.push(Frame::Template { braces: 0 });
                Slash::StartsRegex
            }
        }
    }

    fn close_brace(&mut self) -> Slash {
        match self.frames.last_mut() {
            Some(Frame::Template { braces: 0 }) => {
                self.frames.pop();
                self.enter_template()
            }
            Some(&mut Frame::Jsx { braces: 0, resume }) => {
                self.frames.pop();
                let end = self.scan_jsx(resume);
                self.leave_jsx(end)
            }
            Some(Frame::Template { braces } | Frame::Jsx { braces, .. }) => {
                *braces -= 1;
                Slash::StartsRegex
            }
            None => Slash::StartsRegex,
        }
    }

    fn leave_jsx(&mut self, end: JsxEnd) -> Slash {
        match end {
            JsxEnd::Closed => Slash::Divides,
            JsxEnd::Interpolation(resume) => {
                self.frames.push(Frame::Jsx { braces: 0, resume });
                Slash::StartsRegex
            }
        }
    }

    fn scan_jsx(&mut self, mut position: JsxPosition) -> JsxEnd {
        while let Some(byte) = self.cursor.peek() {
            if byte == b'{' {
                self.cursor.bump();
                return JsxEnd::Interpolation(position);
            }
            let Some(tag) = position.tag else {
                self.cursor.bump();
                if byte == b'<' {
                    position.tag = Some(if self.skip_closing_slash() {
                        Tag::Closing
                    } else {
                        Tag::Opening
                    });
                }
                continue;
            };
            if self.skip_comment() {
                continue;
            }
            if self.cursor.starts_with(b"/>") {
                self.cursor.advance(2);
                position.tag = None;
                if position.open_elements == 0 {
                    return JsxEnd::Closed;
                }
                continue;
            }
            self.cursor.bump();
            match byte {
                b'>' => {
                    position.tag = None;
                    match tag {
                        Tag::Opening => position.open_elements += 1,
                        Tag::Closing => {
                            position.open_elements = position.open_elements.saturating_sub(1);
                            if position.open_elements == 0 {
                                return JsxEnd::Closed;
                            }
                        }
                    }
                }
                b'"' | b'\'' => self.cursor.skip_past(&[byte]),
                _ => {}
            }
        }
        JsxEnd::Closed
    }

    fn skip_closing_slash(&mut self) -> bool {
        let rest = &self.bytes[self.cursor.position()..];
        let blanks = rest
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        if rest.get(blanks) != Some(&b'/') {
            return false;
        }
        self.cursor.advance(blanks + 1);
        true
    }
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || !byte.is_ascii()
}

fn is_member_access(before_word: &[u8]) -> bool {
    let mut previous = before_word
        .iter()
        .rev()
        .skip_while(|byte| byte.is_ascii_whitespace());
    previous.next() == Some(&b'.') && previous.next() != Some(&b'.')
}

fn is_identifier_start(byte: u8) -> bool {
    is_identifier_byte(byte) && !byte.is_ascii_digit()
}

fn starts_jsx_element(after_angle: &[u8]) -> bool {
    match after_angle.first() {
        Some(b'>') => true,
        Some(&first) if is_identifier_start(first) => !starts_type_parameters(after_angle),
        _ => false,
    }
}

fn starts_type_parameters(after_angle: &[u8]) -> bool {
    let name_end = after_angle
        .iter()
        .position(|&byte| !is_identifier_byte(byte))
        .unwrap_or(after_angle.len());
    let after_name = &after_angle[name_end..];
    let next = &after_name[after_name
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count()..];
    next.starts_with(b",")
        || next.starts_with(b"=")
        || next.starts_with(b">(")
        || (next.starts_with(b"extends") && !next.get(7).copied().is_some_and(is_identifier_byte))
}

fn skip_template(cursor: &mut Cursor) -> TemplateEnd {
    while let Some(byte) = cursor.peek() {
        if cursor.starts_with(b"${") {
            cursor.advance(2);
            return TemplateEnd::Interpolation;
        }
        cursor.bump();
        match byte {
            b'\\' => {
                cursor.bump();
            }
            b'`' => return TemplateEnd::Closed,
            _ => {}
        }
    }
    TemplateEnd::Closed
}

fn skip_regex(cursor: &mut Cursor) {
    let mut in_class = false;
    while let Some(byte) = cursor.peek() {
        if byte == b'\n' {
            return;
        }
        cursor.bump();
        match byte {
            b'\\' => {
                cursor.bump();
            }
            b'[' => in_class = true,
            b']' => in_class = false,
            b'/' if !in_class => {
                while cursor.peek().is_some_and(|flag| flag.is_ascii_alphabetic()) {
                    cursor.bump();
                }
                return;
            }
            _ => {}
        }
    }
}
