use serde::{Deserialize, Serialize};

/// Represents messages sent between peers via signaling server
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SignalingMessage {
    /// Request to join a specific room
    Join { room: String },
    /// Notification that the room was successfully joined
    Joined {
        room: String,
        #[serde(rename = "isInitiator")]
        is_initiator: bool,
    },
    /// Notification that another peer has joined the room
    PeerJoined,
    /// SDP Offer
    Offer { sdp: String },
    /// SDP Answer
    Answer { sdp: String },
    /// Trickle ICE Candidate
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        #[serde(rename = "sdpMLineIndex")]
        sdp_m_line_index: Option<u16>,
    },
    /// An error received from the signaling server
    Error { message: String },
}
