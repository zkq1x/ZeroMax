use serde_json::json;
use tracing::info;

use crate::client::MaxClient;
use crate::error::{self, Result};
use crate::protocol::Opcode;

impl MaxClient {
    /// Change the current user's profile.
    pub async fn change_profile(
        &self,
        first_name: &str,
        last_name: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        info!("Changing profile");

        let mut payload = json!({
            "firstName": first_name,
            "avatarType": "USER_AVATAR",
        });

        if let Some(ln) = last_name {
            payload["lastName"] = json!(ln);
        }
        if let Some(desc) = description {
            payload["description"] = json!(desc);
        }

        let response = self.transport.request(Opcode::Profile, payload).await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Logout the current session.
    pub async fn logout(&self) -> Result<()> {
        info!("Logging out");
        let response = self.transport.request(Opcode::Logout, json!({})).await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }

    /// Close all sessions except the current one.
    pub async fn close_all_sessions(&self) -> Result<()> {
        info!("Closing all sessions");
        let response = self
            .transport
            .request(Opcode::SessionsClose, json!({}))
            .await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }
}
