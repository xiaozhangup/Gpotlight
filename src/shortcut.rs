use crate::config::ConfigStore;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub struct GlobalShortcutManager {
    #[cfg(feature = "portal-shortcuts")]
    command_sender: tokio::sync::mpsc::UnboundedSender<ShortcutCommand>,
}

impl GlobalShortcutManager {
    pub fn spawn<F>(config: Rc<RefCell<ConfigStore>>, on_toggle: F) -> Self
    where
        F: Fn() + 'static,
    {
        let shortcut = config.borrow().current().shortcut.clone();

        #[cfg(feature = "portal-shortcuts")]
        {
            let (toggle_sender, toggle_receiver) = mpsc::channel::<()>();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                while toggle_receiver.try_recv().is_ok() {
                    on_toggle();
                }
                glib::ControlFlow::Continue
            });

            let (command_sender, command_receiver) =
                tokio::sync::mpsc::unbounded_channel::<ShortcutCommand>();
            if command_sender
                .send(ShortcutCommand::Rebind(shortcut.clone()))
                .is_err()
            {
                tracing::warn!("failed to queue initial global shortcut binding");
            }

            std::thread::spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::warn!(error = ?err, "failed to create tokio runtime");
                        return;
                    }
                };

                runtime.block_on(portal_shortcuts::run_manager(
                    command_receiver,
                    toggle_sender,
                ));
            });

            Self { command_sender }
        }

        #[cfg(not(feature = "portal-shortcuts"))]
        {
            let _ = (shortcut, on_toggle);
            Self {}
        }
    }

    pub fn rebind(&self, shortcut: String) {
        #[cfg(feature = "portal-shortcuts")]
        if self
            .command_sender
            .send(ShortcutCommand::Rebind(shortcut.clone()))
            .is_err()
        {
            tracing::warn!(shortcut, "failed to request global shortcut rebind");
        }

        #[cfg(not(feature = "portal-shortcuts"))]
        let _ = shortcut;
    }
}

#[cfg(feature = "portal-shortcuts")]
#[derive(Debug)]
enum ShortcutCommand {
    Rebind(String),
}

#[cfg(feature = "portal-shortcuts")]
mod portal_shortcuts {
    use super::ShortcutCommand;
    use anyhow::Result;
    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use ashpd::desktop::{CreateSessionOptions, Session};
    use futures_util::StreamExt;
    use std::sync::mpsc;

    pub async fn run_manager(
        mut command_receiver: tokio::sync::mpsc::UnboundedReceiver<ShortcutCommand>,
        toggle_sender: mpsc::Sender<()>,
    ) {
        let portal = match GlobalShortcuts::new().await {
            Ok(portal) => {
                tracing::info!(
                    version = portal.version(),
                    "global shortcuts portal available"
                );
                portal
            }
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    "global shortcut portal unavailable; use tray menu or app actions"
                );
                return;
            }
        };

        let mut registration: Option<Registration> = None;
        while let Some(command) = command_receiver.recv().await {
            match command {
                ShortcutCommand::Rebind(shortcut) => {
                    if let Some(registration) = registration.take() {
                        registration.close().await;
                    }

                    match Registration::bind(&portal, &shortcut, toggle_sender.clone()).await {
                        Ok(next) => registration = Some(next),
                        Err(err) => tracing::warn!(
                            error = ?err,
                            shortcut,
                            "failed to register global shortcut"
                        ),
                    }
                }
            }
        }

        if let Some(registration) = registration {
            registration.close().await;
        }
    }

    struct Registration {
        session: Session<GlobalShortcuts>,
        listener: tokio::task::JoinHandle<()>,
    }

    impl Registration {
        async fn bind(
            portal: &GlobalShortcuts,
            shortcut: &str,
            toggle_sender: mpsc::Sender<()>,
        ) -> Result<Self> {
            let session = portal
                .create_session(CreateSessionOptions::default())
                .await?;
            let preferred_trigger = portal_trigger(shortcut);
            let shortcuts = [NewShortcut::new("toggle", "Toggle Gpotlight")
                .preferred_trigger(preferred_trigger.as_deref())];

            let request = portal
                .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
                .await?;
            let response = request.response()?;
            if response.shortcuts().is_empty() {
                tracing::warn!(
                    shortcut,
                    preferred_trigger,
                    "global shortcut was not accepted by the portal"
                );
            } else {
                for shortcut in response.shortcuts() {
                    tracing::info!(
                        shortcut_id = shortcut.id(),
                        trigger = shortcut.trigger_description(),
                        "global shortcut registered"
                    );
                }
            }

            let mut stream = portal.receive_activated().await?;
            let listener = tokio::spawn(async move {
                while let Some(activated) = stream.next().await {
                    if activated.shortcut_id() == "toggle" && toggle_sender.send(()).is_err() {
                        break;
                    }
                }
            });

            Ok(Self { session, listener })
        }

        async fn close(self) {
            self.listener.abort();
            if let Err(err) = self.session.close().await {
                tracing::warn!(error = ?err, "failed to close global shortcut session");
            }
        }
    }

    fn portal_trigger(shortcut: &str) -> Option<String> {
        if shortcut.contains('+') {
            return Some(shortcut.to_string());
        }

        let mut converted = shortcut
            .replace("<Primary>", "CTRL+")
            .replace("<Control>", "CTRL+")
            .replace("<Ctrl>", "CTRL+")
            .replace("<Alt>", "ALT+")
            .replace("<Shift>", "SHIFT+")
            .replace("<Super>", "LOGO+")
            .replace("<Meta>", "LOGO+")
            .replace('<', "")
            .replace('>', "");

        if converted.ends_with('+') {
            converted.pop();
        }

        (!converted.is_empty()).then_some(converted)
    }
}
