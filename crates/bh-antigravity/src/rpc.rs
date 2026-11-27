use std::{error::Error as _, time::Duration};

use bh_provider::ProviderError;
use reqwest::{Response, header::CONTENT_TYPE};
use serde_json::Value;

use crate::{envelope, hub::Hub};

const ERROR_BODY_LIMIT: usize = 4_096;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub(crate) struct HubClient {
    http: reqwest::Client,
    hub: Hub,
}

impl HubClient {
    pub(crate) fn new(http: reqwest::Client, hub: Hub) -> Self {
        Self { http, hub }
    }

    pub(crate) async fn call_raw(
        &self,
        method: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProviderError> {
        let response = self
            .http
            .post(self.hub.url(method))
            .header(CONTENT_TYPE, "application/json")
            .header("connect-protocol-version", "1")
            .header("x-codeium-csrf-token", self.hub.csrf().clone())
            .timeout(timeout)
            .body(body.to_string())
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            let body = read_error_body(response).await;
            return Err(ProviderError::http(status.as_u16(), &body, None, None));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(transport)
    }

    pub(crate) async fn call(
        &self,
        method: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let bytes = self.call_raw(method, body, timeout).await?;
        if bytes.is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        serde_json::from_slice(&bytes).map_err(|error| ProviderError::Protocol {
            message: format!("antigravity answered {method} with unreadable json: {error}"),
        })
    }

    pub(crate) async fn subscribe(
        &self,
        method: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<Response, ProviderError> {
        let framed = envelope::encode(body.to_string().as_bytes()).map_err(|error| {
            ProviderError::InvalidRequest {
                reason: error.to_string(),
            }
        })?;
        let request = self
            .http
            .post(self.hub.url(method))
            .header(CONTENT_TYPE, "application/connect+json")
            .header("connect-protocol-version", "1")
            .header("x-codeium-csrf-token", self.hub.csrf().clone())
            .body(framed)
            .send();
        let response = tokio::time::timeout(timeout, request)
            .await
            .map_err(|_| ProviderError::Timeout {
                phase: bh_provider::TimeoutPhase::FirstEvent,
            })?
            .map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            let body = read_error_body(response).await;
            return Err(ProviderError::http(status.as_u16(), &body, None, None));
        }
        Ok(response)
    }
}

async fn read_error_body(mut response: Response) -> Vec<u8> {
    let deadline = tokio::time::Instant::now() + ERROR_BODY_TIMEOUT;
    let mut body = Vec::new();
    while body.len() < ERROR_BODY_LIMIT {
        match tokio::time::timeout_at(deadline, response.chunk()).await {
            Ok(Ok(Some(chunk))) => body.extend_from_slice(&chunk),
            Ok(Ok(None)) => break,
            Ok(Err(error)) => {
                tracing::warn!(error = %transport(error), "an antigravity error body was cut short");
                break;
            }
            Err(_) => {
                tracing::warn!("an antigravity error body took too long and was cut short");
                break;
            }
        }
    }
    body.truncate(ERROR_BODY_LIMIT);
    body
}

pub(crate) fn transport(error: reqwest::Error) -> ProviderError {
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
