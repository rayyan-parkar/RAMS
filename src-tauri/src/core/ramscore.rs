use crate::signaling::SignalingHandler;

pub struct RAMSCore {
    rtc: str0m::Rtc,
    signaling_handler: SignalingHandler,
    pub pending_remote_candidates: Vec<str0m::Candidate>,
}

impl RAMSCore{
    
}