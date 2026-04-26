#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod auth;
mod chat_list;
mod qr;
mod runtime;

use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, Model, ModelRc, VecModel};
use tokio::runtime::Handle;
use tracing_subscriber::EnvFilter;
use zeromax_core::QrLoginData;

use auth::{AuthController, CodeOutcome, ResumeOutcome};
use chat_list::{ChatListViewModel, ChatRow};

slint::include_modules!();

/// Bag of dependencies passed to callback wiring. Keeps signatures short.
#[derive(Clone)]
struct Wiring {
    rt: Handle,
    auth: Arc<AuthController>,
    chats: Arc<ChatListViewModel>,
    rows: Arc<StdMutex<Vec<ChatRow>>>,
}

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    tracing::info!("ZeroMax desktop starting");

    let rt = runtime::Runtime::new()?;
    let auth = Arc::new(AuthController::new(runtime::data_dir()?));
    let chats = Arc::new(ChatListViewModel::new(auth.client_handle()));
    let rows: Arc<StdMutex<Vec<ChatRow>>> = Arc::new(StdMutex::new(Vec::new()));

    let app = AppWindow::new()?;
    let wiring = Wiring {
        rt: rt.handle(),
        auth: auth.clone(),
        chats: chats.clone(),
        rows: rows.clone(),
    };

    wire_callbacks(&app, &wiring);
    spawn_resume(&app, &wiring);

    app.run()?;
    Ok(())
}

fn wire_callbacks(app: &AppWindow, w: &Wiring) {
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
        let w_for = w.clone();
        app.on_choose_qr(move || {
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let w_for = w_for.clone();
            w_for.rt.clone().spawn(async move {
                match w_for.auth.start_qr().await {
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
                            start_qr_polling(weak, w_for, data);
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
        let w_for = w.clone();
        app.on_submit_phone(move |phone| {
            let phone = phone.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let w_for = w_for.clone();
            w_for.rt.clone().spawn(async move {
                match w_for.auth.start_sms(&phone).await {
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
        let w_for = w.clone();
        app.on_submit_code(move |code| {
            let code = code.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let w_for = w_for.clone();
            w_for.rt.clone().spawn(async move {
                match w_for.auth.submit_code(&code).await {
                    Ok(CodeOutcome::Authed { display_name }) => {
                        finish_login(&weak, &w_for, display_name);
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
        let w_for = w.clone();
        app.on_submit_2fa(move |password| {
            let password = password.to_string();
            ui_busy(&weak, true);
            ui_error(&weak, "");
            let weak = weak.clone();
            let w_for = w_for.clone();
            w_for.rt.clone().spawn(async move {
                let track_id = match w_for.auth.current_2fa_track_id().await {
                    Some(t) => t,
                    None => {
                        show_error(&weak, "Internal: no 2FA track id".into());
                        return;
                    }
                };
                match w_for.auth.submit_2fa(&track_id, &password).await {
                    Ok(name) => finish_login(&weak, &w_for, name),
                    Err(e) => show_error(&weak, format!("2FA failed: {e:#}")),
                }
            });
        });
    }
    {
        let weak = app.as_weak();
        let w_for = w.clone();
        app.on_back_to_choose(move || {
            let weak = weak.clone();
            let auth = w_for.auth.clone();
            w_for.rt.spawn(async move {
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
        let w_for = w.clone();
        app.on_logout(move || {
            let weak = weak.clone();
            let w_for = w_for.clone();
            w_for.rt.clone().spawn(async move {
                let _ = w_for.auth.logout().await;
                w_for.rows.lock().unwrap().clear();
                let _ = weak.upgrade_in_event_loop(|ui| {
                    ui.set_account_name("".into());
                    let empty: Rc<VecModel<ChatRowData>> = Rc::new(VecModel::default());
                    ui.set_chats(ModelRc::from(empty));
                    ui.set_selected_chat_idx(-1);
                    ui.set_screen(Screen::LoginChoose);
                });
            });
        });
    }
    {
        let weak = app.as_weak();
        let rows = w.rows.clone();
        app.on_chat_selected(move |idx| {
            let idx = idx as usize;
            let info = rows.lock().unwrap().get(idx).map(|r| (r.id, r.title.clone()));
            if let Some((id, title)) = info {
                tracing::info!(idx, id, title = %title, "Chat selected");
                if let Some(ui) = weak.upgrade() {
                    ui.set_selected_chat_idx(idx as i32);
                }
            }
        });
    }
}

fn spawn_resume(app: &AppWindow, w: &Wiring) {
    let weak = app.as_weak();
    let w_for = w.clone();
    w.rt.spawn(async move {
        let outcome = w_for.auth.try_resume().await;
        match outcome {
            Ok(ResumeOutcome::Authed { display_name }) => {
                let weak2 = weak.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_account_name(display_name.into());
                    ui.set_screen(Screen::Authed);
                });
                spawn_load_chats(weak2, &w_for);
            }
            Ok(ResumeOutcome::NeedLogin) => {
                let _ = weak.upgrade_in_event_loop(|ui| ui.set_screen(Screen::LoginChoose));
            }
            Err(e) => {
                tracing::warn!(error = %e, "try_resume errored");
                let _ = weak.upgrade_in_event_loop(|ui| ui.set_screen(Screen::LoginChoose));
            }
        }
    });
}

fn start_qr_polling(weak: slint::Weak<AppWindow>, w: Wiring, data: QrLoginData) {
    let interval = Duration::from_millis((data.polling_interval_ms as u64).max(500));
    let until_expire_ms = (data.expires_at_ms - now_ms()).max(0) as u64;
    let deadline = Instant::now() + Duration::from_millis(until_expire_ms);
    let track_id = data.track_id.clone();

    let w_for_task = w.clone();
    let weak_for_task = weak.clone();
    let handle = w.rt.spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if Instant::now() >= deadline {
                let _ = weak_for_task.upgrade_in_event_loop(|ui| {
                    ui.set_qr_status("QR code expired. Press Back and try again.".into());
                });
                return;
            }
            match w_for_task.auth.poll_qr_once(&track_id).await {
                Ok(false) => continue,
                Ok(true) => {
                    let _ = weak_for_task.upgrade_in_event_loop(|ui| {
                        ui.set_qr_status("Scanned — finishing login…".into());
                    });
                    match w_for_task.auth.complete_qr(&track_id).await {
                        Ok(name) => finish_login(&weak_for_task, &w_for_task, name),
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

    let auth_for_store = w.auth.clone();
    w.rt.spawn(async move {
        auth_for_store.set_qr_task(handle).await;
    });
}

fn finish_login(weak: &slint::Weak<AppWindow>, w: &Wiring, display_name: String) {
    let weak2 = weak.clone();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_account_name(display_name.into());
        ui.set_busy(false);
        ui.set_error_message("".into());
        ui.set_screen(Screen::Authed);
    });
    spawn_load_chats(weak2, w);
}

fn spawn_load_chats(weak: slint::Weak<AppWindow>, w: &Wiring) {
    let chats_vm = w.chats.clone();
    let rows_store = w.rows.clone();
    w.rt.spawn(async move {
        match chats_vm.load().await {
            Ok(rows) => {
                let model_data: Vec<ChatRowData> = rows.iter().map(to_slint_row).collect();
                *rows_store.lock().unwrap() = rows;
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let model = Rc::new(VecModel::from(model_data));
                    let len = model.row_count() as i32;
                    ui.set_chats(ModelRc::from(model));
                    ui.set_selected_chat_idx(-1);
                    tracing::info!(count = len, "Chat list loaded");
                });
            }
            Err(e) => tracing::warn!(error = %e, "Failed to load chat list"),
        }
    });
}

fn to_slint_row(r: &ChatRow) -> ChatRowData {
    let (cr, cg, cb) = r.avatar_color;
    ChatRowData {
        id_text: r.id.to_string().into(),
        title: r.title.clone().into(),
        preview: r.preview.clone().into(),
        time: r.time_label.clone().into(),
        initial: r.initial.clone().into(),
        avatar_bg: slint::Color::from_rgb_u8(cr, cg, cb),
    }
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
