pub mod builder;

#[cfg(feature = "core")]
pub mod core;

#[cfg(feature = "hybrid")]
pub mod hybrid;

#[cfg(feature = "quick")]
pub mod media;

#[cfg(feature = "quick")]
pub mod quick;

#[cfg(feature = "quick")]
pub mod signaling;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (webm_tx, webm_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (vp8_tx, vp8_rx) = tokio::sync::mpsc::unbounded_channel();
    
    // Start background background demuxer automatically
    // It blocks waiting for bytes from the frontend
    media::demuxer::start_demuxer_thread(webm_rx, vp8_tx);

    let vp8_rx_state = tokio::sync::Mutex::new(Some(vp8_rx));
    let rtc_task_state = builder::RtcTask(tokio::sync::Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(webm_tx)
        .manage(vp8_rx_state)
        .manage(rtc_task_state)
        .setup(|app| {
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.with_webview(|webview| {
                        use webkit2gtk::{WebViewExt, PermissionRequestExt};
                        let inner = webview.inner();
                        inner.connect_permission_request(|_, request| {
                            println!("WebKitGTK: Auto-granting permission request");
                            request.allow();
                            true
                        });
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            media::ipc::send_video_chunk,
            builder::start_rtc,
            builder::abort_rtc,
            media::ipc::subscribe_video
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
