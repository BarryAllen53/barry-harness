const HEADER_LEN: usize = 5;
const COMPRESSED: u8 = 0x01;
const END_STREAM: u8 = 0x02;
const MAX_MESSAGE_LEN: usize = 64 << 20;

pub(crate) fn encode(message: &[u8]) -> Result<Vec<u8>, EnvelopeError> {
    let length = u32::try_from(message.len()).map_err(|_| EnvelopeError::TooLarge)?;
    let mut framed = Vec::with_capacity(HEADER_LEN + message.len());
    framed.push(0);
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(message);
    Ok(framed)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) end_stream: bool,
    pub(crate) payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnvelopeError {
    TooLarge,
    Compressed,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "a connect message is larger than the adapter accepts",
            Self::Compressed => "the hub sent a compressed connect message",
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct EnvelopeDecoder {
    buffer: Vec<u8>,
}

impl EnvelopeDecoder {
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, EnvelopeError> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        let mut consumed = 0;
        while let Some(header) = self.buffer.get(consumed..consumed + HEADER_LEN) {
            let flags = header[0];
            let length = usize::try_from(u32::from_be_bytes([
                header[1], header[2], header[3], header[4],
            ]))
            .map_err(|_| EnvelopeError::TooLarge)?;
            if length > MAX_MESSAGE_LEN {
                return Err(EnvelopeError::TooLarge);
            }
            if flags & COMPRESSED != 0 {
                return Err(EnvelopeError::Compressed);
            }
            let start = consumed + HEADER_LEN;
            let Some(payload) = self.buffer.get(start..start + length) else {
                break;
            };
            frames.push(Frame {
                end_stream: flags & END_STREAM != 0,
                payload: payload.to_vec(),
            });
            consumed = start + length;
        }
        self.buffer.drain(..consumed);
        Ok(frames)
    }
}
