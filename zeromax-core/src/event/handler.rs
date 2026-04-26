use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::types::{Chat, Message, ReactionInfo};
use crate::transport::Frame;

/// Type-erased async handler.
pub type BoxHandler<T> = Arc<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Helper to box an async closure into a `BoxHandler`.
///
/// Usage:
/// ```ignore
/// let h = handler(|msg: Message| async move {
///     println!("Got: {}", msg.text);
/// });
/// ```
pub fn handler<T, F, Fut>(f: F) -> BoxHandler<T>
where
    T: Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    Arc::new(move |arg| Box::pin(f(arg)))
}

/// A handler paired with an optional message filter.
pub type FilteredHandler = (
    Option<super::filter::BoxFilter>,
    BoxHandler<Message>,
);

/// All registered event handlers.
///
/// Mirrors the handler lists from `pymax/protocols.py:76-89`.
pub struct HandlerRegistry {
    pub on_message: Vec<FilteredHandler>,
    pub on_message_edit: Vec<FilteredHandler>,
    pub on_message_delete: Vec<FilteredHandler>,
    pub on_chat_update: Vec<BoxHandler<Chat>>,
    pub on_reaction_change: Vec<BoxHandler<ReactionEvent>>,
    pub on_raw: Vec<BoxHandler<Frame>>,
    pub on_start: Option<BoxHandler<()>>,
}

/// Data passed to reaction change handlers.
#[derive(Debug, Clone)]
pub struct ReactionEvent {
    pub message_id: String,
    pub chat_id: i64,
    pub reaction_info: ReactionInfo,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            on_message: Vec::new(),
            on_message_edit: Vec::new(),
            on_message_delete: Vec::new(),
            on_chat_update: Vec::new(),
            on_reaction_change: Vec::new(),
            on_raw: Vec::new(),
            on_start: None,
        }
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
