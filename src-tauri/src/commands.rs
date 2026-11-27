#![allow(clippy::needless_pass_by_value)]

use std::{collections::HashMap, sync::Arc, sync::Mutex, sync::PoisonError};

use bh_auth::{CredentialStore, Pending, ProviderId};
use bh_config::Settings;
use bh_provider::{ProviderRequest, ReasoningConfig, ReasoningDisplay};
use bh_types::{
    Effort, ModelRef, Stability, SystemPrompt, SystemSegment,
    ipc::{ErrorEnvelope, StreamFrame},
};
use tauri::{Window, ipc::Channel};

use crate::{
    errors,
    guard::{Conversations, StoreHook, TurnGuard},
    providers, turn,
};

pub const PROMPT_MAX_CHARS: usize = 200_000;

pub struct AppState {
    settings: Mutex<Settings>,
    store: CredentialStore,
    conversations: Conversations,
    pending: tokio::sync::Mutex<Option<(ProviderId, Pending)>>,
    sessions: Mutex<HashMap<String, String>>,
    history: Option<Arc<bh_store::Store>>,
}

impl AppState {
    pub fn from_environment() -> Self {
        Self {
            settings: Mutex::new(bh_config::load()),
            store: CredentialStore::new(),
            conversations: Conversations::default(),
            pending: tokio::sync::Mutex::new(None),
            sessions: Mutex::new(HashMap::new()),
            history: open_store(),
        }
    }

    pub fn history(&self) -> Option<&Arc<bh_store::Store>> {
        self.history.as_ref()
    }

    pub fn session(&self, label: &str) -> String {
        let mut sessions = self.sessions.lock().unwrap_or_else(PoisonError::into_inner);
        sessions
            .entry(label.to_owned())
            .or_insert_with(|| next_session_id(self.history.as_deref()))
            .clone()
    }

    pub fn new_session(&self, label: &str) -> String {
        let id = next_session_id(self.history.as_deref());
        self.sessions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_owned(), id.clone());
        id
    }
    pub fn reset_window(&self, label: &str) {
        if self.conversations.reset(label) {
            tracing::info!(window = label, "conversation reset");
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn replace_settings(&self, settings: Settings) -> Result<(), ErrorEnvelope> {
        let settings = settings.sanitized();
        bh_config::save(&settings).map_err(|message| ErrorEnvelope {
            code: bh_types::ErrorCode::Internal,
            message,
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: "settings".to_owned(),
        })?;
        *self.settings.lock().unwrap_or_else(PoisonError::into_inner) = settings;
        Ok(())
    }

    pub fn store(&self) -> &CredentialStore {
        &self.store
    }

    pub fn pending(&self) -> &tokio::sync::Mutex<Option<(ProviderId, Pending)>> {
        &self.pending
    }
}

fn next_session_id(store: Option<&bh_store::Store>) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    store.map_or_else(
        || {
            COUNTER
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                .to_string()
        },
        bh_store::Store::new_session_id,
    )
}

fn open_store() -> Option<Arc<bh_store::Store>> {
    let base = std::env::var_os("APPDATA")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let path = base.join("BarryHarness").join("sessions.db");
    match bh_store::Store::open(&path) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::warn!(%error, "sessions will not be saved");
            None
        }
    }
}
pub fn parse_effort(value: &str) -> Effort {
    match value {
        "minimal" => Effort::Minimal,
        "low" => Effort::Low,
        "medium" => Effort::Medium,
        "high" => Effort::High,
        "xhigh" => Effort::Xhigh,
        "max" => Effort::Max,
        _ => Effort::None,
    }
}

pub fn system_prompt(settings: &Settings) -> SystemPrompt {
    if settings.system_prompt.trim().is_empty() {
        return SystemPrompt::default();
    }
    SystemPrompt {
        segments: vec![SystemSegment {
            text: settings.system_prompt.clone(),
            stability: Stability::Static,
        }],
    }
}

#[tauri::command]
pub async fn send_prompt(
    window: Window,
    state: tauri::State<'_, AppState>,
    prompt: String,
    on_frame: Channel<StreamFrame>,
) -> Result<(), ErrorEnvelope> {
    let prompt = validate_prompt(prompt)?;
    let user_text = prompt.clone();
    let settings = state.settings();
    let provider = providers::resolve(&settings, state.store()).await?;
    let label = window.label().to_owned();
    let ticket = state
        .conversations
        .lock()
        .entry(label.clone())
        .or_default()
        .begin(
            prompt,
            ModelRef {
                provider: provider.kind(),
                model: settings.model.clone(),
            },
        )
        .map_err(|_| {
            errors::invalid_request(
                "A response is already being generated. Stop it or wait for it to finish.",
                "busy",
            )
        })?;
    let session = state.session(&label);
    let mut guard = TurnGuard::new(state.conversations.clone(), label, ticket.turn, on_frame);
    if let Some(store) = state.history() {
        guard = guard.with_store(StoreHook {
            store: Arc::clone(store),
            session,
            title: user_text.chars().take(60).collect(),
            user: user_text,
        });
    }
    let capabilities = provider.capabilities(&settings.model);
    let system = system_prompt(&settings);
    let cache = bh_anthropic::plan_cache(&[], &system, &ticket.history, &capabilities.cache);
    let request = ProviderRequest {
        model: settings.model,
        system,
        messages: ticket.history,
        tools: Vec::new(),
        max_output_tokens: capabilities.max_output_tokens,
        reasoning: ReasoningConfig {
            effort: parse_effort(&settings.effort),
            display: ReasoningDisplay::Summarized,
        },
        cache,
    };
    tauri::async_runtime::spawn(async move {
        let run = turn::drive(
            provider.as_ref(),
            request,
            ticket.cancel,
            guard.correlation_id(),
            guard.sink(),
        )
        .await;
        guard.settle(run);
    });
    Ok(())
}

#[tauri::command]
pub fn new_chat(window: Window, state: tauri::State<'_, AppState>) {
    state.reset_window(window.label());
    state.new_session(window.label());
}

#[tauri::command]
pub async fn cancel_turn(
    window: Window,
    state: tauri::State<'_, AppState>,
) -> Result<(), ErrorEnvelope> {
    if let Some(conversation) = state.conversations.lock().get(window.label()) {
        conversation.cancel();
    }
    Ok(())
}

fn validate_prompt(prompt: String) -> Result<String, ErrorEnvelope> {
    if prompt.trim().is_empty() {
        return Err(errors::invalid_request(
            "Type a message to send.",
            "validation",
        ));
    }
    if prompt.chars().count() > PROMPT_MAX_CHARS {
        return Err(errors::invalid_request(
            "The message is longer than 200,000 characters. Shorten it, then send again.",
            "validation",
        ));
    }
    Ok(prompt)
}
