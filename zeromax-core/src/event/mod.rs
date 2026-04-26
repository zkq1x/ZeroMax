pub mod filter;
pub mod handler;

pub use filter::{BoxFilter, Filter, Filters};
pub use handler::{handler, BoxHandler, FilteredHandler, HandlerRegistry, ReactionEvent};

use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use crate::protocol::Opcode;
use crate::transport::Frame;
use crate::types::{Chat, Message, MessageStatus, ReactionInfo};

/// Spawn a background task that reads incoming frames and dispatches to handlers.
///
/// Mirrors `BaseTransport._dispatch_incoming()` from `pymax/interfaces.py`.
pub fn spawn_dispatcher(
    mut rx: broadcast::Receiver<Frame>,
    handlers: Arc<HandlerRegistry>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        debug!("Event dispatcher started");

        loop {
            let frame = match rx.recv().await {
                Ok(f) => f,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "Dispatcher lagged, some events dropped");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Dispatcher channel closed");
                    break;
                }
            };

            // Raw handlers — every frame.
            for h in &handlers.on_raw {
                let h = h.clone();
                let f = frame.clone();
                tokio::spawn(async move { h(f).await });
            }

            match frame.opcode {
                op if op == Opcode::NotifMessage as u16 => {
                    dispatch_message(&frame, &handlers).await;
                }
                op if op == Opcode::NotifChat as u16 => {
                    dispatch_chat_update(&frame, &handlers).await;
                }
                op if op == Opcode::NotifMsgReactionsChanged as u16 => {
                    dispatch_reaction(&frame, &handlers).await;
                }
                _ => {}
            }
        }

        debug!("Event dispatcher exited");
    })
}

/// Dispatch a NOTIF_MESSAGE to the appropriate handler list.
async fn dispatch_message(frame: &Frame, handlers: &HandlerRegistry) {
    let Some(msg) = Message::from_payload(&frame.payload) else {
        return;
    };

    let handler_list = match &msg.status {
        Some(MessageStatus::Edited) => &handlers.on_message_edit,
        Some(MessageStatus::Removed) => &handlers.on_message_delete,
        _ => &handlers.on_message,
    };

    for (filter, h) in handler_list {
        let pass = match filter {
            Some(f) => f.matches(&msg),
            None => true,
        };
        if pass {
            let h = h.clone();
            let m = msg.clone();
            tokio::spawn(async move { h(m).await });
        }
    }
}

/// Dispatch a NOTIF_CHAT to chat update handlers.
async fn dispatch_chat_update(frame: &Frame, handlers: &HandlerRegistry) {
    let Some(chat_val) = frame.payload.get("chat") else {
        return;
    };
    let Ok(chat) = serde_json::from_value::<Chat>(chat_val.clone()) else {
        return;
    };

    for h in &handlers.on_chat_update {
        let h = h.clone();
        let c = chat.clone();
        tokio::spawn(async move { h(c).await });
    }
}

/// Dispatch a NOTIF_MSG_REACTIONS_CHANGED to reaction handlers.
async fn dispatch_reaction(frame: &Frame, handlers: &HandlerRegistry) {
    let p = &frame.payload;

    let Some(chat_id) = p.get("chatId").and_then(|v| v.as_i64()) else {
        return;
    };
    let Some(message_id) = p.get("messageId").and_then(|v| v.as_str()) else {
        return;
    };

    let info = ReactionInfo {
        total_count: p.get("totalCount").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        your_reaction: p.get("yourReaction").and_then(|v| v.as_str()).map(|s| s.to_string()),
        counters: p
            .get("counters")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
    };

    let event = ReactionEvent {
        message_id: message_id.to_string(),
        chat_id,
        reaction_info: info,
    };

    for h in &handlers.on_reaction_change {
        let h = h.clone();
        let e = event.clone();
        tokio::spawn(async move { h(e).await });
    }
}
