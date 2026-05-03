use crate::config::{ConfigStore, PluginConfig};
use crate::i18n::I18n;
use crate::plugin::SharedRegistry;
use crate::shortcut::GlobalShortcutManager;
use crate::theme;
use crate::tray::TrayManager;
use crate::ui::SpotlightWindow;
use adw::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SettingsWindow {
    window: adw::ApplicationWindow,
}

impl SettingsWindow {
    pub fn new(
        app: &adw::Application,
        i18n: Rc<I18n>,
        config: Rc<RefCell<ConfigStore>>,
        plugins: SharedRegistry,
        spotlight: Rc<SpotlightWindow>,
        shortcut_manager: Rc<GlobalShortcutManager>,
        tray_manager: Rc<TrayManager>,
    ) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(i18n.t("settings"))
            .default_width(560)
            .default_height(460)
            .build();
        window.set_hide_on_close(true);
        window.set_resizable(true);
        theme::apply_to_window(&window);

        let title = i18n.t("settings");
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new(&title, ""))
            .show_start_title_buttons(true)
            .show_end_title_buttons(true)
            .build();
        toolbar.add_top_bar(&header);

        let page = adw::PreferencesPage::new();

        let shortcut_group = adw::PreferencesGroup::builder()
            .title(i18n.t("shortcut"))
            .build();

        let shortcut_enabled = adw::SwitchRow::builder()
            .title(i18n.t("enable_shortcut"))
            .active(config.borrow().current().shortcuts_enabled)
            .build();
        {
            let config = config.clone();
            let shortcut_manager = shortcut_manager.clone();
            shortcut_enabled.connect_active_notify(move |row| {
                let enabled = row.is_active();
                let shortcut = config.borrow().current().shortcut.clone();
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.shortcuts_enabled = enabled)
                {
                    tracing::warn!(error = ?err, "failed to save shortcut enabled state");
                } else {
                    shortcut_manager.set_enabled(enabled, shortcut);
                }
            });
        }
        shortcut_group.add(&shortcut_enabled);

        let shortcut_config = adw::ActionRow::builder()
            .title(i18n.t("configure_shortcut"))
            .activatable(true)
            .build();
        let configure_button = gtk::Button::with_label(&i18n.t("configure"));
        configure_button.set_valign(gtk::Align::Center);
        configure_button.add_css_class("pill");
        shortcut_config.add_suffix(&configure_button);
        shortcut_config.set_activatable_widget(Some(&configure_button));
        {
            let config = config.clone();
            let shortcut_manager = shortcut_manager.clone();
            configure_button.connect_clicked(move |_| {
                let shortcut = config.borrow().current().shortcut.clone();
                shortcut_manager.configure(shortcut, None);
                open_keyboard_settings();
            });
        }
        shortcut_group.add(&shortcut_config);

        let tray_enabled = adw::SwitchRow::builder()
            .title(i18n.t("enable_tray"))
            .active(config.borrow().current().tray_enabled)
            .build();
        {
            let config = config.clone();
            let tray_manager = tray_manager.clone();
            tray_enabled.connect_active_notify(move |row| {
                let enabled = row.is_active();
                if let Err(err) = config.borrow_mut().update(|cfg| cfg.tray_enabled = enabled) {
                    tracing::warn!(error = ?err, "failed to save tray enabled state");
                } else {
                    tray_manager.set_enabled(enabled);
                }
            });
        }
        shortcut_group.add(&tray_enabled);
        page.add(&shortcut_group);

        let window_group = adw::PreferencesGroup::builder()
            .title(i18n.t("window_position"))
            .build();

        let offset = gtk::SpinButton::with_range(24.0, 240.0, 4.0);
        offset.set_value(config.borrow().current().window.panel_offset_y as f64);
        {
            let config = config.clone();
            let spotlight = spotlight.clone();
            offset.connect_value_changed(move |spin| {
                let value = spin.value() as i32;
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.window.panel_offset_y = value)
                {
                    tracing::warn!(error = ?err, "failed to save window offset");
                }
                spotlight.apply_window_config();
            });
        }
        window_group.add(&spin_row("Y offset", &offset));

        let width = gtk::SpinButton::with_range(480.0, 920.0, 8.0);
        width.set_value(config.borrow().current().window.panel_width as f64);
        {
            let config = config.clone();
            let spotlight = spotlight.clone();
            width.connect_value_changed(move |spin| {
                let value = spin.value() as i32;
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.window.panel_width = value)
                {
                    tracing::warn!(error = ?err, "failed to save panel width");
                }
                spotlight.apply_window_config();
            });
        }
        window_group.add(&spin_row("Panel width", &width));

        let max_results = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
        max_results.set_value(config.borrow().current().window.max_visible_results as f64);
        {
            let config = config.clone();
            let spotlight = spotlight.clone();
            max_results.connect_value_changed(move |spin| {
                let value = spin.value() as i32;
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.window.max_visible_results = value)
                {
                    tracing::warn!(error = ?err, "failed to save max visible results");
                }
                spotlight.apply_window_config();
            });
        }
        window_group.add(&spin_row(&i18n.t("max_visible_results"), &max_results));
        page.add(&window_group);

        let plugins_group = adw::PreferencesGroup::builder()
            .title(i18n.t("plugins"))
            .build();
        for plugin in plugins.borrow().plugin_metadata() {
            let row = adw::SwitchRow::builder()
                .title(plugin.name)
                .subtitle(plugin.description)
                .active(config.borrow().plugin_enabled(&plugin.id))
                .build();
            {
                let config = config.clone();
                let id = plugin.id.clone();
                row.connect_active_notify(move |row| {
                    let enabled = row.is_active();
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        cfg.plugins.insert(id.clone(), PluginConfig { enabled });
                    }) {
                        tracing::warn!(error = ?err, plugin_id = id, "failed to save plugin state");
                    }
                });
            }
            plugins_group.add(&row);
        }
        page.add(&plugins_group);

        toolbar.set_content(Some(&page));
        window.set_content(Some(&toolbar));

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn spin_row(title: &str, spin: &gtk::SpinButton) -> adw::ActionRow {
    spin.set_valign(gtk::Align::Center);
    spin.set_width_chars(6);

    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(spin);
    row
}

fn open_keyboard_settings() {
    if let Err(err) = std::process::Command::new("gnome-control-center")
        .arg("keyboard")
        .spawn()
    {
        tracing::warn!(error = ?err, "failed to open GNOME keyboard settings");
    }
}
