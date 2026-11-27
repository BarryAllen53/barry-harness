use bh_provider::{CacheCapabilities, CacheMode, CacheTtl, ProviderCapabilities, ReasoningDisplay};
use bh_types::Effort;

const CURRENT_FAMILY_PREFIXES: [&str; 4] = [
    "claude-opus-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
    "claude-fable-5",
];

pub fn capabilities_for(model: &str) -> ProviderCapabilities {
    if CURRENT_FAMILY_PREFIXES
        .iter()
        .any(|prefix| model.starts_with(prefix))
    {
        current_family()
    } else {
        unknown_model()
    }
}

fn current_family() -> ProviderCapabilities {
    ProviderCapabilities {
        context_window: 1_000_000,
        max_output_tokens: 128_000,
        efforts: vec![
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ],
        reasoning_display: vec![ReasoningDisplay::Summarized, ReasoningDisplay::Omitted],
        tools: true,
        parallel_tool_calls: true,
        vision_input: true,
        cache: CacheCapabilities {
            mode: CacheMode::ExplicitBreakpoints,
            max_breakpoints: 4,
            min_prefix_tokens: 512,
            ttls: vec![CacheTtl::FiveMinutes, CacheTtl::OneHour],
            lookback_blocks: 20,
        },
        streaming_tool_args: true,
    }
}

fn unknown_model() -> ProviderCapabilities {
    ProviderCapabilities {
        context_window: 200_000,
        max_output_tokens: 32_000,
        efforts: vec![Effort::Low, Effort::Medium, Effort::High],
        reasoning_display: vec![ReasoningDisplay::Summarized, ReasoningDisplay::Omitted],
        tools: true,
        parallel_tool_calls: true,
        vision_input: true,
        cache: CacheCapabilities {
            mode: CacheMode::ExplicitBreakpoints,
            max_breakpoints: 4,
            min_prefix_tokens: 4096,
            ttls: vec![CacheTtl::FiveMinutes],
            lookback_blocks: 20,
        },
        streaming_tool_args: true,
    }
}
