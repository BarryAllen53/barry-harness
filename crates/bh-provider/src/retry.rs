use std::{
    hash::{BuildHasher, RandomState},
    time::Duration,
};

use tokio_util::sync::CancellationToken;

use crate::ProviderError;

const BACKOFF_FACTOR: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            max_attempts: 5,
        }
    }
}

impl RetryPolicy {
    pub fn backoff_ceiling(&self, retry_index: u32) -> Duration {
        BACKOFF_FACTOR
            .checked_pow(retry_index)
            .and_then(|multiplier| self.base_delay.checked_mul(multiplier))
            .map_or(self.max_delay, |delay| delay.min(self.max_delay))
    }

    pub fn delay(&self, retry_index: u32, retry_after: Option<Duration>, random: u64) -> Duration {
        if let Some(retry_after) = retry_after {
            return retry_after.min(self.max_delay);
        }
        let ceiling =
            u64::try_from(self.backoff_ceiling(retry_index).as_nanos()).unwrap_or(u64::MAX);
        Duration::from_nanos(random % ceiling.saturating_add(1))
    }
}

pub async fn retry<T, Operation, Attempt>(
    policy: &RetryPolicy,
    cancel: &CancellationToken,
    mut operation: Operation,
) -> Result<T, ProviderError>
where
    Operation: FnMut() -> Attempt,
    Attempt: Future<Output = Result<T, ProviderError>>,
{
    let mut retry_index = 0;
    loop {
        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(ProviderError::Cancelled),
            outcome = operation() => outcome,
        };
        let error = match outcome {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        if !error.is_retryable() || retry_index + 1 >= policy.max_attempts {
            return Err(error);
        }
        let delay = policy.delay(retry_index, error.retry_after(), random_u64());
        tracing::warn!(
            attempt = retry_index + 1,
            code = error.code().as_str(),
            delay_ms = u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            "retrying provider request"
        );
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(ProviderError::Cancelled),
            () = tokio::time::sleep(delay) => {}
        }
        retry_index += 1;
    }
}

fn random_u64() -> u64 {
    RandomState::new().hash_one(std::time::SystemTime::now())
}
