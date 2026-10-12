# Development

Use npm and `package-lock.json`; Rust dependencies use `src-tauri/Cargo.lock`. Application version is read from `package.json` by Tauri; keep the Cargo package version in sync.

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

No Spotify configuration is required for the preview shell.

## Checks

```sh
npm run check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo check --manifest-path src-tauri/Cargo.toml --locked
```

For automated native Linux acceptance, install `webkitgtk-webdriver` and Xvfb, build the native debug executable and run `python3 scripts/test-native.py /absolute/path/to/melora`. The checks use disposable application data and save local screenshots in `artifacts/runtime`.

The asset audit rejects private inputs and source maps in `dist` and unexpected native resources. Publication checks use a local pre-push hook and local destination/identity approval; run `npm run gate` explicitly before each push. The gate scans complete ancestry of outgoing commits, their trees and metadata, the index and publishable working files. It is deliberately conservative: unusual binaries, symlinks, submodules, unapproved identities or incomplete history block publication. Pattern checks require human review for unrecognised secrets and personal information.

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

The build disables installers and packages only the release executable, font licences, notices, readme and empty `data` folder. Frontend assets and fonts are embedded in the executable. The WebView2 loader is linked statically; the OS WebView2 Runtime is required. ZIP path and SHA-256 are printed and the checksum is saved beside the archive.

If Microsoft's SDK download is blocked, report the exact required hostname and widen the host network allowlist only for that host. Do not disable the network policy.

## Runtime acceptance

- Windows: extract to a writable folder; launch without installation; confirm native title controls, displayed version/platform and synthetic preview. Change settings, close and relaunch; inspect neighbouring `data/preferences.json`. Also test an unwritable folder and missing WebView2.
- Linux/macOS: confirm the same shell and IPC, then preference persistence in the platform app-data directory. macOS builds require a Mac.
- Keyboard: tab through navigation, playlists, track titles and settings; select with Enter; verify visible focus and the skip link.
- Layout: test narrow and wide windows, 125–200% scale and reduced motion; content should reflow and remain reachable.

This milestone contains no Spotify authentication or audio. Embedded playback is supported only after audible playback has been independently verified on each OS. Linux AppImage and macOS application bundle delivery follow in later milestones.
