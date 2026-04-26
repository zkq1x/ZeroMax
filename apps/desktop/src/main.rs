#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod runtime;

use slint::ComponentHandle;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    tracing::info!("ZeroMax desktop starting");

    let rt = runtime::Runtime::new()?;
    let app = AppWindow::new()?;

    wire_connect_button(&app, rt.handle());

    app.run()?;
    Ok(())
}

fn wire_connect_button(app: &AppWindow, rt: tokio::runtime::Handle) {
    let weak = app.as_weak();
    app.on_connect_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_busy(true);
            ui.set_status("Connecting…".into());
        }

        let weak_for_task = weak.clone();
        rt.spawn(async move {
            let outcome = runtime::test_connect().await;
            let msg = match outcome {
                Ok(s) => s,
                Err(e) => format!("Failed: {e:#}"),
            };
            tracing::info!(result = %msg, "test_connect finished");
            let _ = weak_for_task.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                ui.set_status(msg.into());
            });
        });
    });
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
