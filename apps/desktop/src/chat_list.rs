use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use tokio::sync::Mutex;
use zeromax_core::types::{Chat, Dialog};
use zeromax_core::User;

use crate::auth::ClientHandle;

/// One row of the sidebar chat list — flat shape consumed by Slint.
#[derive(Clone, Debug)]
pub struct ChatRow {
    pub id: i64,
    /// Kind drives conversation-loading dispatch in phase 5.
    #[allow(dead_code)]
    pub kind: ChatKind,
    pub title: String,
    pub preview: String,
    pub time_label: String,
    pub initial: String,
    pub avatar_color: (u8, u8, u8),
    /// Original `last_message_time` (ms) — kept for tie-breaking and incremental updates.
    pub last_event_time: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatKind {
    Dialog,
    Chat,
    Channel,
}

#[derive(Clone, Default)]
struct ResolvedUser {
    name: String,
    #[allow(dead_code)]
    avatar_url: Option<String>,
}

pub struct ChatListViewModel {
    client: ClientHandle,
    user_cache: Arc<Mutex<HashMap<i64, ResolvedUser>>>,
}

impl ChatListViewModel {
    pub fn new(client: ClientHandle) -> Self {
        Self {
            client,
            user_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build the merged & sorted chat list.
    ///
    /// Resolves missing dialog participants by calling `fetch_users` once per load.
    pub async fn load(&self) -> Result<Vec<ChatRow>> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow!("Client not connected"))?;

        let my_id = client.me.as_ref().map(|m| m.id).unwrap_or(0);

        // Seed user cache from any contacts we already have.
        {
            let mut cache = self.user_cache.lock().await;
            for u in &client.contacts {
                cache
                    .entry(u.id)
                    .or_insert_with(|| user_info_from(u));
            }
        }

        // Collect dialog participant ids that we don't know yet.
        let missing: Vec<i64> = {
            let cache = self.user_cache.lock().await;
            let mut set = std::collections::BTreeSet::<i64>::new();
            for d in &client.dialogs {
                for uid_str in d.participants.keys() {
                    if let Ok(uid) = uid_str.parse::<i64>() {
                        if uid != my_id && !cache.contains_key(&uid) {
                            set.insert(uid);
                        }
                    }
                }
            }
            set.into_iter().collect()
        };

        if !missing.is_empty() {
            match client.fetch_users(&missing).await {
                Ok(users) => {
                    let mut cache = self.user_cache.lock().await;
                    for u in &users {
                        cache.insert(u.id, user_info_from(u));
                    }
                }
                Err(e) => tracing::warn!(error = %e, count = missing.len(), "fetch_users failed"),
            }
        }

        let cache = self.user_cache.lock().await.clone();

        let mut rows: Vec<ChatRow> = Vec::new();

        for d in &client.dialogs {
            if d.id == 0 {
                continue;
            }
            rows.push(row_from_dialog(d, my_id, &cache));
        }
        for c in &client.chats {
            rows.push(row_from_chat(c, ChatKind::Chat));
        }
        for c in &client.channels {
            rows.push(row_from_chat(c, ChatKind::Channel));
        }

        rows.sort_by(|a, b| b.last_event_time.cmp(&a.last_event_time));
        Ok(rows)
    }
}

fn user_info_from(u: &User) -> ResolvedUser {
    let name = best_user_name(u);
    ResolvedUser {
        name,
        avatar_url: u.base_url.clone(),
    }
}

fn best_user_name(u: &User) -> String {
    for n in &u.names {
        if let Some(s) = n.name.as_deref() {
            if !s.is_empty() {
                return s.to_string();
            }
        }
        let first = n.first_name.as_deref().unwrap_or("");
        let last = n.last_name.as_deref().unwrap_or("");
        let combined = format!("{first} {last}").trim().to_string();
        if !combined.is_empty() {
            return combined;
        }
    }
    format!("User {}", u.id)
}

fn row_from_dialog(d: &Dialog, my_id: i64, cache: &HashMap<i64, ResolvedUser>) -> ChatRow {
    let other_id = d
        .participants
        .keys()
        .filter_map(|s| s.parse::<i64>().ok())
        .find(|&uid| uid != my_id)
        .unwrap_or(0);

    let title = cache
        .get(&other_id)
        .map(|u| u.name.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("User {other_id}"));

    let last_time = effective_time(d.last_message_time, d.last_event_time);

    ChatRow {
        id: d.id,
        kind: ChatKind::Dialog,
        initial: initial_of(&title),
        avatar_color: color_for_id(other_id),
        title,
        preview: d.last_message_text.clone(),
        time_label: format_time(last_time),
        last_event_time: last_time,
    }
}

fn row_from_chat(c: &Chat, kind: ChatKind) -> ChatRow {
    let title = c
        .title
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            ChatKind::Channel => format!("Channel {}", c.id),
            _ => format!("Chat {}", c.id),
        });

    let last_time = effective_time(c.last_message_time, c.last_event_time);

    ChatRow {
        id: c.id,
        kind,
        initial: initial_of(&title),
        avatar_color: color_for_id(c.id),
        title,
        preview: c.last_message_text.clone(),
        time_label: format_time(last_time),
        last_event_time: last_time,
    }
}

fn effective_time(last_message: i64, last_event: i64) -> i64 {
    if last_message > 0 {
        last_message
    } else {
        last_event
    }
}

fn initial_of(title: &str) -> String {
    title
        .chars()
        .find(|c| !c.is_whitespace())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// 8-color palette for avatar placeholders, indexed deterministically by id.
const AVATAR_COLORS: [(u8, u8, u8); 8] = [
    (0xe9, 0x6b, 0x6b), // red
    (0xed, 0x95, 0x55), // orange
    (0xea, 0xc8, 0x4b), // yellow
    (0x6f, 0xc1, 0x6f), // green
    (0x4e, 0xa6, 0xd4), // blue
    (0x7e, 0x7c, 0xe2), // indigo
    (0xb3, 0x6b, 0xd3), // purple
    (0xea, 0x7c, 0xa9), // pink
];

fn color_for_id(id: i64) -> (u8, u8, u8) {
    let idx = (id.unsigned_abs() % AVATAR_COLORS.len() as u64) as usize;
    AVATAR_COLORS[idx]
}

fn format_time(ts_millis: i64) -> String {
    if ts_millis <= 0 {
        return String::new();
    }
    let Some(dt_utc) = DateTime::from_timestamp_millis(ts_millis) else {
        return String::new();
    };
    let local: DateTime<Local> = dt_utc.into();
    let now: DateTime<Local> = Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.year() == now.year() {
        local.format("%d.%m").to_string()
    } else {
        local.format("%d.%m.%y").to_string()
    }
}

// chrono's `Datelike` trait is needed for `.year()` above.
use chrono::Datelike;
