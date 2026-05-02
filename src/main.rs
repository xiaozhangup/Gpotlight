mod app;
mod config;
mod desktop_entry;
mod i18n;
mod ipc;
mod plugin;
mod shortcut;
mod theme;
mod tray;
mod ui;

use app::GpotlightApp;
use desktop_entry::APP_ID;
use gtk::prelude::*;

fn main() -> glib::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("toggle") {
        ipc::send_toggle_if_running();
        return glib::ExitCode::SUCCESS;
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let app = gtk::Application::builder().application_id(APP_ID).build();

    app.connect_activate(|gtk_app| {
        if let Err(err) = GpotlightApp::new(gtk_app).and_then(|app| app.start()) {
            tracing::error!(error = ?err, "failed to start application");
        }
    });

    app.run()
}
