use serde::{Deserialize, Serialize, de::IgnoredAny};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JsonText(String);

#[derive(Debug, thiserror::Error)]
#[error("text is not valid JSON: {0}")]
pub struct InvalidJson(#[source] serde_json::Error);

impl JsonText {
    pub fn new(text: String) -> Result<Self, InvalidJson> {
        serde_json::from_str::<IgnoredAny>(&text).map_err(InvalidJson)?;
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for JsonText {
    type Error = InvalidJson;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text)
    }
}

impl From<JsonText> for String {
    fn from(text: JsonText) -> Self {
        text.0
    }
}
