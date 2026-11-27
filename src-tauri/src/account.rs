#![allow(clippy::needless_pass_by_value)]

use bh_auth::{
    AuthError, Credential, CredentialStore, Mode, Pending, ProviderId, SIGN_IN_TIMEOUT, Tokens,
};
use bh_config::Settings;
use bh_types::{ErrorCode, UserAction, ipc::ErrorEnvelope};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStatus {
    pub provider: String,
    pub signed_in: bool,
    pub account_label: Option<String>,
    pub expires_at: Option<u64>,
    pub has_api_key: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SignInOutcome {
    SignedIn {
        account_label: Option<String>,
        expires_at: u64,
    },
    NeedsCode {
        url: String,
    },
}

pub fn parse_provider(name: &str) -> Result<ProviderId, ErrorEnvelope> {
    match name {
        "anthropic" => Ok(ProviderId::Anthropic),
        "openai" => Ok(ProviderId::OpenAi),
        "gemini" => Ok(ProviderId::Gemini),
        other => Err(ErrorEnvelope {
            code: ErrorCode::InvalidRequest,
            message: format!("Unknown provider {other}."),
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: "settings".to_owned(),
        }),
    }
}

fn provider_name(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Anthropic => "anthropic",
        ProviderId::OpenAi => "openai",
        ProviderId::Gemini => "gemini",
    }
}

fn status(store: &CredentialStore, provider: ProviderId) -> SignInStatus {
    let state = store.state(provider, Mode::OAuth).ok().flatten();
    SignInStatus {
        provider: provider_name(provider).to_owned(),
        signed_in: state.is_some(),
        account_label: state.as_ref().and_then(|state| state.account_label.clone()),
        expires_at: state.map(|state| state.expires_at),
        has_api_key: store.get(provider, Mode::ApiKey).ok().flatten().is_some(),
    }
}

pub fn auth_envelope(error: AuthError) -> ErrorEnvelope {
    ErrorEnvelope {
        code: match error.kind {
            bh_auth::AuthErrorKind::Store => ErrorCode::Internal,
            bh_auth::AuthErrorKind::StateMismatch | bh_auth::AuthErrorKind::Provider => {
                ErrorCode::AuthInvalid
            }
            _ => ErrorCode::AuthMissing,
        },
        message: error.message,
        retryable: false,
        user_action: Some(UserAction::AddApiKey),
        request_id: None,
        correlation_id: "auth".to_owned(),
    }
}

fn store_tokens(
    store: &CredentialStore,
    provider: ProviderId,
    tokens: Tokens,
) -> Result<SignInOutcome, ErrorEnvelope> {
    let account_label = tokens.account_label.clone();
    let expires_at = tokens.expires_at;
    store
        .set(provider, Mode::OAuth, &Credential::OAuth(tokens))
        .map_err(auth_envelope)?;
    tracing::info!(
        provider = provider_name(provider),
        "oauth credential stored"
    );
    Ok(SignInOutcome::SignedIn {
        account_label,
        expires_at,
    })
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn settings_set(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, ErrorEnvelope> {
    state.replace_settings(settings)?;
    Ok(state.settings())
}

#[tauri::command]
pub fn credential_state(state: State<'_, AppState>) -> Vec<SignInStatus> {
    [
        ProviderId::Anthropic,
        ProviderId::OpenAi,
        ProviderId::Gemini,
    ]
    .into_iter()
    .map(|provider| status(state.store(), provider))
    .collect()
}

#[tauri::command]
pub fn credential_set(
    state: State<'_, AppState>,
    provider: String,
    api_key: String,
) -> Result<SignInStatus, ErrorEnvelope> {
    let provider = parse_provider(&provider)?;
    let trimmed = api_key.trim().to_owned();
    if trimmed.is_empty() {
        return Err(ErrorEnvelope {
            code: ErrorCode::InvalidRequest,
            message: "Paste an API key first.".to_owned(),
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: "settings".to_owned(),
        });
    }
    state
        .store()
        .set(provider, Mode::ApiKey, &Credential::ApiKey(trimmed))
        .map_err(auth_envelope)?;
    Ok(status(state.store(), provider))
}

#[tauri::command]
pub fn credential_delete(
    state: State<'_, AppState>,
    provider: String,
) -> Result<SignInStatus, ErrorEnvelope> {
    let provider = parse_provider(&provider)?;
    state
        .store()
        .delete(provider, Mode::ApiKey)
        .map_err(auth_envelope)?;
    Ok(status(state.store(), provider))
}

#[tauri::command]
pub async fn sign_in(
    state: State<'_, AppState>,
    provider: String,
) -> Result<SignInOutcome, ErrorEnvelope> {
    start_sign_in(state, provider, false).await
}

#[tauri::command]
pub async fn sign_in_paste(
    state: State<'_, AppState>,
    provider: String,
) -> Result<SignInOutcome, ErrorEnvelope> {
    start_sign_in(state, provider, true).await
}

async fn start_sign_in(
    state: State<'_, AppState>,
    provider: String,
    paste: bool,
) -> Result<SignInOutcome, ErrorEnvelope> {
    let provider = parse_provider(&provider)?;
    let pending = if paste {
        Pending::start_paste(provider)
            .await
            .map_err(auth_envelope)?
    } else {
        Pending::start(provider).await.map_err(auth_envelope)?
    };
    let url = pending.url().to_owned();
    tracing::info!(
        host = url.split('?').next().unwrap_or_default(),
        len = url.len(),
        "sign-in page generated"
    );
    if let Err(error) = open_browser(&url) {
        tracing::warn!(%error, "the browser could not be opened");
    }
    if pending.needs_code() {
        *state.pending().lock().await = Some((provider, pending));
        return Ok(SignInOutcome::NeedsCode { url });
    }
    let outcome = pending.wait(SIGN_IN_TIMEOUT).await;
    tracing::info!(
        provider = provider_name(provider),
        ok = outcome.is_ok(),
        error = outcome.as_ref().err().map(|error| error.message.as_str()),
        "sign-in finished"
    );
    let tokens = outcome.map_err(auth_envelope)?;
    store_tokens(state.store(), provider, tokens)
}

#[tauri::command]
pub async fn sign_in_code(
    state: State<'_, AppState>,
    provider: String,
    code: String,
) -> Result<SignInOutcome, ErrorEnvelope> {
    let provider = parse_provider(&provider)?;
    let pending = state.pending().lock().await.take();
    let Some((started_for, pending)) = pending else {
        return Err(ErrorEnvelope {
            code: ErrorCode::AuthInvalid,
            message: "That sign-in attempt is no longer active. Start again.".to_owned(),
            retryable: false,
            user_action: Some(UserAction::AddApiKey),
            request_id: None,
            correlation_id: "auth".to_owned(),
        });
    };
    if started_for != provider {
        return Err(ErrorEnvelope {
            code: ErrorCode::AuthInvalid,
            message: "The code belongs to another provider. Start again.".to_owned(),
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: "auth".to_owned(),
        });
    }
    let tokens = pending
        .finish_with_code(&code)
        .await
        .map_err(auth_envelope)?;
    store_tokens(state.store(), provider, tokens)
}

#[tauri::command]
pub fn sign_out(
    state: State<'_, AppState>,
    provider: String,
) -> Result<SignInStatus, ErrorEnvelope> {
    let provider = parse_provider(&provider)?;
    state
        .store()
        .delete(provider, Mode::OAuth)
        .map_err(auth_envelope)?;
    Ok(status(state.store(), provider))
}

fn open_browser(url: &str) -> std::io::Result<()> {
    open::that_detached(url)
}
