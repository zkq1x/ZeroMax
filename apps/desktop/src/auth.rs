use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use zeromax_core::{ClientConfig, CodeResult, MaxClient, QrLoginData};

/// Placeholder phone for flows that don't use the user's phone (QR, resume).
///
/// `MaxClient::new` regex-validates the phone, but for QR/resume the value
/// itself is unused at the protocol level — it's just config plumbing.
const PLACEHOLDER_PHONE: &str = "+79991234567";
const SMS_LANG: &str = "ru";

/// Shared handle to the connected client. Held by both the auth controller and
/// post-login view models (chat list, conversation, …).
pub type ClientHandle = Arc<Mutex<Option<MaxClient>>>;

/// Outcome of a resume attempt at app startup.
pub enum ResumeOutcome {
    Authed { display_name: String },
    NeedLogin,
}

/// Outcome of submitting an SMS code.
#[allow(dead_code)]
pub enum CodeOutcome {
    Authed { display_name: String },
    TwoFactorRequired { track_id: String, hint: Option<String> },
}

/// Owns the `MaxClient` across the login → authed lifecycle, plus auth-flow
/// scratch state (temp tokens, QR polling task).
///
/// SMS flow uses DESKTOP transport (raw socket), QR uses WEB transport (WebSocket).
/// The same client stays around for follow-up operations (chat list, etc.) and
/// is reachable by other modules through `client_handle()`.
pub struct AuthController {
    work_dir: PathBuf,
    client: ClientHandle,
    flow: Arc<Mutex<FlowState>>,
}

#[derive(Default)]
struct FlowState {
    temp_token: Option<String>,
    two_fa_track_id: Option<String>,
    qr_track_id: Option<String>,
    qr_task: Option<JoinHandle<()>>,
}

impl AuthController {
    pub fn new(work_dir: PathBuf) -> Self {
        Self {
            work_dir,
            client: Arc::new(Mutex::new(None)),
            flow: Arc::new(Mutex::new(FlowState::default())),
        }
    }

    /// Get a clone of the shared client handle for use by other view models.
    pub fn client_handle(&self) -> ClientHandle {
        self.client.clone()
    }

    /// Try to resume from disk-stored token. Skips network if no token on disk.
    pub async fn try_resume(&self) -> Result<ResumeOutcome> {
        let config = ClientConfig::new(PLACEHOLDER_PHONE)
            .device_type("WEB")
            .work_dir(self.work_dir.clone());

        let mut client = MaxClient::new(config)
            .await
            .context("Opening session storage failed")?;

        if !client.has_token() {
            return Ok(ResumeOutcome::NeedLogin);
        }

        match client.connect().await {
            Ok(()) if client.me.is_some() => {
                let name = display_name(&client);
                *self.client.lock().await = Some(client);
                Ok(ResumeOutcome::Authed { display_name: name })
            }
            Ok(()) => Ok(ResumeOutcome::NeedLogin),
            Err(e) => {
                tracing::warn!(error = %e, "Resume failed; user will re-login");
                Ok(ResumeOutcome::NeedLogin)
            }
        }
    }

    /// Begin SMS code flow: build DESKTOP client, handshake, request code.
    pub async fn start_sms(&self, phone: &str) -> Result<()> {
        self.cancel_flow().await;
        self.wipe_session();

        let config = ClientConfig::new(phone)
            .device_type("DESKTOP")
            .work_dir(self.work_dir.clone());
        let mut client = MaxClient::new(config).await?;

        client.connect().await.context("Handshake failed")?;
        let temp_token = client.request_code(phone, SMS_LANG).await?;

        *self.client.lock().await = Some(client);
        self.flow.lock().await.temp_token = Some(temp_token);
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<CodeOutcome> {
        let temp_token = self
            .flow
            .lock()
            .await
            .temp_token
            .clone()
            .ok_or_else(|| anyhow!("No temp_token"))?;

        let mut client_guard = self.client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No active client"))?;

        match client.verify_code(code, &temp_token).await? {
            CodeResult::LoggedIn { token } => {
                client.set_token(token).await?;
                client.sync().await?;
                Ok(CodeOutcome::Authed {
                    display_name: display_name(client),
                })
            }
            CodeResult::TwoFactorRequired { track_id, hint } => {
                drop(client_guard);
                self.flow.lock().await.two_fa_track_id = Some(track_id.clone());
                Ok(CodeOutcome::TwoFactorRequired { track_id, hint })
            }
        }
    }

    pub async fn current_2fa_track_id(&self) -> Option<String> {
        self.flow.lock().await.two_fa_track_id.clone()
    }

    pub async fn submit_2fa(&self, track_id: &str, password: &str) -> Result<String> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No active client"))?;

        let token = client
            .check_2fa_password(track_id, password)
            .await?
            .ok_or_else(|| anyhow!("2FA password incorrect"))?;
        client.set_token(token).await?;
        client.sync().await?;
        Ok(display_name(client))
    }

    /// Begin QR flow: build WEB client, handshake, request QR data.
    pub async fn start_qr(&self) -> Result<QrLoginData> {
        self.cancel_flow().await;
        self.wipe_session();

        let config = ClientConfig::new(PLACEHOLDER_PHONE)
            .device_type("WEB")
            .work_dir(self.work_dir.clone());
        let mut client = MaxClient::new(config).await?;

        client.connect().await.context("Handshake failed")?;
        let qr = client.request_qr().await?;

        *self.client.lock().await = Some(client);
        self.flow.lock().await.qr_track_id = Some(qr.track_id.clone());
        Ok(qr)
    }

    pub async fn poll_qr_once(&self, track_id: &str) -> Result<bool> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No active client for QR poll"))?;
        Ok(client.poll_qr_status(track_id).await?)
    }

    pub async fn complete_qr(&self, track_id: &str) -> Result<String> {
        let mut client_guard = self.client.lock().await;
        let client = client_guard
            .as_mut()
            .ok_or_else(|| anyhow!("No active client"))?;

        let token = client.complete_qr_login(track_id).await?;
        client.set_token(token).await?;
        client.sync().await?;
        Ok(display_name(client))
    }

    pub async fn set_qr_task(&self, handle: JoinHandle<()>) {
        if let Some(prev) = self.flow.lock().await.qr_task.replace(handle) {
            prev.abort();
        }
    }

    /// Drop client + abort any running QR task. Used on Back / before starting a new flow.
    pub async fn cancel_flow(&self) {
        if let Some(task) = self.flow.lock().await.qr_task.take() {
            task.abort();
        }
        if let Some(mut client) = self.client.lock().await.take() {
            let _ = client.close().await;
        }
        let mut flow = self.flow.lock().await;
        flow.temp_token = None;
        flow.two_fa_track_id = None;
        flow.qr_track_id = None;
    }

    pub async fn logout(&self) -> Result<()> {
        if let Some(task) = self.flow.lock().await.qr_task.take() {
            task.abort();
        }
        if let Some(mut client) = self.client.lock().await.take() {
            let _ = client.close().await;
        }
        let mut flow = self.flow.lock().await;
        flow.temp_token = None;
        flow.two_fa_track_id = None;
        flow.qr_track_id = None;
        drop(flow);

        self.wipe_session();
        Ok(())
    }

    fn wipe_session(&self) {
        let db = self.work_dir.join("session.db");
        if db.exists() {
            if let Err(e) = std::fs::remove_file(&db) {
                tracing::warn!(error = %e, "Failed to remove session.db");
            }
        }
    }
}

fn display_name(client: &MaxClient) -> String {
    let Some(me) = &client.me else {
        return "(unknown)".to_string();
    };
    for n in &me.names {
        if let Some(s) = n.name.as_deref() {
            if !s.is_empty() {
                return s.to_string();
            }
        }
        let first = n.first_name.as_deref().unwrap_or("");
        let last = n.last_name.as_deref().unwrap_or("");
        let combined = format!("{first} {last}").trim().to_string();
        if !combined.is_empty() {
            return combined;
        }
    }
    me.phone.clone()
}
