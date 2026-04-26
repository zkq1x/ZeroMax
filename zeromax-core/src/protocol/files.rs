use std::sync::Arc;

use serde_json::json;
use tokio::sync::{broadcast, oneshot};
use tracing::{debug, info, warn};

use crate::constants::DEFAULT_TIMEOUT;
use crate::error::{self, Error, Result};
use crate::protocol::Opcode;
use crate::transport::Frame;

/// Result of a successful upload — used to build message attachments.
#[derive(Debug, Clone)]
pub enum UploadResult {
    Photo { photo_token: String },
    Video { video_id: i64, token: String },
    File { file_id: i64 },
}

impl UploadResult {
    /// Convert to the JSON attachment payload for `send_message`.
    pub fn to_attach_json(&self) -> serde_json::Value {
        match self {
            UploadResult::Photo { photo_token } => json!({
                "_type": "PHOTO",
                "photoToken": photo_token,
            }),
            UploadResult::Video { video_id, token } => json!({
                "_type": "VIDEO",
                "videoId": video_id,
                "token": token,
            }),
            UploadResult::File { file_id } => json!({
                "_type": "FILE",
                "fileId": file_id,
            }),
        }
    }
}

/// Waiter map for file upload confirmations (NOTIF_ATTACH).
///
/// Mirrors `_file_upload_waiters` from `pymax/core.py`.
#[derive(Debug, Clone, Default)]
pub struct UploadWaiters {
    inner: Arc<dashmap::DashMap<i64, oneshot::Sender<Frame>>>,
}

impl UploadWaiters {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a waiter for a file/video ID. Returns the receiver.
    pub fn wait_for(&self, id: i64) -> oneshot::Receiver<Frame> {
        let (tx, rx) = oneshot::channel();
        self.inner.insert(id, tx);
        rx
    }

    /// Try to fulfill a waiter from a NOTIF_ATTACH frame.
    /// Returns `true` if a waiter was resolved.
    pub fn try_fulfill(&self, frame: &Frame) -> bool {
        if frame.opcode != Opcode::NotifAttach as u16 {
            return false;
        }

        let mut fulfilled = false;
        for key in ["fileId", "videoId"] {
            if let Some(id) = frame.payload.get(key).and_then(|v| v.as_i64()) {
                if let Some((_, tx)) = self.inner.remove(&id) {
                    let _ = tx.send(frame.clone());
                    debug!(key, id, "Fulfilled upload waiter");
                    fulfilled = true;
                }
            }
        }
        fulfilled
    }

    /// Cancel a waiter (e.g. on timeout).
    pub fn cancel(&self, id: i64) {
        self.inner.remove(&id);
    }
}

/// Spawn a background task that listens for NOTIF_ATTACH and fulfills waiters.
pub fn spawn_upload_watcher(
    mut rx: broadcast::Receiver<Frame>,
    waiters: UploadWaiters,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(frame) => {
                    waiters.try_fulfill(&frame);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Upload watcher lagged");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

use crate::client::MaxClient;

impl MaxClient {
    /// Upload a photo and return the token for attaching to a message.
    ///
    /// Mirrors `MessageMixin._upload_photo()` from `pymax/mixins/message.py`.
    pub async fn upload_photo(&self, data: Vec<u8>, filename: &str) -> Result<UploadResult> {
        info!(filename, "Uploading photo");

        // 1. Request upload URL.
        let response = self
            .transport
            .request(Opcode::PhotoUpload, json!({"count": 1, "profile": false}))
            .await?;
        error::check_payload(&response.payload)?;

        let url = response
            .payload
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::UnexpectedResponse("No upload URL for photo".into()))?;

        // 2. Upload via HTTP multipart.
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str("image/jpeg")
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

        let form = reqwest::multipart::Form::new().part("file", part);

        let http_resp = reqwest::Client::new()
            .post(url)
            .multipart(form)
            .send()
            .await?;

        if !http_resp.status().is_success() {
            return Err(Error::UnexpectedResponse(format!(
                "Photo upload HTTP {}",
                http_resp.status()
            )));
        }

        let body: serde_json::Value = http_resp.json().await?;

        // 3. Extract photo token from response.
        let token = body
            .get("photos")
            .and_then(|v| v.as_object())
            .and_then(|m| m.values().next())
            .and_then(|v| v.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::UnexpectedResponse("No photo token in upload response".into()))?;

        info!("Photo uploaded successfully");
        Ok(UploadResult::Photo { photo_token: token })
    }

    /// Upload a file and return the file ID for attaching to a message.
    ///
    /// Mirrors `MessageMixin._upload_file()` from `pymax/mixins/message.py`.
    pub async fn upload_file(
        &self,
        data: Vec<u8>,
        filename: &str,
        waiters: &UploadWaiters,
    ) -> Result<UploadResult> {
        info!(filename, size = data.len(), "Uploading file");

        // 1. Request upload URL.
        let response = self
            .transport
            .request(Opcode::FileUpload, json!({"count": 1, "profile": false}))
            .await?;
        error::check_payload(&response.payload)?;

        let info = response
            .payload
            .get("info")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| Error::UnexpectedResponse("No upload info for file".into()))?;

        let url = info
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::UnexpectedResponse("No upload URL".into()))?;
        let file_id = info
            .get("fileId")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| Error::UnexpectedResponse("No file ID".into()))?;

        // 2. Register waiter for NOTIF_ATTACH.
        let rx = waiters.wait_for(file_id);

        // 3. Upload via HTTP PUT with Content-Range.
        let file_size = data.len();
        let http_resp = reqwest::Client::new()
            .post(url)
            .header("Content-Disposition", format!("attachment; filename={filename}"))
            .header("Content-Length", file_size.to_string())
            .header(
                "Content-Range",
                format!("0-{}/{}", file_size.saturating_sub(1), file_size),
            )
            .body(data)
            .send()
            .await?;

        if !http_resp.status().is_success() {
            waiters.cancel(file_id);
            return Err(Error::UnexpectedResponse(format!(
                "File upload HTTP {}",
                http_resp.status()
            )));
        }

        // 4. Wait for server confirmation (NOTIF_ATTACH).
        match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(_)) => {
                info!(file_id, "File upload confirmed");
                Ok(UploadResult::File { file_id })
            }
            _ => {
                waiters.cancel(file_id);
                Err(Error::Timeout(DEFAULT_TIMEOUT))
            }
        }
    }

    /// Upload a video and return the video ID + token.
    ///
    /// Mirrors `MessageMixin._upload_video()` from `pymax/mixins/message.py`.
    pub async fn upload_video(
        &self,
        data: Vec<u8>,
        filename: &str,
        waiters: &UploadWaiters,
    ) -> Result<UploadResult> {
        info!(filename, size = data.len(), "Uploading video");

        let response = self
            .transport
            .request(Opcode::VideoUpload, json!({"count": 1, "profile": false}))
            .await?;
        error::check_payload(&response.payload)?;

        let info = response
            .payload
            .get("info")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| Error::UnexpectedResponse("No upload info for video".into()))?;

        let url = info.get("url").and_then(|v| v.as_str())
            .ok_or_else(|| Error::UnexpectedResponse("No upload URL".into()))?;
        let video_id = info.get("videoId").and_then(|v| v.as_i64())
            .ok_or_else(|| Error::UnexpectedResponse("No video ID".into()))?;
        let token = info.get("token").and_then(|v| v.as_str())
            .ok_or_else(|| Error::UnexpectedResponse("No video token".into()))?
            .to_string();

        let rx = waiters.wait_for(video_id);

        let file_size = data.len();
        let http_resp = reqwest::Client::new()
            .post(url)
            .header("Content-Disposition", format!("attachment; filename={filename}"))
            .header("Content-Length", file_size.to_string())
            .header("Content-Range", format!("0-{}/{}", file_size.saturating_sub(1), file_size))
            .header("Connection", "keep-alive")
            .body(data)
            .send()
            .await?;

        if !http_resp.status().is_success() {
            waiters.cancel(video_id);
            return Err(Error::UnexpectedResponse(format!(
                "Video upload HTTP {}",
                http_resp.status()
            )));
        }

        match tokio::time::timeout(DEFAULT_TIMEOUT, rx).await {
            Ok(Ok(_)) => {
                info!(video_id, "Video upload confirmed");
                Ok(UploadResult::Video { video_id, token })
            }
            _ => {
                waiters.cancel(video_id);
                Err(Error::Timeout(DEFAULT_TIMEOUT))
            }
        }
    }

    /// Send a message with attachments (convenience wrapper).
    pub async fn send_message_with_attachments(
        &self,
        chat_id: i64,
        text: &str,
        attachments: &[UploadResult],
        reply_to: Option<i64>,
        notify: bool,
    ) -> Result<crate::types::Message> {
        let cid = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let attaches: Vec<serde_json::Value> = attachments.iter().map(|a| a.to_attach_json()).collect();

        let mut message = json!({
            "cid": cid,
            "text": text,
            "elements": [],
            "attaches": attaches,
        });

        if let Some(reply_id) = reply_to {
            message["link"] = json!({
                "type": "REPLY",
                "messageId": reply_id.to_string(),
            });
        }

        let payload = json!({
            "chatId": chat_id,
            "message": message,
            "notify": notify,
        });

        let response = self.transport.request(Opcode::MsgSend, payload).await?;
        error::check_payload(&response.payload)?;

        crate::types::Message::from_payload(&response.payload)
            .ok_or_else(|| Error::UnexpectedResponse("Missing message in response".into()))
    }
}
