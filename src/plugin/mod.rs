pub mod builtin;
pub mod manifest;

use crate::config::{AppConfig, ConfigStore, PluginConfig};
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub subtitle: String,
    pub icon: Option<String>,
    pub pinned: bool,
    pub action: PluginAction,
    pub buttons: Vec<SearchResultButton>,
    pub refresh_key: Option<String>,
    pub refresh_interval_ms: Option<u64>,
    pub source_plugin_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchResultButton {
    pub title: String,
    pub icon: Option<String>,
    pub action: PluginAction,
    pub close_on_activate: bool,
    pub refresh_after_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum PluginAction {
    LaunchDesktopFile(String),
    OpenUri(String),
    CopyText(String),
    AppAction(String),
    LaunchCommand { command: String, args: Vec<String> },
    Noop,
}

pub trait SearchPlugin {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn query(&self, query: &str) -> Vec<SearchResult>;

    fn query_with_config(
        &self,
        query: &str,
        config: &PluginConfig,
        app_config: &AppConfig,
    ) -> Vec<SearchResult> {
        let _ = (config, app_config);
        self.query(query)
    }

    fn config_items(&self) -> Vec<PluginConfigItem> {
        Vec::new()
    }
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Box<dyn SearchPlugin>>,
}

impl PluginRegistry {
    pub fn register<P>(&mut self, plugin: P)
    where
        P: SearchPlugin + 'static,
    {
        self.plugins.push(Box::new(plugin));
    }

    pub fn plugin_metadata(&self) -> Vec<PluginMetadata> {
        self.plugins
            .iter()
            .map(|plugin| PluginMetadata {
                id: plugin.id().to_string(),
                name: plugin.name().to_string(),
                description: plugin.description().to_string(),
                config_items: plugin.config_items(),
            })
            .collect()
    }

    pub fn search(&self, config: &ConfigStore, query: &str) -> Vec<SearchResult> {
        let app_config = config.current();
        let query_is_empty = query.trim().is_empty();
        let mut results: Vec<SearchResult> = self
            .plugins
            .iter()
            .filter_map(|plugin| {
                let plugin_config = config.plugin_config(plugin.id());
                let plugin_query = config.plugin_query(plugin.id(), query).or_else(|| {
                    (query_is_empty && plugin_config.enabled && plugin_config.show_in_global_search)
                        .then_some("")
                })?;
                Some(self.query_plugin(plugin.as_ref(), plugin_query, &plugin_config, app_config))
            })
            .flatten()
            .filter(|result| !query_is_empty || result.pinned)
            .collect();

        if app_config.usage_ranking_enabled {
            results.sort_by_key(|result| {
                (
                    std::cmp::Reverse(result.pinned),
                    std::cmp::Reverse(config.usage_count(&result.usage_key())),
                )
            });
        } else {
            results.sort_by_key(|result| std::cmp::Reverse(result.pinned));
        }

        results.into_iter().take(100).collect()
    }

    pub fn search_plugin(
        &self,
        config: &ConfigStore,
        plugin_id: &str,
        query: &str,
    ) -> Vec<SearchResult> {
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.id() == plugin_id) else {
            return Vec::new();
        };
        let plugin_config = config.plugin_config(plugin.id());
        if !plugin_config.enabled {
            return Vec::new();
        }
        self.query_plugin(plugin.as_ref(), query, &plugin_config, config.current())
    }

    fn query_plugin(
        &self,
        plugin: &dyn SearchPlugin,
        query: &str,
        plugin_config: &PluginConfig,
        app_config: &AppConfig,
    ) -> Vec<SearchResult> {
        plugin
            .query_with_config(query, plugin_config, app_config)
            .into_iter()
            .map(|mut result| {
                result.source_plugin_id = Some(plugin.id().to_string());
                result
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config_items: Vec<PluginConfigItem>,
}

#[derive(Debug, Clone)]
pub struct PluginConfigItem {
    pub key: String,
    pub title: String,
    pub description: String,
    pub kind: PluginConfigKind,
    pub default_value: toml::Value,
}

#[derive(Debug, Clone)]
pub enum PluginConfigKind {
    Bool,
    Text,
    Choice { options: Vec<PluginConfigChoice> },
    Integer { min: i64, max: i64, step: i64 },
}

#[derive(Debug, Clone)]
pub struct PluginConfigChoice {
    pub value: String,
    pub label: String,
}

impl SearchResult {
    pub fn usage_key(&self) -> String {
        match &self.action {
            PluginAction::Noop => format!("noop:{}:{}", self.title, self.subtitle),
            _ => self.action.usage_key_fragment(),
        }
    }
}

impl SearchResultButton {
    pub fn usage_key(&self, result: &SearchResult) -> String {
        format!(
            "button:{}:{}:{}",
            result.usage_key(),
            self.title,
            self.action.usage_key_fragment()
        )
    }
}

impl PluginAction {
    fn usage_key_fragment(&self) -> String {
        match self {
            PluginAction::LaunchDesktopFile(desktop_id) => format!("desktop:{desktop_id}"),
            PluginAction::OpenUri(uri) => format!("uri:{uri}"),
            PluginAction::CopyText(text) => format!("copy:{text}"),
            PluginAction::AppAction(action) => format!("app-action:{action}"),
            PluginAction::LaunchCommand { command, args } => {
                format!("command:{}:{}", command, args.join("\u{1f}"))
            }
            PluginAction::Noop => "noop".to_string(),
        }
    }
}

pub fn activate_result(result: &SearchResult, window: &gtk::Window) {
    activate_action(&result.action, window, true);
}

pub fn activate_action(action: &PluginAction, window: &gtk::Window, close_window: bool) {
    match action {
        PluginAction::LaunchDesktopFile(desktop_id) => {
            let ctx = gio::AppLaunchContext::new();
            if let Some(info) = gio::AppInfo::all().into_iter().find(|app| {
                app.id()
                    .map(|id| id.as_str() == desktop_id)
                    .unwrap_or(false)
            }) {
                if let Err(err) = info.launch(&[], Some(&ctx)) {
                    tracing::warn!(error = ?err, desktop_id, "failed to launch desktop file");
                }
            }
        }
        PluginAction::OpenUri(uri) => {
            if let Err(err) = open::that(uri) {
                tracing::warn!(error = ?err, uri, "failed to open uri");
            }
        }
        PluginAction::CopyText(text) => {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(text);
            }
        }
        PluginAction::AppAction(action) => {
            if let Some(app) = window.application() {
                app.activate_action(action, None);
            }
        }
        PluginAction::LaunchCommand { command, args } => {
            if let Err(err) = Command::new("setsid").arg(command).args(args).spawn() {
                tracing::warn!(error = ?err, command, args = ?args, "failed to launch command");
            }
        }
        PluginAction::Noop => {}
    }

    if close_window {
        window.set_visible(false);
    }
}

pub type SharedRegistry = Rc<RefCell<PluginRegistry>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigStore;

    struct StaticPlugin {
        id: &'static str,
        pinned: bool,
    }

    impl SearchPlugin for StaticPlugin {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> &str {
            self.id
        }

        fn description(&self) -> &str {
            self.id
        }

        fn query(&self, query: &str) -> Vec<SearchResult> {
            vec![SearchResult {
                title: format!("{}:{query}", self.id),
                subtitle: String::new(),
                icon: None,
                pinned: self.pinned,
                action: PluginAction::Noop,
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            }]
        }
    }

    #[test]
    fn empty_search_only_keeps_pinned_results() {
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "normal",
            pinned: false,
        });
        registry.register(StaticPlugin {
            id: "pinned",
            pinned: true,
        });
        let config = ConfigStore::from_config_for_test(AppConfig::default());

        let results = registry.search(&config, "");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "pinned:");
    }
}
