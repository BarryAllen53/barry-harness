mod catalog;
mod envelope;
mod hub;
mod rpc;
mod stream;

use std::{collections::VecDeque, path::PathBuf, time::Duration};

use bh_provider::{
    CacheCapabilities, CacheMode, Provider, ProviderCapabilities, ProviderError, ProviderEvent,
    ProviderRequest, ProviderStream, ReasoningDisplay,
};
use bh_types::{ContentBlock, Effort, ErrorCode, ProviderKind, Role, Usage};
use futures_util::{StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

pub use catalog::{Catalog, CatalogModel, Quota};

use crate::{
    rpc::{HubClient, transport},
    stream::{CascadeGuard, Pump, Turn, Updates, await_snapshot, deadline_after},
};

const USER_AGENT: &str = concat!("BarryHarness/", env!("CARGO_PKG_VERSION"));
const CASCADE_SOURCE_CLIENT: u32 = 1;
const UNKNOWN_CONTEXT_WINDOW: u64 = 128_000;
const UNKNOWN_MAX_OUTPUT_TOKENS: u32 = 8_192;

#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub connect: Duration,
    pub call: Duration,
    pub first_event: Duration,
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(2),
            call: Duration::from_secs(20),
            first_event: Duration::from_secs(60),
            idle: Duration::from_secs(120),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AntigravityConfig {
    pub hub_directory: Option<PathBuf>,
    pub server_image: String,
    pub timeouts: Timeouts,
}

impl Default for AntigravityConfig {
    fn default() -> Self {
        Self {
            hub_directory: std::env::home_dir()
                .map(|home| home.join(".gemini").join("antigravity").join("daemon")),
            server_image: hub::LANGUAGE_SERVER_IMAGE.to_owned(),
            timeouts: Timeouts::default(),
        }
    }
}

pub struct AntigravityProvider {
    client: HubClient,
    catalog: Catalog,
    timeouts: Timeouts,
}

impl std::fmt::Debug for AntigravityProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AntigravityProvider")
            .field("client", &self.client)
            .field("models", &self.catalog.models().len())
            .finish_non_exhaustive()
    }
}

impl AntigravityProvider {
    pub async fn connect(config: &AntigravityConfig) -> Result<Self, ProviderError> {
        let Some(directory) = &config.hub_directory else {
            return Err(hub::not_running());
        };
        let files = hub::read_hub_files(directory);
        if files.is_empty() {
            return Err(hub::not_running());
        }
        let http = reqwest::Client::builder()
            .connect_timeout(config.timeouts.connect)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(USER_AGENT)
            .build()
            .map_err(transport)?;
        let servers = hub::language_servers(&config.server_image).await;
        for file in hub::rank(files, servers.as_ref()) {
            let Some(hub) = file.hub() else {
                tracing::debug!(
                    port = file.port(),
                    "an antigravity hub token is not a header value"
                );
                continue;
            };
            let client = HubClient::new(http.clone(), hub);
            match load_catalog(&client, config.timeouts.call).await {
                Ok(catalog) => {
                    tracing::debug!(
                        port = file.port(),
                        models = catalog.models().len(),
                        "antigravity hub found"
                    );
                    return Ok(Self {
                        client,
                        catalog,
                        timeouts: config.timeouts,
                    });
                }
                Err(error) => {
                    tracing::debug!(port = file.port(), %error, "an antigravity hub did not answer");
                }
            }
        }
        Err(hub::not_running())
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    async fn start_cascade(&self) -> Result<String, ProviderError> {
        let answer = self
            .client
            .call(
                "StartCascade",
                &json!({ "source": CASCADE_SOURCE_CLIENT }),
                self.timeouts.call,
            )
            .await?;
        answer
            .get("cascadeId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ProviderError::Protocol {
                message: "antigravity started a cascade without an id".to_owned(),
            })
    }

    fn watch(&self, cascade_id: &str, cancel: &CancellationToken) -> CascadeGuard {
        let completed = CancellationToken::new();
        let abandoned = CancellationToken::new();
        let client = self.client.clone();
        let cancel = cancel.clone();
        let done = completed.clone();
        let gone = abandoned.clone();
        let body = json!({ "cascadeId": cascade_id });
        let timeout = self.timeouts.call;
        tokio::spawn(async move {
            tokio::select! {
                biased;
                () = done.cancelled() => return,
                () = cancel.cancelled() => {}
                () = gone.cancelled() => {}
            }
            match client.call("CancelCascadeInvocation", &body, timeout).await {
                Ok(_) => tracing::debug!("the antigravity run was cancelled"),
                Err(error) => tracing::warn!(%error, "the antigravity run could not be cancelled"),
            }
        });
        CascadeGuard::new(completed, abandoned)
    }

    async fn run(
        &self,
        request: ProviderRequest,
        cancel: CancellationToken,
    ) -> Result<ProviderStream, ProviderError> {
        let model = self
            .catalog
            .find(&request.model)
            .ok_or_else(|| ProviderError::Reported {
                code: ErrorCode::ModelUnavailable,
                message: format!(
                    "Antigravity does not offer the model {}. Pick a model from the list.",
                    request.model
                ),
            })?;
        let text = prompt_text(&request);
        if text.trim().is_empty() {
            return Err(ProviderError::InvalidRequest {
                reason: "the conversation has no text to send".to_owned(),
            });
        }
        let cascade_id = unless_cancelled(&cancel, self.start_cascade()).await?;
        let guard = self.watch(&cascade_id, &cancel);
        let response = unless_cancelled(
            &cancel,
            self.client.subscribe(
                "StreamAgentStateUpdates",
                &json!({ "conversationId": cascade_id }),
                self.timeouts.call,
            ),
        )
        .await?;
        let mut updates = Updates::new(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(transport)),
        );
        let mut turn = Turn::default();
        await_snapshot(
            &mut updates,
            &mut turn,
            &cancel,
            deadline_after(self.timeouts.call),
        )
        .await?;
        unless_cancelled(
            &cancel,
            self.client.call(
                "SendUserCascadeMessage",
                &message_body(&cascade_id, model.wire_name(), &text),
                self.timeouts.call,
            ),
        )
        .await?;
        let mut pending = VecDeque::new();
        pending.push_back(Ok(ProviderEvent::MessageStart {
            id: cascade_id,
            model: request.model,
            usage: Usage::default(),
        }));
        Ok(stream::provider_events(Pump {
            updates: Some(updates),
            turn,
            pending,
            cancel,
            deadline: deadline_after(self.timeouts.first_event),
            idle: self.timeouts.idle,
            guard,
        }))
    }
}

impl Provider for AntigravityProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Antigravity
    }

    fn capabilities(&self, model: &str) -> ProviderCapabilities {
        let entry = self.catalog.find(model);
        let thinking = entry.is_some_and(|entry| entry.thinking);
        ProviderCapabilities {
            context_window: entry.map_or(UNKNOWN_CONTEXT_WINDOW, |entry| entry.context_window),
            max_output_tokens: entry
                .map_or(UNKNOWN_MAX_OUTPUT_TOKENS, |entry| entry.max_output_tokens),
            efforts: vec![Effort::None],
            reasoning_display: if thinking {
                vec![ReasoningDisplay::Summarized]
            } else {
                vec![ReasoningDisplay::Omitted]
            },
            tools: false,
            parallel_tool_calls: false,
            vision_input: false,
            cache: CacheCapabilities {
                mode: CacheMode::AutomaticPrefixWithKey,
                max_breakpoints: 0,
                min_prefix_tokens: 0,
                ttls: Vec::new(),
                lookback_blocks: 0,
            },
            streaming_tool_args: false,
        }
    }

    fn stream(
        &self,
        request: ProviderRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<ProviderStream, ProviderError>> {
        Box::pin(self.run(request, cancel))
    }
}

async fn unless_cancelled<T>(
    cancel: &CancellationToken,
    work: impl Future<Output = Result<T, ProviderError>>,
) -> Result<T, ProviderError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(ProviderError::Cancelled),
        result = work => result,
    }
}

async fn load_catalog(client: &HubClient, timeout: Duration) -> Result<Catalog, ProviderError> {
    let body = client
        .call_raw("GetAvailableModels", &json!({}), timeout)
        .await?;
    Catalog::from_response(&body).map_err(|error| ProviderError::Protocol {
        message: format!("antigravity sent a model catalog the adapter cannot read: {error}"),
    })
}

fn message_body(cascade_id: &str, wire_name: &str, text: &str) -> Value {
    json!({
        "cascadeId": cascade_id,
        "items": [{ "text": text }],
        "cascadeConfig": {
            "plannerConfig": {
                "planModel": wire_name,
                "requestedModel": { "model": wire_name },
                "toolConfig": { "forceDisableToolCalling": true },
            },
        },
    })
}

fn prompt_text(request: &ProviderRequest) -> String {
    let system = request
        .system
        .segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let turns: Vec<(Role, String)> = request
        .messages
        .iter()
        .filter_map(|message| {
            let text = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.trim().is_empty()).then_some((message.role, text))
        })
        .collect();
    if let [(Role::User, only)] = turns.as_slice()
        && system.is_empty()
    {
        return only.clone();
    }
    let mut prompt =
        String::from("Continue the conversation below. Reply only to the last user message.\n\n");
    if !system.is_empty() {
        prompt.push_str("[System]\n");
        prompt.push_str(&system);
        prompt.push_str("\n\n");
    }
    for (role, text) in &turns {
        prompt.push_str(match role {
            Role::User => "[User]\n",
            Role::Assistant => "[Assistant]\n",
        });
        prompt.push_str(text);
        prompt.push_str("\n\n");
    }
    prompt.truncate(prompt.trim_end().len());
    prompt
}
