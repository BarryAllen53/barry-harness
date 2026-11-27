use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AuthMissing,
    AuthInvalid,
    PermissionDenied,
    RateLimited,
    Overloaded,
    QuotaExceeded,
    InvalidRequest,
    ContextExceeded,
    ContentRefused,
    ModelUnavailable,
    NetworkUnreachable,
    Timeout,
    TruncatedStream,
    ProviderProtocol,
    ToolInvalidInput,
    ToolFailed,
    ToolTimeout,
    PolicyDenied,
    SandboxFailure,
    McpUnavailable,
    McpProtocol,
    StoreCorrupt,
    StoreBusy,
    MigrationFailed,
    KeyringUnavailable,
    ConfigInvalid,
    Cancelled,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserAction {
    AddApiKey,
    ReplaceApiKey,
    ReduceContext,
    ChangeModel,
    Retry,
    OpenSettings,
    ReviewPolicy,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthMissing => "auth_missing",
            Self::AuthInvalid => "auth_invalid",
            Self::PermissionDenied => "permission_denied",
            Self::RateLimited => "rate_limited",
            Self::Overloaded => "overloaded",
            Self::QuotaExceeded => "quota_exceeded",
            Self::InvalidRequest => "invalid_request",
            Self::ContextExceeded => "context_exceeded",
            Self::ContentRefused => "content_refused",
            Self::ModelUnavailable => "model_unavailable",
            Self::NetworkUnreachable => "network_unreachable",
            Self::Timeout => "timeout",
            Self::TruncatedStream => "truncated_stream",
            Self::ProviderProtocol => "provider_protocol",
            Self::ToolInvalidInput => "tool_invalid_input",
            Self::ToolFailed => "tool_failed",
            Self::ToolTimeout => "tool_timeout",
            Self::PolicyDenied => "policy_denied",
            Self::SandboxFailure => "sandbox_failure",
            Self::McpUnavailable => "mcp_unavailable",
            Self::McpProtocol => "mcp_protocol",
            Self::StoreCorrupt => "store_corrupt",
            Self::StoreBusy => "store_busy",
            Self::MigrationFailed => "migration_failed",
            Self::KeyringUnavailable => "keyring_unavailable",
            Self::ConfigInvalid => "config_invalid",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }

    pub fn user_action(self) -> Option<UserAction> {
        match self {
            Self::AuthMissing => Some(UserAction::AddApiKey),
            Self::AuthInvalid => Some(UserAction::ReplaceApiKey),
            Self::ContextExceeded => Some(UserAction::ReduceContext),
            Self::ModelUnavailable => Some(UserAction::ChangeModel),
            Self::RateLimited | Self::Overloaded | Self::Timeout | Self::NetworkUnreachable => {
                Some(UserAction::Retry)
            }
            Self::ConfigInvalid | Self::KeyringUnavailable => Some(UserAction::OpenSettings),
            Self::PolicyDenied => Some(UserAction::ReviewPolicy),
            _ => None,
        }
    }
}

impl UserAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AddApiKey => "add_api_key",
            Self::ReplaceApiKey => "replace_api_key",
            Self::ReduceContext => "reduce_context",
            Self::ChangeModel => "change_model",
            Self::Retry => "retry",
            Self::OpenSettings => "open_settings",
            Self::ReviewPolicy => "review_policy",
        }
    }
}
