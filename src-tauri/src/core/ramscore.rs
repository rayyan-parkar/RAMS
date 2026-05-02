use std::time::Instant;
use str0m::RtcError;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::media::{Direction, MediaKind};
use crate::signaling::{SignalingHandler, SignalingMessage, SignalingRole, SignalingState};

/// A superset of str0m that implements a signaling state machine.
/// rtc : a new str0m Rtc object
/// signaling_handler: A signaling state machine
/// pending_remote_candidates: A queue of remote candidates that have been received but not yet applied
/// pending_offer: A pending offer that has not yet been applied
pub struct RAMSCore {
    pub rtc: str0m::Rtc,
    pub signaling_handler: SignalingHandler,
    pub pending_remote_candidates: Vec<str0m::Candidate>,
    pub pending_offer: Option<SdpPendingOffer>,
}

impl RAMSCore {
    /// Creates a new low-level RAMSCore object
    pub fn new(role: SignalingRole) -> Self {
        Self {
            rtc: str0m::Rtc::new(Instant::now()),
            signaling_handler: SignalingHandler::new(role),
            pending_remote_candidates: Vec::new(),
            pending_offer: None,
        }
    }

    /// Transparently passes input to str0m.
    pub fn handle_input(&mut self, input: str0m::Input) -> Result<(), RtcError> {
        self.rtc.handle_input(input)
    }

    /// Transparently polls output from str0m and updates signaling state automatically.
    pub fn poll_output(&mut self) -> Result<str0m::Output, RtcError> {
        let output = self.rtc.poll_output()?;

        // Automated State Transition: If str0m connects, move our signaling state to Stable.
        if let str0m::Output::Event(str0m::Event::Connected) = &output {
            if self.signaling_handler.state == SignalingState::TricklingIce {
                let _ = self.signaling_handler.advance(SignalingState::Stable);
            }
        }

        Ok(output)
    }

    /// Adds an audio track with the specified direction.
    pub fn add_audio(&mut self, direction: Direction) {
        self.rtc.sdp_api().add_media(MediaKind::Audio, direction, None, None, None);
    }

    /// Adds a video track with the specified direction.
    pub fn add_video(&mut self, direction: Direction) {
        self.rtc.sdp_api().add_media(MediaKind::Video, direction, None, None, None);
    }

    /// Adds a data channel with the given label.
    pub fn add_data_channel(&mut self, label: impl Into<String>) {
        self.rtc.sdp_api().add_channel(label.into().into());
    }

    /// Negotiates any pending local changes (tracks, channels) and creates an SDP Offer.
    /// Updates the signaling state machine to WaitingForAnswer.
    pub fn create_offer(&mut self) -> Result<SignalingMessage, String> {
        if self.signaling_handler.role != SignalingRole::Initiator {
            return Err("Only the Initiator can create an offer".to_string());
        }

        let (offer, pending) = self.rtc
            .sdp_api()
            .apply()
            .ok_or_else(|| "No local changes to negotiate".to_string())?;

        self.pending_offer = Some(pending);
        self.signaling_handler
            .advance(SignalingState::WaitingForAnswer)?;

        Ok(SignalingMessage::Offer {
            sdp: offer.to_sdp_string(),
        })
    }

    /// Handles incoming signaling messages and updates the signaling state machine.
    /// Returns an Option<SignalingMessage> containing an answer if an offer was received.
    pub fn handle_signaling(
        &mut self,
        msg: SignalingMessage,
    ) -> Result<Option<SignalingMessage>, String> {
        match msg {
            SignalingMessage::Offer { sdp } => {
                if self.signaling_handler.role != SignalingRole::Responder {
                    return Err("Initiator cannot accept an Offer".to_string());
                }
                let answer = self.handle_offer(sdp)?;
                Ok(Some(answer))
            }
            SignalingMessage::Answer { sdp } => {
                if self.signaling_handler.role != SignalingRole::Initiator {
                    return Err("Responder cannot accept an Answer".to_string());
                }
                self.handle_answer(sdp)?;
                Ok(None)
            }
            SignalingMessage::Candidate { candidate } => {
                self.handle_candidate(candidate)?;
                Ok(None)
            }
        }
    }


    /// Handles an incoming SDP Offer, updates the signaling state machine, and returns an SDP Answer.
    fn handle_offer(&mut self, sdp: String) -> Result<SignalingMessage, String> {
        self.signaling_handler.advance(SignalingState::TricklingIce)?;

        let offer = SdpOffer::from_sdp_string(&sdp)
            .map_err(|e| format!("Invalid SDP Offer: {:?}", e))?;

        let answer = self
            .rtc
            .sdp_api()
            .accept_offer(offer)
            .map_err(|e| format!("Failed to accept offer: {:?}", e))?;

        self.flush_pending_candidates();

        Ok(SignalingMessage::Answer {
            sdp: answer.to_sdp_string(),
        })
    }

    /// Handles an incoming SDP Answer, updates the signaling state machine, and applies the answer to the RTC.
    fn handle_answer(&mut self, sdp: String) -> Result<(), String> {
        let pending = self
            .pending_offer
            .take()
            .ok_or_else(|| "No pending offer to apply answer to".to_string())?;

        let answer = SdpAnswer::from_sdp_string(&sdp)
            .map_err(|e| format!("Invalid SDP Answer: {:?}", e))?;

        self.rtc
            .sdp_api()
            .accept_answer(pending, answer)
            .map_err(|e| format!("Failed to accept answer: {:?}", e))?;

        self.signaling_handler.advance(SignalingState::TricklingIce)?;
        self.flush_pending_candidates();

        Ok(())
    }

    /// Handles an incoming ICE Candidate and adds it to the RTC.
    fn handle_candidate(&mut self, candidate: String) -> Result<(), String> {
        let cand = str0m::Candidate::from_sdp_string(&candidate)
            .map_err(|e| format!("Invalid SDP ICE Candidate: {:?}", e))?;

        self.buffer_or_apply_candidate(cand);
        Ok(())
    }

    /// Checks whether we need to add ICE candidates to the buffer, or add them to RTC depending on the signaling state
    fn buffer_or_apply_candidate(&mut self, cand: str0m::Candidate) {
        let should_buffer = matches!(
            self.signaling_handler.state,
            SignalingState::Idle | SignalingState::WaitingForAnswer
        );

        if should_buffer {
            self.pending_remote_candidates.push(cand);
        } else {
            self.rtc.add_remote_candidate(cand);
        }
    }

    /// Deletes all candidates from the buffer and adds them to the RTC in str0m
    fn flush_pending_candidates(&mut self) {
        for cand in self.pending_remote_candidates.drain(..) {
            self.rtc.add_remote_candidate(cand);
        }
    }
}