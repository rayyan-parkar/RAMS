pub mod core;
#[cfg(feature = "quick")]
pub mod quick;
pub mod signaling;

#[cfg(feature = "quick")]
use std::sync::Arc;
#[cfg(feature = "quick")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "quick")]
use tokio::sync::Mutex;
#[cfg(feature = "quick")]
use tauri::{AppHandle, Emitter, State};
#[cfg(feature = "quick")]
use crate::quick::quick_connect::{QuickConnection, QuickEvent};

/// Global application state managed by Tauri.
#[cfg(feature = "quick")]
struct AppState {
    /// Atomic wrapper around the active WebRTC connection.
    connection: Arc<Mutex<Option<QuickConnection>>>,
}

/// Initializes a new WebRTC session via the QuickConnect orchestrator.
/// Bridges background QuickEvents to the Tauri frontend via event emission.
#[tauri::command]
#[cfg(feature = "quick")]
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

            // Spawn a task to bridge internal QuickEvents to the frontend.
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
                        QuickEvent::MediaData(mid, data) => {
                            // Atomic packet counter for throttled bridge logging.
                            static PKT_COUNT: AtomicU64 = AtomicU64::new(0);
                            let count = PKT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                            if count % 100 == 0 {
                                println!("BRIDGE: Forwarded {} packets total. Last: {} ({} bytes)", count, mid, data.len());
                            }
                            let _ = app_handle.emit("webrtc-media-data", (mid, data));
                        }
                    }
                }
            });

            Ok(())
        }
        Err(e) => Err(format!("Failed to connect: {:?}", e)),
    }
}

/// Tears down the active WebRTC session and cleans up background tasks.
#[tauri::command]
#[cfg(feature = "quick")]
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

/// Routes raw video frames from the frontend to the WebRTC encoder/packetizer.
#[tauri::command]
#[cfg(feature = "quick")]
async fn send_video_chunk(
    data: Vec<u8>,
    timestamp: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut conn_guard = state.connection.lock().await;
    if let Some(conn) = conn_guard.as_mut() {
        let mut core = conn.core.lock().await;
        if let Some(mid) = core.video_mid {
            core.write_media(mid, data, timestamp)?;
        }
    }
    Ok(())
}

/// Routes raw audio frames from the frontend to the WebRTC encoder/packetizer.
#[tauri::command]
#[cfg(feature = "quick")]
async fn send_audio_chunk(
    data: Vec<u8>,
    timestamp: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut conn_guard = state.connection.lock().await;
    if let Some(conn) = conn_guard.as_mut() {
        let mut core = conn.core.lock().await;
        if let Some(mid) = core.audio_mid {
            core.write_audio_media(mid, data, timestamp)?;
        }
    }
    Ok(())
}

/// Main entry point for the RAMS Tauri application.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init());

    #[cfg(feature = "quick")]
    {
        builder = builder
            .manage(AppState {
                connection: Arc::new(Mutex::new(None)),
            })
            .invoke_handler(tauri::generate_handler![
                start_quick_call,
                close_call,
                send_video_chunk,
                send_audio_chunk,
            ]);
    }

    builder
        .setup(|app| {
            // WebKitGTK specific configuration for Linux/NixOS environments.
            // Automatically grants media permissions to bypass UI prompts in embedded views.
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
