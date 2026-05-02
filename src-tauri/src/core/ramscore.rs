use std::time::Instant;
use str0m::RtcError;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::media::{Direction, MediaKind};
use crate::signaling::{SignalingHandler, SignalingMessage, SignalingRole, SignalingState};
use str0m::media::{Mid};

/// A superset of str0m that implements a signaling state machine.
pub struct RAMSCore {
    pub rtc: str0m::Rtc,
    pub signaling_handler: SignalingHandler,
    pub pending_remote_candidates: Vec<str0m::Candidate>,
    pub pending_offer: Option<SdpPendingOffer>,
    pub start_time: Instant,
    pub video_mid: Option<Mid>,
}

impl RAMSCore {
    /// Creates a new low-level RAMSCore object
    pub fn new(role: SignalingRole) -> Self {
        let now = Instant::now();
        Self {
            rtc: str0m::Rtc::new(now),
            signaling_handler: SignalingHandler::new(role),
            pending_remote_candidates: Vec::new(),
            pending_offer: None,
            start_time: now,
            video_mid: None,
        }
    }

    /// Transparently passes input to str0m.
    pub fn handle_input(&mut self, input: str0m::Input) -> Result<(), RtcError> {
        self.rtc.handle_input(input)
    }

    /// Transparently polls output from str0m and updates signaling state automatically.
    pub fn poll_output(&mut self) -> Result<str0m::Output, RtcError> {
        let output = self.rtc.poll_output()?;
        
        match &output {
            str0m::Output::Event(str0m::Event::Connected) => {
                if self.signaling_handler.state == SignalingState::TricklingIce {
                    let _ = self.signaling_handler.advance(SignalingState::Stable);
                }
            }
            str0m::Output::Event(str0m::Event::MediaAdded(media)) => {
                if media.kind == str0m::media::MediaKind::Video {
                    self.video_mid = Some(media.mid);
                }
            }
            _ => {}
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

    fn handle_candidate(&mut self, candidate: String) -> Result<(), String> {
        let cand = str0m::Candidate::from_sdp_string(&candidate)
            .map_err(|e| format!("Invalid SDP ICE Candidate: {:?}", e))?;

        self.buffer_or_apply_candidate(cand);
        Ok(())
    }

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

    fn flush_pending_candidates(&mut self) {
        for cand in self.pending_remote_candidates.drain(..) {
            self.rtc.add_remote_candidate(cand);
        }
    }

    /// Writes a media chunk (VP8/WebM) to the specified MID.
    pub fn write_media(&mut self, mid: Mid, data: Vec<u8>) -> Result<(), String> {
        let writer = self.rtc.writer(mid).ok_or_else(|| "No writer for MID".to_string())?;
        
        let pt = writer.payload_params().next().map(|p| p.pt()).ok_or_else(|| "No PT for MID".to_string())?;
        
        let now = Instant::now();
        let rtp_time = str0m::media::MediaTime::new(
            (now - self.start_time).as_micros() as u64, 
            str0m::media::Frequency::MICROS
        );
        
        writer.write(pt, now, rtp_time, data)
            .map_err(|e| format!("Failed to write media: {:?}", e))?;
            
        Ok(())
    }
}