use serde_json::json;
use tracing::info;

use crate::client::MaxClient;
use crate::error::{self, Result};
use crate::protocol::Opcode;
use crate::types::FolderList;

impl MaxClient {
    /// Get all chat folders.
    pub async fn get_folders(&self) -> Result<FolderList> {
        let payload = json!({"folderSync": 0});
        let response = self.transport.request(Opcode::FoldersGet, payload).await?;
        error::check_payload(&response.payload)?;

        serde_json::from_value(response.payload)
            .map_err(|e| crate::error::Error::UnexpectedResponse(e.to_string()))
    }

    /// Create a chat folder.
    pub async fn create_folder(
        &self,
        id: &str,
        title: &str,
        chat_ids: &[i64],
    ) -> Result<serde_json::Value> {
        info!(title, "Creating folder");

        let payload = json!({
            "id": id,
            "title": title,
            "include": chat_ids,
            "filters": [],
        });

        let response = self
            .transport
            .request(Opcode::FoldersUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;
        Ok(response.payload)
    }

    /// Update a chat folder.
    pub async fn update_folder(
        &self,
        id: &str,
        title: &str,
        chat_ids: &[i64],
    ) -> Result<serde_json::Value> {
        let payload = json!({
            "id": id,
            "title": title,
            "include": chat_ids,
            "filters": [],
            "options": [],
        });

        let response = self
            .transport
            .request(Opcode::FoldersUpdate, payload)
            .await?;
        error::check_payload(&response.payload)?;
        Ok(response.payload)
    }

    /// Delete chat folders.
    pub async fn delete_folders(&self, folder_ids: &[String]) -> Result<()> {
        let payload = json!({"folderIds": folder_ids});
        let response = self
            .transport
            .request(Opcode::FoldersDelete, payload)
            .await?;
        error::check_payload(&response.payload)?;
        Ok(())
    }
}
