pub mod core;
pub mod quick;
pub mod signaling;

use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use crate::quick::quick_connect::{QuickConnection, QuickEvent};

struct AppState {
    connection: Arc<Mutex<Option<QuickConnection>>>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
async fn start_quick_call(
    room_id: String,
    ws_url: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    println!("Tauri: start_quick_call for room {} at {}", room_id, ws_url);

    let mut conn_guard = state.connection.lock().await;
    if conn_guard.is_some() {
        return Err("A connection is already active. Close it first.".to_string());
    }

    match crate::quick::quick_connect::connect(&ws_url, &room_id).await {
        Ok((conn, mut event_rx)) => {
            *conn_guard = Some(conn);

            // Spawn a task to bridge QuickEvents to Tauri events
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match event {
                        QuickEvent::Connecting(msg) => {
                            let _ = app_handle.emit("webrtc-connecting", msg);
                        }
                        QuickEvent::Connected => {
                            let _ = app_handle.emit("webrtc-connected", "Session established");
                        }
                        QuickEvent::IceState(state) => {
                            let _ = app_handle.emit("webrtc-ice-state", state);
                        }
                        QuickEvent::MediaData(mid, len) => {
                            let _ = app_handle.emit("webrtc-media-data", (mid, len));
                        }
                    }
                }
            });

            Ok(())
        }
        Err(e) => Err(format!("Failed to connect: {:?}", e)),
    }
}

#[tauri::command]
async fn close_call(state: State<'_, AppState>) -> Result<(), String> {
    println!("Tauri: close_call");
    let mut conn_guard = state.connection.lock().await;
    if let Some(conn) = conn_guard.take() {
        conn.close().await;
        Ok(())
    } else {
        Err("No active connection to close.".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            connection: Arc::new(Mutex::new(None)),
        })
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
            start_quick_call,
            close_call,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
