use crate::config::ConfigStore;
use std::cell::RefCell;
use std::rc::Rc;

pub struct GlobalShortcutManager;

impl GlobalShortcutManager {
    pub fn spawn<F>(config: Rc<RefCell<ConfigStore>>, on_toggle: F) -> Self
    where
        F: Fn() + 'static,
    {
        let _ = (config, on_toggle);

        // GlobalShortcuts portal registration is intentionally disabled.
        // Users should configure a system shortcut that runs:
        //
        //     gpotlight toggle
        //
        // Keeping this manager as a no-op preserves the app wiring while avoiding
        // portal registration side effects and stale shortcut ids.
        Self
    }

    pub fn set_enabled(&self, enabled: bool, shortcut: String) {
        let _ = (enabled, shortcut);
    }
}
