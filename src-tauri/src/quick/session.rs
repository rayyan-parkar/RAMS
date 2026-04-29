use crate::signaling::client::SignalingClient;

pub struct RamsSession {
    pub is_initiator: bool,
    pub signaling: SignalingClient,
}
