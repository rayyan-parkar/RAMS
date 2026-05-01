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
    // WebM chunks from the frontend go directly to the event loop (no demuxer needed for DataChannel approach)
    let (webm_tx, webm_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    let webm_rx_state = tokio::sync::Mutex::new(Some(webm_rx));
    let rtc_task_state = builder::RtcTask(tokio::sync::Mutex::new(None));
    let remote_video_channel: media::ipc::RemoteVideoChannel =
        std::sync::Arc::new(tokio::sync::Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(webm_tx)
        .manage(webm_rx_state)
        .manage(rtc_task_state)
        .manage(remote_video_channel)
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
                        use webkit2gtk::{PermissionRequestExt, WebViewExt};
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
