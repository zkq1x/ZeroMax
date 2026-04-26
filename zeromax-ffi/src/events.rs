use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{debug, warn};

use zeromax_core::protocol::Opcode;
use zeromax_core::transport::Frame;
use zeromax_core::types::common::MessageStatus;
use zeromax_core::types::message::Message;

use crate::types::{FfiChatItem, FfiMessage};

/// Callback interface exposed to Swift/Kotlin via UniFFI.
pub trait EventListener: Send + Sync {
    fn on_new_message(&self, message: FfiMessage);
    fn on_message_edited(&self, message: FfiMessage);
    fn on_message_deleted(&self, message: FfiMessage);
    fn on_chat_updated(&self, chat: FfiChatItem);
    fn on_typing(&self, chat_id: i64, user_id: i64);
}

/// Spawn a task that reads frames and calls the listener.
pub fn spawn_event_bridge(
    mut rx: broadcast::Receiver<Frame>,
    listener: Arc<dyn EventListener>,
    my_id: Option<i64>,
    user_names: Arc<std::sync::Mutex<HashMap<i64, String>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        debug!("FFI event bridge started");
        loop {
            let frame = match rx.recv().await {
                Ok(f) => f,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Event bridge lagged");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Event bridge channel closed");
                    break;
                }
            };

            match frame.opcode {
                op if op == Opcode::NotifMessage as u16 => {
                    if let Some(msg) = Message::from_payload(&frame.payload) {
                        let mut ffi_msg = FfiMessage::from_core(&msg, my_id);
                        // Enrich with cached sender name.
                        if ffi_msg.sender_id != 0 && ffi_msg.sender_name.is_empty() {
                            if let Some(name) = user_names.lock().unwrap().get(&ffi_msg.sender_id) {
                                ffi_msg.sender_name = name.clone();
                            }
                        }
                        match &msg.status {
                            Some(MessageStatus::Edited) => listener.on_message_edited(ffi_msg),
                            Some(MessageStatus::Removed) => listener.on_message_deleted(ffi_msg),
                            _ => listener.on_new_message(ffi_msg),
                        }
                    }
                }
                op if op == Opcode::NotifTyping as u16 => {
                    let p = &frame.payload;
                    if let (Some(chat_id), Some(user_id)) = (
                        p.get("chatId").and_then(|v| v.as_i64()),
                        p.get("userId").and_then(|v| v.as_i64()),
                    ) {
                        listener.on_typing(chat_id, user_id);
                    }
                }
                op if op == Opcode::NotifChat as u16 => {
                    if let Some(chat_val) = frame.payload.get("chat") {
                        if let Ok(chat) =
                            serde_json::from_value::<zeromax_core::types::chat::Chat>(
                                chat_val.clone(),
                            )
                        {
                            listener.on_chat_updated(FfiChatItem::from_chat(&chat));
                        }
                    }
                }
                _ => {}
            }
        }
        debug!("FFI event bridge exited");
    })
}
