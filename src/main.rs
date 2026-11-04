mod app;
mod autostart;
mod config;
mod i18n;
mod ipc;
mod plugin;
mod theme;
mod tray;
mod ui;

use adw::prelude::*;
use app::GpotlightApp;
use std::cell::Cell;

const APP_ID: &str = "io.github.gpotlight.Gpotlight";

fn main() -> glib::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("toggle") {
        ipc::send_toggle_if_running();
        return glib::ExitCode::SUCCESS;
    }

    if std::env::args().nth(1).as_deref() == Some("settings") {
        ipc::send_settings_if_running();
        return glib::ExitCode::SUCCESS;
    }

    if ipc::is_running() {
        return glib::ExitCode::SUCCESS;
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let app = adw::Application::builder().application_id(APP_ID).build();

    let initialized = Cell::new(false);
    app.connect_activate(move |gtk_app| {
        // D-Bus activation can happen more than once in the primary process.
        // Keep its windows, IPC listener and background services singular.
        if initialized.replace(true) {
            return;
        }
        if let Err(err) = GpotlightApp::new(gtk_app).and_then(|app| app.start()) {
            initialized.set(false);
            tracing::error!(error = ?err, "failed to start application");
        }
    });

    app.run()
}
