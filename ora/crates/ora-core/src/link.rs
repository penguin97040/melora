//! Spotify links pasted by the user ("Share → Copy link") and `spotify:` URIs, reduced to a
//! validated URI. Nothing else from the input survives: no query, no tracking parameters.

use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Album,
    Artist,
    Playlist,
    Show,
    Track,
    Episode,
}

impl Kind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "album" => Kind::Album,
            "artist" => Kind::Artist,
            "playlist" => Kind::Playlist,
            "show" => Kind::Show,
            "track" => Kind::Track,
            "episode" => Kind::Episode,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Album => "album",
            Kind::Artist => "artist",
            Kind::Playlist => "playlist",
            Kind::Show => "show",
            Kind::Track => "track",
            Kind::Episode => "episode",
        }
    }
}

/// A validated `spotify:{kind}:{id}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyUri {
    pub kind: Kind,
    id: String,
}

impl SpotifyUri {
    pub fn uri(&self) -> String {
        format!("spotify:{}:{}", self.kind.name(), self.id)
    }

    /// The `open.spotify.com` page, for "Open in Spotify".
    pub fn page(&self) -> String {
        format!("https://open.spotify.com/{}/{}", self.kind.name(), self.id)
    }

    /// Albums, artists, playlists and shows play as a context; tracks and episodes as items.
    pub fn is_context(&self) -> bool {
        !matches!(self.kind, Kind::Track | Kind::Episode)
    }

    fn new(kind: &str, id: &str) -> Option<Self> {
        let kind = Kind::parse(kind)?;
        is_base62_id(id).then(|| SpotifyUri {
            kind,
            id: id.to_owned(),
        })
    }
}

/// Spotify IDs are 22 base-62 characters.
pub fn is_base62_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// `https://open.spotify.com/[intl-xx/]{kind}/{id}[?…]` or `spotify:{kind}:{id}`.
pub fn parse(input: &str) -> Option<SpotifyUri> {
    let input = input.trim();
    if input.len() > 512 {
        return None;
    }
    if let Some(rest) = input.strip_prefix("spotify:") {
        let mut parts = rest.split(':');
        let (kind, id) = (parts.next()?, parts.next()?);
        return if parts.next().is_none() {
            SpotifyUri::new(kind, id)
        } else {
            None
        };
    }
    let url = Url::parse(input).ok()?;
    let valid = url.scheme() == "https"
        && url.host_str() == Some("open.spotify.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !valid {
        return None;
    }
    let mut segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segments.first().is_some_and(|s| s.starts_with("intl-")) {
        segments.remove(0);
    }
    match segments.as_slice() {
        [kind, id] => SpotifyUri::new(kind, id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Built from a pattern so no fixture looks like a real Spotify ID.
    fn id() -> String {
        "AbCd0123456789".chars().chain("xyzXYZ01".chars()).collect()
    }

    #[test]
    fn links_and_uris_become_validated_uris() {
        let id = id();
        for (input, uri, context) in [
            (
                format!("https://open.spotify.com/album/{id}"),
                "album",
                true,
            ),
            (
                format!("https://open.spotify.com/playlist/{id}?si=abc123"),
                "playlist",
                true,
            ),
            (
                format!("https://open.spotify.com/intl-de/track/{id}"),
                "track",
                false,
            ),
            (format!("  spotify:artist:{id} "), "artist", true),
            (format!("spotify:episode:{id}"), "episode", false),
        ] {
            let parsed = parse(&input).unwrap_or_else(|| panic!("{input}"));
            assert_eq!(parsed.uri(), format!("spotify:{uri}:{id}"));
            assert_eq!(parsed.is_context(), context);
        }
    }

    #[test]
    fn anything_else_is_refused() {
        let id = id();
        let with_userinfo = format!("https://user{}open.spotify.com/album/{id}", '@');
        for bad in [
            format!("http://open.spotify.com/album/{id}"),
            format!("https://open.spotify.com.evil.example/album/{id}"),
            format!("https://open.spotify.com:8443/album/{id}"),
            with_userinfo,
            format!("https://open.spotify.com/user/{id}"),
            format!("https://open.spotify.com/album/{id}/extra"),
            "https://open.spotify.com/album/short".to_owned(),
            format!("https://open.spotify.com/album/{}", &id[..21]),
            format!("https://open.spotify.com/album/{}-", &id[..21]),
            format!("spotify:album:{id}:extra"),
            format!("spotify:user:{id}"),
            "javascript:alert(1)".to_owned(),
            "x".repeat(600),
        ] {
            assert_eq!(parse(&bad), None, "{bad}");
        }
    }
}
