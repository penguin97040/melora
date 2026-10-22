//! Spotify Web API calls and the mapping from HTTP replies to `OraError`. Domain models only
//! leave this module; raw DTOs stay private.

use crate::{
    http::{HttpClient, Method, Request, Response, TransportError},
    link::SpotifyUri,
    secret::Secret,
};
use ora_types::{ForbiddenHint, OraError, Profile, RepeatMode};
use serde::Deserialize;

pub const API_BASE: &str = "https://api.spotify.com/v1";
/// The only image host a client's CSP allows (`img-src`). Images elsewhere would be blocked, so
/// they are dropped here instead of failing silently in the UI.
pub const IMAGE_HOST: &str = "i.scdn.co";

/// A failed call: `Unauthorized` is handled by refreshing once and replaying once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    Unauthorized,
    Other(OraError),
}

pub fn transport_error(error: TransportError) -> OraError {
    match error {
        TransportError::Offline => OraError::Offline,
        TransportError::Network => OraError::Network,
    }
}

#[derive(Deserialize)]
struct QuotaBody {
    error: QuotaError,
}

#[derive(Deserialize)]
struct QuotaError {
    reason: Option<String>,
}

/// Maps a non-success reply. 403 is never retried; 429 never loops.
pub fn error_for(response: &Response, forbidden: ForbiddenHint) -> ApiError {
    ApiError::Other(match response.status {
        401 => return ApiError::Unauthorized,
        403 => OraError::Forbidden { hint: forbidden },
        404 => OraError::NotFound,
        429 => {
            let quota = serde_json::from_slice::<QuotaBody>(&response.body)
                .ok()
                .and_then(|b| b.error.reason)
                .is_some_and(|reason| reason == "QUOTA_EXCEEDED");
            if quota {
                OraError::QuotaExceeded {
                    retry_after_s: response.retry_after_s,
                }
            } else {
                OraError::RateLimited {
                    retry_after_s: response.retry_after_s,
                }
            }
        }
        _ => OraError::Network,
    })
}

#[derive(Deserialize)]
struct MeBody {
    account_id: Option<String>,
    id: String,
    display_name: Option<String>,
    #[serde(default)]
    images: Vec<ImageBody>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct ImageBody {
    url: String,
    width: Option<u32>,
}

#[derive(Deserialize)]
struct ExternalUrls {
    spotify: Option<String>,
}

/// `GET /me`, reduced to the fields that still exist after Spotify's 2026 changes.
pub async fn me<H: HttpClient>(http: &H, access: &Secret) -> Result<Profile, ApiError> {
    let response = http
        .send(Request {
            method: Method::Get,
            url: format!("{API_BASE}/me"),
            bearer: Some(access.clone()),
            form: Vec::new(),
            json: None,
        })
        .await
        .map_err(|e| ApiError::Other(transport_error(e)))?;
    if response.status != 200 {
        // Straight after sign-in, a 403 here almost always means the allowlist.
        return Err(error_for(&response, ForbiddenHint::Allowlist));
    }
    let body: MeBody =
        serde_json::from_slice(&response.body).map_err(|_| ApiError::Other(OraError::Network))?;
    // The smallest image that exists is enough for an avatar.
    let image_url = body
        .images
        .into_iter()
        .filter(|image| is_allowed_image(&image.url))
        .min_by_key(|image| image.width.unwrap_or(u32::MAX))
        .map(|image| image.url);
    let spotify_url = body
        .external_urls
        .and_then(|urls| urls.spotify)
        .filter(|url| crate::browser::spotify_page(url).is_some());
    Ok(Profile {
        account_id: body.account_id.unwrap_or_else(|| body.id.clone()),
        id: body.id,
        display_name: body.display_name.filter(|name| !name.trim().is_empty()),
        image_url,
        spotify_url,
    })
}

/// Player commands: 403 means Premium is missing or the action is restricted, and 404 means
/// Spotify knows no active device. Neither is retried.
fn player_error(response: &Response) -> ApiError {
    if response.status == 404 {
        ApiError::Other(OraError::NoActiveDevice)
    } else {
        error_for(response, ForbiddenHint::Premium)
    }
}

async fn player_put<H: HttpClient>(
    http: &H,
    access: &Secret,
    url: String,
    body: Option<serde_json::Value>,
) -> Result<(), ApiError> {
    let response = http
        .send(Request {
            method: Method::Put,
            url,
            bearer: Some(access.clone()),
            form: Vec::new(),
            json: body,
        })
        .await
        .map_err(|e| ApiError::Other(transport_error(e)))?;
    if matches!(response.status, 200 | 202 | 204) {
        Ok(())
    } else {
        Err(player_error(&response))
    }
}

/// `PUT /me/player`: makes `device_id` the active device. With `play`, playback continues
/// there; otherwise it keeps its current paused or playing state.
pub async fn transfer<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    play: bool,
) -> Result<(), ApiError> {
    let body = serde_json::json!({ "device_ids": [device_id], "play": play });
    player_put(http, access, format!("{API_BASE}/me/player"), Some(body)).await
}

/// `PUT /me/player/play?device_id=…`: starts an album, artist, playlist or show as a context,
/// or a single track or episode.
pub async fn play<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    target: &SpotifyUri,
) -> Result<(), ApiError> {
    let body = if target.is_context() {
        serde_json::json!({ "context_uri": target.uri() })
    } else {
        serde_json::json!({ "uris": [target.uri()] })
    };
    let url = format!("{API_BASE}/me/player/play?device_id={}", encode(device_id));
    player_put(http, access, url, Some(body)).await
}

/// `PUT /me/player/shuffle?state=…&device_id=…` (no body).
pub async fn shuffle<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    state: bool,
) -> Result<(), ApiError> {
    let url = format!(
        "{API_BASE}/me/player/shuffle?state={state}&device_id={}",
        encode(device_id)
    );
    player_put(http, access, url, None).await
}

/// `PUT /me/player/repeat?state=off|context|track&device_id=…` (no body).
pub async fn repeat<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    mode: RepeatMode,
) -> Result<(), ApiError> {
    let state = match mode {
        RepeatMode::Off => "off",
        RepeatMode::Context => "context",
        RepeatMode::Track => "track",
    };
    let url = format!(
        "{API_BASE}/me/player/repeat?state={state}&device_id={}",
        encode(device_id)
    );
    player_put(http, access, url, None).await
}

fn encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub(crate) fn is_allowed_image(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some(IMAGE_HOST)
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttp;

    #[tokio::test]
    async fn profile_uses_current_fields_and_smallest_image() {
        let http = MockHttp::default();
        http.push(
            200,
            r#"{"account_id":"acct","id":"user1","display_name":"Kiri","type":"user",
                "images":[{"url":"https://i.scdn.co/image/big","width":300,"height":300},
                          {"url":"https://i.scdn.co/image/small","width":64,"height":64}],
                "external_urls":{"spotify":"https://open.spotify.com/user/user1"},
                "country":"NZ","product":"premium"}"#,
        );
        let profile = me(&http, &Secret::new("a")).await.unwrap();
        assert_eq!(
            profile,
            Profile {
                account_id: "acct".into(),
                id: "user1".into(),
                display_name: Some("Kiri".into()),
                image_url: Some("https://i.scdn.co/image/small".into()),
                spotify_url: Some("https://open.spotify.com/user/user1".into()),
            }
        );
        let seen = &http.requests()[0];
        assert_eq!(seen.url, "https://api.spotify.com/v1/me");
        assert!(seen.had_bearer);
    }

    #[tokio::test]
    async fn missing_optional_fields_and_unsafe_links_are_dropped() {
        let http = MockHttp::default();
        http.push(
            200,
            r#"{"id":"user1","display_name":" ","images":[{"url":"http://insecure/x","width":1}],
                "external_urls":{"spotify":"https://evil.example/user1"}}"#,
        );
        let profile = me(&http, &Secret::new("a")).await.unwrap();
        assert_eq!(profile.account_id, "user1");
        assert_eq!(profile.display_name, None);
        assert_eq!(profile.image_url, None);
        assert_eq!(profile.spotify_url, None);
    }

    #[tokio::test]
    async fn only_images_the_csp_allows_are_kept() {
        let http = MockHttp::default();
        http.push(
            200,
            r#"{"id":"user1","images":[
                {"url":"https://platform-lookaside.fbsbx.com/platform/profilepic/x","width":1},
                {"url":"https://i.scdn.co.example/image/x","width":2},
                {"url":"https://i.scdn.co:8443/image/x","width":3},
                {"url":"https://i.scdn.co/image/kept","width":300}]}"#,
        );
        let profile = me(&http, &Secret::new("a")).await.unwrap();
        assert_eq!(
            profile.image_url.as_deref(),
            Some("https://i.scdn.co/image/kept")
        );
    }

    #[tokio::test]
    async fn transfer_and_play_send_the_documented_requests() {
        let http = MockHttp::default();
        http.push(204, "");
        http.push(202, "");
        http.push(204, "");
        let access = Secret::new("a");
        transfer(&http, &access, "device1", true).await.unwrap();
        let id: String = "AbCd0123456789".chars().chain("xyzXYZ01".chars()).collect();
        let album = crate::link::parse(&format!("spotify:album:{id}")).unwrap();
        play(&http, &access, "device 1", &album).await.unwrap();
        let track = crate::link::parse(&format!("spotify:track:{id}")).unwrap();
        play(&http, &access, "device1", &track).await.unwrap();

        let seen = http.requests();
        assert!(seen.iter().all(|r| r.method == Method::Put && r.had_bearer));
        assert_eq!(seen[0].url, "https://api.spotify.com/v1/me/player");
        assert_eq!(
            seen[0].json,
            Some(serde_json::json!({"device_ids": ["device1"], "play": true}))
        );
        assert_eq!(
            seen[1].url,
            "https://api.spotify.com/v1/me/player/play?device_id=device+1"
        );
        assert_eq!(
            seen[1].json,
            Some(serde_json::json!({"context_uri": format!("spotify:album:{id}")}))
        );
        assert_eq!(
            seen[2].json,
            Some(serde_json::json!({"uris": [format!("spotify:track:{id}")]}))
        );
    }

    #[tokio::test]
    async fn shuffle_and_repeat_target_the_device_without_a_body() {
        let http = MockHttp::default();
        http.push(204, "");
        http.push(204, "");
        let access = Secret::new("a");
        shuffle(&http, &access, "device1", true).await.unwrap();
        repeat(&http, &access, "device1", RepeatMode::Context)
            .await
            .unwrap();
        let seen = http.requests();
        assert_eq!(
            seen[0].url,
            "https://api.spotify.com/v1/me/player/shuffle?state=true&device_id=device1"
        );
        assert_eq!(
            seen[1].url,
            "https://api.spotify.com/v1/me/player/repeat?state=context&device_id=device1"
        );
        assert!(seen
            .iter()
            .all(|r| r.method == Method::Put && r.json.is_none() && r.form.is_empty()));
    }

    #[tokio::test]
    async fn player_refusals_point_at_premium_or_the_device() {
        let http = MockHttp::default();
        http.push(403, "");
        http.push(404, "");
        http.push(401, "");
        let access = Secret::new("a");
        assert_eq!(
            transfer(&http, &access, "d", false).await.err(),
            Some(ApiError::Other(OraError::Forbidden {
                hint: ForbiddenHint::Premium
            }))
        );
        assert_eq!(
            transfer(&http, &access, "d", false).await.err(),
            Some(ApiError::Other(OraError::NoActiveDevice))
        );
        assert_eq!(
            transfer(&http, &access, "d", false).await.err(),
            Some(ApiError::Unauthorized)
        );
        assert_eq!(http.requests().len(), 3);
    }

    #[tokio::test]
    async fn statuses_map_to_errors() {
        let http = MockHttp::default();
        http.push(401, "");
        http.push(403, r#"{"error":{"status":403,"message":"x"}}"#);
        http.push(404, "");
        http.push(500, "");
        http.push_reply(Ok(Response {
            status: 429,
            retry_after_s: Some(7200),
            body: br#"{"error":{"status":429,"message":"m","reason":"QUOTA_EXCEEDED"}}"#.to_vec(),
        }));
        http.push_reply(Ok(Response {
            status: 429,
            retry_after_s: Some(3),
            body: Vec::new(),
        }));
        http.push_reply(Err(TransportError::Offline));
        let expected = [
            ApiError::Unauthorized,
            ApiError::Other(OraError::Forbidden {
                hint: ForbiddenHint::Allowlist,
            }),
            ApiError::Other(OraError::NotFound),
            ApiError::Other(OraError::Network),
            ApiError::Other(OraError::QuotaExceeded {
                retry_after_s: Some(7200),
            }),
            ApiError::Other(OraError::RateLimited {
                retry_after_s: Some(3),
            }),
            ApiError::Other(OraError::Offline),
        ];
        for expected in expected {
            assert_eq!(me(&http, &Secret::new("a")).await.err(), Some(expected));
        }
        // Every call was made exactly once: nothing here retries by itself.
        assert_eq!(http.requests().len(), 7);
    }
}
