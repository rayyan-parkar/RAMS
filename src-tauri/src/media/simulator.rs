use std::time::Duration;
use tokio::time::interval;

/// A simulator task that generates dummy 30fps media payloads
/// Useful for fairly benchmarking the native WebRTC stack without Tauri IPC bottlenecks.
pub async fn run_video_simulator(writer_channel: tokio::sync::mpsc::Sender<Vec<u8>>) {
    println!("Started Video Frame Simulator (30fps)");
    let mut ticker = interval(Duration::from_millis(33)); // ~30 fps

    loop {
        ticker.tick().await;
        // Generate a simple dummy payload (e.g. 10KB of blank/noisy data simulating a VP8 frame)
        let dummy_frame = vec![0u8; 1024 * 10];

        if writer_channel.send(dummy_frame).await.is_err() {
            println!("Simulator stopped, channel disconnected.");
            break;
        }
    }
}
