use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;

use super::protocol::SignalingMessage;

/// A simple WebSocket client for exchanging signaling messages setup by RamsBuilder.
/// Small note: tx and rx are standard naming conventions that mean transmit and receive!
pub struct SignalingClient {
    tx: mpsc::Sender<SignalingMessage>,
    rx: mpsc::Receiver<SignalingMessage>,
}

impl SignalingClient {
    /// Connects to the WebSocket URL and spawns background tasks to handle incoming/outgoing messages
    pub async fn connect(url: &str) -> Result<Self, String> {
        let (ws_stream, _) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| format!("Failed to connect to signaling server: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // Channels for bridging synchronous code or other async tasks to the WS streams
        let (tx_in, mut tx_out) = mpsc::channel::<SignalingMessage>(32);
        let (tx_rx, rx_out) = mpsc::channel::<SignalingMessage>(32);

        // Task: Read from internal channel and send to WebSocket
        tokio::spawn(async move {
            while let Some(msg) = tx_out.recv().await {
                if let Ok(json) = serde_json::to_string(&msg) {
                    if write.send(tokio_tungstenite::tungstenite::Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
            }
        });

        // Task: Read from WebSocket and send to internal channel
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = msg {
                    if let Ok(signaling_msg) = serde_json::from_str::<SignalingMessage>(&text) {
                        if tx_rx.send(signaling_msg).await.is_err() {
                            break; // Receiver was dropped
                        }
                    }
                }
            }
        });

        Ok(Self {
            tx: tx_in,
            rx: rx_out,
        })
    }

    /// Enqueue a message to be sent to the signaling server
    pub async fn send(&self, msg: SignalingMessage) -> Result<(), String> {
        self.tx.send(msg).await.map_err(|_| "Client disconnected".into())
    }

    /// Wait for the next incoming signaling message
    pub async fn recv(&mut self) -> Option<SignalingMessage> {
        self.rx.recv().await
    }
}
