use crate::hybrid::session::HybridSession;
use crate::signaling::client::SignalingClient;
use crate::signaling::protocol::SignalingMessage;
use std::time::Instant;
use str0m::{Candidate, Output};

/// Reactor handles the 'quick' tier's async execution loop.
/// It multiplexes UDP socket reads, WebRTC state timeout events, and incoming signaling via WebSockets
/// without dedicating a separate thread to each.
pub struct EventLoop {
    pub session: HybridSession,
    pub socket: tokio::net::UdpSocket,
    pub signaling: SignalingClient,
    pub is_initiator: bool,
}

impl EventLoop {
    pub fn new(session: HybridSession, signaling: SignalingClient, is_initiator: bool) -> Self {
        // Bind to any available local UDP port
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let socket = tokio::net::UdpSocket::from_std(socket).unwrap();

        Self {
            signaling,
            session,
            socket,
            is_initiator,
        }
    }

    /// Helper: register the UDP socket as a local ICE candidate with str0m.
    /// Without this, str0m has no candidates in its SDP and ICE can never connect.
    fn register_local_candidate(&mut self) {
        let local_addr = self.socket.local_addr().unwrap();
        // str0m needs to know about our local socket so it can include it as an ICE candidate
        if let Ok(candidate) = Candidate::host(local_addr, "udp") {
            self.session.state_machine.rtc.add_local_candidate(candidate);
            println!("Registered local ICE candidate: {}", local_addr);
        } else {
            println!("WARNING: Failed to create host candidate from {}", local_addr);
        }
    }

    /// Helper: generate an SDP offer, store the pending state, and send it via signaling.
    async fn send_offer(&mut self) {
        let mut change = self.session.state_machine.rtc.sdp_api();
        change.add_media(
            str0m::media::MediaKind::Video,
            str0m::media::Direction::SendRecv,
            None, None, None,
        );

        if let Some((offer, pending)) = change.apply() {
            self.session.pending_offer = Some(pending);
            let sdp_string = offer.to_sdp_string();
            println!("SDP Offer generated ({} bytes)", sdp_string.len());

            if let Err(e) = self.signaling.send(SignalingMessage::Offer {
                sdp: sdp_string,
            }).await {
                println!("Failed to send SDP offer: {}", e);
            }
        } else {
            println!("WARNING: sdp_api().apply() returned None — no changes to negotiate.");
        }
    }

    /// Starts the async event loop to poll str0m, dispatch UDP packets, route events, and feed VP8 frames.
    pub async fn run(mut self, mut vp8_rx: tokio::sync::mpsc::UnboundedReceiver<crate::media::demuxer::Vp8Packet>) -> Result<(), String> {
        let mut buf = vec![0u8; 2000];

        // CRITICAL: Register local UDP socket as an ICE candidate BEFORE generating any SDP.
        // Without this, the SDP offer/answer will have no candidates and ICE connectivity fails.
        self.register_local_candidate();

        let local_addr = self.socket.local_addr().map_err(|e| e.to_string())?;

        // If initiator: DON'T send the offer yet. Wait for PeerJoined first.
        // The offer would be lost if sent before the remote peer has connected to the room.
        // If NOT initiator: do nothing — we'll receive an offer from the initiator.
        if self.is_initiator {
            println!("We are the Initiator. Waiting for remote peer to join before sending offer...");
        } else {
            println!("We are the Responder. Waiting for incoming SDP offer...");
        }

        loop {
            // Poll str0m for its requested output action
            match self.session.state_machine.rtc.poll_output() {
                Ok(Output::Transmit(transmit)) => {
                    // str0m wants to send a UDP packet (STUN, SRTP, etc.)
                    let _ = self
                        .socket
                        .send_to(&transmit.contents, transmit.destination)
                        .await;
                }
                Ok(Output::Timeout(target_time)) => {
                    let timeout = tokio::time::sleep_until(tokio::time::Instant::from_std(target_time));

                    tokio::select! {
                        _ = timeout => {
                            let _ = self.session.state_machine.rtc.handle_input(
                                str0m::Input::Timeout(Instant::now())
                            );
                        }
                        result = self.socket.recv_from(&mut buf) => {
                            if let Ok((n, source)) = result {
                                if let Ok(recv) = str0m::net::Receive::new(
                                    str0m::net::Protocol::Udp, source, local_addr, &buf[..n]
                                ) {
                                    let _ = self.session.state_machine.rtc.handle_input(
                                        str0m::Input::Receive(Instant::now(), recv)
                                    );
                                }
                            }
                        }
                        Some(vp8) = vp8_rx.recv() => {
                            println!("VP8 Frame Size {} loaded", vp8.data.len());
                        }
                        Some(signaling_msg) = self.signaling.recv() => {
                            match &signaling_msg {
                                SignalingMessage::PeerJoined => {
                                    // Remote peer has joined! If we're the initiator, NOW send the offer.
                                    if self.is_initiator {
                                        println!("Remote peer joined. Generating and sending SDP offer...");
                                        self.send_offer().await;
                                    }
                                }
                                _ => {
                                    // Delegate offer/answer/ICE to HybridSession
                                    match self.session.handle_signaling(signaling_msg) {
                                        Ok(Some(answer)) => {
                                            // We received an offer and produced an answer — send it back
                                            let sdp_string = answer.to_sdp_string();
                                            println!("SDP Answer generated ({} bytes)", sdp_string.len());
                                            if let Err(e) = self.signaling.send(SignalingMessage::Answer {
                                                sdp: sdp_string,
                                            }).await {
                                                println!("Failed to send SDP answer: {}", e);
                                            }
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            println!("Signaling error: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Output::Event(e)) => {
                    match e {
                        str0m::Event::Connected => println!("WebRTC Connected Successfully!"),
                        str0m::Event::IceConnectionStateChange(state) => {
                            println!("ICE State: {:?}", state);
                        }
                        str0m::Event::MediaAdded(media) => {
                            println!("Media added: mid={}, kind={:?}", media.mid, media.kind);
                        }
                        _ => println!("str0m Event: {:?}", e),
                    }
                }
                Err(e) => {
                    println!("str0m shutdown or error: {:?}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}
