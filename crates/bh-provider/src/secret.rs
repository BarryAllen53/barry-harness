use std::fmt;

use crate::ProviderError;

pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: String) -> Result<Self, ProviderError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(ProviderError::Auth {
                message: "the API key is empty".to_owned(),
            });
        }
        if trimmed.len() == value.len() {
            Ok(Self(value))
        } else {
            Ok(Self(trimmed.to_owned()))
        }
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey(<redacted>)")
    }
}
