use std::sync::atomic::{AtomicU64, Ordering};

use bh_types::{ContentBlock, Message, ModelRef, Role, TextBlock};
use tokio_util::sync::CancellationToken;

static TURNS_STARTED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub struct Conversation {
    messages: Vec<Message>,
    state: TurnState,
}

#[derive(Debug, Default)]
enum TurnState {
    #[default]
    Idle,
    Running(RunningTurn),
}

#[derive(Debug)]
struct RunningTurn {
    turn: u64,
    cancel: CancellationToken,
    user: Message,
    origin: ModelRef,
}

#[derive(Debug)]
pub struct TurnTicket {
    pub turn: u64,
    pub cancel: CancellationToken,
    pub history: Vec<Message>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnBusy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    Appended,
    Discarded,
    Stale,
}

impl Conversation {
    pub fn begin(&mut self, prompt: String, origin: ModelRef) -> Result<TurnTicket, TurnBusy> {
        if matches!(self.state, TurnState::Running(_)) {
            return Err(TurnBusy);
        }
        let turn = TURNS_STARTED.fetch_add(1, Ordering::Relaxed) + 1;
        let user = Message {
            role: Role::User,
            blocks: vec![ContentBlock::Text(TextBlock { text: prompt })],
            origin: None,
        };
        let mut history = Vec::with_capacity(self.messages.len() + 1);
        history.extend_from_slice(&self.messages);
        history.push(user.clone());
        let cancel = CancellationToken::new();
        self.state = TurnState::Running(RunningTurn {
            turn,
            cancel: cancel.clone(),
            user,
            origin,
        });
        Ok(TurnTicket {
            turn,
            cancel,
            history,
        })
    }

    pub fn cancel(&self) -> bool {
        match &self.state {
            TurnState::Running(running) => {
                running.cancel.cancel();
                true
            }
            TurnState::Idle => false,
        }
    }

    pub fn finish(&mut self, turn: u64, blocks: Vec<ContentBlock>) -> Commit {
        let running = match std::mem::take(&mut self.state) {
            TurnState::Running(running) if running.turn == turn => running,
            other => {
                self.state = other;
                return Commit::Stale;
            }
        };
        if !has_visible_text(&blocks) {
            return Commit::Discarded;
        }
        self.messages.push(running.user);
        self.messages.push(Message {
            role: Role::Assistant,
            blocks,
            origin: Some(running.origin),
        });
        Commit::Appended
    }
}

fn has_visible_text(blocks: &[ContentBlock]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, ContentBlock::Text(text) if !text.text.trim().is_empty()))
}
