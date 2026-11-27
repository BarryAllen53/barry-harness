use bh_types::{ContentBlock, StopReason, ToolCallId, Usage};

#[derive(Clone, Debug)]
pub enum ProviderEvent {
    MessageStart {
        id: String,
        model: String,
        usage: Usage,
    },
    BlockStart {
        index: u32,
        kind: BlockKind,
    },
    TextDelta {
        index: u32,
        text: String,
    },
    ReasoningSummaryDelta {
        index: u32,
        text: String,
    },
    ToolArgumentsDelta {
        index: u32,
        fragment: String,
    },
    BlockDone {
        index: u32,
        block: ContentBlock,
    },
    UsageUpdate {
        usage: Usage,
    },
    MessageStop {
        stop_reason: StopReason,
        raw_stop: Option<String>,
        usage: Usage,
    },
    Warning {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Reasoning,
    ToolUse { call_id: ToolCallId, name: String },
}
