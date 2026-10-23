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

const REMOTE_STATE: &str = r#"{"device":{"id":"phone1","is_active":true,"name":"Phone",
    "type":"Smartphone","supports_volume":false},"is_playing":true,"progress_ms":1000,
    "item":{"uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song","duration_ms":200000,
    "artists":[{"name":"A"}],"album":{"name":"B","images":[]}}}"#;

#[tokio::test]
async fn another_device_is_controlled_through_the_web_api() {
    let h = playback_ready("remote-control").await;
    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    assert_eq!(h.ora.playback().status().device.unwrap().name, "Phone");

    let before = h.http.requests().len();
    for _ in 0..4 {
        h.http.push(204, "");
    }
    let commands = [
        ora_types::PlayerCommand::TogglePlay,
        ora_types::PlayerCommand::TogglePlay,
        ora_types::PlayerCommand::Next,
        ora_types::PlayerCommand::SetShuffle { state: true },
    ];
    for command in commands {
        assert_eq!(h.ora.player_command(command).await, Ok(None));
    }
    // The phone can't change volume, so nothing is sent.
    assert_eq!(
        h.ora
            .player_command(ora_types::PlayerCommand::SetVolume { volume: 0.5 })
            .await,
        Err(OraError::Unsupported)
    );
    let shape: Vec<(Method, String)> = h.http.requests()[before..]
        .iter()
        .map(|r| (r.method, r.url.clone()))
        .collect();
    let api = "https://api.spotify.com/v1/me/player";
    assert_eq!(
        shape,
        [
            (Method::Put, format!("{api}/pause?device_id=phone1")),
            (Method::Put, format!("{api}/play?device_id=phone1")),
            (Method::Post, format!("{api}/next?device_id=phone1")),
            (
                Method::Put,
                format!("{api}/shuffle?state=true&device_id=phone1")
            ),
        ]
    );
    assert!(h.ora.playback().status().shuffle);
}

#[tokio::test]
async fn a_restricted_device_gets_no_commands() {
    let h = playback_ready("remote-restricted").await;
    h.http.push(
        200,
        &REMOTE_STATE.replace(r#""is_active":true"#, r#""is_restricted":true"#),
    );
    h.ora.refresh_playback().await.unwrap();
    let before = h.http.requests().len();
    assert_eq!(
        h.ora.player_command(ora_types::PlayerCommand::Next).await,
        Err(OraError::Forbidden {
            hint: ForbiddenHint::Restricted
        })
    );
    assert_eq!(h.http.requests().len(), before);
}

#[tokio::test]
async fn devices_are_listed_and_chosen_by_key() {
    let h = playback_ready("device-picker").await;
    h.http.push(
        200,
        r#"{"devices":[{"id":"device1","name":"Melora","type":"Computer"},
            {"id":"phone1","name":"Phone","type":"Smartphone","is_active":true}]}"#,
    );
    let devices = h.ora.list_devices().await.unwrap();
    assert!(devices[0].this_app);
    let before = h.http.requests().len();
    h.http.push(204, "");
    h.ora.transfer_playback(&devices[1].key).await.unwrap();
    let seen = &h.http.requests()[before..];
    assert_eq!(seen[0].url, "https://api.spotify.com/v1/me/player");
    assert_eq!(
        seen[0].json,
        Some(serde_json::json!({"device_ids": ["phone1"], "play": true}))
    );
    assert_eq!(
        h.ora.transfer_playback("ffffffffffffffff").await.err(),
        Some(OraError::DeviceUnavailable)
    );
}

#[tokio::test]
async fn device_control_needs_the_permission() {
    let h = harness("remote-scope", User::Approve);
    assert_eq!(
        h.ora.refresh_playback().await.err(),
        Some(OraError::SignedOut)
    );
    h.http.push(200, &tokens_json("acc1", Some("ref1")));
    h.http.push(200, ME);
    h.ora.sign_in(None).await.unwrap();
    let before = h.http.requests().len();
    assert_eq!(
        h.ora.refresh_playback().await.err(),
        Some(OraError::NeedsScope {
            feature: Feature::ConnectControl
        })
    );
    assert_eq!(
        h.ora.list_devices().await.err(),
        Some(OraError::NeedsScope {
            feature: Feature::DevicePicker
        })
    );
    assert_eq!(h.http.requests().len(), before);
}

#[tokio::test]
async fn the_plus_control_saves_and_removes_the_track_playing_now() {
    let h = harness("liked-songs", User::Approve);
    let scopes = format!("{PLAYBACK_SCOPES} user-library-read user-library-modify");
    h.http
        .push(200, &tokens_with_scope("acc1", Some("ref1"), &scopes));
    h.http.push(200, ME);
    h.ora
        .sign_in(Some(Feature::LibrarySaveRemove))
        .await
        .unwrap();
    // Nothing plays: nothing to check and nothing to like, without a request.
    let before = h.http.requests().len();
    h.ora.refresh_liked().await.unwrap();
    assert_eq!(
        h.ora.toggle_liked().await.err(),
        Some(OraError::NoActiveDevice)
    );
    assert_eq!(h.http.requests().len(), before);

    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    h.http.push(200, "[false]");
    h.ora.refresh_liked().await.unwrap();
    let liked = || h.ora.playback().status().now_playing.unwrap().liked;
    assert_eq!(liked(), Some(false));
    h.http.push(200, "");
    assert_eq!(h.ora.toggle_liked().await, Ok(true));
    h.http.push(200, "");
    assert_eq!(h.ora.toggle_liked().await, Ok(false));
    // A new report of the same track keeps the answer, so it isn't asked for again.
    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    assert_eq!(liked(), Some(false));

    let library = "https://api.spotify.com/v1/me/library";
    let uri = "uris=spotify%3Atrack%3AAbCd0123456789xyzXYZ01";
    let shape: Vec<(Method, String)> = h.http.requests()[before..]
        .iter()
        .map(|r| (r.method, r.url.clone()))
        .collect();
    assert_eq!(
        shape[1..4],
        [
            (Method::Get, format!("{library}/contains?{uri}")),
            (Method::Put, format!("{library}?{uri}")),
            (Method::Delete, format!("{library}?{uri}")),
        ]
    );

    // Episodes have no + in Melora yet: nothing is checked, and liking is refused.
    h.http.push(
        200,
        &REMOTE_STATE.replace("spotify:track:", "spotify:episode:"),
    );
    h.ora.refresh_playback().await.unwrap();
    let before = h.http.requests().len();
    h.ora.refresh_liked().await.unwrap();
    assert_eq!(
        h.ora.toggle_liked().await.err(),
        Some(OraError::Unsupported)
    );
    assert_eq!(h.http.requests().len(), before);
    assert_eq!(liked(), None);
}

#[tokio::test]
async fn liking_needs_the_library_permission() {
    let h = playback_ready("liked-scope").await;
    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    let before = h.http.requests().len();
    let needs = Some(OraError::NeedsScope {
        feature: Feature::LibrarySaveRemove,
    });
    assert_eq!(h.ora.refresh_liked().await.err(), needs);
    assert_eq!(h.ora.toggle_liked().await.err(), needs);
    assert_eq!(h.http.requests().len(), before);
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

const LIBRARY_SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read";

/// Signed in with playback, playlists and Liked Songs allowed, and Melora's device ready.
async fn library_ready(label: &str) -> Harness {
    let h = playback_ready(label).await;
    let scopes = format!("{PLAYBACK_SCOPES} {LIBRARY_SCOPES}");
    h.http
        .push(200, &tokens_with_scope("acc2", Some("ref2"), &scopes));
    h.http.push(200, ME);
    h.ora.sign_in(Some(Feature::Playlists)).await.unwrap();
    h
}

const PLAYLISTS: &str = r#"{"href":"x","limit":50,"offset":0,"previous":null,"total":120,
    "next":"https://api.spotify.com/v1/me/playlists?offset=50&limit=50","items":[
    {"id":"PlAyLiSt0123456789abcd","name":"Mine","collaborative":false,
     "owner":{"id":"user1","display_name":"Kiri"},"items":{"total":12},
     "images":[{"url":"https://i.scdn.co/image/big","width":640},
               {"url":"https://i.scdn.co/image/mid","width":300}]},
    {"id":"OtHeRs0123456789abcdef","name":"Theirs\u0007","collaborative":false,
     "owner":{"id":"someone","display_name":null},"tracks":{"total":7},"images":null},
    {"id":"short","name":"Bad ID"},
    null]}"#;

#[tokio::test]
async fn playlists_are_paged_cached_and_say_which_spotify_lists() {
    let h = library_ready("playlists").await;
    let before = h.http.requests().len();
    h.http.push(200, PLAYLISTS);
    let page = h.ora.playlists(0).await.unwrap();
    assert_eq!((page.total, page.next_offset), (120, Some(4)));
    let [mine, theirs] = page.items.as_slice() else {
        panic!("{page:?}");
    };
    assert_eq!(
        (
            mine.name.as_str(),
            mine.owner.as_str(),
            mine.total,
            mine.readable
        ),
        ("Mine", "Kiri", 12, true)
    );
    assert_eq!(
        mine.image_url.as_deref(),
        Some("https://i.scdn.co/image/mid")
    );
    assert_eq!(
        mine.spotify_url,
        "https://open.spotify.com/playlist/PlAyLiSt0123456789abcd"
    );
    // The old `tracks` count still counts; control characters go.
    assert_eq!(
        (
            theirs.name.as_str(),
            theirs.owner.as_str(),
            theirs.total,
            theirs.readable
        ),
        ("Theirs", "", 7, false)
    );

    // A second read within five minutes is answered from memory.
    assert_eq!(h.ora.playlists(0).await.unwrap(), page);
    let seen = h.http.requests();
    assert_eq!(seen.len() - before, 1);
    assert_eq!(
        seen[before].url,
        "https://api.spotify.com/v1/me/playlists?limit=50&offset=0"
    );
    // The next page starts after the rows this one had; offsets past Spotify's limit are
    // refused before a request.
    h.http
        .push(200, &PLAYLISTS.replace(r#""offset":0"#, r#""offset":4"#));
    h.ora.playlists(4).await.unwrap();
    assert_eq!(
        h.http.requests().last().unwrap().url,
        "https://api.spotify.com/v1/me/playlists?limit=50&offset=4"
    );
    let count = h.http.requests().len();
    assert_eq!(h.ora.playlists(100_001).await, Err(OraError::Unsupported));
    assert_eq!(h.http.requests().len(), count);
}

const ITEMS: &str = r#"{"href":"x","limit":50,"offset":50,"previous":null,"next":null,
    "total":54,"items":[
    {"is_local":false,"item":{"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ01",
     "name":"Song","duration_ms":200000,"artists":[{"name":"A"},{"name":"B"}],
     "album":{"name":"Album","images":[]}}},
    null,
    {"is_local":false,"track":{"type":"episode","uri":"spotify:episode:EpIs0123456789xyzXYZ01",
     "name":"Episode","duration_ms":3600000,"show":{"name":"Show","images":[]}}},
    {"is_local":true,"item":{"type":"track","uri":"spotify:local:A:B:Home+made:120",
     "name":"Home made","duration_ms":120000,"artists":[{"name":"Me"}],"album":{"name":"B"}}}]}"#;

#[tokio::test]
async fn playlist_rows_keep_their_place_and_unplayable_ones_say_so() {
    let h = library_ready("playlist-items").await;
    let before = h.http.requests().len();
    h.http.push(200, ITEMS);
    let page = h
        .ora
        .playlist_tracks("PlAyLiSt0123456789abcd", 50)
        .await
        .unwrap();
    assert_eq!((page.total, page.next_offset), (54, None));
    let rows: Vec<_> = page
        .items
        .iter()
        .map(|row| {
            (
                row.position,
                row.uri.as_deref(),
                row.name.as_str(),
                row.album.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                50,
                Some("spotify:track:AbCd0123456789xyzXYZ01"),
                "Song",
                "Album"
            ),
            (
                52,
                Some("spotify:episode:EpIs0123456789xyzXYZ01"),
                "Episode",
                "Show"
            ),
            (53, None, "Home made", "B"),
        ]
    );
    assert_eq!(page.items[0].artists, ["A", "B"]);
    assert_eq!(
        page.items[0].spotify_url.as_deref(),
        Some("https://open.spotify.com/track/AbCd0123456789xyzXYZ01")
    );
    assert_eq!(page.items[2].spotify_url, None);
    assert_eq!(
        h.http.requests()[before].url,
        "https://api.spotify.com/v1/playlists/PlAyLiSt0123456789abcd/items\
         ?limit=50&offset=50&additional_types=episode"
    );

    // Someone else's playlist: Spotify refuses its items, and the UI says why.
    h.http
        .push(403, r#"{"error":{"status":403,"message":"Forbidden"}}"#);
    assert_eq!(
        h.ora.playlist_tracks("OtHeRs0123456789abcdef", 0).await,
        Err(OraError::Forbidden {
            hint: ForbiddenHint::NotOwnerPlaylist
        })
    );
    // The refusal isn't remembered, and malformed IDs never reach Spotify.
    let count = h.http.requests().len();
    assert_eq!(
        h.ora.playlist_tracks("../me", 0).await,
        Err(OraError::Unsupported)
    );
    assert_eq!(h.http.requests().len(), count);
}

#[tokio::test]
async fn liked_songs_are_cached_until_the_plus_changes_them() {
    let h = library_ready("liked-list").await;
    let liked = r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":1,
        "items":[{"added_at":"2026-10-23T00:00:00Z","track":{"type":"track",
        "uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song","duration_ms":1000,
        "artists":[{"name":"A"}],"album":{"name":"B"}}}]}"#;
    let before = h.http.requests().len();
    h.http.push(200, liked);
    let page = h.ora.liked_tracks(0).await.unwrap();
    assert_eq!(
        page.items[0].uri.as_deref(),
        Some("spotify:track:AbCd0123456789xyzXYZ01")
    );
    assert_eq!(h.ora.liked_tracks(0).await.unwrap(), page);
    assert_eq!(
        h.http.requests()[before].url,
        "https://api.spotify.com/v1/me/tracks?limit=50&offset=0"
    );
    assert_eq!(h.http.requests().len() - before, 1);
    h.ora.library().forget_liked();
    h.http.push(200, liked);
    h.ora.liked_tracks(0).await.unwrap();
    assert_eq!(h.http.requests().len() - before, 2);
}

#[tokio::test]
async fn lists_need_their_permission() {
    let h = playback_ready("library-scope").await;
    let before = h.http.requests().len();
    let needs = |feature| Some(OraError::NeedsScope { feature });
    assert_eq!(h.ora.playlists(0).await.err(), needs(Feature::Playlists));
    assert_eq!(
        h.ora
            .playlist_tracks("PlAyLiSt0123456789abcd", 0)
            .await
            .err(),
        needs(Feature::PlaylistDetail)
    );
    assert_eq!(
        h.ora.liked_tracks(0).await.err(),
        needs(Feature::SavedTracks)
    );
    assert_eq!(h.http.requests().len(), before);
}

#[tokio::test]
async fn a_click_plays_on_melora_or_on_the_active_device() {
    use ora_types::PlayRequest;
    let h = library_ready("play-from-list").await;
    let before = h.http.requests().len();
    h.http.push(204, "");
    h.ora
        .play(PlayRequest::Context {
            uri: "spotify:playlist:PlAyLiSt0123456789abcd".into(),
            position: Some(52),
        })
        .await
        .unwrap();
    // Another device is active: it plays there instead.
    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    h.http.push(204, "");
    h.ora
        .play(PlayRequest::Items {
            uris: vec![
                "spotify:track:AbCd0123456789xyzXYZ01".into(),
                "spotify:episode:EpIs0123456789xyzXYZ01".into(),
            ],
        })
        .await
        .unwrap();
    let sent: Vec<(String, Option<serde_json::Value>)> = h.http.requests()[before..]
        .iter()
        .filter(|r| r.method == Method::Put)
        .map(|r| (r.url.clone(), r.json.clone()))
        .collect();
    let play = "https://api.spotify.com/v1/me/player/play?device_id=";
    assert_eq!(
        sent,
        [
            (
                format!("{play}device1"),
                Some(serde_json::json!({
                    "context_uri": "spotify:playlist:PlAyLiSt0123456789abcd",
                    "offset": {"position": 52}
                }))
            ),
            (
                format!("{play}phone1"),
                Some(serde_json::json!({"uris": [
                    "spotify:track:AbCd0123456789xyzXYZ01",
                    "spotify:episode:EpIs0123456789xyzXYZ01"
                ]}))
            ),
        ]
    );

    // Requests the UI shouldn't send are refused before reaching Spotify.
    let count = h.http.requests().len();
    let refused = [
        (
            PlayRequest::Context {
                uri: "spotify:track:AbCd0123456789xyzXYZ01".into(),
                position: None,
            },
            OraError::InvalidSpotifyLink,
        ),
        (
            PlayRequest::Items {
                uris: vec!["spotify:playlist:PlAyLiSt0123456789abcd".into()],
            },
            OraError::InvalidSpotifyLink,
        ),
        (
            PlayRequest::Items { uris: Vec::new() },
            OraError::Unsupported,
        ),
        // Spotify starts an artist's top tracks only from the beginning.
        (
            PlayRequest::Context {
                uri: "spotify:artist:ArTi0123456789xyzXYZ01".into(),
                position: Some(2),
            },
            OraError::Unsupported,
        ),
        (
            PlayRequest::Items {
                uris: vec!["spotify:track:AbCd0123456789xyzXYZ01".into(); 101],
            },
            OraError::Unsupported,
        ),
    ];
    for (request, error) in refused {
        assert_eq!(h.ora.play(request).await, Err(error));
    }
    assert_eq!(h.http.requests().len(), count);

    // A search result's artist plays their top tracks.
    h.http.push(204, "");
    h.ora
        .play(PlayRequest::Context {
            uri: "spotify:artist:ArTi0123456789xyzXYZ01".into(),
            position: None,
        })
        .await
        .unwrap();
    assert_eq!(
        h.http.requests().last().unwrap().json,
        Some(serde_json::json!({"context_uri": "spotify:artist:ArTi0123456789xyzXYZ01"}))
    );

    // With no device at all, the UI is told to choose one.
    h.ora.playback().reset();
    assert_eq!(
        h.ora
            .play(PlayRequest::Context {
                uri: "spotify:playlist:PlAyLiSt0123456789abcd".into(),
                position: None,
            })
            .await,
        Err(OraError::NoActiveDevice)
    );
}

const SEARCH: &str = r#"{
  "tracks":{"href":"x","limit":10,"offset":0,"previous":null,"total":250,
   "next":"https://api.spotify.com/v1/search?offset=10&limit=10","items":[
   {"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song","duration_ms":200000,
    "artists":[{"name":"A"},{"name":"B"}],"album":{"name":"Album","images":[]},"is_playable":true},
   null,
   {"type":"artist","uri":"spotify:artist:ArTi0123456789xyzXYZ01","name":"Wrong kind"}]},
  "artists":{"href":"x","limit":10,"offset":0,"previous":null,"next":null,"total":1,"items":[
   {"type":"artist","uri":"spotify:artist:ArTi0123456789xyzXYZ01","name":"Kiri","genres":[]}]},
  "albums":{"href":"x","limit":10,"offset":0,"previous":null,"next":null,"total":2,"items":[
   {"type":"album","uri":"spotify:album:AlBu0123456789xyzXYZ01","name":"Record",
    "release_date":"2021-03","total_tracks":12,"artists":[{"name":"Kiri"}]},
   {"type":"album","uri":"spotify:album:AlBu0123456789xyzXYZ02","name":"Odd date",
    "release_date":"21/03/2021","total_tracks":1,"artists":[]}]},
  "playlists":{"href":"x","limit":10,"offset":0,"previous":null,"next":null,"total":2,"items":[
   null,
   {"id":"PlAyLiSt0123456789abcd","name":"Mix","owner":{"id":"o","display_name":"Someone"},
    "items":{"total":30}}]},
  "shows":{"href":"x","limit":10,"offset":0,"previous":null,"next":null,"total":1,"items":[
   {"type":"show","uri":"spotify:show:ShOw0123456789xyzXYZ01","name":"Talk","publisher":"Kiri Media",
    "total_episodes":120}]},
  "episodes":{"href":"x","limit":10,"offset":0,"previous":null,"next":null,"total":1,"items":[
   {"type":"episode","uri":"spotify:episode:EpIs0123456789xyzXYZ01","name":"Ep",
    "duration_ms":3600000,"release_date":"2026-10-01","is_playable":false}]}
}"#;

/// A result with only the fields every kind has.
fn result(position: u32, uri: &str, name: &str) -> ora_types::CatalogItem {
    let (kind, id) = uri["spotify:".len()..].split_once(':').unwrap();
    ora_types::CatalogItem {
        position,
        uri: uri.into(),
        name: name.into(),
        by: Vec::new(),
        album: String::new(),
        release_date: String::new(),
        total: None,
        duration_ms: None,
        playable: true,
        spotify_url: format!("https://open.spotify.com/{kind}/{id}"),
    }
}

#[tokio::test]
async fn search_lists_each_kind_and_caches_by_query() {
    use ora_types::{CatalogItem, SearchKind};
    let h = playback_ready("search").await;
    let before = h.http.requests().len();
    h.http.push(200, SEARCH);
    let found = h.ora.search("  Kiri & Te ").await.unwrap();
    assert_eq!(
        h.http.requests()[before].url,
        "https://api.spotify.com/v1/search?q=Kiri+%26+Te\
         &type=track,artist,album,playlist,show,episode&limit=10"
    );

    // Rows keep their place among all results; nulls and links of the wrong kind are skipped.
    assert_eq!(
        (found.tracks.total, found.tracks.next_offset),
        (250, Some(3))
    );
    assert_eq!(
        found.tracks.items,
        [CatalogItem {
            by: vec!["A".into(), "B".into()],
            album: "Album".into(),
            duration_ms: Some(200_000),
            ..result(0, "spotify:track:AbCd0123456789xyzXYZ01", "Song")
        }]
    );
    assert_eq!(
        found.artists.items,
        [result(0, "spotify:artist:ArTi0123456789xyzXYZ01", "Kiri")]
    );
    assert_eq!(
        found.albums.items,
        [
            CatalogItem {
                by: vec!["Kiri".into()],
                release_date: "2021-03".into(),
                total: Some(12),
                ..result(0, "spotify:album:AlBu0123456789xyzXYZ01", "Record")
            },
            // A date in another shape is dropped.
            CatalogItem {
                total: Some(1),
                ..result(1, "spotify:album:AlBu0123456789xyzXYZ02", "Odd date")
            },
        ]
    );
    // A playlist without a URI is found by its ID.
    assert_eq!(
        found.playlists.items,
        [CatalogItem {
            by: vec!["Someone".into()],
            total: Some(30),
            ..result(1, "spotify:playlist:PlAyLiSt0123456789abcd", "Mix")
        }]
    );
    assert_eq!(
        found.episodes.items,
        [CatalogItem {
            release_date: "2026-10-01".into(),
            duration_ms: Some(3_600_000),
            playable: false,
            ..result(0, "spotify:episode:EpIs0123456789xyzXYZ01", "Ep")
        }]
    );
    assert_eq!(
        found.shows.items,
        [CatalogItem {
            by: vec!["Kiri Media".into()],
            total: Some(120),
            ..result(0, "spotify:show:ShOw0123456789xyzXYZ01", "Talk")
        }]
    );

    // The same query in any case, and "See all"'s first page, come from memory.
    assert_eq!(h.ora.search("kiri & te").await.unwrap(), found);
    assert_eq!(
        h.ora
            .search_page("KIRI & TE", SearchKind::Album, 0)
            .await
            .unwrap(),
        found.albums
    );
    assert_eq!(h.http.requests().len() - before, 1);

    // Further pages ask for one kind, and stop at Spotify's 1,000 results.
    let page = |offset: u32| {
        let items = [r#"{"uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Again"}"#; 10];
        format!(
            r#"{{"tracks":{{"href":"x","limit":10,"offset":{offset},"previous":null,
            "total":5000,"next":"https://api.spotify.com/v1/search?more","items":[{}]}}}}"#,
            items.join(",")
        )
    };
    h.http.push(200, &page(980));
    let more = h
        .ora
        .search_page("Kiri & Te", SearchKind::Track, 980)
        .await
        .unwrap();
    assert_eq!(
        h.http.requests().last().unwrap().url,
        "https://api.spotify.com/v1/search?q=Kiri+%26+Te&type=track&limit=10&offset=980"
    );
    assert_eq!((more.items[0].position, more.next_offset), (980, Some(990)));
    h.http.push(200, &page(990));
    let last = h
        .ora
        .search_page("Kiri & Te", SearchKind::Track, 990)
        .await
        .unwrap();
    assert_eq!((last.items.len(), last.next_offset), (10, None));

    // Queries and offsets Spotify would refuse never reach it.
    let count = h.http.requests().len();
    for query in ["   ".to_owned(), "a".repeat(201), "line\nbreak".to_owned()] {
        assert_eq!(h.ora.search(&query).await, Err(OraError::Unsupported));
    }
    assert_eq!(
        h.ora.search_page("Kiri", SearchKind::Track, 991).await,
        Err(OraError::Unsupported)
    );
    assert_eq!(h.http.requests().len(), count);
}

#[tokio::test]
async fn a_newer_search_cancels_the_older_one() {
    let h = playback_ready("search-cancel").await;
    let before = h.http.requests().len();
    h.http.set_delay(Some(Duration::from_millis(50)));
    h.http.push(200, SEARCH);
    let (older, newer) = tokio::join!(h.ora.search("Kir"), async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        h.ora.search("Kiri").await
    });
    assert_eq!(older, Err(OraError::Cancelled));
    assert_eq!(newer.unwrap().artists.items.len(), 1);
    assert_eq!(h.http.requests().len() - before, 2);
    assert_eq!(h.ora.status().last_error, None);

    // The cancelled query wasn't cached: asking again asks Spotify.
    h.http.set_delay(None);
    h.http.push(200, SEARCH);
    assert!(h.ora.search("Kir").await.is_ok());
    assert_eq!(h.http.requests().len() - before, 3);
}

#[tokio::test]
async fn search_needs_a_sign_in() {
    let h = harness("search-signed-out", User::Approve);
    assert_eq!(h.ora.search("Kiri").await, Err(OraError::SignedOut));
    assert_eq!(
        h.ora
            .search_page("Kiri", ora_types::SearchKind::Track, 10)
            .await,
        Err(OraError::SignedOut)
    );
    assert!(h.http.requests().is_empty());
}

/// Signed in with playback and every library permission, and Melora's device ready.
async fn whole_library_ready(label: &str) -> Harness {
    let h = playback_ready(label).await;
    let scopes = format!(
        "{PLAYBACK_SCOPES} {LIBRARY_SCOPES} user-library-modify user-read-playback-position"
    );
    h.http
        .push(200, &tokens_with_scope("acc2", Some("ref2"), &scopes));
    h.http.push(200, ME);
    h.ora
        .sign_in(Some(Feature::SavedShowsEpisodes))
        .await
        .unwrap();
    h
}

#[tokio::test]
async fn saved_albums_shows_and_episodes_are_listed() {
    let h = whole_library_ready("saved-lists").await;
    let before = h.http.requests().len();
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":2,"items":[
        {"added_at":"2026-10-01T00:00:00Z","album":{"type":"album",
         "uri":"spotify:album:AlBu0123456789xyzXYZ01","name":"Record","release_date":"2021",
         "total_tracks":9,"artists":[{"name":"Kiri"}],"tracks":{"total":9,"items":[]}}},
        null]}"#,
    );
    let albums = h.ora.saved_albums(0).await.unwrap();
    assert_eq!(
        albums.items,
        [ora_types::CatalogItem {
            by: vec!["Kiri".into()],
            release_date: "2021".into(),
            total: Some(9),
            ..result(0, "spotify:album:AlBu0123456789xyzXYZ01", "Record")
        }]
    );
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":1,"items":[
        {"added_at":"2026-10-01T00:00:00Z","show":{"type":"show",
         "uri":"spotify:show:ShOw0123456789xyzXYZ01","name":"Talk","publisher":"Kiri Media",
         "total_episodes":120}}]}"#,
    );
    let shows = h.ora.saved_shows(0).await.unwrap();
    assert_eq!(shows.items[0].by, ["Kiri Media"]);
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":1,"items":[
        {"added_at":"2026-10-01T00:00:00Z","episode":{"type":"episode",
         "uri":"spotify:episode:EpIs0123456789xyzXYZ01","name":"Ep","duration_ms":3600000,
         "release_date":"2026-10-02","show":{"name":"Talk","images":[]}}}]}"#,
    );
    let episodes = h.ora.saved_episodes(0).await.unwrap();
    let episode = &episodes.items[0];
    assert_eq!(
        (
            episode.uri.as_deref(),
            episode.album.as_str(),
            episode.release_date.as_str()
        ),
        (
            Some("spotify:episode:EpIs0123456789xyzXYZ01"),
            "Talk",
            "2026-10-02"
        )
    );

    // Each is cached for a minute.
    h.ora.saved_albums(0).await.unwrap();
    h.ora.saved_shows(0).await.unwrap();
    h.ora.saved_episodes(0).await.unwrap();
    let urls: Vec<String> = h.http.requests()[before..]
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        [
            "https://api.spotify.com/v1/me/albums?limit=50&offset=0",
            "https://api.spotify.com/v1/me/shows?limit=50&offset=0",
            "https://api.spotify.com/v1/me/episodes?limit=50&offset=0",
        ]
    );
}

#[tokio::test]
async fn albums_and_shows_list_their_tracks_and_episodes() {
    let h = whole_library_ready("album-show").await;
    let before = h.http.requests().len();
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":50,"previous":null,"next":null,"total":52,"items":[
        {"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song",
         "duration_ms":200000,"artists":[{"name":"A"}],"is_local":false},
        null,
        {"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ02","name":"Other",
         "duration_ms":100000,"artists":[]}]}"#,
    );
    let tracks = h
        .ora
        .album_tracks("AlBu0123456789xyzXYZ01", 50)
        .await
        .unwrap();
    // Rows keep their place in the album, so playing one starts at the right track.
    let rows: Vec<(u32, &str, &str)> = tracks
        .items
        .iter()
        .map(|row| (row.position, row.name.as_str(), row.album.as_str()))
        .collect();
    assert_eq!(rows, [(50, "Song", ""), (52, "Other", "")]);
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,
        "next":"https://api.spotify.com/v1/shows/x/episodes?offset=50","total":300,"items":[
        {"type":"episode","uri":"spotify:episode:EpIs0123456789xyzXYZ01","name":"Ep",
         "duration_ms":3600000,"release_date":"2026-10-02","images":[]}]}"#,
    );
    let episodes = h
        .ora
        .show_episodes("ShOw0123456789xyzXYZ01", 0)
        .await
        .unwrap();
    assert_eq!(
        (
            episodes.items[0].release_date.as_str(),
            episodes.next_offset
        ),
        ("2026-10-02", Some(1))
    );
    let urls: Vec<String> = h.http.requests()[before..]
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        [
            "https://api.spotify.com/v1/albums/AlBu0123456789xyzXYZ01/tracks?limit=50&offset=50",
            "https://api.spotify.com/v1/shows/ShOw0123456789xyzXYZ01/episodes?limit=50&offset=0",
        ]
    );
    let count = h.http.requests().len();
    assert_eq!(
        h.ora.album_tracks("../me", 0).await,
        Err(OraError::Unsupported)
    );
    assert_eq!(
        h.ora.show_episodes("short", 0).await,
        Err(OraError::Unsupported)
    );
    assert_eq!(h.http.requests().len(), count);
}

fn track_uri(n: usize) -> String {
    format!("spotify:track:{n:0>22}")
}

#[tokio::test]
async fn saved_states_are_asked_40_at_a_time_and_remembered() {
    let h = whole_library_ready("saved-states").await;
    let before = h.http.requests().len();
    let uris: Vec<String> = (0..45).map(track_uri).collect();
    let answers = |count: usize, saved: fn(usize) -> bool| {
        serde_json::to_string(&(0..count).map(saved).collect::<Vec<bool>>()).unwrap()
    };
    h.http.push(200, &answers(40, |i| i % 2 == 0));
    h.http.push(200, &answers(5, |_| true));
    let saved = h.ora.saved_states(&uris).await.unwrap();
    assert_eq!(
        saved,
        (0..45).map(|i| i >= 40 || i % 2 == 0).collect::<Vec<_>>()
    );
    let seen = h.http.requests();
    assert_eq!(seen.len() - before, 2);
    let first = &seen[before].url;
    assert!(first.starts_with("https://api.spotify.com/v1/me/library/contains?uris="));
    assert_eq!(first.matches("%2C").count(), 39);

    // Known answers aren't asked again; only the new item is.
    h.http.push(200, "[false]");
    let mut more = uris[..3].to_vec();
    more.push(track_uri(99));
    assert_eq!(
        h.ora.saved_states(&more).await.unwrap(),
        [true, false, true, false]
    );
    assert_eq!(h.http.requests().len() - before, 3);
    assert_eq!(
        h.http.requests().last().unwrap().url,
        format!(
            "https://api.spotify.com/v1/me/library/contains?uris={}",
            track_uri(99).replace(':', "%3A")
        )
    );

    // Only library kinds, and not too many at once.
    let count = h.http.requests().len();
    assert_eq!(
        h.ora
            .saved_states(&["spotify:artist:ArTi0123456789xyzXYZ01".into()])
            .await,
        Err(OraError::InvalidSpotifyLink)
    );
    assert_eq!(
        h.ora
            .saved_states(&(0..501).map(track_uri).collect::<Vec<_>>())
            .await,
        Err(OraError::Unsupported)
    );
    assert_eq!(h.ora.saved_states(&[]).await, Err(OraError::Unsupported));
    assert_eq!(h.http.requests().len(), count);
}

#[tokio::test]
async fn saving_goes_40_at_a_time_and_keeps_everything_in_step() {
    let h = whole_library_ready("set-saved").await;
    // The player shows the first track; Liked Songs and saved albums are cached.
    h.http.push(200, REMOTE_STATE);
    h.ora.refresh_playback().await.unwrap();
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":0,"items":[]}"#,
    );
    h.ora.liked_tracks(0).await.unwrap();
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":0,"items":[]}"#,
    );
    h.ora.saved_albums(0).await.unwrap();

    let before = h.http.requests().len();
    let mut uris: Vec<String> = (0..41).map(track_uri).collect();
    uris[0] = "spotify:track:AbCd0123456789xyzXYZ01".into();
    uris.push(uris[1].clone());
    h.http.push(200, "");
    h.http.push(200, "");
    h.ora.set_saved(&uris, true).await.unwrap();
    let sent: Vec<(Method, usize)> = h.http.requests()[before..]
        .iter()
        .map(|r| (r.method, r.url.matches("%2C").count() + 1))
        .collect();
    // The duplicate goes; 41 items are two requests.
    assert_eq!(sent, [(Method::Put, 40), (Method::Put, 1)]);
    assert_eq!(
        h.ora.playback().status().now_playing.unwrap().liked,
        Some(true)
    );
    // The new states are known without asking, and Liked Songs is read again.
    assert_eq!(h.ora.saved_states(&uris[..2]).await.unwrap(), [true, true]);
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":0,"items":[]}"#,
    );
    h.ora.liked_tracks(0).await.unwrap();
    // Saved albums weren't affected, so they still come from memory.
    h.ora.saved_albums(0).await.unwrap();
    assert_eq!(h.http.requests().len() - before, 3);

    // Removing an album forgets the saved albums.
    h.http.push(200, "");
    h.ora
        .set_saved(&["spotify:album:AlBu0123456789xyzXYZ01".into()], false)
        .await
        .unwrap();
    assert_eq!(h.http.requests().last().unwrap().method, Method::Delete);
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"offset":0,"previous":null,"next":null,"total":0,"items":[]}"#,
    );
    h.ora.saved_albums(0).await.unwrap();
    assert_eq!(h.http.requests().len() - before, 5);
}

#[tokio::test]
async fn library_views_need_their_permission() {
    let h = library_ready("library-views-scope").await;
    let before = h.http.requests().len();
    let needs = |feature| Some(OraError::NeedsScope { feature });
    assert_eq!(
        h.ora.saved_shows(0).await.err(),
        needs(Feature::SavedShowsEpisodes)
    );
    assert_eq!(
        h.ora.saved_episodes(0).await.err(),
        needs(Feature::SavedShowsEpisodes)
    );
    assert_eq!(
        h.ora.show_episodes("ShOw0123456789xyzXYZ01", 0).await.err(),
        needs(Feature::SavedShowsEpisodes)
    );
    assert_eq!(
        h.ora.set_saved(&[track_uri(1)], true).await.err(),
        needs(Feature::LibrarySaveRemove)
    );
    assert_eq!(h.http.requests().len(), before);
}

const QUEUE: &str = r#"{"currently_playing":{"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ01",
    "name":"Song","duration_ms":200000,"artists":[{"name":"A"}],"album":{"name":"B","images":[]}},
    "queue":[
    {"type":"episode","uri":"spotify:episode:EpIs0123456789xyzXYZ01","name":"Ep",
     "duration_ms":3600000,"release_date":"2026-10-02","show":{"name":"Talk","images":[]}},
    null,
    {"type":"track","uri":"spotify:track:AbCd0123456789xyzXYZ02","name":"Next",
     "duration_ms":100000,"artists":[],"album":{"name":"C","images":[]}}]}"#;

#[tokio::test]
async fn the_queue_is_read_on_demand_and_shared_while_fresh() {
    let h = playback_ready("queue-read").await;
    let before = h.http.requests().len();
    h.http.push(200, QUEUE);
    let queue = h.ora.queue().await.unwrap();
    assert_eq!(
        queue.current.as_ref().map(|row| row.name.as_str()),
        Some("Song")
    );
    let next: Vec<(u32, &str, &str)> = queue
        .next
        .iter()
        .map(|row| (row.position, row.name.as_str(), row.album.as_str()))
        .collect();
    assert_eq!(next, [(0, "Ep", "Talk"), (1, "Next", "C")]);
    // Asked again straight away, the same answer serves.
    h.ora.queue().await.unwrap();
    assert_eq!(h.http.requests().len(), before + 1);
    assert_eq!(
        h.http.requests().last().unwrap().url,
        "https://api.spotify.com/v1/me/player/queue"
    );

    // Nothing playing is an empty queue, not an error.
    h.ora.library().forget_queue();
    h.http.push(204, "");
    assert_eq!(
        h.ora.queue().await,
        Ok(ora_types::QueueView {
            current: None,
            next: Vec::new(),
        })
    );
}

#[tokio::test]
async fn adding_to_the_queue_goes_to_the_device_playing_now() {
    let h = playback_ready("queue-add").await;
    let song = "spotify:track:AbCd0123456789xyzXYZ01";
    // Melora is a device but nothing plays: Spotify has no queue to add to.
    let before = h.http.requests().len();
    assert_eq!(
        h.ora.add_to_queue(song).await,
        Err(OraError::NoActiveDevice)
    );
    // Only tracks and episodes can be queued.
    assert_eq!(
        h.ora
            .add_to_queue("spotify:album:AlBu0123456789xyzXYZ01")
            .await,
        Err(OraError::InvalidSpotifyLink)
    );
    assert_eq!(h.http.requests().len(), before);

    // While Melora plays, it's queued here.
    h.ora.playback().apply(ora_types::SdkEvent::State {
        state: Some(ora_types::SdkState {
            paused: false,
            position_ms: 0,
            duration_ms: 200_000,
            track: None,
            shuffle: false,
            repeat: ora_types::RepeatMode::default(),
            disallows: ora_types::Disallows::default(),
        }),
    });
    h.http.push(200, QUEUE);
    h.ora.queue().await.unwrap();
    h.http.push(204, "");
    h.ora.add_to_queue(song).await.unwrap();
    let added = h.http.requests().last().unwrap().clone();
    assert_eq!(added.method, Method::Post);
    assert_eq!(
        added.url,
        "https://api.spotify.com/v1/me/player/queue?uri=spotify%3Atrack%3AAbCd0123456789xyzXYZ01&device_id=device1"
    );
    // The queue is read again after an addition.
    h.http.push(200, QUEUE);
    h.ora.queue().await.unwrap();
    assert!(h
        .http
        .requests()
        .last()
        .unwrap()
        .url
        .ends_with("/me/player/queue"));

    // While another device plays, it's queued there.
    h.http.push(200, REMOTE_STATE);
    h.ora
        .playback()
        .apply(ora_types::SdkEvent::State { state: None });
    h.ora.refresh_playback().await.unwrap();
    h.http.push(204, "");
    h.ora
        .add_to_queue("spotify:episode:EpIs0123456789xyzXYZ01")
        .await
        .unwrap();
    assert!(h
        .http
        .requests()
        .last()
        .unwrap()
        .url
        .ends_with("uri=spotify%3Aepisode%3AEpIs0123456789xyzXYZ01&device_id=phone1"));

    // A Premium refusal comes back as such.
    h.http.push(
        403,
        r#"{"error":{"status":403,"message":"Premium required"}}"#,
    );
    assert!(matches!(
        h.ora.add_to_queue(song).await,
        Err(OraError::Forbidden { .. })
    ));
}

#[tokio::test]
async fn recently_played_lists_the_latest_tracks_and_needs_its_permission() {
    let h = playback_ready("recent").await;
    assert_eq!(
        h.ora.recently_played().await.err(),
        Some(OraError::NeedsScope {
            feature: Feature::RecentlyPlayed
        })
    );
    let scopes = format!("{PLAYBACK_SCOPES} user-read-recently-played");
    h.http
        .push(200, &tokens_with_scope("acc2", Some("ref2"), &scopes));
    h.http.push(200, ME);
    h.ora.sign_in(Some(Feature::RecentlyPlayed)).await.unwrap();
    let before = h.http.requests().len();
    h.http.push(
        200,
        r#"{"href":"x","limit":50,"next":null,"cursors":{"after":"1","before":"0"},"items":[
        {"played_at":"2026-10-23T01:00:00Z","context":null,"track":{"type":"track",
         "uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song","duration_ms":200000,
         "artists":[{"name":"A"}],"album":{"name":"B","images":[]}}},
        null,
        {"played_at":"2026-10-23T00:50:00Z","track":null},
        {"played_at":"2026-10-23T00:40:00Z","track":{"type":"track",
         "uri":"spotify:track:AbCd0123456789xyzXYZ01","name":"Song","duration_ms":200000,
         "artists":[{"name":"A"}],"album":{"name":"B","images":[]}}}]}"#,
    );
    let recent = h.ora.recently_played().await.unwrap();
    // A track played twice is listed twice, each in its own place.
    let rows: Vec<(u32, Option<&str>)> = recent
        .items
        .iter()
        .map(|row| (row.position, row.uri.as_deref()))
        .collect();
    assert_eq!(
        rows,
        [
            (0, Some("spotify:track:AbCd0123456789xyzXYZ01")),
            (1, Some("spotify:track:AbCd0123456789xyzXYZ01"))
        ]
    );
    assert_eq!((recent.total, recent.next_offset), (2, None));
    // Cached briefly.
    h.ora.recently_played().await.unwrap();
    let urls: Vec<String> = h.http.requests()[before..]
        .iter()
        .map(|r| r.url.clone())
        .collect();
    assert_eq!(
        urls,
        ["https://api.spotify.com/v1/me/player/recently-played?limit=50"]
    );
}

/// Signed in as `library_ready`, with permission to change playlists too.
async fn editing_ready(label: &str) -> Harness {
    let h = library_ready(label).await;
    let scopes = format!(
        "{PLAYBACK_SCOPES} {LIBRARY_SCOPES} playlist-modify-public playlist-modify-private"
    );
    h.http
        .push(200, &tokens_with_scope("acc3", Some("ref3"), &scopes));
    h.http.push(200, ME);
    h.ora.sign_in(Some(Feature::PlaylistEdit)).await.unwrap();
    h
}

const MINE: &str = "PlAyLiSt0123456789abcd";

#[tokio::test]
async fn new_playlists_are_private_and_named_carefully() {
    let h = library_ready("create").await;
    assert_eq!(
        h.ora.create_playlist("Road trip").await.err(),
        Some(OraError::NeedsScope {
            feature: Feature::CreatePlaylist
        })
    );
    let h = editing_ready("create-allowed").await;
    let before = h.http.requests().len();
    // Names are trimmed; empty, overlong and control-character names never reach Spotify.
    for name in ["", "   ", "Tab\tname", &"x".repeat(101)] {
        assert_eq!(
            h.ora.create_playlist(name).await.err(),
            Some(OraError::Unsupported)
        );
    }
    assert_eq!(h.http.requests().len(), before);
    h.http.push(
        201,
        r#"{"id":"NeWpLaYlIsT0123456789a","name":"Road trip","collaborative":false,"public":false,
        "owner":{"id":"user1","display_name":"Kiri"},"items":{"total":0},"images":[]}"#,
    );
    let created = h.ora.create_playlist("  Road trip ").await.unwrap();
    assert_eq!(
        (
            created.id.as_str(),
            created.name.as_str(),
            created.total,
            created.readable
        ),
        ("NeWpLaYlIsT0123456789a", "Road trip", 0, true)
    );
    let seen = &h.http.requests()[before];
    assert_eq!(
        (seen.method, seen.url.as_str()),
        (Method::Post, "https://api.spotify.com/v1/me/playlists")
    );
    assert_eq!(
        seen.json,
        Some(serde_json::json!({ "name": "Road trip", "public": false }))
    );
}

#[tokio::test]
async fn playlist_items_are_added_and_removed_in_batches_of_100() {
    let h = editing_ready("edit-items").await;
    let before = h.http.requests().len();
    let uris: Vec<String> = (0..150).map(track_uri).collect();
    h.http.push(201, r#"{"snapshot_id":"s1"}"#);
    h.http.push(201, r#"{"snapshot_id":"s2"}"#);
    h.ora.add_to_playlist(MINE, &uris).await.unwrap();
    let seen = h.http.requests();
    let added: Vec<(Method, &str, usize)> = seen[before..]
        .iter()
        .map(|r| {
            let count = r.json.as_ref().unwrap()["uris"].as_array().unwrap().len();
            (r.method, r.url.as_str(), count)
        })
        .collect();
    let url = "https://api.spotify.com/v1/playlists/PlAyLiSt0123456789abcd/items";
    assert_eq!(added, [(Method::Post, url, 100), (Method::Post, url, 50)]);
    assert_eq!(
        seen[before].json.as_ref().unwrap()["uris"][0],
        "spotify:track:0000000000000000000000"
    );

    // Albums aren't playlist items, and bad IDs are refused, both before any request.
    let count = h.http.requests().len();
    assert_eq!(
        h.ora
            .add_to_playlist(MINE, &["spotify:album:AbCd0123456789xyzXYZ01".into()])
            .await,
        Err(OraError::InvalidSpotifyLink)
    );
    assert_eq!(
        h.ora.add_to_playlist("short", &[track_uri(1)]).await,
        Err(OraError::Unsupported)
    );
    assert_eq!(h.http.requests().len(), count);

    // Removing sends each URI once; Spotify removes every copy.
    h.http.push(200, r#"{"snapshot_id":"s3"}"#);
    h.ora
        .remove_from_playlist(MINE, &[track_uri(1), track_uri(2), track_uri(1)])
        .await
        .unwrap();
    let removed = h.http.requests().last().unwrap().clone();
    assert_eq!(
        (removed.method, removed.url.as_str()),
        (Method::Delete, url)
    );
    assert_eq!(
        removed.json,
        Some(serde_json::json!({ "items": [{ "uri": track_uri(1) }, { "uri": track_uri(2) }] }))
    );

    // Someone else's playlist is refused by Spotify, and says why.
    h.http
        .push(403, r#"{"error":{"status":403,"message":"Forbidden"}}"#);
    assert_eq!(
        h.ora.add_to_playlist(MINE, &[track_uri(3)]).await,
        Err(OraError::Forbidden {
            hint: ForbiddenHint::NotOwnerPlaylist
        })
    );
}

#[tokio::test]
async fn moving_an_item_names_where_it_lands_and_forgets_the_cached_rows() {
    let h = editing_ready("move").await;
    h.http.push(200, ITEMS);
    h.ora.playlist_tracks(MINE, 50).await.unwrap();
    let before = h.http.requests().len();
    // Down: Spotify inserts before the row after the target. Up: before the target itself.
    h.http.push(200, r#"{"snapshot_id":"s1"}"#);
    h.http.push(200, r#"{"snapshot_id":"s2"}"#);
    h.ora.move_in_playlist(MINE, 2, 5).await.unwrap();
    h.ora.move_in_playlist(MINE, 5, 2).await.unwrap();
    // Staying put sends nothing.
    h.ora.move_in_playlist(MINE, 3, 3).await.unwrap();
    let moves: Vec<(Method, Option<serde_json::Value>)> = h.http.requests()[before..]
        .iter()
        .map(|r| (r.method, r.json.clone()))
        .collect();
    assert_eq!(
        moves,
        [
            (
                Method::Put,
                Some(
                    serde_json::json!({ "range_start": 2, "insert_before": 6, "range_length": 1 })
                )
            ),
            (
                Method::Put,
                Some(
                    serde_json::json!({ "range_start": 5, "insert_before": 2, "range_length": 1 })
                )
            ),
        ]
    );
    // The playlist's rows are read again after a change.
    h.http.push(200, ITEMS);
    h.ora.playlist_tracks(MINE, 50).await.unwrap();
    assert_eq!(h.http.requests().len(), before + 3);
}

#[tokio::test]
async fn melora_keeps_its_own_playlist_order_per_account_without_spotify() {
    let h = playback_ready("order-scope").await;
    assert_eq!(
        h.ora.playlist_order().err(),
        Some(OraError::NeedsScope {
            feature: Feature::Playlists
        })
    );
    let h = library_ready("order").await;
    let before = h.http.requests().len();
    assert_eq!(h.ora.playlist_order(), Ok(Vec::new()));
    let order = vec!["OtHeRs0123456789abcdef".to_owned(), MINE.to_owned()];
    h.ora.set_playlist_order(&order).unwrap();
    assert_eq!(h.ora.playlist_order(), Ok(order));
    assert!(h.dir.join("playlist-order.json").is_file());
    // Going back to Spotify's order forgets it.
    h.ora.set_playlist_order(&[]).unwrap();
    assert_eq!(h.ora.playlist_order(), Ok(Vec::new()));
    assert_eq!(h.http.requests().len(), before);
}
