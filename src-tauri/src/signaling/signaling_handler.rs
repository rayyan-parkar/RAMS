/// Represents the role of the signaling handler in a WebRTC session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingRole {
    /// Initiates an SDP Offer.
    Initiator,

    /// Waits for an SDP Offer and creates an Answer.
    Responder,
}

/// Represents the logical state of the signaling process
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingState {
    /// Initial state, waiting for peer (Initiator) or waiting for offer (Responder)
    Idle,

    /// Initiator only: Offer sent, waiting for remote SDP Answer
    WaitingForAnswer,
    
    /// SDP handshake complete, now exchanging ICE Candidates
    TricklingIce,
    
    /// ICE candidates exchanged and a connection has been established
    Stable,

    /// Something went wrong
    Error(String),
}

pub struct SignalingHandler {
    pub role: SignalingRole,
    pub state: SignalingState,
}

impl SignalingHandler {
    pub fn new(role: SignalingRole) -> Self {
        Self {
            role,
            state: SignalingState::Idle,
        }
    }

    /// Advances the state machine. Returns an Error if the transition is invalid.
    pub fn advance(&mut self, next: SignalingState) -> Result<(), String> {
        let is_valid = match (&self.role, &self.state, &next) {
            // Initiator transitions
            (SignalingRole::Initiator, SignalingState::Idle, SignalingState::WaitingForAnswer) => true,
            (SignalingRole::Initiator, SignalingState::WaitingForAnswer, SignalingState::TricklingIce) => true,
            
            // Responder transitions (skips WaitingForAnswer)
            (SignalingRole::Responder, SignalingState::Idle, SignalingState::TricklingIce) => true,
            
            // Shared transitions
            (_, SignalingState::TricklingIce, SignalingState::Stable) => true,
            
            // We can always transition to Error from anywhere
            (_, _, SignalingState::Error(_)) => true,
            
            _ => false,
        };

        if is_valid {
            self.state = next;
            Ok(())
        } else {
            Err(format!(
                "Invalid transition {:?} -> {:?} for role {:?}",
                self.state, next, self.role
            ))
        }
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.state = SignalingState::Error(message.into());
    }

    pub fn reset(&mut self) {
        self.state = SignalingState::Idle;
    }

    pub fn is_stable(&self) -> bool {
        matches!(self.state, SignalingState::Stable)
    }

    pub fn is_error(&self) -> bool {
        matches!(self.state, SignalingState::Error(_))
    }
}
