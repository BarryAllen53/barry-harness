#![allow(clippy::too_many_lines)]

use leptos::{prelude::*, task::spawn_local};

use crate::ipc;

const ALL_EFFORTS: [(&str, &str); 5] = [
    ("none", "Off"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra high"),
];

const PERMISSIONS: [(&str, &str); 3] = [
    ("ask", "Ask first"),
    ("allow", "Allow tools"),
    ("deny", "No tools"),
];

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "openai" => "OpenAI",
        "gemini" => "Google",
        "antigravity" => "Antigravity",
        _ => "Anthropic",
    }
}

pub fn persist(settings: RwSignal<Option<ipc::Settings>>, apply: impl FnOnce(&mut ipc::Settings)) {
    let Some(mut current) = settings.get_untracked() else {
        return;
    };
    apply(&mut current);
    settings.set(Some(current.clone()));
    spawn_local(async move {
        if let Ok(saved) = ipc::settings_set(&current).await {
            settings.set(Some(saved));
        }
    });
}

#[component]
pub fn ComposerToolbar(
    settings: RwSignal<Option<ipc::Settings>>,
    models: RwSignal<Vec<ipc::ModelInfo>>,
) -> impl IntoView {
    let selected_model = move || {
        settings.with(|current| {
            current.as_ref().map_or_else(String::new, |settings| {
                format!("{}|{}", settings.provider, settings.model)
            })
        })
    };
    let effort = move || {
        settings.with(|current| {
            current
                .as_ref()
                .map_or_else(String::new, |settings| settings.effort.clone())
        })
    };
    let permission = move || {
        settings.with(|current| {
            current
                .as_ref()
                .map_or_else(String::new, |settings| settings.permission_mode.clone())
        })
    };
    view! {
        <div class="composer-toolbar">
            <label class="toolbar-field">
                <span>"Model"</span>
                <select
                    prop:value=selected_model
                    on:focus=move |_| {
                        spawn_local(async move {
                            if let Ok(list) = ipc::models_list().await {
                                models.set(list);
                            }
                        });
                    }
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        if let Some((provider, model)) = value.split_once('|') {
                            let provider = provider.to_owned();
                            let model = model.to_owned();
                            persist(settings, move |current| {
                                current.provider = provider;
                                current.model = model;
                            });
                        }
                    }
                >
                    {move || {
                        let list = models.get();
                        if list.is_empty() {
                            return view! {
                                <option value="" disabled selected=true>
                                    "Connect a provider in Settings"
                                </option>
                            }
                                .into_any();
                        }
                        list
                            .into_iter()
                            .map(|model| {
                                let value = format!("{}|{}", model.provider, model.id);
                                let label = format!(
                                    "{} / {}",
                                    provider_label(&model.provider),
                                    model.label
                                );
                                view! { <option value=value>{label}</option> }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </select>
            </label>
            <label class="toolbar-field">
                <span>"Effort"</span>
                <select
                    prop:value=effort
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        persist(settings, move |current| current.effort = value);
                    }
                >
                    {move || {
                        let allowed = models.with(|list| {
                            let current = settings.get_untracked();
                            current
                                .and_then(|settings| {
                                    list.iter()
                                        .find(|model| {
                                            model.provider == settings.provider
                                                && model.id == settings.model
                                        })
                                        .map(|model| model.efforts.clone())
                                })
                                .filter(|efforts| !efforts.is_empty())
                                .unwrap_or_else(|| {
                                    ALL_EFFORTS.iter().map(|(value, _)| (*value).to_owned()).collect()
                                })
                        });
                        ALL_EFFORTS
                            .into_iter()
                            .filter(|(value, _)| allowed.iter().any(|item| item == value))
                            .map(|(value, label)| view! { <option value=value>{label}</option> })
                            .collect_view()
                    }}
                </select>
            </label>
            <label class="toolbar-field">
                <span>"Permissions"</span>
                <select
                    prop:value=permission
                    on:change=move |event| {
                        let value = event_target_value(&event);
                        persist(settings, move |current| current.permission_mode = value);
                    }
                >
                    {PERMISSIONS
                        .into_iter()
                        .map(|(value, label)| view! { <option value=value>{label}</option> })
                        .collect_view()}
                </select>
            </label>
        </div>
    }
}

#[component]
pub fn ProjectBar(
    settings: RwSignal<Option<ipc::Settings>>,
    projects: RwSignal<Vec<ipc::ProjectEntry>>,
) -> impl IntoView {
    let refresh = move || {
        spawn_local(async move {
            match ipc::settings_get().await {
                Ok(loaded) => {
                    projects.set(loaded.projects.clone());
                    settings.set(Some(loaded));
                }
                Err(error) => {
                    leptos::logging::warn!("projects could not be loaded: {}", error.message);
                }
            }
        });
    };
    let active = move |id: &str| {
        settings.with(|current| {
            current
                .as_ref()
                .and_then(|settings| settings.active_project.clone())
                .as_deref()
                == Some(id)
        })
    };
    view! {
        <div class="project-bar" aria-label="Projects">
            <span class="toolbar-label">"Project"</span>
            {move || {
                projects
                    .get()
                    .into_iter()
                    .map(|project| {
                        let id = project.id.clone();
                        let selected_id = project.id.clone();
                        let remove_id = project.id.clone();
                        let name = project.name.clone();
                        let label_name = name.clone();
                        view! {
                            <span class="project-chip">
                                <button
                                    type="button"
                                    class="project-name"
                                    aria-pressed=move || if active(&selected_id) { "true" } else { "false" }
                                    on:click=move |_| {
                                        let id = id.clone();
                                        persist(settings, move |current| {
                                            current.active_project = Some(id);
                                        });
                                    }
                                >
                                    {name}
                                </button>
                                <button
                                    type="button"
                                    class="project-remove"
                                    aria-label=format!("Remove project {label_name}")
                                    on:click=move |_| {
                                        let remove_id = remove_id.clone();
                                        spawn_local(async move {
                                            match ipc::project_remove(&remove_id).await {
                                                Ok(_) => refresh(),
                                                Err(error) => leptos::logging::warn!(
                                                    "project could not be removed: {}",
                                                    error.message
                                                ),
                                            }
                                        });
                                    }
                                >
                                    "x"
                                </button>
                            </span>
                        }
                    })
                    .collect_view()
            }}
            <button
                type="button"
                class="button button-quiet"
                on:click=move |_| {
                    spawn_local(async move {
                        match ipc::project_pick().await {
                            Ok(_) => refresh(),
                            Err(error) => leptos::logging::warn!(
                                "project could not be added: {}",
                                error.message
                            ),
                        }
                    });
                }
            >
                "Add project"
            </button>
        </div>
    }
}
