use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tracing::{debug, info};

use crate::client::MaxClient;
use crate::error::{self, Result};
use crate::protocol::Opcode;
use crate::types::{Message, ReactionInfo};

impl MaxClient {
    /// Send a text message to a chat.
    ///
    /// Mirrors `MessageMixin.send_message()` from `pymax/mixins/message.py`.
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        notify: bool,
    ) -> Result<Message> {
        info!(chat_id, "Sending message");

        let cid = now_ms();
        let mut message = json!({
            "cid": cid,
            "text": text,
            "elements": [],
            "attaches": [],
        });

        if let Some(reply_id) = reply_to {
            message["link"] = json!({
                "type": "REPLY",
                "messageId": reply_id.to_string(),
            });
        }

        let payload = json!({
            "chatId": chat_id,
            "message": message,
            "notify": notify,
        });

        let response = self.transport.request(Opcode::MsgSend, payload).await?;
        error::check_payload(&response.payload)?;

        Message::from_payload(&response.payload)
            .ok_or_else(|| error::Error::UnexpectedResponse("Missing message in response".into()))
    }

    /// Edit an existing message.
    pub async fn edit_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> Result<Message> {
        info!(chat_id, message_id, "Editing message");

        let payload = json!({
            "chatId": chat_id,
            "messageId": message_id,
            "text": text,
            "elements": [],
            "attaches": [],
        });

        let response = self.transport.request(Opcode::MsgEdit, payload).await?;
        error::check_payload(&response.payload)?;

        Message::from_payload(&response.payload)
            .ok_or_else(|| error::Error::UnexpectedResponse("Missing message in response".into()))
    }

    /// Delete messages.
    pub async fn delete_message(
        &self,
        chat_id: i64,
        message_ids: &[i64],
        for_me: bool,
    ) -> Result<()> {
        info!(chat_id, count = message_ids.len(), "Deleting messages");

        let payload = json!({
            "chatId": chat_id,
            "messageIds": message_ids,
            "forMe": for_me,
        });

        let response = self.transport.request(Opcode::MsgDelete, payload).await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Pin a message in a chat.
    pub async fn pin_message(
        &self,
        chat_id: i64,
        message_id: i64,
        notify_pin: bool,
    ) -> Result<()> {
        let payload = json!({
            "chatId": chat_id,
            "notifyPin": notify_pin,
            "pinMessageId": message_id,
        });

        let response = self.transport.request(Opcode::ChatUpdate, payload).await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Fetch message history for a chat.
    pub async fn fetch_history(
        &self,
        chat_id: i64,
        from_time: Option<i64>,
        forward: i32,
        backward: i32,
    ) -> Result<Vec<Message>> {
        let from = from_time.unwrap_or_else(|| now_ms() as i64);

        info!(chat_id, from, forward, backward, "Fetching history");

        let payload = json!({
            "chatId": chat_id,
            "from": from,
            "forward": forward,
            "backward": backward,
            "getMessages": true,
        });

        let response = self
            .transport
            .send_and_wait(Opcode::ChatHistory, payload, 0, std::time::Duration::from_secs(10))
            .await?;
        error::check_payload(&response.payload)?;

        // Debug: show payload keys and messages array length.
        if let Some(obj) = response.payload.as_object() {
            let keys: Vec<&String> = obj.keys().collect();
            let msg_count = obj.get("messages").and_then(|v| v.as_array()).map(|a| a.len());
            eprintln!("[DEBUG] history response keys={:?} messages_count={:?}", keys, msg_count);
            // Show first message structure if available.
            if let Some(msgs) = obj.get("messages").and_then(|v| v.as_array()) {
                if let Some(first) = msgs.first() {
                    let keys: Vec<&String> = first.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                    eprintln!("[DEBUG] first message keys={:?}", keys);
                }
            }
        }

        let messages: Vec<Message> = response
            .payload
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(Message::from_payload)
                    .collect()
            })
            .unwrap_or_default();

        debug!(count = messages.len(), "History fetched");
        Ok(messages)
    }

    /// Send typing indicator.
    pub async fn send_typing(&self, chat_id: i64) -> Result<()> {
        let payload = json!({"chatId": chat_id});
        self.transport.request(Opcode::MsgTyping, payload).await?;
        Ok(())
    }

    /// Mark a message as read.
    pub async fn read_message(&self, chat_id: i64, message_id: i64) -> Result<()> {
        let payload = json!({
            "type": "READ_MESSAGE",
            "chatId": chat_id,
            "messageId": message_id.to_string(),
            "mark": now_ms(),
        });

        let response = self.transport.request(Opcode::ChatMark, payload).await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    // ── Reactions ──────────────────────────────────────────────────

    /// Add a reaction (emoji) to a message.
    pub async fn add_reaction(
        &self,
        chat_id: i64,
        message_id: &str,
        reaction: &str,
    ) -> Result<Option<ReactionInfo>> {
        let payload = json!({
            "chatId": chat_id,
            "messageId": message_id,
            "reaction": {
                "reactionType": "EMOJI",
                "id": reaction,
            },
        });

        let response = self.transport.request(Opcode::MsgReaction, payload).await?;
        error::check_payload(&response.payload)?;

        let info = response
            .payload
            .get("reactionInfo")
            .and_then(|v| serde_json::from_value::<ReactionInfo>(v.clone()).ok());
        Ok(info)
    }

    /// Remove your reaction from a message.
    pub async fn remove_reaction(
        &self,
        chat_id: i64,
        message_id: &str,
    ) -> Result<Option<ReactionInfo>> {
        let payload = json!({
            "chatId": chat_id,
            "messageId": message_id,
        });

        let response = self
            .transport
            .request(Opcode::MsgCancelReaction, payload)
            .await?;
        error::check_payload(&response.payload)?;

        let info = response
            .payload
            .get("reactionInfo")
            .and_then(|v| serde_json::from_value::<ReactionInfo>(v.clone()).ok());
        Ok(info)
    }

    /// Get reactions for messages.
    pub async fn get_reactions(
        &self,
        chat_id: i64,
        message_ids: &[String],
    ) -> Result<std::collections::HashMap<String, ReactionInfo>> {
        let payload = json!({
            "chatId": chat_id,
            "messageIds": message_ids,
        });

        let response = self
            .transport
            .request(Opcode::MsgGetReactions, payload)
            .await?;
        error::check_payload(&response.payload)?;

        let mut result = std::collections::HashMap::new();
        if let Some(reactions) = response.payload.get("messagesReactions").and_then(|v| v.as_object()) {
            for (msg_id, data) in reactions {
                if let Ok(info) = serde_json::from_value::<ReactionInfo>(data.clone()) {
                    result.insert(msg_id.clone(), info);
                }
            }
        }
        Ok(result)
    }

    /// Get a file download URL.
    pub async fn get_file_url(
        &self,
        chat_id: i64,
        message_id: i64,
        file_id: i64,
    ) -> Result<String> {
        let payload = json!({
            "chatId": chat_id,
            "messageId": message_id.to_string(),
            "fileId": file_id,
        });

        let response = self.transport.request(Opcode::FileDownload, payload).await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| error::Error::UnexpectedResponse("No URL in file response".into()))
    }

    /// Get a video play URL.
    pub async fn get_video_url(
        &self,
        chat_id: i64,
        message_id: i64,
        video_id: i64,
    ) -> Result<String> {
        let payload = json!({
            "chatId": chat_id,
            "messageId": message_id.to_string(),
            "videoId": video_id,
        });

        let response = self.transport.request(Opcode::VideoPlay, payload).await?;
        error::check_payload(&response.payload)?;

        // Video response has a non-standard shape — find the URL field.
        let url = response
            .payload
            .as_object()
            .and_then(|obj| {
                obj.iter()
                    .find(|(k, _)| *k != "EXTERNAL" && *k != "cache")
                    .and_then(|(_, v)| v.as_str())
            })
            .map(|s| s.to_string())
            .ok_or_else(|| error::Error::UnexpectedResponse("No URL in video response".into()))?;

        Ok(url)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
