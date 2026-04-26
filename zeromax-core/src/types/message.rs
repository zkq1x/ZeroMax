use serde::{Deserialize, Serialize};

use super::attachment::Attachment;
use super::common::{MessageStatus, MessageType};
use super::reaction::ReactionInfo;

/// A text formatting element within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    #[serde(rename = "type")]
    pub element_type: String,
    pub length: i32,
    #[serde(rename = "from", default)]
    pub from_pos: Option<i32>,
}

/// A link to another message (reply).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageLink {
    pub chat_id: i64,
    pub message: Box<InnerMessage>,
    #[serde(rename = "type")]
    pub link_type: String,
}

/// Inner message structure as it appears on the wire inside `payload.message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InnerMessage {
    pub id: i64,
    pub time: i64,
    #[serde(default)]
    pub text: String,
    #[serde(rename = "type", default)]
    pub msg_type: Option<MessageType>,
    #[serde(default)]
    pub sender: Option<i64>,
    #[serde(default)]
    pub status: Option<MessageStatus>,
    #[serde(default)]
    pub elements: Vec<Element>,
    /// Raw attaches — parsed tolerantly to skip unknown types.
    #[serde(default)]
    pub attaches: Vec<serde_json::Value>,
    #[serde(default)]
    pub link: Option<MessageLink>,
    #[serde(default)]
    pub reaction_info: Option<ReactionInfo>,
    #[serde(default)]
    pub options: Option<i64>,
}

/// Full message as seen by the application layer.
///
/// `chat_id` comes from the outer notification payload, while the rest
/// comes from the inner `message` key. See `pymax/types.py:704-705`.
#[derive(Debug, Clone)]
pub struct Message {
    pub chat_id: Option<i64>,
    pub id: i64,
    pub time: i64,
    pub text: String,
    pub msg_type: Option<MessageType>,
    pub sender: Option<i64>,
    pub status: Option<MessageStatus>,
    pub elements: Vec<Element>,
    pub attaches: Vec<Attachment>,
    pub link: Option<MessageLink>,
    pub reaction_info: Option<ReactionInfo>,
    pub options: Option<i64>,
}

impl Message {
    /// Parse a message from a NOTIF_MESSAGE payload.
    ///
    /// Handles the PyMax quirk where `chatId` is at the outer level
    /// but everything else is inside the `message` key.
    pub fn from_payload(payload: &serde_json::Value) -> Option<Self> {
        let chat_id = payload.get("chatId").and_then(|v| v.as_i64());

        let inner_val = payload.get("message").unwrap_or(payload);
        let inner: InnerMessage = match serde_json::from_value(inner_val.clone()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[MSG PARSE ERROR] {e}");
                eprintln!("[MSG PARSE ERROR] keys: {:?}", inner_val.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                return None;
            }
        };

        // Parse attachments tolerantly — skip unknown types.
        let attaches: Vec<Attachment> = inner
            .attaches
            .iter()
            .filter_map(|v| serde_json::from_value::<Attachment>(v.clone()).ok())
            .collect();

        Some(Self {
            chat_id,
            id: inner.id,
            time: inner.time,
            text: inner.text,
            msg_type: inner.msg_type,
            sender: inner.sender,
            status: inner.status,
            elements: inner.elements,
            attaches,
            link: inner.link,
            reaction_info: inner.reaction_info,
            options: inner.options,
        })
    }
}
