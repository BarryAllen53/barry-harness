use std::{fmt, time::Duration};

use bh_types::{ErrorCode, ToolCallId};

const BODY_EXCERPT_LIMIT: usize = 2048;
const SECRET_PREFIXES: [&str; 4] = ["sk-ant-", "sk-", "AIza", "ya29."];
const REDACTED: &str = "[REDACTED]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutPhase {
    FirstEvent,
    Idle,
}

impl fmt::Display for TimeoutPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FirstEvent => "waiting for the first event",
            Self::Idle => "waiting for the next event",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider returned HTTP {status}{}: {body_excerpt}", request_suffix(.request_id.as_deref()))]
    Http {
        status: u16,
        body_excerpt: String,
        request_id: Option<String>,
        retry_after: Option<Duration>,
    },
    #[error("transport failure: {message}")]
    Transport { message: String },
    #[error("provider stream error {kind}: {message}")]
    Stream { kind: String, message: String },
    #[error("provider stream ended before the message was complete")]
    TruncatedStream,
    #[error("tool call {call_id} has malformed arguments")]
    MalformedToolArguments { call_id: ToolCallId, raw: String },
    #[error("provider protocol violation: {message}")]
    Protocol { message: String },
    #[error("authentication is not configured: {message}")]
    Auth { message: String },
    #[error("invalid request: {reason}")]
    InvalidRequest { reason: String },
    #[error("timed out {phase}")]
    Timeout { phase: TimeoutPhase },
    #[error("subscription usage limit reached{until}", until = reset_suffix(*.reset_at))]
    UsageLimit {
        message: String,
        reset_at: Option<u64>,
    },
    #[error("{message}")]
    Reported { code: ErrorCode, message: String },
    #[error("request cancelled")]
    Cancelled,
}

fn reset_suffix(reset_at: Option<u64>) -> String {
    reset_at.map_or_else(String::new, |seconds| {
        let minutes = seconds / 60 % 60;
        let hours = seconds / 3600 % 24;
        format!(" until {hours:02}:{minutes:02} UTC")
    })
}

fn request_suffix(request_id: Option<&str>) -> String {
    request_id.map_or_else(String::new, |id| format!(" (request id {id})"))
}

impl ProviderError {
    pub fn http(
        status: u16,
        body: &[u8],
        request_id: Option<String>,
        retry_after: Option<Duration>,
    ) -> Self {
        Self::Http {
            status,
            body_excerpt: redact_secrets(&excerpt(body)),
            request_id,
            retry_after,
        }
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Http {
                status,
                body_excerpt,
                ..
            } => http_code(*status, body_excerpt),
            Self::Transport { .. } => ErrorCode::NetworkUnreachable,
            Self::Stream { kind, .. } => match kind.as_str() {
                "overloaded_error" | "api_error" => ErrorCode::Overloaded,
                "rate_limit_error" => ErrorCode::RateLimited,
                _ => ErrorCode::ProviderProtocol,
            },
            Self::TruncatedStream => ErrorCode::TruncatedStream,
            Self::MalformedToolArguments { .. } | Self::Protocol { .. } => {
                ErrorCode::ProviderProtocol
            }
            Self::Auth { .. } => ErrorCode::AuthMissing,
            Self::InvalidRequest { .. } => ErrorCode::InvalidRequest,
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::UsageLimit { .. } => ErrorCode::QuotaExceeded,
            Self::Reported { code, .. } => *code,
            Self::Cancelled => ErrorCode::Cancelled,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http {
                status,
                body_excerpt,
                ..
            } => match status {
                408 | 409 | 500..=504 | 529 => true,
                429 => !mentions_quota(body_excerpt),
                _ => false,
            },
            Self::Transport { .. }
            | Self::Timeout {
                phase: TimeoutPhase::FirstEvent,
            } => true,
            _ => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

fn http_code(status: u16, body_excerpt: &str) -> ErrorCode {
    match status {
        400 if mentions_context_limit(body_excerpt) => ErrorCode::ContextExceeded,
        400 | 409 | 413 | 422 => ErrorCode::InvalidRequest,
        401 => ErrorCode::AuthInvalid,
        403 => ErrorCode::PermissionDenied,
        404 => ErrorCode::ModelUnavailable,
        408 => ErrorCode::Timeout,
        429 if mentions_quota(body_excerpt) => ErrorCode::QuotaExceeded,
        429 => ErrorCode::RateLimited,
        500..=504 | 529 => ErrorCode::Overloaded,
        _ => ErrorCode::ProviderProtocol,
    }
}

fn mentions_context_limit(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    ["prompt is too long", "context window", "context length"]
        .iter()
        .any(|needle| body.contains(needle))
}

fn mentions_quota(body: &str) -> bool {
    let body = body.to_ascii_lowercase();
    ["billing", "quota", "credit balance"]
        .iter()
        .any(|needle| body.contains(needle))
}

fn excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if text.len() <= BODY_EXCERPT_LIMIT {
        return text.into_owned();
    }
    let mut end = BODY_EXCERPT_LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn redact_secrets(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((start, prefix)) = find_secret_start(rest) {
        output.push_str(&rest[..start]);
        let after_prefix = &rest[start + prefix.len()..];
        let secret_len = after_prefix
            .find(|character: char| !is_secret_character(character))
            .unwrap_or(after_prefix.len());
        if secret_len == 0 {
            output.push_str(prefix);
        } else {
            output.push_str(REDACTED);
        }
        rest = &after_prefix[secret_len..];
    }
    output.push_str(rest);
    output
}

fn find_secret_start(text: &str) -> Option<(usize, &'static str)> {
    SECRET_PREFIXES
        .iter()
        .filter_map(|prefix| {
            text.match_indices(prefix)
                .map(|(position, _)| position)
                .find(|position| {
                    text[..*position]
                        .chars()
                        .next_back()
                        .is_none_or(|previous| !is_secret_character(previous))
                })
                .map(|position| (position, *prefix))
        })
        .min_by_key(|(position, prefix)| (*position, usize::MAX - prefix.len()))
}

fn is_secret_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    let seconds: f64 = value.parse().ok()?;
    Some(Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX))
}
