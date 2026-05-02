use gio::prelude::*;
use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

pub fn current_system_theme() -> SystemTheme {
    let settings = gio::Settings::new("org.gnome.desktop.interface");
    match settings.string("color-scheme").as_str() {
        "prefer-dark" => SystemTheme::Dark,
        _ => SystemTheme::Light,
    }
}

pub fn apply_to_window(window: &impl IsA<gtk::Window>) {
    apply_theme(current_system_theme(), Some(window));

    let settings = gio::Settings::new("org.gnome.desktop.interface");
    let window = window.clone().upcast::<gtk::Window>();
    settings.connect_changed(Some("color-scheme"), move |settings, _| {
        let theme = match settings.string("color-scheme").as_str() {
            "prefer-dark" => SystemTheme::Dark,
            _ => SystemTheme::Light,
        };
        apply_theme(theme, Some(&window));
    });
}

fn apply_theme(theme: SystemTheme, window: Option<&impl IsA<gtk::Window>>) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(theme == SystemTheme::Dark);
    }

    if let Some(window) = window {
        apply_theme_class(window, theme);
    }
}

fn apply_theme_class(window: &impl IsA<gtk::Window>, theme: SystemTheme) {
    let window = window.as_ref();
    window.remove_css_class("system-light");
    window.remove_css_class("system-dark");
    match theme {
        SystemTheme::Light => window.add_css_class("system-light"),
        SystemTheme::Dark => window.add_css_class("system-dark"),
    }
}
