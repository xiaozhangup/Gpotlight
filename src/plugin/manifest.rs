use super::{
    PluginAction, PluginConfigChoice, PluginConfigItem, PluginConfigKind, PluginRegistry,
    SearchPlugin, SearchResult,
};
use crate::config::{AppConfig, PluginConfig};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct PluginManifest {
    id: String,
    name: String,
    description: String,
    command: String,
    args: Option<Vec<String>>,
    config: Option<Vec<PluginManifestConfigItem>>,
}

#[derive(Debug, Deserialize)]
struct PluginManifestConfigItem {
    key: String,
    title: String,
    description: Option<String>,
    #[serde(flatten)]
    kind: PluginManifestConfigKind,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum PluginManifestConfigKind {
    Bool {
        default: Option<bool>,
    },
    Text {
        default: Option<String>,
    },
    Choice {
        default: Option<String>,
        options: Vec<PluginManifestConfigChoice>,
    },
    Integer {
        default: Option<i64>,
        min: Option<i64>,
        max: Option<i64>,
        step: Option<i64>,
    },
}

#[derive(Debug, Deserialize)]
struct PluginManifestConfigChoice {
    value: String,
    label: String,
}

#[derive(Debug, Deserialize)]
struct PluginResult {
    title: String,
    subtitle: Option<String>,
    icon: Option<String>,
    pinned: Option<bool>,
    action: Option<PluginResultAction>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum PluginResultAction {
    OpenUri { uri: String },
    CopyText { text: String },
    Noop,
}

pub fn register_manifest_plugins(registry: &mut PluginRegistry) {
    for path in plugin_manifest_paths() {
        match ExternalCommandPlugin::from_manifest(&path) {
            Ok(plugin) => registry.register(plugin),
            Err(err) => {
                tracing::warn!(error = ?err, path = %path.display(), "failed to load plugin")
            }
        }
    }
}

struct ExternalCommandPlugin {
    manifest: PluginManifest,
}

impl ExternalCommandPlugin {
    fn from_manifest(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)?;
        let manifest = toml::from_str(&raw)?;
        Ok(Self { manifest })
    }

    fn args_for_query(&self, query: &str) -> Vec<String> {
        self.manifest
            .args
            .clone()
            .unwrap_or_else(|| vec!["{query}".to_string()])
            .into_iter()
            .map(|arg| arg.replace("{query}", query))
            .collect()
    }
}

impl SearchPlugin for ExternalCommandPlugin {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn description(&self) -> &str {
        &self.manifest.description
    }

    fn config_items(&self) -> Vec<PluginConfigItem> {
        self.manifest
            .config
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .map(|item| match &item.kind {
                        PluginManifestConfigKind::Bool { default } => PluginConfigItem {
                            key: item.key.clone(),
                            title: item.title.clone(),
                            description: item.description.clone().unwrap_or_default(),
                            kind: PluginConfigKind::Bool,
                            default_value: toml::Value::Boolean(default.unwrap_or(false)),
                        },
                        PluginManifestConfigKind::Text { default } => PluginConfigItem {
                            key: item.key.clone(),
                            title: item.title.clone(),
                            description: item.description.clone().unwrap_or_default(),
                            kind: PluginConfigKind::Text,
                            default_value: toml::Value::String(default.clone().unwrap_or_default()),
                        },
                        PluginManifestConfigKind::Choice { default, options } => PluginConfigItem {
                            key: item.key.clone(),
                            title: item.title.clone(),
                            description: item.description.clone().unwrap_or_default(),
                            kind: PluginConfigKind::Choice {
                                options: options
                                    .iter()
                                    .map(|option| PluginConfigChoice {
                                        value: option.value.clone(),
                                        label: option.label.clone(),
                                    })
                                    .collect(),
                            },
                            default_value: toml::Value::String(default.clone().unwrap_or_default()),
                        },
                        PluginManifestConfigKind::Integer {
                            default,
                            min,
                            max,
                            step,
                        } => PluginConfigItem {
                            key: item.key.clone(),
                            title: item.title.clone(),
                            description: item.description.clone().unwrap_or_default(),
                            kind: PluginConfigKind::Integer {
                                min: min.unwrap_or(0),
                                max: max.unwrap_or(100),
                                step: step.unwrap_or(1),
                            },
                            default_value: toml::Value::Integer(default.unwrap_or(0)),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
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
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let output = match Command::new(&self.manifest.command)
            .args(self.args_for_query(query))
            .env("GPOTLIGHT_PLUGIN_CONFIG", plugin_config_json(config))
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                tracing::warn!(
                    plugin_id = self.manifest.id,
                    status = ?output.status,
                    "external plugin exited unsuccessfully"
                );
                return Vec::new();
            }
            Err(err) => {
                tracing::warn!(
                    error = ?err,
                    plugin_id = self.manifest.id,
                    "failed to execute external plugin"
                );
                return Vec::new();
            }
        };

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| serde_json::from_str::<PluginResult>(line).ok())
            .map(|item| SearchResult {
                title: item.title,
                subtitle: item.subtitle.unwrap_or_default(),
                icon: item.icon,
                pinned: item.pinned.unwrap_or(false),
                action: match item.action.unwrap_or(PluginResultAction::Noop) {
                    PluginResultAction::OpenUri { uri } => PluginAction::OpenUri(uri),
                    PluginResultAction::CopyText { text } => PluginAction::CopyText(text),
                    PluginResultAction::Noop => PluginAction::Noop,
                },
            })
            .collect()
    }
}

fn plugin_config_json(config: &PluginConfig) -> String {
    let custom: serde_json::Map<String, serde_json::Value> = config
        .custom
        .iter()
        .map(|(key, value)| {
            let value = match value {
                toml::Value::String(value) => serde_json::Value::String(value.clone()),
                toml::Value::Integer(value) => serde_json::Value::Number((*value).into()),
                toml::Value::Float(value) => serde_json::Number::from_f64(*value)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                toml::Value::Boolean(value) => serde_json::Value::Bool(*value),
                _ => serde_json::Value::Null,
            };
            (key.clone(), value)
        })
        .collect();

    serde_json::Value::Object(custom).to_string()
}

fn plugin_manifest_paths() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(config_dir) = dirs::config_dir() {
        roots.push(config_dir.join("gpotlight").join("plugins"));
    }
    roots.push(PathBuf::from("plugins"));

    roots
        .into_iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect()
}
