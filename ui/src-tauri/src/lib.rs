//! Tauri shell for Ariadne Desktop.
//!
//! The window is a pure REST/SSE client of `ariadned`: everything the UI needs
//! it gets over HTTP from the daemon's TCP listener, so this shell stays empty
//! on purpose — no commands, no daemon internals.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
