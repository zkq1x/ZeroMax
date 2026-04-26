use serde_json::json;
use tracing::{debug, info};

use crate::client::MaxClient;
use crate::error::{self, Error, Result};
use crate::protocol::Opcode;
use crate::types::{Contact, Session, User};

impl MaxClient {
    /// Fetch user info from the server by IDs.
    pub async fn fetch_users(&self, user_ids: &[i64]) -> Result<Vec<User>> {
        info!(count = user_ids.len(), "Fetching users");

        let payload = json!({"contactIds": user_ids});
        let response = self.transport.request(Opcode::ContactInfo, payload).await?;
        error::check_payload(&response.payload)?;

        let users: Vec<User> = response
            .payload
            .get("contacts")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<User>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        debug!(count = users.len(), "Fetched users");
        Ok(users)
    }

    /// Get a single user by ID.
    pub async fn get_user(&self, user_id: i64) -> Result<Option<User>> {
        let mut users = self.fetch_users(&[user_id]).await?;
        Ok(users.pop())
    }

    /// Search for a user by phone number.
    pub async fn search_by_phone(&self, phone: &str) -> Result<User> {
        info!(phone, "Searching user by phone");

        let payload = json!({"phone": phone});
        let response = self
            .transport
            .request(Opcode::ContactInfoByPhone, payload)
            .await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("contact")
            .and_then(|v| serde_json::from_value::<User>(v.clone()).ok())
            .ok_or_else(|| Error::UnexpectedResponse("No user in phone search response".into()))
    }

    /// Add a contact.
    pub async fn add_contact(&self, contact_id: i64) -> Result<Contact> {
        let payload = json!({
            "contactId": contact_id,
            "action": "ADD",
        });

        let response = self
            .transport
            .request(Opcode::ContactUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("contact")
            .and_then(|v| serde_json::from_value::<Contact>(v.clone()).ok())
            .ok_or_else(|| Error::UnexpectedResponse("No contact in add response".into()))
    }

    /// Remove a contact.
    pub async fn remove_contact(&self, contact_id: i64) -> Result<()> {
        let payload = json!({
            "contactId": contact_id,
            "action": "REMOVE",
        });

        let response = self
            .transport
            .request(Opcode::ContactUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Get all active sessions.
    pub async fn get_sessions(&self) -> Result<Vec<Session>> {
        let response = self
            .transport
            .request(Opcode::SessionsInfo, json!({}))
            .await?;
        error::check_payload(&response.payload)?;

        let sessions: Vec<Session> = response
            .payload
            .get("sessions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value::<Session>(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(sessions)
    }

    /// Compute a dialog (DM) chat ID from two user IDs.
    pub fn compute_dialog_id(user_a: i64, user_b: i64) -> i64 {
        user_a ^ user_b
    }
}
