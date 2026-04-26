use std::path::PathBuf;

use anyhow::Context;
use tokio::runtime::Handle;
use zeromax_core::{ClientConfig, MaxClient};

/// Owns the tokio multi-thread runtime that all `zeromax-core` async work runs on.
///
/// Lives for the duration of the process. UI tasks are pushed back to the Slint
/// event loop via `slint::Weak::upgrade_in_event_loop`; tokio tasks dispatch
/// network/protocol work via `Handle::spawn`.
pub struct Runtime {
    rt: tokio::runtime::Runtime,
}

impl Runtime {
    pub fn new() -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("zeromax-net")
            .build()
            .context("Failed to build tokio runtime")?;
        Ok(Self { rt })
    }

    pub fn handle(&self) -> Handle {
        self.rt.handle().clone()
    }
}

/// OS-appropriate user-data directory for ZeroMax (session DB, logs, cache).
///
/// macOS: `~/Library/Application Support/ZeroMax/`
/// Linux: `~/.local/share/ZeroMax/`
/// Windows: `%APPDATA%\ZeroMax\data\`
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("ru", "ZeroMax", "ZeroMax")
        .context("Cannot resolve project directories")?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir).context("Failed to create data dir")?;
    Ok(dir)
}

/// Smoke test: open WebSocket to MAX, perform handshake, disconnect.
///
/// Uses a placeholder phone — only the handshake is exercised, not auth.
/// Proves: tokio runtime ↔ zeromax-core ↔ network ↔ result-back-to-UI works.
pub async fn test_connect() -> anyhow::Result<String> {
    let work_dir = data_dir()?;
    let config = ClientConfig::new("+79991234567")
        .device_type("WEB")
        .work_dir(work_dir);

    let mut client = MaxClient::new(config)
        .await
        .context("MaxClient::new failed")?;
    client.connect().await.context("connect failed")?;
    let _ = client.close().await;

    Ok("Handshake OK — WS reachable, no auth performed.".to_string())
}
