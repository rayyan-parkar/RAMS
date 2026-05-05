use rams_lib::core::ramscore::RAMSCore;
use rams_lib::signaling::signaling_handler::{SignalingRole, SignalingState};
use str0m::media::Direction;

#[test]
fn test_core_initialization() {
    let core = RAMSCore::new(SignalingRole::Initiator);
    assert_eq!(core.signaling_handler.role, SignalingRole::Initiator);
    assert_eq!(core.signaling_handler.state, SignalingState::Idle);
    assert!(core.video_mid.is_none());
    assert!(core.audio_mid.is_none());
}

#[test]
fn test_media_staging() {
    let mut core = RAMSCore::new(SignalingRole::Initiator);
    
    core.add_video(Direction::SendRecv);
    assert_eq!(core.pending_video_direction, Some(Direction::SendRecv));

    core.add_audio(Direction::SendRecv);
    assert_eq!(core.pending_audio_direction, Some(Direction::SendRecv));
}

#[test]
fn test_offer_creation_fails_for_responder() {
    let mut core = RAMSCore::new(SignalingRole::Responder);
    core.add_video(Direction::SendRecv);
    let result = core.create_offer();
    assert!(result.is_err());
    assert_eq!(result.err().unwrap(), "Only the Initiator can create an offer");
}

#[test]
fn test_ice_candidate_buffering() {
    let mut core = RAMSCore::new(SignalingRole::Initiator);
    
    // In Idle state, candidates should be buffered
    let cand = str0m::Candidate::host("127.0.0.1:1234".parse().unwrap(), "udp").unwrap();
    core.buffer_or_apply_candidate(cand.clone());
    
    assert_eq!(core.pending_remote_candidates.len(), 1);
    assert_eq!(core.pending_remote_candidates[0], cand);
}

#[test]
fn test_handle_candidate_invalid_format() {
    let mut core = RAMSCore::new(SignalingRole::Initiator);
    // Malformed SDP candidate string
    let result = core.handle_candidate("not a candidate".to_string());
    assert!(result.is_err());
}
