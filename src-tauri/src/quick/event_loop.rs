use crate::hybrid::session::HybridSession;
use crate::media::ipc::RemoteVideoChannel;
use crate::signaling::client::SignalingClient;
use crate::signaling::protocol::SignalingMessage;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::Instant;
use str0m::channel::ChannelId;
use str0m::{Candidate, Output};

/// Reactor handles the 'quick' tier's async execution loop.
/// It multiplexes UDP socket reads, WebRTC state timeout events, and incoming signaling via WebSockets
/// without dedicating a separate thread to each.
pub struct EventLoop {
    pub session: HybridSession,
    pub socket: tokio::net::UdpSocket,
    pub signaling: SignalingClient,
    pub is_initiator: bool,
    pub remote_video_channel: RemoteVideoChannel,
}

impl EventLoop {
    pub fn new(
        session: HybridSession,
        signaling: SignalingClient,
        is_initiator: bool,
        remote_video_channel: RemoteVideoChannel,
    ) -> Self {
        // Bind to any available local UDP port
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let socket = tokio::net::UdpSocket::from_std(socket).unwrap();

        Self {
            signaling,
            session,
            socket,
            is_initiator,
            remote_video_channel,
        }
    }

    /// Helper: register the UDP socket as a local ICE candidate with str0m.
    /// Without this, str0m has no candidates in its SDP and ICE can never connect.
    fn register_local_candidate(&mut self) {
        let local_addr = match self.resolve_local_candidate_addr() {
            Ok(addr) => addr,
            Err(e) => {
                println!("WARNING: Failed to resolve local ICE address: {}", e);
                self.socket.local_addr().unwrap()
            }
        };
        // str0m needs to know about our local socket so it can include it as an ICE candidate
        if let Ok(candidate) = Candidate::host(local_addr, "udp") {
            self.session.state_machine.rtc.add_local_candidate(candidate);
            println!("Registered local ICE candidate: {}", local_addr);
        } else {
            println!("WARNING: Failed to create host candidate from {}", local_addr);
        }
    }

    fn resolve_local_candidate_addr(&self) -> Result<SocketAddr, String> {
        let bound = self.socket.local_addr().map_err(|e| e.to_string())?;
        let port = bound.port();

        if !bound.ip().is_unspecified() {
            return Ok(bound);
        }

        let detected_ip = Self::detect_local_ip().ok_or_else(|| {
            "Unable to detect non-0.0.0.0 local IP for ICE candidate".to_string()
        })?;

        Ok(SocketAddr::new(detected_ip, port))
    }

    fn detect_local_ip() -> Option<IpAddr> {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        let _ = socket.connect("1.1.1.1:80");
        let local = socket.local_addr().ok()?;
        if local.ip().is_unspecified() {
            None
        } else {
            Some(local.ip())
        }
    }

    /// Helper: generate an SDP offer with a data channel for media transport,
    /// store the pending state, and send it via signaling.
    async fn send_offer(&mut self) {
        // Create a data channel BEFORE generating the SDP offer so it's included in the offer.
        // str0m will include an m=application line in the SDP for SCTP/DataChannel support.
        self.session
            .state_machine
            .rtc
            .direct_api()
            .create_data_channel(str0m::channel::ChannelConfig {
                label: "media".to_string(),
                ..Default::default()
            });

        let change = self.session.state_machine.rtc.sdp_api();

        if let Some((offer, pending)) = change.apply() {
            self.session.pending_offer = Some(pending);
            let sdp_string = offer.to_sdp_string();
            println!("SDP Offer generated ({} bytes)", sdp_string.len());

            if let Err(e) = self
                .signaling
                .send(SignalingMessage::Offer {
                    sdp: sdp_string,
                })
                .await
            {
                println!("Failed to send SDP offer: {}", e);
            }
        } else {
            println!("WARNING: sdp_api().apply() returned None — no changes to negotiate.");
        }
    }

    /// Starts the async event loop to poll str0m, dispatch UDP packets, route events,
    /// and forward media chunks between the frontend and the remote peer via DataChannel.
    pub async fn run(
        mut self,
        mut webm_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    ) -> Result<(), String> {
        let mut buf = vec![0u8; 2000];

        // CRITICAL: Register local UDP socket as an ICE candidate BEFORE generating any SDP.
        // Without this, the SDP offer/answer will have no candidates and ICE connectivity fails.
        self.register_local_candidate();

        let local_addr = self
            .resolve_local_candidate_addr()
            .unwrap_or_else(|_| self.socket.local_addr().unwrap());

        // Track the data channel ID once it's open
        let mut data_channel_id: Option<ChannelId> = None;

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
                    // str0m wants to send a UDP packet (STUN, DTLS, SCTP, etc.)
                    let _ = self
                        .socket
                        .send_to(&transmit.contents, transmit.destination)
                        .await;
                }
                Ok(Output::Timeout(target_time)) => {
                    let timeout =
                        tokio::time::sleep_until(tokio::time::Instant::from_std(target_time));

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
                        Some(webm_chunk) = webm_rx.recv() => {
                            // Forward WebM chunk from frontend to remote peer via DataChannel
                            if let Some(cid) = data_channel_id {
                                if let Some(mut channel) = self.session.state_machine.rtc.channel(cid) {
                                    if let Err(e) = channel.write(true, &webm_chunk) {
                                        println!("DataChannel write error: {:?}", e);
                                    }
                                }
                            }
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
                        str0m::Event::ChannelOpen(cid, label) => {
                            println!("DataChannel opened: id={:?}, label={}", cid, label);
                            data_channel_id = Some(cid);
                        }
                        str0m::Event::ChannelData(channel_data) => {
                            // Received WebM chunk from remote peer — forward to frontend
                            let data = channel_data.data;
                            let channel_state = self.remote_video_channel.lock().await;
                            if let Some(ref channel) = *channel_state {
                                if let Err(e) = channel.send(data) {
                                    println!("Failed to send remote video to frontend: {:?}", e);
                                }
                            }
                        }
                        str0m::Event::ChannelClose(cid) => {
                            println!("DataChannel closed: {:?}", cid);
                            data_channel_id = None;
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
