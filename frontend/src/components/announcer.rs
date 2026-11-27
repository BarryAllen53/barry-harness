use std::time::Duration;

use leptos::prelude::*;

use crate::logic::announcer::{Announcer, Urgency};

#[derive(Clone, Copy)]
pub struct Announcements {
    queue: StoredValue<Announcer>,
    region: RwSignal<String>,
}

impl Default for Announcements {
    fn default() -> Self {
        Self {
            queue: StoredValue::new(Announcer::default()),
            region: RwSignal::new(String::new()),
        }
    }
}

impl Announcements {
    pub fn say(self, text: impl Into<String>, urgency: Urgency) {
        let text = text.into();
        self.queue
            .update_value(|queue| queue.submit(text, urgency, now_ms()));
        self.pump();
    }

    fn pump(self) {
        let now = now_ms();
        if let Some(text) = self
            .queue
            .try_update_value(|queue| queue.poll(now))
            .flatten()
        {
            self.region.set(text);
        }
        if let Some(wake) = self.queue.with_value(Announcer::next_wake) {
            set_timeout(
                move || self.pump(),
                Duration::from_millis(wake.saturating_sub(now)),
            );
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn now_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}

#[component]
pub fn AnnouncerRegion(announcements: Announcements) -> impl IntoView {
    view! {
        <div class="visually-hidden" role="status" aria-live="polite" aria-atomic="true">
            {move || announcements.region.get()}
        </div>
    }
}
