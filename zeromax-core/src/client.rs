use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tracing::info;
use uuid::Uuid;

use crate::constants::{API_HOST, API_PORT, SESSION_DB_NAME, WEBSOCKET_URI};
use crate::error::{self, Error, Result};
use crate::event::{self, BoxFilter, BoxHandler, HandlerRegistry, ReactionEvent};
use crate::protocol::Opcode;
use crate::storage::{AuthData, SessionStorage, SqliteStorage};
use crate::transport::{Frame, Transport};
use crate::types::{Chat, Dialog, Me, Message, User};

/// Configuration for the MAX client.
pub struct ClientConfig {
    /// Phone number in international format (e.g. "+79991234567").
    pub phone: String,

    /// WebSocket URI to connect to.
    pub uri: String,

    /// Working directory for session storage.
    pub work_dir: PathBuf,

    /// Session database filename.
    pub session_name: String,

    /// Pre-existing auth token (skips auth flow if set).
    pub token: Option<String>,

    /// Pre-existing device ID.
    pub device_id: Option<Uuid>,

    /// User agent configuration.
    pub user_agent: Option<UserAgentConfig>,

    /// Whether to automatically reconnect on disconnect.
    pub reconnect: bool,

    /// Delay between reconnection attempts.
    pub reconnect_delay_secs: f64,
}

impl ClientConfig {
    /// Create a minimal config with just a phone number.
    pub fn new(phone: impl Into<String>) -> Self {
        Self {
            phone: phone.into(),
            uri: WEBSOCKET_URI.to_string(),
            work_dir: PathBuf::from("."),
            session_name: SESSION_DB_NAME.to_string(),
            token: None,
            device_id: None,
            user_agent: None,
            reconnect: true,
            reconnect_delay_secs: 1.0,
        }
    }

    /// Set the working directory.
    pub fn work_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.work_dir = path.into();
        self
    }

    /// Set a pre-existing token.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Set a custom WebSocket URI.
    pub fn uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = uri.into();
        self
    }

    /// Set the device type ("WEB", "DESKTOP", "ANDROID", "IOS").
    pub fn device_type(mut self, dt: impl Into<String>) -> Self {
        let dt = dt.into();
        let mut ua = self.user_agent.unwrap_or_else(UserAgentConfig::random);
        ua.device_type = dt;
        self.user_agent = Some(ua);
        self
    }

    /// Disable auto-reconnect.
    pub fn no_reconnect(mut self) -> Self {
        self.reconnect = false;
        self
    }
}

/// User agent fields sent during handshake and sync.
///
/// Mirrors `UserAgentPayload` from `pymax/payloads.py`.
#[derive(Debug, Clone)]
pub struct UserAgentConfig {
    pub device_type: String,
    pub locale: String,
    pub device_locale: String,
    pub os_version: String,
    pub device_name: String,
    pub header_user_agent: String,
    pub app_version: String,
    pub screen: String,
    pub timezone: String,
    pub client_session_id: u32,
    pub build_number: u32,
}

/// The main MAX messenger client.
///
/// Mirrors `MaxClient` from `pymax/core.py`.
pub struct MaxClient {
    pub transport: Transport,
    pub(crate) storage: SqliteStorage,
    pub(crate) token: Option<String>,
    pub device_id: Uuid,
    pub user_agent: UserAgentConfig,
    pub(crate) config: ClientConfig,

    /// Raw sync data from the server.
    pub(crate) sync_data: Option<serde_json::Value>,

    /// Current user profile.
    pub me: Option<Me>,
    /// Cached dialogs (1:1 chats).
    pub dialogs: Vec<Dialog>,
    /// Cached group chats.
    pub chats: Vec<Chat>,
    /// Cached channels.
    pub channels: Vec<Chat>,
    /// Cached contacts.
    pub contacts: Vec<User>,

    /// Event handler registry.
    pub(crate) handlers: HandlerRegistry,

    /// Handle to the dispatcher task (if listening).
    dispatcher_handle: Option<tokio::task::JoinHandle<()>>,

    connected: bool,
}

impl MaxClient {
    /// Create a new client from config.
    ///
    /// Opens the session database and loads any persisted auth.
    pub async fn new(config: ClientConfig) -> Result<Self> {
        // Validate phone number.
        let re = regex::Regex::new(crate::constants::PHONE_REGEX).unwrap();
        if !re.is_match(&config.phone) {
            return Err(Error::InvalidPhone(config.phone.clone()));
        }

        // Open storage.
        let db_path = config.work_dir.join(&config.session_name);
        let storage = SqliteStorage::open(&db_path).await?;

        // Load or create device ID.
        let device_id = match config.device_id {
            Some(id) => id,
            None => storage.get_or_create_device_id().await?,
        };

        // Load token from config or storage.
        let token = match &config.token {
            Some(t) => Some(t.clone()),
            None => storage.load_auth().await?.map(|a| a.token),
        };

        let user_agent = config.user_agent.clone().unwrap_or_else(UserAgentConfig::random);

        // Choose transport based on device type.
        let transport = if user_agent.device_type == "WEB" {
            Transport::new_websocket(&config.uri)
        } else {
            Transport::new_socket(API_HOST, API_PORT)
        };

        info!(
            phone = %config.phone,
            device_id = %device_id,
            has_token = token.is_some(),
            "Client created"
        );

        Ok(Self {
            transport,
            storage,
            token,
            device_id,
            user_agent,
            config,
            sync_data: None,
            me: None,
            dialogs: Vec::new(),
            chats: Vec::new(),
            channels: Vec::new(),
            contacts: Vec::new(),
            handlers: HandlerRegistry::new(),
            dispatcher_handle: None,
            connected: false,
        })
    }

    /// Connect to the server, perform handshake, and sync.
    ///
    /// If a token is available, performs token-based login (sync).
    /// Otherwise, auth flow must be done separately.
    pub async fn connect(&mut self) -> Result<()> {
        // 1. Transport connect (WS or Socket).
        match &mut self.transport {
            Transport::WebSocket(ws) => {
                ws.connect(&self.user_agent.header_user_agent).await?;
            }
            Transport::Socket(s) => {
                s.connect().await?;
            }
        }

        // 2. Handshake (SESSION_INIT)
        self.handshake().await?;

        // 3. Sync if we have a token
        if self.token.is_some() {
            self.sync().await?;
        } else {
            info!("No token — skipping sync, auth required");
        }

        self.connected = true;
        Ok(())
    }

    /// Perform the protocol handshake.
    ///
    /// Sends SESSION_INIT with device ID and user agent.
    /// Mirrors `BaseTransport._handshake()` from `pymax/interfaces.py`.
    async fn handshake(&self) -> Result<serde_json::Value> {
        let ua = self.user_agent_payload();

        let payload = json!({
            "deviceId": self.device_id.to_string(),
            "userAgent": ua,
        });

        info!("Sending handshake");
        eprintln!("[DEBUG] Handshake payload: {}", serde_json::to_string_pretty(&payload).unwrap_or_default());
        let response = self.transport.request(Opcode::SessionInit, payload).await?;
        eprintln!("[DEBUG] Handshake response: {}", serde_json::to_string_pretty(&response.payload).unwrap_or_default());

        error::check_payload(&response.payload)?;

        info!("Handshake completed");
        Ok(response.payload)
    }

    /// Save the current token to persistent storage.
    pub async fn persist_token(&self) -> Result<()> {
        if let Some(ref token) = self.token {
            self.storage
                .save_auth(&AuthData {
                    device_id: self.device_id,
                    token: token.clone(),
                })
                .await?;
        }
        Ok(())
    }

    /// Whether the client is connected and synced.
    pub fn is_connected(&self) -> bool {
        self.connected && self.transport.is_connected()
    }

    /// Subscribe to incoming server events (notifications).
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<crate::transport::Frame> {
        self.transport.subscribe()
    }

    /// Reset state for reconnection — clears caches and creates a new transport.
    pub fn reset_for_reconnect(&mut self) {
        self.dialogs.clear();
        self.chats.clear();
        self.channels.clear();
        self.contacts.clear();
        self.me = None;
        self.sync_data = None;
        self.connected = false;
        self.transport = if self.user_agent.device_type == "WEB" {
            Transport::new_websocket(&self.config.uri)
        } else {
            Transport::new_socket(API_HOST, API_PORT)
        };
    }

    /// Close the connection.
    pub async fn close(&mut self) -> Result<()> {
        self.connected = false;
        if let Some(handle) = self.dispatcher_handle.take() {
            handle.abort();
        }
        self.transport.close().await
    }

    // ── Event handler registration ─────────────────────────────

    /// Register a handler for incoming messages (with optional filter).
    pub fn on_message(
        &mut self,
        filter: Option<BoxFilter>,
        handler: BoxHandler<Message>,
    ) {
        self.handlers.on_message.push((filter, handler));
    }

    /// Register a handler for edited messages.
    pub fn on_message_edit(
        &mut self,
        filter: Option<BoxFilter>,
        handler: BoxHandler<Message>,
    ) {
        self.handlers.on_message_edit.push((filter, handler));
    }

    /// Register a handler for deleted messages.
    pub fn on_message_delete(
        &mut self,
        filter: Option<BoxFilter>,
        handler: BoxHandler<Message>,
    ) {
        self.handlers.on_message_delete.push((filter, handler));
    }

    /// Register a handler for chat updates.
    pub fn on_chat_update(&mut self, handler: BoxHandler<Chat>) {
        self.handlers.on_chat_update.push(handler);
    }

    /// Register a handler for reaction changes.
    pub fn on_reaction_change(&mut self, handler: BoxHandler<ReactionEvent>) {
        self.handlers.on_reaction_change.push(handler);
    }

    /// Register a handler for raw frames.
    pub fn on_raw(&mut self, handler: BoxHandler<Frame>) {
        self.handlers.on_raw.push(handler);
    }

    /// Register a handler called once after connect + sync.
    pub fn on_start(&mut self, handler: BoxHandler<()>) {
        self.handlers.on_start = Some(handler);
    }

    // ── Event loop ─────────────────────────────────────────────

    /// Start the event dispatcher. Incoming notifications will be
    /// routed to registered handlers.
    ///
    /// Call this after `connect()` and handler registration.
    pub fn start_dispatcher(&mut self) {
        let rx = self.transport.subscribe();
        let handlers = Arc::new(std::mem::take(&mut self.handlers));
        self.dispatcher_handle = Some(event::spawn_dispatcher(rx, handlers));
        info!("Event dispatcher started");
    }

    /// Connect, sync, start dispatcher, and run the event loop
    /// with automatic reconnection.
    ///
    /// Mirrors the reconnect loop from `pymax/core.py:283-338`.
    pub async fn start(&mut self) -> Result<()> {
        let reconnect = self.config.reconnect;
        let delay = std::time::Duration::from_secs_f64(self.config.reconnect_delay_secs);

        loop {
            // Connect + handshake + sync.
            match self.connect().await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!(error = %e, "Connection failed");
                    if !reconnect {
                        return Err(e);
                    }
                    tracing::info!(delay_secs = delay.as_secs_f64(), "Reconnecting after failure");
                    tokio::time::sleep(delay).await;
                    continue;
                }
            }

            self.start_dispatcher();
            info!("Client started, waiting for events");

            // Wait until transport disconnects or ctrl-c.
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if !self.transport.is_connected() {
                    tracing::warn!("Connection lost");
                    break;
                }
            }

            // Cleanup.
            let _ = self.close().await;

            if !reconnect {
                info!("Reconnect disabled, exiting");
                break;
            }

            // Reset state for reconnection.
            self.dialogs.clear();
            self.chats.clear();
            self.channels.clear();
            self.contacts.clear();
            self.me = None;
            self.sync_data = None;
            self.transport = if self.user_agent.device_type == "WEB" {
            Transport::new_websocket(&self.config.uri)
        } else {
            Transport::new_socket(API_HOST, API_PORT)
        };

            info!(delay_secs = delay.as_secs_f64(), "Reconnecting");
            tokio::time::sleep(delay).await;
        }

        Ok(())
    }
}
