use anyhow::Result;
use bytes::Bytes;
use fabstir_llm_node::api::websocket::transport::*;
use std::time::Duration;

#[tokio::test]
async fn test_message_framing() -> Result<()> {
    let framer = MessageFramer::new();

    let data = b"Hello, WebSocket!";
    let frame = framer.create_frame(data)?;

    assert!(frame.len() > data.len()); // Should have headers
    assert!(frame.starts_with(&[0x81])); // Text frame with FIN bit

    Ok(())
}

#[tokio::test]
async fn test_message_parsing() -> Result<()> {
    let parser = MessageParser::new();

    // Create a valid WebSocket text frame
    let text = "Test message";
    let frame = create_text_frame(text);

    let parsed = parser.parse_frame(&frame)?;
    assert_eq!(parsed.opcode, OpCode::Text);
    assert_eq!(parsed.payload_str()?, text);

    Ok(())
}

#[tokio::test]
async fn test_binary_frame_handling() -> Result<()> {
    let transport = TransportLayer::new();

    let data = vec![0x01, 0x02, 0x03, 0x04];
    let frame = transport.encode_binary(&data)?;

    let decoded = transport.decode_frame(&frame)?;
    assert_eq!(decoded.opcode, OpCode::Binary);
    assert_eq!(decoded.payload, data);

    Ok(())
}

#[tokio::test]
async fn test_close_frame() -> Result<()> {
    let transport = TransportLayer::new();

    let close_frame = transport.create_close_frame(1000, "Normal closure")?;

    let parsed = transport.decode_frame(&close_frame)?;
    assert_eq!(parsed.opcode, OpCode::Close);

    let (code, reason) = parsed.close_data()?;
    assert_eq!(code, 1000);
    assert_eq!(reason, "Normal closure");

    Ok(())
}

#[tokio::test]
async fn test_ping_pong_frames() -> Result<()> {
    let transport = TransportLayer::new();

    let ping_data = b"ping";
    let ping_frame = transport.create_ping_frame(ping_data)?;

    let parsed = transport.decode_frame(&ping_frame)?;
    assert_eq!(parsed.opcode, OpCode::Ping);

    // Create pong response
    let pong_frame = transport.create_pong_frame(&parsed.payload)?;
    let pong_parsed = transport.decode_frame(&pong_frame)?;

    assert_eq!(pong_parsed.opcode, OpCode::Pong);
    assert_eq!(pong_parsed.payload, ping_data);

    Ok(())
}

#[tokio::test]
async fn test_fragmented_message() -> Result<()> {
    let transport = TransportLayer::new();

    let message = "This is a long message that will be fragmented";
    let fragments = transport.fragment_message(message.as_bytes(), 20)?;

    assert!(fragments.len() > 1);

    // First fragment should not have FIN bit
    assert!(!fragments[0].is_final);
    // Last fragment should have FIN bit
    assert!(fragments.last().unwrap().is_final);

    // Reassemble
    let reassembled = transport.reassemble_fragments(fragments)?;
    assert_eq!(reassembled, message.as_bytes());

    Ok(())
}

#[tokio::test]
async fn test_compression() -> Result<()> {
    let mut transport = TransportLayer::new();
    transport.enable_compression(CompressionMethod::Deflate);

    let data = "Repeated data repeated data repeated data repeated data";
    let compressed = transport.compress(data.as_bytes())?;

    assert!(compressed.len() < data.len());

    let decompressed = transport.decompress(&compressed)?;
    assert_eq!(decompressed, data.as_bytes());

    Ok(())
}

#[tokio::test]
async fn test_masking() -> Result<()> {
    let transport = TransportLayer::new();

    let data = b"Secret message";
    let mask = [0x12, 0x34, 0x56, 0x78];

    let masked = transport.apply_mask(data, &mask)?;
    assert_ne!(masked, data);

    // Applying mask again should unmask
    let unmasked = transport.apply_mask(&masked, &mask)?;
    assert_eq!(unmasked, data);

    Ok(())
}

#[tokio::test]
async fn test_frame_validation() -> Result<()> {
    let validator = FrameValidator::new();

    // Valid frame
    let valid_frame = create_text_frame("Valid");
    assert!(validator.validate(&valid_frame).is_ok());

    // Invalid frame (too short)
    let invalid_frame = vec![0x81];
    assert!(validator.validate(&invalid_frame).is_err());

    // Invalid opcode
    let mut bad_opcode = create_text_frame("Test");
    bad_opcode[0] = 0x8F; // Invalid opcode 15
    assert!(validator.validate(&bad_opcode).is_err());

    Ok(())
}

#[tokio::test]
async fn test_flow_control() -> Result<()> {
    let mut transport = TransportLayer::new();
    transport.set_max_frame_size(100);

    // Small message should pass
    let small = vec![0; 50];
    assert!(transport.check_frame_size(&small).is_ok());

    // Large message should be rejected
    let large = vec![0; 200];
    assert!(transport.check_frame_size(&large).is_err());

    Ok(())
}

#[tokio::test]
async fn test_connection_upgrade() -> Result<()> {
    let transport = TransportLayer::new();

    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    let accept = transport.calculate_accept_key(key)?;

    // Should produce correct SHA-1 hash
    assert_eq!(accept, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");

    Ok(())
}

#[tokio::test]
async fn test_error_recovery() -> Result<()> {
    let transport = TransportLayer::new();

    // Corrupted frame
    let mut frame = create_text_frame("Test");
    frame[1] = 0xFF; // Invalid payload length

    let result = transport.decode_frame(&frame);
    assert!(result.is_err());

    // Should be able to recover and process next frame
    let valid_frame = create_text_frame("Valid");
    let decoded = transport.decode_frame(&valid_frame)?;
    assert_eq!(decoded.payload_str()?, "Valid");

    Ok(())
}

// Helper functions

fn create_text_frame(text: &str) -> Vec<u8> {
    let mut frame = vec![0x81]; // FIN + Text opcode
    let len = text.len();

    if len < 126 {
        frame.push(len as u8);
    } else if len < 65536 {
        frame.push(126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }

    frame.extend_from_slice(text.as_bytes());
    frame
}
