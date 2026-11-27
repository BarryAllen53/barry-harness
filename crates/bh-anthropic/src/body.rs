use bh_provider::{
    BreakpointAt, CachePlan, CacheTtl, ProviderCapabilities, ProviderError, ProviderRequest,
    ReasoningDisplay,
};
use bh_types::{
    ContentBlock, Effort, Message, ProviderKind, Role, ToolResultBlock, ToolResultPart,
    ToolUseBlock,
};
use serde::Serialize;
use serde_json::value::RawValue;

#[derive(Serialize)]
struct MessagesBody<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    thinking: Thinking,
    output_config: OutputConfig,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    system: Vec<WireText<'a>>,
    messages: Vec<WireMessage<'a>>,
}

#[derive(Serialize)]
struct Thinking {
    #[serde(rename = "type")]
    kind: &'static str,
    display: &'static str,
}

#[derive(Serialize)]
struct OutputConfig {
    effort: &'static str,
}

#[derive(Clone, Copy, Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl: Option<&'static str>,
}

impl CacheControl {
    fn new(ttl: CacheTtl) -> Self {
        Self {
            kind: "ephemeral",
            ttl: match ttl {
                CacheTtl::FiveMinutes => None,
                CacheTtl::OneHour => Some("1h"),
            },
        }
    }
}

#[derive(Serialize)]
struct WireTool<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: &'a RawValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct WireText<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'static str,
    content: Vec<WireBlock<'a>>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum WireBlock<'a> {
    Text(WireText<'a>),
    Reasoning(&'a RawValue),
    ToolUse(WireToolUse<'a>),
    ToolResult(WireToolResult<'a>),
}

#[derive(Serialize)]
struct WireToolUse<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    id: &'a str,
    name: &'a str,
    input: &'a RawValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct WireToolResult<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    tool_use_id: &'a str,
    content: Vec<WireText<'a>>,
    #[serde(skip_serializing_if = "is_false")]
    is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

struct Markers {
    last_tool: Option<CacheControl>,
    system: Vec<(usize, CacheControl)>,
    blocks: Vec<(usize, usize, CacheControl)>,
}

impl Markers {
    fn system(&self, segment: usize) -> Option<CacheControl> {
        self.system
            .iter()
            .find(|(index, _)| *index == segment)
            .map(|(_, control)| *control)
    }

    fn block(&self, message: usize, block: usize) -> Option<CacheControl> {
        self.blocks
            .iter()
            .find(|(m, b, _)| *m == message && *b == block)
            .map(|(_, _, control)| *control)
    }
}

pub(crate) fn encode(
    request: &ProviderRequest,
    capabilities: &ProviderCapabilities,
) -> Result<Vec<u8>, ProviderError> {
    let effort = effort_value(request.reasoning.effort, capabilities)?;
    let display = display_value(request.reasoning.display, capabilities)?;
    let markers = markers(&request.cache, request, capabilities)?;
    let tool_count = request.tools.len();
    let tools = request
        .tools
        .iter()
        .enumerate()
        .map(|(index, tool)| WireTool {
            name: &tool.name,
            description: &tool.description,
            input_schema: &tool.input_schema,
            cache_control: if index + 1 == tool_count {
                markers.last_tool
            } else {
                None
            },
        })
        .collect();
    let system = request
        .system
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| WireText {
            kind: "text",
            text: &segment.text,
            cache_control: markers.system(index),
        })
        .collect();
    let messages = wire_messages(&request.messages, &markers)?;
    let body = MessagesBody {
        model: &request.model,
        max_tokens: request.max_output_tokens,
        stream: true,
        thinking: Thinking {
            kind: "adaptive",
            display,
        },
        output_config: OutputConfig { effort },
        tools,
        system,
        messages,
    };
    serde_json::to_vec(&body).map_err(|error| ProviderError::InvalidRequest {
        reason: format!("request body could not be serialized: {error}"),
    })
}

fn effort_value(
    effort: Effort,
    capabilities: &ProviderCapabilities,
) -> Result<&'static str, ProviderError> {
    let value = match effort {
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max => "max",
        Effort::None | Effort::Minimal => {
            return Err(ProviderError::InvalidRequest {
                reason: format!("effort {effort:?} is not supported by Anthropic models"),
            });
        }
    };
    if capabilities.efforts.contains(&effort) {
        Ok(value)
    } else {
        Err(ProviderError::InvalidRequest {
            reason: format!("effort {value} is not supported by this model"),
        })
    }
}

fn display_value(
    display: ReasoningDisplay,
    capabilities: &ProviderCapabilities,
) -> Result<&'static str, ProviderError> {
    if !capabilities.reasoning_display.contains(&display) {
        return Err(ProviderError::InvalidRequest {
            reason: format!("reasoning display {display:?} is not supported by this model"),
        });
    }
    Ok(match display {
        ReasoningDisplay::Summarized => "summarized",
        ReasoningDisplay::Omitted => "omitted",
    })
}

fn invalid_plan(reason: impl Into<String>) -> ProviderError {
    ProviderError::InvalidRequest {
        reason: format!("invalid cache plan: {}", reason.into()),
    }
}

fn markers(
    plan: &CachePlan,
    request: &ProviderRequest,
    capabilities: &ProviderCapabilities,
) -> Result<Markers, ProviderError> {
    if plan.breakpoints.len() > usize::from(capabilities.cache.max_breakpoints) {
        return Err(invalid_plan(format!(
            "{} breakpoints exceed the limit of {}",
            plan.breakpoints.len(),
            capabilities.cache.max_breakpoints
        )));
    }
    let mut markers = Markers {
        last_tool: None,
        system: Vec::new(),
        blocks: Vec::new(),
    };
    for (position, breakpoint) in plan.breakpoints.iter().enumerate() {
        if let Some(previous) = position.checked_sub(1).map(|index| plan.breakpoints[index]) {
            if previous.at >= breakpoint.at {
                return Err(invalid_plan(
                    "breakpoints must be unique and in prompt order",
                ));
            }
            if previous.ttl < breakpoint.ttl {
                return Err(invalid_plan("longer TTL entries must precede shorter ones"));
            }
        }
        if !capabilities.cache.ttls.contains(&breakpoint.ttl) {
            return Err(invalid_plan(format!(
                "TTL {:?} is not supported by this model",
                breakpoint.ttl
            )));
        }
        let control = CacheControl::new(breakpoint.ttl);
        match breakpoint.at {
            BreakpointAt::LastTool => {
                if request.tools.is_empty() {
                    return Err(invalid_plan("tool breakpoint without tools"));
                }
                markers.last_tool = Some(control);
            }
            BreakpointAt::SystemSegment(segment) => {
                if segment >= request.system.segments.len() {
                    return Err(invalid_plan(format!(
                        "system segment {segment} does not exist"
                    )));
                }
                markers.system.push((segment, control));
            }
            BreakpointAt::MessageBlock { message, block } => {
                let target = request
                    .messages
                    .get(message)
                    .and_then(|candidate| candidate.blocks.get(block))
                    .ok_or_else(|| {
                        invalid_plan(format!("message {message} block {block} does not exist"))
                    })?;
                if matches!(target, ContentBlock::Reasoning(_)) {
                    return Err(invalid_plan(format!(
                        "message {message} block {block} is a reasoning block"
                    )));
                }
                markers.blocks.push((message, block, control));
            }
        }
    }
    Ok(markers)
}

fn wire_messages<'a>(
    messages: &'a [Message],
    markers: &Markers,
) -> Result<Vec<WireMessage<'a>>, ProviderError> {
    let mut wire_messages: Vec<WireMessage<'a>> = Vec::with_capacity(messages.len());
    for (index, message) in messages.iter().enumerate() {
        let wire = wire_message(index, message, markers)?;
        if wire.content.is_empty() {
            continue;
        }
        match wire_messages.last_mut() {
            Some(previous) if previous.role == wire.role => previous.content.extend(wire.content),
            _ => wire_messages.push(wire),
        }
    }
    Ok(wire_messages)
}

fn wire_message<'a>(
    message_index: usize,
    message: &'a Message,
    markers: &Markers,
) -> Result<WireMessage<'a>, ProviderError> {
    let mut content = Vec::with_capacity(message.blocks.len());
    for (block_index, block) in message.blocks.iter().enumerate() {
        let cache_control = markers.block(message_index, block_index);
        let wire = match block {
            ContentBlock::Text(text) => WireBlock::Text(WireText {
                kind: "text",
                text: &text.text,
                cache_control,
            }),
            ContentBlock::Reasoning(reasoning) => {
                if reasoning.provider != ProviderKind::Anthropic {
                    continue;
                }
                WireBlock::Reasoning(&reasoning.raw)
            }
            ContentBlock::ToolUse(tool_use) => {
                WireBlock::ToolUse(wire_tool_use(tool_use, cache_control)?)
            }
            ContentBlock::ToolResult(result) => {
                WireBlock::ToolResult(wire_tool_result(result, cache_control))
            }
        };
        content.push(wire);
    }
    Ok(WireMessage {
        role: match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        },
        content,
    })
}

fn wire_tool_use(
    tool_use: &ToolUseBlock,
    cache_control: Option<CacheControl>,
) -> Result<WireToolUse<'_>, ProviderError> {
    let input =
        serde_json::from_str::<&RawValue>(tool_use.arguments.as_str()).map_err(|error| {
            ProviderError::InvalidRequest {
                reason: format!(
                    "tool call {} arguments are not JSON: {error}",
                    tool_use.call_id
                ),
            }
        })?;
    Ok(WireToolUse {
        kind: "tool_use",
        id: tool_use.call_id.as_str(),
        name: &tool_use.name,
        input,
        cache_control,
    })
}

fn wire_tool_result(
    result: &ToolResultBlock,
    cache_control: Option<CacheControl>,
) -> WireToolResult<'_> {
    WireToolResult {
        kind: "tool_result",
        tool_use_id: result.call_id.as_str(),
        content: result
            .content
            .iter()
            .map(|part| match part {
                ToolResultPart::Text { text } => WireText {
                    kind: "text",
                    text,
                    cache_control: None,
                },
            })
            .collect(),
        is_error: result.is_error,
        cache_control,
    }
}
