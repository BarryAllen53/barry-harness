use std::collections::VecDeque;

pub const COALESCE_WINDOW_MS: u64 = 400;
pub const MIN_GAP_MS: u64 = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    Lifecycle,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    text: String,
    urgency: Urgency,
    ready_at: u64,
}

#[derive(Debug, Default)]
pub struct Announcer {
    queue: VecDeque<Pending>,
    last_spoken_at: Option<u64>,
    last_text: Option<String>,
    repeats: u32,
}

impl Announcer {
    pub fn submit(&mut self, text: impl Into<String>, urgency: Urgency, now: u64) {
        let text = text.into();
        match urgency {
            Urgency::Lifecycle => {
                self.queue
                    .retain(|pending| pending.urgency == Urgency::Terminal);
                self.queue.push_back(Pending {
                    text,
                    urgency,
                    ready_at: now + COALESCE_WINDOW_MS,
                });
            }
            Urgency::Terminal => {
                self.queue.clear();
                self.queue.push_back(Pending {
                    text,
                    urgency,
                    ready_at: now,
                });
            }
        }
    }

    pub fn next_wake(&self) -> Option<u64> {
        self.queue.front().map(|pending| self.due_at(pending))
    }

    pub fn poll(&mut self, now: u64) -> Option<String> {
        if self.next_wake()? > now {
            return None;
        }
        let pending = self.queue.pop_front()?;
        self.last_spoken_at = Some(now);
        if self.last_text.as_deref() == Some(pending.text.as_str()) {
            self.repeats += 1;
            return Some(format!("{} ({})", pending.text, self.repeats));
        }
        self.repeats = 1;
        self.last_text = Some(pending.text.clone());
        Some(pending.text)
    }

    fn due_at(&self, pending: &Pending) -> u64 {
        match (pending.urgency, self.last_spoken_at) {
            (Urgency::Lifecycle, Some(spoken_at)) => pending.ready_at.max(spoken_at + MIN_GAP_MS),
            _ => pending.ready_at,
        }
    }
}
