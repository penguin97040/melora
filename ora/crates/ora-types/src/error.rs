//! Errors that cross into the UI. They are safe to serialise and log: no tokens, codes,
//! verifiers, URLs with secrets or raw server messages.

use crate::features::Feature;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum ForbiddenHint {
    /// A 403 straight after signing in usually means the account isn't on the app's User
    /// Management allowlist in the Spotify Developer Dashboard.
    Allowlist,
    Premium,
    Restricted,
    NotOwnerPlaylist,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum SignInFailure {
    /// The browser could not be opened.
    BrowserUnavailable,
    /// Nobody completed the approval page within five minutes. This is also what an
    /// unregistered redirect URI looks like: Spotify shows its own error and never calls back.
    TimedOut,
    /// Spotify rejected the Client ID (`invalid_client`).
    InvalidClient,
    /// The callback was malformed or didn't match this sign-in.
    InvalidCallback,
    /// Another program is using the sign-in callback port.
    CallbackPortBusy,
    /// Spotify refused the authorisation code exchange.
    ExchangeRejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub enum OraError {
    SignedOut,
    /// The refresh token was rejected (`invalid_grant`); the user must sign in again.
    SessionExpired,
    NeedsScope {
        feature: Feature,
    },
    Forbidden {
        hint: ForbiddenHint,
    },
    RateLimited {
        #[serde(rename = "retryAfterS")]
        retry_after_s: Option<u32>,
    },
    /// Spotify's `QUOTA_EXCEEDED`: never retried automatically.
    QuotaExceeded {
        #[serde(rename = "retryAfterS")]
        retry_after_s: Option<u32>,
    },
    NotFound,
    NoActiveDevice,
    Offline,
    Network,
    Cancelled,
    /// The OS credential store could not be used.
    StorageUnavailable,
    /// No Client ID has been set up.
    NotConfigured,
    /// The Client ID doesn't look like one (20 to 64 letters and digits).
    InvalidClientId,
    /// The settings file could not be written.
    SettingsUnwritable,
    SignIn {
        reason: SignInFailure,
    },
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialise_with_camel_case_tags() {
        let value = serde_json::to_value(OraError::QuotaExceeded {
            retry_after_s: Some(30),
        })
        .ok();
        assert_eq!(
            value,
            Some(serde_json::json!({"kind": "quotaExceeded", "retryAfterS": 30}))
        );
        let value = serde_json::to_value(OraError::SignIn {
            reason: SignInFailure::TimedOut,
        })
        .ok();
        assert_eq!(
            value,
            Some(serde_json::json!({"kind": "signIn", "reason": "TimedOut"}))
        );
    }
}
