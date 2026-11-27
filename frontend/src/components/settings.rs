#![allow(clippy::too_many_lines)]

use leptos::{ev, prelude::*, task::spawn_local};

use crate::{
    components::icon::{Icon, IconName},
    ipc,
};

const PROVIDERS: [(&str, &str); 4] = [
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("gemini", "Google"),
    ("antigravity", "Antigravity"),
];

const LOCAL_HUB_PROVIDER: &str = "antigravity";

const EFFORTS: [(&str, &str); 5] = [
    ("none", "Off"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra high"),
];

const THEMES: [(&str, &str); 4] = [
    ("system", "System"),
    ("light", "Light"),
    ("dark", "Dark"),
    ("contrast", "High contrast"),
];

#[component]
pub fn SettingsView(
    settings: RwSignal<Option<ipc::Settings>>,
    statuses: RwSignal<Vec<ipc::SignInStatus>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let message = RwSignal::new(None::<String>);
    let code_for = RwSignal::new(None::<String>);
    let sign_in_url = RwSignal::new(String::new());
    let code = RwSignal::new(String::new());
    let draft = RwSignal::new(settings.get_untracked().unwrap_or_default());

    let refresh = move || {
        spawn_local(async move {
            match ipc::credential_state().await {
                Ok(list) => statuses.set(list),
                Err(error) => message.set(Some(error.message)),
            }
        });
    };

    let save = move || {
        let value = draft.get_untracked();
        spawn_local(async move {
            match ipc::settings_set(&value).await {
                Ok(saved) => {
                    apply_font_scale(saved.font_scale);
                    settings.set(Some(saved));
                    message.set(Some("Settings saved.".to_owned()));
                }
                Err(error) => message.set(Some(error.message)),
            }
        });
    };

    let status_of = move |provider: &str| {
        statuses.with(|list| {
            list.iter()
                .find(|status| status.provider == provider)
                .cloned()
        })
    };

    view! {
        <section class="settings" aria-label="Settings">
            <div class="settings-header">
                <h1 class="chat-title" tabindex="-1">"Settings"</h1>
                <button type="button" class="button" on:click=move |_| on_close.run(())>
                    <Icon name=IconName::ChevronLeft />
                    <span>"Back to chat"</span>
                </button>
            </div>
            <div class="settings-body">
                <fieldset class="settings-group">
                    <legend>"Model"</legend>
                    <label class="field">
                        <span>"Provider"</span>
                        <select
                            prop:value=move || draft.get().provider
                            on:change=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.provider = value);
                            }
                        >
                            {PROVIDERS
                                .into_iter()
                                .map(|(value, label)| {
                                    view! { <option value=value selected=move || draft.get().provider == value>{label}</option> }
                                })
                                .collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>"Model id"</span>
                        <input
                            type="text"
                            prop:value=move || draft.get().model
                            on:input=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.model = value);
                            }
                        />
                    </label>
                    <label class="field">
                        <span>"Effort"</span>
                        <select
                            prop:value=move || draft.get().effort
                            on:change=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.effort = value);
                            }
                        >
                            {EFFORTS
                                .into_iter()
                                .map(|(value, label)| {
                                    view! { <option value=value selected=move || draft.get().effort == value>{label}</option> }
                                })
                                .collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>"System prompt"</span>
                        <textarea
                            rows="3"
                            prop:value=move || draft.get().system_prompt
                            on:input=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.system_prompt = value);
                            }
                        ></textarea>
                    </label>
                </fieldset>

                <fieldset class="settings-group">
                    <legend>"Accounts"</legend>
                    {PROVIDERS
                        .into_iter()
                        .filter(|(provider, _)| *provider != LOCAL_HUB_PROVIDER)
                        .map(|(provider, label)| {
                            let key_input = RwSignal::new(String::new());
                            let provider_name = provider.to_owned();
                            let remove_name = provider.to_owned();
                            let paste_name = provider.to_owned();
                            let sign_in_name = provider.to_owned();
                            let code_name = provider.to_owned();
                            view! {
                                <div class="account">
                                    <h2 class="account-name">{label}</h2>
                                    <p class="account-status" role="status">
                                        {move || match status_of(provider) {
                                            Some(status) if status.signed_in => {
                                                let account = status
                                                    .account_label
                                                    .clone()
                                                    .unwrap_or_else(|| "this account".to_owned());
                                                format!("Signed in as {account}")
                                            }
                                            Some(status) if status.has_api_key => {
                                                "API key saved".to_owned()
                                            }
                                            _ => "Not connected".to_owned(),
                                        }}
                                    </p>
                                    <div class="account-row">
                                        <label class="field">
                                            <span>{format!("{label} API key")}</span>
                                            <input
                                                type="password"
                                                autocomplete="off"
                                                prop:value=move || key_input.get()
                                                on:input=move |event| key_input.set(event_target_value(&event))
                                            />
                                        </label>
                                        <button
                                            type="button"
                                            class="button"
                                            on:click=move |_| {
                                                let provider = provider_name.clone();
                                                let key = key_input.get_untracked();
                                                spawn_local(async move {
                                                    match ipc::credential_set(&provider, &key).await {
                                                        Ok(_) => {
                                                            key_input.set(String::new());
                                                            message.set(Some("API key saved.".to_owned()));
                                                            refresh();
                                                        }
                                                        Err(error) => message.set(Some(error.message)),
                                                    }
                                                });
                                            }
                                        >
                                            "Save key"
                                        </button>
                                        <button
                                            type="button"
                                            class="button"
                                            on:click=move |_| {
                                                let provider = remove_name.clone();
                                                spawn_local(async move {
                                                    match ipc::credential_delete(&provider).await {
                                                        Ok(_) => {
                                                            message.set(Some("API key removed.".to_owned()));
                                                            refresh();
                                                        }
                                                        Err(error) => message.set(Some(error.message)),
                                                    }
                                                });
                                            }
                                        >
                                            "Remove key"
                                        </button>
                                    </div>
                                    <div class="account-row">
                                        <button
                                            type="button"
                                            class="button button-primary"
                                            on:click=move |_| {
                                                let provider = sign_in_name.clone();
                                                spawn_local(async move {
                                                    match ipc::sign_in(&provider).await {
                                                        Ok(outcome) if outcome.signed_in() => {
                                                            message.set(Some("Signed in.".to_owned()));
                                                            code_for.set(None);
                                                            refresh();
                                                        }
                                                        Ok(outcome) if outcome.needs_code() => {
                                                            code_for.set(Some(provider.clone()));
                                                            sign_in_url.set(outcome.url.clone().unwrap_or_default());
                                                            message.set(Some(
                                                                "Finish the sign-in in the browser, then paste the code shown there."
                                                                    .to_owned(),
                                                            ));
                                                        }
                                                        Ok(_) => message.set(Some("The sign-in did not finish.".to_owned())),
                                                        Err(error) => message.set(Some(error.message)),
                                                    }
                                                });
                                            }
                                        >
                                            "Sign in with subscription"
                                        </button>
                                        <button
                                            type="button"
                                            class="button"
                                            on:click=move |_| {
                                                let provider = paste_name.clone();
                                                spawn_local(async move {
                                                    match ipc::sign_in_paste(&provider).await {
                                                        Ok(outcome) if outcome.needs_code() => {
                                                            code_for.set(Some(provider.clone()));
                                                            sign_in_url.set(outcome.url.clone().unwrap_or_default());
                                                            message.set(Some(
                                                                "Open Claude in the browser, finish the sign-in, then paste the code shown there."
                                                                    .to_owned(),
                                                            ));
                                                        }
                                                        Ok(_) => message.set(Some("The sign-in did not finish.".to_owned())),
                                                        Err(error) => message.set(Some(error.message)),
                                                    }
                                                });
                                            }
                                        >
                                            "Paste code instead"
                                        </button>
                                        <button
                                            type="button"
                                            class="button"
                                            on:click=move |_| {
                                                let provider = code_name.clone();
                                                spawn_local(async move {
                                                    match ipc::sign_out(&provider).await {
                                                        Ok(_) => {
                                                            message.set(Some("Signed out.".to_owned()));
                                                            refresh();
                                                        }
                                                        Err(error) => message.set(Some(error.message)),
                                                    }
                                                });
                                            }
                                        >
                                            "Sign out"
                                        </button>
                                    </div>
                                </div>
                            }
                        })
                        .collect_view()}
                    <Show when=move || !sign_in_url.get().is_empty()>
                        <div class="settings-link">
                            <p>"The browser page for this sign-in:"</p>
                            <a
                                href=move || sign_in_url.get()
                                target="_blank"
                                rel="noreferrer"
                            >
                                "Open the sign-in page"
                            </a>
                            <input type="text" readonly prop:value=move || sign_in_url.get() />
                        </div>
                    </Show>                    <Show when=move || code_for.get().is_some()>
                        <form
                            class="account-row"
                            on:submit=move |event: ev::SubmitEvent| {
                                event.prevent_default();
                                let Some(provider) = code_for.get_untracked() else {
                                    return;
                                };
                                let entered = code.get_untracked();
                                spawn_local(async move {
                                    match ipc::sign_in_code(&provider, &entered).await {
                                        Ok(outcome) if outcome.signed_in() => {
                                            message.set(Some("Signed in.".to_owned()));
                                            code_for.set(None);
                                            code.set(String::new());
                                            refresh();
                                        }
                                        Ok(_) => message.set(Some("The sign-in did not finish.".to_owned())),
                                        Err(error) => message.set(Some(error.message)),
                                    }
                                });
                            }
                        >
                            <label class="field">
                                <span>"Code from the browser"</span>
                                <input
                                    type="text"
                                    autocomplete="off"
                                    prop:value=move || code.get()
                                    on:input=move |event| code.set(event_target_value(&event))
                                />
                            </label>
                            <button type="submit" class="button button-primary">
                                "Finish sign-in"
                            </button>
                        </form>
                    </Show>
                </fieldset>

                <fieldset class="settings-group">
                    <legend>"Appearance"</legend>
                    <label class="field">
                        <span>{move || format!("Text size: {:.0} percent", draft.get().font_scale * 100.0)}</span>
                        <input
                            type="range"
                            min="0.8"
                            max="1.6"
                            step="0.05"
                            prop:value=move || draft.get().font_scale.to_string()
                            on:input=move |event| {
                                if let Ok(value) = event_target_value(&event).parse::<f32>() {
                                    draft.update(|current| current.font_scale = value);
                                    apply_font_scale(value);
                                }
                            }
                        />
                    </label>
                    <label class="field">
                        <span>"Theme"</span>
                        <select
                            prop:value=move || draft.get().theme
                            on:change=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.theme = value);
                            }
                        >
                            {THEMES
                                .into_iter()
                                .map(|(value, label)| {
                                    view! { <option value=value selected=move || draft.get().theme == value>{label}</option> }
                                })
                                .collect_view()}
                        </select>
                    </label>
                    <label class="field">
                        <span>"Tool permission mode"</span>
                        <select
                            prop:value=move || draft.get().permission_mode
                            on:change=move |event| {
                                let value = event_target_value(&event);
                                draft.update(|current| current.permission_mode = value);
                            }
                        >
                            <option value="ask" selected=move || draft.get().permission_mode == "ask">"Ask every time"</option>
                            <option value="allow" selected=move || draft.get().permission_mode == "allow">"Allow without asking"</option>
                            <option value="deny" selected=move || draft.get().permission_mode == "deny">"Never run tools"</option>
                        </select>
                    </label>
                    <p class="settings-note">
                        "The permission mode is stored now and enforced when tools land."
                    </p>
                </fieldset>

                <div class="settings-actions">
                    <button type="button" class="button button-primary" on:click=move |_| save()>
                        "Save settings"
                    </button>
                    {move || message.get().map(|text| view! { <p class="settings-message" role="status">{text}</p> })}
                </div>
            </div>
        </section>
    }
}

pub fn apply_font_scale(scale: f32) {
    if let Some(root) = leptos::prelude::document().document_element() {
        let percent = (scale.clamp(0.8, 1.6) * 100.0).round();
        let _ = root.set_attribute("style", &format!("font-size: {percent}%"));
    }
}
