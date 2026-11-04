use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

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

impl AppConfig {
    pub fn plugin_config(&self, id: &str) -> &PluginConfig {
        self.plugins
            .get(id)
            .unwrap_or_else(|| PluginConfig::defaults_for(id))
    }

    pub fn plugin_config_mut(&mut self, id: &str) -> &mut PluginConfig {
        self.plugins
            .entry(id.to_string())
            .or_insert_with(|| PluginConfig::defaults_for(id).clone())
    }
}

impl PluginConfig {
    fn defaults_for(id: &str) -> &'static Self {
        static STANDARD: LazyLock<PluginConfig> = LazyLock::new(PluginConfig::default);
        static BROWSER_HISTORY: LazyLock<PluginConfig> = LazyLock::new(|| PluginConfig {
            show_in_global_search: false,
            trigger_prefix: "h".into(),
            ..PluginConfig::default()
        });
        match id {
            "bundled.browser-history" => &BROWSER_HISTORY,
            _ => &STANDARD,
        }
    }

    pub fn prefixed_query<'a>(&self, query: &'a str) -> Option<&'a str> {
        if !self.enabled {
            return None;
        }
        let prefix = self.trigger_prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        let rest = query.strip_prefix(prefix)?;
        // Word prefixes such as "h" require a boundary, so "hello"
        // remains an ordinary search. Symbolic prefixes keep /query.
        let word_prefix = prefix.chars().last().is_some_and(char::is_alphanumeric);
        (!word_prefix || rest.is_empty() || rest.starts_with(char::is_whitespace))
            .then(|| rest.trim_start())
    }

    pub fn query<'a>(&self, query: &'a str) -> Option<&'a str> {
        self.prefixed_query(query).or_else(|| {
            (self.enabled && self.show_in_global_search && !query.trim().is_empty())
                .then_some(query)
        })
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
        self.config.plugin_config(id).clone()
    }

    pub fn plugin_query<'a>(&self, id: &str, query: &'a str) -> Option<&'a str> {
        self.config.plugin_config(id).query(query)
    }

    pub fn record_usage(&mut self, key: &str) -> Result<()> {
        self.update(|cfg| {
            let count = cfg.usage.entry(key.to_string()).or_default();
            *count = count.saturating_add(1);
        })
    }

    #[cfg(test)]
    pub(crate) fn from_config_for_test(config: AppConfig) -> Self {
        Self {
            path: PathBuf::new(),
            config,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_history_defaults_to_a_delimited_h_prefix() {
        let store = ConfigStore::from_config_for_test(AppConfig::default());
        let id = "bundled.browser-history";
        let config = store.plugin_config(id);
        assert!(config.enabled);
        assert!(!config.show_in_global_search);
        assert_eq!(config.trigger_prefix, "h");
        for query in ["", "   ", "hello", "history", "https://example.com"] {
            assert_eq!(store.plugin_query(id, query), None);
        }
        assert_eq!(store.plugin_query(id, "h"), Some(""));
        assert_eq!(store.plugin_query(id, "h rust"), Some("rust"));
        assert_eq!(store.plugin_query(id, "h\t浏览器"), Some("浏览器"));
    }

    #[test]
    fn changing_history_settings_preserves_defaults_and_explicit_overrides() {
        let id = "bundled.browser-history";
        let mut config = AppConfig::default();
        config
            .plugin_config_mut(id)
            .custom
            .insert("history_days".into(), toml::Value::Integer(7));
        assert!(!config.plugin_config(id).show_in_global_search);
        assert_eq!(config.plugin_config(id).trigger_prefix, "h");
        config.plugin_config_mut(id).show_in_global_search = true;
        config.plugin_config_mut(id).trigger_prefix = "!".into();
        let reloaded: AppConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        let store = ConfigStore::from_config_for_test(reloaded);
        assert_eq!(store.plugin_query(id, "!docs"), Some("docs"));
        assert_eq!(store.plugin_query(id, "hello"), Some("hello"));
        assert_eq!(
            store.plugin_config(id).custom["history_days"].as_integer(),
            Some(7)
        );
    }

    fn store_with_plugin(id: &str, plugin: PluginConfig) -> ConfigStore {
        let mut config = AppConfig::default();
        config.plugins.insert(id.to_string(), plugin);
        ConfigStore {
            path: PathBuf::new(),
            config,
        }
    }

    #[test]
    fn global_plugin_ignores_empty_queries() {
        let store = store_with_plugin(
            "test",
            PluginConfig {
                show_in_global_search: true,
                ..PluginConfig::default()
            },
        );

        assert_eq!(store.plugin_query("test", ""), None);
        assert_eq!(store.plugin_query("test", "   "), None);
        assert_eq!(store.plugin_query("test", "abc"), Some("abc"));
    }

    #[test]
    fn prefixed_plugin_allows_empty_query_after_prefix() {
        let store = store_with_plugin(
            "test",
            PluginConfig {
                show_in_global_search: false,
                trigger_prefix: "/".to_string(),
                ..PluginConfig::default()
            },
        );

        assert_eq!(store.plugin_query("test", ""), None);
        assert_eq!(store.plugin_query("test", "abc"), None);
        assert_eq!(store.plugin_query("test", "/"), Some(""));
        assert_eq!(store.plugin_query("test", "/abc"), Some("abc"));
        assert_eq!(store.plugin_query("test", "/ abc"), Some("abc"));
    }

    #[test]
    fn prefixed_global_plugin_prefers_prefix_mode_when_prefix_matches() {
        let store = store_with_plugin(
            "test",
            PluginConfig {
                show_in_global_search: true,
                trigger_prefix: "/".to_string(),
                ..PluginConfig::default()
            },
        );

        assert_eq!(store.plugin_query("test", ""), None);
        assert_eq!(store.plugin_query("test", "abc"), Some("abc"));
        assert_eq!(store.plugin_query("test", "/"), Some(""));
        assert_eq!(store.plugin_query("test", "/abc"), Some("abc"));
        assert_eq!(store.plugin_query("test", "/ abc"), Some("abc"));
    }
}
