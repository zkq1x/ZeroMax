use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::transport::frame::Frame;

/// Thread-safe map of pending request/response pairs.
///
/// Each outgoing request stores a `seq → oneshot::Sender<Frame>`.
/// When the recv loop gets a response with a matching `seq`, it resolves the sender.
///
/// Mirrors the `_pending` dict from `pymax/mixins/websocket.py`.
#[derive(Debug, Clone)]
pub struct PendingMap {
    inner: Arc<DashMap<u64, oneshot::Sender<Frame>>>,
}

impl PendingMap {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Insert a new pending request. Returns the receiver to await.
    pub fn insert(&self, seq: u64) -> oneshot::Receiver<Frame> {
        // Cancel any existing pending request with the same seq.
        if let Some((_, old_tx)) = self.inner.remove(&seq) {
            drop(old_tx); // receiver will get RecvError
        }
        let (tx, rx) = oneshot::channel();
        self.inner.insert(seq, tx);
        rx
    }

    /// Try to resolve a pending request. Returns `true` if matched.
    pub fn resolve(&self, seq: u64, frame: Frame) -> bool {
        if let Some((_, tx)) = self.inner.remove(&seq) {
            let _ = tx.send(frame);
            true
        } else {
            false
        }
    }

    /// Remove a pending request without resolving it.
    pub fn remove(&self, seq: u64) {
        self.inner.remove(&seq);
    }

    /// Cancel all pending requests (e.g. on disconnect).
    pub fn cancel_all(&self) {
        self.inner.clear();
    }

    /// Number of currently pending requests.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Default for PendingMap {
    fn default() -> Self {
        Self::new()
    }
}
