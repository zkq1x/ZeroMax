use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::constants::{
    CIRCUIT_BREAKER_COOLDOWN, CIRCUIT_BREAKER_THRESHOLD, DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT,
};
use crate::protocol::Opcode;

/// A queued outgoing message.
struct QueuedMessage {
    opcode: Opcode,
    payload: Value,
    cmd: u32,
    timeout: Duration,
    retry_count: u32,
    max_retries: u32,
}

/// Circuit breaker state.
///
/// Mirrors `_circuit_breaker` / `_error_count` / `_last_error_time`
/// from `pymax/interfaces.py:418-482`.
pub struct CircuitBreaker {
    error_count: AtomicU32,
    tripped: AtomicBool,
    last_error: std::sync::Mutex<Instant>,
    threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            error_count: AtomicU32::new(0),
            tripped: AtomicBool::new(false),
            last_error: std::sync::Mutex::new(Instant::now()),
            threshold: CIRCUIT_BREAKER_THRESHOLD,
            cooldown: CIRCUIT_BREAKER_COOLDOWN,
        }
    }

    /// Record a successful operation — decrease error count.
    pub fn record_success(&self) {
        let prev = self.error_count.load(Ordering::Relaxed);
        if prev > 0 {
            self.error_count.store(prev.saturating_sub(1), Ordering::Relaxed);
        }
    }

    /// Record a failure — increment error count, trip if threshold reached.
    pub fn record_failure(&self) {
        let count = self.error_count.fetch_add(1, Ordering::Relaxed) + 1;
        *self.last_error.lock().unwrap() = Instant::now();
        if count >= self.threshold {
            self.tripped.store(true, Ordering::Relaxed);
            warn!(count, "Circuit breaker tripped");
        }
    }

    /// Check if the breaker is tripped. If cooldown has passed, reset.
    pub fn is_tripped(&self) -> bool {
        if !self.tripped.load(Ordering::Relaxed) {
            return false;
        }
        let elapsed = self.last_error.lock().unwrap().elapsed();
        if elapsed >= self.cooldown {
            self.tripped.store(false, Ordering::Relaxed);
            self.error_count.store(0, Ordering::Relaxed);
            info!("Circuit breaker reset after cooldown");
            return false;
        }
        true
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for sending messages through the outgoing queue.
#[derive(Clone)]
pub struct QueueSender {
    tx: mpsc::Sender<QueuedMessage>,
}

impl QueueSender {
    /// Queue a message for sending with retry support.
    pub async fn send(
        &self,
        opcode: Opcode,
        payload: Value,
    ) -> Result<(), mpsc::error::SendError<()>> {
        self.tx
            .send(QueuedMessage {
                opcode,
                payload,
                cmd: 0,
                timeout: DEFAULT_TIMEOUT,
                retry_count: 0,
                max_retries: DEFAULT_MAX_RETRIES,
            })
            .await
            .map_err(|_| mpsc::error::SendError(()))
    }
}

/// Type-erased async sender function used by the outgoing loop.
pub type SendFn = Arc<
    dyn Fn(Opcode, Value, u32, Duration) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::error::Result<crate::transport::Frame>> + Send>>
        + Send
        + Sync,
>;

/// Spawn the outgoing queue consumer.
///
/// `send_fn` is a closure that calls transport.send_and_wait().
/// This avoids sharing the transport directly across tasks.
///
/// Mirrors `BaseTransport._outgoing_loop()` from `pymax/interfaces.py:418-482`.
pub fn spawn_outgoing_loop(
    send_fn: SendFn,
    breaker: Arc<CircuitBreaker>,
) -> (QueueSender, tokio::task::JoinHandle<()>) {
    let (tx, mut rx) = mpsc::channel::<QueuedMessage>(256);
    let sender = QueueSender { tx: tx.clone() };
    let tx_retry = tx.clone();

    let handle = tokio::spawn(async move {
        debug!("Outgoing queue loop started");

        while let Some(mut msg) = rx.recv().await {
            if breaker.is_tripped() {
                debug!("Circuit breaker active, delaying");
                tokio::time::sleep(Duration::from_secs(5)).await;
                let _ = tx_retry.send(msg).await;
                continue;
            }

            match send_fn(msg.opcode, msg.payload.clone(), msg.cmd, msg.timeout).await {
                Ok(_) => {
                    breaker.record_success();
                    debug!(opcode = ?msg.opcode, "Queue message sent");
                }
                Err(e) => {
                    breaker.record_failure();
                    msg.retry_count += 1;

                    if msg.retry_count <= msg.max_retries {
                        let delay = retry_delay(&e, msg.retry_count);
                        warn!(
                            opcode = ?msg.opcode,
                            retry = msg.retry_count,
                            delay_secs = delay.as_secs(),
                            error = %e,
                            "Queue send failed, retrying"
                        );
                        tokio::time::sleep(delay).await;
                        let _ = tx_retry.send(msg).await;
                    } else {
                        warn!(
                            opcode = ?msg.opcode,
                            retries = msg.max_retries,
                            "Queue message dropped after max retries"
                        );
                    }
                }
            }
        }

        debug!("Outgoing queue loop exited");
    });

    (sender, handle)
}

/// Compute retry delay based on error type and attempt number.
///
/// Mirrors `BaseTransport._get_retry_delay()` from `pymax/interfaces.py:484-492`.
fn retry_delay(error: &crate::error::Error, attempt: u32) -> Duration {
    match error {
        crate::error::Error::NotConnected => Duration::from_secs(2),
        crate::error::Error::Timeout(_) => Duration::from_secs(5),
        _ => Duration::from_secs(2u64.pow(attempt.min(5))),
    }
}
