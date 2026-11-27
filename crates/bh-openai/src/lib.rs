mod body;
mod stream;

use std::{error::Error as _, net::IpAddr, time::Duration};

use bh_provider::{
    CacheCapabilities, CacheMode, Provider, ProviderCapabilities, ProviderError, ProviderRequest,
    ProviderStream, ReasoningDisplay, RetryPolicy, TimeoutPhase,
};
use bh_types::{Effort, ProviderKind};
use futures_util::{StreamExt, future::BoxFuture};
use reqwest::{
    Response, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderValue},
};
use tokio_util::sync::CancellationToken;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const CHATGPT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const USER_AGENT: &str = concat!("BarryHarness/", env!("CARGO_PKG_VERSION"));
const ERROR_BODY_LIMIT: usize = 4_096;
pub const CHATGPT_USER_AGENT: &str = "codex_cli_rs/0.20.0";

#[derive(Clone)]
pub enum OpenAiAuth {
    ApiKey(String),
    ChatGpt {
        access_token: String,
        account_id: String,
    },
}

impl std::fmt::Debug for OpenAiAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey(_) => formatter
                .debug_tuple("ApiKey")
                .field(&"<redacted>")
                .finish(),
            Self::ChatGpt { .. } => formatter
                .debug_struct("ChatGpt")
                .field("access_token", &"<redacted>")
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub connect: Duration,
    pub first_event: Duration,
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            first_event: Duration::from_secs(60),
            idle: Duration::from_secs(90),
        }
    }
}

#[derive(Clone)]
pub struct OpenAiConfig {
    pub base_url: String,
    pub auth: OpenAiAuth,
    pub retry: RetryPolicy,
    pub timeouts: Timeouts,
}

impl std::fmt::Debug for OpenAiConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiConfig")
            .field("base_url", &self.base_url)
            .field("auth", &self.auth)
            .field("retry", &self.retry)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl OpenAiConfig {
    pub fn api_key(key: &str) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            auth: OpenAiAuth::ApiKey(key.to_owned()),
            retry: RetryPolicy::default(),
            timeouts: Timeouts::default(),
        }
    }

    pub fn chatgpt(access_token: &str, account_id: &str) -> Self {
        Self {
            base_url: CHATGPT_BASE_URL.to_owned(),
            auth: OpenAiAuth::ChatGpt {
                access_token: access_token.to_owned(),
                account_id: account_id.to_owned(),
            },
            retry: RetryPolicy::default(),
            timeouts: Timeouts::default(),
        }
    }
}

pub struct OpenAiProvider {
    http: reqwest::Client,
    endpoint: Url,
    auth: OpenAiAuth,
    retry: RetryPolicy,
    timeouts: Timeouts,
}

impl std::fmt::Debug for OpenAiProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenAiProvider")
            .field("endpoint", &self.endpoint.as_str())
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl OpenAiProvider {
    pub fn new(config: &OpenAiConfig) -> Result<Self, ProviderError> {
        let endpoint = responses_endpoint(&config.base_url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(config.timeouts.connect)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(USER_AGENT)
            .build()
            .map_err(transport)?;
        Ok(Self {
            http,
            endpoint,
            auth: config.auth.clone(),
            retry: config.retry,
            timeouts: config.timeouts,
        })
    }

    async fn attempt(
        &self,
        body: Vec<u8>,
        first_event: Duration,
    ) -> Result<(Response, tokio::time::Instant), ProviderError> {
        let deadline = stream::deadline_after(first_event);
        let response = tokio::time::timeout_at(deadline, self.send(body))
            .await
            .map_err(|_| ProviderError::Timeout {
                phase: TimeoutPhase::FirstEvent,
            })??;
        Ok((response, deadline))
    }

    async fn send(&self, body: Vec<u8>) -> Result<Response, ProviderError> {
        let mut request = self
            .http
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "text/event-stream")
            .body(body);
        request = match &self.auth {
            OpenAiAuth::ApiKey(key) => request.header("authorization", bearer(key)?),
            OpenAiAuth::ChatGpt {
                access_token,
                account_id,
            } => request
                .header("authorization", bearer(access_token)?)
                .header("chatgpt-account-id", account_id)
                .header("originator", "codex_cli_rs")
                .header("session_id", session_id())
                .header("user-agent", CHATGPT_USER_AGENT),
        };
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(bh_provider::parse_retry_after);
        let body = read_error_body(response).await;
        Err(ProviderError::http(
            status.as_u16(),
            &body,
            request_id,
            retry_after,
        ))
    }
}

fn session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:032x}{count:016x}")
}
fn bearer(token: &str) -> Result<HeaderValue, ProviderError> {
    let mut value =
        HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| ProviderError::Auth {
            message: "the credential contains characters that are not valid in a header".to_owned(),
        })?;
    value.set_sensitive(true);
    Ok(value)
}

impl Provider for OpenAiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAi
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
            let body = body::encode(&request, &self.auth)?;
            let (response, first_event) = bh_provider::retry(&self.retry, &cancel, || {
                self.attempt(body.clone(), self.timeouts.first_event)
            })
            .await?;
            let chunks = response
                .bytes_stream()
                .map(|chunk| chunk.map_err(transport));
            Ok(stream::provider_events(
                chunks,
                cancel,
                stream::Deadlines {
                    first_event,
                    idle: self.timeouts.idle,
                },
            ))
        })
    }
}

pub fn capabilities_for(model: &str) -> ProviderCapabilities {
    let known = model.starts_with("gpt-5") || model.starts_with("gpt-4.1");
    let (context_window, max_output_tokens) = if known {
        (400_000, 128_000)
    } else {
        (128_000, 16_384)
    };
    ProviderCapabilities {
        context_window,
        max_output_tokens,
        efforts: vec![Effort::None, Effort::Low, Effort::Medium, Effort::High],
        reasoning_display: vec![ReasoningDisplay::Summarized, ReasoningDisplay::Omitted],
        tools: known,
        parallel_tool_calls: known,
        vision_input: known,
        cache: CacheCapabilities {
            mode: CacheMode::AutomaticPrefixWithKey,
            max_breakpoints: 0,
            min_prefix_tokens: 1_024,
            ttls: Vec::new(),
            lookback_blocks: 0,
        },
        streaming_tool_args: known,
    }
}

fn responses_endpoint(base_url: &str) -> Result<Url, ProviderError> {
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
    Url::parse(&format!(
        "{}/responses",
        base.as_str().trim_end_matches('/')
    ))
    .map_err(|error| invalid(&format!("cannot be joined: {error}")))
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
