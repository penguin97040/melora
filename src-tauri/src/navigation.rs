use tauri::Url;

// Only the bundled shell (or the exact development origin) can replace the UI.
// Fragments remain available for the keyboard skip link.
pub fn allowed(url: &Url, dev_url: Option<&Url>) -> bool {
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
            assert!(allowed(&Url::parse(source).unwrap(), None));
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
            assert!(!allowed(&Url::parse(source).unwrap(), None));
        }
        let mut user_info = Url::parse("http://tauri.localhost/").unwrap();
        user_info.set_username("synthetic").unwrap();
        assert!(!allowed(&user_info, None));
    }

    #[test]
    fn development_navigation_requires_exact_origin() {
        let dev = Url::parse("http://127.0.0.1:1420").unwrap();
        assert!(allowed(
            &Url::parse("http://127.0.0.1:1420/#content").unwrap(),
            Some(&dev)
        ));
        for source in [
            "http://127.0.0.1:5173/",
            "http://localhost:1420/",
            "https://127.0.0.1:1420/",
        ] {
            assert!(!allowed(&Url::parse(source).unwrap(), Some(&dev)));
        }
    }
}
