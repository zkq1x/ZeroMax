use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tracing::info;

fn extract_last_msg_text(raw: &serde_json::Value) -> String {
    raw.get("lastMessage")
        .and_then(|lm| {
            lm.get("message").and_then(|m| m.get("text")).and_then(|t| t.as_str())
                .or_else(|| lm.get("text").and_then(|t| t.as_str()))
        })
        .unwrap_or("").to_string()
}

fn extract_last_msg_time(raw: &serde_json::Value) -> i64 {
    raw.get("lastMessage")
        .and_then(|lm| {
            lm.get("message").and_then(|m| m.get("time")).and_then(|t| t.as_i64())
                .or_else(|| lm.get("time").and_then(|t| t.as_i64()))
        })
        .unwrap_or(0)
}

use crate::client::MaxClient;
use crate::error::{self, Error, Result};
use crate::protocol::Opcode;
use crate::types::Chat;

impl MaxClient {
    /// Get info about one or more chats by ID.
    pub async fn get_chats(&self, chat_ids: &[i64]) -> Result<Vec<Chat>> {
        let payload = json!({"chatIds": chat_ids});
        let response = self.transport.request(Opcode::ChatInfo, payload).await?;
        error::check_payload(&response.payload)?;

        let chats = response
            .payload
            .get("chats")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<Chat>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(chats)
    }

    /// Get info about a single chat.
    pub async fn get_chat(&self, chat_id: i64) -> Result<Chat> {
        let mut chats = self.get_chats(&[chat_id]).await?;
        chats
            .pop()
            .ok_or_else(|| Error::UnexpectedResponse("Chat not found".into()))
    }

    /// Create a new group chat.
    pub async fn create_group(
        &mut self,
        name: &str,
        participant_ids: &[i64],
        notify: bool,
    ) -> Result<Chat> {
        info!(name, "Creating group");

        let cid = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let payload = json!({
            "message": {
                "cid": cid,
                "attaches": [{
                    "_type": "CONTROL",
                    "event": "new",
                    "chatType": "CHAT",
                    "title": name,
                    "userIds": participant_ids,
                }],
            },
            "notify": notify,
        });

        let response = self.transport.request(Opcode::MsgSend, payload).await?;
        error::check_payload(&response.payload)?;

        let chat: Chat = response
            .payload
            .get("chat")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .ok_or_else(|| Error::UnexpectedResponse("No chat in create_group response".into()))?;

        self.chats.push(chat.clone());
        Ok(chat)
    }

    /// Join a chat by invite link.
    pub async fn join_chat(&mut self, link: &str) -> Result<Chat> {
        let proceed_link = extract_join_path(link);

        let payload = json!({"link": proceed_link});
        let response = self.transport.request(Opcode::ChatJoin, payload).await?;
        error::check_payload(&response.payload)?;

        let chat: Chat = response
            .payload
            .get("chat")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .ok_or_else(|| Error::UnexpectedResponse("No chat in join response".into()))?;

        self.chats.push(chat.clone());
        Ok(chat)
    }

    /// Leave a chat.
    pub async fn leave_chat(&mut self, chat_id: i64) -> Result<()> {
        let payload = json!({"chatId": chat_id});
        let response = self.transport.request(Opcode::ChatLeave, payload).await?;
        error::check_payload(&response.payload)?;

        self.chats.retain(|c| c.id != chat_id);
        self.channels.retain(|c| c.id != chat_id);
        Ok(())
    }

    /// Invite users to a chat.
    pub async fn invite_users(
        &self,
        chat_id: i64,
        user_ids: &[i64],
        show_history: bool,
    ) -> Result<Chat> {
        let payload = json!({
            "chatId": chat_id,
            "userIds": user_ids,
            "showHistory": show_history,
            "operation": "add",
        });

        let response = self
            .transport
            .request(Opcode::ChatMembersUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("chat")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .ok_or_else(|| Error::UnexpectedResponse("No chat in invite response".into()))
    }

    /// Remove users from a chat.
    pub async fn remove_users(
        &self,
        chat_id: i64,
        user_ids: &[i64],
        clean_msg_period: i32,
    ) -> Result<()> {
        let payload = json!({
            "chatId": chat_id,
            "userIds": user_ids,
            "operation": "remove",
            "cleanMsgPeriod": clean_msg_period,
        });

        let response = self
            .transport
            .request(Opcode::ChatMembersUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Fetch chat list with pagination, distributing into dialogs/chats/channels.
    pub async fn fetch_chats(&mut self, marker: Option<i64>) -> Result<Vec<Chat>> {
        let marker = marker.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        });

        let payload = json!({"marker": marker});
        let response = self.transport.request(Opcode::ChatsList, payload).await?;
        error::check_payload(&response.payload)?;

        let raw_chats = response.payload.get("chats")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut result: Vec<Chat> = Vec::new();

        for raw in &raw_chats {
            let chat_type = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match chat_type {
                "DIALOG" => {
                    if let Ok(mut d) = serde_json::from_value::<crate::types::Dialog>(raw.clone()) {
                        d.last_message_text = extract_last_msg_text(raw);
                        d.last_message_time = extract_last_msg_time(raw);
                        if !self.dialogs.iter().any(|x| x.id == d.id) {
                            self.dialogs.push(d);
                        }
                    }
                }
                "CHANNEL" => {
                    if let Ok(mut c) = serde_json::from_value::<Chat>(raw.clone()) {
                        c.last_message_text = extract_last_msg_text(raw);
                        c.last_message_time = extract_last_msg_time(raw);
                        if !self.channels.iter().any(|x| x.id == c.id) {
                            self.channels.push(c.clone());
                        }
                        result.push(c);
                    }
                }
                _ => {
                    if let Ok(mut c) = serde_json::from_value::<Chat>(raw.clone()) {
                        c.last_message_text = extract_last_msg_text(raw);
                        c.last_message_time = extract_last_msg_time(raw);
                        if !self.chats.iter().any(|x| x.id == c.id) {
                            self.chats.push(c.clone());
                        }
                        result.push(c);
                    }
                }
            }
        }

        Ok(result)
    }

    /// Get chat members.
    pub async fn get_members(
        &self,
        chat_id: i64,
        count: i32,
        marker: Option<i32>,
    ) -> Result<serde_json::Value> {
        let payload = json!({
            "type": "MEMBER",
            "chatId": chat_id,
            "count": count,
            "marker": marker,
        });

        let response = self.transport.request(Opcode::ChatMembers, payload).await?;
        error::check_payload(&response.payload)?;
        Ok(response.payload)
    }

    /// Resolve a chat by its invite link (without joining).
    pub async fn resolve_link(&self, link: &str) -> Result<Option<Chat>> {
        let proceed_link = extract_join_path(link);
        let payload = json!({"link": proceed_link});
        let response = self.transport.request(Opcode::LinkInfo, payload).await?;
        error::check_payload(&response.payload)?;

        Ok(response
            .payload
            .get("chat")
            .and_then(|v| serde_json::from_value(v.clone()).ok()))
    }
}

/// Extract the `join/...` path from a full invite link.
fn extract_join_path(link: &str) -> &str {
    link.find("join/")
        .map(|idx| &link[idx..])
        .unwrap_or(link)
}
