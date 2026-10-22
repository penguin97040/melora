//! Sign-in state as the UI sees it. Nothing here ever holds a token or a verifier.

use crate::{error::OraError, features::Feature};
use serde::{Deserialize, Serialize};

/// The signed-in Spotify account, mapped from `GET /me`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Profile {
    /// Stable account identity (`account_id`). Prefer it over `id` for comparisons.
    pub account_id: String,
    pub id: String,
    pub display_name: Option<String>,
    /// The smallest profile image Spotify offers, if any.
    pub image_url: Option<String>,
    /// `external_urls.spotify`, for "Open in Spotify".
    pub spotify_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum AuthState {
    /// No Client ID has been set up yet.
    NotConfigured,
    SignedOut,
    /// Waiting for the user to approve access in their browser.
    SigningIn,
    SignedIn {
        /// `None` until `GET /me` has answered.
        profile: Option<Profile>,
        /// The OS credential store was unavailable, so the session ends when the app closes.
        #[serde(rename = "sessionOnly")]
        session_only: bool,
        /// Features whose scopes this sign-in covers. Others need `sign_in` with that feature,
        /// which asks Spotify for the extra permission.
        granted: Vec<Feature>,
    },
    /// Spotify no longer accepts the saved sign-in (for example after six months).
    SessionExpired,
}

/// Everything the UI needs to show the account state, pushed on every change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub struct Status {
    pub auth: AuthState,
    /// The Client ID comes from a development environment variable and can't be edited.
    pub client_id_from_environment: bool,
    /// A short masked form of the configured Client ID (first and last four characters).
    pub client_id_hint: Option<String>,
    /// The exact redirect URI to register in the Spotify Developer Dashboard.
    pub redirect_uri: String,
    /// The most recent failure worth showing, cleared by the next successful action.
    pub last_error: Option<OraError>,
}
