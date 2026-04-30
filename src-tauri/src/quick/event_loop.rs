use crate::hybrid::session::HybridSession;
use crate::signaling::client::SignalingClient;
use std::time::Instant;
use str0m::Output;
use tokio::net::UdpSocket;

/// Reactor handles the 'quick' tier's async execution loop.
/// It multiplexes UDP socket reads, WebRTC state timeout events, and incoming signaling via WebSockets
/// without dedicating a separate thread to each.
pub struct EventLoop {
    pub session: HybridSession,
    pub socket: UdpSocket,
    pub signaling: SignalingClient,
}

impl EventLoop {
    /// Starts the async event loop to poll str0m, dispatch UDP packets, and route WebXR events.
    pub async fn run(mut self) -> Result<(), String> {
        let mut buf = vec![0u8; 2000];

        // Ensure we retrieve our local socket address so `str0m` knows where bytes land.
        let local_addr = self.socket.local_addr().map_err(|e| e.to_string())?;

        loop {
            // Poll str0m for its requested output action
            match self.session.state_machine.rtc.poll_output() {
                Ok(Output::Transmit(transmit)) => {
                    // str0m wants to send a UDP packet (e.g., STUN bing, SRTP packet, etc.)
                    let _ = self
                        .socket
                        .send_to(&transmit.contents, transmit.destination)
                        .await;
                }
                Ok(Output::Timeout(target_time)) => {
                    // str0m requests to "wait" until `target_time` before progressing its internal state
                    let timeout = tokio::time::sleep_until(tokio::time::Instant::from_std(target_time));

                    tokio::select! {
                        _ = timeout => {
                            // The target time has elapsed. Inform str0m to progress its time-sensitive states.
                            // Note: We use Instant::now() here to progress the internal clock of the state machine.
                            let _ = self.session.state_machine.rtc.handle_input(str0m::Input::Timeout(Instant::now()));
                        }
                        result = self.socket.recv_from(&mut buf) => {
                            if let Ok((n, source)) = result {
                                // We received UDP bytes from a peer. Feed them into the str0m instance.
                                // It will unpack the packets to update state or produce DataChannel events.
                                if let Ok(recv) = str0m::net::Receive::new(str0m::net::Protocol::Udp, source, local_addr, &buf[..n]) {
                                    let _ = self.session.state_machine.rtc.handle_input(str0m::Input::Receive(Instant::now(), recv));
                                }
                            }
                        }
                        Some(signaling_msg) = self.signaling.recv() => {
                            // We received a JSON signaling payload via WebSocket. Let HybridSession parse it 
                            // and update the str0m SDR/ICE state machine instantly.
                            if let Err(e) = self.session.handle_signaling(signaling_msg) {
                                println!("Signaling error: {}", e);
                            }
                        }
                    }
                }
                Ok(Output::Event(e)) => {
                    // str0m generated a high-level application event
                    match e {
                        str0m::Event::Connected => println!("WebRTC Connected Successfully!"),
                        // TODO: You will intercept Candidate events here and send them via self.signaling!
                        str0m::Event::IceConnectionStateChange(state) => println!("ICE State: {:?}", state),
                        _ => println!("str0m Event: {:?}", e),
                    }
                }
                Err(e) => {
                    println!("str0m shutdown gracefully or errored: {:?}", e);
                    break;
                }
            }
        }

        Ok(())
    }
}
