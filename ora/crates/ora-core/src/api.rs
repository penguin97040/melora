//! Spotify Web API calls and the mapping from HTTP replies to `OraError`. Domain models only
//! leave this module; raw DTOs stay private.

use crate::{
    http::{HttpClient, Method, Request, Response, TransportError},
    link::SpotifyUri,
    secret::Secret,
};
use ora_types::{
    DeviceKind, Disallows, ForbiddenHint, OraError, Profile, RepeatMode, SdkState, SdkTrack,
};
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
    player_send(http, access, Method::Put, url, body).await
}

async fn player_send<H: HttpClient>(
    http: &H,
    access: &Secret,
    method: Method,
    url: String,
    body: Option<serde_json::Value>,
) -> Result<(), ApiError> {
    let response = http
        .send(Request {
            method,
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

/// `PUT /me/player/pause?device_id=…`.
pub async fn pause<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
) -> Result<(), ApiError> {
    let url = format!("{API_BASE}/me/player/pause?device_id={}", encode(device_id));
    player_put(http, access, url, None).await
}

/// `PUT /me/player/play?device_id=…` with no body: carries on with the current item.
pub async fn resume<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
) -> Result<(), ApiError> {
    let url = format!("{API_BASE}/me/player/play?device_id={}", encode(device_id));
    player_put(http, access, url, None).await
}

/// `PUT /me/player/seek?position_ms=…&device_id=…`.
pub async fn seek<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    position_ms: u64,
) -> Result<(), ApiError> {
    let url = format!(
        "{API_BASE}/me/player/seek?position_ms={position_ms}&device_id={}",
        encode(device_id)
    );
    player_put(http, access, url, None).await
}

/// `PUT /me/player/volume?volume_percent=…&device_id=…`.
pub async fn volume<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
    percent: u8,
) -> Result<(), ApiError> {
    let url = format!(
        "{API_BASE}/me/player/volume?volume_percent={}&device_id={}",
        percent.min(100),
        encode(device_id)
    );
    player_put(http, access, url, None).await
}

/// `POST /me/player/next?device_id=…` (no body).
pub async fn next<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
) -> Result<(), ApiError> {
    let url = format!("{API_BASE}/me/player/next?device_id={}", encode(device_id));
    player_send(http, access, Method::Post, url, None).await
}

/// `POST /me/player/previous?device_id=…` (no body).
pub async fn previous<H: HttpClient>(
    http: &H,
    access: &Secret,
    device_id: &str,
) -> Result<(), ApiError> {
    let url = format!(
        "{API_BASE}/me/player/previous?device_id={}",
        encode(device_id)
    );
    player_send(http, access, Method::Post, url, None).await
}

/// A Spotify Connect device as the Web API reports it. The ID never leaves `ora-core`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiDevice {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub active: bool,
    pub restricted: bool,
    pub supports_volume: bool,
    pub volume_percent: Option<u8>,
}

/// `GET /me/player`: the active device and what it plays, in the SDK's state shape so both
/// sources are validated the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerState {
    pub device: ApiDevice,
    pub state: SdkState,
}

#[derive(Deserialize)]
struct DevicesBody {
    #[serde(default)]
    devices: Vec<DeviceBody>,
}

#[derive(Deserialize)]
struct DeviceBody {
    id: Option<String>,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_restricted: bool,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    volume_percent: Option<u32>,
    #[serde(default)]
    supports_volume: bool,
}

#[derive(Deserialize)]
struct PlayerBody {
    device: Option<DeviceBody>,
    #[serde(default)]
    is_playing: bool,
    progress_ms: Option<u64>,
    #[serde(default)]
    shuffle_state: bool,
    repeat_state: Option<String>,
    item: Option<ItemBody>,
    actions: Option<ActionsBody>,
}

/// The reference describes `actions` as the disallows object itself, while replies nest it in
/// `actions.disallows`; both are read.
#[derive(Deserialize)]
struct ActionsBody {
    disallows: Option<DisallowsBody>,
    #[serde(flatten)]
    direct: DisallowsBody,
}

#[derive(Default, Deserialize)]
struct DisallowsBody {
    #[serde(default)]
    pausing: bool,
    #[serde(default)]
    resuming: bool,
    #[serde(default)]
    seeking: bool,
    #[serde(default)]
    skipping_next: bool,
    #[serde(default)]
    skipping_prev: bool,
}

/// A track (`album`, `artists`) or an episode (`show`, `images`).
#[derive(Deserialize)]
struct ItemBody {
    uri: Option<String>,
    name: Option<String>,
    #[serde(default)]
    duration_ms: u64,
    #[serde(default)]
    artists: Vec<NamedBody>,
    album: Option<NamedBody>,
    show: Option<NamedBody>,
    #[serde(default)]
    images: Vec<ImageBody>,
}

#[derive(Deserialize)]
struct NamedBody {
    name: Option<String>,
    #[serde(default)]
    images: Vec<ImageBody>,
}

fn device_kind(kind: &str) -> DeviceKind {
    match kind.to_ascii_lowercase().as_str() {
        "computer" => DeviceKind::Computer,
        "smartphone" => DeviceKind::Smartphone,
        "tablet" => DeviceKind::Tablet,
        "speaker" | "avr" | "audiodongle" | "castaudio" => DeviceKind::Speaker,
        "tv" | "stb" | "castvideo" => DeviceKind::Tv,
        "gameconsole" => DeviceKind::GameConsole,
        "automobile" => DeviceKind::Automobile,
        _ => DeviceKind::Other,
    }
}

/// Connect device IDs are letters and digits; a device without a usable ID can't be chosen.
fn device(body: DeviceBody) -> Option<ApiDevice> {
    let id = body.id.filter(|id| {
        (1..=128).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })?;
    Some(ApiDevice {
        id,
        name: body.name.unwrap_or_default(),
        kind: device_kind(body.kind.as_deref().unwrap_or_default()),
        active: body.is_active,
        restricted: body.is_restricted,
        supports_volume: body.supports_volume,
        volume_percent: body.volume_percent.map(|v| v.min(100) as u8),
    })
}

/// The image closest to 300 px wide, from the allowed image host only.
fn pick_image(images: Vec<ImageBody>) -> Option<String> {
    images
        .into_iter()
        .filter(|image| is_allowed_image(&image.url))
        .min_by_key(|image| image.width.map_or(u32::MAX, |w| w.abs_diff(300)))
        .map(|image| image.url)
}

fn track(item: ItemBody) -> Option<SdkTrack> {
    let uri = item.uri?;
    let (artists, album, images) = match (item.album, item.show) {
        (Some(album), _) => (
            item.artists.into_iter().filter_map(|a| a.name).collect(),
            album.name.unwrap_or_default(),
            album.images,
        ),
        (None, Some(show)) => (
            show.name.into_iter().collect(),
            String::new(),
            if item.images.is_empty() {
                show.images
            } else {
                item.images
            },
        ),
        (None, None) => (Vec::new(), String::new(), item.images),
    };
    Some(SdkTrack {
        uri,
        name: item.name.unwrap_or_default(),
        artists,
        album,
        image_url: pick_image(images),
    })
}

async fn player_get<H: HttpClient>(
    http: &H,
    access: &Secret,
    url: String,
) -> Result<Response, ApiError> {
    let response = http
        .send(Request {
            method: Method::Get,
            url,
            bearer: Some(access.clone()),
            form: Vec::new(),
            json: None,
        })
        .await
        .map_err(|e| ApiError::Other(transport_error(e)))?;
    if matches!(response.status, 200 | 204) {
        Ok(response)
    } else {
        Err(player_error(&response))
    }
}

/// `GET /me/player/devices`. Devices without a usable ID are left out.
pub async fn devices<H: HttpClient>(http: &H, access: &Secret) -> Result<Vec<ApiDevice>, ApiError> {
    let response = player_get(http, access, format!("{API_BASE}/me/player/devices")).await?;
    if response.body.is_empty() {
        return Ok(Vec::new());
    }
    let body: DevicesBody =
        serde_json::from_slice(&response.body).map_err(|_| ApiError::Other(OraError::Network))?;
    Ok(body.devices.into_iter().filter_map(device).collect())
}

/// `GET /me/player?additional_types=episode`. `None` when no device is active (204).
pub async fn player_state<H: HttpClient>(
    http: &H,
    access: &Secret,
) -> Result<Option<PlayerState>, ApiError> {
    let url = format!("{API_BASE}/me/player?additional_types=episode");
    let response = player_get(http, access, url).await?;
    if response.status == 204 || response.body.is_empty() {
        return Ok(None);
    }
    let body: PlayerBody =
        serde_json::from_slice(&response.body).map_err(|_| ApiError::Other(OraError::Network))?;
    let Some(device) = body.device.and_then(device) else {
        return Ok(None);
    };
    let disallows = body.actions.map_or_else(Disallows::default, |actions| {
        let nested = actions.disallows.unwrap_or_default();
        let direct = actions.direct;
        Disallows {
            pause: nested.pausing || direct.pausing,
            resume: nested.resuming || direct.resuming,
            next: nested.skipping_next || direct.skipping_next,
            previous: nested.skipping_prev || direct.skipping_prev,
            seek: nested.seeking || direct.seeking,
        }
    });
    let duration_ms = body.item.as_ref().map_or(0, |item| item.duration_ms);
    Ok(Some(PlayerState {
        device,
        state: SdkState {
            paused: !body.is_playing,
            position_ms: body.progress_ms.unwrap_or(0),
            duration_ms,
            track: body.item.and_then(track),
            shuffle: body.shuffle_state,
            repeat: match body.repeat_state.as_deref() {
                Some("track") => RepeatMode::Track,
                Some("context") => RepeatMode::Context,
                _ => RepeatMode::Off,
            },
            disallows,
        },
    }))
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
    async fn remote_controls_send_the_documented_requests() {
        let http = MockHttp::default();
        for _ in 0..6 {
            http.push(204, "");
        }
        let access = Secret::new("a");
        pause(&http, &access, "d1").await.unwrap();
        resume(&http, &access, "d1").await.unwrap();
        seek(&http, &access, "d1", 61_000).await.unwrap();
        volume(&http, &access, "d1", 250).await.unwrap();
        next(&http, &access, "d1").await.unwrap();
        previous(&http, &access, "d1").await.unwrap();
        let seen = http.requests();
        let shape: Vec<(Method, &str)> = seen.iter().map(|r| (r.method, r.url.as_str())).collect();
        assert_eq!(
            shape,
            [
                (
                    Method::Put,
                    "https://api.spotify.com/v1/me/player/pause?device_id=d1"
                ),
                (
                    Method::Put,
                    "https://api.spotify.com/v1/me/player/play?device_id=d1"
                ),
                (
                    Method::Put,
                    "https://api.spotify.com/v1/me/player/seek?position_ms=61000&device_id=d1"
                ),
                (
                    Method::Put,
                    "https://api.spotify.com/v1/me/player/volume?volume_percent=100&device_id=d1"
                ),
                (
                    Method::Post,
                    "https://api.spotify.com/v1/me/player/next?device_id=d1"
                ),
                (
                    Method::Post,
                    "https://api.spotify.com/v1/me/player/previous?device_id=d1"
                ),
            ]
        );
        assert!(seen
            .iter()
            .all(|r| r.had_bearer && r.json.is_none() && r.form.is_empty()));
    }

    #[tokio::test]
    async fn devices_without_a_usable_id_are_left_out() {
        let http = MockHttp::default();
        http.push(
            200,
            r#"{"devices":[
                {"id":"abc123","is_active":true,"is_restricted":false,"name":"Kitchen",
                 "type":"Speaker","volume_percent":140,"supports_volume":true},
                {"id":null,"name":"Ghost","type":"Computer"},
                {"id":"bad id","name":"Space","type":"Computer"},
                {"id":"def456","is_restricted":true,"name":null,"type":"CastVideo"},
                {"id":"ghi789","name":"Car","type":"SomethingNew","volume_percent":null}
            ]}"#,
        );
        let list = devices(&http, &Secret::new("a")).await.unwrap();
        assert_eq!(
            list.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            ["abc123", "def456", "ghi789"]
        );
        assert_eq!(list[0].kind, DeviceKind::Speaker);
        assert!(list[0].active && list[0].supports_volume);
        assert_eq!(list[0].volume_percent, Some(100));
        assert!(list[1].restricted);
        assert_eq!(list[1].kind, DeviceKind::Tv);
        assert_eq!(list[1].name, "");
        assert_eq!(list[2].kind, DeviceKind::Other);
        assert_eq!(list[2].volume_percent, None);
        assert_eq!(
            http.requests()[0].url,
            "https://api.spotify.com/v1/me/player/devices"
        );
    }

    #[tokio::test]
    async fn playback_state_reads_tracks_episodes_and_nothing_playing() {
        let http = MockHttp::default();
        http.push(
            200,
            r#"{"device":{"id":"abc123","is_active":true,"name":"Phone","type":"Smartphone",
                 "supports_volume":false,"volume_percent":null},
               "is_playing":true,"progress_ms":5000,"shuffle_state":true,"repeat_state":"track",
               "actions":{"disallows":{"seeking":true,"skipping_prev":true}},
               "item":{"type":"track","uri":"spotify:track:x","name":"Song","duration_ms":200000,
                 "artists":[{"name":"A"},{"name":null}],
                 "album":{"name":"Album","images":[
                   {"url":"https://i.scdn.co/image/big","width":640},
                   {"url":"https://i.scdn.co/image/mid","width":300},
                   {"url":"https://evil.example/x","width":300}]}}}"#,
        );
        http.push(
            200,
            r#"{"device":{"id":"abc123","name":"Phone","type":"Smartphone"},
               "is_playing":false,"progress_ms":null,"repeat_state":"context",
               "actions":{"pausing":true},
               "item":{"type":"episode","uri":"spotify:episode:y","name":"Episode",
                 "duration_ms":90000,"images":[],
                 "show":{"name":"Show","images":[{"url":"https://i.scdn.co/image/show","width":64}]}}}"#,
        );
        http.push(204, "");
        http.push(200, r#"{"device":null,"is_playing":false}"#);
        let access = Secret::new("a");

        let reply = player_state(&http, &access).await.unwrap().unwrap();
        assert_eq!(reply.device.id, "abc123");
        assert_eq!(reply.device.kind, DeviceKind::Smartphone);
        let state = reply.state;
        assert!(!state.paused && state.shuffle);
        assert_eq!(state.repeat, RepeatMode::Track);
        assert_eq!((state.position_ms, state.duration_ms), (5000, 200_000));
        assert!(state.disallows.seek && state.disallows.previous && !state.disallows.next);
        let track = state.track.unwrap();
        assert_eq!(track.artists, ["A"]);
        assert_eq!(track.album, "Album");
        assert_eq!(
            track.image_url.as_deref(),
            Some("https://i.scdn.co/image/mid")
        );

        let state = player_state(&http, &access).await.unwrap().unwrap().state;
        assert!(state.paused && state.disallows.pause);
        assert_eq!(state.repeat, RepeatMode::Context);
        assert_eq!(state.position_ms, 0);
        let episode = state.track.unwrap();
        assert_eq!(episode.artists, ["Show"]);
        assert_eq!(
            episode.image_url.as_deref(),
            Some("https://i.scdn.co/image/show")
        );

        assert_eq!(player_state(&http, &access).await.unwrap(), None);
        assert_eq!(player_state(&http, &access).await.unwrap(), None);
        assert_eq!(
            http.requests()[0].url,
            "https://api.spotify.com/v1/me/player?additional_types=episode"
        );
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
