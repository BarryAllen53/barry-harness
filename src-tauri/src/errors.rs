use bh_provider::ProviderError;
use bh_types::{ErrorCode, ipc::ErrorEnvelope};

pub fn from_provider(error: &ProviderError, correlation_id: &str) -> ErrorEnvelope {
    if let ProviderError::UsageLimit { reset_at, .. } = error {
        return ErrorEnvelope {
            code: ErrorCode::QuotaExceeded,
            message: usage_limit_message(*reset_at),
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: correlation_id.to_owned(),
        };
    }
    let code = error.code();
    if let ProviderError::Reported { message, .. } = error {
        return ErrorEnvelope {
            code,
            message: message.clone(),
            retryable: false,
            user_action: code.user_action(),
            request_id: None,
            correlation_id: correlation_id.to_owned(),
        };
    }
    if matches!(
        code,
        ErrorCode::InvalidRequest | ErrorCode::ModelUnavailable
    ) {
        return ErrorEnvelope {
            code,
            message: format!("The provider rejected the request: {error}"),
            retryable: false,
            user_action: code.user_action(),
            request_id: None,
            correlation_id: correlation_id.to_owned(),
        };
    }
    let request_id = match error {
        ProviderError::Http { request_id, .. } => request_id.clone(),
        _ => None,
    };
    ErrorEnvelope {
        code,
        message: presentable_message(code).to_owned(),
        retryable: error.is_retryable(),
        user_action: code.user_action(),
        request_id,
        correlation_id: correlation_id.to_owned(),
    }
}

pub fn invalid_request(message: &str, correlation_id: &str) -> ErrorEnvelope {
    fixed(ErrorCode::InvalidRequest, message, correlation_id)
}

pub fn internal(correlation_id: &str) -> ErrorEnvelope {
    fixed(
        ErrorCode::Internal,
        "Something went wrong inside Barry while generating the response. Send again.",
        correlation_id,
    )
}

fn usage_limit_message(reset_at: Option<u64>) -> String {
    match reset_at {
        Some(seconds) => format!(
            "Your Claude usage limit is reached. It resets at {:02}:{:02} UTC. Send again after that.",
            seconds / 3600 % 24,
            seconds / 60 % 60
        ),
        None => {
            "Your Claude usage limit is reached. Wait for the window to reset, then send again."
                .to_owned()
        }
    }
}
fn fixed(code: ErrorCode, message: &str, correlation_id: &str) -> ErrorEnvelope {
    ErrorEnvelope {
        code,
        message: message.to_owned(),
        retryable: false,
        user_action: code.user_action(),
        request_id: None,
        correlation_id: correlation_id.to_owned(),
    }
}

fn presentable_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::AuthMissing => {
            "No Anthropic API key is set. Set the ANTHROPIC_API_KEY environment variable to your key, then restart Barry."
        }
        ErrorCode::AuthInvalid => {
            "The Anthropic API key was rejected. Check the ANTHROPIC_API_KEY environment variable, then restart Barry."
        }
        ErrorCode::PermissionDenied => "This API key is not allowed to use the selected model.",
        ErrorCode::RateLimited => "Too many requests right now. Wait a moment, then send again.",
        ErrorCode::QuotaExceeded => {
            "The Anthropic account has no remaining credit. Check billing, then send again."
        }
        ErrorCode::Overloaded => "The provider is overloaded. Wait a moment, then send again.",
        ErrorCode::ContextExceeded => {
            "The conversation is too long for the model. Restart Barry to start a new conversation, or send a shorter message."
        }
        ErrorCode::ModelUnavailable => "The selected model is not available.",
        ErrorCode::NetworkUnreachable => {
            "Barry could not reach the provider. Check the network connection, then send again."
        }
        ErrorCode::Timeout => "The provider took too long to respond. Send again.",
        ErrorCode::TruncatedStream => "The response ended unexpectedly. Send again.",
        ErrorCode::InvalidRequest => "The provider rejected the request.",
        ErrorCode::Cancelled => "The response was stopped.",
        _ => "Something went wrong while generating the response.",
    }
}
