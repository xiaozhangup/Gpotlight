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
    // Normalize once when the desktop database is loaded, not for every keystroke.
    search_name: String,
    search_comment: String,
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
                    search_name: name.to_lowercase(),
                    search_comment: comment.to_lowercase(),
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
        self.search_name.contains(needle)
            || self.search_comment.contains(needle)
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

    for pinyin in text.to_pinyin().flatten() {
        plain.push_str(pinyin.plain());
        initials.push_str(pinyin.first_letter());
    }

    format!("{} {}", plain.to_lowercase(), initials.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_search_preserves_unicode_and_optional_pinyin_matches() {
        let name = "音乐播放器";
        let comment = "Play MUSIC and Podcasts";
        let plugin = AppLauncherPlugin {
            apps: vec![DesktopApp {
                id: "music.desktop".into(),
                name: name.into(),
                comment: comment.into(),
                icon: None,
                search_name: name.to_lowercase(),
                search_comment: comment.to_lowercase(),
                pinyin: pinyin_search_text(name),
            }],
        };
        let config = PluginConfig::default();
        let mut app_config = AppConfig::default();

        for query in ["MUSIC", "音乐", "bofang", "bfq"] {
            assert_eq!(
                plugin.query_with_config(query, &config, &app_config).len(),
                1
            );
        }
        app_config.pinyin_search_enabled = false;
        assert!(plugin
            .query_with_config("bofang", &config, &app_config)
            .is_empty());
        assert_eq!(
            plugin.query_with_config("音乐", &config, &app_config).len(),
            1
        );
        assert!(plugin
            .query_with_config("", &config, &app_config)
            .is_empty());
    }

    #[test]
    #[ignore = "timing benchmark; run with --release --ignored --nocapture"]
    fn benchmark_precomputed_application_search() {
        use std::hint::black_box;
        use std::time::Instant;

        let apps: Vec<_> = (0..1_000)
            .map(|index| {
                let name = format!("Product {index} 音乐播放器");
                let comment = format!("Application {index}: Text Editor and Media Tools");
                DesktopApp {
                    id: format!("app-{index}.desktop"),
                    search_name: name.to_lowercase(),
                    search_comment: comment.to_lowercase(),
                    pinyin: pinyin_search_text(&name),
                    name,
                    comment,
                    icon: None,
                }
            })
            .collect();
        let needles = ["product", "editor", "missing", "17", "音乐", "bfq"];
        let measure = |precomputed: bool| {
            let start = Instant::now();
            let mut count = 0;
            for _ in 0..100 {
                for needle in needles {
                    for app in black_box(&apps) {
                        let matched = if precomputed {
                            app.matches(black_box(needle), true)
                        } else {
                            app.name.to_lowercase().contains(black_box(needle))
                                || app.comment.to_lowercase().contains(black_box(needle))
                                || app.pinyin.contains(black_box(needle))
                        };
                        count += usize::from(matched);
                    }
                }
            }
            (black_box(count), start.elapsed())
        };
        let (old_count, old_duration) = measure(false);
        let (new_count, new_duration) = measure(true);
        assert_eq!(old_count, new_count);
        eprintln!(
            "600 searches over 1,000 applications: old {:?}, indexed {:?}, {:.2}x faster",
            old_duration,
            new_duration,
            old_duration.as_secs_f64() / new_duration.as_secs_f64()
        );
    }
}
