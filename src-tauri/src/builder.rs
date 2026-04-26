use std::sync::Arc;

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

        // TODO
        // 1. Establish WebSocket connection to sig_url
        // 2. Send join message with room_key
        // 3. Negotiate SDP
        // 4. Return the running RamsSession
        
        Err("Not implemented yet".into())
    }
}
