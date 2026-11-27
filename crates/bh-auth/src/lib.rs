mod endpoints;
mod loopback;
mod pkce;
mod store;
mod token;

use std::{fmt, time::Duration};

use reqwest::Url;

use crate::{
    endpoints::{Endpoints, Redirect},
    loopback::Loopback,
};

pub use endpoints::ProviderId;
pub use store::{Credential, CredentialStore, Mode, SignInState};
pub use token::Tokens;

pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthErrorKind {
    CodeRequired,
    Timeout,
    StateMismatch,
    Provider,
    Network,
    Store,
    Configuration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthError {
    pub kind: AuthErrorKind,
    pub message: String,
}

impl AuthError {
    pub fn new(kind: AuthErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn code_required() -> Self {
        Self::new(
            AuthErrorKind::CodeRequired,
            "the provider asks for the code shown in the browser",
        )
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(AuthErrorKind::Timeout, message)
    }

    pub fn state_mismatch() -> Self {
        Self::new(
            AuthErrorKind::StateMismatch,
            "the sign-in response did not match this request",
        )
    }

    pub fn provider(message: impl Into<String>) -> Self {
        Self::new(AuthErrorKind::Provider, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(AuthErrorKind::Network, message)
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(AuthErrorKind::Configuration, message)
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AuthError {}

pub struct Pending {
    endpoints: Endpoints,
    client_secret: Option<String>,
    verifier: String,
    state: String,
    redirect_uri: String,
    url: String,
    listener: Option<Loopback>,
}

impl Pending {
    pub async fn start(provider: ProviderId) -> Result<Self, AuthError> {
        Self::build(provider, None).await
    }

    pub async fn start_paste(provider: ProviderId) -> Result<Self, AuthError> {
        if provider != ProviderId::Anthropic {
            return Err(AuthError::new(
                AuthErrorKind::Provider,
                "only Claude accepts a pasted code",
            ));
        }
        Self::build(
            provider,
            Some(Redirect::PasteCode {
                callback: endpoints::ANTHROPIC_CALLBACK,
            }),
        )
        .await
    }

    async fn build(provider: ProviderId, redirect: Option<Redirect>) -> Result<Self, AuthError> {
        let endpoints = *endpoints::get(provider);
        let client_secret = endpoints.sign_in_secret()?;
        let pkce = pkce::generate();
        let state = pkce::random_token(24);
        let redirect = redirect.unwrap_or(endpoints.redirect);
        let (redirect_uri, listener) = match redirect {
            Redirect::Loopback { port, path } => {
                let listener = Loopback::bind(port, path).await?;
                (endpoints.redirect_uri(listener.port()), Some(listener))
            }
            Redirect::PasteCode { callback } => (callback.to_owned(), None),
        };
        let url = authorize_url(&endpoints, &redirect_uri, &state, &pkce.challenge);
        Ok(Self {
            endpoints,
            client_secret,
            verifier: pkce.verifier,
            state,
            redirect_uri,
            url,
            listener,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn needs_code(&self) -> bool {
        self.listener.is_none()
    }

    pub async fn wait(self, window: Duration) -> Result<Tokens, AuthError> {
        let Some(mut listener) = self.listener else {
            return Err(AuthError::code_required());
        };
        let (code, mut stream) = listener.accept_code(&self.state, window).await?;
        let result = token::exchange(
            &self.endpoints,
            &code,
            &self.verifier,
            &self.redirect_uri,
            &self.state,
            self.client_secret.as_deref(),
        )
        .await;
        match &result {
            Ok(_) => loopback::respond_ok(&mut stream).await,
            Err(error) => loopback::respond_error(&mut stream, &error.message).await,
        }
        result
    }

    pub async fn finish_with_code(self, input: &str) -> Result<Tokens, AuthError> {
        if self.listener.is_some() {
            return Err(AuthError::new(
                AuthErrorKind::Provider,
                "this provider redirects to the app, not to a pasted code",
            ));
        }
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AuthError::code_required());
        }
        let (code, state) = match trimmed.split_once('#') {
            Some((code, state)) => (code.trim(), Some(state.trim())),
            None => (trimmed, None),
        };
        if let Some(state) = state
            && state != self.state
        {
            return Err(AuthError::state_mismatch());
        }
        token::exchange(
            &self.endpoints,
            code,
            &self.verifier,
            &self.redirect_uri,
            &self.state,
            self.client_secret.as_deref(),
        )
        .await
    }
}

pub async fn usable_tokens(
    store: &CredentialStore,
    provider: ProviderId,
) -> Result<Option<Tokens>, AuthError> {
    let Some(Credential::OAuth(tokens)) = store.get(provider, Mode::OAuth)? else {
        return Ok(None);
    };
    let now = token::now_secs();
    if !tokens.access.is_empty() && tokens.expires_at > now + 30 {
        return Ok(Some(tokens));
    }
    let Some(refresh_token) = tokens.refresh.clone() else {
        return Ok(Some(tokens));
    };
    let refreshed = token::refresh(endpoints::get(provider), &refresh_token).await?;
    let merged = Tokens {
        account_label: refreshed.account_label.or(tokens.account_label),
        account_id: refreshed.account_id.or(tokens.account_id),
        ..refreshed
    };
    store.set(provider, Mode::OAuth, &Credential::OAuth(merged.clone()))?;
    Ok(Some(merged))
}

fn authorize_url(
    endpoints: &Endpoints,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    let mut url = Url::parse(endpoints.authorize).expect("the authorize url is a constant");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", endpoints.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        query.append_pair("scope", &endpoints.scopes.join(" "));
        query.append_pair("state", state);
        query.append_pair("code_challenge", challenge);
        query.append_pair("code_challenge_method", "S256");
        for (key, value) in endpoints.extra {
            query.append_pair(key, value);
        }
    }
    url.into()
}
