use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub locale: String,
    pub shortcut: String,
    #[serde(default = "default_shortcuts_enabled")]
    pub shortcuts_enabled: bool,
    #[serde(default = "default_tray_enabled")]
    pub tray_enabled: bool,
    #[serde(default = "default_usage_ranking_enabled")]
    pub usage_ranking_enabled: bool,
    #[serde(default = "default_pinyin_search_enabled")]
    pub pinyin_search_enabled: bool,
    pub window: WindowConfig,
    pub plugins: IndexMap<String, PluginConfig>,
    #[serde(default)]
    pub usage: IndexMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub host_width: i32,
    pub host_height: i32,
    pub panel_width: i32,
    pub panel_offset_y: i32,
    #[serde(default = "default_max_visible_results")]
    pub max_visible_results: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default = "default_plugin_enabled")]
    pub enabled: bool,
    #[serde(default = "default_plugin_show_in_global_search")]
    pub show_in_global_search: bool,
    #[serde(default)]
    pub trigger_prefix: String,
    #[serde(default)]
    pub custom: IndexMap<String, toml::Value>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "zh-CN".to_string(),
            shortcut: "LOGO+space".to_string(),
            shortcuts_enabled: default_shortcuts_enabled(),
            tray_enabled: default_tray_enabled(),
            usage_ranking_enabled: default_usage_ranking_enabled(),
            pinyin_search_enabled: default_pinyin_search_enabled(),
            window: WindowConfig {
                host_width: 960,
                host_height: 620,
                panel_width: 720,
                panel_offset_y: 92,
                max_visible_results: default_max_visible_results(),
            },
            plugins: IndexMap::new(),
            usage: IndexMap::new(),
        }
    }
}

fn default_max_visible_results() -> i32 {
    6
}

fn default_shortcuts_enabled() -> bool {
    true
}

fn default_tray_enabled() -> bool {
    true
}

fn default_usage_ranking_enabled() -> bool {
    true
}

fn default_pinyin_search_enabled() -> bool {
    true
}

fn default_plugin_enabled() -> bool {
    true
}

fn default_plugin_show_in_global_search() -> bool {
    true
}

pub struct ConfigStore {
    path: PathBuf,
    config: AppConfig,
}

impl ConfigStore {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let config = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?
        } else {
            AppConfig::default()
        };

        Ok(Self { path, config })
    }

    pub fn current(&self) -> &AppConfig {
        &self.config
    }

    pub fn update<F>(&mut self, update: F) -> Result<()>
    where
        F: FnOnce(&mut AppConfig),
    {
        update(&mut self.config);
        self.save()
    }

    pub fn plugin_config(&self, id: &str) -> PluginConfig {
        self.config.plugins.get(id).cloned().unwrap_or_default()
    }

    pub fn plugin_query<'a>(&self, id: &str, query: &'a str) -> Option<&'a str> {
        let plugin = self.plugin_config(id);
        if !plugin.enabled {
            return None;
        }

        if plugin.show_in_global_search {
            return Some(query);
        }

        let prefix = plugin.trigger_prefix.trim();
        if prefix.is_empty() {
            return None;
        }

        query.strip_prefix(prefix).map(str::trim_start)
    }

    pub fn usage_count(&self, key: &str) -> u32 {
        self.config.usage.get(key).copied().unwrap_or(0)
    }

    pub fn record_usage(&mut self, key: &str) -> Result<()> {
        self.update(|cfg| {
            let count = cfg.usage.entry(key.to_string()).or_default();
            *count = count.saturating_add(1);
        })
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw = toml::to_string_pretty(&self.config)?;
        fs::write(&self.path, raw)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: default_plugin_enabled(),
            show_in_global_search: default_plugin_show_in_global_search(),
            trigger_prefix: String::new(),
            custom: IndexMap::new(),
        }
    }
}

fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("XDG config directory is unavailable")?;
    Ok(base.join("gpotlight").join("config.toml"))
}
