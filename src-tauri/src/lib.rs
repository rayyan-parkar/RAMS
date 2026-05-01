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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(webm_tx)
        .manage(vp8_rx_state)
        .invoke_handler(tauri::generate_handler![
            greet,
            media::ipc::send_video_chunk,
            builder::start_rtc,
            media::ipc::subscribe_video
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
