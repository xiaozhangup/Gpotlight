use crate::config::ConfigStore;
use crate::i18n::I18n;
use crate::ipc;
use crate::plugin::{builtin::register_builtin_plugins, PluginRegistry};
use crate::tray::TrayManager;
use crate::ui::{SettingsWindow, SpotlightWindow};
use adw::prelude::*;
use anyhow::Result;
use gio::ApplicationHoldGuard;
use std::cell::RefCell;
use std::rc::Rc;

pub struct GpotlightApp<'a> {
    gtk_app: &'a adw::Application,
    config: Rc<RefCell<ConfigStore>>,
    i18n: Rc<I18n>,
    plugins: Rc<RefCell<PluginRegistry>>,
}

#[allow(dead_code)]
struct RuntimeHold(ApplicationHoldGuard);

impl<'a> GpotlightApp<'a> {
    pub fn new(gtk_app: &'a adw::Application) -> Result<Self> {
        let config = Rc::new(RefCell::new(ConfigStore::load()?));
        let locale = config.borrow().current().locale.clone();
        let i18n = Rc::new(I18n::load(&locale));

        // Start with an empty registry; plugins are loaded lazily after the window
        // is set up so the UI is responsive from the very first toggle.
        let plugins = Rc::new(RefCell::new(PluginRegistry::default()));

        Ok(Self {
            gtk_app,
            config,
            i18n,
            plugins,
        })
    }

    pub fn start(self) -> Result<()> {
        Box::leak(Box::new(RuntimeHold(self.gtk_app.hold())));

        // Kick off plugin loading in the next idle cycle so that all window
        // setup and IPC registration finish first.  This keeps startup snappy
        // and means the shortcut is already registered when the user first
        // presses it.
        {
            let plugins = self.plugins.clone();
            let config = self.config.clone();
            glib::idle_add_local_once(move || {
                register_builtin_plugins(
                    &mut plugins.borrow_mut(),
                    config.borrow().current(),
                );
            });
        }

        let spotlight = Rc::new(SpotlightWindow::new(
            self.gtk_app,
            self.i18n.clone(),
            self.config.clone(),
            self.plugins.clone(),
        ));

        let toggle_action = gio::SimpleAction::new("toggle", None);
        {
            let spotlight = spotlight.clone();
            toggle_action.connect_activate(move |_, _| spotlight.toggle());
        }
        self.gtk_app.add_action(&toggle_action);

        let quit_action = gio::SimpleAction::new("quit", None);
        {
            let gtk_app = self.gtk_app.clone();
            quit_action.connect_activate(move |_, _| gtk_app.quit());
        }
        self.gtk_app.add_action(&quit_action);

        let tray_manager = Rc::new(TrayManager::new(
            self.i18n.clone(),
            {
                let app = self.gtk_app.clone();
                move || app.activate_action("toggle", None)
            },
            {
                let app = self.gtk_app.clone();
                move || app.activate_action("settings", None)
            },
            {
                let app = self.gtk_app.clone();
                move || app.activate_action("quit", None)
            },
        ));

        let settings = Rc::new(SettingsWindow::new(
            self.gtk_app,
            self.i18n.clone(),
            self.config.clone(),
            self.plugins.clone(),
            spotlight.clone(),
            tray_manager.clone(),
        ));

        let settings_action = gio::SimpleAction::new("settings", None);
        {
            let settings = settings.clone();
            settings_action.connect_activate(move |_, _| settings.present());
        }
        self.gtk_app.add_action(&settings_action);

        if let Err(err) = ipc::spawn_toggle_server(
            {
                let app = self.gtk_app.clone();
                move || app.activate_action("toggle", None)
            },
            {
                let app = self.gtk_app.clone();
                move || app.activate_action("settings", None)
            },
        ) {
            tracing::warn!(error = ?err, "failed to start IPC toggle server");
        }

        tray_manager.set_enabled(self.config.borrow().current().tray_enabled);

        spotlight.prime();
        Ok(())
    }
}
