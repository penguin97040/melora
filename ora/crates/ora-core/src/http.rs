//! The only way `ora-core` talks to Spotify. A trait, so tests script responses without any
//! network, and so secrets stay inside `Secret` until the request is built.

use crate::secret::Secret;
use std::{fmt, future::Future, time::Duration};

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

pub struct Request {
    pub method: Method,
    /// Never contains a secret: tokens travel in the header or the form body.
    pub url: String,
    pub bearer: Option<Secret>,
    pub form: Vec<(&'static str, Secret)>,
}

impl fmt::Debug for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

pub struct Response {
    pub status: u16,
    /// `Retry-After` in whole seconds, when Spotify sends one.
    pub retry_after_s: Option<u32>,
    /// May hold tokens, so it is never printed.
    pub body: Vec<u8>,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("retry_after_s", &self.retry_after_s)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// No connection could be made (no network, DNS failure, proxy down).
    Offline,
    /// The connection failed part-way, timed out or returned an unreadable reply.
    Network,
}

pub trait HttpClient: Send + Sync + 'static {
    fn send(
        &self,
        request: Request,
    ) -> impl Future<Output = Result<Response, TransportError>> + Send;
}

pub struct ReqwestClient {
    client: reqwest::Client,
}

impl ReqwestClient {
    pub fn new() -> Result<Self, TransportError> {
        // Install ring once for the process; an error only means a provider already exists.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| TransportError::Network)?;
        Ok(Self { client })
    }
}

impl HttpClient for ReqwestClient {
    async fn send(&self, request: Request) -> Result<Response, TransportError> {
        let mut builder = match request.method {
            Method::Get => self.client.get(&request.url),
            Method::Post => self.client.post(&request.url),
        };
        if let Some(bearer) = &request.bearer {
            builder = builder.bearer_auth(bearer.expose());
        }
        if !request.form.is_empty() {
            let pairs: Vec<(&str, &str)> =
                request.form.iter().map(|(k, v)| (*k, v.expose())).collect();
            builder = builder.form(&pairs);
        }
        let mut response = builder.send().await.map_err(|error| {
            if error.is_connect() {
                TransportError::Offline
            } else {
                TransportError::Network
            }
        })?;
        let status = response.status().as_u16();
        let retry_after_s = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u32>().ok());
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TransportError::Network)?
        {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(TransportError::Network);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            status,
            retry_after_s,
            body,
        })
    }
}

#[cfg(test)]
pub mod mock {
    //! A scripted client: each request takes the next queued reply and is recorded.

    use super::*;
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    pub type Reply = Result<Response, TransportError>;

    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: Method,
        pub url: String,
        pub had_bearer: bool,
        /// Form keys and values, for assertions in tests only.
        pub form: Vec<(String, String)>,
    }

    #[derive(Clone, Default)]
    pub struct MockHttp {
        replies: Arc<Mutex<VecDeque<Reply>>>,
        pub seen: Arc<Mutex<Vec<Seen>>>,
        delay: Arc<Mutex<Option<Duration>>>,
    }

    impl MockHttp {
        /// Makes every later request take this long, in every clone.
        pub fn set_delay(&self, delay: Option<Duration>) {
            *self.delay.lock().unwrap() = delay;
        }

        pub fn push(&self, status: u16, body: &str) {
            self.replies.lock().unwrap().push_back(Ok(Response {
                status,
                retry_after_s: None,
                body: body.as_bytes().to_vec(),
            }));
        }

        pub fn push_reply(&self, reply: Reply) {
            self.replies.lock().unwrap().push_back(reply);
        }

        pub fn requests(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl HttpClient for MockHttp {
        async fn send(&self, request: Request) -> Result<Response, TransportError> {
            self.seen.lock().unwrap().push(Seen {
                method: request.method,
                url: request.url.clone(),
                had_bearer: request.bearer.is_some(),
                form: request
                    .form
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), v.expose().to_owned()))
                    .collect(),
            });
            let delay = *self.delay.lock().unwrap();
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            let reply = self.replies.lock().unwrap().pop_front();
            reply.unwrap_or(Err(TransportError::Network))
        }
    }
}
