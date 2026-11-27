use std::collections::{BTreeMap, HashSet, VecDeque};

use bh_provider::{BlockKind, ProviderError, ProviderEvent, SseEvent};
use bh_types::{
    ContentBlock, JsonText, ProviderKind, ReasoningBlock, StopReason, TextBlock, ToolCallId,
    ToolUseBlock, Usage,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

pub(crate) type StreamItem = Result<ProviderEvent, ProviderError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    Continue,
    Done,
}

#[derive(Deserialize)]
struct Tagged<'a> {
    #[serde(rename = "type", borrow)]
    kind: &'a str,
}

#[derive(Deserialize)]
struct MessageStartEvent {
    message: WireMessage,
}

#[derive(Deserialize)]
struct WireMessage {
    id: String,
    model: String,
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize)]
struct BlockStartEvent<'a> {
    index: u32,
    #[serde(borrow)]
    content_block: &'a RawValue,
}

#[derive(Deserialize)]
struct TextStart {
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct ThinkingStart {
    #[serde(default)]
    thinking: String,
    #[serde(default)]
    signature: String,
}

#[derive(Deserialize)]
struct ToolUseStart {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct BlockDeltaEvent {
    index: u32,
    delta: WireDelta,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireDelta {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        signature: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct BlockStopEvent {
    index: u32,
}

#[derive(Deserialize)]
struct MessageDeltaEvent {
    delta: WireMessageDelta,
    #[serde(default)]
    usage: WireUsage,
}

#[derive(Deserialize)]
struct WireMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ErrorEvent {
    error: WireError,
}

#[derive(Deserialize)]
struct WireError {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    message: String,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct WireUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<WireCacheCreation>,
}

#[derive(Clone, Copy, Default, Deserialize)]
struct WireCacheCreation {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

impl WireUsage {
    fn merge(&mut self, update: WireUsage) {
        overwrite(&mut self.input_tokens, update.input_tokens);
        overwrite(&mut self.output_tokens, update.output_tokens);
        overwrite(
            &mut self.cache_creation_input_tokens,
            update.cache_creation_input_tokens,
        );
        overwrite(
            &mut self.cache_read_input_tokens,
            update.cache_read_input_tokens,
        );
        if let Some(update) = update.cache_creation {
            let current = self.cache_creation.get_or_insert_default();
            overwrite(
                &mut current.ephemeral_5m_input_tokens,
                update.ephemeral_5m_input_tokens,
            );
            overwrite(
                &mut current.ephemeral_1h_input_tokens,
                update.ephemeral_1h_input_tokens,
            );
        }
    }

    fn normalize(&self) -> Usage {
        let (five_minute_writes, one_hour_writes) = match self.cache_creation {
            Some(creation) => (
                creation.ephemeral_5m_input_tokens.unwrap_or(0),
                creation.ephemeral_1h_input_tokens.unwrap_or(0),
            ),
            None => (self.cache_creation_input_tokens.unwrap_or(0), 0),
        };
        Usage {
            input_tokens: self.input_tokens.unwrap_or(0),
            output_tokens: self.output_tokens.unwrap_or(0),
            cache_read_tokens: self.cache_read_input_tokens.unwrap_or(0),
            cache_write_5m_tokens: five_minute_writes,
            cache_write_1h_tokens: one_hour_writes,
            reasoning_tokens: 0,
        }
    }
}

fn overwrite(target: &mut Option<u64>, update: Option<u64>) {
    if update.is_some() {
        *target = update;
    }
}

#[derive(Serialize)]
struct ThinkingRaw<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    thinking: &'a str,
    signature: &'a str,
}

enum PartialBlock {
    Text(String),
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking(Box<RawValue>),
    ToolUse {
        call_id: ToolCallId,
        name: String,
        arguments: String,
    },
    Ignored,
}

#[derive(Default)]
pub(crate) struct Assembler {
    open_blocks: BTreeMap<u32, PartialBlock>,
    started: HashSet<u32>,
    usage: WireUsage,
    stop_reason: Option<String>,
    warned: HashSet<String>,
    started_message: bool,
    complete: bool,
}

impl Assembler {
    pub(crate) fn is_complete(&self) -> bool {
        self.complete
    }

    pub(crate) fn push(&mut self, event: &SseEvent, out: &mut VecDeque<StreamItem>) -> Flow {
        match self.handle(event, out) {
            Ok(flow) => flow,
            Err(error) => {
                out.push_back(Err(error));
                Flow::Done
            }
        }
    }

    fn handle(
        &mut self,
        event: &SseEvent,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<Flow, ProviderError> {
        let data = event.data.as_str();
        let tag: Tagged<'_> = parse(data, &event.event)?;
        match tag.kind {
            "message_start" => self.message_start(parse(data, tag.kind)?, out)?,
            "content_block_start" => self.block_start(&parse(data, tag.kind)?, out)?,
            "content_block_delta" => self.block_delta(parse(data, tag.kind)?, out)?,
            "content_block_stop" => self.block_stop(&parse(data, tag.kind)?, out)?,
            "message_delta" => self.message_delta(parse(data, tag.kind)?, out)?,
            "message_stop" => return self.message_stop(out),
            "ping" => {}
            "error" => {
                let ErrorEvent { error } = parse(data, tag.kind)?;
                return Err(ProviderError::Stream {
                    kind: error.kind,
                    message: error.message,
                });
            }
            unknown => self.warn_once("unknown_event", unknown, out),
        }
        Ok(Flow::Continue)
    }

    fn require_message(&self, kind: &str) -> Result<(), ProviderError> {
        if self.started_message {
            Ok(())
        } else {
            Err(protocol(format!("{kind} arrived before message_start")))
        }
    }

    fn message_start(
        &mut self,
        event: MessageStartEvent,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<(), ProviderError> {
        if self.started_message {
            return Err(protocol("message_start arrived twice"));
        }
        self.started_message = true;
        self.usage.merge(event.message.usage);
        out.push_back(Ok(ProviderEvent::MessageStart {
            id: event.message.id,
            model: event.message.model,
            usage: self.usage.normalize(),
        }));
        Ok(())
    }

    fn block_start(
        &mut self,
        event: &BlockStartEvent<'_>,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<(), ProviderError> {
        self.require_message("content_block_start")?;
        if !self.started.insert(event.index) {
            return Err(protocol(format!(
                "content block {} started twice",
                event.index
            )));
        }
        let raw = event.content_block.get();
        let tag: Tagged<'_> = parse(raw, "content_block")?;
        let (partial, kind) = match tag.kind {
            "text" => {
                let start: TextStart = parse(raw, tag.kind)?;
                (PartialBlock::Text(start.text), Some(BlockKind::Text))
            }
            "thinking" => {
                let start: ThinkingStart = parse(raw, tag.kind)?;
                (
                    PartialBlock::Thinking {
                        thinking: start.thinking,
                        signature: start.signature,
                    },
                    Some(BlockKind::Reasoning),
                )
            }
            "redacted_thinking" => (
                PartialBlock::RedactedThinking(event.content_block.to_owned()),
                Some(BlockKind::Reasoning),
            ),
            "tool_use" => {
                let start: ToolUseStart = parse(raw, tag.kind)?;
                let call_id = ToolCallId::new(start.id);
                (
                    PartialBlock::ToolUse {
                        call_id: call_id.clone(),
                        name: start.name.clone(),
                        arguments: String::new(),
                    },
                    Some(BlockKind::ToolUse {
                        call_id,
                        name: start.name,
                    }),
                )
            }
            unknown => {
                self.warn_once("unknown_block", unknown, out);
                (PartialBlock::Ignored, None)
            }
        };
        self.open_blocks.insert(event.index, partial);
        if let Some(kind) = kind {
            out.push_back(Ok(ProviderEvent::BlockStart {
                index: event.index,
                kind,
            }));
        }
        Ok(())
    }

    fn block_delta(
        &mut self,
        event: BlockDeltaEvent,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<(), ProviderError> {
        let index = event.index;
        let block = self
            .open_blocks
            .get_mut(&index)
            .ok_or_else(|| protocol(format!("delta for content block {index} that is not open")))?;
        let emitted = match (block, event.delta) {
            (PartialBlock::Ignored, _) => None,
            (_, WireDelta::Unknown) => {
                self.warn_once("unknown_delta", "content_block_delta", out);
                None
            }
            (PartialBlock::Text(text), WireDelta::TextDelta { text: delta }) => {
                text.push_str(&delta);
                Some(ProviderEvent::TextDelta { index, text: delta })
            }
            (
                PartialBlock::Thinking { thinking, .. },
                WireDelta::ThinkingDelta { thinking: delta },
            ) => {
                thinking.push_str(&delta);
                Some(ProviderEvent::ReasoningSummaryDelta { index, text: delta })
            }
            (
                PartialBlock::Thinking { signature, .. },
                WireDelta::SignatureDelta { signature: delta },
            ) => {
                signature.push_str(&delta);
                None
            }
            (
                PartialBlock::ToolUse { arguments, .. },
                WireDelta::InputJsonDelta { partial_json },
            ) => {
                arguments.push_str(&partial_json);
                Some(ProviderEvent::ToolArgumentsDelta {
                    index,
                    fragment: partial_json,
                })
            }
            _ => {
                return Err(protocol(format!(
                    "delta type does not match content block {index}"
                )));
            }
        };
        if let Some(emitted) = emitted {
            out.push_back(Ok(emitted));
        }
        Ok(())
    }

    fn block_stop(
        &mut self,
        event: &BlockStopEvent,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<(), ProviderError> {
        let index = event.index;
        let block = self
            .open_blocks
            .remove(&index)
            .ok_or_else(|| protocol(format!("stop for content block {index} that is not open")))?;
        let block = match block {
            PartialBlock::Ignored => return Ok(()),
            PartialBlock::Text(text) => ContentBlock::Text(TextBlock { text }),
            PartialBlock::Thinking {
                thinking,
                signature,
            } => ContentBlock::Reasoning(thinking_block(thinking, &signature)?),
            PartialBlock::RedactedThinking(raw) => ContentBlock::Reasoning(ReasoningBlock {
                provider: ProviderKind::Anthropic,
                raw,
                summary: None,
            }),
            PartialBlock::ToolUse {
                call_id,
                name,
                arguments,
            } => match tool_use_block(call_id, name, arguments) {
                Ok(block) => block,
                Err(error) => {
                    out.push_back(Err(error));
                    return Ok(());
                }
            },
        };
        out.push_back(Ok(ProviderEvent::BlockDone { index, block }));
        Ok(())
    }

    fn message_delta(
        &mut self,
        event: MessageDeltaEvent,
        out: &mut VecDeque<StreamItem>,
    ) -> Result<(), ProviderError> {
        self.require_message("message_delta")?;
        if event.delta.stop_reason.is_some() {
            self.stop_reason = event.delta.stop_reason;
        }
        self.usage.merge(event.usage);
        out.push_back(Ok(ProviderEvent::UsageUpdate {
            usage: self.usage.normalize(),
        }));
        Ok(())
    }

    fn message_stop(&mut self, out: &mut VecDeque<StreamItem>) -> Result<Flow, ProviderError> {
        self.require_message("message_stop")?;
        if let Some(index) = self.open_blocks.keys().next() {
            return Err(protocol(format!(
                "message_stop arrived while content block {index} is open"
            )));
        }
        let raw_stop = self.stop_reason.take();
        let stop_reason = raw_stop.as_deref().map_or(StopReason::Other, stop_reason);
        self.complete = true;
        out.push_back(Ok(ProviderEvent::MessageStop {
            stop_reason,
            raw_stop,
            usage: self.usage.normalize(),
        }));
        Ok(Flow::Done)
    }

    fn warn_once(&mut self, code: &str, kind: &str, out: &mut VecDeque<StreamItem>) {
        if self.warned.insert(format!("{code}:{kind}")) {
            out.push_back(Ok(ProviderEvent::Warning {
                code: code.to_owned(),
                message: format!("ignored unsupported {kind}"),
            }));
        }
    }
}

fn parse<'a, T: Deserialize<'a>>(data: &'a str, what: &str) -> Result<T, ProviderError> {
    serde_json::from_str(data)
        .map_err(|error| protocol(format!("malformed {what} payload: {error}")))
}

fn protocol(message: impl Into<String>) -> ProviderError {
    ProviderError::Protocol {
        message: message.into(),
    }
}

fn thinking_block(thinking: String, signature: &str) -> Result<ReasoningBlock, ProviderError> {
    let raw = serde_json::to_string(&ThinkingRaw {
        kind: "thinking",
        thinking: &thinking,
        signature,
    })
    .and_then(RawValue::from_string)
    .map_err(|error| protocol(format!("thinking block could not be encoded: {error}")))?;
    Ok(ReasoningBlock {
        provider: ProviderKind::Anthropic,
        raw,
        summary: (!thinking.is_empty()).then_some(thinking),
    })
}

fn tool_use_block(
    call_id: ToolCallId,
    name: String,
    arguments: String,
) -> Result<ContentBlock, ProviderError> {
    let compact = if arguments.trim().is_empty() {
        "{}".to_owned()
    } else {
        compact_json(&arguments)
    };
    match JsonText::new(compact) {
        Ok(json) if json.as_str().starts_with('{') => Ok(ContentBlock::ToolUse(ToolUseBlock {
            call_id,
            name,
            arguments: json,
            provider_item_id: None,
        })),
        _ => Err(ProviderError::MalformedToolArguments {
            call_id,
            raw: arguments,
        }),
    }
}

fn compact_json(json: &str) -> String {
    let mut output = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in json.chars() {
        if in_string {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if !matches!(character, ' ' | '\t' | '\n' | '\r') {
            if character == '"' {
                in_string = true;
            }
            output.push(character);
        }
    }
    output
}

fn stop_reason(raw: &str) -> StopReason {
    match raw {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        "pause_turn" => StopReason::PauseTurn,
        "model_context_window_exceeded" => StopReason::ContextExceeded,
        _ => StopReason::Other,
    }
}
