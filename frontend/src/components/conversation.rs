use leptos::{html::Div, prelude::*};

use crate::{
    components::icon::{Icon, IconName},
    logic::{
        reducer::TurnView,
        transcript::{Ending, FinalMessage, paragraphs, reasoning_label},
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub label: String,
    pub iso: String,
}

impl Stamp {
    pub fn now() -> Self {
        let date = js_sys::Date::new_0();
        Self {
            label: format!("{:02}:{:02}", date.get_hours(), date.get_minutes()),
            iso: String::from(date.to_iso_string()),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AssistantState {
    pub view: TurnView,
    pub reasoning_started_at: Option<f64>,
    pub reasoning_elapsed_ms: Option<u64>,
    pub final_message: Option<FinalMessage>,
}

#[derive(Clone)]
pub enum Entry {
    User {
        id: u64,
        stamp: Stamp,
        text: String,
    },
    Assistant {
        id: u64,
        stamp: Stamp,
        state: RwSignal<AssistantState>,
    },
}

impl Entry {
    fn id(&self) -> u64 {
        match self {
            Self::User { id, .. } | Self::Assistant { id, .. } => *id,
        }
    }

    fn track(&self) {
        if let Self::Assistant { state, .. } = self {
            state.track();
        }
    }
}

const FOLLOW_THRESHOLD_PX: i32 = 32;

#[component]
pub fn Conversation(entries: RwSignal<Vec<Entry>>) -> impl IntoView {
    let log = NodeRef::<Div>::new();
    let following = RwSignal::new(true);
    let scroll_to_latest = move || {
        if let Some(element) = log.get_untracked() {
            element.set_scroll_top(element.scroll_height());
        }
    };
    Effect::new(move || {
        entries.with(|list| list.last().map(Entry::track));
        if following.get_untracked() {
            request_animation_frame(scroll_to_latest);
        }
    });
    let on_scroll = move |_| {
        if let Some(element) = log.get_untracked() {
            let distance = element.scroll_height() - element.scroll_top() - element.client_height();
            following.set(distance <= FOLLOW_THRESHOLD_PX);
        }
    };
    view! {
        <div
            class="conversation"
            id="conversation"
            role="log"
            aria-live="off"
            aria-label="Conversation: New chat"
            tabindex="-1"
            node_ref=log
            on:scroll=on_scroll
        >
            <h2 class="visually-hidden">"Conversation"</h2>
            <Show when=move || entries.with(Vec::is_empty)>
                <div class="empty-state">
                    <Icon name=IconName::LogoMark />
                    <p class="empty-state-title">"What should we work on?"</p>
                </div>
            </Show>
            <ol class="messages">
                <For each=move || entries.get() key=Entry::id let(entry)>
                    <li>{render_entry(entry)}</li>
                </For>
            </ol>
        </div>
        <Show when=move || !following.get() && entries.with(|list| !list.is_empty())>
            <button
                type="button"
                class="button jump-latest"
                on:click=move |_| {
                    following.set(true);
                    scroll_to_latest();
                }
            >
                <Icon name=IconName::ChevronDown />
                <span>"Jump to latest"</span>
            </button>
        </Show>
    }
}

fn render_entry(entry: Entry) -> AnyView {
    match entry {
        Entry::User { id, stamp, text } => view! {
            <article
                class="message message-user"
                id=format!("message-{id}")
                aria-labelledby=format!("message-{id}-heading")
            >
                <h3 class="visually-hidden" id=format!("message-{id}-heading")>
                    {format!("You, {}", stamp.label)}
                </h3>
                <p class="message-meta" aria-hidden="true">
                    <span class="sender">"You"</span>
                    <time datetime=stamp.iso>{stamp.label.clone()}</time>
                </p>
                <div class="message-body">
                    {paragraphs(&text)
                        .into_iter()
                        .map(|paragraph| view! { <p>{paragraph}</p> })
                        .collect_view()}
                </div>
            </article>
        }
        .into_any(),
        Entry::Assistant { id, stamp, state } => assistant_article(id, stamp, state),
    }
}

fn assistant_article(id: u64, stamp: Stamp, state: RwSignal<AssistantState>) -> AnyView {
    let busy = move || state.with(|current| current.final_message.is_none());
    let has_reasoning = move || state.with(|current| !current.view.reasoning_text().is_empty());
    view! {
        <article
            class="message message-assistant"
            id=format!("message-{id}")
            aria-labelledby=format!("message-{id}-heading")
            aria-busy=move || if busy() { "true" } else { "false" }
        >
            <h3 class="visually-hidden" id=format!("message-{id}-heading")>
                {format!("Barry, {}", stamp.label)}
            </h3>
            <p class="message-meta" aria-hidden="true">
                <Icon name=IconName::LogoMark />
                <span class="sender">"Barry"</span>
                <time datetime=stamp.iso>{stamp.label.clone()}</time>
            </p>
            <Show when=busy>
                <p class="chip chip-running">
                    <Icon name=IconName::Spinner />
                    <span>"Generating"</span>
                </p>
            </Show>
            <Show when=has_reasoning>
                <details class="reasoning">
                    <summary>
                        <Icon name=IconName::Brain />
                        <span>
                            {move || state.with(|current| reasoning_label(current.reasoning_elapsed_ms))}
                        </span>
                    </summary>
                    <Show when=busy>
                        <div class="reasoning-body streaming" aria-hidden="true">
                            {move || state.with(|current| current.view.reasoning_text())}
                        </div>
                    </Show>
                    {move || state.with(final_reasoning)}
                </details>
            </Show>
            <Show when=busy>
                <div class="message-body streaming" aria-hidden="true">
                    {move || state.with(|current| current.view.answer_text())}
                </div>
            </Show>
            {move || state.with(final_answer)}
        </article>
    }
    .into_any()
}

fn final_reasoning(state: &AssistantState) -> Option<AnyView> {
    let message = state.final_message.as_ref()?;
    Some(
        view! {
            <div class="reasoning-body">
                {message
                    .reasoning
                    .iter()
                    .map(|paragraph| view! { <p>{paragraph.clone()}</p> })
                    .collect_view()}
            </div>
        }
        .into_any(),
    )
}

fn final_answer(state: &AssistantState) -> Option<AnyView> {
    let message = state.final_message.as_ref()?;
    let ending = match &message.ending {
        Ending::Failed(error) => Some(
            view! {
                <div class="message-error">
                    <Icon name=IconName::Error />
                    <div>
                        <p class="message-error-title">"Request failed"</p>
                        <p>{error.message.clone()}</p>
                    </div>
                </div>
            }
            .into_any(),
        ),
        _ => message.chip().map(|label| {
            view! {
                <p class="chip">
                    <Icon name=IconName::Stop />
                    <span>{label}</span>
                </p>
            }
            .into_any()
        }),
    };
    Some(
        view! {
            <div class="message-body">
                {message
                    .paragraphs
                    .iter()
                    .map(|paragraph| view! { <p>{paragraph.clone()}</p> })
                    .collect_view()}
            </div>
            {ending}
        }
        .into_any(),
    )
}
