fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "window_chrome",
            "app_info",
            "save_preferences",
            "show_main_window",
        ]),
    ))
    .expect("Native permission generation failed");
}
