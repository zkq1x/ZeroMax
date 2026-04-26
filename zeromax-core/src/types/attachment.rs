use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attachment types, dispatched by `_type` tag in the wire format.
///
/// Mirrors the manual `a["_type"]` dispatch in `pymax/types.py:715-729`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "_type")]
pub enum Attachment {
    #[serde(rename = "PHOTO")]
    Photo(PhotoAttach),
    #[serde(rename = "VIDEO")]
    Video(VideoAttach),
    #[serde(rename = "FILE")]
    File(FileAttach),
    #[serde(rename = "STICKER")]
    Sticker(StickerAttach),
    #[serde(rename = "AUDIO")]
    Audio(AudioAttach),
    #[serde(rename = "CONTROL")]
    Control(ControlAttach),
    #[serde(rename = "CONTACT")]
    Contact(ContactAttach),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoAttach {
    pub base_url: String,
    pub height: i32,
    pub width: i32,
    pub photo_id: i64,
    pub photo_token: String,
    #[serde(default)]
    pub preview_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoAttach {
    pub height: i32,
    pub width: i32,
    pub video_id: i64,
    pub duration: i64,
    #[serde(default)]
    pub preview_data: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    pub token: String,
    #[serde(default)]
    pub video_type: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAttach {
    pub file_id: i64,
    pub name: String,
    pub size: i64,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickerAttach {
    pub sticker_id: i64,
    pub set_id: i64,
    pub url: String,
    #[serde(default)]
    pub author_type: Option<String>,
    #[serde(default)]
    pub sticker_type: Option<String>,
    #[serde(default)]
    pub audio: Option<bool>,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub lottie_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioAttach {
    pub audio_id: i64,
    pub url: String,
    pub duration: i64,
    #[serde(default)]
    pub wave: Option<String>,
    #[serde(default)]
    pub transcription_status: Option<String>,
    pub token: String,
}

/// Control attachment for system events (chat created, members changed, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlAttach {
    pub event: String,
    /// All extra fields not covered above.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactAttach {
    pub contact_id: i64,
    pub first_name: String,
    pub last_name: String,
    pub name: String,
    pub photo_url: String,
}
