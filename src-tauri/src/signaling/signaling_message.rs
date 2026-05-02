use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SignalingMessage {
    Offer { sdp: String },
    Answer { sdp: String },
    Candidate { candidate: String },
}
