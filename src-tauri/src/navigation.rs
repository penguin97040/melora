use tauri::Url;

// Only the bundled shell (or the exact development origin) can replace the UI.
// Fragments remain available for the keyboard skip link. On macOS and Linux this handler
// also sees sub-frames, so the Web Playback SDK's frame is allowed by its exact URL, and only
// once the capability probe found DRM (`sdk_frame`). Windows never reports sub-frames here.
pub fn allowed(url: &Url, dev_url: Option<&Url>, sdk_frame: bool) -> bool {
    if sdk_frame && url.as_str() == ora_tauri::SDK_FRAME_URL {
        return true;
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || !matches!(url.path(), "" | "/" | "/index.html")
    {
        return false;
    }
    if let Some(dev) = dev_url {
        return url.origin() == dev.origin();
    }
    url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http", Some("tauri.localhost"))
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_navigation_allows_shell_and_skip_link_only() {
        for source in [
            "tauri://localhost",
            "tauri://localhost/index.html",
            "http://tauri.localhost/#content",
        ] {
            assert!(allowed(&Url::parse(source).unwrap(), None, false));
        }
        for source in [
            "https://example.invalid/",
            "http://tauri.localhost.example.invalid/",
            "http://tauri.localhost:1420/",
            "tauri://other/index.html",
            "file:///index.html",
            "data:text/html,preview",
            "http://tauri.localhost/?redirect=preview",
            "http://tauri.localhost/other.html",
        ] {
            assert!(!allowed(&Url::parse(source).unwrap(), None, false));
        }
        let mut user_info = Url::parse("http://tauri.localhost/").unwrap();
        user_info.set_username("synthetic").unwrap();
        assert!(!allowed(&user_info, None, false));
    }

    #[test]
    fn the_sdk_frame_is_allowed_only_after_the_probe_and_only_exactly() {
        let frame = Url::parse("https://sdk.scdn.co/embedded/index.html").unwrap();
        assert!(!allowed(&frame, None, false));
        assert!(allowed(&frame, None, true));
        let dev = Url::parse("http://127.0.0.1:1420").unwrap();
        assert!(allowed(&frame, Some(&dev), true));
        let mut user_info = frame.clone();
        user_info.set_username("synthetic").unwrap();
        for source in [
            "http://sdk.scdn.co/embedded/index.html",
            "https://sdk.scdn.co:8443/embedded/index.html",
            "https://sdk.scdn.co/embedded/index.html?x=1",
            "https://sdk.scdn.co/embedded/other.html",
            "https://sdk.scdn.co/spotify-player.js",
            "https://sdk.scdn.co.example.invalid/embedded/index.html",
            "https://open.spotify.com/",
            user_info.as_str(),
        ] {
            assert!(
                !allowed(&Url::parse(source).unwrap(), None, true),
                "{source}"
            );
        }
    }

    #[test]
    fn development_navigation_requires_exact_origin() {
        let dev = Url::parse("http://127.0.0.1:1420").unwrap();
        assert!(allowed(
            &Url::parse("http://127.0.0.1:1420/#content").unwrap(),
            Some(&dev),
            false
        ));
        for source in [
            "http://127.0.0.1:5173/",
            "http://localhost:1420/",
            "https://127.0.0.1:1420/",
        ] {
            assert!(!allowed(&Url::parse(source).unwrap(), Some(&dev), false));
        }
    }
}
