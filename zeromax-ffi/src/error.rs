use std::fmt;

/// FFI-compatible error type exposed to Swift/Kotlin via UniFFI.
///
/// UDL `[Error] enum` requires flat variants — message goes in Display impl.
#[derive(Debug)]
pub enum FfiError {
    Auth,
    NotConnected,
    Timeout,
    InvalidPhone,
    Server,
    Network,
    Internal,
}

// UniFFI requires Display for error types.
impl fmt::Display for FfiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FfiError::Auth => write!(f, "Authentication failed"),
            FfiError::NotConnected => write!(f, "Not connected"),
            FfiError::Timeout => write!(f, "Request timed out"),
            FfiError::InvalidPhone => write!(f, "Invalid phone number"),
            FfiError::Server => write!(f, "Server error"),
            FfiError::Network => write!(f, "Network error"),
            FfiError::Internal => write!(f, "Internal error"),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<zeromax_core::Error> for FfiError {
    fn from(e: zeromax_core::Error) -> Self {
        // Log the full error to stderr so it's visible in the terminal.
        eprintln!("[ZeroMax FFI ERROR] {e}");

        match e {
            zeromax_core::Error::Auth(_) => FfiError::Auth,
            zeromax_core::Error::NotConnected => FfiError::NotConnected,
            zeromax_core::Error::Timeout(_) => FfiError::Timeout,
            zeromax_core::Error::InvalidPhone(_) => FfiError::InvalidPhone,
            zeromax_core::Error::Server { .. } | zeromax_core::Error::RateLimited { .. } => {
                FfiError::Server
            }
            zeromax_core::Error::WebSocket(_) | zeromax_core::Error::Http(_) => FfiError::Network,
            _ => FfiError::Internal,
        }
    }
}
