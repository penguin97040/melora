# Development

Use npm and `package-lock.json`; Rust dependencies use the root Cargo workspace (`Cargo.toml`, `Cargo.lock`), with members `src-tauri` and the shared crates in `ora/crates/`. Application version is read from `package.json` by Tauri; keep the Cargo package version in sync.

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
- macOS: Rust and Xcode Command Line Tools. Build and test on a supported macOS version.

No Spotify configuration is required for the preview shell. To test sign-in, enter a Client ID in Settings, or set `MELORA_SPOTIFY_CLIENT_ID` for a development run (it overrides the saved value and can't be edited in the app). Never commit a real Client ID.

## Checks

```sh
npm run check
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo check --workspace --locked
```

`npm run check` also runs `scripts/check-ora-boundaries.sh`, which needs the `wasm32-unknown-unknown` Rust target (`rustup target add wasm32-unknown-unknown`). It fails if `ora-core` gains a Tauri dependency, if `ora-types` stops building for WebAssembly, or if the TypeScript bindings in `src/native/bindings/` differ from what `cargo test -p ora-types` generates. The bindings are generated files: change the Rust types, regenerate, and stage the result.

For automated native Linux acceptance, install `webkitgtk-webdriver`, Xvfb and `xdotool`, build the native debug executable and run `python3 scripts/test-native.py /absolute/path/to/melora`. The checks use disposable application data and save local screenshots in `artifacts/runtime`.

To check opening bounds on a smaller simulated monitor, add `--opening-only --screen 1280x720`. Add `--device-scale 2` to simulate GTK scaling. These checks run without a window manager; decoration subtraction is covered by Rust tests. Actual OS DPI and decorated-window behaviour need testing on each desktop OS.

For repeated Linux startup measurements, build with `npm run tauri -- build --no-bundle` and run `python3 scripts/test-startup.py /absolute/path/to/release/melora`. This uses Xvfb and system libX11 to sample the hidden window, centred reveal and first-paint timing with default, restored and corrupt preferences. It records any blank samples before GTK's first paint and rejects subsequent blank frames or geometry changes. Xvfb has no compositor; verify visible flash behaviour and launch timing separately on real Windows, Linux and macOS desktops.

The asset audit rejects private inputs and source maps in `dist` and unexpected native resources. Publication checks use a local pre-push hook and local destination/identity approval; run `npm run gate` explicitly before each push. The gate scans complete ancestry of outgoing commits, their trees, paths and raw metadata, the index and publishable working files. It requires exactly one approved push destination and redacts sensitive filenames in diagnostics. It is deliberately conservative: unusual binaries, symlinks, submodules, unapproved identities or incomplete history block publication. Pattern checks require human review for unrecognised secrets and personal information.

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

The build disables installers and packages only the release executable, font licences, notices, readme and empty `data` folder. Frontend assets and fonts are embedded in the executable. The WebView2 loader is linked statically; the OS WebView2 Runtime is required. The collector writes a hashed notice inventory; packaging rejects missing, altered, extra or linked notice files and checks archive CRC and exact input bytes before replacing the ZIP. ZIP path and SHA-256 are printed and the checksum is saved beside the archive.

If Microsoft's SDK download is blocked, report the exact required hostname and widen the host network allowlist only for that host. Do not disable the network policy.

## Runtime acceptance

- Windows: extract to a writable folder; launch without installation; confirm integrated title controls, displayed version/platform and synthetic preview. Change settings, close and relaunch; inspect neighbouring `data/preferences.json`. Also test an unwritable folder and missing WebView2.
- Linux/macOS: confirm the same shell and IPC, then preference persistence in the platform app-data directory. macOS builds require a Mac.
- Keyboard: tab through navigation, playlists, track titles, shuffle, repeat and settings; select with Enter or Space; verify visible focus and the skip link. Shuffle toggles and repeat cycles Off → All → Song → Off, retaining selections across navigation and resetting on relaunch.
- Startup: the native window starts hidden. After local preference loading settles and the shell commits, `show_main_window` finishes sizing/centring and reveals it once. Failed preference loading still reveals the shell with an error. Confirm repeated launches reveal a centred shell without a visible move, and that resizing remains unchanged after duplicate readiness calls.
- Layout: opening size prefers 1440 × 940 logical pixels, bounded by the current monitor's work area after native decorations. Artwork shrinks with the remaining player height to a 6rem minimum before the pane scrolls; text and controls retain their sizes. Stacked narrow layouts keep width-based square artwork. At 100% interface scale the full player fits at the preferred size. Test shorter windows, 125–200% scale, wrapping titles and reduced motion; scroll to all details and verify complete control focus outlines stay visible.

Spotify sign-in (Authorization Code with PKCE, a one-shot `127.0.0.1` callback listener and the OS credential store) is implemented in `ora/crates/ora-core`; its tests mock HTTP and the store and make no network calls. Real sign-in, persistence across relaunch and sign-out need a Windows, macOS or Linux desktop with a credential store and a browser. Embedded playback uses Spotify's Web Playback SDK through the shared `ora/sdk-bridge`, loaded only after a DRM capability probe passes. Audible playback has been confirmed on Windows (WebView2); WebKitGTK on Linux reports no DRM, so the SDK isn't loaded there. Embedded playback is supported only after audible playback has been independently verified on each OS. Linux AppImage and macOS application bundle delivery follow in later milestones.

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
