use crate::config::{AppConfig, PluginConfig};
use crate::plugin::{
    PluginAction, PluginConfigChoice, PluginConfigItem, PluginConfigKind, SearchPlugin,
    SearchResult,
};

pub(super) struct WebSearchPlugin;

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
            buttons: Vec::new(),
            refresh_key: None,
            refresh_interval_ms: None,
            source_plugin_id: None,
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
