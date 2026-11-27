mod assembler;
mod body;
mod cache_plan;
mod capabilities;
mod stream;

use std::{error::Error as _, net::IpAddr, time::Duration};

use bh_provider::{
    ApiKey, Provider, ProviderCapabilities, ProviderError, ProviderRequest, ProviderStream,
    RetryPolicy, TimeoutPhase, parse_retry_after, retry,
};
use bh_types::{Effort, ProviderKind, Stability, SystemSegment};
use futures_util::{StreamExt, future::BoxFuture};
use reqwest::{
    Response, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue, LOCATION, RETRY_AFTER},
    redirect,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

pub use cache_plan::plan_cache;
pub use capabilities::capabilities_for;

use crate::stream::{Deadlines, deadline_after, provider_events};

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com";
pub const ANTHROPIC_VERSION: &str = "2026-10-21";
const MESSAGES_PATH: &str = "/v1/messages";
const ERROR_BODY_LIMIT: usize = 4096;
const REDACTED_KEY: &str = "[REDACTED]";
const USER_AGENT: &str = concat!("BarryHarness/", env!("CARGO_PKG_VERSION"));
pub const OAUTH_BETA: &str = "oauth-2025-04-20";
const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
pub const CLAUDE_CODE_AGENT: &str = "claude-cli/2.1.290";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    pub connect: Duration,
    pub first_event: Duration,
    pub first_event_high_effort: Duration,
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            first_event: Duration::from_secs(60),
            first_event_high_effort: Duration::from_secs(180),
            idle: Duration::from_secs(90),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnthropicConfig {
    pub base_url: String,
    pub retry: RetryPolicy,
    pub timeouts: Timeouts,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            base_url: ANTHROPIC_API_URL.to_owned(),
            retry: RetryPolicy::default(),
            timeouts: Timeouts::default(),
        }
    }
}

pub struct AnthropicProvider {
    http: reqwest::Client,
    endpoint: Url,
    header_name: &'static str,
    header: HeaderValue,
    secret: String,
    beta: bool,
    retry: RetryPolicy,
    timeouts: Timeouts,
}

impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AnthropicProvider")
            .field("endpoint", &self.endpoint)
            .field("header_name", &self.header_name)
            .field("secret", &"<redacted>")
            .field("beta", &self.beta)
            .field("retry", &self.retry)
            .field("timeouts", &self.timeouts)
            .finish_non_exhaustive()
    }
}

impl AnthropicProvider {
    pub fn new(api_key: &ApiKey, config: &AnthropicConfig) -> Result<Self, ProviderError> {
        let secret = api_key.expose().to_owned();
        let header = Self::sensitive(&secret)?;
        Self::build(config, "x-api-key", header, secret, false)
    }

    pub fn with_oauth(access_token: &str, config: &AnthropicConfig) -> Result<Self, ProviderError> {
        let header = Self::sensitive(&format!("Bearer {access_token}"))?;
        Self::build(
            config,
            AUTHORIZATION.as_str(),
            header,
            access_token.to_owned(),
            true,
        )
    }

    fn build(
        config: &AnthropicConfig,
        header_name: &'static str,
        header: HeaderValue,
        secret: String,
        beta: bool,
    ) -> Result<Self, ProviderError> {
        let endpoint = messages_endpoint(&config.base_url)?;
        let mut builder = reqwest::Client::builder()
            .connect_timeout(config.timeouts.connect)
            .redirect(redirect::Policy::none())
            .user_agent(USER_AGENT);
        if endpoint.scheme() == "http" {
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(transport)?;
        Ok(Self {
            http,
            endpoint,
            header_name,
            header,
            secret,
            beta,
            retry: config.retry,
            timeouts: config.timeouts,
        })
    }

    fn sensitive(value: &str) -> Result<HeaderValue, ProviderError> {
        let mut header = HeaderValue::from_str(value).map_err(|_| ProviderError::Auth {
            message: "the credential contains characters that are not valid in a header".to_owned(),
        })?;
        header.set_sensitive(true);
        Ok(header)
    }

    async fn attempt(
        &self,
        body: Vec<u8>,
        first_event: Duration,
    ) -> Result<(Response, Instant), ProviderError> {
        let deadline = deadline_after(first_event);
        let response = tokio::time::timeout_at(deadline, self.send(body))
            .await
            .map_err(|_| ProviderError::Timeout {
                phase: TimeoutPhase::FirstEvent,
            })??;
        Ok((response, deadline))
    }

    fn redact_key(&self, text: &str) -> String {
        redact_key(text, &self.secret)
    }

    async fn send(&self, body: Vec<u8>) -> Result<Response, ProviderError> {
        let mut endpoint = self.endpoint.clone();
        if self.beta {
            endpoint.query_pairs_mut().append_pair("beta", "true");
        }
        let mut request = self
            .http
            .post(endpoint)
            .header(self.header_name, self.header.clone())
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "text/event-stream")
            .body(body);
        if self.beta {
            request = request
                .header("anthropic-beta", OAUTH_BETA)
                .header("x-app", "cli")
                .header("user-agent", CLAUDE_CODE_AGENT);
        }
        let response = request.send().await.map_err(transport)?;
        let request_id = response
            .headers()
            .get("request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let status = response.status();
        if status.is_success() {
            tracing::debug!(
                request_id = request_id.as_deref(),
                "anthropic stream opened"
            );
            return Ok(response);
        }
        let retry_after = response
            .headers()
            .get(RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after);
        let location = response
            .headers()
            .get(LOCATION)
            .filter(|_| status.is_redirection())
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        let unified_status = response
            .headers()
            .get("anthropic-ratelimit-unified-status")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let unified_reset = response
            .headers()
            .get("anthropic-ratelimit-unified-reset")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        let body = read_error_body(response).await;
        let body = String::from_utf8_lossy(&body);
        let text = match location {
            Some(location) if body.is_empty() => format!("redirect to {location}"),
            Some(location) => format!("redirect to {location}: {body}"),
            None => body.into_owned(),
        };
        if status.as_u16() == 429 && unified_reset.is_some() {
            tracing::warn!(
                status = 429,
                unified_status = unified_status.as_deref(),
                unified_reset,
                "the subscription usage limit is reached"
            );
            return Err(ProviderError::UsageLimit {
                message: self.redact_key(&text),
                reset_at: unified_reset,
            });
        }
        tracing::warn!(
            status = status.as_u16(),
            request_id = request_id.as_deref(),
            "anthropic request failed"
        );
        Err(ProviderError::http(
            status.as_u16(),
            self.redact_key(&text).as_bytes(),
            request_id,
            retry_after,
        ))
    }
}

fn with_claude_code_identity(mut request: ProviderRequest) -> ProviderRequest {
    let mut segments = Vec::with_capacity(request.system.segments.len() + 1);
    segments.push(SystemSegment {
        text: CLAUDE_CODE_IDENTITY.to_owned(),
        stability: Stability::Static,
    });
    segments.extend(request.system.segments);
    request.system.segments = segments;
    request
}
impl Provider for AnthropicProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn capabilities(&self, model: &str) -> ProviderCapabilities {
        capabilities_for(model)
    }

    fn stream(
        &self,
        request: ProviderRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<ProviderStream, ProviderError>> {
        Box::pin(async move {
            let request = if self.beta {
                with_claude_code_identity(request)
            } else {
                request
            };
            let body = body::encode(&request, &capabilities_for(&request.model))?;
            let first_event = if request.reasoning.effort >= Effort::High {
                self.timeouts.first_event_high_effort
            } else {
                self.timeouts.first_event
            };
            let (response, first_event_deadline) = retry(&self.retry, &cancel, || {
                self.attempt(body.clone(), first_event)
            })
            .await?;
            let chunks = response
                .bytes_stream()
                .map(|chunk| chunk.map_err(transport));
            Ok(provider_events(
                chunks,
                cancel,
                Deadlines {
                    first_event: first_event_deadline,
                    idle: self.timeouts.idle,
                },
            ))
        })
    }
}

fn redact_key(text: &str, key: &str) -> String {
    let mut forms = vec![
        percent_encoded(key, b"0123456789ABCDEF"),
        percent_encoded(key, b"0123456789abcdef"),
        key.to_owned(),
    ];
    forms.dedup();
    forms
        .iter()
        .filter(|form| !form.is_empty())
        .fold(text.to_owned(), |redacted, form| {
            redacted.replace(form.as_str(), REDACTED_KEY)
        })
}

fn percent_encoded(text: &str, hex_digits: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(hex_digits[usize::from(byte >> 4)]));
            encoded.push(char::from(hex_digits[usize::from(byte & 0x0F)]));
        }
    }
    encoded
}

fn messages_endpoint(base_url: &str) -> Result<Url, ProviderError> {
    let invalid = |reason: &str| ProviderError::InvalidRequest {
        reason: format!("base URL {reason}"),
    };
    let base = Url::parse(base_url).map_err(|error| invalid(&format!("is not valid: {error}")))?;
    if !base.username().is_empty() || base.password().is_some() {
        return Err(invalid("must not contain credentials"));
    }
    if base.query().is_some() || base.fragment().is_some() {
        return Err(invalid("must not contain a query or fragment"));
    }
    match base.scheme() {
        "https" => {}
        "http" if is_loopback(&base) => {}
        "http" => return Err(invalid("must use https unless the host is loopback")),
        other => return Err(invalid(&format!("has unsupported scheme {other}"))),
    }
    let joined = format!("{}{MESSAGES_PATH}", base.as_str().trim_end_matches('/'));
    Url::parse(&joined).map_err(|error| invalid(&format!("cannot be joined: {error}")))
}

fn is_loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

async fn read_error_body(mut response: Response) -> Vec<u8> {
    let mut body = Vec::new();
    while body.len() < ERROR_BODY_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(error = %transport(error), "error response body was cut short");
                break;
            }
        }
    }
    body
}

fn transport(error: reqwest::Error) -> ProviderError {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    ProviderError::Transport { message }
}
