use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::common::{AccessType, ChatType};
use super::message::Message;

/// A chat (group, channel, or dialog).
///
/// Mirrors `Chat` from `pymax/types.py:837-940`.
/// Also used for `Channel` (which adds no fields in Python).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub cid: i64,
    #[serde(rename = "type", default)]
    pub chat_type: Option<ChatType>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub access: Option<AccessType>,
    #[serde(default)]
    pub owner: i64,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    #[serde(default)]
    pub join_time: i64,
    #[serde(default)]
    pub last_event_time: i64,
    #[serde(default)]
    pub participants_count: i32,
    #[serde(default)]
    pub participants: HashMap<String, i64>,
    #[serde(default)]
    pub admin_participants: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub admins: Vec<i64>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub invited_by: Option<i64>,
    #[serde(default)]
    pub base_icon_url: Option<String>,
    #[serde(default)]
    pub base_raw_icon_url: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, bool>,
    #[serde(default)]
    pub messages_count: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub restrictions: Option<i32>,
    #[serde(default)]
    pub prev_message_id: Option<String>,
    #[serde(default)]
    pub last_fire_delayed_error_time: i64,
    #[serde(default)]
    pub last_delayed_update_time: i64,

    /// Last message — deserialized separately due to the nested structure.
    #[serde(skip)]
    pub last_message: Option<Message>,

    /// Last message text — extracted during sync for sidebar preview.
    #[serde(skip)]
    pub last_message_text: String,

    /// Last message timestamp — for correct sidebar sort order.
    #[serde(skip)]
    pub last_message_time: i64,
}

/// A dialog (1:1 conversation).
///
/// Mirrors `Dialog` from `pymax/types.py:768-834`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub cid: Option<i64>,
    #[serde(rename = "type", default)]
    pub chat_type: Option<ChatType>,
    #[serde(default)]
    pub owner: i64,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    #[serde(default)]
    pub join_time: i64,
    #[serde(default)]
    pub last_event_time: i64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub participants: HashMap<String, i64>,
    #[serde(default)]
    pub options: HashMap<String, bool>,
    #[serde(default)]
    pub has_bots: Option<bool>,
    #[serde(default)]
    pub prev_message_id: Option<String>,
    #[serde(default)]
    pub last_fire_delayed_error_time: i64,
    #[serde(default)]
    pub last_delayed_update_time: i64,

    #[serde(skip)]
    pub last_message: Option<Message>,

    #[serde(skip)]
    pub last_message_text: String,

    #[serde(skip)]
    pub last_message_time: i64,
}
