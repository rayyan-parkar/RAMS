use std::sync::Arc;
use crate::signaling::client::SignalingClient;
use crate::signaling::protocol::SignalingMessage;

/// A builder for establishing WebRTC connections.
/// This acts as the primary configuration struct for the framework's 'quick' tier.
#[derive(Debug, Default, Clone)]
pub struct RamsBuilder {
    audio: bool,
    video: bool,
    signaling_url: Option<String>,
}

impl RamsBuilder {
    pub fn new() -> Self {
        Self::default()
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

    /// Sets the URL of the signaling server (for e.g. "ws://localhost:8080")
    pub fn with_signaling<S: Into<String>>(mut self, url: S) -> Self {
        self.signaling_url = Some(url.into());
        self
    }

    /// Completes the builder configuration and joins a specific routing room via the signaling server.
    /// This resolves into an active RamsSession in the future. 
    #[cfg(feature = "quick")]
    pub async fn join_room(self, room_key: &str) -> Result<crate::quick::RamsSession, String> {
        let sig_url = self.signaling_url.as_ref()
            .ok_or("Signaling URL is required to join a room.")?;

        println!("RamsBuilder joining room '{}' at {}", room_key, sig_url);

        // Establish WebSocket connection to sig_url
        let mut client = SignalingClient::connect(sig_url).await?;

        // Send join message with room_key
        client.send(SignalingMessage::Join { room: room_key.to_string() }).await?;

        // Negotiate Joined state
        let is_initiator = loop {
            match client.recv().await {
                Some(SignalingMessage::Joined { room, is_initiator: init }) if room == room_key => {
                    break init;
                }
                Some(SignalingMessage::Error { message }) => {
                    return Err(format!("Signaling verification failed: {}", message));
                }
                None => {
                    return Err("Signaling server abruptly disconnected while joining.".into());
                }
                _ => continue, // ignore intermediate messages for now
            }
        };

        println!("Successfully joined room '{}'. Initiator mode: {}", room_key, is_initiator);

        // Return the configured RamsSession
        Ok(crate::quick::RamsSession {
            is_initiator,
            signaling: client,
        })
    }
}
