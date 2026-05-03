use super::{
    PluginAction, PluginConfigChoice, PluginConfigItem, PluginConfigKind, PluginRegistry,
    SearchPlugin, SearchResult,
};
use crate::config::{AppConfig, PluginConfig};
use crate::plugin::manifest::register_manifest_plugins;
use gio::prelude::*;
use pinyin::ToPinyin;
use std::fs;
use std::path::{Path, PathBuf};

pub fn register_builtin_plugins(registry: &mut PluginRegistry, _config: &AppConfig) {
    registry.register(SystemActionsPlugin);
    registry.register(AppLauncherPlugin::load());
    registry.register(CalculatorPlugin);
    registry.register(WebSearchPlugin);
    register_manifest_plugins(registry);
}

struct SystemActionsPlugin;

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
            },
            SearchResult {
                title: "Quit Gpotlight".to_string(),
                subtitle: "Exit the running Gpotlight process".to_string(),
                icon: Some("application-exit-symbolic".to_string()),
                pinned: false,
                action: PluginAction::AppAction("quit".to_string()),
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

struct AppLauncherPlugin {
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
    fn load() -> Self {
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
            })
            .collect()
    }
}

struct CalculatorPlugin;

impl SearchPlugin for CalculatorPlugin {
    fn id(&self) -> &str {
        "builtin.calculator"
    }

    fn name(&self) -> &str {
        "Calculator"
    }

    fn description(&self) -> &str {
        "Evaluate simple arithmetic expressions"
    }

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let expr = query.trim();
        if expr.is_empty() || !expr.chars().any(|c| "+-*/".contains(c)) {
            return Vec::new();
        }

        match eval_arithmetic(expr) {
            Some(value) => vec![SearchResult {
                title: format_number(value),
                subtitle: expr.to_string(),
                icon: Some("accessories-calculator-symbolic".to_string()),
                pinned: true,
                action: PluginAction::CopyText(format_number(value)),
            }],
            None => Vec::new(),
        }
    }
}

struct WebSearchPlugin;

impl SearchPlugin for WebSearchPlugin {
    fn id(&self) -> &str {
        "builtin.web-search"
    }

    fn name(&self) -> &str {
        "Web Search"
    }

    fn description(&self) -> &str {
        "Open a browser search"
    }

    fn config_items(&self) -> Vec<PluginConfigItem> {
        vec![PluginConfigItem {
            key: "search_engine".to_string(),
            title: "Search engine".to_string(),
            description: "Choose the search engine used for web results".to_string(),
            kind: PluginConfigKind::Choice {
                options: vec![
                    PluginConfigChoice {
                        value: "google".to_string(),
                        label: "Google".to_string(),
                    },
                    PluginConfigChoice {
                        value: "bing".to_string(),
                        label: "Bing".to_string(),
                    },
                    PluginConfigChoice {
                        value: "baidu".to_string(),
                        label: "Baidu".to_string(),
                    },
                    PluginConfigChoice {
                        value: "duckduckgo".to_string(),
                        label: "DuckDuckGo".to_string(),
                    },
                ],
            },
            default_value: toml::Value::String("google".to_string()),
        }]
    }

    fn query(&self, query: &str) -> Vec<SearchResult> {
        self.query_with_config(query, &PluginConfig::default(), &AppConfig::default())
    }

    fn query_with_config(
        &self,
        query: &str,
        config: &PluginConfig,
        _app_config: &AppConfig,
    ) -> Vec<SearchResult> {
        let q = query.trim();
        if q.len() < 2 {
            return Vec::new();
        }

        let engine = config
            .custom
            .get("search_engine")
            .and_then(toml::Value::as_str)
            .unwrap_or("google");
        let engine = WebSearchEngine::from_config(engine);
        let encoded = encode_query(q);

        vec![SearchResult {
            title: format!("Search {} for \"{q}\"", engine.label()),
            subtitle: engine.label().to_string(),
            icon: Some("web-browser-symbolic".to_string()),
            pinned: false,
            action: PluginAction::OpenUri(engine.search_url(&encoded)),
        }]
    }
}

enum WebSearchEngine {
    Google,
    Bing,
    Baidu,
    DuckDuckGo,
}

impl WebSearchEngine {
    fn from_config(value: &str) -> Self {
        match value {
            "bing" => Self::Bing,
            "baidu" => Self::Baidu,
            "duckduckgo" => Self::DuckDuckGo,
            _ => Self::Google,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Bing => "Bing",
            Self::Baidu => "Baidu",
            Self::DuckDuckGo => "DuckDuckGo",
        }
    }

    fn search_url(&self, encoded_query: &str) -> String {
        match self {
            Self::Google => format!("https://www.google.com/search?q={encoded_query}"),
            Self::Bing => format!("https://www.bing.com/search?q={encoded_query}"),
            Self::Baidu => format!("https://www.baidu.com/s?wd={encoded_query}"),
            Self::DuckDuckGo => format!("https://duckduckgo.com/?q={encoded_query}"),
        }
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

fn encode_query(query: &str) -> String {
    let mut encoded = String::new();
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn eval_arithmetic(input: &str) -> Option<f64> {
    let mut parser = ArithmeticParser::new(input);
    let value = parser.parse_expr()?;
    parser.skip_ws();
    parser.is_done().then_some(value)
}

fn format_number(value: f64) -> String {
    let text = format!("{value:.12}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct ArithmeticParser<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> ArithmeticParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            cursor: 0,
        }
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') => {
                    self.cursor += 1;
                    value += self.parse_term()?;
                }
                Some(b'-') => {
                    self.cursor += 1;
                    value -= self.parse_term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') => {
                    self.cursor += 1;
                    value *= self.parse_factor()?;
                }
                Some(b'/') => {
                    self.cursor += 1;
                    value /= self.parse_factor()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_ws();
        if self.peek() == Some(b'(') {
            self.cursor += 1;
            let value = self.parse_expr()?;
            self.skip_ws();
            (self.peek()? == b')').then(|| self.cursor += 1)?;
            return Some(value);
        }

        let start = self.cursor;
        if self.peek() == Some(b'-') {
            self.cursor += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.')) {
            self.cursor += 1;
        }
        std::str::from_utf8(&self.input[start..self.cursor])
            .ok()?
            .parse()
            .ok()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.cursor += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.cursor).copied()
    }

    fn is_done(&self) -> bool {
        self.cursor == self.input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_parser_handles_basic_expressions() {
        assert_eq!(eval_arithmetic("1+3"), Some(4.0));
        assert_eq!(eval_arithmetic("2 * (3 + 4)"), Some(14.0));
        assert_eq!(eval_arithmetic("10 / 2 - 1"), Some(4.0));
    }

    #[test]
    fn calculator_plugin_copies_result_text() {
        let results = CalculatorPlugin.query("1+3");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "4");
        assert!(results[0].pinned);
        assert!(matches!(results[0].action, PluginAction::CopyText(ref text) if text == "4"));
    }
}
