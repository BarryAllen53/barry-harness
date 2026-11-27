use bh_provider::{Breakpoint, BreakpointAt, CacheCapabilities, CachePlan, CacheTtl};
use bh_types::{ContentBlock, Message, Role, Stability, SystemPrompt, ToolResultPart, ToolSpec};

const CHARACTERS_PER_TOKEN: u64 = 4;

pub fn plan_cache(
    tools: &[ToolSpec],
    system: &SystemPrompt,
    messages: &[Message],
    capabilities: &CacheCapabilities,
) -> CachePlan {
    let minimum = u64::from(capabilities.min_prefix_tokens);
    let stable_ttl = if capabilities.ttls.contains(&CacheTtl::OneHour) {
        CacheTtl::OneHour
    } else {
        CacheTtl::FiveMinutes
    };
    let mut characters: u64 = 0;
    let mut breakpoints = Vec::new();

    characters += tools.iter().map(tool_characters).sum::<u64>();
    if !tools.is_empty() && tokens(characters) >= minimum {
        breakpoints.push(Breakpoint {
            at: BreakpointAt::LastTool,
            ttl: stable_ttl,
        });
    }

    let last_stable = system
        .segments
        .iter()
        .rposition(|segment| segment.stability <= Stability::Project);
    let mut system_characters = system.segments.iter().map(|segment| count(&segment.text));
    if let Some(last_stable) = last_stable {
        characters += system_characters
            .by_ref()
            .take(last_stable + 1)
            .sum::<u64>();
        if tokens(characters) >= minimum {
            breakpoints.push(Breakpoint {
                at: BreakpointAt::SystemSegment(last_stable),
                ttl: stable_ttl,
            });
        }
    }
    characters += system_characters.sum::<u64>();

    let anchor_message = previous_request_tail(messages);
    let tail_message = messages.len().checked_sub(1);
    let mut anchor = None;
    let mut tail = None;
    for (message_index, message) in messages.iter().enumerate() {
        for (block_index, block) in message.blocks.iter().enumerate() {
            characters += block_characters(block);
            if matches!(block, ContentBlock::Reasoning(_)) || tokens(characters) < minimum {
                continue;
            }
            let position = BreakpointAt::MessageBlock {
                message: message_index,
                block: block_index,
            };
            if Some(message_index) == anchor_message {
                anchor = Some(position);
            }
            if Some(message_index) == tail_message {
                tail = Some(position);
            }
        }
    }
    let anchor =
        anchor.filter(|_| breakpoints.len() + 2 <= usize::from(capabilities.max_breakpoints));
    for at in [anchor, tail].into_iter().flatten() {
        breakpoints.push(Breakpoint {
            at,
            ttl: CacheTtl::FiveMinutes,
        });
    }
    breakpoints.truncate(usize::from(capabilities.max_breakpoints));
    CachePlan { breakpoints }
}

fn previous_request_tail(messages: &[Message]) -> Option<usize> {
    let last = messages.len().checked_sub(1)?;
    (0..last).rev().find(|&index| {
        messages[index].role == Role::User && messages[index + 1].role == Role::Assistant
    })
}

fn tokens(characters: u64) -> u64 {
    characters / CHARACTERS_PER_TOKEN
}

fn count(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

fn tool_characters(tool: &ToolSpec) -> u64 {
    count(&tool.name) + count(&tool.description) + count(tool.input_schema.get())
}

fn block_characters(block: &ContentBlock) -> u64 {
    match block {
        ContentBlock::Text(text) => count(&text.text),
        ContentBlock::Reasoning(reasoning) => count(reasoning.raw.get()),
        ContentBlock::ToolUse(tool_use) => {
            count(&tool_use.name) + count(tool_use.arguments.as_str())
        }
        ContentBlock::ToolResult(result) => result
            .content
            .iter()
            .map(|part| match part {
                ToolResultPart::Text { text } => count(text),
            })
            .sum(),
    }
}
