use std::collections::BTreeMap;

use bh_provider::{Provider, ProviderError, ProviderEvent, ProviderRequest, ProviderStream};
use bh_types::{
    ContentBlock, StopReason, TextBlock,
    ipc::{CloseReason, CompletedBlock, StreamFrame, UiEvent},
};
use futures_util::StreamExt;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

use crate::{coalescer::Coalescer, errors};

pub trait FrameSink {
    fn send(&self, frame: StreamFrame) -> Result<(), String>;
}

impl FrameSink for tauri::ipc::Channel<StreamFrame> {
    fn send(&self, frame: StreamFrame) -> Result<(), String> {
        tauri::ipc::Channel::send(self, frame).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
pub struct TurnRun {
    pub blocks: Vec<ContentBlock>,
    pub close: CloseReason,
}

#[derive(Debug, Default)]
struct BlockCollector {
    done: BTreeMap<u32, ContentBlock>,
    partial_text: BTreeMap<u32, String>,
}

impl BlockCollector {
    fn observe(&mut self, event: &ProviderEvent) {
        match event {
            ProviderEvent::TextDelta { index, text } => {
                self.partial_text.entry(*index).or_default().push_str(text);
            }
            ProviderEvent::BlockDone { index, block } => {
                self.partial_text.remove(index);
                self.done.insert(*index, block.clone());
            }
            _ => {}
        }
    }

    fn into_blocks(self) -> Vec<ContentBlock> {
        let mut blocks = self.done;
        for (index, text) in self.partial_text {
            blocks
                .entry(index)
                .or_insert(ContentBlock::Text(TextBlock { text }));
        }
        blocks.into_values().collect()
    }
}

pub async fn drive(
    provider: &dyn Provider,
    request: ProviderRequest,
    cancel: CancellationToken,
    correlation_id: &str,
    sink: &impl FrameSink,
) -> TurnRun {
    let mut collector = BlockCollector::default();
    let outcome = match provider.stream(request, cancel.clone()).await {
        Ok(events) => pump(events, &cancel, &mut collector, sink).await,
        Err(error) => Err(error),
    };
    let close = match outcome {
        Ok(stop_reason) => CloseReason::Completed { stop_reason },
        Err(_) if cancel.is_cancelled() => CloseReason::Cancelled,
        Err(ProviderError::Cancelled) => CloseReason::Cancelled,
        Err(error) => {
            tracing::warn!(correlation_id, code = error.code().as_str(), %error, "turn failed");
            CloseReason::Failed {
                error: errors::from_provider(&error, correlation_id),
            }
        }
    };
    TurnRun {
        blocks: collector.into_blocks(),
        close,
    }
}

async fn pump(
    mut events: ProviderStream,
    cancel: &CancellationToken,
    collector: &mut BlockCollector,
    sink: &impl FrameSink,
) -> Result<StopReason, ProviderError> {
    let mut coalescer = Coalescer::default();
    let mut stop_reason = None;
    let result = loop {
        let deadline = coalescer.deadline();
        let flush_timer = async move {
            match deadline {
                Some(deadline) => sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = cancel.cancelled() => break Err(ProviderError::Cancelled),
            item = events.next() => match item {
                None => break stop_reason.ok_or(ProviderError::TruncatedStream),
                Some(Err(error)) => break Err(error),
                Some(Ok(event)) => {
                    if let ProviderEvent::MessageStop { stop_reason: reason, .. } = &event {
                        stop_reason = Some(*reason);
                    }
                    collector.observe(&event);
                    if let Some(ui_event) = to_ui_event(event)
                        && let Some(frame) = coalescer.push(ui_event, Instant::now())
                        && let Err(error) = deliver(sink, frame, cancel)
                    {
                        break Err(error);
                    }
                }
            },
            () = flush_timer => {
                if let Some(frame) = coalescer.flush()
                    && let Err(error) = deliver(sink, frame, cancel)
                {
                    break Err(error);
                }
            }
        }
    };
    if let Some(frame) = coalescer.flush() {
        deliver(sink, frame, cancel)?;
    }
    result
}

fn deliver(
    sink: &impl FrameSink,
    frame: StreamFrame,
    cancel: &CancellationToken,
) -> Result<(), ProviderError> {
    sink.send(frame).map_err(|error| {
        tracing::warn!(%error, "frame delivery failed, cancelling the turn");
        cancel.cancel();
        ProviderError::Cancelled
    })
}

fn to_ui_event(event: ProviderEvent) -> Option<UiEvent> {
    match event {
        ProviderEvent::TextDelta { index, text } => Some(UiEvent::TextDelta { index, text }),
        ProviderEvent::ReasoningSummaryDelta { index, text } => {
            Some(UiEvent::ReasoningDelta { index, text })
        }
        ProviderEvent::BlockDone { index, block } => {
            let block = match block {
                ContentBlock::Text(text) => CompletedBlock::Text { text: text.text },
                ContentBlock::Reasoning(reasoning) => CompletedBlock::Reasoning {
                    summary: reasoning.summary.unwrap_or_default(),
                },
                ContentBlock::ToolUse(_) | ContentBlock::ToolResult(_) => return None,
            };
            Some(UiEvent::BlockDone { index, block })
        }
        ProviderEvent::Warning { code, message } => {
            tracing::warn!(code, message, "provider warning");
            None
        }
        ProviderEvent::MessageStart { .. }
        | ProviderEvent::BlockStart { .. }
        | ProviderEvent::ToolArgumentsDelta { .. }
        | ProviderEvent::UsageUpdate { .. }
        | ProviderEvent::MessageStop { .. } => None,
    }
}
