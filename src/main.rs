mod app;
mod config;
mod i18n;
mod ipc;
mod plugin;
mod shortcut;
mod theme;
mod tray;
mod ui;

use adw::prelude::*;
use app::GpotlightApp;

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

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_activate(|gtk_app| {
        if let Err(err) = GpotlightApp::new(gtk_app).and_then(|app| app.start()) {
            tracing::error!(error = ?err, "failed to start application");
        }
    });

    app.run()
}
