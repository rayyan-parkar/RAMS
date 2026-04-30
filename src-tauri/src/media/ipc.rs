use tauri::ipc::Channel;

/// IPC handlers for buffering real video chunks from the Svelte frontend.
/// 
/// In a live environment, Svelte uses MediaRecorder to capture webm chunks,
/// translates them to Uint8Arrays, and streams them here.
#[tauri::command]
pub async fn send_video_chunk(chunk: Vec<u8>) -> Result<(), String> {
    // TODO: I need to route this payload to our running EventLoop via a channel.
    println!("Received IPC video chunk of size {} bytes", chunk.len());
    Ok(())
}

/// Receives video chunks from Rust back over to the Svelte frontend to be played
#[tauri::command]
pub async fn subscribe_video(on_chunk: Channel<Vec<u8>>) -> Result<(), String> {
    // Store the Svelte-provided callback channel and push received RTP/VP8 frames to it later
    println!("Frontend subscribed to backend incoming video frames.");
    Ok(())
}
