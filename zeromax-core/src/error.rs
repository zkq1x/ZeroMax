use std::time::Duration;

/// All errors that can occur in the ZeroMax library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Server returned an error payload.
    #[error("[{code}] {message}")]
    Server {
        code: String,
        message: String,
        title: String,
        localized_message: Option<String>,
    },

    /// Server rate-limited the request.
    #[error("Rate limited: {message}")]
    RateLimited {
        code: String,
        message: String,
        title: String,
        localized_message: Option<String>,
    },

    /// Authentication failed.
    #[error("Authentication failed: {0}")]
    Auth(String),

    /// Not connected to the server.
    #[error("Not connected")]
    NotConnected,

    /// A request timed out.
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),

    /// Invalid phone number format.
    #[error("Invalid phone number: {0}")]
    InvalidPhone(String),

    /// WebSocket transport error.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] Box<tokio_tungstenite::tungstenite::Error>),

    /// SQLite storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] sqlx::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// HTTP error (file uploads).
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Unexpected response structure.
    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),
}

/// Convenience type alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Check a server response payload for error fields and return the appropriate error.
///
/// Mirrors `MixinsUtils.handle_error` from `pymax/utils.py`.
pub fn check_payload(payload: &serde_json::Value) -> Result<()> {
    let error_code = match payload.get("error").and_then(|v| v.as_str()) {
        Some(code) => code,
        None => return Ok(()),
    };

    let message = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = payload
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let localized_message = payload
        .get("localizedMessage")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    tracing::error!(
        code = error_code,
        message = %message,
        title = %title,
        localized = ?localized_message,
        "Server returned error"
    );

    if error_code == "too.many.requests" {
        return Err(Error::RateLimited {
            code: error_code.to_string(),
            message,
            title,
            localized_message,
        });
    }

    Err(Error::Server {
        code: error_code.to_string(),
        message,
        title,
        localized_message,
    })
}
