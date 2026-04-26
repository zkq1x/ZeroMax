use tokio::sync::broadcast;

use crate::constants::{API_HOST, API_PORT, WEBSOCKET_URI};
use crate::error::Result;
use crate::protocol::Opcode;
use crate::transport::frame::Frame;
use crate::transport::socket::SocketTransport;
use crate::transport::websocket::WsTransport;

/// Unified transport that wraps either WebSocket or Socket.
pub enum Transport {
    WebSocket(WsTransport),
    Socket(SocketTransport),
}

impl Transport {
    pub fn new_websocket(uri: &str) -> Self {
        Self::WebSocket(WsTransport::new(uri))
    }

    pub fn new_socket(host: &str, port: u16) -> Self {
        Self::Socket(SocketTransport::new(host, port))
    }

    pub fn is_connected(&self) -> bool {
        match self {
            Self::WebSocket(ws) => ws.is_connected(),
            Self::Socket(s) => s.is_connected(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        match self {
            Self::WebSocket(ws) => ws.subscribe(),
            Self::Socket(s) => s.subscribe(),
        }
    }

    pub async fn connect_ws(&mut self, user_agent_header: &str) -> Result<()> {
        match self {
            Self::WebSocket(ws) => ws.connect(user_agent_header).await,
            Self::Socket(_) => Err(crate::error::Error::UnexpectedResponse(
                "Cannot WS connect on socket transport".into(),
            )),
        }
    }

    pub async fn connect_socket(&mut self) -> Result<()> {
        match self {
            Self::Socket(s) => s.connect().await,
            Self::WebSocket(_) => Err(crate::error::Error::UnexpectedResponse(
                "Cannot socket connect on WS transport".into(),
            )),
        }
    }

    pub async fn request(&self, opcode: Opcode, payload: serde_json::Value) -> Result<Frame> {
        match self {
            Self::WebSocket(ws) => ws.request(opcode, payload).await,
            Self::Socket(s) => s.request(opcode, payload).await,
        }
    }

    pub async fn send_and_wait(
        &self,
        opcode: Opcode,
        payload: serde_json::Value,
        cmd: u32,
        timeout: std::time::Duration,
    ) -> Result<Frame> {
        match self {
            Self::WebSocket(ws) => ws.send_and_wait(opcode, payload, cmd, timeout).await,
            Self::Socket(s) => s.send_and_wait(opcode, payload, cmd, timeout).await,
        }
    }

    pub async fn close(&mut self) -> Result<()> {
        match self {
            Self::WebSocket(ws) => ws.close().await,
            Self::Socket(s) => s.close().await,
        }
    }
}
