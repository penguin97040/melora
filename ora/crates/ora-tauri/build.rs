// Generates one `allow-*` permission per command; the app grants them to its main window only.
const COMMANDS: &[&str] = &[
    "status",
    "sign_in",
    "cancel_sign_in",
    "sign_out",
    "set_client_id",
    "profile",
    "subscribe",
    "open_in_spotify",
    "access_token",
    "sdk_event",
    "sdk_attach",
    "subscribe_playback",
    "play_here",
    "playback_command",
    "refresh_playback",
    "list_devices",
    "transfer_playback",
    "refresh_liked",
    "toggle_liked",
    "playlists",
    "playlist_tracks",
    "liked_tracks",
    "play",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
