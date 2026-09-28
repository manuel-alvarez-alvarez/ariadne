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

/// WKWebView changes straight quotes and double hyphens into typography marks.
/// That corrupts code and structured data typed into app text fields. WebKit
/// reads these values before it creates a webview, so set them first.
///
/// WebKitGTK has no equivalent setting. Its build excludes automatic text
/// replacement, so quote and dash substitution does not exist on Linux.
#[cfg(target_os = "macos")]
fn disable_smart_substitution() {
    use objc2_foundation::{ns_string, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.setBool_forKey(false, ns_string!("WebAutomaticQuoteSubstitutionEnabled"));
    defaults.setBool_forKey(false, ns_string!("WebAutomaticDashSubstitutionEnabled"));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    disable_dmabuf_renderer();

    #[cfg(target_os = "macos")]
    disable_smart_substitution();

    // Windows uses WebView2, and CI does not build this shell on Windows.
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
