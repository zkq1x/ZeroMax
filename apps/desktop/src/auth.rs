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

/// Owns the `MaxClient` across the login → authed lifecycle.
///
/// SMS flow uses DESKTOP transport (raw socket), QR uses WEB transport (WebSocket).
/// After login completes, the same client stays for follow-up operations.
pub struct AuthController {
    work_dir: PathBuf,
    inner: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    client: Option<MaxClient>,
    temp_token: Option<String>,
    two_fa_track_id: Option<String>,
    qr_track_id: Option<String>,
    qr_task: Option<JoinHandle<()>>,
}

impl AuthController {
    pub fn new(work_dir: PathBuf) -> Self {
        Self {
            work_dir,
            inner: Arc::new(Mutex::new(State::default())),
        }
    }

    /// Try to resume from disk-stored token.
    ///
    /// Skips network entirely if no token is on disk — first-launch is instant.
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
                self.inner.lock().await.client = Some(client);
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

        // No token yet ⇒ connect() does handshake-only (skips sync).
        client.connect().await.context("Handshake failed")?;
        let temp_token = client.request_code(phone, SMS_LANG).await?;

        let mut state = self.inner.lock().await;
        state.client = Some(client);
        state.temp_token = Some(temp_token);
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<CodeOutcome> {
        let mut state = self.inner.lock().await;
        let temp_token = state
            .temp_token
            .clone()
            .ok_or_else(|| anyhow!("No temp_token"))?;
        let client = state
            .client
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
                state.two_fa_track_id = Some(track_id.clone());
                Ok(CodeOutcome::TwoFactorRequired { track_id, hint })
            }
        }
    }

    pub async fn current_2fa_track_id(&self) -> Option<String> {
        self.inner.lock().await.two_fa_track_id.clone()
    }

    pub async fn submit_2fa(&self, track_id: &str, password: &str) -> Result<String> {
        let mut state = self.inner.lock().await;
        let client = state
            .client
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

        let mut state = self.inner.lock().await;
        state.client = Some(client);
        state.qr_track_id = Some(qr.track_id.clone());
        Ok(qr)
    }

    pub async fn poll_qr_once(&self, track_id: &str) -> Result<bool> {
        let mut state = self.inner.lock().await;
        let client = state
            .client
            .as_mut()
            .ok_or_else(|| anyhow!("No active client for QR poll"))?;
        Ok(client.poll_qr_status(track_id).await?)
    }

    pub async fn complete_qr(&self, track_id: &str) -> Result<String> {
        let mut state = self.inner.lock().await;
        let client = state
            .client
            .as_mut()
            .ok_or_else(|| anyhow!("No active client"))?;

        let token = client.complete_qr_login(track_id).await?;
        client.set_token(token).await?;
        client.sync().await?;
        Ok(display_name(client))
    }

    pub async fn set_qr_task(&self, handle: JoinHandle<()>) {
        let mut state = self.inner.lock().await;
        if let Some(prev) = state.qr_task.replace(handle) {
            prev.abort();
        }
    }

    /// Drop client + abort any running QR task. Used on Back / before starting a new flow.
    pub async fn cancel_flow(&self) {
        let mut state = self.inner.lock().await;
        if let Some(task) = state.qr_task.take() {
            task.abort();
        }
        if let Some(mut client) = state.client.take() {
            let _ = client.close().await;
        }
        state.temp_token = None;
        state.two_fa_track_id = None;
        state.qr_track_id = None;
    }

    pub async fn logout(&self) -> Result<()> {
        let mut state = self.inner.lock().await;
        if let Some(task) = state.qr_task.take() {
            task.abort();
        }
        if let Some(mut client) = state.client.take() {
            let _ = client.close().await;
        }
        state.temp_token = None;
        state.two_fa_track_id = None;
        state.qr_track_id = None;
        drop(state);

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
