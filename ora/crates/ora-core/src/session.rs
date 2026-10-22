//! The account session: sign-in, token refresh, profile, sign-out and the Client ID setting.
//!
//! Rules:
//! - The refresh token lives only in the credential store and in memory; the access token only
//!   in memory. Neither is ever part of `Status`.
//! - A generation counter makes sign-out and cancel win over anything still in flight: late
//!   results from an older generation are discarded (and never persisted).
//! - Credential-store writes and deletions take turns behind one gate, and a write first checks
//!   the generation, so a sign-out's deletion always lands after any write that beat it.
//! - The store never holds more than the UI admits to: a session-only sign-in leaves it empty.
//! - No lock is held across network or credential-store I/O.

use crate::{
    api::{self, ApiError},
    browser::{self, Browser},
    config::{ClientId, ConfigStore},
    http::HttpClient,
    loopback::{registered_redirect_uri, Callback, CallbackError},
    oauth::{self, TokenError},
    pkce, scopes_for,
    secret::Secret,
    store::{CredentialStore, StoreError, REFRESH_TOKEN, SESSION},
};
use ora_types::{AuthState, Feature, OraError, Profile, SignInFailure, Status};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

/// Refresh this long before Spotify's expiry so a request never carries a dying token.
const REFRESH_MARGIN: Duration = Duration::from_secs(120);
/// Phase 2 asks only for what sign-in and the profile need; later slices ask for more.
const SIGN_IN_FEATURES: [Feature; 2] = [Feature::SignIn, Feature::Profile];

pub struct Options {
    /// Shown on the callback page, for example "Melora".
    pub app_name: &'static str,
    pub settings_dir: PathBuf,
    /// A development override for the Client ID, for example `MELORA_SPOTIFY_CLIENT_ID`.
    pub client_id_variable: &'static str,
    pub sign_in_timeout: Duration,
    /// The loopback port for the sign-in callback; register the matching URI exactly
    /// (`loopback::registered_redirect_uri`). 0 picks a free port each time.
    pub callback_port: u16,
}

struct Session {
    refresh: Secret,
    access: Option<(Secret, Instant)>,
    profile: Option<Profile>,
    session_only: bool,
}

enum Auth {
    NotConfigured,
    SignedOut,
    SigningIn,
    SignedIn(Session),
    SessionExpired,
}

struct Inner {
    client_id: Option<ClientId>,
    from_environment: bool,
    auth: Auth,
    /// What to return to if the current sign-in is cancelled or fails.
    before_sign_in: Option<Auth>,
    last_error: Option<OraError>,
    /// Shown in the UI so the Dashboard entry always matches what is sent.
    redirect_uri: String,
}

pub struct Ora<H: HttpClient, S: CredentialStore, B: Browser> {
    http: H,
    store: Arc<S>,
    browser: B,
    config: ConfigStore,
    app_name: &'static str,
    sign_in_timeout: Duration,
    callback_port: u16,
    inner: Mutex<Inner>,
    /// Bumped (while holding `inner`) by sign-out, cancel and Client ID changes.
    generation: watch::Sender<u64>,
    /// Serialises refreshes so concurrent callers share one token request.
    refresh_gate: tokio::sync::Mutex<()>,
    /// Serialises credential-store writes and deletions (see the module rules).
    store_gate: tokio::sync::Mutex<()>,
    status: watch::Sender<Status>,
}

impl<H: HttpClient, S: CredentialStore, B: Browser> Ora<H, S, B> {
    pub fn new(http: H, store: S, browser: B, options: Options) -> Self {
        let config = ConfigStore::new(options.settings_dir, options.client_id_variable);
        let loaded = config.load();
        let inner = Inner {
            auth: if loaded.client_id.is_some() {
                Auth::SignedOut
            } else {
                Auth::NotConfigured
            },
            client_id: loaded.client_id,
            from_environment: loaded.from_environment,
            before_sign_in: None,
            last_error: loaded.error,
            redirect_uri: registered_redirect_uri(options.callback_port),
        };
        let status = watch::Sender::new(snapshot(&inner));
        Self {
            http,
            store: Arc::new(store),
            browser,
            config,
            app_name: options.app_name,
            sign_in_timeout: options.sign_in_timeout,
            callback_port: options.callback_port,
            inner: Mutex::new(inner),
            generation: watch::Sender::new(0),
            refresh_gate: tokio::sync::Mutex::new(()),
            store_gate: tokio::sync::Mutex::new(()),
            status,
        }
    }

    pub fn status(&self) -> Status {
        self.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock can't leave secrets half-written in a way that
        // matters here, so recover the guard instead of poisoning every later call.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies a change and publishes the resulting status.
    fn update<T>(&self, change: impl FnOnce(&mut Inner, &Self) -> T) -> T {
        let mut inner = self.lock();
        let result = change(&mut inner, self);
        self.status.send_replace(snapshot(&inner));
        result
    }

    fn current_generation(&self) -> u64 {
        *self.generation.borrow()
    }

    fn bump_generation(&self) {
        self.generation.send_modify(|g| *g += 1);
    }

    async fn store_call<T: Send + 'static>(
        &self,
        call: impl FnOnce(&S) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let store = Arc::clone(&self.store);
        tokio::task::spawn_blocking(move || call(&store))
            .await
            .unwrap_or(Err(StoreError::Failed))
    }

    /// Runs a store write only while `generation` is current; `None` means it was skipped.
    async fn store_if_current<T: Send + 'static>(
        &self,
        generation: u64,
        call: impl FnOnce(&S) -> Result<T, StoreError> + Send + 'static,
    ) -> Option<Result<T, StoreError>> {
        let _gate = self.store_gate.lock().await;
        if self.current_generation() != generation {
            return None;
        }
        Some(self.store_call(call).await)
    }

    /// At startup: pick up a saved sign-in, then fetch the profile. A sign-in saved for a
    /// different Client ID (another copy, or a development override) is left alone: refreshing
    /// it here would fail and delete it for the copy it belongs to.
    pub async fn restore(&self) {
        let generation = self.current_generation();
        let client_id = {
            let inner = self.lock();
            match (&inner.auth, &inner.client_id) {
                (Auth::SignedOut, Some(id)) => id.clone(),
                _ => return,
            }
        };
        let saved = self
            .store_call(|store| Ok((store.get(REFRESH_TOKEN)?, store.get(SESSION)?)))
            .await;
        match saved {
            Ok((Some(_), Some(metadata))) if !issued_to(&metadata, &client_id) => {}
            Ok((Some(refresh), _)) => {
                let restored = self.update(|inner, ora| {
                    let current = ora.current_generation() == generation
                        && matches!(inner.auth, Auth::SignedOut);
                    if current {
                        inner.auth = Auth::SignedIn(Session {
                            refresh,
                            access: None,
                            profile: None,
                            session_only: false,
                        });
                    }
                    current
                });
                if restored {
                    let _ = self.load_profile().await;
                }
            }
            Ok((None, _)) => {}
            Err(_) => self.update(|inner, _| inner.last_error = Some(OraError::StorageUnavailable)),
        }
    }

    pub async fn sign_in(&self) -> Result<Status, OraError> {
        let started = self.update(|inner, ora| {
            let client_id = inner.client_id.clone().ok_or(OraError::NotConfigured)?;
            if matches!(inner.auth, Auth::SigningIn) {
                // One sign-in at a time. Its browser tab is already open, and a fixed callback
                // port can't be bound twice; Cancel ends it.
                return Ok(None);
            }
            let previous = std::mem::replace(&mut inner.auth, Auth::SigningIn);
            inner.before_sign_in = Some(previous);
            inner.last_error = None;
            Ok::<_, OraError>(Some((client_id, ora.current_generation())))
        })?;
        let Some((client_id, generation)) = started else {
            return Ok(self.status());
        };
        match self.sign_in_flow(&client_id, generation).await {
            Ok(()) => {
                let _ = self.load_profile().await;
                Ok(self.status())
            }
            Err(error) => {
                self.update(|inner, ora| {
                    if ora.current_generation() == generation {
                        inner.auth = inner.before_sign_in.take().unwrap_or(Auth::SignedOut);
                        if error != OraError::Cancelled {
                            inner.last_error = Some(error.clone());
                        }
                    }
                });
                Err(error)
            }
        }
    }

    async fn sign_in_flow(&self, client_id: &ClientId, generation: u64) -> Result<(), OraError> {
        let pkce = pkce::generate().map_err(|_| OraError::Unsupported)?;
        let state = pkce::state().map_err(|_| OraError::Unsupported)?;
        let callback =
            Callback::bind(self.callback_port)
                .await
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied => {
                        OraError::SignIn {
                            reason: SignInFailure::CallbackPortBusy,
                        }
                    }
                    _ => OraError::Unsupported,
                })?;
        let redirect_uri = callback.redirect_uri().to_owned();
        let url = oauth::authorize_url(
            client_id,
            &redirect_uri,
            &scopes_for(&SIGN_IN_FEATURES),
            &state,
            &pkce.challenge,
        );
        if !browser::is_authorize_url(&url) {
            return Err(OraError::Unsupported);
        }
        self.browser.open(&url).map_err(|_| OraError::SignIn {
            reason: SignInFailure::BrowserUnavailable,
        })?;

        let mut generations = self.generation.subscribe();
        let code = tokio::select! {
            result = callback.wait(&state, self.app_name, self.sign_in_timeout) => {
                result.map_err(|error| match error {
                    CallbackError::Denied => OraError::Cancelled,
                    CallbackError::TimedOut => OraError::SignIn { reason: SignInFailure::TimedOut },
                    CallbackError::Invalid | CallbackError::Io => {
                        OraError::SignIn { reason: SignInFailure::InvalidCallback }
                    }
                })?
            }
            _ = generations.wait_for(|g| *g != generation) => return Err(OraError::Cancelled),
        };

        let tokens =
            oauth::exchange_code(&self.http, client_id, code, &redirect_uri, pkce.verifier)
                .await
                .map_err(|error| match error {
                    TokenError::InvalidClient => OraError::SignIn {
                        reason: SignInFailure::InvalidClient,
                    },
                    TokenError::Transport(error) => api::transport_error(error),
                    TokenError::RateLimited(retry_after_s) => {
                        OraError::RateLimited { retry_after_s }
                    }
                    _ => OraError::SignIn {
                        reason: SignInFailure::ExchangeRejected,
                    },
                })?;
        let refresh = tokens.refresh.ok_or(OraError::SignIn {
            reason: SignInFailure::ExchangeRejected,
        })?;
        // Persist before reporting success. Without a usable store, continue for this
        // session only, say so, and leave nothing half-saved behind.
        let metadata = Secret::new(session_metadata(&tokens.scope, client_id));
        let saved = {
            let refresh = refresh.clone();
            self.store_if_current(generation, move |store| {
                store.set(REFRESH_TOKEN, &refresh)?;
                store.set(SESSION, &metadata)
            })
            .await
            .ok_or(OraError::Cancelled)?
        };
        if saved.is_err() {
            let _ = self.forget_stored_if_current(generation).await;
        }
        let accepted = self.update(|inner, ora| {
            if ora.current_generation() != generation {
                return false;
            }
            inner.before_sign_in = None;
            inner.last_error = saved.err().map(|_| OraError::StorageUnavailable);
            inner.auth = Auth::SignedIn(Session {
                refresh,
                access: Some((tokens.access, tokens.expires_at)),
                profile: None,
                session_only: saved.is_err(),
            });
            true
        });
        if !accepted {
            // Signed out while saving: make sure nothing stays behind.
            let _ = self.forget_stored().await;
            return Err(OraError::Cancelled);
        }
        Ok(())
    }

    pub fn cancel_sign_in(&self) -> Status {
        self.update(|inner, ora| {
            if matches!(inner.auth, Auth::SigningIn) {
                ora.bump_generation();
                inner.auth = inner.before_sign_in.take().unwrap_or(Auth::SignedOut);
            }
        });
        self.status()
    }

    /// Clears memory first, then the credential store, and reports a failed deletion.
    pub async fn sign_out(&self) -> Result<Status, OraError> {
        self.update(|inner, ora| {
            ora.bump_generation();
            inner.before_sign_in = None;
            inner.last_error = None;
            inner.auth = if inner.client_id.is_some() {
                Auth::SignedOut
            } else {
                Auth::NotConfigured
            };
        });
        match self.forget_stored().await {
            Ok(()) => Ok(self.status()),
            Err(_) => {
                self.update(|inner, _| inner.last_error = Some(OraError::StorageUnavailable));
                Err(OraError::StorageUnavailable)
            }
        }
    }

    async fn forget_stored(&self) -> Result<(), StoreError> {
        let _gate = self.store_gate.lock().await;
        self.store_call(forget).await
    }

    /// For cleanup after a failed save: a newer sign-in's entries are never touched.
    async fn forget_stored_if_current(&self, generation: u64) -> Option<Result<(), StoreError>> {
        self.store_if_current(generation, forget).await
    }

    pub async fn set_client_id(&self, value: &str) -> Result<Status, OraError> {
        if self.lock().from_environment {
            return Err(OraError::Unsupported);
        }
        let id = ClientId::parse(value)?;
        self.config.save(&id)?;
        let replaced_session = self.update(|inner, _| {
            let changed = inner.client_id.as_ref() != Some(&id);
            inner.client_id = Some(id);
            inner.last_error = None;
            if matches!(inner.auth, Auth::NotConfigured) {
                inner.auth = Auth::SignedOut;
            }
            changed && !matches!(inner.auth, Auth::SignedOut)
        });
        if replaced_session {
            // Tokens belong to the previous Spotify app.
            let _ = self.sign_out().await;
        }
        Ok(self.status())
    }

    pub async fn profile(&self) -> Result<Option<Profile>, OraError> {
        match &self.lock().auth {
            Auth::SignedIn(Session {
                profile: Some(profile),
                ..
            }) => return Ok(Some(profile.clone())),
            Auth::SignedIn(_) => {}
            _ => return Ok(None),
        }
        self.load_profile().await
    }

    async fn load_profile(&self) -> Result<Option<Profile>, OraError> {
        let generation = self.current_generation();
        let access = self.fresh_access().await.map_err(|e| self.record(e))?;
        let result = match api::me(&self.http, &access).await {
            Err(ApiError::Unauthorized) => {
                let replay = self
                    .forced_refresh(&access)
                    .await
                    .map_err(|e| self.record(e))?;
                api::me(&self.http, &replay).await
            }
            other => other,
        };
        match result {
            Ok(profile) => {
                self.update(|inner, ora| {
                    if ora.current_generation() == generation {
                        if let Auth::SignedIn(session) = &mut inner.auth {
                            session.profile = Some(profile.clone());
                        }
                    }
                });
                Ok(Some(profile))
            }
            Err(ApiError::Unauthorized) => Err(self.expire(generation).await),
            Err(ApiError::Other(error)) => Err(self.record(error)),
        }
    }

    fn record(&self, error: OraError) -> OraError {
        if error != OraError::Cancelled {
            self.update(|inner, _| inner.last_error = Some(error.clone()));
        }
        error
    }

    /// The current access token, refreshed first when it is missing or about to expire.
    pub async fn fresh_access(&self) -> Result<Secret, OraError> {
        if let Some(access) = self.usable_access()? {
            return Ok(access);
        }
        let _gate = self.refresh_gate.lock().await;
        // Another caller may have refreshed while this one waited.
        if let Some(access) = self.usable_access()? {
            return Ok(access);
        }
        self.refresh_now().await
    }

    /// After a 401 with `stale`: refresh unless someone else already replaced it.
    async fn forced_refresh(&self, stale: &Secret) -> Result<Secret, OraError> {
        let _gate = self.refresh_gate.lock().await;
        if let Some(access) = self.usable_access()? {
            if !access.matches(stale.expose()) {
                return Ok(access);
            }
        }
        self.refresh_now().await
    }

    fn usable_access(&self) -> Result<Option<Secret>, OraError> {
        match &self.lock().auth {
            Auth::SignedIn(session) => Ok(session
                .access
                .as_ref()
                .filter(|(_, expires_at)| *expires_at > Instant::now() + REFRESH_MARGIN)
                .map(|(access, _)| access.clone())),
            Auth::SessionExpired => Err(OraError::SessionExpired),
            _ => Err(OraError::SignedOut),
        }
    }

    /// Only called while holding `refresh_gate`.
    async fn refresh_now(&self) -> Result<Secret, OraError> {
        let generation = self.current_generation();
        let (client_id, refresh, session_only) = {
            let inner = self.lock();
            match (&inner.client_id, &inner.auth) {
                (Some(id), Auth::SignedIn(session)) => {
                    (id.clone(), session.refresh.clone(), session.session_only)
                }
                (None, _) => return Err(OraError::NotConfigured),
                _ => return Err(OraError::SignedOut),
            }
        };
        let tokens = match oauth::refresh(&self.http, &client_id, &refresh).await {
            Ok(tokens) => tokens,
            Err(TokenError::InvalidGrant | TokenError::InvalidClient) => {
                return Err(self.expire(generation).await);
            }
            Err(TokenError::RateLimited(retry_after_s)) => {
                return Err(OraError::RateLimited { retry_after_s })
            }
            Err(TokenError::Transport(error)) => return Err(api::transport_error(error)),
            Err(TokenError::Rejected | TokenError::Malformed) => return Err(OraError::Network),
        };
        // A rotated refresh token is saved before the refresh counts as done. If it can't be,
        // the session continues in memory only and the outdated stored entries go.
        let mut stored_ok = true;
        if let (Some(rotated), false) = (&tokens.refresh, session_only) {
            let rotated = rotated.clone();
            stored_ok = self
                .store_if_current(generation, move |store| store.set(REFRESH_TOKEN, &rotated))
                .await
                .ok_or(OraError::Cancelled)?
                .is_ok();
            if !stored_ok {
                let _ = self.forget_stored_if_current(generation).await;
            }
        }
        let access = tokens.access.clone();
        let accepted = self.update(|inner, ora| {
            if ora.current_generation() != generation {
                return false;
            }
            if let Auth::SignedIn(session) = &mut inner.auth {
                session.access = Some((tokens.access, tokens.expires_at));
                if let Some(rotated) = tokens.refresh {
                    session.refresh = rotated;
                }
                if !stored_ok {
                    session.session_only = true;
                    inner.last_error = Some(OraError::StorageUnavailable);
                }
                return true;
            }
            false
        });
        if accepted {
            Ok(access)
        } else {
            Err(OraError::Cancelled)
        }
    }

    /// The saved sign-in is no longer accepted: forget it and ask for a new sign-in.
    async fn expire(&self, generation: u64) -> OraError {
        let current = self.update(|inner, ora| {
            if ora.current_generation() != generation {
                return false;
            }
            ora.bump_generation();
            inner.auth = Auth::SessionExpired;
            inner.last_error = Some(OraError::SessionExpired);
            true
        });
        if current {
            let _ = self.forget_stored().await;
        }
        OraError::SessionExpired
    }
}

fn snapshot(inner: &Inner) -> Status {
    Status {
        auth: match &inner.auth {
            Auth::NotConfigured => AuthState::NotConfigured,
            Auth::SignedOut => AuthState::SignedOut,
            Auth::SigningIn => AuthState::SigningIn,
            Auth::SignedIn(session) => AuthState::SignedIn {
                profile: session.profile.clone(),
                session_only: session.session_only,
            },
            Auth::SessionExpired => AuthState::SessionExpired,
        },
        client_id_from_environment: inner.from_environment,
        client_id_hint: inner.client_id.as_ref().map(ClientId::hint),
        redirect_uri: inner.redirect_uri.clone(),
        last_error: inner.last_error.clone(),
    }
}

/// Deletes both entries, attempting each, and reports either failure.
fn forget<S: CredentialStore>(store: &S) -> Result<(), StoreError> {
    let first = store.delete(REFRESH_TOKEN);
    let second = store.delete(SESSION);
    first.and(second)
}

/// `{authorizedAt, scopes, accountId, clientId}`. Spotify doesn't reveal when a refresh token
/// was issued, and it expires six months after authorisation, so record the time here. The
/// Client ID says which Spotify app the token belongs to.
fn session_metadata(scopes: &str, client_id: &ClientId) -> String {
    let authorized_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    serde_json::json!({
        "authorizedAt": authorized_at,
        "scopes": scopes,
        "accountId": null,
        "clientId": client_id.as_str(),
    })
    .to_string()
}

/// Whether saved session metadata belongs to `client_id`. Entries saved before the Client ID
/// was recorded can't tell, so they are kept as they were.
fn issued_to(metadata: &Secret, client_id: &ClientId) -> bool {
    serde_json::from_str::<serde_json::Value>(metadata.expose())
        .ok()
        .and_then(|value| {
            value["clientId"]
                .as_str()
                .map(|id| id == client_id.as_str())
        })
        .unwrap_or(true)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
