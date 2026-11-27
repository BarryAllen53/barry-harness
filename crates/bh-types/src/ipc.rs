use serde::{Deserialize, Serialize};

use crate::{ErrorCode, StopReason, UserAction};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub user_action: Option<UserAction>,
    pub request_id: Option<String>,
    pub correlation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamFrame {
    Batch { events: Vec<UiEvent> },
    Closed { reason: CloseReason },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    TextDelta { index: u32, text: String },
    ReasoningDelta { index: u32, text: String },
    BlockDone { index: u32, block: CompletedBlock },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletedBlock {
    Text { text: String },
    Reasoning { summary: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloseReason {
    Completed { stop_reason: StopReason },
    Cancelled,
    Failed { error: ErrorEnvelope },
}
