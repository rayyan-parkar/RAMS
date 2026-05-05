#![cfg(feature = "quick")]
use rams_lib::quick::quick_connect::{WireMessage, parse_stun_srflx};

#[test]
fn test_wire_message_serialization() {
    let msg = WireMessage::Join { room: "test-room".to_string() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"join\""));
    assert!(json.contains("\"room\":\"test-room\""));
}

#[test]
fn test_signaling_offer_serialization() {
    let msg = WireMessage::Offer { sdp: "v=0...".to_string() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"offer\""));
    assert!(json.contains("\"sdp\":\"v=0...\""));
}

#[test]
fn test_signaling_answer_serialization() {
    let msg = WireMessage::Answer { sdp: "v=0...".to_string() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"answer\""));
}

#[test]
fn test_signaling_candidate_serialization() {
    let msg = WireMessage::Candidate { candidate: "cand...".to_string() };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"candidate\""));
}

#[test]
fn test_stun_parsing_xor_mapped() {
    // A real-world XOR-MAPPED-ADDRESS response snippet
    // Magic Cookie: 0x2112A442
    // XOR'd Port: 0x2112 ^ 12345 (0x3039) = 0x112B
    let mut data = vec![0u8; 40];
    data[0..2].copy_from_slice(&[0x01, 0x01]); // Binding Success
    data[2..4].copy_from_slice(&20u16.to_be_bytes()); // Length
    
    let attr_pos = 20;
    data[attr_pos..attr_pos+2].copy_from_slice(&0x0020u16.to_be_bytes()); // XOR-MAPPED-ADDRESS
    data[attr_pos+2..attr_pos+4].copy_from_slice(&8u16.to_be_bytes()); // Length
    data[attr_pos+5] = 0x01; // IPv4
    
    // Port 12345 (0x3039) -> XOR with 0x2112 = 0x112B
    data[attr_pos+6..attr_pos+8].copy_from_slice(&0x112Bu16.to_be_bytes());
    
    // IP 1.2.3.4 -> XOR with 0x2112A442
    // 1 ^ 0x21 = 0x20
    // 2 ^ 0x12 = 0x10
    // 3 ^ 0xA4 = 0xA7
    // 4 ^ 0x42 = 0x46
    data[attr_pos+8..attr_pos+12].copy_from_slice(&[0x20, 0x10, 0xA7, 0x46]);

    let result = parse_stun_srflx(&data).expect("Failed to parse STUN");
    assert_eq!(result.port(), 12345);
    assert_eq!(result.ip().to_string(), "1.2.3.4");
}

#[test]
fn test_stun_parsing_mapped_address() {
    let mut data = vec![0u8; 40];
    data[0..2].copy_from_slice(&[0x01, 0x01]); // Binding Success
    data[2..4].copy_from_slice(&20u16.to_be_bytes()); // Length
    
    let attr_pos = 20;
    data[attr_pos..attr_pos+2].copy_from_slice(&0x0001u16.to_be_bytes()); // MAPPED-ADDRESS
    data[attr_pos+2..attr_pos+4].copy_from_slice(&8u16.to_be_bytes()); // Length
    data[attr_pos+5] = 0x01; // IPv4
    data[attr_pos+6..attr_pos+8].copy_from_slice(&12345u16.to_be_bytes()); // Port
    data[attr_pos+8..attr_pos+12].copy_from_slice(&[1, 2, 3, 4]); // IP

    let result = parse_stun_srflx(&data).expect("Failed to parse STUN");
    assert_eq!(result.port(), 12345);
    assert_eq!(result.ip().to_string(), "1.2.3.4");
}
