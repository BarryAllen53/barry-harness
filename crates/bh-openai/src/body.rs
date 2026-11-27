use bh_provider::{ProviderError, ProviderRequest};
use bh_types::{ContentBlock, Effort, Role};
use serde_json::{Value, json};

use crate::OpenAiAuth;

pub fn encode(request: &ProviderRequest, auth: &OpenAiAuth) -> Result<Vec<u8>, ProviderError> {
    let mut input = Vec::new();
    for message in &request.messages {
        let kind = match message.role {
            Role::User => "input_text",
            Role::Assistant => "output_text",
        };
        let mut parts = Vec::new();
        for block in &message.blocks {
            if let ContentBlock::Text(text) = block {
                parts.push(json!({ "type": kind, "text": text.text }));
            }
        }
        if parts.is_empty() {
            continue;
        }
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        input.push(json!({ "role": role, "content": parts }));
    }
    let instructions = request
        .system
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = json!({
        "model": request.model,
        "store": false,
        "stream": true,
        "input": input,
        "max_output_tokens": request.max_output_tokens,
    });
    if !instructions.is_empty() {
        body["instructions"] = Value::String(instructions);
    }
    if let Some(effort) = effort(request.reasoning.effort)
        && matches!(auth, OpenAiAuth::ApiKey(_) | OpenAiAuth::ChatGpt { .. })
    {
        body["reasoning"] = json!({ "effort": effort });
    }
    serde_json::to_vec(&body).map_err(|error| ProviderError::InvalidRequest {
        reason: format!("the request body could not be encoded: {error}"),
    })
}

fn effort(effort: Effort) -> Option<&'static str> {
    match effort {
        Effort::None => None,
        Effort::Minimal | Effort::Low => Some("low"),
        Effort::Medium => Some("medium"),
        Effort::High | Effort::Xhigh | Effort::Max => Some("high"),
    }
}
