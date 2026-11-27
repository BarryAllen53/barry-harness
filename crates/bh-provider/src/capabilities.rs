use bh_types::Effort;

use crate::{CacheTtl, ReasoningDisplay};

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProviderCapabilities {
    pub context_window: u64,
    pub max_output_tokens: u32,
    pub efforts: Vec<Effort>,
    pub reasoning_display: Vec<ReasoningDisplay>,
    pub tools: bool,
    pub parallel_tool_calls: bool,
    pub vision_input: bool,
    pub cache: CacheCapabilities,
    pub streaming_tool_args: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheCapabilities {
    pub mode: CacheMode,
    pub max_breakpoints: u8,
    pub min_prefix_tokens: u32,
    pub ttls: Vec<CacheTtl>,
    pub lookback_blocks: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheMode {
    ExplicitBreakpoints,
    AutomaticPrefixWithKey,
}
