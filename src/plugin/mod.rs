pub mod builtin;
pub mod manifest;

use crate::config::{AppConfig, ConfigStore, PluginConfig};
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub subtitle: String,
    pub icon: Option<String>,
    pub pinned: bool,
    pub action: PluginAction,
}

#[derive(Debug, Clone)]
pub enum PluginAction {
    LaunchDesktopFile(String),
    OpenUri(String),
    CopyText(String),
    AppAction(String),
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
        let mut results: Vec<SearchResult> = self
            .plugins
            .iter()
            .filter_map(|plugin| {
                let plugin_query = config.plugin_query(plugin.id(), query)?;
                let plugin_config = config.plugin_config(plugin.id());
                Some(plugin.query_with_config(plugin_query, &plugin_config, app_config))
            })
            .flatten()
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
            PluginAction::LaunchDesktopFile(desktop_id) => format!("desktop:{desktop_id}"),
            PluginAction::OpenUri(uri) => format!("uri:{uri}"),
            PluginAction::CopyText(text) => format!("copy:{text}"),
            PluginAction::AppAction(action) => format!("app-action:{action}"),
            PluginAction::Noop => format!("noop:{}:{}", self.title, self.subtitle),
        }
    }
}

pub fn activate_result(result: &SearchResult, window: &gtk::Window) {
    match &result.action {
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
        PluginAction::Noop => {}
    }

    window.set_visible(false);
}

pub type SharedRegistry = Rc<RefCell<PluginRegistry>>;
