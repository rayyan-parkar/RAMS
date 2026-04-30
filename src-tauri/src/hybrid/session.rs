use crate::core::engine::RamsEngine;
use crate::signaling::protocol::SignalingMessage;
use str0m::change::Change;
use str0m::{Sdp, Candidate};

/// HybridSession acts as an intermediate state manager.
/// It owns a core RamsEngine and knows how to translate SignalingMessage objects
/// direct internal str0m state changes without requiring a specific IO event loop.
pub struct HybridSession {
    pub engine: RamsEngine,
}

impl HybridSession {
    /// Creates a new HybridSession initializing the RamsEngine
    pub fn new() -> Result<Self, str0m::RtcError> {
        Ok(Self {
            engine: RamsEngine::new()?,
        })
    }

    /// Feeds incoming out-of-band signaling messages into the WebRTC state machine
    pub fn handle_signaling(&mut self, msg: SignalingMessage) -> Result<(), String> {
        match msg {
            SignalingMessage::Offer { sdp } => {
                let sdp_offer = Sdp::parse(&sdp).map_err(|e| format!("Failed to parse SDP Offer: {:?}", e))?;
                let mut change = self.engine.rtc.sdp_api();
                change.accept_offer(sdp_offer).map_err(|e| format!("Rejecting offer: {:?}", e))?;
            }
            SignalingMessage::Answer { sdp } => {
                let sdp_answer = Sdp::parse(&sdp).map_err(|e| format!("Failed to parse SDP Answer: {:?}", e))?;
                let mut change = self.engine.rtc.sdp_api();
                change.accept_answer(sdp_answer).map_err(|e| format!("Rejecting answer: {:?}", e))?;
            }
            SignalingMessage::IceCandidate { candidate, .. } => {
                let ice_candidate = Candidate::parse(&candidate)
                    .map_err(|e| format!("Failed to parse ICE Candidate: {:?}", e))?;
                self.engine.rtc.add_remote_candidate(ice_candidate);
            }
            _ => {
                // Ignore Room/Peer joined messages at this level as this only manages WebRTC coordination
            }
        }
        Ok(())
    }
}
