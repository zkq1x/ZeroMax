use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::Path;
use uuid::Uuid;

use crate::error::Result;
use crate::storage::{AuthData, SessionStorage};

/// SQLite-backed session storage.
///
/// Mirrors `pymax/crud.py` Database + `pymax/models.py` Auth.
pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at the given path.
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;

        // Create table if not exists.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS auth (
                device_id TEXT PRIMARY KEY,
                token TEXT
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

impl SessionStorage for SqliteStorage {
    async fn load_auth(&self) -> Result<Option<AuthData>> {
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT device_id, token FROM auth LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some((device_id_str, Some(token))) => {
                let device_id = Uuid::parse_str(&device_id_str)
                    .map_err(|e| crate::error::Error::UnexpectedResponse(e.to_string()))?;
                Ok(Some(AuthData { device_id, token }))
            }
            _ => Ok(None),
        }
    }

    async fn save_auth(&self, data: &AuthData) -> Result<()> {
        sqlx::query(
            "INSERT INTO auth (device_id, token) VALUES (?, ?)
             ON CONFLICT(device_id) DO UPDATE SET token = excluded.token",
        )
        .bind(data.device_id.to_string())
        .bind(&data.token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_or_create_device_id(&self) -> Result<Uuid> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT device_id FROM auth LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;

        if let Some((id_str,)) = row {
            let id = Uuid::parse_str(&id_str)
                .map_err(|e| crate::error::Error::UnexpectedResponse(e.to_string()))?;
            return Ok(id);
        }

        let new_id = Uuid::new_v4();
        sqlx::query("INSERT INTO auth (device_id) VALUES (?)")
            .bind(new_id.to_string())
            .execute(&self.pool)
            .await?;

        Ok(new_id)
    }
}
