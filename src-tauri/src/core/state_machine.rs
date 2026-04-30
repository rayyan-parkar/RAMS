use str0m::{Rtc, RtcConfig, RtcError};

/// The Core level logic for RAMS.
/// This acts as a wrapper over the str0m::Rtc state machine.
/// It provides pure, sans-IO WebRTC primitives with default properties.
pub struct StateMachine {
    pub rtc: Rtc,
}

impl StateMachine {
    /// Create a new sans-IO StateMachine wrapper
    pub fn new() -> Result<Self, RtcError> {
        let mut config = RtcConfig::new();
        // Potential future config properties could be exposed here independently of str0m
        
        Ok(Self {
            rtc: config.build(),
        })
    }
}
