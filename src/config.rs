use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub locale: String,
    pub shortcut: String,
    pub window: WindowConfig,
    pub plugins: IndexMap<String, PluginConfig>,
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
    pub enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "zh-CN".to_string(),
            shortcut: "LOGO+space".to_string(),
            window: WindowConfig {
                host_width: 960,
                host_height: 620,
                panel_width: 720,
                panel_offset_y: 92,
                max_visible_results: default_max_visible_results(),
            },
            plugins: IndexMap::new(),
        }
    }
}

fn default_max_visible_results() -> i32 {
    6
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

    pub fn plugin_enabled(&self, id: &str) -> bool {
        self.config
            .plugins
            .get(id)
            .map(|plugin| plugin.enabled)
            .unwrap_or(true)
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

fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("XDG config directory is unavailable")?;
    Ok(base.join("gpotlight").join("config.toml"))
}
