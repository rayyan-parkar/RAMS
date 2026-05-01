use tauri::ipc::Channel;
use tokio::sync::mpsc::UnboundedSender;

/// Receives video chunks from the Svelte frontend and routes them to the demuxer thread.
#[tauri::command]
pub async fn send_video_chunk(
    chunk: Vec<u8>,
    sender: tauri::State<'_, UnboundedSender<Vec<u8>>>
) -> Result<(), String> {
    if let Err(e) = sender.send(chunk) {
        println!("Failed to route video chunk to demuxer: {:?}", e);
    }
    Ok(())
}

/// Subscribes the Svelte frontend to receive remote video chunks from Rust.
#[tauri::command]
pub fn subscribe_video(_on_chunk: Channel<Vec<u8>>) -> Result<(), String> {
    // In future iterations, we will route incoming RTP frames to this channel.
    println!("Frontend subscribed to Remote Video!");
    Ok(())
}
