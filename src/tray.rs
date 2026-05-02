use crate::i18n::I18n;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub struct TrayManager {
    #[cfg(feature = "tray")]
    i18n: Rc<I18n>,
    #[cfg(feature = "tray")]
    on_toggle: Rc<dyn Fn()>,
    #[cfg(feature = "tray")]
    on_settings: Rc<dyn Fn()>,
    #[cfg(feature = "tray")]
    on_quit: Rc<dyn Fn()>,
    #[cfg(feature = "tray")]
    handle: RefCell<Option<ksni::blocking::Handle<TrayService>>>,
}

#[cfg(feature = "tray")]
impl TrayManager {
    pub fn new<T, S, Q>(i18n: Rc<I18n>, on_toggle: T, on_settings: S, on_quit: Q) -> Self
    where
        T: Fn() + 'static,
        S: Fn() + 'static,
        Q: Fn() + 'static,
    {
        Self {
            i18n,
            on_toggle: Rc::new(on_toggle),
            on_settings: Rc::new(on_settings),
            on_quit: Rc::new(on_quit),
            handle: RefCell::new(None),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.spawn();
        } else {
            self.shutdown();
        }
    }

    fn spawn(&self) {
        if self
            .handle
            .borrow()
            .as_ref()
            .is_some_and(|handle| !handle.is_closed())
        {
            return;
        }

        let i18n = self.i18n.clone();
        let title = i18n.t("app_name");
        let open_settings = i18n.t("open_settings");
        let quit = i18n.t("quit");
        let (sender, receiver) = mpsc::channel::<TrayCommand>();

        let on_toggle = self.on_toggle.clone();
        let on_settings = self.on_settings.clone();
        let on_quit = self.on_quit.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || loop {
            match receiver.try_recv() {
                Ok(TrayCommand::Toggle) => on_toggle(),
                Ok(TrayCommand::Settings) => on_settings(),
                Ok(TrayCommand::Quit) => on_quit(),
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        });

        use ksni::blocking::TrayMethods;

        let service = TrayService {
            title,
            sender,
            open_settings,
            quit,
        };

        match service.assume_sni_available(true).spawn() {
            Ok(handle) => {
                self.handle.replace(Some(handle));
            }
            Err(err) => tracing::warn!(error = ?err, "failed to spawn tray service"),
        }
    }

    fn shutdown(&self) {
        if let Some(handle) = self.handle.borrow_mut().take() {
            handle.shutdown().wait();
        }
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
impl TrayManager {
    pub fn new<T, S, Q>(_i18n: Rc<I18n>, _on_toggle: T, _on_settings: S, _on_quit: Q) -> Self
    where
        T: Fn() + 'static,
        S: Fn() + 'static,
        Q: Fn() + 'static,
    {
        Self {}
    }

    pub fn set_enabled(&self, _enabled: bool) {}
}
