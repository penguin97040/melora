//! The one-shot local web server that receives Spotify's redirect after the user approves
//! access. It listens on `127.0.0.1` with an OS-assigned port, accepts only loopback peers,
//! never logs or echoes the request, and stops after the first matching callback.

use crate::secret::Secret;
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MAX_REQUEST_BYTES: usize = 8 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackError {
    TimedOut,
    /// The user declined on Spotify's page (`error=access_denied`).
    Denied,
    /// Spotify reported another error, or the callback had no code.
    Invalid,
    Io,
}

pub struct Callback {
    listener: TcpListener,
    redirect_uri: String,
}

enum Outcome {
    Code(Secret),
    Failed(CallbackError),
    /// Not this sign-in's callback (wrong path or state): keep waiting.
    Ignore,
}

impl Callback {
    pub async fn bind() -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let port = listener.local_addr()?.port();
        Ok(Self {
            listener,
            redirect_uri: format!("http://127.0.0.1:{port}/callback"),
        })
    }

    /// Registered in the Spotify Dashboard without a port, which loopback literals allow.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the callback that carries `state`. Other requests get a short reply and
    /// waiting continues. The listener closes when this returns.
    pub async fn wait(
        self,
        state: &Secret,
        app_name: &str,
        timeout: Duration,
    ) -> Result<Secret, CallbackError> {
        tokio::time::timeout(timeout, async {
            loop {
                let (stream, peer) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|_| CallbackError::Io)?;
                if !peer.ip().is_loopback() {
                    continue;
                }
                match handle(stream, state, app_name).await {
                    Outcome::Code(code) => return Ok(code),
                    Outcome::Failed(error) => return Err(error),
                    Outcome::Ignore => continue,
                }
            }
        })
        .await
        .unwrap_or(Err(CallbackError::TimedOut))
    }
}

async fn handle(mut stream: TcpStream, state: &Secret, app_name: &str) -> Outcome {
    let Ok(Some(target)) = tokio::time::timeout(READ_TIMEOUT, read_target(&mut stream))
        .await
        .unwrap_or(Ok(None))
    else {
        return Outcome::Ignore;
    };
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    if path != "/callback" {
        reply(&mut stream, "404 Not Found", "Not found.").await;
        return Outcome::Ignore;
    }
    let mut code = None;
    let mut error = None;
    let mut state_matches = false;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" => code = Some(Secret::new(value.into_owned())),
            "error" => error = Some(value.into_owned()),
            "state" => state_matches = state.matches(&value),
            _ => {}
        }
    }
    if !state_matches {
        reply(
            &mut stream,
            "400 Bad Request",
            &format!("This sign-in link doesn't match. Return to {app_name} and try again."),
        )
        .await;
        return Outcome::Ignore;
    }
    match (code, error.as_deref()) {
        (_, Some("access_denied")) => {
            reply(
                &mut stream,
                "200 OK",
                &format!("Sign-in cancelled. You can close this tab and return to {app_name}."),
            )
            .await;
            Outcome::Failed(CallbackError::Denied)
        }
        (Some(code), None) if !code.expose().is_empty() => {
            reply(
                &mut stream,
                "200 OK",
                &format!("Signed in. You can close this tab and return to {app_name}."),
            )
            .await;
            Outcome::Code(code)
        }
        _ => {
            reply(
                &mut stream,
                "400 Bad Request",
                &format!(
                    "Spotify didn't complete the sign-in. Return to {app_name} and try again."
                ),
            )
            .await;
            Outcome::Failed(CallbackError::Invalid)
        }
    }
}

/// Reads the request head (capped) and returns the target of a `GET` request line.
async fn read_target(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut head = Vec::with_capacity(1024);
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await?;
        if read == 0 || head.len() + read > MAX_REQUEST_BYTES {
            return Ok(None);
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let Some(line) = head.split(|&b| b == b'\r').next() else {
        return Ok(None);
    };
    let line = String::from_utf8_lossy(line);
    let mut parts = line.split(' ');
    Ok(match (parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(target), Some(version)) if version.starts_with("HTTP/1.") => {
            Some(target.to_owned())
        }
        _ => None,
    })
}

async fn reply(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html lang=\"en-NZ\"><meta charset=\"utf-8\"><title>Spotify sign-in</title><p>{message}</p></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Plays the browser: sends one GET and returns the reply's status line.
    pub(crate) async fn get(redirect_uri: &str, path_and_query: &str) -> std::io::Result<String> {
        let address = redirect_uri
            .trim_start_matches("http://")
            .trim_end_matches("/callback")
            .to_owned();
        let mut stream = TcpStream::connect(address).await?;
        stream
            .write_all(
                format!("GET {path_and_query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut response = String::new();
        stream.read_to_string(&mut response).await?;
        Ok(response.lines().next().unwrap_or_default().to_owned())
    }

    async fn run(requests: Vec<&'static str>) -> (Result<Secret, CallbackError>, Vec<String>) {
        let callback = Callback::bind().await.unwrap();
        let redirect = callback.redirect_uri().to_owned();
        let state = Secret::new("good-state");
        let client = tokio::spawn(async move {
            let mut statuses = Vec::new();
            for request in requests {
                statuses.push(get(&redirect, request).await.unwrap_or_default());
            }
            statuses
        });
        let result = callback
            .wait(&state, "Melora", Duration::from_secs(5))
            .await;
        (result, client.await.unwrap())
    }

    #[tokio::test]
    async fn redirect_uri_uses_the_loopback_literal_and_a_real_port() {
        let callback = Callback::bind().await.unwrap();
        let uri = callback.redirect_uri();
        assert!(uri.starts_with("http://127.0.0.1:"));
        assert!(uri.ends_with("/callback"));
        assert!(!uri.contains("localhost"));
        assert!(!uri.contains(":0/"));
    }

    #[tokio::test]
    async fn favicon_and_wrong_state_are_ignored_until_the_real_callback() {
        let (result, statuses) = run(vec![
            "/favicon.ico",
            "/callback?code=evil&state=other",
            "/callback?code=abc&state=good-state",
        ])
        .await;
        assert_eq!(result.unwrap().expose(), "abc");
        assert!(statuses[0].contains("404"));
        assert!(statuses[1].contains("400"));
        assert!(statuses[2].contains("200"));
    }

    #[tokio::test]
    async fn denied_and_missing_code_fail_cleanly() {
        let (result, _) = run(vec!["/callback?error=access_denied&state=good-state"]).await;
        assert_eq!(result.err(), Some(CallbackError::Denied));
        let (result, _) = run(vec!["/callback?state=good-state"]).await;
        assert_eq!(result.err(), Some(CallbackError::Invalid));
        let (result, _) = run(vec!["/callback?error=server_error&state=good-state"]).await;
        assert_eq!(result.err(), Some(CallbackError::Invalid));
    }

    #[tokio::test]
    async fn duplicate_callback_finds_the_listener_closed() {
        let callback = Callback::bind().await.unwrap();
        let redirect = callback.redirect_uri().to_owned();
        let state = Secret::new("good-state");
        let first = tokio::spawn({
            let redirect = redirect.clone();
            async move { get(&redirect, "/callback?code=abc&state=good-state").await }
        });
        let code = callback
            .wait(&state, "Melora", Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(code.expose(), "abc");
        assert!(first.await.unwrap().unwrap().contains("200"));
        assert!(get(&redirect, "/callback?code=again&state=good-state")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn waiting_times_out() {
        let callback = Callback::bind().await.unwrap();
        let state = Secret::new("good-state");
        let result = callback
            .wait(&state, "Melora", Duration::from_millis(50))
            .await;
        assert_eq!(result.err(), Some(CallbackError::TimedOut));
    }

    #[tokio::test]
    async fn oversized_or_non_get_requests_are_ignored() {
        let long = Box::leak(format!("/callback?pad={}", "x".repeat(9000)).into_boxed_str());
        let callback = Callback::bind().await.unwrap();
        let redirect = callback.redirect_uri().to_owned();
        let state = Secret::new("good-state");
        let client = tokio::spawn(async move {
            let _ = get(&redirect, long).await;
            let address = redirect
                .trim_start_matches("http://")
                .trim_end_matches("/callback")
                .to_owned();
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream
                .write_all(b"POST /callback?code=x&state=good-state HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            drop(stream);
            get(&redirect, "/callback?code=ok&state=good-state").await
        });
        let code = callback
            .wait(&state, "Melora", Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(code.expose(), "ok");
        assert!(client.await.unwrap().unwrap().contains("200"));
    }
}
