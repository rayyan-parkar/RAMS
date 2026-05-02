use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
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
    MediaData(str0m::media::Mid, usize),
}

/// Handle to a running quick connection.
pub struct QuickConnection {
    core: std::sync::Arc<Mutex<RAMSCore>>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
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
enum WireMessage {
    Join {
        room: String,
    },
    Joined {
        room: String,
        #[serde(rename = "isInitiator")]
        is_initiator: bool,
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
    let (ws_stream, _) = connect_async(ws_url).await?;
    let (mut ws_write, mut ws_read) = ws_stream.split();

    // 2. Negotiate Role (Join Room)
    // We wait for the server to tell us if we are the caller (Initiator) or receiver (Responder)
    let role = negotiate_signaling_role(&mut ws_read, &mut ws_write, room).await?;

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
    let ws_task = tokio::spawn(async move {
        if let Err(e) = run_connection_loop(task_core, shutdown_rx, ws_rx, ws_read, ws_write, ws_tx, task_event_tx, task_ws_url).await {
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
) -> Result<SignalingRole, QuickError>
where
    R: StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    W: SinkExt<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let join = WireMessage::Join { room: room.to_string() };
    ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
        Utf8Bytes::from(serde_json::to_string(&join)?),
    )).await?;

    while let Some(msg) = ws_read.next().await {
        let msg = msg?;
        if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
            let parsed: WireMessage = serde_json::from_str(&text)?;
            if let WireMessage::Joined { is_initiator, .. } = parsed {
                return Ok(if is_initiator {
                    SignalingRole::Initiator
                } else {
                    SignalingRole::Responder
                });
            }
        }
    }

    Err(QuickError::Protocol("Signaling server closed before joining".to_string()))
}

/// The main event loop that orchestrates all I/O for the WebRTC session.
/// It uses tokio::select! to multiplex between signaling, media data, and library timeouts.
async fn run_connection_loop<R, W>(
    core: std::sync::Arc<Mutex<RAMSCore>>,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut ws_rx: mpsc::UnboundedReceiver<WireMessage>,
    mut ws_read: R,
    mut ws_write: W,
    ws_tx: mpsc::UnboundedSender<WireMessage>,
    event_tx: mpsc::UnboundedSender<QuickEvent>,
    ws_url: String,
) -> Result<(), QuickError>
where
    R: StreamExt<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    W: SinkExt<tokio_tungstenite::tungstenite::Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    println!("Quick: Entering background connection loop");
    // Initialize the UDP socket for media transmission
    let socket = bind_local_socket().await?;

    // Prepare local tracks and candidates before starting the loop
    initialize_local_media(&core).await.map_err(QuickError::Protocol)?;
    add_local_candidate(&core, &socket, &ws_tx, &ws_url).map_err(QuickError::Protocol)?;

    let mut timeout = Instant::now() + Duration::from_millis(100);
    let mut udp_buf = vec![0u8; 2000];

    loop {
        tokio::select! {
            // Check for graceful shutdown signal
            _ = &mut shutdown_rx => break,

            // Handle outgoing signaling messages from the core to the WebSocket
            Some(wire) = ws_rx.recv() => {
                ws_write.send(tokio_tungstenite::tungstenite::Message::Text(
                    Utf8Bytes::from(serde_json::to_string(&wire).unwrap_or_default()),
                )).await?;
            }

            // Handle incoming signaling messages from the WebSocket
            Some(ws_msg) = ws_read.next() => {
                let ws_msg = ws_msg?;
                if let tokio_tungstenite::tungstenite::Message::Text(text) = ws_msg {
                    if let Ok(parsed) = serde_json::from_str::<WireMessage>(&text) {
                        if let Err(err) = dispatch_websocket_message_to_core(&core, parsed, &ws_tx).await {
                            core.lock().await.signaling_handler.set_error(err);
                        }
                    }
                }
            }

            // Handle incoming UDP media packets (STUN, DTLS, RTP)
            Ok((n, source)) = socket.recv_from(&mut udp_buf) => {
                let destination = socket.local_addr()?;
                let contents = &udp_buf[..n];

                // Feed the raw bytes into the str0m engine
                if let Ok(receive) = str0m::net::Receive::new(str0m::net::Protocol::Udp, source, destination, contents) {
                    let input = str0m::Input::Receive(Instant::now(), receive);
                    let _ = core.lock().await.handle_input(input);
                }
            }

            // Drive the internal clock of the WebRTC engine
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(timeout)) => {
                let _ = core.lock().await.handle_input(str0m::Input::Timeout(Instant::now()));
            }
        }

        // After every event, we check if the engine has data it wants to send out
        timeout = flush_core_outputs_to_network(&core, &socket, &ws_tx, &event_tx).await;
    }

    Ok(())
}

async fn initialize_local_media(core: &std::sync::Arc<Mutex<RAMSCore>>) -> Result<(), String> {
    let mut core = core.lock().await;
    core.add_audio(Direction::SendRecv);
    core.add_video(Direction::SendRecv);
    Ok(())
}

fn add_local_candidate(
    core: &std::sync::Arc<Mutex<RAMSCore>>,
    socket: &UdpSocket,
    ws_tx: &mpsc::UnboundedSender<WireMessage>,
    ws_url: &str,
) -> Result<(), String> {
    let mut addr = socket.local_addr().map_err(|e| e.to_string())?;
    if addr.ip().is_unspecified() {
        if let Some(local_ip) = get_local_ip(ws_url) {
            println!("Quick: Discovered local IP: {}", local_ip);
            addr.set_ip(local_ip);
        } else {
            println!("Quick: Failed to discover local IP, falling back to 127.0.0.1");
            addr.set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));
        }
    }
    let candidate = str0m::Candidate::host(addr, "udp").map_err(|e| e.to_string())?;

    if let Ok(mut guard) = core.try_lock() {
        let _ = guard.rtc.add_local_candidate(candidate.clone());
    }

    println!("Quick: Sending local ICE candidate: {}", candidate.to_sdp_string());
    let _ = ws_tx.send(WireMessage::Candidate {
        candidate: candidate.to_sdp_string(),
    });

    Ok(())
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
            let mut core = core.lock().await;
            if core.signaling_handler.role == SignalingRole::Initiator {
                let offer = core.create_offer()?;
                let _ = ws_tx.send(offer.into());
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
                }
            }
            Ok(())
        }
        WireMessage::Joined { .. } | WireMessage::Join { .. } => Ok(()),
    }
}

/// Polls the RAMSCore for pending work (transmissions, timeouts, events) and executes it.
/// This is the "driver" part of the Sans-I/O pattern.
async fn flush_core_outputs_to_network(
    core: &std::sync::Arc<Mutex<RAMSCore>>,
    socket: &UdpSocket,
    _ws_tx: &mpsc::UnboundedSender<WireMessage>,
    event_tx: &mpsc::UnboundedSender<QuickEvent>,
) -> Instant {
    let mut next_timeout = Instant::now() + Duration::from_millis(100);

    loop {
        let output = match core.lock().await.poll_output() {
            Ok(output) => output,
            Err(_) => break,
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
            }
            str0m::Output::Event(event) => {
                // High-level events for the user application.
                match event {
                    str0m::Event::Connected => {
                        println!("Quick: WebRTC Connected!");
                        let _ = event_tx.send(QuickEvent::Connected);
                    }
                    str0m::Event::IceConnectionStateChange(state) => {
                        println!("Quick: ICE State Change: {:?}", state);
                        let _ = event_tx.send(QuickEvent::IceState(format!("{:?}", state)));
                    }
                    str0m::Event::MediaData(data) => {
                        // Route incoming str0m MediaData events
                        let _ = event_tx.send(QuickEvent::MediaData(data.mid, data.data.len()));
                    }
                    _ => {
                        // println!("Quick: Other str0m Event: {:?}", event);
                    }
                }
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
