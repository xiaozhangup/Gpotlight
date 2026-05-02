use crate::config::{ConfigStore, PluginConfig};
use crate::i18n::I18n;
use crate::plugin::SharedRegistry;
use crate::shortcut::GlobalShortcutManager;
use crate::theme;
use crate::ui::SpotlightWindow;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

pub struct SettingsWindow {
    window: gtk::ApplicationWindow,
}

impl SettingsWindow {
    pub fn new(
        app: &gtk::Application,
        i18n: Rc<I18n>,
        config: Rc<RefCell<ConfigStore>>,
        plugins: SharedRegistry,
        spotlight: Rc<SpotlightWindow>,
        shortcut_manager: Rc<GlobalShortcutManager>,
    ) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(i18n.t("settings"))
            .default_width(560)
            .default_height(620)
            .build();
        window.set_hide_on_close(true);
        theme::apply_to_window(&window);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);

        let shortcut_label = section_title(&i18n.t("shortcut"));
        let shortcut =
            shortcut_capture_button(config.clone(), i18n.clone(), shortcut_manager.clone());

        let window_label = section_title(&i18n.t("window_position"));
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

        let plugins_label = section_title(&i18n.t("plugins"));
        let plugins_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for plugin in plugins.borrow().plugin_metadata() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.set_valign(gtk::Align::Center);
            let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
            labels.set_hexpand(true);
            let name = gtk::Label::new(Some(&plugin.name));
            name.set_halign(gtk::Align::Start);
            name.add_css_class("heading");
            let description = gtk::Label::new(Some(&plugin.description));
            description.set_halign(gtk::Align::Start);
            description.set_wrap(true);
            description.add_css_class("dim-label");

            let toggle = gtk::Switch::new();
            toggle.set_active(config.borrow().plugin_enabled(&plugin.id));
            toggle.set_halign(gtk::Align::End);
            toggle.set_valign(gtk::Align::Center);
            {
                let config = config.clone();
                let id = plugin.id.clone();
                toggle.connect_active_notify(move |switch| {
                    let enabled = switch.is_active();
                    if let Err(err) = config.borrow_mut().update(|cfg| {
                        cfg.plugins.insert(id.clone(), PluginConfig { enabled });
                    }) {
                        tracing::warn!(error = ?err, plugin_id = id, "failed to save plugin state");
                    }
                });
            }

            labels.append(&name);
            labels.append(&description);
            row.append(&labels);
            row.append(&toggle);
            plugins_box.append(&row);
        }

        root.append(&shortcut_label);
        root.append(&shortcut);
        root.append(&window_label);
        root.append(&gtk::Label::new(Some("Y offset")));
        root.append(&offset);
        root.append(&gtk::Label::new(Some("Panel width")));
        root.append(&width);
        root.append(&gtk::Label::new(Some(&i18n.t("max_visible_results"))));
        root.append(&max_results);
        root.append(&plugins_label);
        root.append(&plugins_box);
        window.set_child(Some(&root));

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn section_title(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_halign(gtk::Align::Start);
    label.add_css_class("title-3");
    label
}

fn shortcut_capture_button(
    config: Rc<RefCell<ConfigStore>>,
    i18n: Rc<I18n>,
    shortcut_manager: Rc<GlobalShortcutManager>,
) -> gtk::Button {
    let button = gtk::Button::with_label(&config.borrow().current().shortcut);
    button.set_halign(gtk::Align::Start);
    button.add_css_class("pill");

    let capturing = Rc::new(RefCell::new(false));
    {
        let capturing = capturing.clone();
        let i18n = i18n.clone();
        button.connect_clicked(move |button| {
            *capturing.borrow_mut() = true;
            button.set_label(&i18n.t("press_shortcut"));
            button.grab_focus();
        });
    }

    let key = gtk::EventControllerKey::new();
    {
        let button = button.clone();
        let config = config.clone();
        let capturing = capturing.clone();
        key.connect_key_pressed(move |_, key, _, modifiers| {
            if !*capturing.borrow() {
                return glib::Propagation::Proceed;
            }

            if key == gtk::gdk::Key::Escape {
                *capturing.borrow_mut() = false;
                button.set_label(&config.borrow().current().shortcut);
                return glib::Propagation::Stop;
            }

            if let Some(shortcut) = accelerator_from_key(key, modifiers) {
                button.set_label(&shortcut);
                if let Err(err) = config
                    .borrow_mut()
                    .update(|cfg| cfg.shortcut = shortcut.clone())
                {
                    tracing::warn!(error = ?err, "failed to save shortcut");
                } else {
                    shortcut_manager.rebind(shortcut);
                }
                *capturing.borrow_mut() = false;
                return glib::Propagation::Stop;
            }

            glib::Propagation::Stop
        });
    }
    button.add_controller(key);

    button
}

fn accelerator_from_key(key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType) -> Option<String> {
    if is_modifier_key(key) {
        return None;
    }

    let mut parts = Vec::new();
    if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
        parts.push("CTRL".to_string());
    }
    if modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) {
        parts.push("ALT".to_string());
    }
    if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        parts.push("SHIFT".to_string());
    }
    if modifiers.contains(gtk::gdk::ModifierType::SUPER_MASK) {
        parts.push("LOGO".to_string());
    }

    let key_name = key.name()?;
    parts.push(key_name.to_string());
    Some(parts.join("+"))
}

fn is_modifier_key(key: gtk::gdk::Key) -> bool {
    matches!(
        key,
        gtk::gdk::Key::Shift_L
            | gtk::gdk::Key::Shift_R
            | gtk::gdk::Key::Control_L
            | gtk::gdk::Key::Control_R
            | gtk::gdk::Key::Alt_L
            | gtk::gdk::Key::Alt_R
            | gtk::gdk::Key::Super_L
            | gtk::gdk::Key::Super_R
            | gtk::gdk::Key::Meta_L
            | gtk::gdk::Key::Meta_R
    )
}
