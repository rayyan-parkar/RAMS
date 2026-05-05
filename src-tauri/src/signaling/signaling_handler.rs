/// Represents the role of the signaling handler in a WebRTC session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalingRole {
    /// Initiates an SDP Offer.
    Initiator,

    /// Responder role; processes SDP Offer and generates SDP Answer.
    Responder,
}

/// Represents the logical state of the JSEP signaling process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingState {
    /// Initial state: Ready for room assignment or waiting for incoming Offer.
    Idle,

    /// Initiator state: Local SDP Offer created and sent to signaling server.
    WaitingForAnswer,
    
    /// Handshake active: SDP Answer has been exchanged; ICE trickling is underway.
    TricklingIce,
    
    /// Signaling complete: Session description and ICE candidates are fully synchronized.
    Stable,

    /// Fatal error: Handshake or negotiation failed.
    Error(String),
}

/// Manages the WebRTC signaling state machine for a specific role.
pub struct SignalingHandler {
    /// The assigned role (Initiator/Responder) for this session.
    pub role: SignalingRole,
    /// The current state of the JSEP handshake.
    pub state: SignalingState,
}

impl SignalingHandler {
    /// Creates a new SignalingHandler with the specified role.
    ///
    /// # Example
    /// ```
    /// use rams_lib::signaling::signaling_handler::{SignalingHandler, SignalingRole, SignalingState};
    /// let handler = SignalingHandler::new(SignalingRole::Initiator);
    /// assert_eq!(handler.state, SignalingState::Idle);
    /// ```
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
