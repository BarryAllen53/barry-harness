use serde::{Deserialize, Serialize};

use crate::{AuthError, AuthErrorKind, ProviderId, Tokens};

pub const SERVICE: &str = "com.barryharness.desktop";
pub const MAX_SECRET_BYTES: usize = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    OAuth,
    ApiKey,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OAuth => "oauth",
            Self::ApiKey => "api_key",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    OAuth(Tokens),
    ApiKey(String),
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OAuth(tokens) => formatter.debug_tuple("OAuth").field(tokens).finish(),
            Self::ApiKey(_) => formatter
                .debug_tuple("ApiKey")
                .field(&"<redacted>")
                .finish(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInState {
    pub account_label: Option<String>,
    pub expires_at: u64,
}

struct KeyringBackend {
    service: String,
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self {
            service: SERVICE.to_owned(),
        }
    }
}

impl KeyringBackend {
    fn read(&self, user: &str) -> Result<Option<String>, AuthError> {
        let entry = self.entry(user)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(&error)),
        }
    }

    fn write(&self, user: &str, secret: &str) -> Result<(), AuthError> {
        self.entry(user)?
            .set_password(secret)
            .map_err(|error| store_error(&error))
    }

    fn erase(&self, user: &str) -> Result<(), AuthError> {
        match self.entry(user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(&error)),
        }
    }

    fn entry(&self, user: &str) -> Result<keyring::Entry, AuthError> {
        keyring::Entry::new(&self.service, user).map_err(|error| store_error(&error))
    }
}

fn store_error(error: &keyring::Error) -> AuthError {
    AuthError::new(
        AuthErrorKind::Store,
        format!("the credential store rejected the request: {error}"),
    )
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Wire {
    OAuth {
        access: Option<String>,
        refresh: Option<String>,
        expires_at: u64,
        account_label: Option<String>,
        account_id: Option<String>,
    },
    ApiKey {
        key: String,
    },
}

pub struct CredentialStore {
    backend: KeyringBackend,
}

impl CredentialStore {
    pub fn new() -> Self {
        Self {
            backend: KeyringBackend::default(),
        }
    }

    fn user(provider: ProviderId, mode: Mode) -> String {
        let provider = match provider {
            ProviderId::Anthropic => "anthropic",
            ProviderId::OpenAi => "openai",
            ProviderId::Gemini => "gemini",
        };
        format!("{provider}:{}", mode.as_str())
    }

    pub fn get(&self, provider: ProviderId, mode: Mode) -> Result<Option<Credential>, AuthError> {
        let Some(secret) = self.backend.read(&Self::user(provider, mode))? else {
            return Ok(None);
        };
        let wire: Wire = serde_json::from_str(&secret).map_err(|error| {
            AuthError::new(
                AuthErrorKind::Store,
                format!("the stored credential is unreadable: {error}"),
            )
        })?;
        Ok(Some(match wire {
            Wire::OAuth {
                access,
                refresh,
                expires_at,
                account_label,
                account_id,
            } => Credential::OAuth(Tokens {
                access: access.unwrap_or_default(),
                refresh,
                expires_at,
                account_label,
                account_id,
            }),
            Wire::ApiKey { key } => Credential::ApiKey(key),
        }))
    }

    pub fn set(
        &self,
        provider: ProviderId,
        mode: Mode,
        credential: &Credential,
    ) -> Result<(), AuthError> {
        let wire = match credential {
            Credential::OAuth(tokens) => Wire::OAuth {
                access: Some(tokens.access.clone()),
                refresh: tokens.refresh.clone(),
                expires_at: tokens.expires_at,
                account_label: tokens.account_label.clone(),
                account_id: tokens.account_id.clone(),
            },
            Credential::ApiKey(key) => Wire::ApiKey { key: key.clone() },
        };
        let mut wire = wire;
        let mut secret = serde_json::to_string(&wire).map_err(|error| {
            AuthError::new(
                AuthErrorKind::Store,
                format!("the credential could not be encoded: {error}"),
            )
        })?;
        if secret.len() > MAX_SECRET_BYTES
            && let Wire::OAuth { access, .. } = &mut wire
        {
            *access = None;
            secret = serde_json::to_string(&wire).map_err(|error| {
                AuthError::new(
                    AuthErrorKind::Store,
                    format!("the credential could not be encoded: {error}"),
                )
            })?;
        }
        self.backend.write(&Self::user(provider, mode), &secret)
    }

    pub fn delete(&self, provider: ProviderId, mode: Mode) -> Result<(), AuthError> {
        self.backend.erase(&Self::user(provider, mode))
    }

    pub fn state(
        &self,
        provider: ProviderId,
        mode: Mode,
    ) -> Result<Option<SignInState>, AuthError> {
        match self.get(provider, mode)? {
            Some(Credential::OAuth(tokens)) => Ok(Some(SignInState {
                account_label: tokens.account_label,
                expires_at: tokens.expires_at,
            })),
            Some(Credential::ApiKey(_)) | None => Ok(None),
        }
    }
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}
