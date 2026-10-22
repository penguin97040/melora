//! The `ora` Tauri plugin. It owns the shared commands and streams for the main window; the
//! logic itself lives in `ora-core`. No commands are registered yet.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub use ora_core as core;

/// The plugin, registered by each client with `.plugin(ora_tauri::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("ora").build()
}
