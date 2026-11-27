use std::{
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub role: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Session {
    id: String,
    title: String,
    created_at: i64,
    updated_at: i64,
    messages: Vec<StoredMessage>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    sessions: Vec<Session>,
}

#[derive(Debug)]
pub enum StoreError {
    Open(String),
    Query(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(message) => {
                write!(formatter, "the session file could not be opened: {message}")
            }
            Self::Query(message) => {
                write!(formatter, "the session file rejected a change: {message}")
            }
        }
    }
}

impl std::error::Error for StoreError {}

pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(directory) = path.parent() {
            std::fs::create_dir_all(directory)
                .map_err(|error| StoreError::Open(error.to_string()))?;
        }
        let data = match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(error) => return Err(StoreError::Open(error.to_string())),
        };
        Ok(Self {
            path: path.to_path_buf(),
            data: Mutex::new(data),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn persist(&self, data: &Data) -> Result<(), StoreError> {
        let text = serde_json::to_string_pretty(data)
            .map_err(|error| StoreError::Query(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|error| StoreError::Query(error.to_string()))?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| StoreError::Query(error.to_string()))
    }

    pub fn new_session_id(&self) -> String {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("s-{}-{count}", now())
    }

    pub fn append_turn(
        &self,
        session: &str,
        title: &str,
        user: &str,
        assistant: &str,
    ) -> Result<(), StoreError> {
        let now = now();
        let mut data = self.lock();
        let index = if let Some(index) = data.sessions.iter().position(|entry| entry.id == session)
        {
            index
        } else {
            data.sessions.push(Session {
                id: session.to_owned(),
                title: String::from(title),
                created_at: now,
                updated_at: now,
                messages: Vec::new(),
            });
            data.sessions.len() - 1
        };
        let entry = &mut data.sessions[index];
        if entry.title.is_empty() {
            entry.title = String::from(title);
        }
        entry.updated_at = now;
        entry.messages.push(StoredMessage {
            role: "user".to_owned(),
            text: user.to_owned(),
        });
        entry.messages.push(StoredMessage {
            role: "assistant".to_owned(),
            text: assistant.to_owned(),
        });
        self.persist(&data)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<SessionSummary>, StoreError> {
        let data = self.lock();
        let mut summaries: Vec<SessionSummary> = data
            .sessions
            .iter()
            .map(|entry| SessionSummary {
                id: entry.id.clone(),
                title: entry.title.clone(),
                updated_at: entry.updated_at,
            })
            .collect();
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        summaries.truncate(limit);
        Ok(summaries)
    }

    pub fn messages(&self, session: &str) -> Result<Vec<StoredMessage>, StoreError> {
        let data = self.lock();
        Ok(data
            .sessions
            .iter()
            .find(|entry| entry.id == session)
            .map_or_else(Vec::new, |entry| entry.messages.clone()))
    }

    pub fn delete(&self, session: &str) -> Result<(), StoreError> {
        let mut data = self.lock();
        data.sessions.retain(|entry| entry.id != session);
        self.persist(&data)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
}
