use std::path::PathBuf;

use anyhow::Context;
use tokio::runtime::Handle;

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
/// macOS: `~/Library/Application Support/ru.ZeroMax.ZeroMax/`
/// Linux: `~/.local/share/ZeroMax/`
/// Windows: `%APPDATA%\ZeroMax\ZeroMax\data\`
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("ru", "ZeroMax", "ZeroMax")
        .context("Cannot resolve project directories")?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir).context("Failed to create data dir")?;
    Ok(dir)
}
