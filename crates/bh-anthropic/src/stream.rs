use std::{collections::VecDeque, pin::Pin, time::Duration};

use bh_provider::{ProviderError, ProviderStream, SseDecoder, TimeoutPhase};
use futures_util::{Stream, StreamExt, stream};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::assembler::{Assembler, Flow, StreamItem};

const FAR_FUTURE: Duration = Duration::from_hours(24 * 365 * 30);

pub(crate) fn deadline_after(budget: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(budget).unwrap_or(now + FAR_FUTURE)
}

#[derive(Clone, Copy)]
pub(crate) struct Deadlines {
    pub(crate) first_event: Instant,
    pub(crate) idle: Duration,
}

struct EventPump<S> {
    chunks: Pin<Box<S>>,
    decoder: SseDecoder,
    assembler: Assembler,
    pending: VecDeque<StreamItem>,
    cancel: CancellationToken,
    idle: Duration,
    deadline: Instant,
    received_event: bool,
    finished: bool,
}

pub(crate) fn provider_events<S, B>(
    chunks: S,
    cancel: CancellationToken,
    deadlines: Deadlines,
) -> ProviderStream
where
    S: Stream<Item = Result<B, ProviderError>> + Send + 'static,
    B: AsRef<[u8]>,
{
    let pump = EventPump {
        chunks: Box::pin(chunks),
        decoder: SseDecoder::new(),
        assembler: Assembler::default(),
        pending: VecDeque::new(),
        cancel,
        idle: deadlines.idle,
        deadline: deadlines.first_event,
        received_event: false,
        finished: false,
    };
    stream::unfold(pump, |mut pump| async move {
        let item = pump.next_item().await?;
        Some((item, pump))
    })
    .boxed()
}

impl<S, B> EventPump<S>
where
    S: Stream<Item = Result<B, ProviderError>>,
    B: AsRef<[u8]>,
{
    async fn next_item(&mut self) -> Option<StreamItem> {
        loop {
            if self.finished && self.pending.is_empty() {
                return None;
            }
            let message_delivered = self.finished && self.assembler.is_complete();
            if !message_delivered && self.cancel.is_cancelled() {
                return Some(self.finish_with(ProviderError::Cancelled));
            }
            if let Some(item) = self.pending.pop_front() {
                return Some(item);
            }
            let phase = if self.received_event {
                TimeoutPhase::Idle
            } else {
                TimeoutPhase::FirstEvent
            };
            let next = tokio::select! {
                biased;
                () = self.cancel.cancelled() => return Some(self.finish_with(ProviderError::Cancelled)),
                next = tokio::time::timeout_at(self.deadline, self.chunks.next()) => next,
            };
            match next {
                Err(_) => return Some(self.finish_with(ProviderError::Timeout { phase })),
                Ok(None) => {
                    self.finished = true;
                    if !self.assembler.is_complete() {
                        return Some(Err(ProviderError::TruncatedStream));
                    }
                }
                Ok(Some(Err(error))) => return Some(self.finish_with(error)),
                Ok(Some(Ok(chunk))) => self.consume(chunk.as_ref()),
            }
        }
    }

    fn consume(&mut self, chunk: &[u8]) {
        let events = match self.decoder.feed(chunk) {
            Ok(events) => events,
            Err(error) => {
                self.finished = true;
                self.pending.push_back(Err(ProviderError::Protocol {
                    message: error.to_string(),
                }));
                return;
            }
        };
        if !events.is_empty() {
            self.received_event = true;
            self.deadline = deadline_after(self.idle);
        }
        for event in &events {
            if self.assembler.push(event, &mut self.pending) == Flow::Done {
                self.finished = true;
                return;
            }
        }
    }

    fn finish_with(&mut self, error: ProviderError) -> StreamItem {
        self.finished = true;
        self.pending.clear();
        Err(error)
    }
}
