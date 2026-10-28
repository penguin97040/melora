# Third-party notices

Bundled fonts are unmodified Latin WOFF2 assets distributed through Fontsource:

| Font | Use | Licence |
| --- | --- | --- |
| Share Tech Mono | Technical labels | SIL Open Font License 1.1 |
| Chakra Petch Medium | Headings | SIL Open Font License 1.1 |
| Ubuntu Mono | Track lists | Ubuntu Font Licence 1.0 |

Original font licence and copyright notices are in `public/fonts/licences` and accompany portable builds in `licences/fonts`. Ubuntu Mono is copyright Canonical Ltd. Font licences apply to the fonts only; Melora's own licence is in `LICENSE`.

## Spotify brand assets

The Spotify logo and the Like (+) icon are trademarks and assets of Spotify AB. Melora is unofficial and not affiliated with or endorsed by Spotify. They are used, unmodified, only to attribute Spotify content and to save tracks to Liked Songs, as Spotify's Design & Branding Guidelines require (https://developer.spotify.com/documentation/design), under the Spotify Developer Terms (https://developer.spotify.com/terms). They are not covered by any licence of Melora's and may not be reused apart from it.

| File in `src/assets/brand` | Source download (23/10/2026) | SHA-256 |
| --- | --- | --- |
| `Full_Logo_White_RGB.svg` | `2024-spotify-full-logo.zip` | `31cdfcdd58d3533a32d287267a1c404f376749b1fc4da99e4baa2233684f053c` |
| `like-icon-like.svg` | `like-icon-svg.zip` | `e26c263137d9ea1e75082a3937745d1c030c4a2878605a443492aac905160607` |
| `like-icon-liked.svg` | `like-icon-svg.zip` | `c44b6b5e397af23516c19fe9e3508e679c9e44b2b664b9fe1d0e88ee45e66cab` |

## Libraries

React, React DOM and their scheduler use the MIT licence. Tauri, serde and serde_json use MIT OR Apache-2.0. Transitive dependency licences and original notices are collected in the portable archive by the build process. Build tooling is not shipped as an application runtime.

On Windows, and in the Linux `.deb` and `.rpm` packages, the WebView (WebView2, WebKitGTK) and the libraries it uses come from the operating system. The Linux AppImage instead carries its own copies of WebKitGTK, GTK, GStreamer and the libraries they use, taken unmodified from Ubuntu 22.04. Most are under the GNU LGPL; each package's copyright file and licence terms are in the AppImage at `usr/share/doc/<package>/copyright`, and the corresponding sources are available from Ubuntu. The AppImage's runtime (which contains libfuse, squashfuse, musl, zstd and zlib) and its start-up scripts (AppImageKit's AppRun, linuxdeploy and its GTK plugin) have their notices in `usr/share/doc/melora/appimage`.

Melora's own source is licensed under the MIT License (`LICENSE`). It doesn't cover the fonts, the Spotify brand assets or the libraries above, which keep their own licences.
