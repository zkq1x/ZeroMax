use serde_json::json;
use tracing::{debug, info, warn};

use crate::client::{MaxClient, UserAgentConfig};
use crate::constants::DEFAULT_SYNC_CHATS_COUNT;
use crate::error::{self, Result};
use crate::protocol::Opcode;
use crate::types::{Chat, Dialog, Me, User};

impl MaxClient {
    /// Perform initial sync after authentication.
    ///
    /// Sends the LOGIN opcode with the stored token to fetch
    /// chats, contacts, and profile. Mirrors `BaseTransport._sync()`
    /// from `pymax/interfaces.py`.
    pub async fn sync(&mut self) -> Result<serde_json::Value> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| error::Error::Auth("No token available for sync".into()))?
            .clone();

        let ua = self.user_agent_payload();

        let payload = json!({
            "interactive": true,
            "token": token,
            "chatsSyncFrom": 0,
            "contactsSyncFrom": 0,
            "presenceSyncFrom": 0,
            "draftsSyncFrom": 0,
            "chatsCount": DEFAULT_SYNC_CHATS_COUNT,
            "userAgent": ua,
        });

        info!("Starting initial sync");
        let response = self.transport.request(Opcode::Login, payload).await?;

        error::check_payload(&response.payload)?;

        // Parse chats by type.
        if let Some(raw_chats) = response.payload.get("chats").and_then(|v| v.as_array()) {
            for raw in raw_chats {
                let chat_type = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match chat_type {
                    "DIALOG" => match serde_json::from_value::<Dialog>(raw.clone()) {
                        Ok(mut d) => {
                            d.last_message_text = extract_last_msg_text(raw);
                            d.last_message_time = extract_last_msg_time(raw);
                            self.dialogs.push(d);
                        }
                        Err(e) => warn!(error = %e, "Failed to parse dialog"),
                    },
                    "CHAT" => match serde_json::from_value::<Chat>(raw.clone()) {
                        Ok(mut c) => {
                            c.last_message_text = extract_last_msg_text(raw);
                            c.last_message_time = extract_last_msg_time(raw);
                            self.chats.push(c);
                        }
                        Err(e) => warn!(error = %e, "Failed to parse chat"),
                    },
                    "CHANNEL" => match serde_json::from_value::<Chat>(raw.clone()) {
                        Ok(mut c) => {
                            c.last_message_text = extract_last_msg_text(raw);
                            c.last_message_time = extract_last_msg_time(raw);
                            self.channels.push(c);
                        }
                        Err(e) => warn!(error = %e, "Failed to parse channel"),
                    },
                    _ => warn!(chat_type, "Unknown chat type"),
                }
            }
        }

        // Parse contacts.
        if let Some(raw_contacts) = response.payload.get("contacts").and_then(|v| v.as_array()) {
            for raw in raw_contacts {
                match serde_json::from_value::<User>(raw.clone()) {
                    Ok(u) => self.contacts.push(u),
                    Err(e) => warn!(error = %e, "Failed to parse contact"),
                }
            }
        }

        // Parse profile.
        let has_profile = response.payload.get("profile").is_some();
        let has_contact = response.payload.get("profile").and_then(|v| v.get("contact")).is_some();
        eprintln!("[DEBUG] sync: has_profile={} has_contact={}", has_profile, has_contact);
        if !has_profile {
            // Try alternative key names.
            let keys: Vec<&String> = response.payload.as_object().map(|o| o.keys().collect()).unwrap_or_default();
            eprintln!("[DEBUG] sync response top-level keys: {:?}", keys);
        }
        if let Some(contact) = response
            .payload
            .get("profile")
            .and_then(|v| v.get("contact"))
        {
            match serde_json::from_value::<Me>(contact.clone()) {
                Ok(me) => {
                    info!(user_id = me.id, "Profile loaded");
                    self.me = Some(me);
                }
                Err(e) => {
                    eprintln!("[DEBUG] Me parse error: {e}");
                    eprintln!("[DEBUG] contact keys: {:?}", contact.as_object().map(|o| o.keys().collect::<Vec<_>>()));
                    warn!(error = %e, "Failed to parse profile");
                }
            }
        }

        eprintln!("[SYNC] dialogs={} chats={} channels={} contacts={}",
            self.dialogs.len(), self.chats.len(), self.channels.len(), self.contacts.len());

        self.sync_data = Some(response.payload.clone());
        Ok(response.payload)
    }

    /// Build the user agent JSON object for protocol payloads.
    pub fn user_agent_payload(&self) -> serde_json::Value {
        let ua = &self.user_agent;
        json!({
            "deviceType": ua.device_type,
            "locale": ua.locale,
            "deviceLocale": ua.device_locale,
            "osVersion": ua.os_version,
            "deviceName": ua.device_name,
            "headerUserAgent": ua.header_user_agent,
            "appVersion": ua.app_version,
            "screen": ua.screen,
            "timezone": ua.timezone,
            "clientSessionId": ua.client_session_id,
            "buildNumber": ua.build_number,
        })
    }
}

/// Extract the text from `lastMessage.message.text` or `lastMessage.text` in raw JSON.
fn extract_last_msg_text(raw: &serde_json::Value) -> String {
    raw.get("lastMessage")
        .and_then(|lm| {
            lm.get("message")
                .and_then(|m| m.get("text"))
                .and_then(|t| t.as_str())
                .or_else(|| lm.get("text").and_then(|t| t.as_str()))
        })
        .unwrap_or("")
        .to_string()
}

/// Extract the time from `lastMessage.message.time` or `lastMessage.time`.
fn extract_last_msg_time(raw: &serde_json::Value) -> i64 {
    raw.get("lastMessage")
        .and_then(|lm| {
            lm.get("message")
                .and_then(|m| m.get("time"))
                .and_then(|t| t.as_i64())
                .or_else(|| lm.get("time").and_then(|t| t.as_i64()))
        })
        .unwrap_or(0)
}

impl UserAgentConfig {
    /// Generate a randomized user agent config that mimics a web client.
    pub fn random() -> Self {
        use crate::constants::*;
        use rand::Rng;

        let mut rng = rand::thread_rng();
        let mut pick = |arr: &[&str]| -> String { arr[rng.gen_range(0..arr.len())].to_string() };

        Self {
            device_type: "WEB".to_string(),
            locale: DEFAULT_LOCALE.to_string(),
            device_locale: DEFAULT_LOCALE.to_string(),
            os_version: pick(OS_VERSIONS),
            device_name: pick(DEVICE_NAMES),
            header_user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
            app_version: DEFAULT_APP_VERSION.to_string(),
            screen: pick(SCREEN_SIZES),
            timezone: pick(TIMEZONES),
            client_session_id: rng.gen_range(1..=15),
            build_number: DEFAULT_BUILD_NUMBER,
        }
    }
}
