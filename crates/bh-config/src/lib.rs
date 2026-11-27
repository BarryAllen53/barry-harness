use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MIN_FONT_SCALE: f32 = 0.8;
pub const MAX_FONT_SCALE: f32 = 1.6;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub system_prompt: String,
    pub font_scale: f32,
    pub theme: String,
    pub permission_mode: String,
    #[serde(default)]
    pub projects: Vec<ProjectEntry>,
    #[serde(default)]
    pub active_project: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: VERSION,
            provider: "anthropic".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            effort: "high".to_owned(),
            system_prompt: String::new(),
            font_scale: 1.0,
            theme: "system".to_owned(),
            permission_mode: "ask".to_owned(),
            projects: Vec::new(),
            active_project: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.version = VERSION;
        if !self.font_scale.is_finite() {
            self.font_scale = 1.0;
        }
        self.font_scale = self.font_scale.clamp(MIN_FONT_SCALE, MAX_FONT_SCALE);
        if !matches!(
            self.provider.as_str(),
            "anthropic" | "openai" | "gemini" | "antigravity"
        ) {
            self.provider = String::from("anthropic");
        }
        self
    }
}

pub fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from);
    base.join("BarryHarness").join("settings.json")
}

pub fn load() -> Settings {
    load_from(&settings_path())
}

pub fn save(settings: &Settings) -> Result<(), String> {
    save_to(&settings_path(), settings)
}

fn load_from(path: &Path) -> Settings {
    let Ok(text) = fs::read_to_string(path) else {
        return Settings::default();
    };
    serde_json::from_str::<Settings>(&text)
        .map_or_else(|_| Settings::default(), Settings::sanitized)
}

fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    let settings = settings.clone().sanitized();
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let text = serde_json::to_string_pretty(&settings)
        .map_err(|error| format!("cannot encode the settings: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, text)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))
}
