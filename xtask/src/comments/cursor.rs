pub struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    line: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            position: 0,
            line: 1,
        }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    pub fn starts_with(&self, pattern: &[u8]) -> bool {
        self.bytes[self.position..].starts_with(pattern)
    }

    pub fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        if byte == b'\n' {
            self.line += 1;
        }
        Some(byte)
    }

    pub fn advance(&mut self, count: usize) {
        for _ in 0..count {
            if self.bump().is_none() {
                return;
            }
        }
    }

    pub fn skip_past(&mut self, terminator: &[u8]) {
        while self.peek().is_some() {
            if self.starts_with(terminator) {
                self.advance(terminator.len());
                return;
            }
            self.bump();
        }
    }

    pub fn skip_line(&mut self) {
        while let Some(byte) = self.peek() {
            if byte == b'\n' {
                return;
            }
            self.bump();
        }
    }

    pub fn skip_escaped_string(&mut self, quote: u8) {
        while let Some(byte) = self.peek() {
            if byte == b'\n' {
                return;
            }
            self.bump();
            if byte == b'\\' {
                self.bump();
            } else if byte == quote {
                return;
            }
        }
    }

    pub fn skip_raw_string(&mut self, quote: u8) {
        while let Some(byte) = self.peek() {
            if byte == b'\n' {
                return;
            }
            self.bump();
            if byte == quote {
                return;
            }
        }
    }
}
