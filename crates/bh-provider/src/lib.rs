mod capabilities;
mod error;
mod event;
mod request;
mod retry;
mod secret;
mod sse;

use bh_types::ProviderKind;
use futures_util::{future::BoxFuture, stream::BoxStream};
use tokio_util::sync::CancellationToken;

pub use capabilities::{CacheCapabilities, CacheMode, ProviderCapabilities};
pub use error::{ProviderError, TimeoutPhase, parse_retry_after};
pub use event::{BlockKind, ProviderEvent};
pub use request::{
    Breakpoint, BreakpointAt, CachePlan, CacheTtl, ProviderRequest, ReasoningConfig,
    ReasoningDisplay,
};
pub use retry::{RetryPolicy, retry};
pub use secret::ApiKey;
pub use sse::{SseDecoder, SseError, SseEvent};

pub type ProviderStream = BoxStream<'static, Result<ProviderEvent, ProviderError>>;

pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    fn capabilities(&self, model: &str) -> ProviderCapabilities;

    fn stream(
        &self,
        request: ProviderRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<ProviderStream, ProviderError>>;
}
