use crate::config::ConfigStore;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub struct GlobalShortcut;

impl GlobalShortcut {
    pub fn spawn<F>(config: Rc<RefCell<ConfigStore>>, on_toggle: F)
    where
        F: Fn() + 'static,
    {
        let shortcut = config.borrow().current().shortcut.clone();

        #[cfg(feature = "portal-shortcuts")]
        {
            let (sender, receiver) = mpsc::channel::<()>();
            glib::timeout_add_local(Duration::from_millis(50), move || {
                while receiver.try_recv().is_ok() {
                    on_toggle();
                }
                glib::ControlFlow::Continue
            });

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

                if let Err(err) = runtime.block_on(portal_shortcuts::bind_toggle(&shortcut, sender))
                {
                    tracing::warn!(
                        error = ?err,
                        shortcut,
                        "global shortcut portal unavailable; use tray menu or app actions"
                    );
                }
            });
        }

        #[cfg(not(feature = "portal-shortcuts"))]
        {
            let _ = (shortcut, on_toggle);
        }
    }
}

#[cfg(feature = "portal-shortcuts")]
mod portal_shortcuts {
    use anyhow::Result;
    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use ashpd::desktop::CreateSessionOptions;
    use futures_util::StreamExt;
    use std::sync::mpsc;

    pub async fn bind_toggle(shortcut: &str, sender: mpsc::Sender<()>) -> Result<()> {
        let portal = GlobalShortcuts::new().await?;
        tracing::info!(
            version = portal.version(),
            "global shortcuts portal available"
        );

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
        while let Some(activated) = stream.next().await {
            if activated.shortcut_id() == "toggle" {
                if sender.send(()).is_err() {
                    break;
                }
            }
        }

        Ok(())
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
