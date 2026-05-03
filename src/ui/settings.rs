use crate::autostart;
use crate::config::{ConfigStore, PluginConfig};
use crate::i18n::I18n;
use crate::plugin::{PluginConfigItem, PluginConfigKind, SharedRegistry};
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

        let shortcut_config = adw::ActionRow::builder()
            .title(i18n.t("configure_shortcut"))
            .subtitle(i18n.t("shortcut_command_hint"))
            .activatable(true)
            .build();
        let configure_button = gtk::Button::with_label(&i18n.t("configure"));
        configure_button.set_valign(gtk::Align::Center);
        configure_button.add_css_class("pill");
        shortcut_config.add_suffix(&configure_button);
        shortcut_config.set_activatable_widget(Some(&configure_button));
        configure_button.connect_clicked(move |_| open_keyboard_settings());
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
                let result = {
                    let mut config = config.borrow_mut();
                    config.update(|cfg| cfg.tray_enabled = enabled)
                };
                if let Err(err) = result {
                    tracing::warn!(error = ?err, "failed to save tray enabled state");
                } else {
                    tray_manager.set_enabled(enabled);
                }
            });
        }
        shortcut_group.add(&tray_enabled);

        let autostart_enabled = adw::SwitchRow::builder()
            .title(i18n.t("enable_autostart"))
            .active(autostart::is_enabled())
            .build();
        autostart_enabled.connect_active_notify(move |row| {
            let enabled = row.is_active();
            if let Err(err) = autostart::set_enabled(enabled) {
                tracing::warn!(error = ?err, enabled, "failed to update autostart state");
            }
        });
        shortcut_group.add(&autostart_enabled);

        page.add(&shortcut_group);

        let search_group = adw::PreferencesGroup::builder()
            .title(i18n.t("search_behavior"))
            .build();

        let usage_ranking_enabled = adw::SwitchRow::builder()
            .title(i18n.t("enable_usage_ranking"))
            .subtitle(i18n.t("enable_usage_ranking_hint"))
            .active(config.borrow().current().usage_ranking_enabled)
            .build();
        {
            let config = config.clone();
            usage_ranking_enabled.connect_active_notify(move |row| {
                let enabled = row.is_active();
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.usage_ranking_enabled = enabled)
                {
                    tracing::warn!(error = ?err, "failed to save usage ranking state");
                }
            });
        }
        search_group.add(&usage_ranking_enabled);

        let pinyin_search_enabled = adw::SwitchRow::builder()
            .title(i18n.t("enable_pinyin_search"))
            .subtitle(i18n.t("enable_pinyin_search_hint"))
            .active(config.borrow().current().pinyin_search_enabled)
            .build();
        {
            let config = config.clone();
            pinyin_search_enabled.connect_active_notify(move |row| {
                let enabled = row.is_active();
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.pinyin_search_enabled = enabled)
                {
                    tracing::warn!(error = ?err, "failed to save pinyin search state");
                }
            });
        }
        search_group.add(&pinyin_search_enabled);
        page.add(&search_group);

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
            let plugin_config = config.borrow().plugin_config(&plugin.id);
            let row = adw::ExpanderRow::builder()
                .title(plugin.name.clone())
                .subtitle(plugin_summary(&plugin_config, &plugin.description))
                .enable_expansion(true)
                .build();

            let enabled = gtk::Switch::new();
            enabled.set_active(plugin_config.enabled);
            enabled.set_valign(gtk::Align::Center);
            row.add_suffix(&enabled);
            {
                let config = config.clone();
                let expander = row.clone();
                let description = plugin.description.clone();
                let id = plugin.id.clone();
                enabled.connect_active_notify(move |switch| {
                    let enabled = switch.is_active();
                    let result = {
                        let mut config = config.borrow_mut();
                        config.update(|cfg| {
                            let plugin = cfg.plugins.entry(id.clone()).or_default();
                            plugin.enabled = enabled;
                        })
                    };
                    if let Err(err) = result {
                        tracing::warn!(error = ?err, plugin_id = id, "failed to save plugin state");
                    } else {
                        let plugin_config = config.borrow().plugin_config(&id);
                        expander.set_subtitle(&plugin_summary(&plugin_config, &description));
                    }
                });
            }

            let global_search = adw::SwitchRow::builder()
                .title(i18n.t("plugin_global_search"))
                .active(plugin_config.show_in_global_search)
                .build();
            {
                let config = config.clone();
                let expander = row.clone();
                let description = plugin.description.clone();
                let id = plugin.id.clone();
                global_search.connect_active_notify(move |row| {
                    let enabled = row.is_active();
                    let result = {
                        let mut config = config.borrow_mut();
                        config.update(|cfg| {
                            let plugin = cfg.plugins.entry(id.clone()).or_default();
                            plugin.show_in_global_search = enabled;
                        })
                    };
                    if let Err(err) = result {
                        tracing::warn!(error = ?err, plugin_id = id, "failed to save plugin state");
                    } else {
                        let plugin_config = config.borrow().plugin_config(&id);
                        expander.set_subtitle(&plugin_summary(&plugin_config, &description));
                    }
                });
            }
            row.add_row(&global_search);

            let prefix_entry = gtk::Entry::new();
            prefix_entry.set_text(&plugin_config.trigger_prefix);
            prefix_entry.set_width_chars(8);
            prefix_entry.set_max_width_chars(16);
            let prefix_row = adw::ActionRow::builder()
                .title(i18n.t("plugin_trigger_prefix"))
                .subtitle(i18n.t("plugin_trigger_prefix_hint"))
                .build();
            prefix_row.add_suffix(&prefix_entry);
            {
                let config = config.clone();
                let expander = row.clone();
                let description = plugin.description.clone();
                let id = plugin.id.clone();
                prefix_entry.connect_changed(move |entry| {
                    let prefix = entry.text().to_string();
                    let result = {
                        let mut config = config.borrow_mut();
                        config.update(|cfg| {
                            let plugin = cfg.plugins.entry(id.clone()).or_default();
                            plugin.trigger_prefix = prefix.clone();
                        })
                    };
                    if let Err(err) = result {
                        tracing::warn!(
                            error = ?err,
                            plugin_id = id,
                            "failed to save plugin trigger prefix"
                        );
                    } else {
                        let plugin_config = config.borrow().plugin_config(&id);
                        expander.set_subtitle(&plugin_summary(&plugin_config, &description));
                    }
                });
            }
            row.add_row(&prefix_row);

            for item in plugin.config_items {
                row.add_row(&custom_config_row(&config, &plugin.id, item));
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

fn plugin_summary(config: &PluginConfig, description: &str) -> String {
    let visibility = if config.show_in_global_search {
        "Global search".to_string()
    } else if config.trigger_prefix.trim().is_empty() {
        "Prefix required".to_string()
    } else {
        format!("Prefix: {}", config.trigger_prefix)
    };

    if description.is_empty() {
        visibility
    } else {
        format!("{description} - {visibility}")
    }
}

fn custom_config_row(
    config: &Rc<RefCell<ConfigStore>>,
    plugin_id: &str,
    item: PluginConfigItem,
) -> gtk::Widget {
    match item.kind {
        PluginConfigKind::Bool => {
            let row = adw::SwitchRow::builder()
                .title(item.title)
                .subtitle(item.description)
                .active(
                    config
                        .borrow()
                        .plugin_config(plugin_id)
                        .custom
                        .get(&item.key)
                        .and_then(toml::Value::as_bool)
                        .unwrap_or_else(|| item.default_value.as_bool().unwrap_or(false)),
                )
                .build();
            {
                let config = config.clone();
                let plugin_id = plugin_id.to_string();
                let key = item.key;
                row.connect_active_notify(move |row| {
                    let value = row.is_active();
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        let plugin = cfg.plugins.entry(plugin_id.clone()).or_default();
                        plugin
                            .custom
                            .insert(key.clone(), toml::Value::Boolean(value));
                    }) {
                        tracing::warn!(
                            error = ?err,
                            plugin_id,
                            setting = key,
                            "failed to save plugin custom setting"
                        );
                    }
                });
            }
            row.upcast()
        }
        PluginConfigKind::Text => {
            let entry = gtk::Entry::new();
            entry.set_text(
                config
                    .borrow()
                    .plugin_config(plugin_id)
                    .custom
                    .get(&item.key)
                    .and_then(toml::Value::as_str)
                    .or_else(|| item.default_value.as_str())
                    .unwrap_or_default(),
            );
            let row = adw::ActionRow::builder()
                .title(item.title)
                .subtitle(item.description)
                .build();
            row.add_suffix(&entry);
            {
                let config = config.clone();
                let plugin_id = plugin_id.to_string();
                let key = item.key;
                entry.connect_changed(move |entry| {
                    let value = entry.text().to_string();
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        let plugin = cfg.plugins.entry(plugin_id.clone()).or_default();
                        plugin
                            .custom
                            .insert(key.clone(), toml::Value::String(value.clone()));
                    }) {
                        tracing::warn!(
                            error = ?err,
                            plugin_id,
                            setting = key,
                            "failed to save plugin custom setting"
                        );
                    }
                });
            }
            row.upcast()
        }
        PluginConfigKind::Choice { options } => {
            let current_value = config
                .borrow()
                .plugin_config(plugin_id)
                .custom
                .get(&item.key)
                .and_then(toml::Value::as_str)
                .or_else(|| item.default_value.as_str())
                .unwrap_or_default()
                .to_string();
            let selected = options
                .iter()
                .position(|option| option.value == current_value)
                .unwrap_or(0);
            let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
            let dropdown = gtk::DropDown::from_strings(&labels);
            dropdown.set_selected(selected as u32);
            dropdown.set_valign(gtk::Align::Center);

            let row = adw::ActionRow::builder()
                .title(item.title)
                .subtitle(item.description)
                .build();
            row.add_suffix(&dropdown);
            {
                let config = config.clone();
                let plugin_id = plugin_id.to_string();
                let key = item.key;
                dropdown.connect_selected_notify(move |dropdown| {
                    let selected = dropdown.selected() as usize;
                    let Some(option) = options.get(selected) else {
                        return;
                    };
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        let plugin = cfg.plugins.entry(plugin_id.clone()).or_default();
                        plugin
                            .custom
                            .insert(key.clone(), toml::Value::String(option.value.clone()));
                    }) {
                        tracing::warn!(
                            error = ?err,
                            plugin_id,
                            setting = key,
                            "failed to save plugin custom setting"
                        );
                    }
                });
            }
            row.upcast()
        }
        PluginConfigKind::Integer { min, max, step } => {
            let spin = gtk::SpinButton::with_range(min as f64, max as f64, step as f64);
            spin.set_value(
                config
                    .borrow()
                    .plugin_config(plugin_id)
                    .custom
                    .get(&item.key)
                    .and_then(toml::Value::as_integer)
                    .or_else(|| item.default_value.as_integer())
                    .unwrap_or(min) as f64,
            );
            let row = adw::ActionRow::builder()
                .title(item.title)
                .subtitle(item.description)
                .build();
            row.add_suffix(&spin);
            {
                let config = config.clone();
                let plugin_id = plugin_id.to_string();
                let key = item.key;
                spin.connect_value_changed(move |spin| {
                    let value = spin.value() as i64;
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        let plugin = cfg.plugins.entry(plugin_id.clone()).or_default();
                        plugin
                            .custom
                            .insert(key.clone(), toml::Value::Integer(value));
                    }) {
                        tracing::warn!(
                            error = ?err,
                            plugin_id,
                            setting = key,
                            "failed to save plugin custom setting"
                        );
                    }
                });
            }
            row.upcast()
        }
    }
}

fn open_keyboard_settings() {
    if let Err(err) = std::process::Command::new("setsid")
        .arg("gnome-control-center")
        .arg("keyboard")
        .spawn()
    {
        tracing::warn!(error = ?err, "failed to open GNOME keyboard settings");
    }
}
