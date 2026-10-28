//! The only way `ora-core` talks to Spotify. A trait, so tests script responses without any
//! network, and so secrets stay inside `Secret` until the request is built.

use crate::secret::Secret;
use std::{fmt, future::Future, time::Duration};

/// Larger replies are refused. The biggest real ones (a page of saved albums, each with its
/// tracks) stay well below it.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

pub struct Request {
    pub method: Method,
    /// Never contains a secret: tokens travel in the header or the form body.
    pub url: String,
    pub bearer: Option<Secret>,
    pub form: Vec<(&'static str, Secret)>,
    /// A JSON body (Web API player commands). Never holds a secret.
    pub json: Option<serde_json::Value>,
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
    /// The reply was larger than `MAX_RESPONSE_BYTES`.
    TooLarge,
}

/// Adds a chunk of the reply, unless that would pass `MAX_RESPONSE_BYTES`.
fn append_capped(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), TransportError> {
    if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
        return Err(TransportError::TooLarge);
    }
    body.extend_from_slice(chunk);
    Ok(())
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
            Method::Put => self.client.put(&request.url),
            Method::Delete => self.client.delete(&request.url),
        };
        if let Some(bearer) = &request.bearer {
            builder = builder.bearer_auth(bearer.expose());
        }
        if !request.form.is_empty() {
            let pairs: Vec<(&str, &str)> =
                request.form.iter().map(|(k, v)| (*k, v.expose())).collect();
            builder = builder.form(&pairs);
        }
        if let Some(json) = &request.json {
            builder = builder.json(json);
        } else if request.form.is_empty() && request.method != Method::Get {
            // An explicit empty body sends `Content-Length: 0`; Spotify can refuse a
            // length-less PUT (for example shuffle and repeat, which take no body).
            builder = builder.body(Vec::<u8>::new());
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
            append_capped(&mut body, &chunk)?;
        }
        Ok(Response {
            status,
            retry_after_s,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_past_the_cap_is_refused_as_too_large() {
        let mut body = Vec::new();
        let chunk = vec![0u8; MAX_RESPONSE_BYTES / 2];
        assert_eq!(append_capped(&mut body, &chunk), Ok(()));
        assert_eq!(append_capped(&mut body, &chunk), Ok(()));
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
        assert_eq!(
            append_capped(&mut body, &[0]),
            Err(TransportError::TooLarge)
        );
        assert_eq!(body.len(), MAX_RESPONSE_BYTES);
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
        pub json: Option<serde_json::Value>,
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
                json: request.json.clone(),
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
