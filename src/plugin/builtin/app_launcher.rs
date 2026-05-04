use crate::config::{AppConfig, PluginConfig};
use crate::plugin::{PluginAction, PluginConfigItem, PluginConfigKind, SearchPlugin, SearchResult};
use gio::prelude::*;
use pinyin::ToPinyin;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct AppLauncherPlugin {
    apps: Vec<DesktopApp>,
}

#[derive(Clone)]
struct DesktopApp {
    id: String,
    name: String,
    comment: String,
    icon: Option<String>,
    pinyin: String,
}

impl AppLauncherPlugin {
    pub(super) fn load() -> Self {
        let apps = gio::AppInfo::all()
            .into_iter()
            .filter(|app| app.should_show())
            .filter_map(|app| {
                let id = app.id()?.to_string();
                let name = app.name().to_string();
                let comment = app.description().map(|s| s.to_string()).unwrap_or_default();
                Some(DesktopApp {
                    icon: app_icon_name(&app).or_else(|| desktop_icon_from_file(&id)),
                    pinyin: pinyin_search_text(&name),
                    id,
                    name,
                    comment,
                })
            })
            .collect();

        Self { apps }
    }
}

impl SearchPlugin for AppLauncherPlugin {
    fn id(&self) -> &str {
        "builtin.app-launcher"
    }

    fn name(&self) -> &str {
        "Applications"
    }

    fn description(&self) -> &str {
        "Search installed desktop applications"
    }

    fn config_items(&self) -> Vec<PluginConfigItem> {
        vec![PluginConfigItem {
            key: "use_unified_icon".to_string(),
            title: "Use unified icon".to_string(),
            description: "Show the same icon for all application results".to_string(),
            kind: PluginConfigKind::Bool,
            default_value: toml::Value::Boolean(false),
        }]
    }

    fn query(&self, query: &str) -> Vec<SearchResult> {
        self.query_with_config(query, &PluginConfig::default(), &AppConfig::default())
    }

    fn query_with_config(
        &self,
        query: &str,
        config: &PluginConfig,
        app_config: &AppConfig,
    ) -> Vec<SearchResult> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }

        let use_unified_icon = config
            .custom
            .get("use_unified_icon")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);

        self.apps
            .iter()
            .filter(|app| app.matches(&needle, app_config.pinyin_search_enabled))
            .map(|app| SearchResult {
                title: app.name.clone(),
                subtitle: app.comment.clone(),
                icon: if use_unified_icon {
                    Some("applications-system-symbolic".to_string())
                } else {
                    app.icon
                        .clone()
                        .or_else(|| Some("application-x-executable-symbolic".to_string()))
                },
                pinned: false,
                action: PluginAction::LaunchDesktopFile(app.id.clone()),
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            })
            .collect()
    }
}

impl DesktopApp {
    fn matches(&self, needle: &str, pinyin_enabled: bool) -> bool {
        self.name.to_lowercase().contains(needle)
            || self.comment.to_lowercase().contains(needle)
            || (pinyin_enabled && self.pinyin.contains(needle))
    }
}

fn app_icon_name(app: &gio::AppInfo) -> Option<String> {
    let icon = app.icon()?;
    if let Ok(file_icon) = icon.clone().downcast::<gio::FileIcon>() {
        return file_icon
            .file()
            .path()
            .map(|path| path.to_string_lossy().into_owned());
    }

    icon.downcast::<gio::ThemedIcon>()
        .ok()
        .and_then(|icon| icon.names().first().map(ToString::to_string))
}

fn desktop_icon_from_file(desktop_id: &str) -> Option<String> {
    desktop_file_paths(desktop_id)
        .into_iter()
        .find_map(|path| desktop_icon_from_path(&path))
}

fn desktop_file_paths(desktop_id: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(data_home) = dirs::data_local_dir() {
        paths.push(data_home.join("applications").join(desktop_id));
    }
    if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS") {
        paths.extend(
            std::env::split_paths(&data_dirs).map(|dir| dir.join("applications").join(desktop_id)),
        );
    } else {
        paths.push(PathBuf::from("/usr/local/share/applications").join(desktop_id));
        paths.push(PathBuf::from("/usr/share/applications").join(desktop_id));
    }
    paths
}

fn desktop_icon_from_path(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    raw.lines()
        .find_map(|line| line.strip_prefix("Icon=").map(str::trim))
        .filter(|icon| !icon.is_empty())
        .map(ToString::to_string)
}

fn pinyin_search_text(text: &str) -> String {
    let mut plain = String::new();
    let mut initials = String::new();

    for item in text.to_pinyin() {
        if let Some(pinyin) = item {
            plain.push_str(pinyin.plain());
            initials.push_str(pinyin.first_letter());
        }
    }

    format!("{} {}", plain.to_lowercase(), initials.to_lowercase())
}
