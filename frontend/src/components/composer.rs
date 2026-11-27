use leptos::{ev, html::Textarea, prelude::*};

use crate::components::icon::{Icon, IconName};

#[component]
pub fn Composer(
    generating: Signal<bool>,
    input: NodeRef<Textarea>,
    on_send: Callback<String>,
    on_stop: Callback<()>,
    on_blocked: Callback<()>,
) -> impl IntoView {
    let draft = RwSignal::new(String::new());
    let is_empty = move || draft.with(|text| text.trim().is_empty());
    let submit = move || {
        if generating.get_untracked() {
            on_blocked.run(());
            return;
        }
        if draft.with_untracked(|text| text.trim().is_empty()) {
            return;
        }
        on_send.run(draft.get_untracked());
        draft.set(String::new());
    };
    let on_keydown = move |event: ev::KeyboardEvent| {
        if event.key() == "Enter" && !event.shift_key() && !event.is_composing() {
            event.prevent_default();
            submit();
        }
    };
    view! {
        <form
            class="composer"
            aria-label="Send a message"
            on:submit=move |event: ev::SubmitEvent| {
                event.prevent_default();
                submit();
            }
        >
            <h2 class="visually-hidden">"Message"</h2>
            <label class="visually-hidden" for="composer-input">
                "Message to Barry"
            </label>
            <textarea
                id="composer-input"
                class="composer-input"
                rows="1"
                autofocus
                aria-describedby="composer-hint"
                node_ref=input
                prop:value=move || draft.get()
                on:input=move |event| draft.set(event_target_value(&event))
                on:keydown=on_keydown
            ></textarea>
            <div class="composer-bar">
                <p class="composer-hint" id="composer-hint">
                    "Enter to send, Shift+Enter for a new line"
                </p>
                <span class="visually-hidden" id="send-reason">
                    "Type a message to send"
                </span>
                {move || {
                    if generating.get() {
                        view! {
                            <button
                                type="button"
                                class="button button-stop"
                                aria-keyshortcuts="Control+Period"
                                on:click=move |_| on_stop.run(())
                            >
                                <Icon name=IconName::Stop />
                                <span>"Stop"</span>
                                <span class="visually-hidden">" generating"</span>
                            </button>
                        }
                            .into_any()
                    } else {
                        view! {
                            <button
                                type="submit"
                                class="button button-primary"
                                aria-disabled=move || if is_empty() { "true" } else { "false" }
                                aria-describedby=move || is_empty().then_some("send-reason")
                            >
                                <Icon name=IconName::Send />
                                <span>"Send"</span>
                                <span class="visually-hidden">" message"</span>
                            </button>
                        }
                            .into_any()
                    }
                }}
            </div>
        </form>
    }
}
