/// FFI-compatible mirror types for Swift/Kotlin.
///
/// These are simplified versions of `zeromax_core::types::*` that
/// avoid `serde_json::Value`, `HashMap<String, Value>`, and nested
/// generics — all of which UniFFI cannot handle.

#[derive(Debug, Clone)]
pub struct FfiClientConfig {
    pub phone: String,
    pub work_dir: String,
    pub token: Option<String>,
    /// "WEB" (default, QR login) or "DESKTOP" (phone+code login).
    pub device_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FfiMe {
    pub id: i64,
    pub phone: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub enum FfiChatType {
    Dialog,
    Chat,
    Channel,
}

#[derive(Debug, Clone)]
pub struct FfiChatItem {
    pub id: i64,
    pub chat_type: FfiChatType,
    pub title: String,
    pub last_message_text: String,
    pub last_message_time: i64,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FfiMessage {
    pub id: i64,
    pub chat_id: i64,
    pub time: i64,
    pub text: String,
    pub sender_id: i64,
    pub sender_name: String,
    pub is_outgoing: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FfiCodeResult {
    LoggedIn { token: String },
    TwoFactorRequired { track_id: String, hint: Option<String> },
}

#[derive(Debug, Clone)]
pub struct FfiQrLoginData {
    pub qr_link: String,
    pub track_id: String,
    pub polling_interval_ms: u64,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct FfiUser {
    pub id: i64,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FfiReactionInfo {
    pub total_count: i32,
    pub your_reaction: Option<String>,
}

// ── Conversion from core types ─────────────────────────────────

impl From<&zeromax_core::types::user::Me> for FfiMe {
    fn from(me: &zeromax_core::types::user::Me) -> Self {
        let display_name = me
            .names
            .first()
            .and_then(|n| n.name.clone())
            .or_else(|| me.names.first().and_then(|n| n.first_name.clone()))
            .unwrap_or_default();
        Self {
            id: me.id,
            phone: me.phone.clone(),
            display_name,
        }
    }
}

impl FfiChatItem {
    pub fn from_chat(chat: &zeromax_core::types::chat::Chat) -> Self {
        let chat_type = match chat.chat_type {
            Some(zeromax_core::types::common::ChatType::Dialog) => FfiChatType::Dialog,
            Some(zeromax_core::types::common::ChatType::Channel) => FfiChatType::Channel,
            _ => FfiChatType::Chat,
        };
        Self {
            id: chat.id,
            chat_type,
            title: chat.title.clone().unwrap_or_default(),
            last_message_text: chat.last_message_text.clone(),
            last_message_time: if chat.last_message_time > 0 { chat.last_message_time } else { chat.last_event_time },
            avatar_url: chat.base_icon_url.clone(),
        }
    }

    pub fn from_dialog(dialog: &zeromax_core::types::chat::Dialog) -> Self {
        Self {
            id: dialog.id,
            chat_type: FfiChatType::Dialog,
            title: format!("Dialog {}", dialog.id),
            last_message_text: dialog.last_message_text.clone(),
            last_message_time: if dialog.last_message_time > 0 { dialog.last_message_time } else { dialog.last_event_time },
            avatar_url: None,
        }
    }
}

impl FfiMessage {
    pub fn from_core(msg: &zeromax_core::types::message::Message, my_id: Option<i64>) -> Self {
        Self {
            id: msg.id,
            chat_id: msg.chat_id.unwrap_or(0),
            time: msg.time,
            text: msg.text.clone(),
            sender_id: msg.sender.unwrap_or(0),
            sender_name: String::new(), // Resolved by caller if needed
            is_outgoing: my_id.is_some_and(|me| msg.sender == Some(me)),
            status: msg.status.as_ref().map(|s| format!("{:?}", s)),
        }
    }
}

impl FfiUser {
    pub fn from_core(user: &zeromax_core::types::user::User) -> Self {
        let display_name = user
            .names
            .first()
            .and_then(|n| n.name.clone())
            .or_else(|| user.names.first().and_then(|n| n.first_name.clone()))
            .unwrap_or_default();
        Self {
            id: user.id,
            display_name,
            avatar_url: user.base_url.clone(),
        }
    }
}

impl FfiReactionInfo {
    pub fn from_core(info: &zeromax_core::types::reaction::ReactionInfo) -> Self {
        Self {
            total_count: info.total_count,
            your_reaction: info.your_reaction.clone(),
        }
    }
}

impl From<zeromax_core::protocol::CodeResult> for FfiCodeResult {
    fn from(r: zeromax_core::protocol::CodeResult) -> Self {
        match r {
            zeromax_core::protocol::CodeResult::LoggedIn { token } => {
                FfiCodeResult::LoggedIn { token }
            }
            zeromax_core::protocol::CodeResult::TwoFactorRequired { track_id, hint } => {
                FfiCodeResult::TwoFactorRequired { track_id, hint }
            }
        }
    }
}

impl From<zeromax_core::protocol::QrLoginData> for FfiQrLoginData {
    fn from(qr: zeromax_core::protocol::QrLoginData) -> Self {
        Self {
            qr_link: qr.qr_link,
            track_id: qr.track_id,
            polling_interval_ms: qr.polling_interval_ms,
            expires_at_ms: qr.expires_at_ms,
        }
    }
}
