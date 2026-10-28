# Development

Use npm and `package-lock.json`; Rust dependencies use the root Cargo workspace (`Cargo.toml`, `Cargo.lock`), with members `src-tauri` and the `ora` crates in `ora/crates/`. Application version is read from `package.json` by Tauri; keep the Cargo package version in sync.

```sh
npm ci
npm run check
npm run tauri dev
```

On a mounted filesystem that cannot create symlinks, use `npm ci --no-bin-links`. npm scripts use explicit Node tool paths. Keep `CARGO_TARGET_DIR` on a filesystem supporting native compiler outputs, for example `/tmp/melora-target` in a disposable Linux sandbox.

## Platform prerequisites

Consult [Tauri's current prerequisites](https://v2.tauri.app/start/prerequisites/) before installing packages.

- Windows: Rust MSVC toolchain, Visual Studio C++ Build Tools and system WebView2.
- Ubuntu/Debian: Rust, a C/C++ compiler, pkg-config, WebKitGTK 4.1 development headers, Ayatana AppIndicator development headers, librsvg and patchelf. Build on the oldest distribution baseline you intend to support.
- Fedora: corresponding WebKitGTK 4.1, C/C++, librsvg and AppIndicator development packages from Tauri's current prerequisite guide.
- Native Linux tests: Xvfb, xdotool, WebKitWebDriver, and `dbus-run-session` and `busctl` for the media controls.
- macOS: Rust and Xcode Command Line Tools. Build and test on a supported macOS version.

No Spotify configuration is required to run the app shell (signed out, it shows the welcome). To test sign-in, enter a Client ID in Settings, or set `MELORA_SPOTIFY_CLIENT_ID` for a development run (it overrides the saved value and can't be edited in the app). Never commit a real Client ID.

## Checks

```sh
npm run check
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --workspace --locked
```

`npm run check` also runs `scripts/check-ora-boundaries.sh`. It fails if `ora-core` gains a Tauri dependency, or if the TypeScript bindings in `src/native/bindings/` differ from what `cargo test -p ora-types` generates. The bindings are generated files: change the Rust types, regenerate, and stage the result.

For automated native Linux acceptance, install `webkitgtk-webdriver`, Xvfb and `xdotool`, build the native debug executable and run `python3 scripts/test-native.py /absolute/path/to/melora`. The checks use disposable application data and save local screenshots in `artifacts/runtime`.

To check opening bounds on a smaller simulated monitor, add `--opening-only --screen 1280x720`. Add `--device-scale 2` to simulate GTK scaling. These checks run without a window manager; decoration subtraction is covered by Rust tests. Actual OS DPI and decorated-window behaviour need testing on each desktop OS.

For repeated Linux startup measurements, build with `npm run tauri -- build --no-bundle` and run `python3 scripts/test-startup.py /absolute/path/to/release/melora`. This uses Xvfb and system libX11 to sample the hidden window, centred reveal and first-paint timing with default, restored and corrupt preferences. It records any blank samples before GTK's first paint and rejects subsequent blank frames or geometry changes. Xvfb has no compositor; verify visible flash behaviour and launch timing separately on real Windows, Linux and macOS desktops.

The asset audit rejects private inputs and source maps in `dist` and unexpected native resources. Publication checks use a local pre-push hook and local destination/identity approval; run `npm run gate` explicitly before each push. The gate scans complete ancestry of outgoing commits, their trees, paths and raw metadata, the index and publishable working files. It requires exactly one approved push destination and redacts sensitive filenames in diagnostics. It reads objects as a push sends them (replacement refs are ignored, and their presence blocks publication), and each path and blob is scanned once. It is deliberately conservative: unusual binaries (any file with NUL bytes outside the known font and icon formats), symlinks, submodules, unapproved identities or incomplete history block publication. Only the three font licences and the upstream notices `third-party/notices/sources.json` pins by SHA-256 may name an author's address. Pattern checks require human review for unrecognised secrets and personal information.

## Windows portable build

On Windows with the native MSVC toolchain:

```sh
npm run build:windows
```

On Linux or macOS, follow [Tauri's documented MSVC cross-compilation route](https://v2.tauri.app/distribute/windows-installer/#build-windows-apps-on-linux-and-macos): install LLVM, lld, `llvm-rc`, the Rust target and `cargo-xwin`.

```sh
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
npm run build:windows
```

The build disables installers and packages only the release executable, Melora's `LICENSE`, font licences, notices, readme and empty `data` folder. Frontend assets and fonts are embedded in the executable. The WebView2 loader is linked statically; the OS WebView2 Runtime is required. The collector writes a hashed notice inventory; packaging rejects missing, altered, extra or linked notice files and checks archive CRC and exact input bytes before replacing the ZIP. ZIP path and SHA-256 are printed and the checksum is saved beside the archive.

If Microsoft's SDK download is blocked, report the exact required hostname and widen the host network allowlist only for that host. Do not disable the network policy.

## Linux packages

On a Linux x86_64 host with Docker, Node and Rust (the host fetches the crates and npm packages; the build itself runs offline):

```sh
npm ci
npm run build:linux            # x86_64 and aarch64
npm run build:linux -- x86_64  # or one architecture
```

`scripts/build-linux.mjs` builds in a pinned Ubuntu 22.04 image (`scripts/linux/Dockerfile`, one per architecture), so the packages need glibc 2.35 or later. It produces `.deb`, `.rpm` and an AppImage for x86_64, and `.deb` and `.rpm` for aarch64, cross-compiled (Ubuntu's arm64 WebKitGTK); an aarch64 AppImage would need an ARM machine. Node, the Rust toolchain and the AppImage tools are downloaded once to `~/.cache/melora-linux-build` (`MELORA_LINUX_CACHE`) and checked against the SHA-256 in the script; the AppImage runtime is pinned too. The release build goes to `target/linux` (`MELORA_LINUX_TARGET_DIR`).

The packages hold `/usr/bin/melora`, `Melora.desktop`, the icons and the licences in `/usr/share/doc/melora/licences` (Melora's `LICENSE` and the notices collected for the Linux target). `scripts/package-linux.py` then audits each one inside the container and copies it, with its `.sha256`, to `artifacts/linux/`. The audit checks the exact file list and bytes, the package's dependencies (WebKitGTK 4.1, GTK 3 and the CA certificates only), the desktop entry, the libraries the binary links, glibc 2.35 or older, 16 KiB page alignment on aarch64 (Fedora Asahi Remix) and that no private or account file is inside. The AppImage bundles WebKitGTK from Ubuntu 22.04, so prefer the distribution's package where there is one. It carries those libraries' notices too: after a first bundling, `package-linux.py appimage-notices` asks dpkg which packages own each bundled file and collects the copyright files linuxdeploy didn't copy, and the AppImage is bundled again with them in `usr/share/doc/<package>/copyright`, plus the pinned notices of its runtime (type2-runtime with libfuse, squashfuse, musl, zstd and zlib) and of AppRun and linuxdeploy's GTK plugin in `usr/share/doc/melora/appimage`. Its audit fails if any bundled file has no package, any package has no copyright file, or the runtime isn't the pinned one.

## Versions

Melora is pre-1.0. The middle number is the last finished development phase (0.5 after Phase 5, 0.6 after Phase 6), and the last number counts smaller additions within it (0.5.1 added the podcast skip). 1.0.0 marks the first complete release. The version is set in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `Cargo.lock` and the browser-preview fallback in `src/native/client.ts`; `scripts/tests/test_version.py` fails if they disagree. The `ora` crates keep their own version.

## Runtime acceptance

- Windows: extract to a writable folder; launch without installation; confirm integrated title controls, displayed version/platform and the signed-out welcome. Change settings, close and relaunch; inspect neighbouring `data/preferences.json`. Also test an unwritable folder and missing WebView2.
- Linux/macOS: confirm the same shell and IPC, then preference persistence in the platform app-data directory. macOS builds require a Mac.
- Keyboard: tab through navigation, playlists, track titles, shuffle, repeat and settings; select with Enter or Space; verify visible focus and the skip link. Shuffle toggles and repeat cycles Off → All → Song → Off, retaining selections across navigation and resetting on relaunch.
- Startup: the native window starts hidden. After local preference loading settles and the shell commits, `show_main_window` finishes sizing/centring and reveals it once. Failed preference loading still reveals the shell with an error. Confirm repeated launches reveal a centred shell without a visible move, and that resizing remains unchanged after duplicate readiness calls.
- Layout: opening size prefers 1440 × 940 logical pixels, bounded by the current monitor's work area after native decorations. Artwork shrinks with the remaining player height to a 6rem minimum before the pane scrolls; text and controls retain their sizes. Stacked narrow layouts keep width-based square artwork. At 100% interface scale the full player fits at the preferred size. Test shorter windows, 125–200% scale, wrapping titles and reduced motion; scroll to all details and verify complete control focus outlines stay visible.

Spotify sign-in (Authorization Code with PKCE, a one-shot `127.0.0.1` callback listener and the OS credential store) is implemented in `ora/crates/ora-core`. Signing in again for an extra permission keeps the current sign-in working until the new one is ready (`consenting` in the status), and Cancel can't stop it once Spotify's callback has arrived; its tests mock HTTP and the store and make no network calls. Real sign-in, persistence across relaunch and sign-out need a Windows, macOS or Linux desktop with a credential store and a browser. Embedded playback uses Spotify's Web Playback SDK through `ora/sdk-bridge`, loaded only after a DRM capability probe passes. Audible playback has been confirmed on Windows (WebView2); WebKitGTK on Linux reports no DRM, so the SDK isn't loaded there. Embedded playback is supported only after audible playback has been independently verified on each OS. Other Spotify devices are listed and controlled through the Web API from Rust (`GET /me/player`, `/me/player/devices` and the player commands with `device_id`), so the device picker also works where embedded playback isn't available. The UI gets opaque device keys, never device IDs. The player's **+** saves or removes the playing track through `/me/library` (asking for the library scopes on first use); whether it's liked is kept only while that track plays. The sidebar's playlists and Liked Songs come from `GET /me/playlists`, `/playlists/{id}/items` and `/me/tracks` (`ora-core` `library.rs`), 50 per page, loaded as the list scrolls and cached in memory per account (`cache.rs`: playlists 5 minutes, Liked Songs 1 minute, identical requests shared). Selecting a row plays `PUT /me/player/play` on the active device or Melora's own: a playlist as the context from the row's position, Liked Songs as up to 100 track URIs. Search (`ora-core` `search.rs`) asks `GET /search` 250 ms after typing stops, for the first 10 tracks, artists, albums, playlists and episodes at once; **See all** pages one kind 10 at a time up to Spotify's 1,000 results. A newer search drops the older one's request (it returns `Cancelled`, which the UI ignores; token refreshes are never cut short), and results are cached per account and query for 3 minutes. Songs play as track URIs from the chosen one, episodes alone, and artists, albums and playlists as contexts; a podcast (show) opens its page. Saved albums, shows and episodes come from `GET /me/albums`, `/me/shows` and `/me/episodes`, and an album's or show's page from `/albums/{id}/tracks` and `/shows/{id}/episodes` (cached 1 minute for saved lists, 5 for show episodes, 30 for album tracks). Saved albums come 20 at a time, because each carries all its tracks; a Web API reply may be up to 8 MiB, and a larger one fails as `responseTooLarge`. Saving and removing go through `PUT`/`DELETE /me/library`, and whether items are saved through `GET /me/library/contains`, both 40 URIs at a time; answers are remembered for a minute per account (`library.rs`), an answer older than the item's latest save or removal is dropped, and a change forgets the saved lists it affects and updates the player's **+**. Lists of 150 rows or more render only the rows near the screen (`useWindow` in `src/features/library.ts`), re-measuring on any scroll because the table scrolls in a different container per layout. The queue comes from `GET /me/player/queue`, read when the Queue view opens, when the playing item changes and on Refresh (kept 2 seconds so identical reads share a request); Spotify has no API to reorder or remove from it, so Melora shows its rows without controls. **Add to queue** (a row's context menu) sends `POST /me/player/queue` to the device playing now, or refuses with "No Spotify device is active" when nothing plays. Recently played is one page of `GET /me/player/recently-played?limit=50` (Spotify's limit), cached 30 seconds. Home reads only lists whose permission is already granted and offers an Allow link for the rest. Playlists the account owns or collaborates on can be changed: **+ New** creates a private playlist (`POST /me/playlists`), **Add to playlist…** appends with `POST /playlists/{id}/items` (100 URIs per request, in the body), removing sends `DELETE /playlists/{id}/items` (Spotify removes every copy of a URI), and moving one row sends `PUT /playlists/{id}/items` with `range_start` and `insert_before`. Changes take turns, show at once, and are put back (the list reloads) if Spotify refuses; any change forgets the cached playlists and their items. Rows and sidebar playlists move by dragging (pointer events, not HTML drag and drop, which the WebView can hand to the system for file drops), Alt+↑/↓ or their menu. Melora's own sidebar order is a list of playlist IDs per account in `playlist-order.json` beside `spotify.json` (`ora-core` `order.rs`); playlists not in it follow in Spotify's order, and Spotify's own order is never changed. Media keys and the system's media controls are published from Rust (`ora-tauri` `media.rs`): Windows' `SystemMediaTransportControls` for the main window, and MPRIS (`org.mpris.MediaPlayer2.melora`, over `zbus`) on Linux's session bus. `ora-core` `media.rs` decides what they show (the playback status of whichever device plays) and what each key sends: the same `PlayerCommand` as the player's button, so it reaches Melora's SDK or the remote device alike. In an episode, Next and Previous seek 15 seconds; a control Spotify disallows does nothing. WebView2's own media entry is switched off (wry's default `--disable-features` plus `HardwareMediaKeyHandling`) and the SDK's `enableMediaSession` is false, so Melora appears once. The system fetches the artwork itself, from `i.scdn.co`. The native Linux suite checks MPRIS on a private session bus. The Spotify logo and Like icons in `src/assets/brand` are Spotify's unmodified files (see `THIRD_PARTY_NOTICES.md`): don't edit, recolour or inline them. The Linux packages install `Melora.desktop`, which MPRIS names as its `DesktopEntry`; macOS is on hold (README, Platforms).

The header shares the window title area. Windows/Linux use a resizable undecorated
window; macOS overlays the decorated title bar with native traffic lights and a
hidden native title. Browser previews disable window controls. Test header dragging,
double-click behaviour, minimise/maximise/restore/close, snapping and all eight resize
edges/corners. Native resize events must update the maximise icon and disable resize
targets while maximised. Confirm keyboard focus and restored display preferences.
Run `python3 scripts/test-native.py /path/to/melora --chrome-only` to exercise
controls, dragging/double-clicking and all resize edges under Openbox/Xvfb.
Actual desktop checks are required on Windows, macOS, Linux X11 and Wayland;
headless WebKit acceptance and cross compilation do not establish those behaviours.

## Dependency review

The 13 October 2026 review found no npm advisories. RustSec reports two notices in
Tauri's Linux GTK dependency chain: [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)
for `glib` 0.18.5 string-variant iterator unsoundness and
[RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html)
for the unmaintained `proc-macro-error` 1.0.4 build dependency. The pinned Tauri/GTK
versions require that GLib series, whose official 0.18 branch retains the affected
code. Melora does not use that iterator directly. A compatible upstream fix remains
outstanding; these notices have not been suppressed. Recheck the dependency graph
and advisory database before releases.

App-defined IPC commands have generated permissions granted only to the local main
window. Native navigation permits the bundled shell and its fragment links, or the
exact configured development origin in a development build; remote navigation and
popups are denied. Preferences use bounded regular-file reads, exclusive temporary
files and a synced atomic replacement. Startup probes folder writability without
rewriting existing settings. These protections do not claim to isolate filesystem
operations from a hostile process already running with the same user's permissions.
