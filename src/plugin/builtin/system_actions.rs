use crate::plugin::{PluginAction, SearchPlugin, SearchResult};

pub(super) struct SystemActionsPlugin;

impl SearchPlugin for SystemActionsPlugin {
    fn id(&self) -> &str {
        "builtin.system-actions"
    }

    fn name(&self) -> &str {
        "System Actions"
    }

    fn description(&self) -> &str {
        "Open Gpotlight settings or quit the app"
    }

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }

        let actions = [
            SearchResult {
                title: "Open Gpotlight Settings".to_string(),
                subtitle: "Show the settings window".to_string(),
                icon: Some("emblem-system-symbolic".to_string()),
                pinned: false,
                action: PluginAction::AppAction("settings".to_string()),
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            },
            SearchResult {
                title: "Quit Gpotlight".to_string(),
                subtitle: "Exit the running Gpotlight process".to_string(),
                icon: Some("application-exit-symbolic".to_string()),
                pinned: false,
                action: PluginAction::AppAction("quit".to_string()),
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            },
        ];

        actions
            .into_iter()
            .filter(|action| {
                action.title.to_lowercase().contains(&needle)
                    || action.subtitle.to_lowercase().contains(&needle)
            })
            .collect()
    }
}
