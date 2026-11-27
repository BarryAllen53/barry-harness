#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use bh_anthropic::{AnthropicConfig, AnthropicProvider};
use bh_antigravity::{AntigravityConfig, AntigravityProvider};
use bh_auth::{AuthErrorKind, Credential, CredentialStore, Mode, ProviderId, Tokens};
use bh_config::Settings;
use bh_gemini::{GeminiConfig, GeminiProvider};
use bh_openai::{OpenAiConfig, OpenAiProvider};
use bh_provider::{ApiKey, Provider};
use bh_types::{ErrorCode, UserAction, ipc::ErrorEnvelope};

pub const ANTHROPIC_KEY_VARIABLE: &str = "ANTHROPIC_API_KEY";
pub const OPENAI_KEY_VARIABLE: &str = "OPENAI_API_KEY";
pub const GEMINI_KEY_VARIABLE: &str = "GEMINI_API_KEY";
pub const ANTIGRAVITY: &str = "antigravity";

pub fn provider_id(settings: &Settings) -> ProviderId {
    match settings.provider.as_str() {
        "openai" => ProviderId::OpenAi,
        "gemini" => ProviderId::Gemini,
        _ => ProviderId::Anthropic,
    }
}

pub async fn resolve(
    settings: &Settings,
    store: &CredentialStore,
) -> Result<Arc<dyn Provider>, ErrorEnvelope> {
    if settings.provider == ANTIGRAVITY {
        return antigravity().await;
    }
    let id = provider_id(settings);
    let (oauth, unrefreshable) = match bh_auth::usable_tokens(store, id).await {
        Ok(tokens) => (tokens, None),
        Err(error) if error.kind == AuthErrorKind::Configuration => {
            tracing::info!(
                provider = ?id,
                "the stored sign-in cannot be refreshed, trying the API key instead"
            );
            (None, Some(error))
        }
        Err(error) => return Err(envelope(error.kind, error.message)),
    };
    let api_key = store
        .get(id, Mode::ApiKey)
        .map_err(|error| envelope(error.kind, error.message))?;
    let resolved = match id {
        ProviderId::Anthropic => anthropic(oauth, api_key),
        ProviderId::OpenAi => openai(oauth, api_key),
        ProviderId::Gemini => gemini(oauth, api_key),
    };
    match (resolved, unrefreshable) {
        (Err(missing), Some(error)) if missing.code == ErrorCode::AuthMissing => {
            Err(envelope(error.kind, error.message))
        }
        (resolved, _) => resolved,
    }
}

fn anthropic(
    oauth: Option<Tokens>,
    api_key: Option<Credential>,
) -> Result<Arc<dyn Provider>, ErrorEnvelope> {
    let config = AnthropicConfig::default();
    let provider = match (oauth, api_key) {
        (Some(tokens), _) if !tokens.access.is_empty() => {
            AnthropicProvider::with_oauth(&tokens.access, &config)
        }
        (_, Some(Credential::ApiKey(key))) => {
            AnthropicProvider::new(&ApiKey::new(key).map_err(provider_envelope)?, &config)
        }
        _ => match std::env::var(ANTHROPIC_KEY_VARIABLE) {
            Ok(key) if !key.trim().is_empty() => {
                AnthropicProvider::new(&ApiKey::new(key).map_err(provider_envelope)?, &config)
            }
            _ => return Err(missing(ProviderId::Anthropic, ANTHROPIC_KEY_VARIABLE)),
        },
    };
    provider
        .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
        .map_err(provider_envelope)
}

fn openai(
    oauth: Option<Tokens>,
    api_key: Option<Credential>,
) -> Result<Arc<dyn Provider>, ErrorEnvelope> {
    let config = match (oauth, api_key) {
        (Some(tokens), _) if !tokens.access.is_empty() => chatgpt_config(&tokens)?,
        (_, Some(Credential::ApiKey(key))) => OpenAiConfig::api_key(&key),
        _ => match std::env::var(OPENAI_KEY_VARIABLE) {
            Ok(key) if !key.trim().is_empty() => OpenAiConfig::api_key(key.trim()),
            _ => return Err(missing(ProviderId::OpenAi, OPENAI_KEY_VARIABLE)),
        },
    };
    OpenAiProvider::new(&config)
        .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
        .map_err(provider_envelope)
}

fn chatgpt_config(tokens: &Tokens) -> Result<OpenAiConfig, ErrorEnvelope> {
    let Some(account_id) = tokens.account_id.clone() else {
        return Err(ErrorEnvelope {
            code: ErrorCode::AuthInvalid,
            message: "The ChatGPT sign-in did not include an account id. Sign in to ChatGPT again."
                .to_owned(),
            retryable: false,
            user_action: Some(UserAction::AddApiKey),
            request_id: None,
            correlation_id: "auth".to_owned(),
        });
    };
    Ok(OpenAiConfig::chatgpt(&tokens.access, &account_id))
}

fn gemini(
    oauth: Option<Tokens>,
    api_key: Option<Credential>,
) -> Result<Arc<dyn Provider>, ErrorEnvelope> {
    let config = match (oauth, api_key) {
        (Some(tokens), _) if !tokens.access.is_empty() => GeminiConfig::google(&tokens.access),
        (_, Some(Credential::ApiKey(key))) => GeminiConfig::api_key(&key),
        _ => match std::env::var(GEMINI_KEY_VARIABLE) {
            Ok(key) if !key.trim().is_empty() => GeminiConfig::api_key(key.trim()),
            _ => return Err(missing(ProviderId::Gemini, GEMINI_KEY_VARIABLE)),
        },
    };
    GeminiProvider::new(&config)
        .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
        .map_err(provider_envelope)
}

async fn antigravity() -> Result<Arc<dyn Provider>, ErrorEnvelope> {
    AntigravityProvider::connect(&AntigravityConfig::default())
        .await
        .map(|provider| Arc::new(provider) as Arc<dyn Provider>)
        .map_err(provider_envelope)
}

fn missing(provider: ProviderId, variable: &str) -> ErrorEnvelope {
    let name = match provider {
        ProviderId::Anthropic => "Anthropic",
        ProviderId::OpenAi => "OpenAI",
        ProviderId::Gemini => "Google",
    };
    ErrorEnvelope {
        code: ErrorCode::AuthMissing,
        message: format!(
            "No {name} credential is set. Add an API key or sign in from Settings, or set {variable}."
        ),
        retryable: false,
        user_action: Some(UserAction::AddApiKey),
        request_id: None,
        correlation_id: "auth".to_owned(),
    }
}

fn provider_envelope(error: bh_provider::ProviderError) -> ErrorEnvelope {
    let code = error.code();
    let message = match error {
        bh_provider::ProviderError::Reported { message, .. } => message,
        other => format!("The provider client could not start: {other}"),
    };
    ErrorEnvelope {
        code,
        message,
        retryable: false,
        user_action: code.user_action(),
        request_id: None,
        correlation_id: "provider".to_owned(),
    }
}

fn envelope(kind: AuthErrorKind, message: String) -> ErrorEnvelope {
    ErrorEnvelope {
        code: match kind {
            AuthErrorKind::Store => ErrorCode::Internal,
            AuthErrorKind::Provider | AuthErrorKind::StateMismatch => ErrorCode::AuthInvalid,
            _ => ErrorCode::AuthMissing,
        },
        message,
        retryable: false,
        user_action: None,
        request_id: None,
        correlation_id: "auth".to_owned(),
    }
}
