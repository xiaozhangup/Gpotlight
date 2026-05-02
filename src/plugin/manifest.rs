use super::{PluginAction, PluginRegistry, SearchPlugin, SearchResult};
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
}

#[derive(Debug, Deserialize)]
struct PluginResult {
    title: String,
    subtitle: Option<String>,
    icon: Option<String>,
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

    fn query(&self, query: &str) -> Vec<SearchResult> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }

        let output = match Command::new(&self.manifest.command)
            .args(self.args_for_query(query))
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
                action: match item.action.unwrap_or(PluginResultAction::Noop) {
                    PluginResultAction::OpenUri { uri } => PluginAction::OpenUri(uri),
                    PluginResultAction::CopyText { text } => PluginAction::CopyText(text),
                    PluginResultAction::Noop => PluginAction::Noop,
                },
            })
            .collect()
    }
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
