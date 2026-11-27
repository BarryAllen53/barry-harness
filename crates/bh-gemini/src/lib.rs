mod stream;

use std::{error::Error as _, net::IpAddr, sync::Arc, time::Duration};

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
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const API_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const CLOUD_CODE_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
const USER_AGENT: &str = concat!("BarryHarness/", env!("CARGO_PKG_VERSION"));
const ERROR_BODY_LIMIT: usize = 4_096;

#[derive(Clone)]
pub enum GeminiAuth {
    ApiKey(String),
    Google {
        access_token: String,
        project: Option<String>,
    },
}

impl std::fmt::Debug for GeminiAuth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey(_) => formatter
                .debug_tuple("ApiKey")
                .field(&"<redacted>")
                .finish(),
            Self::Google { project, .. } => formatter
                .debug_struct("Google")
                .field("access_token", &"<redacted>")
                .field("project", project)
                .finish(),
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
pub struct GeminiConfig {
    pub base_url: String,
    pub auth: GeminiAuth,
    pub retry: RetryPolicy,
    pub timeouts: Timeouts,
}

impl std::fmt::Debug for GeminiConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiConfig")
            .field("base_url", &self.base_url)
            .field("auth", &self.auth)
            .field("retry", &self.retry)
            .field("timeouts", &self.timeouts)
            .finish()
    }
}

impl GeminiConfig {
    pub fn api_key(key: &str) -> Self {
        Self {
            base_url: API_BASE_URL.to_owned(),
            auth: GeminiAuth::ApiKey(key.to_owned()),
            retry: RetryPolicy::default(),
            timeouts: Timeouts::default(),
        }
    }

    pub fn google(access_token: &str) -> Self {
        Self {
            base_url: CLOUD_CODE_BASE_URL.to_owned(),
            auth: GeminiAuth::Google {
                access_token: access_token.to_owned(),
                project: None,
            },
            retry: RetryPolicy::default(),
            timeouts: Timeouts::default(),
        }
    }
}

pub struct GeminiProvider {
    http: reqwest::Client,
    base: Url,
    auth: GeminiAuth,
    retry: RetryPolicy,
    timeouts: Timeouts,
    project: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for GeminiProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiProvider")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

impl GeminiProvider {
    pub fn new(config: &GeminiConfig) -> Result<Self, ProviderError> {
        let base = validate_base(&config.base_url)?;
        let http = reqwest::Client::builder()
            .connect_timeout(config.timeouts.connect)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(USER_AGENT)
            .build()
            .map_err(transport)?;
        let project = match &config.auth {
            GeminiAuth::Google { project, .. } => project.clone(),
            GeminiAuth::ApiKey(_) => None,
        };
        Ok(Self {
            http,
            base,
            auth: config.auth.clone(),
            retry: config.retry,
            timeouts: config.timeouts,
            project: Arc::new(Mutex::new(project)),
        })
    }

    fn stream_url(&self, model: &str) -> Result<Url, ProviderError> {
        self.base
            .join(&format!(
                "v1beta/models/{model}:streamGenerateContent?alt=sse"
            ))
            .map_err(|error| ProviderError::InvalidRequest {
                reason: format!("the model name cannot form a url: {error}"),
            })
    }

    fn internal_url(&self, method: &str) -> Result<Url, ProviderError> {
        self.base
            .join(&format!("/v1internal:{method}"))
            .map_err(|error| ProviderError::InvalidRequest {
                reason: format!("the base url cannot form a request: {error}"),
            })
    }

    async fn project(&self) -> Result<String, ProviderError> {
        if let Some(project) = self.project.lock().await.clone() {
            return Ok(project);
        }
        let authorization = match &self.auth {
            GeminiAuth::Google { access_token, .. } => bearer(access_token)?,
            GeminiAuth::ApiKey(_) => {
                return Err(ProviderError::InvalidRequest {
                    reason: "a google sign-in is required for the cloud code endpoint".to_owned(),
                });
            }
        };
        let mut project = self.load_project(&authorization).await;
        if project.is_none() {
            tracing::warn!("loadCodeAssist returned no project, trying onboardUser");
            self.onboard(&authorization).await;
            project = self.load_project(&authorization).await;
        }
        let Some(project) = project else {
            return Err(ProviderError::Protocol {
                message: "google refused code assist for this account (the sign-in client is marked unsupported or has no project). add a gemini api key from google ai studio in settings, or use a cloud project with code assist enabled.".to_owned(),
            });
        };
        *self.project.lock().await = Some(project.clone());
        Ok(project)
    }

    async fn load_project(&self, authorization: &HeaderValue) -> Option<String> {
        let url = self.internal_url("loadCodeAssist").ok()?;
        let response = self
            .http
            .post(url)
            .header("authorization", authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(r#"{"metadata":{"pluginType":"GEMINI"}}"#)
            .send()
            .await
            .ok()?;
        let status = response.status();
        let body = response.text().await.ok()?;
        if !status.is_success() {
            tracing::warn!(status = %status, body = %body.chars().take(300).collect::<String>(), "loadCodeAssist failed");
            return None;
        }
        let parsed: Value = serde_json::from_str(&body).ok()?;
        parsed.get("cloudaicompanionProject").and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.get("id").and_then(Value::as_str).map(str::to_owned))
        })
    }

    async fn onboard(&self, authorization: &HeaderValue) {
        let Ok(url) = self.internal_url("onboardUser") else {
            return;
        };
        let response = self
            .http
            .post(url)
            .header("authorization", authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body("{}")
            .send()
            .await;
        match response {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                tracing::warn!(
                    %status,
                    body = %body.chars().take(300).collect::<String>(),
                    "onboardUser answered"
                );
            }
            Err(error) => {
                tracing::warn!(error = %error.without_url(), "onboardUser could not be reached");
            }
        }
    }

    async fn attempt(
        &self,
        body: Vec<u8>,
        url: Url,
        api_key: Option<&str>,
        first_event: Duration,
    ) -> Result<(Response, tokio::time::Instant), ProviderError> {
        let deadline = stream::deadline_after(first_event);
        let response = tokio::time::timeout_at(deadline, async {
            let mut request = self
                .http
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(ACCEPT, "text/event-stream")
                .body(body);
            if let Some(key) = api_key {
                request = request.header("x-goog-api-key", sensitive(key)?);
            } else if let GeminiAuth::Google { access_token, .. } = &self.auth {
                request = request.header("authorization", bearer(access_token)?);
            }
            let response = request.send().await.map_err(transport)?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            let body = read_error_body(response).await;
            Err(ProviderError::http(status.as_u16(), &body, None, None))
        })
        .await
        .map_err(|_| ProviderError::Timeout {
            phase: TimeoutPhase::FirstEvent,
        })??;
        Ok((response, deadline))
    }
}

impl Provider for GeminiProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
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
            let payload = match &self.auth {
                GeminiAuth::ApiKey(_) => encode(&request),
                GeminiAuth::Google { .. } => {
                    let project = self.project().await?;
                    encode_cloud(&request, &project)
                }
            };
            let body =
                serde_json::to_vec(&payload).map_err(|error| ProviderError::InvalidRequest {
                    reason: format!("the request body could not be encoded: {error}"),
                })?;
            let api_key = match &self.auth {
                GeminiAuth::ApiKey(key) => Some(key.clone()),
                GeminiAuth::Google { .. } => None,
            };
            let url = match &self.auth {
                GeminiAuth::ApiKey(_) => self.stream_url(&request.model)?,
                GeminiAuth::Google { .. } => {
                    let mut url = self.internal_url("streamGenerateContent")?;
                    url.set_query(Some("alt=sse"));
                    url
                }
            };
            let (response, first_event) = bh_provider::retry(&self.retry, &cancel, || {
                self.attempt(
                    body.clone(),
                    url.clone(),
                    api_key.as_deref(),
                    self.timeouts.first_event,
                )
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
                request.model.clone(),
            ))
        })
    }
}

fn encode(request: &ProviderRequest) -> Value {
    let mut payload = json!({
        "contents": contents(request),
        "generationConfig": {
            "maxOutputTokens": request.max_output_tokens,
        },
    });
    let system = system_text(request);
    if !system.is_empty() {
        payload["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    payload
}

fn encode_cloud(request: &ProviderRequest, project: &str) -> Value {
    json!({
        "model": request.model,
        "project": project,
        "request": encode(request),
    })
}

fn contents(request: &ProviderRequest) -> Vec<Value> {
    let mut contents = Vec::new();
    for message in &request.messages {
        let mut parts = Vec::new();
        for block in &message.blocks {
            if let bh_types::ContentBlock::Text(text) = block {
                parts.push(json!({ "text": text.text }));
            }
        }
        if parts.is_empty() {
            continue;
        }
        let role = match message.role {
            bh_types::Role::User => "user",
            bh_types::Role::Assistant => "model",
        };
        contents.push(json!({ "role": role, "parts": parts }));
    }
    contents
}

fn system_text(request: &ProviderRequest) -> String {
    request
        .system
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn capabilities_for(model: &str) -> ProviderCapabilities {
    let known = model.starts_with("gemini-");
    ProviderCapabilities {
        context_window: if known { 1_000_000 } else { 128_000 },
        max_output_tokens: if known { 65_536 } else { 8_192 },
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

fn bearer(token: &str) -> Result<HeaderValue, ProviderError> {
    sensitive(&format!("Bearer {token}"))
}

fn sensitive(secret: &str) -> Result<HeaderValue, ProviderError> {
    let mut value = HeaderValue::from_str(secret).map_err(|_| ProviderError::Auth {
        message: "the credential contains characters that are not valid in a header".to_owned(),
    })?;
    value.set_sensitive(true);
    Ok(value)
}

fn validate_base(base_url: &str) -> Result<Url, ProviderError> {
    let invalid = |reason: &str| ProviderError::InvalidRequest {
        reason: format!("base URL {reason}"),
    };
    let url = Url::parse(base_url).map_err(|error| invalid(&format!("is not valid: {error}")))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("must not contain a query or fragment"));
    }
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        "http" => return Err(invalid("must use https unless the host is loopback")),
        other => return Err(invalid(&format!("has unsupported scheme {other}"))),
    }
    Ok(url)
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
