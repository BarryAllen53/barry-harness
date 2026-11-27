use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

use crate::{AuthError, Endpoints};

const EXPIRY_SKEW_SECONDS: u64 = 60;

const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_at: u64,
    pub account_label: Option<String>,
    pub account_id: Option<String>,
}

impl fmt::Debug for Tokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tokens")
            .field("access", &"<redacted>")
            .field("refresh", &self.refresh.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("account_label", &self.account_label)
            .field("account_id", &self.account_id)
            .finish()
    }
}

#[derive(Default, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub async fn exchange(
    endpoints: &Endpoints,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    state: &str,
    client_secret: Option<&str>,
) -> Result<Tokens, AuthError> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", verifier),
        ("client_id", endpoints.client_id),
        ("redirect_uri", redirect_uri),
    ];
    if matches!(endpoints.token_format, crate::endpoints::TokenFormat::Json) {
        form.push(("state", state));
    }
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    let response = post(endpoints, &form).await?;
    Tokens::from_response(response)
}

pub async fn refresh(endpoints: &Endpoints, refresh_token: &str) -> Result<Tokens, AuthError> {
    let client_secret = endpoints.refresh_secret()?;
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", endpoints.client_id),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let response = post(endpoints, &form).await?;
    let mut tokens = Tokens::from_response(response)?;
    if tokens.refresh.is_none() {
        tokens.refresh = Some(refresh_token.to_owned());
    }
    Ok(tokens)
}

async fn post(endpoints: &Endpoints, form: &[(&str, &str)]) -> Result<TokenResponse, AuthError> {
    let (body, content_type) = match endpoints.token_format {
        crate::endpoints::TokenFormat::Form => (
            form.iter()
                .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
                .collect::<Vec<_>>()
                .join("&"),
            "application/x-www-form-urlencoded",
        ),
        crate::endpoints::TokenFormat::Json => {
            let object = form
                .iter()
                .map(|(key, value)| {
                    (
                        (*key).to_owned(),
                        serde_json::Value::String((*value).to_owned()),
                    )
                })
                .collect::<serde_json::Map<String, serde_json::Value>>();
            let body = serde_json::to_string(&object).map_err(|error| {
                AuthError::network(format!("token request could not be encoded: {error}"))
            })?;
            (body, "application/json")
        }
    };
    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| {
            AuthError::network(format!(
                "token client could not start: {}",
                error.without_url()
            ))
        })?;
    let response = client
        .post(endpoints.token)
        .header("accept", "application/json")
        .header("content-type", content_type)
        .body(body)
        .send()
        .await
        .map_err(|error| {
            AuthError::network(format!("token request failed: {}", error.without_url()))
        })?;
    let status = response.status();
    let body = response.text().await.map_err(|error| {
        AuthError::network(format!(
            "token response unreadable: {}",
            error.without_url()
        ))
    })?;
    let parsed: TokenResponse = serde_json::from_str(&body).unwrap_or_default();
    if !status.is_success() {
        let detail = error_detail(&body, status.as_u16());
        return Err(AuthError::provider(format!(
            "token exchange failed: {detail}"
        )));
    }
    Ok(parsed)
}

fn error_detail(body: &str, status: u16) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return format!("status {status}");
    };
    if let Some(description) = value.get("error_description").and_then(|v| v.as_str()) {
        return description.to_owned();
    }
    match value.get("error") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(error @ serde_json::Value::Object(_)) => error
            .get("message")
            .and_then(|message| message.as_str())
            .map_or_else(
                || {
                    error
                        .get("type")
                        .and_then(|kind| kind.as_str())
                        .map_or_else(|| format!("status {status}"), str::to_owned)
                },
                str::to_owned,
            ),
        _ => format!("status {status}"),
    }
}

impl Tokens {
    fn from_response(response: TokenResponse) -> Result<Self, AuthError> {
        let Some(access) = response.access_token else {
            return Err(AuthError::provider("token response had no access token"));
        };
        let lifetime = response.expires_in.unwrap_or(3_600);
        let claims = response.id_token.as_deref().and_then(claims);
        Ok(Self {
            access,
            refresh: response.refresh_token.filter(|value| !value.is_empty()),
            expires_at: now_secs() + lifetime.saturating_sub(EXPIRY_SKEW_SECONDS),
            account_label: claims.as_ref().and_then(account_label),
            account_id: claims.as_ref().and_then(chatgpt_account_id),
        })
    }
}

fn claims(id_token: &str) -> Option<serde_json::Value> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn account_label(claims: &serde_json::Value) -> Option<String> {
    ["email", "preferred_username", "sub"]
        .into_iter()
        .find_map(|key| claims.get(key).and_then(|value| value.as_str()))
        .map(str::to_owned)
}

fn chatgpt_account_id(claims: &serde_json::Value) -> Option<String> {
    claims
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}
