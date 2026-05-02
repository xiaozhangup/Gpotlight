use crate::i18n::I18n;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub struct Tray;

#[cfg(feature = "tray")]
impl Tray {
    pub fn spawn<T, S, Q>(i18n: Rc<I18n>, on_toggle: T, on_settings: S, on_quit: Q)
    where
        T: Fn() + 'static,
        S: Fn() + 'static,
        Q: Fn() + 'static,
    {
        let title = i18n.t("app_name");
        let open_settings = i18n.t("open_settings");
        let quit = i18n.t("quit");
        let (sender, receiver) = mpsc::channel::<TrayCommand>();

        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(command) = receiver.try_recv() {
                match command {
                    TrayCommand::Toggle => on_toggle(),
                    TrayCommand::Settings => on_settings(),
                    TrayCommand::Quit => on_quit(),
                }
            }
            glib::ControlFlow::Continue
        });

        std::thread::spawn(move || {
            use ksni::blocking::TrayMethods;

            let service = TrayService {
                title,
                sender,
                open_settings,
                quit,
            };

            if let Err(err) = service.assume_sni_available(true).spawn() {
                tracing::warn!(error = ?err, "failed to spawn tray service");
            }
        });
    }
}

#[cfg(feature = "tray")]
struct TrayService {
    title: String,
    sender: mpsc::Sender<TrayCommand>,
    open_settings: String,
    quit: String,
}

#[cfg(feature = "tray")]
enum TrayCommand {
    Toggle,
    Settings,
    Quit,
}

#[cfg(feature = "tray")]
impl ksni::Tray for TrayService {
    fn id(&self) -> String {
        "gpotlight".to_string()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn icon_name(&self) -> String {
        "system-search-symbolic".to_string()
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            ksni::menu::StandardItem {
                label: "Toggle".to_string(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(TrayCommand::Toggle);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: self.open_settings.clone(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(TrayCommand::Settings);
                }),
                ..Default::default()
            }
            .into(),
            ksni::menu::StandardItem {
                label: self.quit.clone(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.sender.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[cfg(not(feature = "tray"))]
impl Tray {
    pub fn spawn<T, S, Q>(_i18n: Rc<I18n>, _on_toggle: T, _on_settings: S, _on_quit: Q)
    where
        T: Fn() + 'static,
        S: Fn() + 'static,
        Q: Fn() + 'static,
    {
    }
}
