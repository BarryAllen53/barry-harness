use bh_types::{Effort, Message, SystemPrompt, ToolSpec};

#[derive(Clone, Debug)]
pub struct ProviderRequest {
    pub model: String,
    pub system: SystemPrompt,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_output_tokens: u32,
    pub reasoning: ReasoningConfig,
    pub cache: CachePlan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReasoningConfig {
    pub effort: Effort,
    pub display: ReasoningDisplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReasoningDisplay {
    Summarized,
    Omitted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CachePlan {
    pub breakpoints: Vec<Breakpoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Breakpoint {
    pub at: BreakpointAt,
    pub ttl: CacheTtl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BreakpointAt {
    LastTool,
    SystemSegment(usize),
    MessageBlock { message: usize, block: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheTtl {
    FiveMinutes,
    OneHour,
}
