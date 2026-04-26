use serde::{Deserialize, Serialize};

/// Chat folder.
///
/// Mirrors `Folder` from `pymax/types.py:1086-1115`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source_id: i64,
    #[serde(default)]
    pub include: Vec<i64>,
    #[serde(default)]
    pub options: Vec<serde_json::Value>,
    #[serde(default)]
    pub update_time: i64,
    #[serde(default)]
    pub filters: Vec<serde_json::Value>,
}

/// Response from folder list request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderList {
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub folder_sync: i64,
}

/// Response from folder create/update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderUpdate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
}
