use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use futures_util::stream::{SplitSink, SplitStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio::net::TcpStream as TokioTcpStream;
use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Utf8Bytes;

use str0m::media::Direction;
use crate::core::RAMSCore;
use crate::signaling::{SignalingMessage, SignalingRole};

/// Events emitted by the quick connection.
#[derive(Debug, Clone)]
pub enum QuickEvent {
    Connecting(String),
    Connected,
    IceState(String),
    MediaData(String, Vec<u8>),
}

/// Handle to a running quick connection.
pub struct QuickConnection {
    pub core: std::sync::Arc<Mutex<RAMSCore>>,
    pub(crate) shutdown: Option<oneshot::Sender<()>>,
    pub(crate) task: tokio::task::JoinHandle<()>,
}

impl QuickConnection {
    /// Provides access to the underlying RAMSCore for advanced control.
    pub fn core(&self) -> std::sync::Arc<Mutex<RAMSCore>> {
        std::sync::Arc::clone(&self.core)
    }

    /// Gracefully stop background tasks.
    pub async fn close(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        let _ = self.task.await;
    }
}

/// Error type for quick connection failures.
#[derive(Debug)]
pub enum QuickError {
    WebSocket(String),
    Serde(String),
    Protocol(String),
    Io(String),
}

/// Error conversion from tungstenite WebSocket errors to QuickErrors.
impl From<tokio_tungstenite::tungstenite::Error> for QuickError {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        QuickError::WebSocket(value.to_string())
    }
}

/// Error conversion from JSON serialization/deserialization errors to QuickErrors.
impl From<serde_json::Error> for QuickError {
    fn from(value: serde_json::Error) -> Self {
        QuickError::Serde(value.to_string())
    }
}

/// Error conversion from I/O errors to QuickErrors.
impl From<std::io::Error> for QuickError {
    fn from(value: std::io::Error) -> Self {
        QuickError::Io(value.to_string())
    }
}

/// Wire message enum to track room joining, and signaling messages over the WebSocket.
/// Utility enum to avoid duplicating signaling logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WireMessage {
    Join {
        room: String,
    },
    Joined {
        room: String,
        #[serde(rename = "isInitiator")]
        is_initiator: bool,
        #[serde(default)]
        stun_servers: Vec<String>,
    },
    PeerJoined,
    // Flattened SignalingMessage variants
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
    },
}

impl From<SignalingMessage> for WireMessage {
    fn from(msg: SignalingMessage) -> Self {
        match msg {
            SignalingMessage::Offer { sdp } => WireMessage::Offer { sdp },
            SignalingMessage::Answer { sdp } => WireMessage::Answer { sdp },
            SignalingMessage::Candidate { candidate } => WireMessage::Candidate { candidate },
        }
    }
}

impl TryFrom<WireMessage> for SignalingMessage {
    type Error = ();

    fn try_from(msg: WireMessage) -> Result<Self, Self::Error> {
        match msg {
            WireMessage::Offer { sdp } => Ok(SignalingMessage::Offer { sdp }),
            WireMessage::Answer { sdp } => Ok(SignalingMessage::Answer { sdp }),
            WireMessage::Candidate { candidate } => Ok(SignalingMessage::Candidate { candidate }),
            _ => Err(()),
        }
    }
}

/// Orchestrator function that sets up a WebRTC session.
/// It performs the initial handshake, sets up local resources, and spawns the background runner.
pub async fn connect(ws_url: &str, room: &str) -> Result<(QuickConnection, mpsc::UnboundedReceiver<QuickEvent>), QuickError> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    println!("Quick: Connecting to {} for room {}", ws_url, room);
    let _ = event_tx.send(QuickEvent::Connecting(format!("Connecting to room: {}", room)));

    // First establish WebSocket Connection
    println!("Quick: Opening websocket connection to signaling server...");
    let (ws_stream, _) = connect_async(ws_url).await?;
    println!("Quick: WebSocket connected, waiting for room role assignment...");
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // 2. Negotiate Role (Join Room)
    // We wait for the server to tell us if we are the caller (Initiator) or receiver (Responder)
    let (role, stun_servers) = negotiate_signaling_role(&mut ws_read, &mut ws_write, room).await?;

    // 3. Initialize Core and Infrastructure
    // Arc<Mutex<>> is used so both the background loop and the main handle can access the core safely.
    let core = std::sync::Arc::new(Mutex::new(RAMSCore::new(role)));
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (ws_tx, ws_rx) = mpsc::unbounded_channel::<WireMessage>();

    // 4. Spawn the background runner
    // This is the "Engine" of the connection that runs as long as the session is active.
    let task_core = std::sync::Arc::clone(&core);
    let task_event_tx = event_tx.clone();
    let task_ws_url = ws_url.to_string();
    let task_room = room.to_string();
    let ws_task = tokio::spawn(async move {
        if let Err(e) = run_connection_loop(task_core, shutdown_rx, ws_rx, ws_read, ws_write, ws_tx, task_event_tx, task_ws_url, task_room, stun_servers).await {
            eprintln!("QuickConnection loop exited with error: {:?}", e);
        }
    });

    Ok((QuickConnection {
        core,
        shutdown: Some(shutdown_tx),
        task: ws_task,
    }, event_rx))
}

/// Helper to handle the initial Join/Joined handshake on the WebSocket.
async fn negotiate_signaling_role<R, W>(
    ws_read: &mut R,
    ws_write: &mut W,
    room: &str,
) -> Result<(SignalingRole, Vec<String>), QuickError>
where
    R: StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    W: SinkExt<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let join = WireMessage::Join { room: room.to_string() };
    println!("Quick: Sending join message for room {}", room);
    ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
        Utf8Bytes::from(serde_json::to_string(&join)?),
    )).await?;

    while let Some(msg) = ws_read.next().await {
        let msg = msg?;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
            println!("Quick: Received websocket text while negotiating role: {}", text);
            let parsed: WireMessage = serde_json::from_str(&text)?;
            if let WireMessage::Joined { is_initiator, stun_servers, .. } = parsed {
                println!(
                    "Quick: Joined room {} as {} (received {} stun servers)",
                    room,
                    if is_initiator { "Initiator" } else { "Responder" },
                    stun_servers.len()
                );
                let role = if is_initiator {
                    SignalingRole::Initiator
                } else {
                    SignalingRole::Responder
                };
                return Ok((role, stun_servers));
            }
        }
    }

    Err(QuickError::Protocol("Signaling server closed before joining".to_string()))
}

/// The main event loop that orchestrates all I/O for the WebRTC session.
/// It uses tokio::select! to multiplex between signaling, media data, and library timeouts.
async fn run_connection_loop(
    core: std::sync::Arc<Mutex<RAMSCore>>,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut ws_rx: mpsc::UnboundedReceiver<WireMessage>,
    mut ws_read: SplitStream<WebSocketStream<MaybeTlsStream<TokioTcpStream>>>,
    mut ws_write: SplitSink<WebSocketStream<MaybeTlsStream<TokioTcpStream>>, tokio_tungstenite::tungstenite::Message>,
    ws_tx: mpsc::UnboundedSender<WireMessage>,
    event_tx: mpsc::UnboundedSender<QuickEvent>,
    ws_url: String,
    room: String,
    stun_servers: Vec<String>,
) -> Result<(), QuickError> {
    println!("Quick: Entering background connection loop");
    // Initialize the UDP socket for media transmission
    let socket = bind_local_socket().await?;
    println!("Quick: Bound local UDP socket at {}", socket.local_addr()?);

    // Prepare local tracks and candidates before starting the loop
    // IMPORTANT: Only the Initiator should prepare media tracks BEFORE the handshake.
    // The Responder must wait to receive the offer first, then accept it.
    {
        let core_guard = core.lock().await;
        if core_guard.signaling_handler.role == crate::signaling::SignalingRole::Initiator {
            println!("Quick: Initiator - initializing local media tracks (audio + video sendrecv)");
            drop(core_guard);
            initialize_local_media(&core).await.map_err(QuickError::Protocol)?;
            println!("Quick: Local media tracks initialized");
        } else {
            println!("Quick: Responder - deferring media track initialization until after offer received");
            drop(core_guard);
        }
    }
    println!("Quick: Creating local ICE candidates (host + srflx)");
    let local_ip = add_local_candidates(&core, &socket, &ws_tx, &ws_url, &stun_servers).await.map_err(QuickError::Protocol)?;

    let mut timeout = Instant::now() + Duration::from_millis(100);
    let mut udp_buf = vec![0u8; 2000];
    let mut last_udp_log = Instant::now();

    // Keep an outgoing buffer for websocket messages when WS is down.
    let mut pending_outbox: VecDeque<WireMessage> = VecDeque::new();
    let mut ws_alive = true;
    // Reconnect scheduling: attempt one reconnect per main-loop iteration when due
    let mut reconnect_backoff_ms: u64 = 250;
    let mut next_reconnect_time = Instant::now();

    loop {
        tokio::select! {
            // Check for graceful shutdown signal
            _ = &mut shutdown_rx => break,

            // Handle outgoing signaling messages from the core to the WebSocket
            Some(wire) = ws_rx.recv() => {
                // If WS is alive try to send, otherwise buffer
                if ws_alive {
                    println!("Quick: Sending websocket signaling message: {:?}", wire);
                    match ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
                        Utf8Bytes::from(serde_json::to_string(&wire).unwrap_or_default()),
                    )).await {
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("Quick: WS send failed, buffering message and marking WS dead: {:?}", e);
                            ws_alive = false;
                            pending_outbox.push_back(wire);
                        }
                    }
                } else {
                    // Buffer while WS is down
                    println!("Quick: WS down, buffering outgoing signaling message");
                    pending_outbox.push_back(wire);
                }
            }

            // Handle incoming signaling messages from the WebSocket
            Some(ws_msg) = ws_read.next(), if ws_alive => {
                match ws_msg {
                    Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                        println!("Quick: Incoming websocket signaling text: {}", text);
                        if let Ok(parsed) = serde_json::from_str::<WireMessage>(&text) {
                            if let Err(err) = dispatch_websocket_message_to_core(&core, parsed, &ws_tx).await {
                                eprintln!("Quick: ERROR dispatching signaling message: {}", err);
                                core.lock().await.signaling_handler.set_error(err);
                            }
                        } else {
                            println!("Quick: Failed to parse websocket signaling message");
                        }
                    }
                    Ok(_) => {
                        // ignore other frame types
                    }
                    Err(e) => {
                        eprintln!("Quick: WebSocket read error: {:?}. Marking WS dead and attempting reconnect.", e);
                        ws_alive = false;
                    }
                }
            }

            // Handle incoming UDP media packets (STUN, DTLS, RTP)
            Ok((n, source)) = socket.recv_from(&mut udp_buf) => {
                // Feed the raw bytes into the str0m engine
                let mut destination = socket.local_addr()?;
                destination.set_ip(local_ip);
                
                let contents = &udp_buf[..n];
                
                // Throttled heartbeat log
                if last_udp_log.elapsed() >= Duration::from_secs(2) {
                    println!("Quick Status: Receiving UDP ({} bytes from {})", n, source);
                    last_udp_log = Instant::now();
                }

                if let Ok(receive) = str0m::net::Receive::new(str0m::net::Protocol::Udp, source, destination, contents) {
                    let input = str0m::Input::Receive(Instant::now(), receive);
                    let _ = core.lock().await.handle_input(input);
                } else {
                    println!("Quick: Failed to parse UDP packet into str0m receive frame");
                }
            }

            // Drive the internal clock of the WebRTC engine
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(timeout)) => {
                let _ = core.lock().await.handle_input(str0m::Input::Timeout(Instant::now()));
            }
        }

        // After every event, we check if the engine has data it wants to send out
        timeout = flush_core_outputs_to_network(&core, &socket, &ws_tx, &event_tx).await;

        // If WS died, schedule a single reconnect attempt per loop iteration (avoids blocking UDP loop)
        if !ws_alive {
            if Instant::now() >= next_reconnect_time {
                // attempt one reconnect
                match connect_async(&ws_url).await {
                    Ok((stream, _)) => {
                        println!("Quick: Reconnected websocket to {}", ws_url);
                        let (new_ws_write, new_ws_read) = stream.split();
                        ws_write = new_ws_write;
                        ws_read = new_ws_read;
                        ws_alive = true;

                        // Re-join room to re-establish signaling session
                        let join = WireMessage::Join { room: room.clone() };
                        if let Err(e) = ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
                            Utf8Bytes::from(serde_json::to_string(&join).unwrap_or_default()),
                        )).await {
                            eprintln!("Quick: Failed to send Join after reconnect: {:?}", e);
                            ws_alive = false;
                            // schedule next attempt
                            next_reconnect_time = Instant::now() + Duration::from_millis(reconnect_backoff_ms);
                            reconnect_backoff_ms = std::cmp::min(reconnect_backoff_ms * 2, 5000);
                        } else {
                            // Drain pending outbox (best-effort)
                            while let Some(pending) = pending_outbox.pop_front() {
                                if let Err(e) = ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
                                    Utf8Bytes::from(serde_json::to_string(&pending).unwrap_or_default()),
                                )).await {
                                    eprintln!("Quick: Failed to send pending message after reconnect: {:?}", e);
                                    // Put it back and mark ws dead to retry later
                                    pending_outbox.push_front(pending);
                                    ws_alive = false;
                                    next_reconnect_time = Instant::now() + Duration::from_millis(reconnect_backoff_ms);
                                    reconnect_backoff_ms = std::cmp::min(reconnect_backoff_ms * 2, 5000);
                                    break;
                                }
                            }

                            if ws_alive {
                                // reset backoff on success
                                reconnect_backoff_ms = 250;
                                next_reconnect_time = Instant::now();
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Quick: Reconnect attempt failed: {:?}. Scheduling retry.", e);
                        next_reconnect_time = Instant::now() + Duration::from_millis(reconnect_backoff_ms);
                        reconnect_backoff_ms = std::cmp::min(reconnect_backoff_ms * 2, 5000);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn initialize_local_media(core: &std::sync::Arc<Mutex<RAMSCore>>) -> Result<(), String> {
    let mut core = core.lock().await;
    println!("Quick: Adding audio SendRecv media line");
    core.add_audio(Direction::SendRecv);
    println!("Quick: Adding video SendRecv media line");
    core.add_video(Direction::SendRecv);
    Ok(())
}

async fn add_local_candidates(
    core: &std::sync::Arc<Mutex<RAMSCore>>,
    socket: &UdpSocket,
    ws_tx: &mpsc::UnboundedSender<WireMessage>,
    ws_url: &str,
    stun_servers: &[String],
) -> Result<IpAddr, String> {
    let mut addr = socket.local_addr().map_err(|e| e.to_string())?;
    println!("Quick: Local UDP candidate base address before IP patch: {}", addr);
    if addr.ip().is_unspecified() {
        if let Some(local_ip) = get_local_ip(ws_url) {
            println!("Quick: Discovered local IP: {}", local_ip);
            addr.set_ip(local_ip);
        } else {
            println!("Quick: Failed to discover local IP, falling back to 127.0.0.1");
            addr.set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
        }
    }
    
    // 1. Add Host Candidate
    let host_candidate = str0m::Candidate::host(addr, "udp").map_err(|e| e.to_string())?;
    println!("Quick: Created host ICE candidate: {:?}", host_candidate);

    {
        let mut guard = core.lock().await;
        let _ = guard.rtc.add_local_candidate(host_candidate.clone());
    }

    println!("Quick: Sending host ICE candidate: {}", host_candidate.to_sdp_string());
    let _ = ws_tx.send(WireMessage::Candidate {
        candidate: host_candidate.to_sdp_string(),
    });

    // 2. Discover and Add srflx Candidates via STUN
    if !stun_servers.is_empty() {
        println!("Quick: Starting STUN candidate discovery via {} servers...", stun_servers.len());
        let srflx_candidates = discover_srflx_candidates(socket, stun_servers).await;
        for cand in srflx_candidates {
            {
                let mut guard = core.lock().await;
                let _ = guard.rtc.add_local_candidate(cand.clone());
            }
            println!("Quick: Sending srflx ICE candidate: {}", cand.to_sdp_string());
            let _ = ws_tx.send(WireMessage::Candidate {
                candidate: cand.to_sdp_string(),
            });
        }
    }

    Ok(addr.ip())
}

/// Translates incoming signaling messages (Offers, Answers, Candidates) into actions for the RAMSCore.
async fn dispatch_websocket_message_to_core(
    core: &std::sync::Arc<Mutex<RAMSCore>>,
    msg: WireMessage,
    ws_tx: &mpsc::UnboundedSender<WireMessage>,
) -> Result<(), String> {
    match msg {
        WireMessage::PeerJoined => {
            // When a peer joins, the Initiator kicks off the negotiation by creating an offer.
            println!("Quick: PeerJoined received");
            let mut core = core.lock().await;
            if core.signaling_handler.role == SignalingRole::Initiator {
                println!("Quick: Initiator creating offer due to peer join");
                let offer = core.create_offer()?;
                println!("Quick: Sending SDP offer: {:?}", offer);
                let _ = ws_tx.send(offer.into());
            } else {
                println!("Quick: PeerJoined ignored because this side is not initiator");
            }
            Ok(())
        }
        msg @ (WireMessage::Offer { .. } | WireMessage::Answer { .. } | WireMessage::Candidate { .. }) => {
            // Pass signaling payloads directly to the core state machine.
            if let Ok(signaling) = SignalingMessage::try_from(msg) {
                println!("Quick: Received remote signaling: {:?}", signaling);
                let mut core = core.lock().await;
                if let Some(response) = core.handle_signaling(signaling)? {
                    println!("Quick: Sending signaling response: {:?}", response);
                    let _ = ws_tx.send(response.into());
                } else {
                    println!("Quick: Signal consumed without immediate response");
                }
            } else {
                println!("Quick: Ignored non-signaling websocket message");
            }
            Ok(())
        }
        WireMessage::Joined { .. } | WireMessage::Join { .. } => Ok(()),
    }
}

/// Polls the RAMSCore for pending work (transmissions, timeouts, events) and executes it.
/// PERF: Limits iterations to prevent busy-waiting during heavy media load on low-spec hardware.
async fn flush_core_outputs_to_network(
    core: &std::sync::Arc<Mutex<RAMSCore>>,
    socket: &UdpSocket,
    _ws_tx: &mpsc::UnboundedSender<WireMessage>,
    event_tx: &mpsc::UnboundedSender<QuickEvent>,
) -> Instant {
    let mut next_timeout = Instant::now() + Duration::from_millis(100);
    let mut event_count = 0;
    const MAX_EVENTS_PER_DRAIN: usize = 32; // Prevent busy-wait on high-throughput systems

    loop {
        // Exit early if we've processed too many events to prevent stalling other tasks
        if event_count >= MAX_EVENTS_PER_DRAIN {
            break;
        }

        let output = match core.lock().await.poll_output() {
            Ok(output) => output,
            Err(err) => {
                println!("Quick: poll_output failed: {:?}", err);
                break;
            }
        };

        match output {
            str0m::Output::Timeout(t) => {
                // The engine tells us when it needs to be woken up next.
                next_timeout = t;
                break;
            }
            str0m::Output::Transmit(transmit) => {
                // Send encrypted WebRTC data (RTP/RTCP/DTLS) over the UDP socket.
                let _ = socket.send_to(&transmit.contents, transmit.destination).await;
                event_count += 1;
            }
            str0m::Output::Event(event) => {
                // High-level events for the user application.
                match event {
                    str0m::Event::Connected => {
                        println!("Quick: ========== DTLS HANDSHAKE COMPLETE ==========");
                        println!("Quick: WebRTC media connection fully established");
                        println!("Quick: Ready for RTP/RTCP media transmission");
                        println!("Quick: =============================================");
                        let _ = event_tx.send(QuickEvent::Connected);
                    }
                    str0m::Event::IceConnectionStateChange(state) => {
                        println!("Quick: ICE State Change: {:?}", state);
                        match state {
                            str0m::IceConnectionState::Connected | str0m::IceConnectionState::Completed => {
                                println!("Quick: ICE connection ready, waiting for DTLS handshake...");
                            }
                            _ => {}
                        }
                        let _ = event_tx.send(QuickEvent::IceState(format!("{:?}", state)));
                    }
                    str0m::Event::MediaData(data) => {
                        let kind = {
                            let core_guard = core.lock().await;
                            if Some(data.mid) == core_guard.video_mid {
                                "video"
                            } else if Some(data.mid) == core_guard.audio_mid {
                                "audio"
                            } else {
                                "unknown"
                            }
                        };
                        if kind != "unknown" {
                            let _ = event_tx.send(QuickEvent::MediaData(kind.to_string(), data.data));
                        } else {
                            println!("Quick: Received MediaData for unknown MID {:?}", data.mid);
                        }
                    }
                    str0m::Event::MediaAdded(media_added) => {
                        println!(
                            "Quick: MediaAdded mid={:?}, kind={:?}, direction={:?}",
                            media_added.mid,
                            media_added.kind,
                            media_added.direction
                        );
                    }
                    other => {
                        println!("Quick: Other str0m event: {:?}", other);
                    }
                }
                event_count += 1;
            }
        }
    }

    next_timeout
}

async fn bind_local_socket() -> Result<UdpSocket, std::io::Error> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
    UdpSocket::bind(addr).await
}

fn get_local_ip(ws_url: &str) -> Option<IpAddr> {
    use std::net::UdpSocket as StdUdpSocket;
    
    // Try to parse the host from the ws_url to use as a hint
    let host = ws_url.trim_start_matches("ws://")
        .trim_start_matches("wss://")
        .split(':')
        .next()?;

    let socket = StdUdpSocket::bind("0.0.0.0:0").ok()?;
    
    // Try to connect to the signaling server's host to find the local interface
    if socket.connect(format!("{}:80", host)).is_ok() {
        if let Ok(addr) = socket.local_addr() {
            if !addr.ip().is_unspecified() && !addr.ip().is_loopback() {
                return Some(addr.ip());
            }
        }
    }

    // Fallback to 8.8.8.8 if the signaling server hint fails
    if let Ok(_) = socket.connect("8.8.8.8:80") {
        if let Ok(addr) = socket.local_addr() {
            if !addr.ip().is_unspecified() && !addr.ip().is_loopback() {
                return Some(addr.ip());
            }
        }
    }

    // Linux-specific fallback: use 'hostname -I' to get all IPs
    if let Ok(output) = std::process::Command::new("hostname").arg("-I").output() {
        let s = String::from_utf8_lossy(&output.stdout);
        for ip_str in s.split_whitespace() {
            if let Ok(ip) = ip_str.parse::<IpAddr>() {
                if !ip.is_loopback() && ip.is_ipv4() {
                    return Some(ip);
                }
            }
        }
    }

    None
}

async fn discover_srflx_candidates(
    socket: &UdpSocket,
    stun_servers: &[String],
) -> Vec<str0m::Candidate> {
    let mut candidates = Vec::new();
    let local_addr = match socket.local_addr() {
        Ok(a) => a,
        Err(_) => return candidates,
    };

    for stun_server in stun_servers {
        let stun_addr = stun_server.trim_start_matches("stun:");
        
        // Resolve stun_addr to SocketAddr
        if let Ok(addrs) = tokio::net::lookup_host(stun_addr).await {
            if let Some(dest) = addrs.into_iter().next() {
                println!("Quick: Sending STUN Binding Request to {}", dest);
                
                // Simple STUN Binding Request (RFC 5389)
                let mut request = [0u8; 20];
                request[0..2].copy_from_slice(&0x0001u16.to_be_bytes()); // Binding Request
                request[4..8].copy_from_slice(&0x2112A442u32.to_be_bytes()); // Magic Cookie
                // Transaction ID (semi-random)
                for i in 8..20 { request[i] = (i * 7) as u8; }

                if socket.send_to(&request, dest).await.is_ok() {
                    let mut buf = [0u8; 1500];
                    // Wait for response
                    if let Ok(Ok((n, _))) = tokio::time::timeout(Duration::from_millis(800), socket.recv_from(&mut buf)).await {
                        if let Some(srflx_addr) = parse_stun_srflx(&buf[..n]) {
                            println!("Quick: Discovered srflx candidate: {}", srflx_addr);
                            if let Ok(cand) = str0m::Candidate::server_reflexive(srflx_addr, local_addr, "udp") {
                                candidates.push(cand);
                            }
                        }
                    }
                }
            }
        }
    }
    candidates
}

pub fn parse_stun_srflx(data: &[u8]) -> Option<SocketAddr> {
    if data.len() < 20 { return None; }
    // Check if it's a Binding Success Response (0x0101)
    if data[0..2] != [0x01, 0x01] { return None; }
    
    let length = u16::from_be_bytes([data[2], data[3]]) as usize;
    let mut pos = 20;
    while pos + 4 <= 20 + length {
        let attr_type = u16::from_be_bytes([data[pos], data[pos+1]]);
        let attr_len = u16::from_be_bytes([data[pos+2], data[pos+3]]) as usize;
        pos += 4;
        
        if attr_type == 0x0020 { // XOR-MAPPED-ADDRESS
            if attr_len >= 8 {
                let family = data[pos+1];
                let x_port = u16::from_be_bytes([data[pos+2], data[pos+3]]);
                let port = x_port ^ 0x2112;
                if family == 0x01 { // IPv4
                    let x_ip = [data[pos+4], data[pos+5], data[pos+6], data[pos+7]];
                    let ip = Ipv4Addr::new(
                        x_ip[0] ^ 0x21, x_ip[1] ^ 0x12, x_ip[2] ^ 0xA4, x_ip[3] ^ 0x42
                    );
                    return Some(SocketAddr::new(IpAddr::V4(ip), port));
                }
            }
        } else if attr_type == 0x0001 { // MAPPED-ADDRESS
             if attr_len >= 8 {
                let family = data[pos+1];
                let port = u16::from_be_bytes([data[pos+2], data[pos+3]]);
                if family == 0x01 { // IPv4
                    let ip = Ipv4Addr::new(data[pos+4], data[pos+5], data[pos+6], data[pos+7]);
                    return Some(SocketAddr::new(IpAddr::V4(ip), port));
                }
            }
        }
        pos += (attr_len + 3) & !3; // Padding
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wire_message_serialization() {
        let msg = WireMessage::Join { room: "test-room".to_string() };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"join\""));
        assert!(json.contains("\"room\":\"test-room\""));
    }

    #[test]
    fn test_stun_parsing_xor_mapped() {
        // A real-world XOR-MAPPED-ADDRESS response snippet
        // Magic Cookie: 0x2112A442
        // XOR'd Port: 0x2112 ^ 12345 (0x3039) = 0x112B
        // XOR'd IP: 192.168.1.1 ^ Magic Cookie
        let mut data = vec![0u8; 40];
        data[0..2].copy_from_slice(&[0x01, 0x01]); // Binding Success
        data[2..4].copy_from_slice(&20u16.to_be_bytes()); // Length
        
        let attr_pos = 20;
        data[attr_pos..attr_pos+2].copy_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED-ADDRESS
        data[attr_pos+2..attr_pos+4].copy_from_slice(&8u16.to_be_bytes()); // Length
        data[attr_pos+5] = 0x01; // IPv4
        
        // Port 12345 (0x3039) -> XOR with 0x2112 = 0x112B
        data[attr_pos+6..attr_pos+8].copy_from_slice(&0x112Bu16.to_be_bytes());
        
        // IP 1.2.3.4 -> XOR with 0x2112A442
        // 1 ^ 0x21 = 0x20
        // 2 ^ 0x12 = 0x10
        // 3 ^ 0xA4 = 0xA7
        // 4 ^ 0x42 = 0x46
        data[attr_pos+8..attr_pos+12].copy_from_slice(&[0x20, 0x10, 0xA7, 0x46]);

        let result = parse_stun_srflx(&data).expect("Failed to parse STUN");
        assert_eq!(result.port(), 12345);
        assert_eq!(result.ip().to_string(), "1.2.3.4");
    }

    #[test]
    fn test_stun_parsing_mapped_address() {
        let mut data = vec![0u8; 40];
        data[0..2].copy_from_slice(&[0x01, 0x01]); // Binding Success
        data[2..4].copy_from_slice(&20u16.to_be_bytes()); // Length
        
        let attr_pos = 20;
        data[attr_pos..attr_pos+2].copy_from_slice(&0x0001u16.to_be_bytes()); // MAPPED-ADDRESS
        data[attr_pos+2..attr_pos+4].copy_from_slice(&8u16.to_be_bytes()); // Length
        data[attr_pos+5] = 0x01; // IPv4
        data[attr_pos+6..attr_pos+8].copy_from_slice(&12345u16.to_be_bytes()); // Port
        data[attr_pos+8..attr_pos+12].copy_from_slice(&[1, 2, 3, 4]); // IP

        let result = parse_stun_srflx(&data).expect("Failed to parse STUN");
        assert_eq!(result.port(), 12345);
        assert_eq!(result.ip().to_string(), "1.2.3.4");
    }
}
