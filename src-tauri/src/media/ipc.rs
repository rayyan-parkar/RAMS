use std::sync::Arc;
use tauri::ipc::Channel;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

/// Shared state for the frontend video channel, allowing the EventLoop
/// to push received remote media chunks back to the Svelte UI.
pub type RemoteVideoChannel = Arc<Mutex<Option<Channel<Vec<u8>>>>>;

/// Receives video chunks from the Svelte frontend and routes them to the event loop.
#[tauri::command]
pub async fn send_video_chunk(
    chunk: Vec<u8>,
    sender: tauri::State<'_, UnboundedSender<Vec<u8>>>,
) -> Result<(), String> {
    if let Err(e) = sender.send(chunk) {
        println!("Failed to route video chunk to event loop: {:?}", e);
    }
    Ok(())
}

/// Subscribes the Svelte frontend to receive remote video chunks from Rust.
/// The Channel is stored in shared state so the EventLoop can access it.
#[tauri::command]
pub async fn subscribe_video(
    on_chunk: Channel<Vec<u8>>,
    channel_state: tauri::State<'_, RemoteVideoChannel>,
) -> Result<(), String> {
    *channel_state.lock().await = Some(on_chunk);
    println!("Frontend subscribed to Remote Video!");
    Ok(())
}
