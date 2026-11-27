#![allow(clippy::too_many_lines)]

use bh_types::ipc::StreamFrame;
use leptos::{ev, html::Textarea, prelude::*, task::spawn_local};

use crate::{
    components::{
        announcer::{Announcements, AnnouncerRegion},
        composer::Composer,
        conversation::{AssistantState, Conversation, Entry, Stamp},
        icon::{Icon, IconName},
        settings::{SettingsView, apply_font_scale},
        theme::ThemePicker,
        toolbar::{ComposerToolbar, ProjectBar},
    },
    ipc,
    logic::{announcer::Urgency, transcript::FinalMessage},
};

#[derive(Clone, Copy)]
struct Chat {
    entries: RwSignal<Vec<Entry>>,
    generating: RwSignal<bool>,
    next_id: StoredValue<u64>,
    announcements: Announcements,
}

impl Chat {
    fn allocate_id(self) -> u64 {
        self.next_id.update_value(|id| *id += 1);
        self.next_id.get_value()
    }

    fn send(self, text: String) {
        if self.generating.get_untracked() {
            return;
        }
        let state = RwSignal::new(AssistantState::default());
        let user = Entry::User {
            id: self.allocate_id(),
            stamp: Stamp::now(),
            text: text.clone(),
        };
        let assistant = Entry::Assistant {
            id: self.allocate_id(),
            stamp: Stamp::now(),
            state,
        };
        self.entries
            .update(|entries| entries.extend([user, assistant]));
        self.generating.set(true);
        self.announcements
            .say("Generating response", Urgency::Lifecycle);
        spawn_local(async move {
            let result = ipc::send_prompt(text, move |frame| self.apply(state, frame)).await;
            if let Err(error) = result {
                let closed = state
                    .try_update(|current| current.view.fail(error))
                    .unwrap_or(false);
                if closed {
                    self.finish(state);
                }
            }
        });
    }

    fn apply(self, state: RwSignal<AssistantState>, frame: StreamFrame) {
        let now = js_sys::Date::now();
        let closed = state
            .try_update(|current| {
                let applied = current.view.apply(frame);
                if applied.reasoning_progressed && current.reasoning_started_at.is_none() {
                    current.reasoning_started_at = Some(now);
                }
                if applied.reasoning_finished {
                    current.reasoning_elapsed_ms = current
                        .reasoning_started_at
                        .map(|started| elapsed_ms(started, now));
                }
                applied.closed
            })
            .unwrap_or(false);
        if closed {
            self.finish(state);
        }
    }

    fn finish(self, state: RwSignal<AssistantState>) {
        let message = state
            .try_update(|current| {
                current.final_message = FinalMessage::assemble(&current.view);
                current.final_message.clone()
            })
            .flatten();
        self.generating.set(false);
        if let Some(message) = message {
            let text = message.announcement();
            let announcements = self.announcements;
            request_animation_frame(move || announcements.say(text, Urgency::Terminal));
        }
    }

    fn stop(self) {
        if !self.generating.get_untracked() {
            return;
        }
        let announcements = self.announcements;
        spawn_local(async move {
            if let Err(error) = ipc::cancel_turn().await {
                announcements.say(format!("Error: {}", error.message), Urgency::Terminal);
            }
        });
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn elapsed_ms(started: f64, now: f64) -> u64 {
    (now - started).max(0.0) as u64
}

#[component]
pub fn App() -> impl IntoView {
    let chat = Chat {
        entries: RwSignal::new(Vec::new()),
        generating: RwSignal::new(false),
        next_id: StoredValue::new(0),
        announcements: Announcements::default(),
    };
    let settings = RwSignal::new(None::<ipc::Settings>);
    let statuses = RwSignal::new(Vec::<ipc::SignInStatus>::new());
    let models = RwSignal::new(Vec::<ipc::ModelInfo>::new());
    let projects = RwSignal::new(Vec::<ipc::ProjectEntry>::new());
    let show_settings = RwSignal::new(false);
    spawn_local(async move {
        match ipc::settings_get().await {
            Ok(loaded) => {
                apply_font_scale(loaded.font_scale);
                projects.set(loaded.projects.clone());
                settings.set(Some(loaded));
            }
            Err(error) => leptos::logging::warn!("settings could not be loaded: {}", error.message),
        }
    });
    spawn_local(async move {
        match ipc::models_list().await {
            Ok(list) => models.set(list),
            Err(error) => leptos::logging::warn!("models could not be listed: {}", error.message),
        }
    });
    let input = NodeRef::<Textarea>::new();
    let focus_input = move || {
        if let Some(element) = input.get_untracked()
            && let Err(error) = element.focus()
        {
            leptos::logging::warn!("could not focus the message box: {error:?}");
        }
    };
    let stop = move || {
        chat.stop();
        focus_input();
    };
    ipc::listen_menu(move |action| match action.as_str() {
        "settings" => show_settings.set(true),
        "new_chat" => {
            show_settings.set(false);
            chat.entries.set(Vec::new());
            spawn_local(async move {
                let _ = ipc::new_chat().await;
            });
            focus_input();
        }
        "about" => chat
            .announcements
            .say("Barry Harness, version 0.1.0", Urgency::Terminal),
        other => leptos::logging::warn!("unknown menu action: {other}"),
    });
    let close_settings = Callback::new(move |()| {
        show_settings.set(false);
        focus_input();
    });
    let shortcut = window_event_listener(ev::keydown, move |event| {
        if event.ctrl_key() && !event.alt_key() && event.key() == "." {
            event.prevent_default();
            stop();
            return;
        }
        if event.ctrl_key() && !event.alt_key() && event.key() == "," {
            event.prevent_default();
            show_settings.update(|open| *open = !*open);
            return;
        }
        if event.key() == "Escape" && show_settings.get_untracked() {
            event.prevent_default();
            show_settings.set(false);
            focus_input();
        }
    });
    on_cleanup(move || shortcut.remove());
    view! {
        <a class="skip-link" href="#conversation">
            "Skip to conversation"
        </a>
        <a class="skip-link" href="#composer-input">
            "Skip to message box"
        </a>
        <header class="app-header">
            <div class="brand">
                <Icon name=IconName::LogoMark />
                <span class="brand-name">"Barry Harness"</span>
            </div>
            <div class="header-actions">
                <button
                    type="button"
                    class="button"
                    aria-keyshortcuts="Control+Comma"
                    on:click=move |_| show_settings.update(|open| *open = !*open)
                >
                    <Icon name=IconName::Settings />
                    <span>"Settings"</span>
                </button>
                <ThemePicker />
            </div>
        </header>
        <main id="main" class="chat" hidden=move || show_settings.get()>
            <h1 class="chat-title" tabindex="-1">
                "New chat"
            </h1>
            <ProjectBar settings=settings projects=projects />
            <Conversation entries=chat.entries />
            <ComposerToolbar settings=settings models=models />
            <Composer
                generating=chat.generating.into()
                input=input
                on_send=Callback::new(move |text| {
                    chat.send(text);
                    focus_input();
                })
                on_stop=Callback::new(move |()| stop())
                on_blocked=Callback::new(move |()| {
                    chat.announcements.say(
                        "Still generating. Press Control+Period to stop.",
                        Urgency::Terminal,
                    );
                    focus_input();
                })
            />
        </main>
        <Show when=move || show_settings.get()>
            <SettingsView settings=settings statuses=statuses on_close=close_settings />
        </Show>
        <AnnouncerRegion announcements=chat.announcements />
    }
}
