use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

use crate::constants::{DEFAULT_PING_INTERVAL, DEFAULT_TIMEOUT, WEBSOCKET_ORIGIN};
use crate::error::{Error, Result};
use crate::protocol::Opcode;
use crate::transport::frame::Frame;
use crate::transport::pending::PendingMap;

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsWriter = Arc<tokio::sync::Mutex<SplitSink<WsStream, WsMessage>>>;

/// WebSocket transport for the MAX messenger protocol.
///
/// Mirrors the connection/send/recv logic from `pymax/mixins/websocket.py`.
pub struct WsTransport {
    uri: String,
    seq: Arc<AtomicU64>,
    pending: PendingMap,
    incoming_tx: broadcast::Sender<Frame>,
    connected: Arc<AtomicBool>,
    writer: Option<WsWriter>,
    recv_handle: Option<JoinHandle<()>>,
    ping_handle: Option<JoinHandle<()>>,
}

impl WsTransport {
    /// Create a new transport (not yet connected).
    pub fn new(uri: &str) -> Self {
        let (incoming_tx, _) = broadcast::channel(256);
        Self {
            uri: uri.to_string(),
            seq: Arc::new(AtomicU64::new(0)),
            pending: PendingMap::new(),
            incoming_tx,
            connected: Arc::new(AtomicBool::new(false)),
            writer: None,
            recv_handle: None,
            ping_handle: None,
        }
    }

    /// Whether the transport is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Subscribe to incoming frames (notifications, events).
    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.incoming_tx.subscribe()
    }

    /// Establish the WebSocket connection.
    ///
    /// Mirrors `WebSocketMixin.connect()` from `pymax/mixins/websocket.py`.
    pub async fn connect(&mut self, user_agent_header: &str) -> Result<()> {
        if self.is_connected() {
            warn!("WebSocket already connected");
            return Ok(());
        }

        info!(uri = %self.uri, "Connecting to WebSocket");

        let mut request = self
            .uri
            .as_str()
            .into_client_request()
            .map_err(|e| Error::WebSocket(Box::new(e)))?;

        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_static(WEBSOCKET_ORIGIN));
        if let Ok(val) = HeaderValue::from_str(user_agent_header) {
            request.headers_mut().insert("User-Agent", val);
        }

        let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| Error::WebSocket(Box::new(e)))?;
        let (writer, reader) = ws_stream.split();

        let writer: WsWriter = Arc::new(tokio::sync::Mutex::new(writer));
        self.writer = Some(writer.clone());
        self.connected.store(true, Ordering::Relaxed);

        // Spawn recv loop.
        self.recv_handle = Some(tokio::spawn(recv_loop(
            reader,
            self.pending.clone(),
            self.incoming_tx.clone(),
            self.connected.clone(),
        )));

        // Spawn ping loop.
        self.ping_handle = Some(tokio::spawn(ping_loop(
            writer,
            self.connected.clone(),
            self.pending.clone(),
            self.seq.clone(),
        )));

        info!("WebSocket connected");
        Ok(())
    }

    /// Send a frame and wait for the matching response.
    ///
    /// Mirrors `WebSocketMixin._send_and_wait()` from `pymax/mixins/websocket.py`.
    pub async fn send_and_wait(
        &self,
        opcode: Opcode,
        payload: serde_json::Value,
        cmd: u32,
        timeout: std::time::Duration,
    ) -> Result<Frame> {
        let writer = self.writer.as_ref().ok_or(Error::NotConnected)?;

        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = Frame::new(seq, opcode as u16, payload, cmd);
        let rx = self.pending.insert(seq);

        let json = serde_json::to_string(&frame)?;
        debug!(opcode = %opcode, seq, "Sending frame");

        {
            let mut w = writer.lock().await;
            w.send(WsMessage::Text(json))
                .await
                .map_err(|e| Error::WebSocket(Box::new(e)))?;
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => {
                debug!(seq, "Received response");
                Ok(response)
            }
            Ok(Err(_)) => {
                self.pending.remove(seq);
                Err(Error::NotConnected)
            }
            Err(_) => {
                self.pending.remove(seq);
                Err(Error::Timeout(timeout))
            }
        }
    }

    /// Send a frame and wait with the default timeout.
    pub async fn request(&self, opcode: Opcode, payload: serde_json::Value) -> Result<Frame> {
        self.send_and_wait(opcode, payload, 0, DEFAULT_TIMEOUT).await
    }

    /// Close the connection and clean up background tasks.
    pub async fn close(&mut self) -> Result<()> {
        self.connected.store(false, Ordering::Relaxed);
        self.pending.cancel_all();

        if let Some(handle) = self.recv_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.ping_handle.take() {
            handle.abort();
        }

        if let Some(writer) = self.writer.take() {
            let mut w = writer.lock().await;
            let _ = w.send(WsMessage::Close(None)).await;
        }

        info!("WebSocket closed");
        Ok(())
    }
}

/// Background task: read frames from WebSocket and dispatch.
async fn recv_loop(
    mut reader: SplitStream<WsStream>,
    pending: PendingMap,
    incoming_tx: broadcast::Sender<Frame>,
    connected: Arc<AtomicBool>,
) {
    debug!("Recv loop started");

    while connected.load(Ordering::Relaxed) {
        let msg = match reader.next().await {
            Some(Ok(msg)) => msg,
            Some(Err(e)) => {
                info!(error = %e, "WebSocket connection closed");
                break;
            }
            None => {
                info!("WebSocket stream ended");
                break;
            }
        };

        let text = match msg {
            WsMessage::Text(t) => t,
            WsMessage::Close(_) => {
                info!("Received close frame");
                break;
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            _ => continue,
        };

        let frame: Frame = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                warn!(error = %e, "Failed to parse frame");
                continue;
            }
        };

        if pending.resolve(frame.seq, frame.clone()) {
            continue;
        }

        let _ = incoming_tx.send(frame);
    }

    connected.store(false, Ordering::Relaxed);
    pending.cancel_all();
    debug!("Recv loop exited");
}

/// Background task: send keepalive pings every 30 seconds.
async fn ping_loop(
    writer: WsWriter,
    connected: Arc<AtomicBool>,
    pending: PendingMap,
    seq: Arc<AtomicU64>,
) {
    loop {
        tokio::time::sleep(DEFAULT_PING_INTERVAL).await;

        if !connected.load(Ordering::Relaxed) {
            break;
        }

        let s = seq.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = Frame::new(
            s,
            Opcode::Ping as u16,
            serde_json::json!({"interactive": true}),
            0,
        );

        let rx = pending.insert(s);

        let json = match serde_json::to_string(&frame) {
            Ok(j) => j,
            Err(e) => {
                warn!(error = %e, "Failed to serialize ping");
                continue;
            }
        };

        {
            let mut w = writer.lock().await;
            if let Err(e) = w.send(WsMessage::Text(json)).await {
                warn!(error = %e, "Failed to send ping");
                break;
            }
        }

        match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(_)) => debug!("Ping acknowledged"),
            Ok(Err(_)) => {
                warn!("Ping channel closed");
                break;
            }
            Err(_) => {
                warn!("Ping timed out");
            }
        }
    }

    debug!("Ping loop exited");
}
