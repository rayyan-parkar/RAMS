use serde::{Deserialize, Serialize};

/// Wire-compatible representation of WebRTC signaling payloads for JSON serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SignalingMessage {
    /// Session Description Protocol (SDP) Offer.
    Offer { sdp: String },
    /// Session Description Protocol (SDP) Answer.
    Answer { sdp: String },
    /// Interactive Connectivity Establishment (ICE) candidate data.
    Candidate { candidate: String },
}
