pub mod sqlite;

use uuid::Uuid;

use crate::error::Result;

/// Persisted authentication data.
#[derive(Debug, Clone)]
pub struct AuthData {
    pub device_id: Uuid,
    pub token: String,
}

/// Trait for persisting auth tokens and device IDs.
///
/// Mirrors `pymax/crud.py` `Database`.
#[trait_variant::make(Send)]
pub trait SessionStorage: Send + Sync {
    /// Load existing auth data, if any.
    async fn load_auth(&self) -> Result<Option<AuthData>>;

    /// Save or update auth data.
    async fn save_auth(&self, data: &AuthData) -> Result<()>;

    /// Load the device ID, creating one if it doesn't exist.
    async fn get_or_create_device_id(&self) -> Result<Uuid>;
}

pub use sqlite::SqliteStorage;
