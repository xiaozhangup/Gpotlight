pub mod builtin;
pub mod manifest;

use crate::config::ConfigStore;
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub subtitle: String,
    pub icon: Option<String>,
    pub action: PluginAction,
}

#[derive(Debug, Clone)]
pub enum PluginAction {
    LaunchDesktopFile(String),
    OpenUri(String),
    CopyText(String),
    Noop,
}

pub trait SearchPlugin {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn query(&self, query: &str) -> Vec<SearchResult>;
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
            })
            .collect()
    }

    pub fn search(&self, config: &ConfigStore, query: &str) -> Vec<SearchResult> {
        self.plugins
            .iter()
            .filter(|plugin| config.plugin_enabled(plugin.id()))
            .flat_map(|plugin| plugin.query(query))
            .take(100)
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
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
        PluginAction::Noop => {}
    }

    window.set_visible(false);
}

pub type SharedRegistry = Rc<RefCell<PluginRegistry>>;
