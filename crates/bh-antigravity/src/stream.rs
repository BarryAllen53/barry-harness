use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    pin::Pin,
    time::Duration,
};

use bh_provider::{ProviderError, ProviderEvent, ProviderStream, TimeoutPhase};
use bh_types::{ContentBlock, ErrorCode, StopReason, TextBlock, Usage};
use futures_util::{Stream, StreamExt, stream};
use serde::Deserialize;
use tokio::time::Instant;
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::envelope::{EnvelopeDecoder, Frame};

const REASONING_INDEX: u32 = 0;
const TEXT_INDEX: u32 = 1;
const STEP_SEPARATOR: &str = "\n\n";
const ERROR_DETAIL_LIMIT: usize = 600;
const FAR_FUTURE: Duration = Duration::from_hours(24 * 365 * 30);
const STATUS_IDLE: &str = "CASCADE_RUN_STATUS_IDLE";
const STATUS_RUNNING: &str = "CASCADE_RUN_STATUS_RUNNING";
const STEP_DONE: &str = "CORTEX_STEP_STATUS_DONE";
const STEP_PLANNER_RESPONSE: &str = "CORTEX_STEP_TYPE_PLANNER_RESPONSE";
const STEP_ERROR_MESSAGE: &str = "CORTEX_STEP_TYPE_ERROR_MESSAGE";
const STEP_USER_INPUT: &str = "CORTEX_STEP_TYPE_USER_INPUT";

pub(crate) type Item = Result<ProviderEvent, ProviderError>;

pub(crate) fn deadline_after(budget: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(budget).unwrap_or(now + FAR_FUTURE)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UpdateMessage {
    update: Option<AgentStateUpdate>,
    error: Option<ConnectError>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ConnectError {
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AgentStateUpdate {
    status: Option<String>,
    main_trajectory_update: Option<TrajectoryUpdate>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TrajectoryUpdate {
    steps_update: Option<StepsUpdate>,
    generator_metadatas_update: Option<GeneratorMetadatasUpdate>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct StepsUpdate {
    indices: Vec<u32>,
    steps: Vec<Step>,
    total_length: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Step {
    #[serde(rename = "type")]
    kind: String,
    status: String,
    planner_response: Option<PlannerResponse>,
    error_message: Option<ErrorStep>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PlannerResponse {
    response: String,
    thinking: String,
    stop_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ErrorStep {
    error: StepError,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct StepError {
    user_error_message: Option<String>,
    model_error_message: Option<String>,
    short_error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct GeneratorMetadatasUpdate {
    indices: Vec<u32>,
    generator_metadatas: Vec<GeneratorMetadata>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct GeneratorMetadata {
    chat_model: Option<ChatModel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatModel {
    usage: Option<ModelUsage>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModelUsage {
    input_tokens: Option<String>,
    output_tokens: Option<String>,
}

pub(crate) enum Decoded {
    Update(Box<AgentStateUpdate>),
    Unreadable,
    End(Option<ProviderError>),
}

pub(crate) fn decode(frame: &Frame) -> Decoded {
    let Ok(message) = serde_json::from_slice::<UpdateMessage>(&frame.payload) else {
        return if frame.end_stream {
            Decoded::End(None)
        } else {
            Decoded::Unreadable
        };
    };
    if frame.end_stream || message.error.is_some() {
        return Decoded::End(message.error.map(|error| {
            ProviderError::Stream {
                kind: error.code.unwrap_or_else(|| "unknown".to_owned()),
                message: error
                    .message
                    .unwrap_or_else(|| "the hub closed the stream".to_owned()),
            }
        }));
    }
    message.update.map_or(Decoded::Unreadable, |update| {
        Decoded::Update(Box::new(update))
    })
}

#[derive(Debug, Default)]
struct StepText {
    response: String,
    thinking: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Continue,
    Finished,
}

#[derive(Debug, Default)]
pub(crate) struct Turn {
    baseline: Option<u32>,
    running: bool,
    steps: BTreeMap<u32, StepText>,
    replied: bool,
    stop: Option<String>,
    usage: BTreeMap<u32, Usage>,
    unsupported: HashSet<String>,
}

impl Turn {
    pub(crate) fn is_subscribed(&self) -> bool {
        self.baseline.is_some()
    }

    pub(crate) fn apply(
        &mut self,
        update: &AgentStateUpdate,
        out: &mut VecDeque<Item>,
    ) -> Progress {
        let trajectory = update.main_trajectory_update.as_ref();
        let steps = trajectory.and_then(|trajectory| trajectory.steps_update.as_ref());
        let Some(baseline) = self.baseline else {
            self.baseline = Some(steps.and_then(|steps| steps.total_length).unwrap_or(0));
            return Progress::Continue;
        };
        if update.status.as_deref() == Some(STATUS_RUNNING) {
            self.running = true;
        }
        if let Some(steps) = steps {
            for (index, step) in steps.indices.iter().zip(&steps.steps) {
                if *index < baseline {
                    continue;
                }
                self.running = true;
                if self.apply_step(*index, step, out) == Progress::Finished {
                    return Progress::Finished;
                }
            }
        }
        if let Some(metadata) =
            trajectory.and_then(|trajectory| trajectory.generator_metadatas_update.as_ref())
        {
            self.record_usage(metadata);
        }
        if self.running && update.status.as_deref() == Some(STATUS_IDLE) {
            self.finish(out);
            return Progress::Finished;
        }
        Progress::Continue
    }

    fn apply_step(&mut self, index: u32, step: &Step, out: &mut VecDeque<Item>) -> Progress {
        match step.kind.as_str() {
            STEP_PLANNER_RESPONSE => {
                if let Some(planner) = &step.planner_response {
                    self.apply_planner(index, planner, out);
                    if step.status == STEP_DONE {
                        self.replied = true;
                        self.stop.clone_from(&planner.stop_reason);
                    }
                }
                Progress::Continue
            }
            STEP_ERROR_MESSAGE => {
                let unexplained = StepError::default();
                let error = step
                    .error_message
                    .as_ref()
                    .map_or(&unexplained, |message| &message.error);
                out.push_back(Err(reported_error(error)));
                Progress::Finished
            }
            STEP_USER_INPUT => Progress::Continue,
            other => {
                if self.unsupported.insert(other.to_owned()) {
                    out.push_back(Ok(ProviderEvent::Warning {
                        code: "unsupported_step".to_owned(),
                        message: format!("antigravity produced a {other} step, which is ignored"),
                    }));
                }
                Progress::Continue
            }
        }
    }

    fn apply_planner(&mut self, index: u32, planner: &PlannerResponse, out: &mut VecDeque<Item>) {
        let earlier_thinking = self.other_step_has(index, |step| &step.thinking);
        let earlier_text = self.other_step_has(index, |step| &step.response);
        let seen = self.steps.entry(index).or_default();
        if let Some(delta) = advance(&mut seen.thinking, &planner.thinking, out) {
            let separated = earlier_thinking && seen.thinking.len() == delta.len();
            emit(&delta, separated, out, |text| {
                ProviderEvent::ReasoningSummaryDelta {
                    index: REASONING_INDEX,
                    text,
                }
            });
        }
        if let Some(delta) = advance(&mut seen.response, &planner.response, out) {
            let separated = earlier_text && seen.response.len() == delta.len();
            emit(&delta, separated, out, |text| ProviderEvent::TextDelta {
                index: TEXT_INDEX,
                text,
            });
        }
    }

    fn other_step_has(&self, index: u32, field: impl Fn(&StepText) -> &String) -> bool {
        self.steps
            .iter()
            .any(|(other, step)| *other != index && !field(step).is_empty())
    }
    fn record_usage(&mut self, metadata: &GeneratorMetadatasUpdate) {
        for (index, generator) in metadata.indices.iter().zip(&metadata.generator_metadatas) {
            let Some(usage) = generator
                .chat_model
                .as_ref()
                .and_then(|model| model.usage.as_ref())
            else {
                continue;
            };
            let tokens = |value: &Option<String>| {
                value
                    .as_deref()
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(0)
            };
            self.usage.insert(
                *index,
                Usage {
                    input_tokens: tokens(&usage.input_tokens),
                    output_tokens: tokens(&usage.output_tokens),
                    ..Usage::default()
                },
            );
        }
    }

    fn total_usage(&self) -> Usage {
        self.usage
            .values()
            .fold(Usage::default(), |total, usage| Usage {
                input_tokens: total.input_tokens.saturating_add(usage.input_tokens),
                output_tokens: total.output_tokens.saturating_add(usage.output_tokens),
                ..total
            })
    }

    fn reply_text(&self) -> String {
        self.steps
            .values()
            .map(|step| step.response.as_str())
            .filter(|response| !response.is_empty())
            .collect::<Vec<_>>()
            .join(STEP_SEPARATOR)
    }

    fn finish(&mut self, out: &mut VecDeque<Item>) {
        let text = self.reply_text();
        if !self.replied && text.is_empty() {
            out.push_back(Err(ProviderError::Protocol {
                message: "antigravity finished the run without a reply".to_owned(),
            }));
            return;
        }
        let stop_reason = match self.stop.as_deref() {
            Some(reason) if reason.contains("MAX_TOKENS") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };
        out.push_back(Ok(ProviderEvent::BlockDone {
            index: TEXT_INDEX,
            block: ContentBlock::Text(TextBlock { text }),
        }));
        out.push_back(Ok(ProviderEvent::MessageStop {
            stop_reason,
            raw_stop: self.stop.take(),
            usage: self.total_usage(),
        }));
    }
}

fn advance(seen: &mut String, current: &str, out: &mut VecDeque<Item>) -> Option<String> {
    if let Some(delta) = current.strip_prefix(seen.as_str()) {
        if delta.is_empty() {
            return None;
        }
        let delta = delta.to_owned();
        seen.push_str(&delta);
        return Some(delta);
    }
    if !current.is_empty() {
        out.push_back(Ok(ProviderEvent::Warning {
            code: "reply_rewritten".to_owned(),
            message: "antigravity rewrote text it had already sent; the shown text may differ"
                .to_owned(),
        }));
        current.clone_into(seen);
    }
    None
}

fn emit(
    delta: &str,
    separated: bool,
    out: &mut VecDeque<Item>,
    event: impl Fn(String) -> ProviderEvent,
) {
    let mut text = String::with_capacity(delta.len() + STEP_SEPARATOR.len());
    if separated {
        text.push_str(STEP_SEPARATOR);
    }
    text.push_str(delta);
    out.push_back(Ok(event(text)));
}

pub(crate) fn reported_error(error: &StepError) -> ProviderError {
    let detail = [
        &error.short_error,
        &error.model_error_message,
        &error.user_error_message,
    ]
    .into_iter()
    .flatten()
    .map(|text| text.trim())
    .find(|text| !text.is_empty())
    .map_or_else(
        || "the run stopped without a reason".to_owned(),
        |text| truncate(text, ERROR_DETAIL_LIMIT),
    );
    let lower = detail.to_ascii_lowercase();
    let mentions = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    let (code, message) = if mentions(&[
        "model not found",
        "plan model not specified",
        "not_found",
        "requested entity was not found",
    ]) {
        (
            ErrorCode::ModelUnavailable,
            format!("Antigravity cannot run the selected model ({detail}). Pick another model."),
        )
    } else if mentions(&["resource_exhausted", "quota"]) {
        (
            ErrorCode::QuotaExceeded,
            format!("Antigravity has no quota left for this model ({detail})."),
        )
    } else {
        (
            ErrorCode::ProviderProtocol,
            format!("Antigravity reported an error: {detail}"),
        )
    };
    ProviderError::Reported { code, message }
}

fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

pub(crate) struct Updates<S> {
    chunks: Pin<Box<S>>,
    decoder: EnvelopeDecoder,
    frames: VecDeque<Frame>,
}

impl<S, B> Updates<S>
where
    S: Stream<Item = Result<B, ProviderError>>,
    B: AsRef<[u8]>,
{
    pub(crate) fn new(chunks: S) -> Self {
        Self {
            chunks: Box::pin(chunks),
            decoder: EnvelopeDecoder::default(),
            frames: VecDeque::new(),
        }
    }

    pub(crate) async fn next_frame(&mut self) -> Result<Option<Frame>, ProviderError> {
        loop {
            if let Some(frame) = self.frames.pop_front() {
                return Ok(Some(frame));
            }
            let Some(chunk) = self.chunks.next().await else {
                return Ok(None);
            };
            let frames =
                self.decoder
                    .feed(chunk?.as_ref())
                    .map_err(|error| ProviderError::Protocol {
                        message: error.to_string(),
                    })?;
            self.frames.extend(frames);
        }
    }
}

pub(crate) async fn await_snapshot<S, B>(
    updates: &mut Updates<S>,
    turn: &mut Turn,
    cancel: &CancellationToken,
    deadline: Instant,
) -> Result<(), ProviderError>
where
    S: Stream<Item = Result<B, ProviderError>>,
    B: AsRef<[u8]>,
{
    let mut ignored = VecDeque::new();
    while !turn.is_subscribed() {
        let frame = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(ProviderError::Cancelled),
            next = tokio::time::timeout_at(deadline, updates.next_frame()) => {
                next.unwrap_or(Err(ProviderError::Timeout { phase: TimeoutPhase::FirstEvent }))
            }
        }?;
        let Some(frame) = frame else {
            return Err(ProviderError::TruncatedStream);
        };
        match decode(&frame) {
            Decoded::Update(update) => {
                turn.apply(&update, &mut ignored);
            }
            Decoded::Unreadable => {}
            Decoded::End(error) => return Err(error.unwrap_or(ProviderError::TruncatedStream)),
        }
    }
    Ok(())
}

pub(crate) struct CascadeGuard {
    completed: CancellationToken,
    _abandoned: DropGuard,
}

impl CascadeGuard {
    pub(crate) fn new(completed: CancellationToken, abandoned: CancellationToken) -> Self {
        Self {
            completed,
            _abandoned: abandoned.drop_guard(),
        }
    }

    fn complete(&self) {
        self.completed.cancel();
    }
}

pub(crate) struct Pump<S> {
    pub(crate) updates: Option<Updates<S>>,
    pub(crate) turn: Turn,
    pub(crate) pending: VecDeque<Item>,
    pub(crate) cancel: CancellationToken,
    pub(crate) deadline: Instant,
    pub(crate) idle: Duration,
    pub(crate) guard: CascadeGuard,
}

pub(crate) fn provider_events<S, B>(pump: Pump<S>) -> ProviderStream
where
    S: Stream<Item = Result<B, ProviderError>> + Send + 'static,
    B: AsRef<[u8]> + Send + 'static,
{
    stream::unfold(pump, |mut pump| async move {
        let item = pump.next_item().await?;
        Some((item, pump))
    })
    .boxed()
}

impl<S, B> Pump<S>
where
    S: Stream<Item = Result<B, ProviderError>>,
    B: AsRef<[u8]>,
{
    async fn next_item(&mut self) -> Option<Item> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Some(item);
            }
            if self.updates.is_some() && self.cancel.is_cancelled() {
                self.updates = None;
                return Some(Err(ProviderError::Cancelled));
            }
            let updates = self.updates.as_mut()?;
            let phase = if self.turn.running {
                TimeoutPhase::Idle
            } else {
                TimeoutPhase::FirstEvent
            };
            let next = tokio::select! {
                biased;
                () = self.cancel.cancelled() => Err(ProviderError::Cancelled),
                next = tokio::time::timeout_at(self.deadline, updates.next_frame()) => {
                    next.unwrap_or(Err(ProviderError::Timeout { phase }))
                }
            };
            match next {
                Err(error) => {
                    self.updates = None;
                    return Some(Err(error));
                }
                Ok(None) => {
                    self.updates = None;
                    return Some(Err(ProviderError::TruncatedStream));
                }
                Ok(Some(frame)) => self.consume(&frame),
            }
        }
    }

    fn consume(&mut self, frame: &Frame) {
        match decode(frame) {
            Decoded::Update(update) => {
                if self.turn.apply(&update, &mut self.pending) == Progress::Finished {
                    self.guard.complete();
                    self.updates = None;
                    return;
                }
            }
            Decoded::Unreadable => self.pending.push_back(Ok(ProviderEvent::Warning {
                code: "unreadable_update".to_owned(),
                message: "antigravity sent an update the adapter cannot read".to_owned(),
            })),
            Decoded::End(error) => {
                self.pending
                    .push_back(Err(error.unwrap_or(ProviderError::TruncatedStream)));
                self.updates = None;
                return;
            }
        }
        if self.turn.running {
            self.deadline = deadline_after(self.idle);
        }
    }
}
