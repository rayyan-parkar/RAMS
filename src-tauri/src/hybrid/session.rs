use crate::core::state_machine::StateMachine;
use crate::signaling::protocol::SignalingMessage;
use str0m::change::SdpAnswer;
use str0m::media::{Direction, MediaKind};
use str0m::Candidate;

/// HybridSession acts as an intermediate state manager.
/// It owns a core StateMachine and knows how to translate SignalingMessage objects
/// into str0m state changes without requiring a specific IO event loop.
pub struct HybridSession {
    pub state_machine: StateMachine,
    pub pending_offer: Option<str0m::change::SdpPendingOffer>,
}

impl HybridSession {
    /// Creates a new HybridSession initializing the StateMachine
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
        // Note: we don't call .apply() here — the EventLoop handles offer generation.
    }

    /// Feeds incoming signaling messages into the WebRTC state machine.
    /// Returns `Ok(Some(SdpAnswer))` when we received an Offer and produced an Answer.
    /// Returns `Ok(None)` for Answer/ICE messages that don't need a reply.
    pub fn handle_signaling(&mut self, msg: SignalingMessage) -> Result<Option<SdpAnswer>, String> {
        match msg {
            SignalingMessage::Offer { sdp } => {
                let sdp_offer = str0m::change::SdpOffer::from_sdp_string(&sdp)
                    .map_err(|e| format!("Failed to parse SDP offer: {:?}", e))?;
                let answer = self.state_machine.rtc.sdp_api()
                    .accept_offer(sdp_offer)
                    .map_err(|e| format!("Rejecting offer: {:?}", e))?;
                Ok(Some(answer))
            }
            SignalingMessage::Answer { sdp } => {
                let sdp_answer = str0m::change::SdpAnswer::from_sdp_string(&sdp)
                    .map_err(|e| format!("Failed to parse SDP answer: {:?}", e))?;
                let pending = self.pending_offer.take()
                    .ok_or_else(|| "Received Answer but no pending offer exists".to_string())?;
                self.state_machine.rtc.sdp_api()
                    .accept_answer(pending, sdp_answer)
                    .map_err(|e| format!("Rejecting answer: {:?}", e))?;
                Ok(None)
            }
            SignalingMessage::IceCandidate { candidate, .. } => {
                let ice_candidate = Candidate::from_sdp_string(&candidate)
                    .map_err(|e| format!("Failed to parse ICE Candidate: {:?}", e))?;
                self.state_machine.rtc.add_remote_candidate(ice_candidate);
                Ok(None)
            }
            _ => {
                // Ignore Room/Peer joined messages at this level
                Ok(None)
            }
        }
    }
}
