use crate::types::{Attachment, Message, MessageStatus};

/// Trait for composable event filters.
///
/// Mirrors `BaseFilter` from `pymax/filters.py`.
pub trait Filter: Send + Sync {
    fn matches(&self, message: &Message) -> bool;
}

/// Boxed filter for dynamic dispatch and composition.
pub struct BoxFilter(Box<dyn Filter>);

impl BoxFilter {
    pub fn new(filter: impl Filter + 'static) -> Self {
        Self(Box::new(filter))
    }

    pub fn matches(&self, message: &Message) -> bool {
        self.0.matches(message)
    }
}

// ── Composition operators ──────────────────────────────────────

impl std::ops::BitAnd for BoxFilter {
    type Output = BoxFilter;
    fn bitand(self, rhs: BoxFilter) -> BoxFilter {
        BoxFilter::new(AndFilter(self, rhs))
    }
}

impl std::ops::BitOr for BoxFilter {
    type Output = BoxFilter;
    fn bitor(self, rhs: BoxFilter) -> BoxFilter {
        BoxFilter::new(OrFilter(self, rhs))
    }
}

impl std::ops::Not for BoxFilter {
    type Output = BoxFilter;
    fn not(self) -> BoxFilter {
        BoxFilter::new(NotFilter(self))
    }
}

// ── Combinator structs ─────────────────────────────────────────

struct AndFilter(BoxFilter, BoxFilter);
impl Filter for AndFilter {
    fn matches(&self, msg: &Message) -> bool {
        self.0.matches(msg) && self.1.matches(msg)
    }
}

struct OrFilter(BoxFilter, BoxFilter);
impl Filter for OrFilter {
    fn matches(&self, msg: &Message) -> bool {
        self.0.matches(msg) || self.1.matches(msg)
    }
}

struct NotFilter(BoxFilter);
impl Filter for NotFilter {
    fn matches(&self, msg: &Message) -> bool {
        !self.0.matches(msg)
    }
}

// ── Built-in filters ───────────────────────────────────────────

/// Filter by chat ID.
struct ChatFilter(i64);
impl Filter for ChatFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.chat_id == Some(self.0)
    }
}

/// Filter by exact text match.
struct TextFilter(String);
impl Filter for TextFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.text == self.0
    }
}

/// Filter by text containing a substring.
struct TextContainsFilter(String);
impl Filter for TextContainsFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.text.contains(&self.0)
    }
}

/// Filter by regex pattern on text.
struct RegexFilter(regex::Regex);
impl Filter for RegexFilter {
    fn matches(&self, msg: &Message) -> bool {
        self.0.is_match(&msg.text)
    }
}

/// Filter by sender user ID.
struct SenderFilter(i64);
impl Filter for SenderFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.sender == Some(self.0)
    }
}

/// Filter by message status.
struct StatusFilter(MessageStatus);
impl Filter for StatusFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.status.as_ref() == Some(&self.0)
    }
}

/// Filter for messages that have any attachment.
struct HasMediaFilter;
impl Filter for HasMediaFilter {
    fn matches(&self, msg: &Message) -> bool {
        !msg.attaches.is_empty()
    }
}

/// Filter for messages that have a file attachment.
struct HasFileFilter;
impl Filter for HasFileFilter {
    fn matches(&self, msg: &Message) -> bool {
        msg.attaches
            .iter()
            .any(|a| matches!(a, Attachment::File(_)))
    }
}

// ── Public filter constructors (mirrors `Filters` class) ───────

/// Composable message filters.
///
/// Usage:
/// ```ignore
/// use zeromax_core::event::Filters;
/// let f = Filters::chat(123) & Filters::text_contains("hello");
/// ```
pub struct Filters;

impl Filters {
    pub fn chat(chat_id: i64) -> BoxFilter {
        BoxFilter::new(ChatFilter(chat_id))
    }

    pub fn text(exact: impl Into<String>) -> BoxFilter {
        BoxFilter::new(TextFilter(exact.into()))
    }

    pub fn text_contains(sub: impl Into<String>) -> BoxFilter {
        BoxFilter::new(TextContainsFilter(sub.into()))
    }

    pub fn text_matches(pattern: &str) -> BoxFilter {
        let re = regex::Regex::new(pattern).expect("Invalid regex pattern");
        BoxFilter::new(RegexFilter(re))
    }

    pub fn sender(user_id: i64) -> BoxFilter {
        BoxFilter::new(SenderFilter(user_id))
    }

    pub fn status(status: MessageStatus) -> BoxFilter {
        BoxFilter::new(StatusFilter(status))
    }

    pub fn has_media() -> BoxFilter {
        BoxFilter::new(HasMediaFilter)
    }

    pub fn has_file() -> BoxFilter {
        BoxFilter::new(HasFileFilter)
    }
}
