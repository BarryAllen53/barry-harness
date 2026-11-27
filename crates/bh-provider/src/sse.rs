use std::mem;

const DEFAULT_MAX_LINE_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_EVENT_BYTES: usize = 32 * 1024 * 1024;
const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";
const DEFAULT_EVENT_TYPE: &str = "message";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SseError {
    #[error("event stream line is not valid UTF-8")]
    InvalidUtf8,
    #[error("event stream line exceeds {limit} bytes")]
    LineTooLong { limit: usize },
    #[error("event data exceeds {limit} bytes")]
    EventTooLarge { limit: usize },
}

#[derive(Debug)]
pub struct SseDecoder {
    partial_line: Vec<u8>,
    skip_line_feed: bool,
    at_stream_start: bool,
    event: String,
    data: String,
    has_data: bool,
    max_line_bytes: usize,
    max_event_bytes: usize,
}

impl Default for SseDecoder {
    fn default() -> Self {
        Self::with_limits(DEFAULT_MAX_LINE_BYTES, DEFAULT_MAX_EVENT_BYTES)
    }
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_limits(max_line_bytes: usize, max_event_bytes: usize) -> Self {
        Self {
            partial_line: Vec::new(),
            skip_line_feed: false,
            at_stream_start: true,
            event: String::new(),
            data: String::new(),
            has_data: false,
            max_line_bytes,
            max_event_bytes,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        let mut events = Vec::new();
        let mut rest = chunk;
        if self.skip_line_feed && !rest.is_empty() {
            self.skip_line_feed = false;
            if rest[0] == b'\n' {
                rest = &rest[1..];
            }
        }
        while let Some(position) = rest.iter().position(|byte| matches!(byte, b'\n' | b'\r')) {
            let terminator = rest[position];
            self.ensure_fits(position)?;
            if self.partial_line.is_empty() {
                self.process_line(&rest[..position], &mut events)?;
            } else {
                self.partial_line.extend_from_slice(&rest[..position]);
                let line = mem::take(&mut self.partial_line);
                self.process_line(&line, &mut events)?;
            }
            rest = &rest[position + 1..];
            if terminator == b'\r' {
                match rest.first() {
                    Some(b'\n') => rest = &rest[1..],
                    Some(_) => {}
                    None => self.skip_line_feed = true,
                }
            }
        }
        self.ensure_fits(rest.len())?;
        self.partial_line.extend_from_slice(rest);
        Ok(events)
    }

    fn ensure_fits(&self, additional: usize) -> Result<(), SseError> {
        if self.partial_line.len().saturating_add(additional) > self.max_line_bytes {
            Err(SseError::LineTooLong {
                limit: self.max_line_bytes,
            })
        } else {
            Ok(())
        }
    }

    fn process_line(&mut self, line: &[u8], events: &mut Vec<SseEvent>) -> Result<(), SseError> {
        let line = if mem::replace(&mut self.at_stream_start, false) {
            line.strip_prefix(BYTE_ORDER_MARK).unwrap_or(line)
        } else {
            line
        };
        let line = std::str::from_utf8(line).map_err(|_| SseError::InvalidUtf8)?;
        if line.is_empty() {
            self.dispatch(events);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => {
                self.event.clear();
                self.event.push_str(value);
            }
            "data" => {
                let separator = usize::from(self.has_data);
                if self.data.len() + separator + value.len() > self.max_event_bytes {
                    return Err(SseError::EventTooLarge {
                        limit: self.max_event_bytes,
                    });
                }
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        let event = mem::take(&mut self.event);
        if !mem::replace(&mut self.has_data, false) {
            return;
        }
        events.push(SseEvent {
            event: if event.is_empty() {
                DEFAULT_EVENT_TYPE.to_owned()
            } else {
                event
            },
            data: mem::take(&mut self.data),
        });
    }
}
