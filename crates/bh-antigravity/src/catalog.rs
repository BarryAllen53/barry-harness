use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub label: String,
    pub thinking: bool,
    pub max_output_tokens: u32,
    pub context_window: u64,
    pub quota: Option<Quota>,
    wire_name: String,
}

impl CatalogModel {
    pub(crate) fn wire_name(&self) -> &str {
        &self.wire_name
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quota {
    pub remaining_fraction: f64,
    pub reset_time: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    models: Vec<CatalogModel>,
}

impl Catalog {
    pub fn models(&self) -> &[CatalogModel] {
        &self.models
    }

    pub(crate) fn find(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|model| model.id == id)
    }

    pub(crate) fn from_response(body: &[u8]) -> Result<Self, serde_json::Error> {
        let available: AvailableModels = serde_json::from_slice(body)?;
        Ok(Self::from_available(&available.response))
    }

    fn from_available(response: &ModelsResponse) -> Self {
        let mut seen = HashSet::new();
        let mut models: Vec<CatalogModel> = response
            .agent_model_sorts
            .iter()
            .flat_map(|sort| &sort.groups)
            .flat_map(|group| &group.model_ids)
            .filter(|id| seen.insert(id.as_str()))
            .filter_map(|id| usable(id, response.models.get(id)?))
            .collect();
        if models.is_empty() {
            models = response
                .models
                .iter()
                .filter(|(_, entry)| entry.display_name.is_some())
                .filter_map(|(id, entry)| usable(id, entry))
                .collect();
        }
        Self { models }
    }
}

fn usable(id: &str, entry: &ModelEntry) -> Option<CatalogModel> {
    if entry.disabled {
        return None;
    }
    let wire_name = entry.model.as_deref().filter(|name| !name.is_empty())?;
    Some(CatalogModel {
        id: id.to_owned(),
        label: entry.display_name.clone().unwrap_or_else(|| id.to_owned()),
        thinking: entry.supports_thinking,
        max_output_tokens: entry.max_output_tokens,
        context_window: entry.max_tokens,
        quota: entry.quota_info.as_ref().map(|quota| Quota {
            remaining_fraction: quota.remaining_fraction,
            reset_time: quota.reset_time.clone(),
        }),
        wire_name: wire_name.to_owned(),
    })
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AvailableModels {
    response: ModelsResponse,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModelsResponse {
    models: BTreeMap<String, ModelEntry>,
    agent_model_sorts: Vec<ModelSort>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ModelSort {
    groups: Vec<ModelGroup>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModelGroup {
    model_ids: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModelEntry {
    display_name: Option<String>,
    supports_thinking: bool,
    max_tokens: u64,
    max_output_tokens: u32,
    quota_info: Option<QuotaInfo>,
    model: Option<String>,
    disabled: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct QuotaInfo {
    remaining_fraction: f64,
    reset_time: Option<String>,
}
