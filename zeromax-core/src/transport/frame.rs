use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single wire-level frame exchanged over WebSocket.
///
/// Format: `{ "ver": 11, "cmd": 0, "seq": N, "opcode": OP, "payload": {...} }`
///
/// Mirrors `BaseWebSocketMessage` from `pymax/payloads.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    /// Protocol version (10 or 11).
    pub ver: u8,

    /// Command ID (0 for most operations).
    pub cmd: u32,

    /// Monotonically increasing sequence number for request/response matching.
    pub seq: u64,

    /// Operation code — see [`crate::protocol::Opcode`].
    pub opcode: u16,

    /// Operation-specific payload.
    pub payload: Value,
}

impl Frame {
    /// Create a new outgoing frame.
    pub fn new(seq: u64, opcode: u16, payload: Value, cmd: u32) -> Self {
        Self {
            ver: crate::constants::PROTOCOL_VERSION,
            cmd,
            seq,
            opcode,
            payload,
        }
    }

    /// Try to extract the `"error"` field from the payload.
    pub fn error_code(&self) -> Option<&str> {
        self.payload.get("error").and_then(|v| v.as_str())
    }
}
