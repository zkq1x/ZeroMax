use serde_json::json;
use tracing::{debug, info};

use crate::client::MaxClient;
use crate::error::{self, Error, Result};
use crate::protocol::Opcode;
use crate::storage::{AuthData, SessionStorage};

/// Result of a code verification attempt.
pub enum CodeResult {
    /// Login succeeded, token obtained.
    LoggedIn { token: String },
    /// 2FA is required — use `two_factor_auth` with the returned challenge.
    TwoFactorRequired { track_id: String, hint: Option<String> },
}

/// QR code login data returned by `request_qr`.
pub struct QrLoginData {
    pub qr_link: String,
    pub track_id: String,
    pub polling_interval_ms: u64,
    pub expires_at_ms: i64,
}

impl MaxClient {
    // ── Phone + Code flow ──────────────────────────────────────────

    /// Request an auth code to be sent to the given phone number.
    /// Returns a temporary token for `verify_code`.
    ///
    /// Mirrors `AuthMixin.request_code()` from `pymax/mixins/auth.py`.
    pub async fn request_code(&self, phone: &str, language: &str) -> Result<String> {
        info!("Requesting auth code");

        let payload = json!({
            "phone": phone,
            "type": "START_AUTH",
            "language": language,
        });

        let response = self.transport.request(Opcode::AuthRequest, payload).await?;
        eprintln!("[DEBUG] AuthRequest response: {}", serde_json::to_string_pretty(&response.payload).unwrap_or_default());
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::UnexpectedResponse("Missing token in auth response".into()))
    }

    /// Resend the auth code.
    pub async fn resend_code(&self, phone: &str, language: &str) -> Result<String> {
        info!("Resending auth code");

        let payload = json!({
            "phone": phone,
            "type": "RESEND",
            "language": language,
        });

        let response = self.transport.request(Opcode::AuthRequest, payload).await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::UnexpectedResponse("Missing token in resend response".into()))
    }

    /// Verify the SMS code. Returns either a token or a 2FA challenge.
    ///
    /// Mirrors `AuthMixin._send_code()` from `pymax/mixins/auth.py`.
    pub async fn verify_code(&self, code: &str, temp_token: &str) -> Result<CodeResult> {
        info!("Verifying auth code");

        let payload = json!({
            "token": temp_token,
            "verifyCode": code,
            "authTokenType": "CHECK_CODE",
        });

        let response = self.transport.request(Opcode::Auth, payload).await?;
        error::check_payload(&response.payload)?;

        // Check for 2FA challenge.
        if let Some(challenge) = response.payload.get("passwordChallenge") {
            let track_id = challenge
                .get("trackId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let hint = challenge
                .get("hint")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            return Ok(CodeResult::TwoFactorRequired { track_id, hint });
        }

        // Extract login token.
        let token = response
            .payload
            .get("tokenAttrs")
            .and_then(|v| v.get("LOGIN"))
            .and_then(|v| v.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Auth("No token in code verification response".into()))?;

        Ok(CodeResult::LoggedIn { token })
    }

    // ── QR Code flow ───────────────────────────────────────────────

    /// Request a QR code for login.
    ///
    /// Mirrors `AuthMixin._request_qr_login()`.
    pub async fn request_qr(&self) -> Result<QrLoginData> {
        info!("Requesting QR login");

        let response = self
            .transport
            .request(Opcode::GetQr, json!({}))
            .await?;
        error::check_payload(&response.payload)?;

        let p = &response.payload;
        Ok(QrLoginData {
            qr_link: p
                .get("qrLink")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            track_id: p
                .get("trackId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            polling_interval_ms: p
                .get("pollingInterval")
                .and_then(|v| v.as_u64())
                .unwrap_or(2000),
            expires_at_ms: p
                .get("expiresAt")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
        })
    }

    /// Poll QR login status. Returns `true` when the user has scanned.
    ///
    /// Mirrors `AuthMixin._poll_qr_login()`.
    pub async fn poll_qr_status(&self, track_id: &str) -> Result<bool> {
        let response = self
            .transport
            .request(Opcode::GetQrStatus, json!({"trackId": track_id}))
            .await?;
        error::check_payload(&response.payload)?;

        let login_available = response
            .payload
            .get("status")
            .and_then(|v| v.get("loginAvailable"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(login_available)
    }

    /// Complete QR login after the user has scanned.
    ///
    /// Mirrors `AuthMixin._get_qr_login_data()`.
    pub async fn complete_qr_login(&self, track_id: &str) -> Result<String> {
        info!("Completing QR login");

        let response = self
            .transport
            .request(Opcode::LoginByQr, json!({"trackId": track_id}))
            .await?;
        error::check_payload(&response.payload)?;

        response
            .payload
            .get("tokenAttrs")
            .and_then(|v| v.get("LOGIN"))
            .and_then(|v| v.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Auth("No token in QR login response".into()))
    }

    // ── 2FA ────────────────────────────────────────────────────────

    /// Submit a 2FA password. Returns the login token on success.
    ///
    /// Mirrors `AuthMixin._check_password()`.
    pub async fn check_2fa_password(
        &self,
        track_id: &str,
        password: &str,
    ) -> Result<Option<String>> {
        let payload = json!({
            "trackId": track_id,
            "password": password,
        });

        let response = self
            .transport
            .request(Opcode::AuthLoginCheckPassword, payload)
            .await?;

        if response.payload.get("error").is_some() {
            return Ok(None); // Wrong password.
        }

        let token = response
            .payload
            .get("tokenAttrs")
            .and_then(|v| v.get("LOGIN"))
            .and_then(|v| v.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(token)
    }

    // ── Token management ───────────────────────────────────────────

    /// Set the auth token and persist it.
    pub async fn set_token(&mut self, token: String) -> Result<()> {
        self.token = Some(token.clone());
        self.storage
            .save_auth(&AuthData {
                device_id: self.device_id,
                token,
            })
            .await?;
        debug!("Token saved");
        Ok(())
    }

    /// Full auth flow: request code → verify → handle 2FA → sync.
    /// `get_code` is a callback that provides the verification code.
    /// `get_password` is a callback for 2FA password (if needed).
    pub async fn login_with_code(
        &mut self,
        temp_token: &str,
        code: &str,
    ) -> Result<()> {
        let result = self.verify_code(code, temp_token).await?;

        let token = match result {
            CodeResult::LoggedIn { token } => token,
            CodeResult::TwoFactorRequired { .. } => {
                return Err(Error::Auth("2FA required — use check_2fa_password()".into()));
            }
        };

        self.set_token(token).await?;
        self.sync().await?;
        info!("Login with code completed");
        Ok(())
    }
}
