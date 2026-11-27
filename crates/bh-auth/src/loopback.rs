use std::{collections::HashMap, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

use crate::AuthError;

pub struct Loopback {
    listener: TcpListener,
    extra: Option<tokio::sync::mpsc::Receiver<TcpStream>>,
    path: &'static str,
    port: u16,
}

impl Loopback {
    pub async fn bind(port: Option<u16>, path: &'static str) -> Result<Self, AuthError> {
        let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
            .await
            .map_err(|error| AuthError::network(format!("cannot listen on 127.0.0.1: {error}")))?;
        let port = listener
            .local_addr()
            .map_err(|error| AuthError::network(format!("cannot read the bound port: {error}")))?
            .port();
        let extra = TcpListener::bind(("::1", port)).await.ok().map(|six| {
            let (sender, receiver) = tokio::sync::mpsc::channel(4);
            tokio::spawn(async move {
                while let Ok((stream, _)) = six.accept().await {
                    if sender.send(stream).await.is_err() {
                        break;
                    }
                }
            });
            receiver
        });
        Ok(Self {
            listener,
            extra,
            path,
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn accept_code(
        &mut self,
        expected_state: &str,
        window: Duration,
    ) -> Result<(String, TcpStream), AuthError> {
        timeout(window, self.accept(expected_state))
            .await
            .map_err(|_| AuthError::timeout("the sign-in did not finish in time"))?
    }

    async fn accept(&mut self, expected_state: &str) -> Result<(String, TcpStream), AuthError> {
        loop {
            let Self {
                listener,
                extra: extra_slot,
                ..
            } = self;
            let stream = if let Some(extra) = extra_slot.as_mut() {
                tokio::select! {
                    accepted = listener.accept() => {
                        accepted
                            .map_err(|error| AuthError::network(format!("accept failed: {error}")))?
                            .0
                    }
                    received = extra.recv() => received
                        .ok_or_else(|| AuthError::network("the sign-in listener stopped"))?,
                }
            } else {
                listener
                    .accept()
                    .await
                    .map_err(|error| AuthError::network(format!("accept failed: {error}")))?
                    .0
            };
            let mut stream = stream;
            let target = read_request_line(&mut stream).await?;
            let Some((path, query)) = split_target(&target) else {
                respond(&mut stream, 400, "Bad request").await;
                continue;
            };
            if path != self.path {
                respond(&mut stream, 404, "Not found").await;
                continue;
            }
            let params = parse_query(query);
            if let Some(code) = params.get("code") {
                let state = params.get("state").map_or("", String::as_str);
                if state != expected_state {
                    respond(&mut stream, 400, "The sign-in response did not match.").await;
                    return Err(AuthError::state_mismatch());
                }
                return Ok((code.clone(), stream));
            }
            if params.contains_key("error") {
                let detail = params
                    .get("error_description")
                    .or_else(|| params.get("error"))
                    .cloned()
                    .unwrap_or_default();
                respond(&mut stream, 400, "Sign-in was refused.").await;
                return Err(AuthError::provider(format!("sign-in refused: {detail}")));
            }
            respond(&mut stream, 400, "Missing code").await;
        }
    }
}

pub(crate) async fn respond_ok(stream: &mut TcpStream) {
    respond(
        stream,
        200,
        "Sign-in finished. You can close this tab and return to Barry Harness.",
    )
    .await;
}

pub(crate) async fn respond_error(stream: &mut TcpStream, message: &str) {
    let text: String = message
        .chars()
        .map(|character| match character {
            '&' | '<' | '>' => ' ',
            other => other,
        })
        .collect();
    respond(stream, 400, &format!("Sign-in failed: {text}")).await;
}

async fn read_request_line(stream: &mut TcpStream) -> Result<String, AuthError> {
    let mut buffer = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| AuthError::network(format!("read failed: {error}")))?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > 8192 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let line = text.lines().next().unwrap_or_default();
    let mut parts = line.split_whitespace();
    let _method = parts.next();
    let target = parts.next().unwrap_or_default().to_owned();
    Ok(target)
}

fn split_target(target: &str) -> Option<(&str, &str)> {
    if !target.starts_with('/') {
        return None;
    }
    match target.split_once('?') {
        Some((path, query)) => Some((path, query)),
        None => Some((target, "")),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = urlencoding::decode(key).ok()?.into_owned();
            let value = urlencoding::decode(value).ok()?.into_owned();
            Some((key, value))
        })
        .collect()
}

async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Error",
    };
    let page =
        format!("<!doctype html><meta charset=\"utf-8\"><title>Barry Harness</title><p>{body}</p>");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
    let _ = stream.shutdown().await;
}
