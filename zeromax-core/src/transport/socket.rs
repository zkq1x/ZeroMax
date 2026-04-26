use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_native_tls::TlsStream;
use tracing::{debug, info, warn};

use crate::constants::{API_HOST, API_PORT, DEFAULT_PING_INTERVAL, DEFAULT_TIMEOUT, RECV_LOOP_BACKOFF};
use crate::error::{Error, Result};
use crate::protocol::Opcode;
use crate::transport::frame::Frame;
use crate::transport::pending::PendingMap;

const HEADER_SIZE: usize = 10;

/// Binary socket transport for the MAX messenger protocol (DESKTOP/ANDROID/IOS).
///
/// Wire format: `[ver:1][cmd:2][seq:1][opcode:2][len:4][payload]`
/// Payload is MessagePack encoded, optionally LZ4 compressed.
///
/// Mirrors `SocketMixin` from `pymax/mixins/socket.py`.
pub struct SocketTransport {
    host: String,
    port: u16,
    seq: Arc<AtomicU64>,
    pending: PendingMap,
    incoming_tx: broadcast::Sender<Frame>,
    connected: Arc<AtomicBool>,
    writer: Option<Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TlsStream<TcpStream>>>>>,
    recv_handle: Option<JoinHandle<()>>,
    ping_handle: Option<JoinHandle<()>>,
}

impl SocketTransport {
    pub fn new(host: &str, port: u16) -> Self {
        let (incoming_tx, _) = broadcast::channel(256);
        Self {
            host: host.to_string(),
            port,
            seq: Arc::new(AtomicU64::new(0)),
            pending: PendingMap::new(),
            incoming_tx,
            connected: Arc::new(AtomicBool::new(false)),
            writer: None,
            recv_handle: None,
            ping_handle: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Frame> {
        self.incoming_tx.subscribe()
    }

    /// Connect via TLS TCP socket.
    pub async fn connect(&mut self) -> Result<()> {
        if self.is_connected() {
            warn!("Socket already connected");
            return Ok(());
        }

        info!(host = %self.host, port = self.port, "Connecting socket");

        let tcp = TcpStream::connect((&*self.host, self.port))
            .await
            .map_err(|e| Error::UnexpectedResponse(format!("TCP connect: {e}")))?;

        let connector = native_tls::TlsConnector::builder()
            .min_protocol_version(Some(native_tls::Protocol::Tlsv12))
            .build()
            .map_err(|e| Error::UnexpectedResponse(format!("TLS builder: {e}")))?;

        let connector = tokio_native_tls::TlsConnector::from(connector);
        let tls = connector
            .connect(&self.host, tcp)
            .await
            .map_err(|e| Error::UnexpectedResponse(format!("TLS connect: {e}")))?;

        let (reader, writer) = tokio::io::split(tls);
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
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

        info!("Socket connected");
        Ok(())
    }

    /// Send a binary frame and wait for matching response.
    pub async fn send_and_wait(
        &self,
        opcode: Opcode,
        payload: serde_json::Value,
        cmd: u32,
        timeout: std::time::Duration,
    ) -> Result<Frame> {
        let writer = self.writer.as_ref().ok_or(Error::NotConnected)?;

        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;

        // Encode payload as MessagePack.
        let payload_bytes = rmp_serde::to_vec(&payload)
            .map_err(|e| Error::UnexpectedResponse(format!("MsgPack encode: {e}")))?;

        let packet = pack_packet(11, cmd as u16, seq, opcode as u16, &payload_bytes);
        let seq_key = seq % 256;
        let rx = self.pending.insert(seq_key);

        eprintln!("[SEND] opcode={} seq={} seq%256={} payload_len={}", opcode, seq, seq_key, payload_bytes.len());

        {
            let mut w = writer.lock().await;
            w.write_all(&packet)
                .await
                .map_err(|e| Error::UnexpectedResponse(format!("Socket write: {e}")))?;
            w.flush()
                .await
                .map_err(|e| Error::UnexpectedResponse(format!("Socket flush: {e}")))?;
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => {
                debug!(seq, "Received socket response");
                Ok(response)
            }
            Ok(Err(_)) => {
                self.pending.remove(seq % 256);
                Err(Error::NotConnected)
            }
            Err(_) => {
                self.pending.remove(seq % 256);
                Err(Error::Timeout(timeout))
            }
        }
    }

    pub async fn request(&self, opcode: Opcode, payload: serde_json::Value) -> Result<Frame> {
        self.send_and_wait(opcode, payload, 0, DEFAULT_TIMEOUT).await
    }

    pub async fn close(&mut self) -> Result<()> {
        self.connected.store(false, Ordering::Relaxed);
        self.pending.cancel_all();

        if let Some(h) = self.recv_handle.take() { h.abort(); }
        if let Some(h) = self.ping_handle.take() { h.abort(); }

        if let Some(writer) = self.writer.take() {
            let mut w = writer.lock().await;
            let _ = w.shutdown().await;
        }

        info!("Socket closed");
        Ok(())
    }
}

// ── Wire format helpers ────────────────────────────────────────

/// Pack a binary packet: `[ver:1][cmd:2][seq:1][opcode:2][len:4][payload]`
fn pack_packet(ver: u8, cmd: u16, seq: u64, opcode: u16, payload: &[u8]) -> Vec<u8> {
    let len = (payload.len() as u32) & 0x00FFFFFF;
    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.push(ver);
    buf.extend_from_slice(&cmd.to_be_bytes());
    buf.push((seq % 256) as u8);
    buf.extend_from_slice(&opcode.to_be_bytes());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

/// Unpack a binary packet into a Frame.
fn unpack_packet(data: &[u8]) -> Option<Frame> {
    if data.len() < HEADER_SIZE {
        return None;
    }

    let ver = data[0];
    let cmd = u16::from_be_bytes([data[1], data[2]]) as u32;
    let seq = data[3] as u64;
    let opcode = u16::from_be_bytes([data[4], data[5]]);
    let packed_len = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
    let comp_flag = packed_len >> 24;
    let payload_length = (packed_len & 0x00FFFFFF) as usize;

    let payload_bytes = &data[HEADER_SIZE..HEADER_SIZE + payload_length.min(data.len() - HEADER_SIZE)];

    if payload_bytes.is_empty() {
        return Some(Frame {
            ver,
            cmd,
            seq,
            opcode,
            payload: serde_json::Value::Null,
        });
    }

    // Decompress if LZ4.
    let decompressed;
    let final_bytes = if comp_flag != 0 {
        match lz4_flex::decompress(payload_bytes, 100_000) {
            Ok(d) => {
                decompressed = d;
                &decompressed
            }
            Err(e) => {
                eprintln!("[UNPACK] LZ4 decompression failed: {e}, comp_flag={comp_flag}, data_len={}", payload_bytes.len());
                return None;
            }
        }
    } else {
        payload_bytes
    };

    // Decode MessagePack → rmpv::Value (supports integer map keys)
    // then convert to serde_json::Value.
    let rmpv_value: rmpv::Value = match rmpv::decode::read_value(&mut &final_bytes[..]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[UNPACK] MsgPack decode failed: {e}, bytes_len={}, first_bytes={:?}",
                final_bytes.len(), &final_bytes[..final_bytes.len().min(32)]);
            return None;
        }
    };
    let value = rmpv_to_json(rmpv_value);

    Some(Frame {
        ver,
        cmd,
        seq,
        opcode,
        payload: value,
    })
}

// ── Background tasks ───────────────────────────────────────────

type TlsWriter = Arc<tokio::sync::Mutex<tokio::io::WriteHalf<TlsStream<TcpStream>>>>;

async fn recv_loop(
    mut reader: tokio::io::ReadHalf<TlsStream<TcpStream>>,
    pending: PendingMap,
    incoming_tx: broadcast::Sender<Frame>,
    connected: Arc<AtomicBool>,
) {
    debug!("Socket recv loop started");

    while connected.load(Ordering::Relaxed) {
        // Read 10-byte header.
        let mut header = [0u8; HEADER_SIZE];
        match reader.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) => {
                info!(error = %e, "Socket read error");
                break;
            }
        }

        // Parse payload length from header.
        let packed_len = u32::from_be_bytes([header[6], header[7], header[8], header[9]]);
        let payload_length = (packed_len & 0x00FFFFFF) as usize;

        // Read payload.
        let mut payload = vec![0u8; payload_length];
        if payload_length > 0 {
            if let Err(e) = reader.read_exact(&mut payload).await {
                info!(error = %e, "Socket payload read error");
                break;
            }
        }

        // Combine and unpack.
        let mut full = Vec::with_capacity(HEADER_SIZE + payload_length);
        full.extend_from_slice(&header);
        full.extend_from_slice(&payload);

        // MsgPack list payloads may contain multiple items.
        let frames = match unpack_packet(&full) {
            Some(frame) => {
                // Check if payload is a list (multiple messages in one packet).
                if let serde_json::Value::Array(items) = &frame.payload {
                    items
                        .iter()
                        .map(|item| Frame {
                            ver: frame.ver,
                            cmd: frame.cmd,
                            seq: frame.seq,
                            opcode: frame.opcode,
                            payload: item.clone(),
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![frame]
                }
            }
            None => {
                eprintln!("[RECV] FAILED to unpack packet! header={:?} payload_len={}", &header, payload_length);
                continue;
            }
        };

        for frame in frames {
            eprintln!("[RECV] opcode={} seq={} seq%256={} pending_count={}",
                frame.opcode, frame.seq, frame.seq % 256, pending.len());
            // seq matching uses seq % 256 for socket.
            if pending.resolve(frame.seq % 256, frame.clone()) {
                eprintln!("[RECV] -> resolved pending for seq%256={}", frame.seq % 256);
                continue;
            }
            eprintln!("[RECV] -> no pending match, broadcasting");
            let _ = incoming_tx.send(frame);
        }
    }

    connected.store(false, Ordering::Relaxed);
    pending.cancel_all();
    debug!("Socket recv loop exited");
}

async fn ping_loop(
    writer: TlsWriter,
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
        let payload = rmp_serde::to_vec(&serde_json::json!({"interactive": true})).unwrap_or_default();
        let packet = pack_packet(11, 0, s, Opcode::Ping as u16, &payload);
        let rx = pending.insert(s % 256);

        {
            let mut w = writer.lock().await;
            if let Err(e) = w.write_all(&packet).await {
                warn!(error = %e, "Socket ping write failed");
                break;
            }
            let _ = w.flush().await;
        }

        match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(_)) => debug!("Socket ping acknowledged"),
            Ok(Err(_)) => { warn!("Socket ping channel closed"); break; }
            Err(_) => { warn!("Socket ping timed out"); }
        }
    }
    debug!("Socket ping loop exited");
}

/// Convert rmpv::Value → serde_json::Value, stringifying integer map keys.
fn rmpv_to_json(v: rmpv::Value) -> serde_json::Value {
    match v {
        rmpv::Value::Nil => serde_json::Value::Null,
        rmpv::Value::Boolean(b) => serde_json::Value::Bool(b),
        rmpv::Value::Integer(i) => {
            if let Some(n) = i.as_i64() {
                serde_json::Value::Number(n.into())
            } else if let Some(n) = i.as_u64() {
                serde_json::Value::Number(n.into())
            } else {
                serde_json::Value::Null
            }
        }
        rmpv::Value::F32(f) => serde_json::json!(f),
        rmpv::Value::F64(f) => serde_json::json!(f),
        rmpv::Value::String(s) => {
            serde_json::Value::String(s.into_str().unwrap_or_default().to_string())
        }
        rmpv::Value::Binary(b) => {
            // Encode binary as base64 string.
            use std::fmt::Write;
            let mut hex = String::with_capacity(b.len() * 2);
            for byte in &b {
                let _ = write!(hex, "{byte:02x}");
            }
            serde_json::Value::String(hex)
        }
        rmpv::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(rmpv_to_json).collect())
        }
        rmpv::Value::Map(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs {
                // Convert any key type to string.
                let key = match k {
                    rmpv::Value::String(s) => s.into_str().unwrap_or_default().to_string(),
                    rmpv::Value::Integer(i) => i.to_string(),
                    other => format!("{other}"),
                };
                map.insert(key, rmpv_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        rmpv::Value::Ext(_, data) => {
            serde_json::Value::String(format!("<ext:{} bytes>", data.len()))
        }
    }
}
