#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod auth;
mod qr;
mod runtime;

use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use tokio::runtime::Handle;
use tracing_subscriber::EnvFilter;
use zeromax_core::QrLoginData;

use auth::{AuthController, CodeOutcome, ResumeOutcome};

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    tracing::info!("ZeroMax desktop starting");

    let rt = runtime::Runtime::new()?;
    let auth = Arc::new(AuthController::new(runtime::data_dir()?));
    let app = AppWindow::new()?;

    wire_callbacks(&app, rt.handle(), auth.clone());
    spawn_resume(&app, rt.handle(), auth);

    app.run()?;
    Ok(())
}

fn wire_callbacks(app: &AppWindow, rt: Handle, auth: Arc<AuthController>) {
    {
        let weak = app.as_weak();
        app.on_choose_phone(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_screen(Screen::LoginPhone);
                ui.set_error_message("".into());
            }
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt2 = rt.clone();
        app.on_choose_qr(move || {
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let auth = auth.clone();
            let rt2 = rt2.clone();
            rt2.clone().spawn(async move {
                match auth.start_qr().await {
                    Ok(data) => match qr::render_buffer(&data.qr_link, 8) {
                        Ok(buf) => {
                            let weak_for_ui = weak.clone();
                            let _ = weak_for_ui.upgrade_in_event_loop(move |ui| {
                                ui.set_qr_image(slint::Image::from_rgba8(buf));
                                ui.set_qr_status("Waiting for scan…".into());
                                ui.set_error_message("".into());
                                ui.set_busy(false);
                                ui.set_screen(Screen::LoginQr);
                            });
                            start_qr_polling(weak, rt2, auth, data);
                        }
                        Err(e) => show_error(&weak, format!("QR render failed: {e:#}")),
                    },
                    Err(e) => show_error(&weak, format!("Failed to start QR: {e:#}")),
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt = rt.clone();
        app.on_submit_phone(move |phone| {
            let phone = phone.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let auth = auth.clone();
            rt.spawn(async move {
                match auth.start_sms(&phone).await {
                    Ok(()) => {
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_phone(phone.into());
                            ui.set_busy(false);
                            ui.set_error_message("".into());
                            ui.set_screen(Screen::LoginCode);
                        });
                    }
                    Err(e) => show_error(&weak, format!("Failed to send code: {e:#}")),
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt = rt.clone();
        app.on_submit_code(move |code| {
            let code = code.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let auth = auth.clone();
            rt.spawn(async move {
                match auth.submit_code(&code).await {
                    Ok(CodeOutcome::Authed { display_name }) => {
                        finish_login(&weak, display_name);
                    }
                    Ok(CodeOutcome::TwoFactorRequired { hint, .. }) => {
                        let hint_str = hint.unwrap_or_default();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_two_fa_hint(hint_str.into());
                            ui.set_busy(false);
                            ui.set_error_message("".into());
                            ui.set_screen(Screen::Login2fa);
                        });
                    }
                    Err(e) => show_error(&weak, format!("Verification failed: {e:#}")),
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt = rt.clone();
        app.on_submit_2fa(move |password| {
            let password = password.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let auth = auth.clone();
            rt.spawn(async move {
                // verify_code earlier returned a TwoFactorRequired with track_id; we don't
                // currently round-trip it through the UI. The controller has captured it
                // internally — but our current API requires the caller to pass it.
                // For MVP we re-stash it on the controller side; here we re-pull from
                // last verify result via a follow-up: ask controller for the current
                // track_id. Add accessor.
                let track_id = match auth.current_2fa_track_id().await {
                    Some(t) => t,
                    None => {
                        show_error(&weak, "Internal: no 2FA track id".into());
                        return;
                    }
                };
                match auth.submit_2fa(&track_id, &password).await {
                    Ok(name) => finish_login(&weak, name),
                    Err(e) => show_error(&weak, format!("2FA failed: {e:#}")),
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt = rt.clone();
        app.on_back_to_choose(move || {
            let weak = weak.clone();
            let auth = auth.clone();
            rt.spawn(async move {
                auth.cancel_flow().await;
                let _ = weak.upgrade_in_event_loop(|ui| {
                    ui.set_busy(false);
                    ui.set_error_message("".into());
                    ui.set_screen(Screen::LoginChoose);
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        let auth = auth.clone();
        let rt = rt.clone();
        app.on_logout(move || {
            let weak = weak.clone();
            let auth = auth.clone();
            rt.spawn(async move {
                let _ = auth.logout().await;
                let _ = weak.upgrade_in_event_loop(|ui| {
                    ui.set_account_name("".into());
                    ui.set_screen(Screen::LoginChoose);
                });
            });
        });
    }
}

fn spawn_resume(app: &AppWindow, rt: Handle, auth: Arc<AuthController>) {
    let weak = app.as_weak();
    rt.spawn(async move {
        let outcome = auth.try_resume().await;
        let next = match outcome {
            Ok(ResumeOutcome::Authed { display_name }) => Some(display_name),
            Ok(ResumeOutcome::NeedLogin) => None,
            Err(e) => {
                tracing::warn!(error = %e, "try_resume errored");
                None
            }
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            match next {
                Some(name) => {
                    ui.set_account_name(name.into());
                    ui.set_screen(Screen::Authed);
                }
                None => ui.set_screen(Screen::LoginChoose),
            }
        });
    });
}

fn start_qr_polling(
    weak: slint::Weak<AppWindow>,
    rt: Handle,
    auth: Arc<AuthController>,
    data: QrLoginData,
) {
    let interval = Duration::from_millis((data.polling_interval_ms as u64).max(500));
    let until_expire_ms = (data.expires_at_ms - now_ms()).max(0) as u64;
    let deadline = Instant::now() + Duration::from_millis(until_expire_ms);
    let track_id = data.track_id.clone();

    let auth_for_task = auth.clone();
    let weak_for_task = weak.clone();
    let handle = rt.spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if Instant::now() >= deadline {
                let _ = weak_for_task.upgrade_in_event_loop(|ui| {
                    ui.set_qr_status("QR code expired. Press Back and try again.".into());
                });
                return;
            }
            match auth_for_task.poll_qr_once(&track_id).await {
                Ok(false) => continue,
                Ok(true) => {
                    let _ = weak_for_task.upgrade_in_event_loop(|ui| {
                        ui.set_qr_status("Scanned — finishing login…".into());
                    });
                    match auth_for_task.complete_qr(&track_id).await {
                        Ok(name) => {
                            let _ = weak_for_task.upgrade_in_event_loop(move |ui| {
                                ui.set_account_name(name.into());
                                ui.set_screen(Screen::Authed);
                            });
                        }
                        Err(e) => {
                            let _ = weak_for_task.upgrade_in_event_loop(move |ui| {
                                ui.set_error_message(format!("Login failed: {e:#}").into());
                            });
                        }
                    }
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "QR poll error");
                    let _ = weak_for_task.upgrade_in_event_loop(move |ui| {
                        ui.set_error_message(format!("Polling error: {e:#}").into());
                    });
                    return;
                }
            }
        }
    });

    let auth_for_store = auth;
    rt.spawn(async move {
        auth_for_store.set_qr_task(handle).await;
    });
}

fn finish_login(weak: &slint::Weak<AppWindow>, display_name: String) {
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_account_name(display_name.into());
        ui.set_busy(false);
        ui.set_error_message("".into());
        ui.set_screen(Screen::Authed);
    });
}

fn show_error(weak: &slint::Weak<AppWindow>, msg: String) {
    tracing::warn!(error = %msg, "Showing error to user");
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_busy(false);
        ui.set_error_message(msg.into());
    });
}

fn ui_busy(weak: &slint::Weak<AppWindow>, busy: bool) {
    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_busy(busy));
}

fn ui_error(weak: &slint::Weak<AppWindow>, msg: &str) {
    let msg = msg.to_string();
    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_error_message(msg.into()));
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn init_tracing() -> anyhow::Result<()> {
    let log_dir = runtime::data_dir()?;
    let log_path = log_dir.join("desktop.log");
    let appender = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,zeromax_core=debug,zeromax_desktop=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(appender)
        .with_ansi(false)
        .init();

    eprintln!("ZeroMax desktop log: {}", log_path.display());
    Ok(())
}
