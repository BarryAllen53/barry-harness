use std::{
    collections::HashSet,
    fmt,
    io::Read,
    path::Path,
    time::{Duration, SystemTime},
};

use bh_provider::ProviderError;
use reqwest::header::HeaderValue;
use serde::Deserialize;

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
const HUB_FILE_PREFIX: &str = "hub_";
const HUB_FILE_EXTENSION: &str = "json";
const HUB_FILE_LIMIT: u64 = 64 * 1024;
pub(crate) const LANGUAGE_SERVER_IMAGE: &str = "language_server";
const PROCESS_LIST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct Hub {
    port: u16,
    csrf: HeaderValue,
}

impl Hub {
    pub(crate) fn url(&self, method: &str) -> String {
        format!("http://127.0.0.1:{}/{SERVICE}/{method}", self.port)
    }

    pub(crate) fn csrf(&self) -> &HeaderValue {
        &self.csrf
    }
}

impl fmt::Debug for Hub {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Hub")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

pub(crate) struct HubFile {
    pid: Option<u32>,
    port: u16,
    csrf_token: String,
    modified: SystemTime,
}

impl HubFile {
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn hub(&self) -> Option<Hub> {
        let mut csrf = HeaderValue::from_str(&self.csrf_token).ok()?;
        csrf.set_sensitive(true);
        Some(Hub {
            port: self.port,
            csrf,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubRecord {
    pid: Option<u32>,
    http_port: u16,
    csrf_token: String,
}

pub(crate) fn read_hub_files(directory: &Path) -> Vec<HubFile> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(%error, "the antigravity hub directory cannot be read");
            return Vec::new();
        }
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry) => Some(entry.path()),
            Err(error) => {
                tracing::debug!(%error, "an antigravity hub entry cannot be read");
                None
            }
        })
        .filter(|path| is_hub_file(path))
        .filter_map(|path| read_hub_file(&path))
        .collect()
}

fn is_hub_file(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some(HUB_FILE_EXTENSION)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(HUB_FILE_PREFIX))
}

fn read_hub_file(path: &Path) -> Option<HubFile> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let result = std::fs::File::open(path).and_then(|file| {
        let modified = file.metadata()?.modified()?;
        let mut text = String::new();
        file.take(HUB_FILE_LIMIT).read_to_string(&mut text)?;
        Ok((text, modified))
    });
    let (text, modified) = match result {
        Ok(read) => read,
        Err(error) => {
            tracing::debug!(file = name, %error, "an antigravity hub file cannot be read");
            return None;
        }
    };
    match serde_json::from_str::<HubRecord>(&text) {
        Ok(record) if record.http_port != 0 && !record.csrf_token.is_empty() => Some(HubFile {
            pid: record.pid,
            port: record.http_port,
            csrf_token: record.csrf_token,
            modified,
        }),
        Ok(_) => {
            tracing::debug!(file = name, "an antigravity hub file has no port or token");
            None
        }
        Err(error) => {
            tracing::debug!(
                file = name,
                line = error.line(),
                column = error.column(),
                "an antigravity hub file is not readable json"
            );
            None
        }
    }
}

pub(crate) fn rank(mut files: Vec<HubFile>, servers: Option<&HashSet<u32>>) -> Vec<HubFile> {
    files.sort_by_key(|file| std::cmp::Reverse(file.modified));
    let Some(servers) = servers else {
        return files;
    };
    files.retain(|file| file.pid.is_some_and(|pid| servers.contains(&pid)));
    files
}

pub(crate) async fn language_servers(image_prefix: &str) -> Option<HashSet<u32>> {
    let mut command = process_list_command(image_prefix);
    command.kill_on_drop(true);
    let output = match tokio::time::timeout(PROCESS_LIST_TIMEOUT, command.output()).await {
        Ok(Ok(output)) if output.status.success() => output,
        Ok(Ok(output)) => {
            tracing::warn!(status = %output.status, "the process list could not be read");
            return None;
        }
        Ok(Err(error)) => {
            tracing::warn!(%error, "the process list could not be read");
            return None;
        }
        Err(_) => {
            tracing::warn!("the process list took too long to read");
            return None;
        }
    };
    Some(parse_process_list(
        &String::from_utf8_lossy(&output.stdout),
        image_prefix,
    ))
}

#[cfg(windows)]
fn process_list_command(image_prefix: &str) -> tokio::process::Command {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = tokio::process::Command::new("tasklist");
    command
        .args(["/FO", "CSV", "/NH", "/FI"])
        .arg(format!("IMAGENAME eq {image_prefix}*"))
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn process_list_command(_image_prefix: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("ps");
    command.args(["-A", "-o", "pid=,comm="]);
    command
}

#[cfg(windows)]
fn parse_process_list(output: &str, image_prefix: &str) -> HashSet<u32> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.trim().trim_matches('"').split("\",\"");
            let image = fields.next()?;
            let pid = fields.next()?.parse().ok()?;
            is_server_image(image, image_prefix).then_some(pid)
        })
        .collect()
}

#[cfg(not(windows))]
fn parse_process_list(output: &str, image_prefix: &str) -> HashSet<u32> {
    output
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.trim().split_once(char::is_whitespace)?;
            let image = command.trim().rsplit('/').next()?;
            is_server_image(image, image_prefix).then_some(pid.parse().ok()?)
        })
        .collect()
}

fn is_server_image(image: &str, image_prefix: &str) -> bool {
    !image_prefix.is_empty()
        && image
            .get(..image_prefix.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(image_prefix))
}

pub(crate) fn not_running() -> ProviderError {
    ProviderError::Reported {
        code: bh_types::ErrorCode::NetworkUnreachable,
        message: "Antigravity is not running. Open Antigravity and sign in, then send again."
            .to_owned(),
    }
}
