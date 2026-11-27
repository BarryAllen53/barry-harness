use bh_types::{
    StopReason,
    ipc::{CloseReason, ErrorEnvelope},
};

use crate::logic::reducer::TurnView;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
    Complete,
    CutOff,
    Refused,
    Stopped,
    Failed(ErrorEnvelope),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalMessage {
    pub paragraphs: Vec<String>,
    pub reasoning: Vec<String>,
    pub ending: Ending,
    pub word_count: usize,
}

impl FinalMessage {
    pub fn assemble(view: &TurnView) -> Option<Self> {
        let ending = match view.outcome()? {
            CloseReason::Completed {
                stop_reason: StopReason::MaxTokens | StopReason::ContextExceeded,
            } => Ending::CutOff,
            CloseReason::Completed {
                stop_reason: StopReason::Refusal,
            } => Ending::Refused,
            CloseReason::Completed { .. } => Ending::Complete,
            CloseReason::Cancelled => Ending::Stopped,
            CloseReason::Failed { error } => Ending::Failed(error.clone()),
        };
        let answer = view.answer_text();
        Some(Self {
            paragraphs: paragraphs(&answer),
            reasoning: paragraphs(&view.reasoning_text()),
            ending,
            word_count: word_count(&answer),
        })
    }

    pub fn announcement(&self) -> String {
        match &self.ending {
            Ending::Complete => format!("Response complete, {}", count_words(self.word_count)),
            Ending::CutOff => format!(
                "Response cut off at the length limit, {}",
                count_words(self.word_count)
            ),
            Ending::Refused => "The model declined to answer".to_owned(),
            Ending::Stopped if self.paragraphs.is_empty() => "Stopped".to_owned(),
            Ending::Stopped => "Stopped. Partial response kept".to_owned(),
            Ending::Failed(error) => format!("Error: {}", error.message),
        }
    }

    pub fn chip(&self) -> Option<&'static str> {
        match self.ending {
            Ending::Complete | Ending::Failed(_) => None,
            Ending::CutOff => Some("Response cut off at the length limit"),
            Ending::Refused => Some("The model declined to answer"),
            Ending::Stopped => Some("Stopped by you"),
        }
    }
}

pub fn paragraphs(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

fn count_words(count: usize) -> String {
    if count == 1 {
        "1 word".to_owned()
    } else {
        format!("{count} words")
    }
}

pub fn reasoning_label(elapsed_ms: Option<u64>) -> String {
    match elapsed_ms {
        None => "Reasoning".to_owned(),
        Some(ms) if ms < 1_000 => "Reasoning, less than a second".to_owned(),
        Some(ms) => {
            let seconds = (ms + 500) / 1_000;
            if seconds == 1 {
                "Reasoning, 1 second".to_owned()
            } else {
                format!("Reasoning, {seconds} seconds")
            }
        }
    }
}
