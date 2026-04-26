use serde::{Deserialize, Serialize};

/// Active session information.
///
/// Mirrors `Session` from `pymax/types.py:1047-1083`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub time: i64,
    #[serde(default)]
    pub current: bool,
}
