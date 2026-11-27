use std::collections::BTreeMap;

use bh_types::ipc::{CloseReason, CompletedBlock, ErrorEnvelope, StreamFrame, UiEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Reasoning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockBuffer {
    pub kind: BlockKind,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnView {
    blocks: BTreeMap<u32, BlockBuffer>,
    outcome: Option<CloseReason>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub reasoning_progressed: bool,
    pub reasoning_finished: bool,
    pub closed: bool,
}

impl TurnView {
    pub fn apply(&mut self, frame: StreamFrame) -> Applied {
        let mut applied = Applied::default();
        if self.outcome.is_some() {
            return applied;
        }
        match frame {
            StreamFrame::Batch { events } => {
                for event in events {
                    self.apply_event(event, &mut applied);
                }
            }
            StreamFrame::Closed { reason } => {
                self.outcome = Some(reason);
                applied.closed = true;
            }
        }
        applied
    }

    pub fn fail(&mut self, error: ErrorEnvelope) -> bool {
        if self.outcome.is_some() {
            return false;
        }
        self.outcome = Some(CloseReason::Failed { error });
        true
    }

    pub fn outcome(&self) -> Option<&CloseReason> {
        self.outcome.as_ref()
    }

    pub fn answer_text(&self) -> String {
        self.joined(BlockKind::Text)
    }

    pub fn reasoning_text(&self) -> String {
        self.joined(BlockKind::Reasoning)
    }

    fn joined(&self, kind: BlockKind) -> String {
        self.blocks
            .values()
            .filter(|block| block.kind == kind && !block.text.trim().is_empty())
            .map(|block| block.text.trim_end())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn apply_event(&mut self, event: UiEvent, applied: &mut Applied) {
        match event {
            UiEvent::TextDelta { index, text } => self.append(index, BlockKind::Text, &text),
            UiEvent::ReasoningDelta { index, text } => {
                applied.reasoning_progressed = true;
                self.append(index, BlockKind::Reasoning, &text);
            }
            UiEvent::BlockDone { index, block } => {
                let (kind, text) = match block {
                    CompletedBlock::Text { text } => (BlockKind::Text, text),
                    CompletedBlock::Reasoning { summary } => {
                        applied.reasoning_finished = true;
                        (BlockKind::Reasoning, summary)
                    }
                };
                self.blocks.insert(
                    index,
                    BlockBuffer {
                        kind,
                        text,
                        done: true,
                    },
                );
            }
        }
    }

    fn append(&mut self, index: u32, kind: BlockKind, text: &str) {
        let block = self.blocks.entry(index).or_insert_with(|| BlockBuffer {
            kind,
            text: String::new(),
            done: false,
        });
        if !block.done && block.kind == kind {
            block.text.push_str(text);
        }
    }
}
