mod client;
mod error;
mod events;
mod types;

pub use client::ZeroMaxClient;
pub use error::FfiError;
pub use events::EventListener;
pub use types::*;

uniffi::include_scaffolding!("zeromax_ffi");
