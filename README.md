# Melora

Melora is a personal desktop music client built with Tauri 2, Rust, React and TypeScript. Windows, Linux and macOS are intended targets.

The first build is a local design preview with invented track titles, artists and artwork. You can browse preview playlists, search, select tracks and adjust display preferences. Shuffle and repeat let you preview mode selections; they leave track order unchanged and reset on relaunch. You can sign in to Spotify and see your account name. With Spotify Premium you can also play Spotify inside Melora (Settings → Playback in Melora, through Spotify's Web Playback SDK); while it plays, the player shows the real track, artwork and controls. The player can also show and control what's playing on your other Spotify devices (a phone, the Spotify app, a speaker), and **Choose device** moves the music between them and Melora. The **+** beside the track title adds it to your Liked Songs or removes it. While a podcast plays, the buttons beside Play skip back or forward 15 seconds. Your library, playlists and search are not implemented yet, so that content stays synthetic and labelled. Melora is unofficial and is not affiliated with, endorsed by or sponsored by Spotify.

## Run the preview

Use Node 22.22.1 or later:

```sh
npm ci
npm run dev
```

Open the local address printed by Vite. Browser settings last for the current session. The native app saves preferences on the device.

For native development and portable builds, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Windows portable preview

The build command produces `artifacts/Melora-<version>-windows-x64.zip`. Extract the complete ZIP to a writable folder and launch `Melora.exe`. No installer is required. The system Microsoft Edge WebView2 Runtime is required and is not bundled. Preview builds are unsigned.

Preferences are saved in `data/preferences.json` beside the executable, and your Spotify Client ID in `data/spotify.json`. An unwritable folder produces a visible error. The Spotify refresh token is stored only in Windows Credential Manager (macOS Keychain or Linux Secret Service elsewhere); moving the portable folder to another computer requires signing in again.

Compilation and runtime verification are separate. Windows and macOS launch still require testing on those systems.

## Spotify configuration

Melora uses your own Spotify app (bring your own Client ID) and Authorization Code with PKCE. It never uses or asks for a Client Secret or your Spotify password.

1. In the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard), create an app. Add the Redirect URI `http://127.0.0.1:43821/callback` exactly. It starts with `http`, which Spotify permits for loopback addresses (the Dashboard shows a generic "not secure" notice), and it must include the port: the Dashboard refuses the port-less form. Melora listens on that port only while you sign in, and Settings shows the exact URI. Tick **Web API** and **Web Playback SDK**.
2. Under **User Management**, add every Spotify account that will sign in, including your own. Spotify answers other accounts with "forbidden" errors.
3. In Melora, open **Settings → Spotify account**, paste the app's Client ID, save, and choose **Sign in with Spotify**. Your browser opens Spotify's approval page.

Melora requests only the scopes the current features need (now `user-read-private`, for your profile). Access tokens stay in the native process memory; nothing is sent anywhere except Spotify. The intended mode is personal, non-commercial Spotify Development Mode, which needs the app owner to have Spotify Premium, allows up to five users and expires sign-ins after six months.

## Licences

No project software licence has been selected. Bundled fonts retain their original licences; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Public visibility does not itself grant reuse rights.
