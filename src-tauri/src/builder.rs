use crate::hybrid::session::HybridSession;
use crate::signaling::client::SignalingClient;
use crate::signaling::protocol::SignalingMessage;

/// A builder for establishing WebRTC connections.
/// This acts as the primary configuration struct for the framework's 'quick' tier.
#[derive(Debug, Default, Clone)]
pub struct RamsBuilder {
    pub audio: bool,
    pub video: bool,
    signaling_url: Option<String>,
}

impl RamsBuilder {
    pub fn new() -> Self {
        // Default to video enabled since that's the primary use case
        Self {
            audio: false,
            video: true,
            ..Self::default()
        }
    }

    /// Enable or disable audio track processing
    pub fn with_audio(mut self, enabled: bool) -> Self {
        self.audio = enabled;
        self
    }

    /// Enable or disable video track processing
    pub fn with_video(mut self, enabled: bool) -> Self {
        self.video = enabled;
        self
    }

    /// Sets the URL of the signaling server (e.g. "ws://localhost:8090")
    pub fn with_signaling<S: Into<String>>(mut self, url: S) -> Self {
        self.signaling_url = Some(url.into());
        self
    }

    /// Completes the builder configuration and joins a specific room via the signaling server.
    #[cfg(feature = "quick")]
    pub async fn join_room(self, room_key: &str) -> Result<crate::quick::RamsSession, String> {
        let sig_url = self
            .signaling_url
            .as_ref()
            .ok_or("Signaling URL is required to join a room.")?;

        println!("RamsBuilder joining room '{}' at {}", room_key, sig_url);

        // Establish WebSocket connection to signaling server
        let mut client = SignalingClient::connect(sig_url).await?;

        // Send join message with room_key
        client
            .send(SignalingMessage::Join {
                room: room_key.to_string(),
            })
            .await?;

        // Wait for Joined confirmation
        let is_initiator = loop {
            match client.recv().await {
                Some(SignalingMessage::Joined {
                    room,
                    is_initiator: init,
                }) if room == room_key => {
                    break init;
                }
                Some(SignalingMessage::Error { message }) => {
                    return Err(format!("Signaling error: {}", message));
                }
                None => {
                    return Err("Signaling server disconnected while joining.".into());
                }
                _ => continue,
            }
        };

        println!(
            "Successfully joined room '{}'. Initiator: {}",
            room_key, is_initiator
        );

        Ok(crate::quick::RamsSession {
            is_initiator,
            signaling: client,
        })
    }
}

pub struct RtcTask(pub tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>);

#[tauri::command]
pub async fn abort_rtc(task: tauri::State<'_, RtcTask>) -> Result<(), String> {
    println!("Aborting RTC Uplink...");
    if let Some(handle) = task.0.lock().await.take() {
        handle.abort();
    }
    Ok(())
}

#[tauri::command]
pub async fn start_rtc(
    room: String,
    sig_url: String,
    app_handle: tauri::AppHandle,
    webm_rx: tauri::State<
        '_,
        tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>>,
    >,
    task: tauri::State<'_, RtcTask>,
    remote_video: tauri::State<'_, crate::media::ipc::RemoteVideoChannel>,
) -> Result<(), String> {
    println!("Starting RTC in room {}", room);
    let builder = RamsBuilder::new().with_signaling(sig_url);
    let enable_audio = builder.audio;
    let enable_video = builder.video;

    // Connect to signaling and join the room
    let session = builder.join_room(&room).await?;

    // Create the hybrid session with a fresh str0m state machine
    let mut hybrid = HybridSession::new().map_err(|e| e.to_string())?;
    hybrid.add_media_tracks(enable_audio, enable_video);

    let rx = webm_rx
        .lock()
        .await
        .take()
        .ok_or("RTC already streaming!")?;

    let remote_channel = remote_video.inner().clone();

    let reactor = crate::quick::event_loop::EventLoop::new(
        hybrid,
        session.signaling,
        session.is_initiator,
        remote_channel,
        app_handle,
    );
    let handle = tokio::spawn(async move {
        let _ = reactor.run(rx).await;
    });

    *task.0.lock().await = Some(handle);

    Ok(())
}
