use std::{cell::RefCell, rc::Rc, time::Duration};

use bh_types::{
    ErrorCode,
    ipc::{ErrorEnvelope, StreamFrame},
};
use leptos::{logging, prelude::set_timeout, task::spawn_local};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;

use crate::logic::{
    channel_order::Packet,
    frame_stream::{FrameStream, StreamStep},
};

#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = invoke, catch)]
    fn tauri_invoke(command: &str, payload: JsValue) -> Result<js_sys::Promise, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = transformCallback, catch)]
    fn transform_callback(callback: &js_sys::Function, once: bool) -> Result<u32, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = unregisterCallback)]
    fn unregister_callback(id: u32);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendPromptArgs {
    prompt: String,
    on_frame: String,
}

#[derive(Deserialize)]
struct ChannelPacket {
    index: u64,
    #[serde(default)]
    message: Option<StreamFrame>,
    #[serde(default)]
    end: bool,
}

struct Subscription {
    id: u32,
    _callback: Closure<dyn FnMut(JsValue)>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        unregister_callback(self.id);
    }
}

pub async fn send_prompt(
    prompt: String,
    mut on_frame: impl FnMut(StreamFrame) + 'static,
) -> Result<(), ErrorEnvelope> {
    let slot: Rc<RefCell<Option<Subscription>>> = Rc::default();
    let callback_slot = Rc::clone(&slot);
    let mut stream = FrameStream::default();
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |raw: JsValue| {
        let mut step = match serde_wasm_bindgen::from_value::<ChannelPacket>(raw) {
            Ok(ChannelPacket {
                index,
                message: Some(frame),
                end: false,
            }) => stream.accept(index, Packet::Message(frame)),
            Ok(ChannelPacket {
                index, end: true, ..
            }) => stream.accept(index, Packet::End),
            Ok(ChannelPacket { index, .. }) => {
                logging::error!("stream packet {index} carried no frame");
                stream.malformed()
            }
            Err(error) => {
                logging::error!("malformed stream frame: {error}");
                stream.malformed()
            }
        };
        if step.wait_for_gap {
            let flushed = stream.gap_expired();
            step = StreamStep {
                frames: step.frames.into_iter().chain(flushed.frames).collect(),
                ended: step.ended || flushed.ended,
                wait_for_gap: false,
            };
        }
        for frame in step.frames {
            on_frame(frame);
        }
        if step.ended
            && let Some(subscription) = callback_slot.borrow_mut().take()
        {
            set_timeout(move || drop(subscription), Duration::ZERO);
        }
    });
    let id = transform_callback(callback.as_ref().unchecked_ref(), false)
        .map_err(|raw| internal_error(&format!("{raw:?}")))?;
    *slot.borrow_mut() = Some(Subscription {
        id,
        _callback: callback,
    });
    let args = SendPromptArgs {
        prompt,
        on_frame: format!("__CHANNEL__:{id}"),
    };
    let result = invoke("send_prompt", &args).await;
    if result.is_err() {
        slot.borrow_mut().take();
    }
    result
}

pub async fn cancel_turn() -> Result<(), ErrorEnvelope> {
    invoke("cancel_turn", &()).await
}

async fn invoke(command: &str, args: &impl Serialize) -> Result<(), ErrorEnvelope> {
    if in_browser_only() {
        return Err(ErrorEnvelope {
            code: ErrorCode::Internal,
            message: format!(
                "{command} needs the desktop app. Start barry.exe or run cargo tauri dev."
            ),
            retryable: false,
            user_action: None,
            request_id: None,
            correlation_id: "preview".to_owned(),
        });
    }
    let payload = match args.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
        Ok(payload) if payload.is_object() => payload,
        Ok(_) => js_sys::Object::new().into(),
        Err(error) => return Err(internal_error(&error.to_string())),
    };
    let promise =
        tauri_invoke(command, payload).map_err(|raw| internal_error(&format!("{raw:?}")))?;
    match JsFuture::from(promise).await {
        Ok(_) => Ok(()),
        Err(raw) => Err(serde_wasm_bindgen::from_value::<ErrorEnvelope>(raw.clone())
            .unwrap_or_else(|_| internal_error(&format!("{raw:?}")))),
    }
}

fn internal_error(detail: &str) -> ErrorEnvelope {
    logging::error!("IPC failure: {detail}");
    ErrorEnvelope {
        code: ErrorCode::Internal,
        message: "Barry Harness could not reach its backend. Restart the app.".to_owned(),
        retryable: false,
        user_action: None,
        request_id: None,
        correlation_id: "ipc".to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub efforts: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            provider: "anthropic".to_owned(),
            model: "claude-opus-5-5".to_owned(),
            effort: "high".to_owned(),
            system_prompt: String::new(),
            font_scale: 1.0,
            theme: "system".to_owned(),
            permission_mode: "ask".to_owned(),
            projects: Vec::new(),
            active_project: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInStatus {
    pub provider: String,
    pub signed_in: bool,
    pub account_label: Option<String>,
    pub expires_at: Option<u64>,
    pub has_api_key: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInOutcome {
    pub kind: String,
    pub account_label: Option<String>,
    pub expires_at: Option<u64>,
    pub url: Option<String>,
}

impl SignInOutcome {
    pub fn signed_in(&self) -> bool {
        self.kind == "signedIn"
    }

    pub fn needs_code(&self) -> bool {
        self.kind == "needsCode"
    }
}

pub async fn settings_get() -> Result<Settings, ErrorEnvelope> {
    invoke_value("settings_get", &()).await
}

pub async fn settings_set(settings: &Settings) -> Result<Settings, ErrorEnvelope> {
    invoke_value("settings_set", &serde_json::json!({ "settings": settings })).await
}

pub async fn credential_state() -> Result<Vec<SignInStatus>, ErrorEnvelope> {
    invoke_value("credential_state", &()).await
}

pub async fn credential_set(provider: &str, api_key: &str) -> Result<SignInStatus, ErrorEnvelope> {
    invoke_value(
        "credential_set",
        &serde_json::json!({ "provider": provider, "apiKey": api_key }),
    )
    .await
}

pub async fn credential_delete(provider: &str) -> Result<SignInStatus, ErrorEnvelope> {
    invoke_value(
        "credential_delete",
        &serde_json::json!({ "provider": provider }),
    )
    .await
}

pub async fn sign_in(provider: &str) -> Result<SignInOutcome, ErrorEnvelope> {
    invoke_value("sign_in", &serde_json::json!({ "provider": provider })).await
}

pub async fn sign_in_paste(provider: &str) -> Result<SignInOutcome, ErrorEnvelope> {
    invoke_value(
        "sign_in_paste",
        &serde_json::json!({ "provider": provider }),
    )
    .await
}

pub async fn sign_in_code(provider: &str, code: &str) -> Result<SignInOutcome, ErrorEnvelope> {
    invoke_value(
        "sign_in_code",
        &serde_json::json!({ "provider": provider, "code": code }),
    )
    .await
}

pub async fn sign_out(provider: &str) -> Result<SignInStatus, ErrorEnvelope> {
    invoke_value("sign_out", &serde_json::json!({ "provider": provider })).await
}

async fn invoke_value<T: DeserializeOwned>(
    command: &str,
    args: &impl Serialize,
) -> Result<T, ErrorEnvelope> {
    if in_browser_only() {
        return preview_value(command);
    }
    let raw = invoke_raw(command, args).await?;
    serde_wasm_bindgen::from_value(raw).map_err(|error| internal_error(&error.to_string()))
}

fn in_browser_only() -> bool {
    let scope = js_sys::global();
    js_sys::Reflect::get(&scope, &JsValue::from_str("__TAURI_INTERNALS__"))
        .map_or(true, |bridge| bridge.is_undefined() || bridge.is_null())
}

fn preview_value<T: DeserializeOwned>(command: &str) -> Result<T, ErrorEnvelope> {
    let value = match command {
        "settings_get" | "settings_set" => serde_wasm_bindgen::to_value(&Settings::default())
            .map_err(|error| internal_error(&error.to_string()))?,
        "credential_state" => serde_wasm_bindgen::to_value(&preview_statuses())
            .map_err(|error| internal_error(&error.to_string()))?,
        other => {
            return Err(ErrorEnvelope {
                code: ErrorCode::Internal,
                message: format!(
                    "{other} needs the desktop app. Start barry.exe or run cargo tauri dev."
                ),
                retryable: false,
                user_action: None,
                request_id: None,
                correlation_id: "preview".to_owned(),
            });
        }
    };
    serde_wasm_bindgen::from_value(value).map_err(|error| internal_error(&error.to_string()))
}

fn preview_statuses() -> Vec<SignInStatus> {
    ["anthropic", "openai", "gemini"]
        .into_iter()
        .map(|provider| SignInStatus {
            provider: provider.to_owned(),
            signed_in: false,
            account_label: None,
            expires_at: None,
            has_api_key: false,
        })
        .collect()
}

async fn invoke_raw(command: &str, args: &impl Serialize) -> Result<JsValue, ErrorEnvelope> {
    let payload = match args.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
        Ok(payload) if payload.is_object() => payload,
        Ok(_) => js_sys::Object::new().into(),
        Err(error) => return Err(internal_error(&error.to_string())),
    };
    let promise =
        tauri_invoke(command, payload).map_err(|raw| internal_error(&format!("{raw:?}")))?;
    match JsFuture::from(promise).await {
        Ok(value) => Ok(value),
        Err(raw) => Err(serde_wasm_bindgen::from_value::<ErrorEnvelope>(raw.clone())
            .unwrap_or_else(|_| internal_error(&format!("{raw:?}")))),
    }
}

pub async fn new_chat() -> Result<(), ErrorEnvelope> {
    invoke("new_chat", &()).await
}

#[derive(Deserialize)]
struct MenuEvent {
    payload: String,
}

pub fn listen_menu(handler: impl Fn(String) + 'static) {
    let callback =
        Closure::<dyn FnMut(JsValue)>::new(
            move |raw: JsValue| match serde_wasm_bindgen::from_value::<MenuEvent>(raw) {
                Ok(event) => handler(event.payload),
                Err(error) => logging::error!("malformed menu event: {error}"),
            },
        );
    let Ok(id) = transform_callback(callback.as_ref().unchecked_ref(), false) else {
        logging::error!("the menu listener could not be registered");
        return;
    };
    callback.forget();
    let args = serde_json::json!({
        "event": "menu",
        "target": { "kind": "Any" },
        "handler": id
    });
    spawn_local(async move {
        if let Err(error) = invoke_raw("plugin:event|listen", &args).await {
            logging::error!("the menu listener was rejected: {}", error.message);
        }
    });
}

pub async fn models_list() -> Result<Vec<ModelInfo>, ErrorEnvelope> {
    invoke_value("models_list", &()).await
}

pub async fn project_pick() -> Result<Vec<ProjectEntry>, ErrorEnvelope> {
    invoke_value("project_pick", &()).await
}

pub async fn project_remove(id: &str) -> Result<Vec<ProjectEntry>, ErrorEnvelope> {
    invoke_value("project_remove", &serde_json::json!({ "id": id })).await
}
