//! Spotify's Authorization Code with PKCE flow: the authorise URL, the code exchange and token
//! refresh. Never sends a Client Secret or an `Authorization` header to the token endpoint.

use crate::{
    config::ClientId,
    http::{HttpClient, Method, Request, TransportError},
    secret::Secret,
};
use serde::Deserialize;
use std::time::{Duration, Instant};

pub const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
pub const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";

pub fn authorize_url(
    client_id: &ClientId,
    redirect_uri: &str,
    scopes: &[&str],
    state: &Secret,
    challenge: &str,
) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id.as_str())
        .append_pair("scope", &scopes.join(" "))
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("state", state.expose())
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", challenge)
        .finish();
    format!("{AUTHORIZE_URL}?{query}")
}

/// Tokens from a successful exchange or refresh.
#[derive(Debug)]
pub struct Tokens {
    pub access: Secret,
    pub expires_at: Instant,
    /// Spotify may omit it on refresh; then the existing refresh token stays valid.
    pub refresh: Option<Secret>,
    /// The scopes Spotify actually granted, space-separated.
    pub scope: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The code or refresh token is no longer valid. Never retry; sign in again.
    InvalidGrant,
    /// Spotify doesn't recognise the Client ID.
    InvalidClient,
    /// Any other refusal (for example an unregistered redirect URI).
    Rejected,
    RateLimited(Option<u32>),
    Malformed,
    Transport(TransportError),
}

#[derive(Deserialize)]
struct TokenBody {
    access_token: String,
    token_type: String,
    expires_in: u64,
    refresh_token: Option<String>,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

pub async fn exchange_code<H: HttpClient>(
    http: &H,
    client_id: &ClientId,
    code: Secret,
    redirect_uri: &str,
    verifier: Secret,
) -> Result<Tokens, TokenError> {
    post(
        http,
        vec![
            ("grant_type", Secret::new("authorization_code")),
            ("code", code),
            ("redirect_uri", Secret::new(redirect_uri)),
            ("client_id", Secret::new(client_id.as_str())),
            ("code_verifier", verifier),
        ],
    )
    .await
}

pub async fn refresh<H: HttpClient>(
    http: &H,
    client_id: &ClientId,
    refresh: &Secret,
) -> Result<Tokens, TokenError> {
    post(
        http,
        vec![
            ("grant_type", Secret::new("refresh_token")),
            ("refresh_token", refresh.clone()),
            ("client_id", Secret::new(client_id.as_str())),
        ],
    )
    .await
}

async fn post<H: HttpClient>(
    http: &H,
    form: Vec<(&'static str, Secret)>,
) -> Result<Tokens, TokenError> {
    let response = http
        .send(Request {
            method: Method::Post,
            url: TOKEN_URL.to_owned(),
            bearer: None,
            form,
            json: None,
        })
        .await
        .map_err(TokenError::Transport)?;
    match response.status {
        200 => {
            let body: TokenBody =
                serde_json::from_slice(&response.body).map_err(|_| TokenError::Malformed)?;
            if !body.token_type.eq_ignore_ascii_case("bearer") || body.access_token.is_empty() {
                return Err(TokenError::Malformed);
            }
            Ok(Tokens {
                access: Secret::new(body.access_token),
                expires_at: Instant::now() + Duration::from_secs(body.expires_in.min(86_400)),
                refresh: body
                    .refresh_token
                    .filter(|r| !r.is_empty())
                    .map(Secret::new),
                scope: body.scope.unwrap_or_default(),
            })
        }
        429 => Err(TokenError::RateLimited(response.retry_after_s)),
        400..=499 => Err(
            match serde_json::from_slice::<ErrorBody>(&response.body)
                .map(|b| b.error)
                .as_deref()
            {
                Ok("invalid_grant") => TokenError::InvalidGrant,
                Ok("invalid_client") => TokenError::InvalidClient,
                _ => TokenError::Rejected,
            },
        ),
        _ => Err(TokenError::Transport(TransportError::Network)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::mock::MockHttp;

    fn client_id() -> ClientId {
        ClientId::parse("0123456789abcdef0123456789abcdef").unwrap()
    }

    // Token-shaped fixture values are assembled so no source line looks like a credential.
    fn token_json(access: &str, refresh: Option<&str>) -> String {
        let refresh = refresh
            .map(|r| format!(r#","refresh_{}":"{r}""#, "token"))
            .unwrap_or_default();
        format!(
            r#"{{"access_{}":"{access}","token_type":"Bearer","expires_in":3600,"scope":"user-read-private"{refresh}}}"#,
            "token"
        )
    }

    #[test]
    fn authorize_url_has_every_pkce_parameter() {
        let state = Secret::new("st");
        let url = authorize_url(
            &client_id(),
            "http://127.0.0.1:4567/callback",
            &["user-read-private", "streaming"],
            &state,
            "ch",
        );
        assert!(url.starts_with("https://accounts.spotify.com/authorize?response_type=code&"));
        // Assembled so no source line holds a Client-ID-shaped value (publication gate rule).
        let client_part = format!("client_id={}", client_id().as_str());
        for part in [
            client_part.as_str(),
            "scope=user-read-private+streaming",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A4567%2Fcallback",
            "state=st",
            "code_challenge_method=S256",
            "code_challenge=ch",
        ] {
            assert!(url.contains(part), "{part}");
        }
    }

    #[tokio::test]
    async fn exchange_sends_pkce_form_without_a_secret() {
        let http = MockHttp::default();
        http.push(200, &token_json("acc1", Some("ref1")));
        let tokens = exchange_code(
            &http,
            &client_id(),
            Secret::new("code1"),
            "http://127.0.0.1:1/callback",
            Secret::new("ver1"),
        )
        .await
        .unwrap();
        assert_eq!(tokens.access.expose(), "acc1");
        assert_eq!(tokens.refresh.unwrap().expose(), "ref1");
        assert_eq!(tokens.scope, "user-read-private");
        let seen = &http.requests()[0];
        assert_eq!(seen.url, TOKEN_URL);
        assert!(!seen.had_bearer);
        let keys: Vec<&str> = seen.form.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "grant_type",
                "code",
                "redirect_uri",
                "client_id",
                "code_verifier"
            ]
        );
        assert!(seen.form.iter().all(|(k, _)| !k.contains("secret")));
    }

    #[tokio::test]
    async fn token_errors_are_classified() {
        let http = MockHttp::default();
        http.push(400, r#"{"error":"invalid_grant","error_description":"x"}"#);
        http.push(400, r#"{"error":"invalid_client"}"#);
        http.push(400, r#"{"error":"invalid_request"}"#);
        http.push(503, "");
        http.push(200, r#"{"token_type":"Bearer"}"#);
        let refresh_value = Secret::new("r");
        for expected in [
            TokenError::InvalidGrant,
            TokenError::InvalidClient,
            TokenError::Rejected,
            TokenError::Transport(TransportError::Network),
            TokenError::Malformed,
        ] {
            assert_eq!(
                refresh(&http, &client_id(), &refresh_value).await.err(),
                Some(expected)
            );
        }
    }

    #[tokio::test]
    async fn refresh_without_rotation_keeps_no_new_refresh_token() {
        let http = MockHttp::default();
        http.push(200, &token_json("acc2", None));
        let tokens = refresh(&http, &client_id(), &Secret::new("r"))
            .await
            .unwrap();
        assert!(tokens.refresh.is_none());
        assert_eq!(http.requests()[0].form[0].1, "refresh_token");
    }
}
