use std::{collections::VecDeque, pin::Pin, time::Duration};

use bh_provider::{
    ProviderError, ProviderEvent, ProviderStream, SseDecoder, SseEvent, TimeoutPhase,
};
use bh_types::{ContentBlock, StopReason, TextBlock, Usage};
use futures_util::{Stream, StreamExt, stream};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

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

struct Pump<S> {
    chunks: Pin<Box<S>>,
    decoder: SseDecoder,
    pending: VecDeque<Result<ProviderEvent, ProviderError>>,
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
    let pump = Pump {
        chunks: Box::pin(chunks),
        decoder: SseDecoder::new(),
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
            for item in decode(&event) {
                if matches!(item, Ok(ProviderEvent::MessageStop { .. })) {
                    self.finished = true;
                }
                self.pending.push_back(item);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn decode(event: &SseEvent) -> Vec<Result<ProviderEvent, ProviderError>> {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&event.data) else {
        return vec![Ok(ProviderEvent::Warning {
            code: "unreadable_event".to_owned(),
            message: event.event.clone(),
        })];
    };
    let kind = event.event.clone();
    let index = data
        .get("output_index")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok());
    match kind.as_str() {
        "response.created" => {
            let response = data.get("response");
            vec![Ok(ProviderEvent::MessageStart {
                id: text(response, "id"),
                model: text(response, "model"),
                usage: usage(response),
            })]
        }
        "response.output_item.added" => {
            let is_message = data
                .get("item")
                .and_then(|item| item.get("type"))
                .and_then(serde_json::Value::as_str)
                == Some("message");
            match (index, is_message) {
                (Some(index), true) => vec![Ok(ProviderEvent::BlockStart {
                    index,
                    kind: bh_provider::BlockKind::Text,
                })],
                _ => Vec::new(),
            }
        }
        "response.reasoning_summary_text.delta" => index.map_or_else(Vec::new, |index| {
            vec![Ok(ProviderEvent::ReasoningSummaryDelta {
                index,
                text: text(Some(&data), "delta"),
            })]
        }),
        "response.output_text.delta" => index.map_or_else(Vec::new, |index| {
            vec![Ok(ProviderEvent::TextDelta {
                index,
                text: text(Some(&data), "delta"),
            })]
        }),
        "response.output_text.done" => index.map_or_else(Vec::new, |index| {
            vec![Ok(ProviderEvent::BlockDone {
                index,
                block: ContentBlock::Text(TextBlock {
                    text: text(Some(&data), "text"),
                }),
            })]
        }),
        "response.completed" => {
            let response = data.get("response");
            vec![Ok(ProviderEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                raw_stop: response
                    .and_then(|value| value.get("status"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                usage: usage(response),
            })]
        }
        "response.incomplete" => {
            let response = data.get("response");
            let reason = response
                .and_then(|value| value.get("incomplete_details"))
                .and_then(|value| value.get("reason"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("incomplete");
            let stop_reason = if reason.contains("max_output_tokens") {
                StopReason::MaxTokens
            } else {
                StopReason::EndTurn
            };
            vec![Ok(ProviderEvent::MessageStop {
                stop_reason,
                raw_stop: Some(reason.to_owned()),
                usage: usage(response),
            })]
        }
        "response.failed" => {
            let message = data
                .get("response")
                .and_then(|value| value.get("error"))
                .and_then(|value| value.get("message"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("the response failed");
            vec![Err(ProviderError::Stream {
                kind: "response_failed".to_owned(),
                message: message.to_owned(),
            })]
        }
        "error" => vec![Err(ProviderError::Stream {
            kind: text(Some(&data), "code"),
            message: text(Some(&data), "message"),
        })],
        "response.output_item.done"
        | "response.content_part.added"
        | "response.content_part.done" => Vec::new(),
        other => vec![Ok(ProviderEvent::Warning {
            code: other.to_owned(),
            message: "unknown streaming event".to_owned(),
        })],
    }
}

fn text(value: Option<&serde_json::Value>, key: &str) -> String {
    value
        .and_then(|value| value.get(key))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn usage(response: Option<&serde_json::Value>) -> Usage {
    let Some(usage) = response.and_then(|value| value.get("usage")) else {
        return Usage::default();
    };
    Usage {
        input_tokens: number(usage, "input_tokens"),
        output_tokens: number(usage, "output_tokens"),
        cache_read_tokens: usage
            .get("input_tokens_details")
            .map_or(0, |details| number(details, "cached_tokens")),
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        reasoning_tokens: usage
            .get("output_tokens_details")
            .map_or(0, |details| number(details, "reasoning_tokens")),
    }
}

fn number(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
}
