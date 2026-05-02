use adw::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

pub fn apply_to_window(window: &impl IsA<gtk::Window>) {
    let style_manager = adw::StyleManager::default();
    style_manager.set_color_scheme(adw::ColorScheme::Default);
    apply_theme(style_manager.is_dark(), Some(window));

    let window = window.clone().upcast::<gtk::Window>();
    style_manager.connect_dark_notify(move |manager| {
        apply_theme(manager.is_dark(), Some(&window));
    });
}

fn apply_theme(is_dark: bool, window: Option<&impl IsA<gtk::Window>>) {
    if let Some(window) = window {
        apply_theme_class(
            window,
            if is_dark {
                SystemTheme::Dark
            } else {
                SystemTheme::Light
            },
        );
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
