use std::{mem, time::Duration};

use bh_types::ipc::{CompletedBlock, StreamFrame, UiEvent};
use tokio::time::Instant;

pub const FLUSH_INTERVAL: Duration = Duration::from_millis(16);
pub const MAX_BATCH_BYTES: usize = 32 * 1024;
pub const MAX_BATCH_EVENTS: usize = 64;

#[derive(Debug, Default)]
pub struct Coalescer {
    events: Vec<UiEvent>,
    bytes: usize,
    opened_at: Option<Instant>,
}

impl Coalescer {
    pub fn push(&mut self, event: UiEvent, now: Instant) -> Option<StreamFrame> {
        let ends_block = matches!(event, UiEvent::BlockDone { .. });
        self.bytes = self.bytes.saturating_add(payload_len(&event));
        if let Some(unmerged) = merge_into(self.events.last_mut(), event) {
            self.events.push(unmerged);
        }
        let opened_at = *self.opened_at.get_or_insert(now);
        let due = ends_block
            || self.events.len() >= MAX_BATCH_EVENTS
            || self.bytes >= MAX_BATCH_BYTES
            || now >= opened_at + FLUSH_INTERVAL;
        if due { self.flush() } else { None }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.opened_at.map(|opened_at| opened_at + FLUSH_INTERVAL)
    }

    pub fn flush(&mut self) -> Option<StreamFrame> {
        self.opened_at = None;
        self.bytes = 0;
        if self.events.is_empty() {
            return None;
        }
        Some(StreamFrame::Batch {
            events: mem::take(&mut self.events),
        })
    }
}

fn payload_len(event: &UiEvent) -> usize {
    match event {
        UiEvent::TextDelta { text, .. } | UiEvent::ReasoningDelta { text, .. } => text.len(),
        UiEvent::BlockDone { block, .. } => match block {
            CompletedBlock::Text { text } => text.len(),
            CompletedBlock::Reasoning { summary } => summary.len(),
        },
    }
}

fn merge_into(last: Option<&mut UiEvent>, event: UiEvent) -> Option<UiEvent> {
    match (last, event) {
        (
            Some(UiEvent::TextDelta { index, text }),
            UiEvent::TextDelta {
                index: next_index,
                text: next_text,
            },
        )
        | (
            Some(UiEvent::ReasoningDelta { index, text }),
            UiEvent::ReasoningDelta {
                index: next_index,
                text: next_text,
            },
        ) if *index == next_index => {
            text.push_str(&next_text);
            None
        }
        (_, event) => Some(event),
    }
}
