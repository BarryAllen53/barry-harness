#![allow(clippy::needless_pass_by_value)]

use bh_antigravity::{AntigravityConfig, AntigravityProvider, CatalogModel, Quota};
use bh_auth::{Credential, Mode, ProviderId};
use bh_config::ProjectEntry;
use bh_types::ipc::ErrorEnvelope;
use reqwest::{RequestBuilder, header::HeaderValue};
use serde::Serialize;
use tauri::State;

use crate::{commands::AppState, providers};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
}

fn efforts_for(provider: &str) -> Vec<String> {
    let scale: &[&str] = match provider {
        "anthropic" => &["none", "low", "medium", "high", "xhigh"],
        providers::ANTIGRAVITY => &["none"],
        _ => &["none", "low", "medium", "high"],
    };
    scale.iter().map(|value| (*value).to_owned()).collect()
}

const ANTHROPIC_DEFAULTS: [(&str, &str); 3] = [
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("claude-haiku-4-5", "Claude Haiku 4.5"),
];
const OPENAI_DEFAULTS: [(&str, &str); 3] = [
    ("gpt-5-codex", "GPT-5 Codex"),
    ("codex-mini-latest", "Codex mini"),
    ("gpt-5.1-codex-max", "GPT-5.1 Codex Max"),
];
const GEMINI_DEFAULTS: [(&str, &str); 5] = [
    ("gemini-4-neon", "Gemini 4 Neon"),
    ("gemini-4-hydrogen", "Gemini 4 Hydrogen"),
    ("gemini-4-argon", "Gemini 4 Argon"),
    ("gemini-3.1-pro-preview", "Gemini 3.1 Pro"),
    ("gemini-2.5-pro", "Gemini 2.5 Pro"),
];

fn defaults(provider: ProviderId) -> Vec<ModelInfo> {
    let name = match provider {
        ProviderId::Anthropic => "anthropic",
        ProviderId::OpenAi => "openai",
        ProviderId::Gemini => "gemini",
    };
    let table: &[(&str, &str)] = match provider {
        ProviderId::Anthropic => &ANTHROPIC_DEFAULTS,
        ProviderId::OpenAi => &OPENAI_DEFAULTS,
        ProviderId::Gemini => &GEMINI_DEFAULTS,
    };
    table
        .iter()
        .map(|(id, label)| ModelInfo {
            provider: name.to_owned(),
            id: (*id).to_owned(),
            label: (*label).to_owned(),
            efforts: efforts_for(name),
        })
        .collect()
}

const USER_AGENT: &str = concat!("BarryHarness/", env!("CARGO_PKG_VERSION"));

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(USER_AGENT)
        .build()
        .inspect_err(|error| tracing::warn!(%error, "the model list client could not start"))
        .ok()
}

fn with_secret(request: RequestBuilder, name: &'static str, value: &str) -> RequestBuilder {
    if let Ok(mut header) = HeaderValue::from_str(value) {
        header.set_sensitive(true);
        request.header(name, header)
    } else {
        tracing::warn!(
            header = name,
            "a stored credential is not a valid header value"
        );
        request
    }
}

fn bearer(request: RequestBuilder, token: &str) -> RequestBuilder {
    with_secret(request, "authorization", &format!("Bearer {token}"))
}

fn credential(state: &AppState, provider: ProviderId, mode: Mode) -> Option<Credential> {
    state.store().get(provider, mode).ok().flatten()
}

fn env_key(provider: ProviderId) -> Option<String> {
    let name = match provider {
        ProviderId::Anthropic => providers::ANTHROPIC_KEY_VARIABLE,
        ProviderId::OpenAi => providers::OPENAI_KEY_VARIABLE,
        ProviderId::Gemini => providers::GEMINI_KEY_VARIABLE,
    };
    std::env::var(name)
        .ok()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
}

async fn anthropic_models(state: &AppState) -> Vec<ModelInfo> {
    let oauth = credential(state, ProviderId::Anthropic, Mode::OAuth);
    let key = credential(state, ProviderId::Anthropic, Mode::ApiKey)
        .or_else(|| env_key(ProviderId::Anthropic).map(Credential::ApiKey));
    let Some(client) = client() else {
        return fallback(ProviderId::Anthropic).await;
    };
    let mut request = client.get("https://api.anthropic.com/v1/models?limit=100");
    request = match (&oauth, &key) {
        (Some(Credential::OAuth(tokens)), _) => bearer(request, &tokens.access)
            .header("anthropic-beta", bh_anthropic::OAUTH_BETA)
            .header("x-app", "cli")
            .header("user-agent", bh_anthropic::CLAUDE_CODE_AGENT),
        (_, Some(Credential::ApiKey(key))) => with_secret(request, "x-api-key", key),
        _ => return fallback(ProviderId::Anthropic).await,
    };
    let response = request
        .header("anthropic-version", bh_anthropic::ANTHROPIC_VERSION)
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            let body = read_json(response).await;
            let models = parse_list(&body, "data", "id", "display_name", "anthropic");
            if models.is_empty() {
                tracing::warn!("anthropic returned no models, using the built-in list");
                fallback(ProviderId::Anthropic).await
            } else {
                models
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "anthropic models could not be listed");
            fallback(ProviderId::Anthropic).await
        }
        Err(error) => {
            tracing::warn!(error = %error.without_url(), "anthropic models could not be listed");
            fallback(ProviderId::Anthropic).await
        }
    }
}

async fn openai_models(state: &AppState) -> Vec<ModelInfo> {
    let oauth = credential(state, ProviderId::OpenAi, Mode::OAuth);
    let key = credential(state, ProviderId::OpenAi, Mode::ApiKey)
        .or_else(|| env_key(ProviderId::OpenAi).map(Credential::ApiKey));
    let Some(Credential::ApiKey(key)) = key else {
        let Some(Credential::OAuth(tokens)) = oauth else {
            return fallback(ProviderId::OpenAi).await;
        };
        let Some(account) = tokens.account_id.clone() else {
            return fallback(ProviderId::OpenAi).await;
        };
        let Some(client) = client() else {
            return fallback(ProviderId::OpenAi).await;
        };
        let probe = bearer(
            client.get("https://chatgpt.com/backend-api/codex/models"),
            &tokens.access,
        )
        .header("chatgpt-account-id", account)
        .header("originator", "codex_cli_rs")
        .header("user-agent", bh_openai::CHATGPT_USER_AGENT)
        .header("session_id", codex_session_id())
        .send()
        .await;
        return match probe {
            Ok(probe) if probe.status().is_success() => {
                let body = read_json(probe).await;
                tracing::info!(
                    shape = %body.to_string().chars().take(400).collect::<String>(),
                    "chatgpt model probe"
                );
                let mut models = parse_list(&body, "models", "id", "name", "openai");
                models.extend(parse_list(&body, "data", "id", "id", "openai"));
                models.extend(parse_list(&body, "data", "slug", "display_name", "openai"));
                if models.is_empty() {
                    fallback(ProviderId::OpenAi).await
                } else {
                    models
                }
            }
            Ok(probe) => {
                tracing::warn!(status = %probe.status(), "chatgpt model probe failed");
                fallback(ProviderId::OpenAi).await
            }
            Err(error) => {
                tracing::warn!(error = %error.without_url(), "chatgpt model probe failed");
                fallback(ProviderId::OpenAi).await
            }
        };
    };
    let Some(client) = client() else {
        return fallback(ProviderId::OpenAi).await;
    };
    let response = bearer(client.get("https://api.openai.com/v1/models"), &key)
        .send()
        .await;
    match response {
        Ok(response) if response.status().is_success() => {
            let body = read_json(response).await;
            let mut models = parse_list(&body, "data", "id", "id", "openai");
            models.retain(|model| {
                model.id.starts_with("gpt-5")
                    || model.id.starts_with("gpt-4.1")
                    || model.id.starts_with("o4")
            });
            if models.is_empty() {
                fallback(ProviderId::OpenAi).await
            } else {
                models
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "openai models could not be listed");
            fallback(ProviderId::OpenAi).await
        }
        Err(error) => {
            tracing::warn!(error = %error.without_url(), "openai models could not be listed");
            fallback(ProviderId::OpenAi).await
        }
    }
}

async fn gemini_models(state: &AppState) -> Vec<ModelInfo> {
    let oauth = credential(state, ProviderId::Gemini, Mode::OAuth);
    let key = credential(state, ProviderId::Gemini, Mode::ApiKey)
        .or_else(|| env_key(ProviderId::Gemini).map(Credential::ApiKey));
    let Some(client) = client() else {
        return fallback(ProviderId::Gemini).await;
    };
    let request =
        client.get("https://generativelanguage.googleapis.com/v1beta/models?pageSize=200");
    let request = match (&oauth, &key) {
        (Some(Credential::OAuth(tokens)), _) => bearer(request, &tokens.access),
        (_, Some(Credential::ApiKey(key))) => with_secret(request, "x-goog-api-key", key),
        _ => request,
    };
    let response = request.send().await;
    match response {
        Ok(response) if response.status().is_success() => {
            let body = read_json(response).await;
            let mut models = parse_list(&body, "models", "name", "displayName", "gemini");
            for model in &mut models {
                if let Some(stripped) = model.id.strip_prefix("models/") {
                    model.id = stripped.to_owned();
                }
            }
            if models.is_empty() {
                fallback(ProviderId::Gemini).await
            } else {
                models
            }
        }
        Ok(response) => {
            tracing::warn!(status = %response.status(), "gemini models could not be listed");
            probe_cloud_code(oauth.as_ref()).await
        }
        Err(error) => {
            tracing::warn!(error = %error.without_url(), "gemini models could not be listed");
            probe_cloud_code(oauth.as_ref()).await
        }
    }
}

async fn probe_cloud_code(oauth: Option<&Credential>) -> Vec<ModelInfo> {
    let Some(Credential::OAuth(tokens)) = oauth else {
        return fallback(ProviderId::Gemini).await;
    };
    let Some(client) = client() else {
        return fallback(ProviderId::Gemini).await;
    };
    let probe = bearer(
        client.post("https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels"),
        &tokens.access,
    )
    .header("content-type", "application/json")
    .body("{}")
    .send()
    .await;
    match probe {
        Ok(probe) if probe.status().is_success() => {
            let body = read_json(probe).await;
            tracing::info!(
                shape = %body.to_string().chars().take(400).collect::<String>(),
                "cloud code model probe"
            );
            let mut models = parse_list(&body, "models", "name", "displayName", "gemini");
            models.extend(parse_list(
                &body,
                "availableModels",
                "model",
                "displayName",
                "gemini",
            ));
            models.extend(parse_list(&body, "data", "id", "name", "gemini"));
            if models.is_empty() {
                fallback(ProviderId::Gemini).await
            } else {
                models
            }
        }
        Ok(probe) => {
            tracing::warn!(status = %probe.status(), "cloud code model probe failed");
            fallback(ProviderId::Gemini).await
        }
        Err(error) => {
            tracing::warn!(error = %error.without_url(), "cloud code model probe failed");
            fallback(ProviderId::Gemini).await
        }
    }
}

async fn antigravity_models() -> Vec<ModelInfo> {
    match AntigravityProvider::connect(&AntigravityConfig::default()).await {
        Ok(provider) => {
            let today = utc_today();
            provider
                .catalog()
                .models()
                .iter()
                .map(|model| antigravity_model(model, &today))
                .collect()
        }
        Err(error) => {
            tracing::debug!(%error, "antigravity models are not available");
            Vec::new()
        }
    }
}

fn antigravity_model(model: &CatalogModel, today: &str) -> ModelInfo {
    ModelInfo {
        provider: providers::ANTIGRAVITY.to_owned(),
        id: model.id.clone(),
        label: quota_label(&model.label, model.quota.as_ref(), today),
        efforts: efforts_for(providers::ANTIGRAVITY),
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn utc_today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() / 86_400);
    civil_date(days)
}

fn civil_date(days_since_epoch: u64) -> String {
    let shifted = days_since_epoch + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn reset_moment(time: &str, today: &str) -> Option<String> {
    if !time.ends_with('Z') {
        return None;
    }
    let date = time.get(..10)?;
    let clock = time.get(11..16)?;
    if date == today {
        return Some(format!("{clock} UTC"));
    }
    let month: usize = date.get(5..7)?.parse().ok()?;
    let name = MONTHS.get(month.checked_sub(1)?)?;
    let day: u32 = date.get(8..10)?.parse().ok()?;
    Some(format!("{name} {day}, {clock} UTC"))
}

fn quota_label(label: &str, quota: Option<&Quota>, today: &str) -> String {
    let reset = quota
        .and_then(|quota| quota.reset_time.as_deref())
        .and_then(|time| reset_moment(time, today));
    match quota {
        Some(quota) if quota.remaining_fraction <= 0.0 => match reset {
            Some(moment) => format!("{label} (no quota left until {moment})"),
            None => format!("{label} (no quota left)"),
        },
        Some(quota) if quota.remaining_fraction < 1.0 => {
            let percent = (quota.remaining_fraction * 100.0).round().clamp(1.0, 99.0);
            format!("{label} ({percent:.0}% quota left)")
        }
        _ => label.to_owned(),
    }
}

fn codex_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos:032x}{count:016x}")
}

static PUBLIC_CATALOG: tokio::sync::OnceCell<serde_json::Value> =
    tokio::sync::OnceCell::const_new();

fn provider_key(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Anthropic => "anthropic",
        ProviderId::OpenAi => "openai",
        ProviderId::Gemini => "google",
    }
}

fn is_chat_model(id: &str) -> bool {
    const NOISE: [&str; 12] = [
        "embedding",
        "image",
        "tts",
        "whisper",
        "transcribe",
        "realtime",
        "audio",
        "moderation",
        "dall-e",
        "sora",
        "veo",
        "live-",
    ];
    !NOISE.iter().any(|noise| id.contains(noise))
}

async fn public_models(provider: ProviderId) -> Vec<ModelInfo> {
    let catalog = PUBLIC_CATALOG
        .get_or_init(|| async {
            let Some(client) = client() else {
                return serde_json::Value::Null;
            };
            match client
                .get("https://models.dev/api.json")
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => read_json(response).await,
                Ok(response) => {
                    tracing::warn!(status = %response.status(), "the public catalog could not be read");
                    serde_json::Value::Null
                }
                Err(error) => {
                    tracing::warn!(error = %error.without_url(), "the public catalog could not be read");
                    serde_json::Value::Null
                }
            }
        })
        .await;
    let key = provider_key(provider);
    let Some(models) = catalog
        .get(key)
        .and_then(|entry| entry.get("models"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    models
        .iter()
        .filter(|(id, _)| is_chat_model(id))
        .map(|(id, entry)| ModelInfo {
            provider: match provider {
                ProviderId::Anthropic => "anthropic".to_owned(),
                ProviderId::OpenAi => "openai".to_owned(),
                ProviderId::Gemini => "gemini".to_owned(),
            },
            id: id.clone(),
            label: entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(id)
                .to_owned(),
            efforts: efforts_for(provider_key(provider)),
        })
        .collect()
}

async fn fallback(provider: ProviderId) -> Vec<ModelInfo> {
    let models = public_models(provider).await;
    if models.is_empty() {
        defaults(provider)
    } else {
        models
    }
}

async fn read_json(response: reqwest::Response) -> serde_json::Value {
    let text = response.text().await.unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}
fn parse_list(
    body: &serde_json::Value,
    list: &str,
    id_key: &str,
    label_key: &str,
    provider: &str,
) -> Vec<ModelInfo> {
    body.get(list)
        .and_then(serde_json::Value::as_array)
        .map_or_else(Vec::new, |items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get(id_key)?.as_str()?.to_owned();
                    let label = item
                        .get(label_key)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(&id)
                        .to_owned();
                    Some(ModelInfo {
                        provider: provider.to_owned(),
                        id,
                        label,
                        efforts: efforts_for(provider),
                    })
                })
                .collect()
        })
}

fn has_credentials(state: &AppState, provider: ProviderId) -> bool {
    credential(state, provider, Mode::OAuth).is_some()
        || credential(state, provider, Mode::ApiKey).is_some()
        || env_key(provider).is_some()
}

fn keep_configured(
    models: Vec<ModelInfo>,
    anthropic: bool,
    openai: bool,
    gemini: bool,
) -> Vec<ModelInfo> {
    models
        .into_iter()
        .filter(|model| match model.provider.as_str() {
            "openai" => openai,
            "gemini" => gemini,
            _ => anthropic,
        })
        .collect()
}

async fn when(configured: bool, listing: impl Future<Output = Vec<ModelInfo>>) -> Vec<ModelInfo> {
    if configured {
        listing.await
    } else {
        Vec::new()
    }
}

#[tauri::command]
pub async fn models_list(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, ErrorEnvelope> {
    let state = state.inner();
    let anthropic = has_credentials(state, ProviderId::Anthropic);
    let openai = has_credentials(state, ProviderId::OpenAi);
    let gemini = has_credentials(state, ProviderId::Gemini);
    let (anthropic_list, openai_list, gemini_list, antigravity_list) = tokio::join!(
        when(anthropic, anthropic_models(state)),
        when(openai, openai_models(state)),
        when(gemini, gemini_models(state)),
        antigravity_models(),
    );
    let mut models = anthropic_list;
    models.extend(openai_list);
    models.extend(gemini_list);
    let mut models = keep_configured(models, anthropic, openai, gemini);
    models.extend(antigravity_list);
    let count = |provider: &str| {
        models
            .iter()
            .filter(|model| model.provider == provider)
            .count()
    };
    tracing::info!(
        anthropic = count("anthropic"),
        openai = count("openai"),
        gemini = count("gemini"),
        antigravity = count(providers::ANTIGRAVITY),
        "model list ready"
    );
    Ok(models)
}

#[tauri::command]
pub fn project_pick(state: State<'_, AppState>) -> Result<Vec<ProjectEntry>, ErrorEnvelope> {
    let Some(directory) = rfd::FileDialog::new()
        .set_title("Choose a project folder")
        .pick_folder()
    else {
        return Ok(state.settings().projects);
    };
    let path = directory.display().to_string();
    let name = directory
        .file_name()
        .map_or_else(|| path.clone(), |name| name.to_string_lossy().into_owned());
    let mut settings = state.settings();
    if !settings.projects.iter().any(|project| project.path == path) {
        settings.projects.push(ProjectEntry {
            id: path.clone(),
            name,
            path: path.clone(),
        });
    }
    settings.active_project = Some(path);
    state.replace_settings(settings)?;
    Ok(state.settings().projects)
}

#[tauri::command]
pub fn project_remove(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<ProjectEntry>, ErrorEnvelope> {
    let mut settings = state.settings();
    settings.projects.retain(|project| project.id != id);
    if settings.active_project.as_deref() == Some(id.as_str()) {
        settings.active_project = settings.projects.first().map(|project| project.id.clone());
    }
    state.replace_settings(settings)?;
    Ok(state.settings().projects)
}

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>) -> Vec<ProjectEntry> {
    state.settings().projects
}
