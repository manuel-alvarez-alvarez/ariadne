//! Tauri shell for Ariadne Desktop.
//!
//! The window is a pure REST/SSE client of `ariadned`: everything the UI needs
//! it gets over HTTP from the daemon's TCP listener, so this shell stays empty
//! on purpose — no commands, no daemon internals.

/// WebKitGTK's DMA-BUF renderer aborts on some Linux systems with
/// "Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...".
/// Disable it before the webview starts, unless the user already set it.
#[cfg(target_os = "linux")]
fn disable_dmabuf_renderer() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: called once, single-threaded, before any other code reads
        // or writes the process environment.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    disable_dmabuf_renderer();

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
