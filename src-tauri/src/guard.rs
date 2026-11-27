use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use bh_types::{
    ContentBlock,
    ipc::{CloseReason, StreamFrame},
};

use crate::{
    errors,
    session::{Commit, Conversation},
    turn::{FrameSink, TurnRun},
};

#[derive(Clone, Debug, Default)]
pub struct Conversations(Arc<Mutex<HashMap<String, Conversation>>>);

impl Conversations {
    pub fn lock(&self) -> MutexGuard<'_, HashMap<String, Conversation>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn reset(&self, label: &str) -> bool {
        match self.lock().remove(label) {
            Some(conversation) => {
                conversation.cancel();
                true
            }
            None => false,
        }
    }
}

pub struct TurnGuard<S: FrameSink> {
    conversations: Conversations,
    label: String,
    turn: u64,
    correlation_id: String,
    sink: S,
    settled: bool,
    store: Option<StoreHook>,
}

#[derive(Clone)]
pub struct StoreHook {
    pub store: std::sync::Arc<bh_store::Store>,
    pub session: String,
    pub title: String,
    pub user: String,
}

impl<S: FrameSink> TurnGuard<S> {
    pub fn new(conversations: Conversations, label: String, turn: u64, sink: S) -> Self {
        Self {
            conversations,
            label,
            turn,
            correlation_id: format!("turn-{turn}"),
            sink,
            settled: false,
            store: None,
        }
    }

    pub fn with_store(mut self, hook: StoreHook) -> Self {
        self.store = Some(hook);
        self
    }

    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn settle(mut self, run: TurnRun) {
        self.close(run.blocks, run.close);
    }

    fn close(&mut self, blocks: Vec<ContentBlock>, reason: CloseReason) {
        if self.settled {
            return;
        }
        self.settled = true;
        let assistant = blocks_text(&blocks);
        let commit = self
            .conversations
            .lock()
            .get_mut(&self.label)
            .map_or(Commit::Stale, |conversation| {
                conversation.finish(self.turn, blocks)
            });
        tracing::info!(
            correlation_id = self.correlation_id,
            ?commit,
            "turn finished"
        );
        if let (Commit::Appended, Some(hook)) = (commit, &self.store)
            && let Err(error) =
                hook.store
                    .append_turn(&hook.session, &hook.title, &hook.user, &assistant)
        {
            tracing::warn!(%error, "the turn was not saved");
        }
        if let Err(error) = self.sink.send(StreamFrame::Closed { reason }) {
            tracing::warn!(
                correlation_id = self.correlation_id,
                %error,
                "closing frame was not delivered"
            );
        }
    }
}

fn blocks_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
impl<S: FrameSink> Drop for TurnGuard<S> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        tracing::error!(
            correlation_id = self.correlation_id,
            "turn ended without settling"
        );
        let error = errors::internal(&self.correlation_id);
        self.close(Vec::new(), CloseReason::Failed { error });
    }
}
