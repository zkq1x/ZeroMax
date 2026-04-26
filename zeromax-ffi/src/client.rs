use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use zeromax_core::{ClientConfig, MaxClient};

use crate::error::FfiError;
use crate::events::EventListener;
use crate::types::*;

/// UniFFI-exposed client facade.
///
/// Wraps `MaxClient` in `Arc<Mutex>` + owns a tokio `Runtime`
/// so all async operations are bridged to synchronous UniFFI calls.
pub struct ZeroMaxClient {
    rt: tokio::runtime::Runtime,
    inner: Arc<Mutex<MaxClient>>,
    my_id: std::sync::Mutex<Option<i64>>,
    /// Cache of user_id → display_name for enriching messages.
    user_names: std::sync::Mutex<HashMap<i64, String>>,
    /// Cache of user_id → avatar URL for dialog avatars.
    user_avatars: std::sync::Mutex<HashMap<i64, String>>,
    event_listener: std::sync::Mutex<Option<Arc<dyn EventListener>>>,
    event_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ZeroMaxClient {
    pub fn new_client(config: FfiClientConfig) -> Result<Self, FfiError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|_| FfiError::Internal)?;

        let mut core_config = ClientConfig::new(&config.phone)
            .work_dir(&config.work_dir);
        if let Some(t) = config.token {
            core_config = core_config.token(t);
        }
        if let Some(dt) = config.device_type {
            core_config = core_config.device_type(dt);
        }

        let client = rt.block_on(MaxClient::new(core_config))?;

        Ok(Self {
            rt,
            inner: Arc::new(Mutex::new(client)),
            my_id: std::sync::Mutex::new(None),
            user_names: std::sync::Mutex::new(HashMap::new()),
            user_avatars: std::sync::Mutex::new(HashMap::new()),
            event_listener: std::sync::Mutex::new(None),
            event_handle: std::sync::Mutex::new(None),
        })
    }

    pub fn connect(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.connect().await?;
            self.populate_caches_from(&client);
            Ok(())
        })
    }

    /// Connect and handshake only — no sync/login.
    /// Use this before QR auth when there's no token yet.
    /// Connect and handshake only — no sync/login.
    /// Works for both WEB (QR auth) and DESKTOP (phone auth).
    pub fn connect_for_auth(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            // Transport connect (WS or Socket based on device_type).
            let ua_header = client.user_agent.header_user_agent.clone();
            match &mut client.transport {
                zeromax_core::transport::Transport::WebSocket(ws) => {
                    ws.connect(&ua_header).await?;
                }
                zeromax_core::transport::Transport::Socket(s) => {
                    s.connect().await?;
                }
            }
            // Handshake only.
            let ua = client.user_agent_payload();
            let payload = serde_json::json!({
                "deviceId": client.device_id.to_string(),
                "userAgent": ua,
            });
            let resp = client.transport.request(
                zeromax_core::Opcode::SessionInit,
                payload,
            ).await?;
            zeromax_core::error::check_payload(&resp.payload)?;
            eprintln!("[DEBUG] connect_for_auth: handshake OK (device_type={})", client.user_agent.device_type);
            Ok(())
        })
    }

    /// Sync only — for use after QR login when already connected.
    pub fn sync_after_login(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.sync().await?;
            // Populate caches inline (not via separate block_on).
            self.populate_caches_from(&client);
            Ok(())
        })
    }

    fn populate_caches(&self) {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            self.populate_caches_from(&client);
        });
    }

    /// Fetch user info for all dialog participants not yet in cache.
    /// Call after sync to resolve dialog names and avatars.
    pub fn resolve_dialog_users(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let my_id = *self.my_id.lock().unwrap();

            // Collect all participant IDs from dialogs that we don't know yet.
            let known = self.user_names.lock().unwrap().clone();
            let mut missing_ids: Vec<i64> = Vec::new();
            for d in &client.dialogs {
                for (uid_str, _) in &d.participants {
                    if let Ok(uid) = uid_str.parse::<i64>() {
                        if Some(uid) != my_id && !known.contains_key(&uid) && !missing_ids.contains(&uid) {
                            missing_ids.push(uid);
                        }
                    }
                }
            }

            if missing_ids.is_empty() {
                return Ok(());
            }

            eprintln!("[DEBUG] Resolving {} dialog users...", missing_ids.len());
            let users = client.fetch_users(&missing_ids).await.unwrap_or_default();

            let mut names = self.user_names.lock().unwrap();
            let mut avatars = self.user_avatars.lock().unwrap();
            for user in &users {
                let display = user.names.first()
                    .and_then(|n| n.name.clone())
                    .or_else(|| user.names.first().and_then(|n| n.first_name.clone()))
                    .unwrap_or_default();
                names.insert(user.id, display);
                if let Some(url) = &user.base_url {
                    avatars.insert(user.id, url.clone());
                }
            }
            eprintln!("[DEBUG] Resolved {} users, cache now has {} entries", users.len(), names.len());
            Ok(())
        })
    }

    fn populate_caches_from(&self, client: &zeromax_core::MaxClient) {
        let mut names = self.user_names.lock().unwrap();
        let mut avatars = self.user_avatars.lock().unwrap();
        if let Some(me) = &client.me {
            *self.my_id.lock().unwrap() = Some(me.id);
            let display = me.names.first()
                .and_then(|n| n.name.clone())
                .or_else(|| me.names.first().and_then(|n| n.first_name.clone()))
                .unwrap_or_default();
            names.insert(me.id, display);
        }
        for contact in &client.contacts {
            let display = contact.names.first()
                .and_then(|n| n.name.clone())
                .or_else(|| contact.names.first().and_then(|n| n.first_name.clone()))
                .unwrap_or_default();
            names.insert(contact.id, display);
            if let Some(url) = &contact.base_url {
                avatars.insert(contact.id, url.clone());
            }
        }
    }

    /// Resolve a sender_id to a display name from cache, fetching if needed.
    fn resolve_name(&self, user_id: i64) -> String {
        // Check cache first.
        if let Some(name) = self.user_names.lock().unwrap().get(&user_id) {
            return name.clone();
        }

        // Try fetching from server.
        if let Ok(user) = self.get_user(user_id) {
            let name = user.display_name.clone();
            self.user_names.lock().unwrap().insert(user_id, name.clone());
            return name;
        }

        String::new()
    }

    /// Enrich an FfiMessage with sender name from cache.
    fn enrich_message(&self, mut msg: FfiMessage) -> FfiMessage {
        if msg.sender_id != 0 && msg.sender_name.is_empty() {
            msg.sender_name = self.resolve_name(msg.sender_id);
        }
        msg
    }

    pub fn is_connected(&self) -> bool {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.is_connected()
        })
    }

    /// Start the reconnect loop in the background.
    /// This will keep the connection alive and automatically reconnect on disconnect.
    /// Call after `connect()` + setting up event listeners.
    pub fn start_background_reconnect(&self) {
        let inner = self.inner.clone();

        self.rt.spawn(async move {
            let delay = std::time::Duration::from_secs(2);
            loop {
                // Wait until disconnected.
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let client = inner.lock().await;
                    if !client.is_connected() {
                        break;
                    }
                }

                tracing::warn!("Connection lost, attempting reconnect...");
                tokio::time::sleep(delay).await;

                // Reconnect.
                let mut client = inner.lock().await;
                client.reset_for_reconnect();

                match client.connect().await {
                    Ok(()) => {
                        tracing::info!("Reconnected successfully");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Reconnect failed, will retry");
                    }
                }
            }
        });

        info!("Background reconnect loop started");
    }

    // ── Auth ───────────────────────────────────────────────────

    pub fn request_code(&self, phone: String, language: String) -> Result<String, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            Ok(client.request_code(&phone, &language).await?)
        })
    }

    pub fn verify_code(&self, code: String, temp_token: String) -> Result<FfiCodeResult, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let result = client.verify_code(&code, &temp_token).await?;
            Ok(FfiCodeResult::from(result))
        })
    }

    pub fn login_with_code(&self, temp_token: String, code: String) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.login_with_code(&temp_token, &code).await?;
            if let Some(me) = &client.me {
                *self.my_id.lock().unwrap() = Some(me.id);
            }
            Ok(())
        })
    }

    pub fn set_token(&self, token: String) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.set_token(token).await?;
            Ok(())
        })
    }

    pub fn check_2fa_password(
        &self,
        track_id: String,
        password: String,
    ) -> Result<Option<String>, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            Ok(client.check_2fa_password(&track_id, &password).await?)
        })
    }

    pub fn request_qr(&self) -> Result<FfiQrLoginData, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let qr = client.request_qr().await?;
            Ok(FfiQrLoginData::from(qr))
        })
    }

    pub fn poll_qr_status(&self, track_id: String) -> Result<bool, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            Ok(client.poll_qr_status(&track_id).await?)
        })
    }

    pub fn complete_qr_login(&self, track_id: String) -> Result<String, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            Ok(client.complete_qr_login(&track_id).await?)
        })
    }

    // ── Data ───────────────────────────────────────────────────

    pub fn get_me(&self) -> Option<FfiMe> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.me.as_ref().map(FfiMe::from)
        })
    }

    /// Load all chats via paginated fetch_chats.
    pub fn load_all_chats(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            let initial_count = client.chats.len() + client.dialogs.len() + client.channels.len();

            // Fetch more chats in a loop until no new ones come.
            let mut marker: Option<i64> = None;
            for _ in 0..10 {
                let fetched = client.fetch_chats(marker).await?;
                if fetched.is_empty() { break; }
                // Use the last chat's last_event_time as next marker.
                marker = fetched.last().map(|c| c.last_event_time);
            }

            let total = client.chats.len() + client.dialogs.len() + client.channels.len();
            eprintln!("[DEBUG] load_all_chats: {} -> {} total chats", initial_count, total);
            Ok(())
        })
    }

    pub fn get_chat_list(&self) -> Vec<FfiChatItem> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let my_id = *self.my_id.lock().unwrap();
            let names = self.user_names.lock().unwrap().clone();
            let avatars = self.user_avatars.lock().unwrap().clone();
            let mut items: Vec<FfiChatItem> = Vec::new();

            for d in &client.dialogs {
                let mut item = FfiChatItem::from_dialog(d);

                // Saved Messages (dialog with yourself, id=0).
                if d.id == 0 {
                    item.title = "Избранное".to_string();
                    items.push(item);
                    continue;
                }

                // Resolve dialog title and avatar from the other participant.
                if let Some(my) = my_id {
                    for (uid_str, _) in &d.participants {
                        if let Ok(uid) = uid_str.parse::<i64>() {
                            if uid != my {
                                if let Some(name) = names.get(&uid) {
                                    item.title = name.clone();
                                }
                                if let Some(url) = avatars.get(&uid) {
                                    item.avatar_url = Some(url.clone());
                                }
                                break;
                            }
                        }
                    }
                }
                items.push(item);
            }
            for c in &client.chats {
                items.push(FfiChatItem::from_chat(c));
            }
            for c in &client.channels {
                let mut item = FfiChatItem::from_chat(c);
                item.chat_type = FfiChatType::Channel;
                items.push(item);
            }

            // Sort by last event time descending.
            items.sort_by(|a, b| b.last_message_time.cmp(&a.last_message_time));
            items
        })
    }

    // ── Messages ───────────────────────────────────────────────

    pub fn fetch_history(
        &self,
        chat_id: i64,
        from_time: Option<i64>,
        count: i32,
    ) -> Result<Vec<FfiMessage>, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let my_id = *self.my_id.lock().unwrap();
            let names = self.user_names.lock().unwrap().clone();
            let messages = client.fetch_history(chat_id, from_time, 0, count).await?;
            eprintln!("[DEBUG] fetch_history chat_id={} got {} messages", chat_id, messages.len());
            let ffi_msgs: Vec<FfiMessage> = messages.iter()
                .map(|m| {
                    let mut msg = FfiMessage::from_core(m, my_id);
                    // Enrich from cache only — no network calls to avoid nested block_on.
                    if msg.sender_id != 0 && msg.sender_name.is_empty() {
                        if let Some(name) = names.get(&msg.sender_id) {
                            msg.sender_name = name.clone();
                        }
                    }
                    msg
                })
                .collect();
            Ok(ffi_msgs)
        })
    }

    pub fn send_message(
        &self,
        chat_id: i64,
        text: String,
        reply_to: Option<i64>,
    ) -> Result<FfiMessage, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let my_id = *self.my_id.lock().unwrap();
            let names = self.user_names.lock().unwrap().clone();
            let msg = client.send_message(chat_id, &text, reply_to, true).await?;
            let mut ffi_msg = FfiMessage::from_core(&msg, my_id);
            if ffi_msg.sender_id != 0 {
                if let Some(name) = names.get(&ffi_msg.sender_id) {
                    ffi_msg.sender_name = name.clone();
                }
            }
            Ok(ffi_msg)
        })
    }

    pub fn read_message(&self, chat_id: i64, message_id: i64) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.read_message(chat_id, message_id).await?;
            Ok(())
        })
    }

    pub fn send_typing(&self, chat_id: i64) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.send_typing(chat_id).await?;
            Ok(())
        })
    }

    // ── Edit / Delete ───────────────────────────────────────────

    pub fn edit_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: String,
    ) -> Result<FfiMessage, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let my_id = *self.my_id.lock().unwrap();
            let names = self.user_names.lock().unwrap().clone();
            let msg = client.edit_message(chat_id, message_id, &text).await?;
            let mut ffi_msg = FfiMessage::from_core(&msg, my_id);
            if ffi_msg.sender_id != 0 {
                if let Some(name) = names.get(&ffi_msg.sender_id) {
                    ffi_msg.sender_name = name.clone();
                }
            }
            Ok(ffi_msg)
        })
    }

    pub fn delete_message(
        &self,
        chat_id: i64,
        message_ids: Vec<i64>,
        for_me: bool,
    ) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.delete_message(chat_id, &message_ids, for_me).await?;
            Ok(())
        })
    }

    // ── Reactions ──────────────────────────────────────────────

    pub fn add_reaction(
        &self,
        chat_id: i64,
        message_id: String,
        reaction: String,
    ) -> Result<Option<FfiReactionInfo>, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let info = client.add_reaction(chat_id, &message_id, &reaction).await?;
            Ok(info.as_ref().map(FfiReactionInfo::from_core))
        })
    }

    pub fn remove_reaction(
        &self,
        chat_id: i64,
        message_id: String,
    ) -> Result<Option<FfiReactionInfo>, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let info = client.remove_reaction(chat_id, &message_id).await?;
            Ok(info.as_ref().map(FfiReactionInfo::from_core))
        })
    }

    // ── User info ─────────────────────────────────────────────

    pub fn get_user(&self, user_id: i64) -> Result<FfiUser, FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            let user = client.get_user(user_id).await?;
            match user {
                Some(u) => Ok(FfiUser::from_core(&u)),
                None => Err(FfiError::Internal),
            }
        })
    }

    // ── Chat management ───────────────────────────────────────

    pub fn join_chat(&self, link: String) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.join_chat(&link).await?;
            Ok(())
        })
    }

    pub fn leave_chat(&self, chat_id: i64) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let mut client = self.inner.lock().await;
            client.leave_chat(chat_id).await?;
            Ok(())
        })
    }

    // ── Profile ───────────────────────────────────────────────

    pub fn change_profile(
        &self,
        first_name: String,
        last_name: Option<String>,
        description: Option<String>,
    ) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client
                .change_profile(&first_name, last_name.as_deref(), description.as_deref())
                .await?;
            Ok(())
        })
    }

    pub fn server_logout(&self) -> Result<(), FfiError> {
        self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.logout().await?;
            Ok(())
        })
    }

    // ── Events ─────────────────────────────────────────────────

    pub fn set_event_listener(&self, listener: Box<dyn EventListener>) {
        *self.event_listener.lock().unwrap() = Some(Arc::from(listener));
    }

    pub fn start_event_loop(&self) {
        let listener = self.event_listener.lock().unwrap().clone();
        let Some(listener) = listener else {
            info!("No event listener set, skipping event loop");
            return;
        };

        let my_id = *self.my_id.lock().unwrap();
        let rx = self.rt.block_on(async {
            let client = self.inner.lock().await;
            client.subscribe()
        });

        let names_cache = Arc::new(std::sync::Mutex::new(
            self.user_names.lock().unwrap().clone(),
        ));

        let handle = self.rt.spawn(async move {
            crate::events::spawn_event_bridge(rx, listener, my_id, names_cache)
                .await
                .ok();
        });

        *self.event_handle.lock().unwrap() = Some(handle);
        info!("FFI event loop started");
    }
}
