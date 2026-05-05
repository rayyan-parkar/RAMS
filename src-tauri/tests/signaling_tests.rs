use rams_lib::signaling::signaling_handler::{SignalingHandler, SignalingRole, SignalingState};

#[test]
fn test_initiator_flow() {
    let mut handler = SignalingHandler::new(SignalingRole::Initiator);
    assert_eq!(handler.state, SignalingState::Idle);

    // Idle -> WaitingForAnswer
    handler.advance(SignalingState::WaitingForAnswer).unwrap();
    assert_eq!(handler.state, SignalingState::WaitingForAnswer);

    // WaitingForAnswer -> TricklingIce
    handler.advance(SignalingState::TricklingIce).unwrap();
    assert_eq!(handler.state, SignalingState::TricklingIce);

    // TricklingIce -> Stable
    handler.advance(SignalingState::Stable).unwrap();
    assert!(handler.is_stable());
}

#[test]
fn test_responder_flow() {
    let mut handler = SignalingHandler::new(SignalingRole::Responder);
    assert_eq!(handler.state, SignalingState::Idle);

    // Idle -> TricklingIce (Responder skips WaitingForAnswer)
    handler.advance(SignalingState::TricklingIce).unwrap();
    assert_eq!(handler.state, SignalingState::TricklingIce);

    // TricklingIce -> Stable
    handler.advance(SignalingState::Stable).unwrap();
    assert!(handler.is_stable());
}

#[test]
fn test_invalid_transitions() {
    let mut handler = SignalingHandler::new(SignalingRole::Initiator);
    
    // Initiator cannot skip WaitingForAnswer
    assert!(handler.advance(SignalingState::TricklingIce).is_err());

    let mut handler = SignalingHandler::new(SignalingRole::Responder);
    // Responder cannot go to WaitingForAnswer
    assert!(handler.advance(SignalingState::WaitingForAnswer).is_err());
}

#[test]
fn test_error_state() {
    let mut handler = SignalingHandler::new(SignalingRole::Initiator);
    handler.set_error("test failure");
    assert!(handler.is_error());
    
    if let SignalingState::Error(msg) = handler.state {
        assert_eq!(msg, "test failure");
    } else {
        panic!("Not in error state");
    }
}
