//! Opening the system browser. Only two kinds of URL may ever be opened: Spotify's authorise
//! page and `open.spotify.com` pages for "Open in Spotify". There is no generic launcher.

use crate::oauth::AUTHORIZE_URL;
use url::Url;

/// The system has no browser (or refused to start it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserUnavailable;

pub trait Browser: Send + Sync + 'static {
    /// Opens a URL that has already been validated by this module.
    fn open(&self, url: &str) -> Result<(), BrowserUnavailable>;
}

pub struct SystemBrowser;

impl Browser for SystemBrowser {
    fn open(&self, url: &str) -> Result<(), BrowserUnavailable> {
        open::that_detached(url).map_err(|_| BrowserUnavailable)
    }
}

/// Spotify's authorise page with a query, and nothing else.
pub fn is_authorize_url(url: &str) -> bool {
    url.starts_with(&format!("{AUTHORIZE_URL}?"))
        && Url::parse(url)
            .is_ok_and(|u| u.host_str() == Some("accounts.spotify.com") && u.path() == "/authorize")
}

/// A normalised `https://open.spotify.com/...` URL, or `None` for anything else.
pub fn spotify_page(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let valid = parsed.scheme() == "https"
        && parsed.host_str() == Some("open.spotify.com")
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.path().len() > 1;
    valid.then(|| parsed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_spotify_pages_are_accepted() {
        assert_eq!(
            spotify_page("https://open.spotify.com/user/abc").as_deref(),
            Some("https://open.spotify.com/user/abc")
        );
        // Built at runtime so the userinfo fixture doesn't read as an email address.
        let with_userinfo = format!("https://user:pw{}open.spotify.com/user/abc", '@');
        for bad in [
            "http://open.spotify.com/user/abc",
            "https://open.spotify.com.evil.example/user",
            with_userinfo.as_str(),
            "https://open.spotify.com:8443/user/abc",
            "https://open.spotify.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "not a url",
        ] {
            assert_eq!(spotify_page(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_the_authorise_page_is_a_sign_in_url() {
        assert!(is_authorize_url(
            "https://accounts.spotify.com/authorize?response_type=code"
        ));
        assert!(!is_authorize_url("https://accounts.spotify.com/authorize"));
        assert!(!is_authorize_url(
            "https://accounts.spotify.com/api/token?x=1"
        ));
        assert!(!is_authorize_url(
            "https://accounts.spotify.com.evil.example/authorize?x=1"
        ));
    }
}
