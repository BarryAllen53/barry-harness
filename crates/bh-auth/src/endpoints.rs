use std::env::{self, VarError};

use crate::AuthError;

const GEMINI_CLIENT_SECRET_VARIABLE: &str = "BH_GEMINI_CLIENT_SECRET";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderId {
    Anthropic,
    OpenAi,
    Gemini,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redirect {
    Loopback {
        port: Option<u16>,
        path: &'static str,
    },
    PasteCode {
        callback: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenFormat {
    Form,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub provider: ProviderId,
    pub authorize: &'static str,
    pub token: &'static str,
    pub client_id: &'static str,
    pub client_secret_variable: Option<&'static str>,
    pub redirect: Redirect,
    pub scopes: &'static [&'static str],
    pub extra: &'static [(&'static str, &'static str)],
    pub token_format: TokenFormat,
}

impl Endpoints {
    pub fn redirect_uri(&self, port: u16) -> String {
        match self.redirect {
            Redirect::Loopback {
                port: Some(fixed),
                path,
            } => format!("http://localhost:{fixed}{path}"),
            Redirect::Loopback { port: None, path } => format!("http://localhost:{port}{path}"),
            Redirect::PasteCode { callback } => callback.to_owned(),
        }
    }

    pub fn sign_in_secret(&self) -> Result<Option<String>, AuthError> {
        self.client_secret().map_err(|unusable| {
            AuthError::configuration(format!(
                "{} {}. Set it to the OAuth client secret of the {} sign-in client, then sign in.",
                unusable.variable,
                unusable.reason,
                self.provider.account_name()
            ))
        })
    }

    pub fn refresh_secret(&self) -> Result<Option<String>, AuthError> {
        self.client_secret().map_err(|unusable| {
            AuthError::configuration(format!(
                "{} sign-in expired and cannot be refreshed: {} {}. Set it, then sign in again, or add {}.",
                self.provider.account_name(),
                unusable.variable,
                unusable.reason,
                self.provider.api_key_name()
            ))
        })
    }

    fn client_secret(&self) -> Result<Option<String>, UnusableSecret> {
        let Some(variable) = self.client_secret_variable else {
            return Ok(None);
        };
        match env::var(variable) {
            Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_owned())),
            Ok(_) | Err(VarError::NotPresent) => Err(UnusableSecret {
                variable,
                reason: "is not set",
            }),
            Err(VarError::NotUnicode(_)) => Err(UnusableSecret {
                variable,
                reason: "is not valid Unicode",
            }),
        }
    }
}

struct UnusableSecret {
    variable: &'static str,
    reason: &'static str,
}

impl ProviderId {
    fn account_name(self) -> &'static str {
        match self {
            Self::Anthropic => "Claude",
            Self::OpenAi => "ChatGPT",
            Self::Gemini => "Google",
        }
    }

    fn api_key_name(self) -> &'static str {
        match self {
            Self::Anthropic => "an Anthropic API key",
            Self::OpenAi => "an OpenAI API key",
            Self::Gemini => "a Gemini API key",
        }
    }
}

pub const ANTHROPIC_CALLBACK: &str = "https://console.anthropic.com/oauth/code/callback";

pub const ANTHROPIC: Endpoints = Endpoints {
    provider: ProviderId::Anthropic,
    authorize: "https://claude.ai/oauth/authorize",
    token: "https://console.anthropic.com/v1/oauth/token",
    client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    client_secret_variable: None,
    redirect: Redirect::Loopback {
        port: Some(54545),
        path: "/callback",
    },
    scopes: &["user:inference", "user:profile"],
    extra: &[("code", "true")],
    token_format: TokenFormat::Json,
};

pub const OPENAI: Endpoints = Endpoints {
    provider: ProviderId::OpenAi,
    authorize: "https://auth.openai.com/oauth/authorize",
    token: "https://auth.openai.com/oauth/token",
    client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
    client_secret_variable: None,
    redirect: Redirect::Loopback {
        port: Some(1455),
        path: "/auth/callback",
    },
    scopes: &["openid", "profile", "email", "offline_access"],
    extra: &[
        ("codex_cli_simplified_flow", "true"),
        ("id_token_add_organizations", "true"),
    ],
    token_format: TokenFormat::Json,
};

pub const GEMINI: Endpoints = Endpoints {
    provider: ProviderId::Gemini,
    authorize: "https://accounts.google.com/o/oauth2/v2/auth",
    token: "https://oauth2.googleapis.com/token",
    client_id: "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
    client_secret_variable: Some(GEMINI_CLIENT_SECRET_VARIABLE),
    redirect: Redirect::Loopback {
        port: None,
        path: "/oauth2callback",
    },
    scopes: &[
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
    ],
    extra: &[("access_type", "offline"), ("prompt", "consent")],
    token_format: TokenFormat::Form,
};

pub fn get(provider: ProviderId) -> &'static Endpoints {
    match provider {
        ProviderId::Anthropic => &ANTHROPIC,
        ProviderId::OpenAi => &OPENAI,
        ProviderId::Gemini => &GEMINI,
    }
}
