use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use serde_json::value::RawValue;

use crate::{JsonText, ModelRef, ProviderKind, ToolCallId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    pub origin: Option<ModelRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextBlock),
    Reasoning(ReasoningBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
}

const CONTENT_BLOCK_VARIANTS: &[&str] = &["text", "reasoning", "tool_use", "tool_result"];

#[derive(Deserialize)]
struct BlockTag<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
}

impl<'de> Deserialize<'de> for ContentBlock {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        let json = raw.get();
        let tag: BlockTag<'_> = serde_json::from_str(json).map_err(D::Error::custom)?;
        let block = match tag.kind.as_ref() {
            "text" => serde_json::from_str(json).map(Self::Text),
            "reasoning" => serde_json::from_str(json).map(Self::Reasoning),
            "tool_use" => serde_json::from_str(json).map(Self::ToolUse),
            "tool_result" => serde_json::from_str(json).map(Self::ToolResult),
            other => return Err(D::Error::unknown_variant(other, CONTENT_BLOCK_VARIANTS)),
        };
        block.map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReasoningBlock {
    pub provider: ProviderKind,
    pub raw: Box<RawValue>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolUseBlock {
    pub call_id: ToolCallId,
    pub name: String,
    pub arguments: JsonText,
    pub provider_item_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultBlock {
    pub call_id: ToolCallId,
    pub content: Vec<ToolResultPart>,
    pub is_error: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultPart {
    Text { text: String },
}
