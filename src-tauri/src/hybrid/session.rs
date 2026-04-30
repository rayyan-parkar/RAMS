use crate::core::state_machine::StateMachine;
use crate::signaling::protocol::SignalingMessage;
use str0m::media::{Direction, MediaKind};
use str0m::Candidate;

/// HybridSession acts as an intermediate state manager.
/// It owns a core RamsEngine and knows how to translate SignalingMessage objects
/// direct internal str0m state changes without requiring a specific IO event loop.
pub struct HybridSession {
    pub state_machine: StateMachine,
    pub pending_offer: Option<str0m::change::SdpPendingOffer>,
}

impl HybridSession {
    /// Creates a new HybridSession initializing the RamsEngine
    pub fn new() -> Result<Self, str0m::RtcError> {
        Ok(Self {
            state_machine: StateMachine::new()?,
            pending_offer: None,
        })
    }

    /// Advertises media tracks for the upcoming SDP offer
    pub fn add_media_tracks(&mut self, audio: bool, video: bool) {
        let mut change = self.state_machine.rtc.sdp_api();
        if audio {
            change.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
        }
        if video {
            change.add_media(MediaKind::Video, Direction::SendRecv, None, None, None);
        }
    }

    /// Feeds incoming out-of-band signaling messages into the WebRTC state machine
    pub fn handle_signaling(&mut self, msg: SignalingMessage) -> Result<(), String> {
        match msg {
            SignalingMessage::Offer { sdp } => {
                let sdp_offer = str0m::change::SdpOffer::from_sdp_string(&sdp).unwrap();
                let mut change = self.state_machine.rtc.sdp_api();
                change.accept_offer(sdp_offer).map_err(|e| format!("Rejecting offer: {:?}", e))?;
            }
            SignalingMessage::Answer { sdp } => {
                let sdp_answer = str0m::change::SdpAnswer::from_sdp_string(&sdp).unwrap();
                let pending = self.pending_offer.take().unwrap();
                let mut change = self.state_machine.rtc.sdp_api();
                change.accept_answer(pending, sdp_answer).map_err(|e| format!("Rejecting answer: {:?}", e))?;
            }
            SignalingMessage::IceCandidate { candidate, .. } => {
                let ice_candidate = Candidate::from_sdp_string(&candidate)
                    .map_err(|e| format!("Failed to parse ICE Candidate: {:?}", e))?;
                self.state_machine.rtc.add_remote_candidate(ice_candidate);
            }
            _ => {
                // Ignore Room/Peer joined messages at this level as this only manages WebRTC coordination
            }
        }
        Ok(())
    }
}
