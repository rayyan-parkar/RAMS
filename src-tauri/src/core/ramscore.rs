use std::time::Instant;
use str0m::RtcError;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::media::{Direction, MediaKind};
use crate::signaling::{SignalingHandler, SignalingMessage, SignalingRole, SignalingState};
use str0m::media::{Mid};

/// A superset of str0m that implements a signaling state machine.
pub struct RAMSCore {
    /// A new str0m Rtc object
    pub rtc: str0m::Rtc,
    /// A handle for the signaling state machine
    pub signaling_handler: SignalingHandler,
    /// A queue of remote candidates that have been received but not yet applied
    pub pending_remote_candidates: Vec<str0m::Candidate>,
    /// A pending offer that has not yet been applied
    pub pending_offer: Option<SdpPendingOffer>,
    /// The time when the RAMSCore was created
    pub start_time: Instant,
    /// The Media Information Descriptor (MID) of the video track, it tracks the identifier of the media, the media type, and the direction.
    pub video_mid: Option<Mid>,
    /// The Media Information Descriptor (MID) of the audio track.
    pub audio_mid: Option<Mid>,
    /// Pending local audio direction to stage in the next local offer.
    pub pending_audio_direction: Option<Direction>,
    /// Pending local video direction to stage in the next local offer.
    pub pending_video_direction: Option<Direction>,
    /// Last time we logged a media event to prevent flooding
    pub last_media_log_time: Instant,
    /// Last time we logged a transmit event
    pub last_transmit_log_time: Instant,
}

impl RAMSCore {
    /// Creates a new low-level RAMSCore object
    pub fn new(role: SignalingRole) -> Self {
        println!("RAMSCore: creating new core with role {:?}", role);
        let now = Instant::now();
        Self {
            rtc: str0m::Rtc::new(now),
            signaling_handler: SignalingHandler::new(role),
            pending_remote_candidates: Vec::new(),
            pending_offer: None,
            start_time: now,
            video_mid: None,
            audio_mid: None,
            pending_audio_direction: None,
            pending_video_direction: None,
            last_media_log_time: now,
            last_transmit_log_time: now,
        }
    }

    /// Transparently passes input to str0m.
    pub fn handle_input(&mut self, input: str0m::Input) -> Result<(), RtcError> {
        // Only log non-timeout events to keep output readable
        if !matches!(input, str0m::Input::Timeout(_)) {
            // Throttled logging could be added here if needed, but for now we'll keep signaling logs
        }
        self.rtc.handle_input(input)
    }

    /// Transparently polls output from str0m and updates signaling state automatically.
    pub fn poll_output(&mut self) -> Result<str0m::Output, RtcError> {
        let output = self.rtc.poll_output()?;
        
        // Suppress noisy output during steady state
        let is_noisy = matches!(&output, str0m::Output::Timeout(_));
        if !is_noisy {
             // Throttled logging for Transmit
             if let str0m::Output::Transmit(transmit) = &output {
                 let now = Instant::now();
                 if now.duration_since(self.last_transmit_log_time) >= std::time::Duration::from_secs(1) {
                     println!("RAMSCore Status: Transmitting UDP ({} bytes to {})", transmit.contents.len(), transmit.destination);
                     self.last_transmit_log_time = now;
                 }
             } else if !matches!(&output, str0m::Output::Event(str0m::Event::MediaData(_))) {
                 // For other non-media events, log them normally
                 println!("RAMSCore: poll_output -> {:?}", output);
             }
        }
        
        match &output {
            str0m::Output::Event(str0m::Event::Connected) => {
                // DTLS/ICE stack is now fully operational
                println!("RAMSCore: DTLS handshake completed successfully, WebRTC connection established");
                if self.signaling_handler.state == SignalingState::TricklingIce {
                    // Transition to Stable as signaling is now essentially complete
                    println!("RAMSCore: connected event observed, moving signaling state to Stable");
                    let _ = self.signaling_handler.advance(SignalingState::Stable);
                }
            }
            str0m::Output::Event(str0m::Event::MediaAdded(media)) => {
                // Track assigned MIDs for later RTP writing
                println!(
                    "RAMSCore: MediaAdded mid={:?}, kind={:?}, direction={:?}",
                    media.mid,
                    media.kind,
                    media.direction
                );
                if media.kind == str0m::media::MediaKind::Video {
                    self.video_mid = Some(media.mid);
                    println!("RAMSCore: stored video_mid = {:?}", self.video_mid);
                } else if media.kind == str0m::media::MediaKind::Audio {
                    self.audio_mid = Some(media.mid);
                    println!("RAMSCore: stored audio_mid = {:?}", self.audio_mid);
                }
            }
            str0m::Output::Event(str0m::Event::IceConnectionStateChange(state)) => {
                // Monitor ICE state transitions for debugging NAT/firewall issues
                println!("RAMSCore: ICE Connection State Change: {:?}", state);
                match state {
                    str0m::IceConnectionState::Connected | str0m::IceConnectionState::Completed => {
                        println!("RAMSCore: ICE connection ready, DTLS handshake will now initiate");
                    }
                    _ => {}
                }
            }
            str0m::Output::Event(str0m::Event::MediaData(data)) => {
                // Throttle logging to prevent console saturation from high-frequency RTP packets
                let now = Instant::now();
                if now.duration_since(self.last_media_log_time) >= std::time::Duration::from_secs(1) {
                    println!("RAMSCore Status: Receiving Media ({} bytes, mid={:?})", data.data.len(), data.mid);
                    self.last_media_log_time = now;
                }
            }
            _ => {}
        }

        Ok(output)
    }

    /// Adds an audio track with the specified direction.
    pub fn add_audio(&mut self, direction: Direction) {
        println!(
            "RAMSCore: queueing audio media with direction {:?} for next offer",
            direction
        );
        self.pending_audio_direction = Some(direction);
    }

    /// Adds a video track with the specified direction.
    pub fn add_video(&mut self, direction: Direction) {
        println!(
            "RAMSCore: queueing video media with direction {:?} for next offer",
            direction
        );
        self.pending_video_direction = Some(direction);
    }

    /// Adds a data channel with the given label.
    pub fn add_data_channel(&mut self, label: impl Into<String>) {
        let label = label.into();
        println!("RAMSCore: adding data channel {}", label);
        self.rtc.sdp_api().add_channel(label.into());
    }

    /// Negotiates any pending local changes (tracks, channels) and creates an SDP Offer.
    pub fn create_offer(&mut self) -> Result<SignalingMessage, String> {
        println!("RAMSCore: create_offer requested with role {:?} and signaling state {:?}", self.signaling_handler.role, self.signaling_handler.state);
        if self.signaling_handler.role != SignalingRole::Initiator {
            return Err("Only the Initiator can create an offer".to_string());
        }

        // Sync clock to prevent DTLS timeouts due to time jumps since initialization
        let _ = self.rtc.handle_input(str0m::Input::Timeout(Instant::now()));
        
        let mut sdp_api = self.rtc.sdp_api();
        let mut staged_changes = 0usize;

        // Synchronize media mid trackers
        if let Some(direction) = self.pending_audio_direction {
            println!(
                "RAMSCore: staging pending audio media in offer with direction {:?}",
                direction
            );
            let mid = sdp_api.add_media(MediaKind::Audio, direction, None, None, None);
            self.audio_mid = Some(mid);
            staged_changes += 1;
        }

        if let Some(direction) = self.pending_video_direction {
            println!(
                "RAMSCore: staging pending video media in offer with direction {:?}",
                direction
            );
            let mid = sdp_api.add_media(MediaKind::Video, direction, None, None, None);
            self.video_mid = Some(mid);
            staged_changes += 1;
        }

        // Finalize SDP structure and extract pending change handle
        let (offer, pending) = sdp_api
            .apply()
            .ok_or_else(|| {
                format!(
                    "No local changes to negotiate (staged_changes={}, pending_audio={:?}, pending_video={:?})",
                    staged_changes,
                    self.pending_audio_direction,
                    self.pending_video_direction
                )
            })?;

        // Clear staged directions as they are now part of the pending offer
        if self.pending_audio_direction.is_some() {
            self.pending_audio_direction = None;
        }
        if self.pending_video_direction.is_some() {
            self.pending_video_direction = None;
        }

        println!("RAMSCore: created SDP offer and pending change set");

        self.pending_offer = Some(pending);
        self.signaling_handler
            .advance(SignalingState::WaitingForAnswer)?;

        println!("RAMSCore: signaling state updated to WaitingForAnswer");

        Ok(SignalingMessage::Offer {
            sdp: offer.to_sdp_string(),
        })
    }

    /// Handles incoming signaling messages and updates the signaling state machine.
    /// This is the entry point for all SDP and ICE negotiations from the remote peer.
    pub fn handle_signaling(
        &mut self,
        msg: SignalingMessage,
    ) -> Result<Option<SignalingMessage>, String> {
        // Sync clock to prevent DTLS timeouts due to time jumps during signaling wait
        let _ = self.rtc.handle_input(str0m::Input::Timeout(Instant::now()));
        
        println!("RAMSCore: handle_signaling({:?}) in role {:?}, state {:?}", msg, self.signaling_handler.role, self.signaling_handler.state);
        match msg {
            SignalingMessage::Offer { sdp } => {
                // Negotiate incoming offer and produce an SDP Answer
                if self.signaling_handler.role != SignalingRole::Responder {
                    return Err("Initiator cannot accept an Offer".to_string());
                }
                let answer = self.handle_offer(sdp)?;
                Ok(Some(answer))
            }
            SignalingMessage::Answer { sdp } => {
                // Apply incoming answer to the previously created local offer
                if self.signaling_handler.role != SignalingRole::Initiator {
                    return Err("Responder cannot accept an Answer".to_string());
                }
                self.handle_answer(sdp)?;
                Ok(None)
            }
            SignalingMessage::Candidate { candidate } => {
                // Route ICE candidate to the RTC stack or buffer it
                self.handle_candidate(candidate)?;
                Ok(None)
            }
        }
    }

    fn handle_offer(&mut self, sdp: String) -> Result<SignalingMessage, String> {
        println!("RAMSCore: handling incoming offer ({} bytes)", sdp.len());
        // Guard: Only accept offer if not already in TricklingIce or Stable
        if self.signaling_handler.state == SignalingState::TricklingIce || self.signaling_handler.state == SignalingState::Stable {
            return Err("Offer already accepted or in invalid state for offer".to_string());
        }
        self.signaling_handler.advance(SignalingState::TricklingIce)?;
        println!("RAMSCore: responder state moved to TricklingIce before accepting offer");

        let offer = SdpOffer::from_sdp_string(&sdp)
            .map_err(|e| format!("Invalid SDP Offer: {:?}", e))?;

        let mut sdp_api = self.rtc.sdp_api();
        // The Responder MUST declare its supported local media tracks to match against the offer.
        let am = sdp_api.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
        self.audio_mid = Some(am);
        let vm = sdp_api.add_media(MediaKind::Video, Direction::SendRecv, None, None, None);
        self.video_mid = Some(vm);

        let answer = sdp_api.accept_offer(offer)
            .map_err(|e| format!("Failed to accept offer: {:?}", e))?;

        println!("RAMSCore: offer accepted and SDP answer prepared");

        self.flush_pending_candidates();
        println!("RAMSCore: flushed pending remote ICE candidates after offer");
        println!("RAMSCore: signaling state after offer: {:?}", self.signaling_handler.state);

        Ok(SignalingMessage::Answer {
            sdp: answer.to_sdp_string(),
        })
    }

    fn handle_answer(&mut self, sdp: String) -> Result<(), String> {
        println!("RAMSCore: handling incoming answer ({} bytes)", sdp.len());
        // Guard: Only accept answer if we have a pending offer
        let pending = self
            .pending_offer
            .take()
            .ok_or_else(|| "No pending offer to apply answer to".to_string())?;

        // Guard: Only accept answer if not already in TricklingIce or Stable
        if self.signaling_handler.state == SignalingState::TricklingIce || self.signaling_handler.state == SignalingState::Stable {
            println!("RAMSCore: WARNING: Already in TricklingIce or Stable state before answer application");
        } else {
            self.signaling_handler.advance(SignalingState::TricklingIce)?;
            println!("RAMSCore: signaling state updated to TricklingIce before answer application");
        }
        self.flush_pending_candidates();
        println!("RAMSCore: flushed pending remote ICE candidates before answer application");

        let answer = SdpAnswer::from_sdp_string(&sdp)
            .map_err(|e| format!("Invalid SDP Answer: {:?}", e))?;

        if let Err(e) = self.rtc.sdp_api().accept_answer(pending, answer) {
            let err_msg = format!("Failed to accept answer: {:?}", e);
            eprintln!("RAMSCore: ERROR: {}", err_msg);
            return Err(err_msg);
        }

        println!("RAMSCore: answer accepted successfully");
        println!("RAMSCore: signaling state after answer: {:?}", self.signaling_handler.state);

        Ok(())
    }

    pub fn handle_candidate(&mut self, candidate: String) -> Result<(), String> {
        println!("RAMSCore: handling incoming ICE candidate: {}", candidate);
        let cand = str0m::Candidate::from_sdp_string(&candidate)
            .map_err(|e| format!("Invalid SDP ICE Candidate: {:?}", e))?;

        self.buffer_or_apply_candidate(cand);
        Ok(())
    }

    /// Decides whether to buffer an ICE candidate or apply it immediately.
    /// Candidates received before the SDP handshake (Offer/Answer) must be buffered.
    pub fn buffer_or_apply_candidate(&mut self, cand: str0m::Candidate) {
        let should_buffer = matches!(
            self.signaling_handler.state,
            SignalingState::Idle | SignalingState::WaitingForAnswer
        );

        println!(
            "RAMSCore: {} ICE candidate while in state {:?}",
            if should_buffer { "buffering" } else { "applying" },
            self.signaling_handler.state
        );

        if should_buffer {
            // Buffer candidate for later flushing after SDP acceptance
            self.pending_remote_candidates.push(cand);
            println!("RAMSCore: pending_remote_candidates size = {}", self.pending_remote_candidates.len());
        } else {
            // Apply directly if the handshake is already sufficient
            self.rtc.add_remote_candidate(cand);
            println!("RAMSCore: candidate applied directly to RTC");
        }
    }

    /// Flushes all buffered ICE candidates into the str0m RTC stack.
    fn flush_pending_candidates(&mut self) {
        println!("RAMSCore: flushing {} pending ICE candidates", self.pending_remote_candidates.len());
        for cand in self.pending_remote_candidates.drain(..) {
            self.rtc.add_remote_candidate(cand);
        }
    }

    /// Writes raw media bytes (e.g., an Opus frame) to the Audio RTP stream.
    pub fn write_audio_media(&mut self, mid: Mid, data: Vec<u8>, timestamp: u64) -> Result<(), String> {
        let now = Instant::now();
        let rtp_time = str0m::media::MediaTime::new(
            timestamp, 
            str0m::media::Frequency::MICROS
        );

        let writer = self.rtc.writer(mid).ok_or("No audio writer")?;
        let pt = writer.payload_params().next().map(|p| p.pt()).ok_or_else(|| "No PT for MID".to_string())?;
        
        writer.write(pt, now, rtp_time, data).map_err(|e| e.to_string())
    }

    /// Writes a media chunk (e.g., H.264/VP8/WebM) to the specified MID.
    /// Expects timestamp in microseconds for high-precision RTP timing.
    pub fn write_media(&mut self, mid: Mid, data: Vec<u8>, timestamp: u64) -> Result<(), String> {
        let writer = self.rtc.writer(mid).ok_or_else(|| "No writer for MID".to_string())?;
        
        // Resolve Payload Type (PT) from the media writer's parameters
        let pt = writer.payload_params().next().map(|p| p.pt()).ok_or_else(|| "No PT for MID".to_string())?;
        
        let now = Instant::now();
        // Convert raw timestamp to str0m-native MediaTime using Microsecond frequency
        let rtp_time = str0m::media::MediaTime::new(
            timestamp, 
            str0m::media::Frequency::MICROS
        );
        
        writer.write(pt, now, rtp_time, data)
            .map_err(|e| format!("Failed to write media: {:?}", e))?;
            
        Ok(())
    }
}