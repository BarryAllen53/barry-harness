use std::{collections::VecDeque, pin::Pin, time::Duration};

use bh_provider::{ProviderError, ProviderEvent, ProviderStream, SseDecoder, TimeoutPhase};
use bh_types::{ContentBlock, StopReason, TextBlock, Usage};
use futures_util::{Stream, StreamExt, stream};
use serde_json::Value;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const FAR_FUTURE: Duration = Duration::from_hours(24 * 365 * 30);
const REASONING_INDEX: u32 = 0;
const TEXT_INDEX: u32 = 1;

pub(crate) fn deadline_after(budget: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(budget).unwrap_or(now + FAR_FUTURE)
}

#[derive(Clone, Copy)]
pub(crate) struct Deadlines {
    pub(crate) first_event: Instant,
    pub(crate) idle: Duration,
}

struct Pump<S> {
    chunks: Pin<Box<S>>,
    decoder: SseDecoder,
    pending: VecDeque<Result<ProviderEvent, ProviderError>>,
    cancel: CancellationToken,
    idle: Duration,
    deadline: Instant,
    received_event: bool,
    finished: bool,
    model: String,
    text: String,
    reasoning: String,
    started: bool,
}

pub(crate) fn provider_events<S, B>(
    chunks: S,
    cancel: CancellationToken,
    deadlines: Deadlines,
    model: String,
) -> ProviderStream
where
    S: Stream<Item = Result<B, ProviderError>> + Send + 'static,
    B: AsRef<[u8]>,
{
    let pump = Pump {
        chunks: Box::pin(chunks),
        decoder: SseDecoder::new(),
        pending: VecDeque::new(),
        cancel,
        idle: deadlines.idle,
        deadline: deadlines.first_event,
        received_event: false,
        finished: false,
        model,
        text: String::new(),
        reasoning: String::new(),
        started: false,
    };
    stream::unfold(pump, |mut pump| async move {
        let item = pump.next_item().await?;
        Some((item, pump))
    })
    .boxed()
}

impl<S, B> Pump<S>
where
    S: Stream<Item = Result<B, ProviderError>>,
    B: AsRef<[u8]>,
{
    async fn next_item(&mut self) -> Option<Result<ProviderEvent, ProviderError>> {
        loop {
            if self.finished && self.pending.is_empty() {
                return None;
            }
            if self.cancel.is_cancelled() {
                return Some(Err(ProviderError::Cancelled));
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
                () = self.cancel.cancelled() => return Some(Err(ProviderError::Cancelled)),
                next = tokio::time::timeout_at(self.deadline, self.chunks.next()) => next,
            };
            match next {
                Err(_) => return Some(Err(ProviderError::Timeout { phase })),
                Ok(None) => {
                    self.finished = true;
                    if !self.pending.is_empty() {
                        continue;
                    }
                    return Some(Err(ProviderError::TruncatedStream));
                }
                Ok(Some(Err(error))) => return Some(Err(error)),
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
        if events.is_empty() {
            return;
        }
        self.received_event = true;
        self.deadline = deadline_after(self.idle);
        for event in events {
            let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
                self.pending.push_back(Ok(ProviderEvent::Warning {
                    code: "unreadable_chunk".to_owned(),
                    message: "the provider sent json the adapter cannot read".to_owned(),
                }));
                continue;
            };
            self.apply(&data);
        }
    }

    fn apply(&mut self, data: &Value) {
        if !self.started {
            self.started = true;
            let model = data
                .get("modelVersion")
                .and_then(Value::as_str)
                .unwrap_or(&self.model)
                .to_owned();
            self.pending.push_back(Ok(ProviderEvent::MessageStart {
                id: "gemini".to_owned(),
                model,
                usage: Usage::default(),
            }));
        }
        if let Some(usage) = data.get("usageMetadata") {
            self.pending.push_back(Ok(ProviderEvent::UsageUpdate {
                usage: usage_of(usage),
            }));
        }
        let Some(candidate) = data
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|candidates| candidates.first())
        else {
            return;
        };
        if let Some(parts) = candidate
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                let Some(text) = part.get("text").and_then(Value::as_str) else {
                    continue;
                };
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    self.reasoning.push_str(text);
                    self.pending
                        .push_back(Ok(ProviderEvent::ReasoningSummaryDelta {
                            index: REASONING_INDEX,
                            text: text.to_owned(),
                        }));
                } else {
                    self.text.push_str(text);
                    self.pending.push_back(Ok(ProviderEvent::TextDelta {
                        index: TEXT_INDEX,
                        text: text.to_owned(),
                    }));
                }
            }
        }
        let Some(reason) = candidate.get("finishReason").and_then(Value::as_str) else {
            return;
        };
        let stop_reason = match reason {
            "MAX_TOKENS" => StopReason::MaxTokens,
            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" => StopReason::Refusal,
            _ => StopReason::EndTurn,
        };
        let usage = data.get("usageMetadata").map_or(Usage::default(), usage_of);
        self.pending.push_back(Ok(ProviderEvent::BlockDone {
            index: TEXT_INDEX,
            block: ContentBlock::Text(TextBlock {
                text: self.text.clone(),
            }),
        }));
        self.pending.push_back(Ok(ProviderEvent::MessageStop {
            stop_reason,
            raw_stop: Some(reason.to_owned()),
            usage,
        }));
        self.finished = true;
    }
}

fn usage_of(usage: &Value) -> Usage {
    let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: number("promptTokenCount"),
        output_tokens: number("candidatesTokenCount"),
        cache_read_tokens: number("cachedContentTokenCount"),
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        reasoning_tokens: number("thoughtsTokenCount"),
    }
}
