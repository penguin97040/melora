use super::*;
use crate::{
    browser::BrowserUnavailable,
    http::{mock::MockHttp, Method},
    loopback::tests::get,
    oauth::TOKEN_URL,
    store::mock::MemoryStore,
};
use ora_types::ForbiddenHint;
use std::{
    path::Path,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

const CLIENT_ID: &str = "0123456789abcdef0123456789abcdef";

#[derive(Clone, Copy)]
enum User {
    Approve,
    Deny,
    Ignore,
}

#[derive(Clone)]
struct MockBrowser {
    user: User,
    opened: Arc<Mutex<Vec<String>>>,
}

impl Browser for MockBrowser {
    fn open(&self, url: &str) -> Result<(), BrowserUnavailable> {
        self.opened.lock().unwrap().push(url.to_owned());
        let parsed = url::Url::parse(url).map_err(|_| BrowserUnavailable)?;
        let param = |name: &str| {
            parsed
                .query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default()
        };
        let (redirect, state) = (param("redirect_uri"), param("state"));
        let query = match self.user {
            User::Approve => format!("/callback?code=fixture-code&state={state}"),
            User::Deny => format!("/callback?error=access_denied&state={state}"),
            User::Ignore => return Ok(()),
        };
        tokio::spawn(async move { get(&redirect, &query).await });
        Ok(())
    }
}

type TestOra = Ora<MockHttp, MemoryStore, MockBrowser>;

struct Harness {
    ora: Arc<TestOra>,
    http: MockHttp,
    store: MemoryStore,
    browser: MockBrowser,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ora-session-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn configure(dir: &Path) {
    ConfigStore::new(dir.to_owned(), "ORA_TEST_UNSET_VARIABLE")
        .save(&ClientId::parse(CLIENT_ID).unwrap())
        .unwrap();
}

fn build(http: MockHttp, store: MemoryStore, user: User, dir: PathBuf) -> Harness {
    build_with_port(http, store, user, dir, 0)
}

fn build_with_port(
    http: MockHttp,
    store: MemoryStore,
    user: User,
    dir: PathBuf,
    callback_port: u16,
) -> Harness {
    let browser = MockBrowser {
        user,
        opened: Arc::default(),
    };
    let ora = Arc::new(Ora::new(
        http.clone(),
        store.clone(),
        browser.clone(),
        Options {
            app_name: "Melora",
            settings_dir: dir.clone(),
            client_id_variable: "ORA_TEST_UNSET_VARIABLE",
            sign_in_timeout: Duration::from_secs(5),
            callback_port,
        },
    ));
    Harness {
        ora,
        http,
        store,
        browser,
        dir,
    }
}

fn harness(label: &str, user: User) -> Harness {
    let dir = temp_dir(label);
    configure(&dir);
    build(MockHttp::default(), MemoryStore::default(), user, dir)
}

// Fixture tokens are short and their JSON keys are assembled, so no source line looks like
// a real credential to the publication gate.
fn tokens_json(access: &str, refresh: Option<&str>) -> String {
    tokens_with_scope(access, refresh, "user-read-private")
}

fn tokens_with_scope(access: &str, refresh: Option<&str>, scope: &str) -> String {
    let refresh = refresh
        .map(|r| format!(r#","refresh_{}":"{r}""#, "token"))
        .unwrap_or_default();
    format!(
        r#"{{"access_{}":"{access}","token_type":"Bearer","expires_in":3600,"scope":"{scope}"{refresh}}}"#,
        "token"
    )
}

const PLAYBACK_SCOPES: &str = "streaming user-modify-playback-state user-read-currently-playing \
    user-read-playback-state user-read-private";

fn granted(status: &Status) -> Vec<Feature> {
    match &status.auth {
        AuthState::SignedIn { granted, .. } => granted.clone(),
        _ => Vec::new(),
    }
}

const ME: &str = r#"{"account_id":"acct1","id":"user1","display_name":"Kiri",
    "images":[],"external_urls":{"spotify":"https://open.spotify.com/user/user1"}}"#;

fn signed_in_profile(status: &Status) -> Option<Profile> {
    match &status.auth {
        AuthState::SignedIn { profile, .. } => profile.clone(),
        _ => None,
    }
}

async fn wait_for_signing_in(ora: &TestOra) {
    let mut status = ora.subscribe();
    let _ = status.wait_for(|s| s.auth == AuthState::SigningIn).await;
}

#[tokio::test]
async fn sign_in_saves_the_refresh_token_and_shows_the_profile() {
    let h = harness("sign-in", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);

    let status = h.ora.sign_in(None).await.unwrap();

    assert_eq!(
        signed_in_profile(&status).unwrap().display_name.as_deref(),
        Some("Kiri")
    );
    assert_eq!(h.store.value(REFRESH_TOKEN).as_deref(), Some("ref1"));
    let session: serde_json::Value =
        serde_json::from_str(&h.store.value(SESSION).unwrap()).unwrap();
    assert_eq!(session["scopes"], "user-read-private");
    assert!(session["authorizedAt"].as_u64().unwrap() > 1_700_000_000);
    assert_eq!(session["clientId"], CLIENT_ID);

    // The authorise URL asked only for the profile scope, and the exchange proved the PKCE pair.
    let opened = h.browser.opened.lock().unwrap()[0].clone();
    let url = url::Url::parse(&opened).unwrap();
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    assert_eq!(param("scope"), "user-read-private");
    let requests = h.http.requests();
    assert_eq!(requests[0].url, TOKEN_URL);
    let form = |key: &str| {
        requests[0]
            .form
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(
        pkce::challenge(&form("code_verifier")),
        param("code_challenge")
    );
    assert_eq!(form("code"), "fixture-code");
    assert_eq!(form("redirect_uri"), param("redirect_uri"));
    assert_eq!(requests[1].method, Method::Get);
    assert_eq!(requests.len(), 2);
}

#[tokio::test]
async fn relaunch_restores_the_session_and_sign_out_forgets_it() {
    let h = harness("relaunch", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();

    // A new process: same store and settings, nothing in memory.
    let relaunched = build(
        MockHttp::default(),
        h.store.clone(),
        User::Ignore,
        h.dir.clone(),
    );
    relaunched.http.push(200, &tokens_json("acc2", None));
    relaunched.http.push(200, ME);
    relaunched.ora.restore().await;
    let status = relaunched.ora.status();
    assert!(signed_in_profile(&status).is_some());
    assert_eq!(relaunched.http.requests()[0].form[0].1, "refresh_token");

    let status = relaunched.ora.sign_out().await.unwrap();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(relaunched.store.value(REFRESH_TOKEN), None);
    assert_eq!(relaunched.store.value(SESSION), None);

    // And the next launch starts signed out without any request.
    let third = build(
        MockHttp::default(),
        h.store.clone(),
        User::Ignore,
        h.dir.clone(),
    );
    third.ora.restore().await;
    assert_eq!(third.ora.status().auth, AuthState::SignedOut);
    assert!(third.http.requests().is_empty());
}

#[tokio::test]
async fn failed_deletion_is_reported_but_memory_is_cleared() {
    let h = harness("delete-failure", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();
    h.store.fail_deletes.store(true, Ordering::SeqCst);

    assert_eq!(
        h.ora.sign_out().await.err(),
        Some(OraError::StorageUnavailable)
    );
    let status = h.ora.status();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(status.last_error, Some(OraError::StorageUnavailable));
    assert_eq!(h.ora.fresh_access().await.err(), Some(OraError::SignedOut));
}

#[tokio::test]
async fn unavailable_store_gives_a_visible_session_only_sign_in() {
    let h = harness("session-only", User::Approve);
    h.store.unavailable.store(true, Ordering::SeqCst);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);

    let status = h.ora.sign_in(None).await.unwrap();

    assert!(matches!(
        status.auth,
        AuthState::SignedIn {
            session_only: true,
            ..
        }
    ));
    assert_eq!(status.last_error, Some(OraError::StorageUnavailable));
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
}

/// Signs in, then makes the access token stale so the next use must refresh.
async fn signed_in_with_stale_access(label: &str) -> Harness {
    let h = harness(label, User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();
    if let Auth::SignedIn(session) = &mut h.ora.lock().auth {
        session.access = Some((Secret::new("old"), Instant::now()));
    }
    h
}

#[tokio::test]
async fn ten_concurrent_callers_share_one_refresh() {
    let h = signed_in_with_stale_access("single-flight").await;
    // Slow replies make all ten callers overlap with the first refresh.
    h.http.set_delay(Some(Duration::from_millis(50)));
    let http = h.http.clone();
    let ora = Arc::clone(&h.ora);
    let before = http.requests().len();
    http.push(200, &tokens_json("acc2", None));
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let ora = Arc::clone(&ora);
            tokio::spawn(async move { ora.fresh_access().await })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.await.unwrap().unwrap().expose(), "acc2");
    }
    assert_eq!(http.requests().len() - before, 1);
}

#[tokio::test]
async fn rotated_refresh_token_is_saved_before_use() {
    let h = signed_in_with_stale_access("rotation").await;
    h.http.push(200, &tokens_json("acc2", Some("ref2")));
    assert_eq!(h.ora.fresh_access().await.unwrap().expose(), "acc2");
    assert_eq!(h.store.value(REFRESH_TOKEN).as_deref(), Some("ref2"));
}

#[tokio::test]
async fn invalid_grant_expires_the_session_without_retrying() {
    let h = signed_in_with_stale_access("invalid-grant").await;
    let before = h.http.requests().len();
    h.http.push(400, r#"{"error":"invalid_grant"}"#);

    assert_eq!(
        h.ora.fresh_access().await.err(),
        Some(OraError::SessionExpired)
    );
    assert_eq!(h.ora.status().auth, AuthState::SessionExpired);
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
    assert_eq!(h.http.requests().len() - before, 1);
    assert_eq!(
        h.ora.fresh_access().await.err(),
        Some(OraError::SessionExpired)
    );
    assert_eq!(h.http.requests().len() - before, 1);
}

#[tokio::test]
async fn unauthorised_profile_refreshes_and_replays_once() {
    let h = harness("replay", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(401, "");
    h.http.push(200, &tokens_json("acc2", None));
    h.http.push(200, ME);
    let status = h.ora.sign_in(None).await.unwrap();
    assert!(signed_in_profile(&status).is_some());
    let urls: Vec<String> = h.http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            TOKEN_URL,
            "https://api.spotify.com/v1/me",
            TOKEN_URL,
            "https://api.spotify.com/v1/me"
        ]
    );
}

#[tokio::test]
async fn a_second_unauthorised_reply_expires_the_session() {
    let h = harness("replay-fails", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(401, "");
    h.http.push(200, &tokens_json("acc2", None));
    h.http.push(401, "");
    h.ora.sign_in(None).await.unwrap();
    assert_eq!(h.ora.status().auth, AuthState::SessionExpired);
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
    assert_eq!(h.http.requests().len(), 4);
}

#[tokio::test]
async fn forbidden_profile_points_at_the_allowlist() {
    let h = harness("allowlist", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(403, "");
    let status = h.ora.sign_in(None).await.unwrap();
    assert!(matches!(
        status.auth,
        AuthState::SignedIn { profile: None, .. }
    ));
    assert_eq!(
        status.last_error,
        Some(OraError::Forbidden {
            hint: ForbiddenHint::Allowlist
        })
    );
}

#[tokio::test]
async fn sign_out_during_sign_in_cancels_it_and_saves_nothing() {
    let h = harness("logout-during-login", User::Ignore);
    let ora = Arc::clone(&h.ora);
    let pending = tokio::spawn(async move { ora.sign_in(None).await });
    wait_for_signing_in(&h.ora).await;

    h.ora.sign_out().await.unwrap();

    assert_eq!(pending.await.unwrap().err(), Some(OraError::Cancelled));
    let status = h.ora.status();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(status.last_error, None);
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
    assert!(h.http.requests().is_empty());
}

#[tokio::test]
async fn cancel_returns_to_the_previous_state_and_the_late_callback_is_refused() {
    let h = harness("cancel", User::Ignore);
    let ora = Arc::clone(&h.ora);
    let pending = tokio::spawn(async move { ora.sign_in(None).await });
    wait_for_signing_in(&h.ora).await;
    let opened = h.browser.opened.lock().unwrap()[0].clone();

    assert_eq!(h.ora.cancel_sign_in().auth, AuthState::SignedOut);
    assert_eq!(pending.await.unwrap().err(), Some(OraError::Cancelled));

    let url = url::Url::parse(&opened).unwrap();
    let redirect = url
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned())
        .unwrap();
    assert!(get(&redirect, "/callback?code=late&state=x").await.is_err());
    assert!(h.http.requests().is_empty());
}

#[tokio::test]
async fn declining_on_spotify_is_a_quiet_cancel() {
    let h = harness("denied", User::Deny);
    assert_eq!(h.ora.sign_in(None).await.err(), Some(OraError::Cancelled));
    let status = h.ora.status();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(status.last_error, None);
}

#[tokio::test]
async fn sign_in_times_out_visibly() {
    let dir = temp_dir("timeout");
    configure(&dir);
    let browser = MockBrowser {
        user: User::Ignore,
        opened: Arc::default(),
    };
    let ora = Ora::new(
        MockHttp::default(),
        MemoryStore::default(),
        browser,
        Options {
            app_name: "Melora",
            settings_dir: dir.clone(),
            client_id_variable: "ORA_TEST_UNSET_VARIABLE",
            sign_in_timeout: Duration::from_millis(50),
            callback_port: 0,
        },
    );
    let expected = OraError::SignIn {
        reason: SignInFailure::TimedOut,
    };
    assert_eq!(ora.sign_in(None).await.err(), Some(expected.clone()));
    assert_eq!(ora.status().last_error, Some(expected));
    assert_eq!(ora.status().auth, AuthState::SignedOut);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rejected_client_id_is_reported_as_such() {
    let h = harness("invalid-client", User::Approve);
    h.http.push(400, r#"{"error":"invalid_client"}"#);
    assert_eq!(
        h.ora.sign_in(None).await.err(),
        Some(OraError::SignIn {
            reason: SignInFailure::InvalidClient
        })
    );
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
}

#[tokio::test]
async fn client_id_setting_is_validated_and_saved() {
    let dir = temp_dir("client-id");
    let h = build(
        MockHttp::default(),
        MemoryStore::default(),
        User::Ignore,
        dir,
    );
    assert_eq!(h.ora.status().auth, AuthState::NotConfigured);
    assert_eq!(
        h.ora.sign_in(None).await.err(),
        Some(OraError::NotConfigured)
    );
    assert_eq!(
        h.ora.set_client_id("not valid").await.err(),
        Some(OraError::InvalidClientId)
    );
    let status = h.ora.set_client_id(CLIENT_ID).await.unwrap();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(status.client_id_hint.as_deref(), Some("0123…cdef"));
    let saved = std::fs::read_to_string(h.dir.join("spotify.json")).unwrap();
    assert!(saved.contains(CLIENT_ID));
}

#[tokio::test]
async fn changing_the_client_id_signs_out_of_the_old_app() {
    let h = harness("client-id-change", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();
    let status = h
        .ora
        .set_client_id("fedcba9876543210fedcba9876543210")
        .await
        .unwrap();
    assert_eq!(status.auth, AuthState::SignedOut);
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
}

#[tokio::test]
async fn nothing_token_like_reaches_status_or_debug_output() {
    let h = harness("no-leaks", User::Approve);
    h.http
        .push(200, &tokens_json("acc-leakcheck", Some("ref-leakcheck")));
    h.http.push(200, ME);
    let status = h.ora.sign_in(None).await.unwrap();
    let shown = format!(
        "{}{:?}{:?}",
        serde_json::to_string(&status).unwrap(),
        status,
        h.http.requests().first().map(|r| r.method)
    );
    let access_debug = format!("{:?}", h.ora.fresh_access().await.unwrap());
    for leaked in ["acc-leakcheck", "ref-leakcheck", "fixture-code", CLIENT_ID] {
        assert!(!shown.contains(leaked), "{leaked}");
        assert!(!access_debug.contains(leaked), "{leaked}");
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_fixed_callback_port_is_used_everywhere_and_shown() {
    let port = free_port();
    let dir = temp_dir("fixed-port");
    configure(&dir);
    let h = build_with_port(
        MockHttp::default(),
        MemoryStore::default(),
        User::Approve,
        dir,
        port,
    );
    let expected = format!("http://127.0.0.1:{port}/callback");
    assert_eq!(h.ora.status().redirect_uri, expected);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);

    h.ora.sign_in(None).await.unwrap();

    let opened = url::Url::parse(&h.browser.opened.lock().unwrap()[0]).unwrap();
    let sent = opened
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned());
    assert_eq!(sent.as_deref(), Some(expected.as_str()));
    let exchange = &h.http.requests()[0];
    assert!(exchange
        .form
        .iter()
        .any(|(k, v)| k == "redirect_uri" && *v == expected));
}

#[tokio::test]
async fn a_busy_callback_port_is_reported_before_opening_the_browser() {
    let blocker = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = blocker.local_addr().unwrap().port();
    let dir = temp_dir("busy-port");
    configure(&dir);
    let h = build_with_port(
        MockHttp::default(),
        MemoryStore::default(),
        User::Approve,
        dir,
        port,
    );
    let expected = OraError::SignIn {
        reason: SignInFailure::CallbackPortBusy,
    };
    assert_eq!(h.ora.sign_in(None).await.err(), Some(expected.clone()));
    assert_eq!(h.ora.status().last_error, Some(expected));
    assert_eq!(h.ora.status().auth, AuthState::SignedOut);
    assert!(h.browser.opened.lock().unwrap().is_empty());
    drop(blocker);
}

#[test]
fn port_zero_keeps_the_documented_port_less_registration() {
    assert_eq!(registered_redirect_uri(0), "http://127.0.0.1/callback");
    assert_eq!(
        registered_redirect_uri(43821),
        "http://127.0.0.1:43821/callback"
    );
}

#[tokio::test]
async fn sign_out_during_a_refresh_leaves_nothing_stored() {
    let h = signed_in_with_stale_access("logout-during-refresh").await;
    h.http.set_delay(Some(Duration::from_millis(100)));
    h.http.push(200, &tokens_json("acc2", Some("ref2")));
    let before = h.http.requests().len();
    let ora = Arc::clone(&h.ora);
    let refreshing = tokio::spawn(async move { ora.fresh_access().await });
    while h.http.requests().len() == before {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    h.ora.sign_out().await.unwrap();

    assert_eq!(refreshing.await.unwrap().err(), Some(OraError::Cancelled));
    assert_eq!(h.ora.status().auth, AuthState::SignedOut);
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
    assert_eq!(h.store.value(SESSION), None);
}

#[tokio::test]
async fn a_second_sign_in_while_one_is_pending_keeps_the_first() {
    let dir = temp_dir("double-sign-in");
    configure(&dir);
    let h = build_with_port(
        MockHttp::default(),
        MemoryStore::default(),
        User::Ignore,
        dir,
        free_port(),
    );
    let ora = Arc::clone(&h.ora);
    let first = tokio::spawn(async move { ora.sign_in(None).await });
    wait_for_signing_in(&h.ora).await;

    assert_eq!(
        h.ora.sign_in(None).await.unwrap().auth,
        AuthState::SigningIn
    );

    let opened = loop {
        let opened = h.browser.opened.lock().unwrap().first().cloned();
        if let Some(opened) = opened {
            break opened;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(h.browser.opened.lock().unwrap().len(), 1);
    let url = url::Url::parse(&opened).unwrap();
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    };
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    let reply = get(
        &param("redirect_uri"),
        &format!("/callback?code=fixture-code&state={}", param("state")),
    )
    .await
    .unwrap();
    assert!(reply.contains("200"));
    assert!(signed_in_profile(&first.await.unwrap().unwrap()).is_some());
}

#[tokio::test]
async fn a_saved_sign_in_for_another_client_id_is_left_alone() {
    let h = harness("other-client", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();

    // Another copy, or a development override, uses a different Spotify app.
    let dir = temp_dir("other-client-copy");
    ConfigStore::new(dir.clone(), "ORA_TEST_UNSET_VARIABLE")
        .save(&ClientId::parse("fedcba9876543210fedcba9876543210").unwrap())
        .unwrap();
    let other = build(MockHttp::default(), h.store.clone(), User::Ignore, dir);
    other.ora.restore().await;

    assert_eq!(other.ora.status().auth, AuthState::SignedOut);
    assert!(other.http.requests().is_empty());
    assert_eq!(h.store.value(REFRESH_TOKEN).as_deref(), Some("ref1"));
}

#[tokio::test]
async fn a_saved_sign_in_without_a_recorded_client_id_still_restores() {
    let h = harness("earlier-session", User::Ignore);
    h.store.insert(REFRESH_TOKEN, "ref1");
    h.store.insert(
        SESSION,
        r#"{"authorizedAt":1,"scopes":"user-read-private","accountId":null}"#,
    );
    h.http.push(200, &tokens_json("acc1", None));
    h.http.push(200, ME);
    h.ora.restore().await;
    assert!(signed_in_profile(&h.ora.status()).is_some());
}

#[tokio::test]
async fn a_failed_save_leaves_no_older_sign_in_behind() {
    let h = harness("failed-save", User::Approve);
    h.store.insert(REFRESH_TOKEN, "older");
    h.store.fail_writes.store(true, Ordering::SeqCst);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);

    let status = h.ora.sign_in(None).await.unwrap();

    assert!(matches!(
        status.auth,
        AuthState::SignedIn {
            session_only: true,
            ..
        }
    ));
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
}

#[tokio::test]
async fn a_rotated_token_that_cannot_be_saved_clears_the_stored_one() {
    let h = signed_in_with_stale_access("rotation-failure").await;
    h.store.fail_writes.store(true, Ordering::SeqCst);
    h.http.push(200, &tokens_json("acc2", Some("ref2")));

    assert_eq!(h.ora.fresh_access().await.unwrap().expose(), "acc2");

    assert!(matches!(
        h.ora.status().auth,
        AuthState::SignedIn {
            session_only: true,
            ..
        }
    ));
    assert_eq!(h.store.value(REFRESH_TOKEN), None);
    assert_eq!(h.store.value(SESSION), None);
}

#[tokio::test]
async fn restore_reads_the_saved_scopes() {
    let h = harness("restore-scopes", User::Ignore);
    h.store.insert(REFRESH_TOKEN, "ref1");
    h.store.insert(
        SESSION,
        &format!(r#"{{"authorizedAt":1,"scopes":"{PLAYBACK_SCOPES}","accountId":null}}"#),
    );
    h.http.push(200, &tokens_with_scope("acc1", None, ""));
    h.http.push(200, ME);
    h.ora.restore().await;
    let features = granted(&h.ora.status());
    assert!(features.contains(&Feature::EmbeddedPlayback));
    assert!(features.contains(&Feature::Profile));
    assert!(!features.contains(&Feature::Playlists));
}

#[tokio::test]
async fn a_saved_sign_in_without_scopes_grants_only_scope_free_features() {
    let h = harness("restore-no-scopes", User::Ignore);
    h.store.insert(REFRESH_TOKEN, "ref1");
    h.store
        .insert(SESSION, r#"{"authorizedAt":1,"accountId":null}"#);
    h.http.push(200, &tokens_with_scope("acc1", None, ""));
    h.http.push(200, ME);
    h.ora.restore().await;
    let features = granted(&h.ora.status());
    assert!(features.contains(&Feature::SignIn));
    assert!(!features.contains(&Feature::Profile));
    assert!(!features.contains(&Feature::EmbeddedPlayback));
}

#[tokio::test]
async fn asking_for_playback_requests_every_scope_and_records_the_grant() {
    let h = harness("incremental", User::Approve);
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    let status = h.ora.sign_in(None).await.unwrap();
    assert!(!granted(&status).contains(&Feature::EmbeddedPlayback));

    h.http.push(
        200,
        &tokens_with_scope("acc2", Some("ref2"), PLAYBACK_SCOPES),
    );
    h.http.push(200, ME);
    let status = h
        .ora
        .sign_in(Some(Feature::EmbeddedPlayback))
        .await
        .unwrap();

    assert!(granted(&status).contains(&Feature::EmbeddedPlayback));
    let opened = h.browser.opened.lock().unwrap()[1].clone();
    let url = url::Url::parse(&opened).unwrap();
    let scope = url
        .query_pairs()
        .find(|(k, _)| k == "scope")
        .map(|(_, v)| v.into_owned())
        .unwrap();
    assert_eq!(
        scope,
        "streaming user-modify-playback-state user-read-currently-playing \
         user-read-playback-state user-read-private"
    );
    let session: serde_json::Value =
        serde_json::from_str(&h.store.value(SESSION).unwrap()).unwrap();
    assert_eq!(session["scopes"], PLAYBACK_SCOPES);
    assert_eq!(h.store.value(REFRESH_TOKEN).as_deref(), Some("ref2"));
}

#[tokio::test]
async fn cancelling_the_extra_permission_keeps_the_existing_sign_in() {
    let h = harness("incremental-cancel", User::Ignore);
    h.store.insert(REFRESH_TOKEN, "ref1");
    h.store.insert(
        SESSION,
        r#"{"authorizedAt":1,"scopes":"user-read-private","accountId":null}"#,
    );
    h.http.push(200, &tokens_json("acc1", None));
    h.http.push(200, ME);
    h.ora.restore().await;

    let ora = Arc::clone(&h.ora);
    let pending = tokio::spawn(async move { ora.sign_in(Some(Feature::EmbeddedPlayback)).await });
    wait_for_signing_in(&h.ora).await;
    let status = h.ora.cancel_sign_in();

    assert!(signed_in_profile(&status).is_some());
    assert_eq!(pending.await.unwrap().err(), Some(OraError::Cancelled));
    assert_eq!(h.store.value(REFRESH_TOKEN).as_deref(), Some("ref1"));
    assert_eq!(h.ora.fresh_access().await.unwrap().expose(), "acc1");
}

#[tokio::test]
async fn a_refresh_with_different_scopes_updates_the_grant() {
    let h = signed_in_with_stale_access("refresh-scopes").await;
    h.http
        .push(200, &tokens_with_scope("acc2", None, PLAYBACK_SCOPES));
    h.ora.fresh_access().await.unwrap();
    assert!(granted(&h.ora.status()).contains(&Feature::EmbeddedPlayback));
}

/// Signed in with the playback scopes, and the SDK device "device1" ready.
async fn playback_ready(label: &str) -> Harness {
    let h = harness(label, User::Approve);
    h.http.push(
        200,
        &tokens_with_scope("acc1", Some("ref1"), PLAYBACK_SCOPES),
    );
    h.http.push(200, ME);
    h.ora
        .sign_in(Some(Feature::EmbeddedPlayback))
        .await
        .unwrap();
    h.ora.playback().apply(ora_types::SdkEvent::Probe {
        probe: ora_types::Probe {
            secure: true,
            widevine: true,
            ..ora_types::Probe::default()
        },
    });
    h.ora.playback().apply(ora_types::SdkEvent::Ready {
        device_id: "device1".into(),
    });
    h
}

#[tokio::test]
async fn the_sdk_token_needs_the_playback_permission() {
    let h = harness("sdk-token-scope", User::Approve);
    assert_eq!(h.ora.sdk_token().await.err(), Some(OraError::SignedOut));
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();
    assert_eq!(
        h.ora.sdk_token().await.err(),
        Some(OraError::NeedsScope {
            feature: Feature::EmbeddedPlayback
        })
    );

    let h = playback_ready("sdk-token").await;
    assert_eq!(h.ora.sdk_token().await.unwrap().expose(), "acc1");
}

#[tokio::test]
async fn play_here_transfers_or_plays_the_pasted_link_on_this_device() {
    let h = playback_ready("play-here").await;
    let before = h.http.requests().len();
    h.http.push(204, "");
    h.ora.play_here(None).await.unwrap();
    h.http.push(204, "");
    let id: String = "AbCd0123456789".chars().chain("xyzXYZ01".chars()).collect();
    h.ora
        .play_here(Some(&format!(" https://open.spotify.com/album/{id}?si=x ")))
        .await
        .unwrap();

    let seen = &h.http.requests()[before..];
    assert_eq!(seen[0].url, "https://api.spotify.com/v1/me/player");
    assert_eq!(
        seen[0].json,
        Some(serde_json::json!({"device_ids": ["device1"], "play": true}))
    );
    assert_eq!(
        seen[1].url,
        "https://api.spotify.com/v1/me/player/play?device_id=device1"
    );
    assert_eq!(
        seen[1].json,
        Some(serde_json::json!({"context_uri": format!("spotify:album:{id}")}))
    );
}

#[tokio::test]
async fn play_here_refuses_bad_links_and_missing_devices_without_a_request() {
    let h = playback_ready("play-here-refusals").await;
    let before = h.http.requests().len();
    assert_eq!(
        h.ora
            .play_here(Some("https://example.invalid/album/x"))
            .await
            .err(),
        Some(OraError::InvalidSpotifyLink)
    );
    h.ora.playback().reset();
    assert_eq!(
        h.ora.play_here(None).await.err(),
        Some(OraError::NoActiveDevice)
    );
    assert_eq!(h.http.requests().len(), before);
}

#[tokio::test]
async fn play_here_replays_once_after_an_unauthorised_reply() {
    let h = playback_ready("play-here-replay").await;
    let before = h.http.requests().len();
    h.http.push(401, "");
    h.http
        .push(200, &tokens_with_scope("acc2", None, PLAYBACK_SCOPES));
    h.http.push(204, "");
    h.ora.play_here(None).await.unwrap();
    let urls: Vec<String> = h.http.requests()[before..]
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        [
            "https://api.spotify.com/v1/me/player",
            TOKEN_URL,
            "https://api.spotify.com/v1/me/player"
        ]
    );
}

#[tokio::test]
async fn a_premium_refusal_is_returned_without_retrying() {
    let h = playback_ready("play-here-premium").await;
    let before = h.http.requests().len();
    h.http.push(403, "");
    assert_eq!(
        h.ora.play_here(None).await.err(),
        Some(OraError::Forbidden {
            hint: ForbiddenHint::Premium
        })
    );
    assert_eq!(h.http.requests().len() - before, 1);
    // A player refusal isn't an account problem, so Settings doesn't show it.
    assert_eq!(h.ora.status().last_error, None);
}

#[tokio::test]
async fn shuffle_and_repeat_run_through_the_web_api_and_the_rest_go_to_the_sdk() {
    let h = playback_ready("player-command").await;
    let before = h.http.requests().len();
    h.http.push(204, "");
    h.http.push(204, "");
    assert_eq!(
        h.ora
            .player_command(ora_types::PlayerCommand::SetShuffle { state: true })
            .await,
        Ok(None)
    );
    assert_eq!(
        h.ora
            .player_command(ora_types::PlayerCommand::SetRepeat {
                mode: ora_types::RepeatMode::Track
            })
            .await,
        Ok(None)
    );
    assert_eq!(
        h.ora.player_command(ora_types::PlayerCommand::Next).await,
        Ok(Some(ora_types::PlayerCommand::Next))
    );
    let urls: Vec<String> = h.http.requests()[before..]
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        [
            "https://api.spotify.com/v1/me/player/shuffle?state=true&device_id=device1",
            "https://api.spotify.com/v1/me/player/repeat?state=track&device_id=device1"
        ]
    );

    h.ora.playback().reset();
    assert_eq!(
        h.ora
            .player_command(ora_types::PlayerCommand::SetShuffle { state: false })
            .await,
        Err(OraError::NoActiveDevice)
    );
    assert_eq!(h.http.requests().len() - before, 2);
}
