use super::{PluginAction, PluginRegistry, SearchPlugin, SearchResult};
use crate::config::AppConfig;
use crate::plugin::manifest::register_manifest_plugins;
use gio::prelude::*;

pub fn register_builtin_plugins(registry: &mut PluginRegistry, _config: &AppConfig) {
    registry.register(AppLauncherPlugin::load());
    registry.register(CalculatorPlugin);
    registry.register(WebSearchPlugin);
    register_manifest_plugins(registry);
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
                    id,
                    name,
                    comment,
                    icon: None,
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

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }

        self.apps
            .iter()
            .filter(|app| app.name.to_lowercase().contains(&needle))
            .take(8)
            .map(|app| SearchResult {
                title: app.name.clone(),
                subtitle: app.comment.clone(),
                icon: app.icon.clone(),
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

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let q = query.trim();
        if q.len() < 2 {
            return Vec::new();
        }

        vec![SearchResult {
            title: format!("Search the web for \"{q}\""),
            subtitle: "DuckDuckGo".to_string(),
            icon: Some("web-browser-symbolic".to_string()),
            action: PluginAction::OpenUri(format!(
                "https://duckduckgo.com/?q={}",
                q.replace(' ', "+")
            )),
        }]
    }
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
            match self.peek()? {
                b'+' => {
                    self.cursor += 1;
                    value += self.parse_term()?;
                }
                b'-' => {
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
            match self.peek()? {
                b'*' => {
                    self.cursor += 1;
                    value *= self.parse_factor()?;
                }
                b'/' => {
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
