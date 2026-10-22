// Generates one `allow-*` permission per command; clients grant them to their main window only.
const COMMANDS: &[&str] = &[
    "status",
    "sign_in",
    "cancel_sign_in",
    "sign_out",
    "set_client_id",
    "profile",
    "subscribe",
    "open_in_spotify",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
