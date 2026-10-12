# Melora

Melora is a personal desktop music client built with Tauri 2, Rust, React and TypeScript. Windows, Linux and macOS are intended targets.

The first build is a local design preview with invented track titles, artists and artwork. You can browse preview playlists, search, select tracks and adjust display preferences. Shuffle and repeat let you preview mode selections; they leave track order unchanged and reset on relaunch. Spotify sign-in and playback are not implemented yet. Melora is unofficial and is not affiliated with Spotify.

## Run the preview

Use Node 22.22.1 or later:

```sh
npm ci
npm run dev
```

Open the local address printed by Vite. Browser settings last for the current session. The native app saves preferences on the device.

For native development and portable builds, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Windows portable preview

The build command produces `artifacts/Melora-0.1.0-windows-x64.zip`. Extract the complete ZIP to a writable folder and launch `Melora.exe`. No installer is required. The system Microsoft Edge WebView2 Runtime is required and is not bundled. Preview builds are unsigned.

Preferences are saved in `data/preferences.json` beside the executable. An unwritable folder produces a visible error. Future Spotify credentials will be stored in Windows Credential Manager; moving the portable folder to another computer will require signing in again.

Compilation and runtime verification are separate. Windows and macOS launch still require testing on those systems.

## Spotify configuration

Future authentication will use a bring-your-own Spotify Client ID and Authorization Code with PKCE. The public `.env.example` contains only a placeholder. Do not use a Spotify Client Secret. The intended mode is personal, non-commercial Spotify Development Mode, subject to current Spotify account and API requirements.

## Licences

No project software licence has been selected. Bundled fonts retain their original licences; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Public visibility does not itself grant reuse rights.
