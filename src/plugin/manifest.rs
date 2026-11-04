use super::{
    PluginAction, PluginConfigChoice, PluginConfigItem, PluginConfigKind, PluginRegistry,
    SearchPlugin, SearchResult, SearchResultButton,
};
use crate::config::{AppConfig, PluginConfig};
use serde::Deserialize;
use std::fs;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const PLUGIN_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_PLUGIN_OUTPUT: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
struct PluginManifest {
    id: String,
    name: String,
    description: String,
    command: String,
    args: Option<Vec<String>>,
    #[serde(default)]
    preserve_order: bool,
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
    refresh_key: Option<String>,
    refresh_interval_ms: Option<u64>,
    action: Option<PluginResultAction>,
    buttons: Option<Vec<PluginResultButton>>,
}

#[derive(Debug, Deserialize)]
struct PluginResultButton {
    title: String,
    icon: Option<String>,
    close_on_activate: Option<bool>,
    refresh_after_ms: Option<u64>,
    action: PluginResultAction,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum PluginResultAction {
    OpenUri {
        uri: String,
    },
    CopyText {
        text: String,
    },
    LaunchCommand {
        command: String,
        args: Option<Vec<String>>,
    },
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
    manifest_dir: PathBuf,
}

impl ExternalCommandPlugin {
    fn from_manifest(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)?;
        let manifest = toml::from_str(&raw)?;
        let manifest_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Ok(Self {
            manifest,
            manifest_dir,
        })
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

    fn command_path(&self) -> PathBuf {
        let command = PathBuf::from(&self.manifest.command);
        if command.is_absolute() || self.manifest.command.contains('/') {
            return command;
        }

        let bundled = self.manifest_dir.join(&command);
        if bundled.exists() {
            bundled
        } else {
            command
        }
    }
}

impl SearchPlugin for ExternalCommandPlugin {
    fn preserve_order(&self) -> bool {
        self.manifest.preserve_order
    }

    fn is_blocking(&self) -> bool {
        true
    }

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
        let output = match bounded_output(
            Command::new(self.command_path())
                .args(self.args_for_query(query))
                .env("GPOTLIGHT_PLUGIN_CONFIG", plugin_config_json(config)),
            PLUGIN_TIMEOUT,
            MAX_PLUGIN_OUTPUT,
        ) {
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
                action: plugin_action(item.action.unwrap_or(PluginResultAction::Noop)),
                refresh_key: item.refresh_key,
                refresh_interval_ms: item.refresh_interval_ms,
                source_plugin_id: None,
                buttons: item
                    .buttons
                    .unwrap_or_default()
                    .into_iter()
                    .map(|button| SearchResultButton {
                        title: button.title,
                        icon: button.icon,
                        close_on_activate: button.close_on_activate.unwrap_or(true),
                        refresh_after_ms: button.refresh_after_ms,
                        action: plugin_action(button.action),
                    })
                    .collect(),
            })
            .collect()
    }
}

// Own the process until it has been reaped. On timeout, excess output, or a read
// error, terminate its process group as well: descendants can keep stdout open
// after the original command exits.
struct PluginProcess(Option<Child>);

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // SAFETY: the command was spawned into a new process group with its
            // own PID as group ID. A negative PID addresses that group only.
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn bounded_output(
    command: &mut Command,
    timeout: Duration,
    max_output: usize,
) -> io::Result<Output> {
    let deadline = Instant::now() + timeout;
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let mut process = PluginProcess(Some(child));
    let child = process.0.as_mut().expect("plugin process is running");
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let fd = stdout.as_raw_fd();
    // SAFETY: stdout owns a valid file descriptor for the entire read loop.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }

    let mut bytes = Vec::with_capacity(max_output.min(8192));
    let mut buffer = [0; 8192];
    let mut eof = false;
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "external plugin timed out",
            ));
        }
        if !eof {
            match stdout.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(count) => {
                    if count > max_output.saturating_sub(bytes.len()) {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "external plugin output exceeded the size limit",
                        ));
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    continue;
                }
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err),
            }
        }
        if eof {
            if let Some(status) = child.try_wait()? {
                process.0.take();
                return Ok(Output {
                    status,
                    stdout: bytes,
                    stderr: Vec::new(),
                });
            }
        }

        // Wait for pipe activity without allocating reader threads or polling a
        // quiet plugin continuously. After EOF, briefly wait for the exit status.
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = if eof {
            remaining.min(Duration::from_millis(10))
        } else {
            remaining
        };
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: descriptor points to a live pollfd, or nfds is zero after EOF.
        let result = unsafe {
            libc::poll(
                &mut descriptor,
                if eof { 0 } else { 1 },
                wait.as_millis().max(1).min(i32::MAX as u128) as i32,
            )
        };
        if result == -1 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
    }
}

fn plugin_action(action: PluginResultAction) -> PluginAction {
    match action {
        PluginResultAction::OpenUri { uri } => PluginAction::OpenUri(uri),
        PluginResultAction::CopyText { text } => PluginAction::CopyText(text),
        PluginResultAction::LaunchCommand { command, args } => PluginAction::LaunchCommand {
            command,
            args: args.unwrap_or_default(),
        },
        PluginResultAction::Noop => PluginAction::Noop,
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
    roots.push(PathBuf::from("/usr/share/gpotlight/plugins"));
    roots.push(PathBuf::from("plugins"));

    roots
        .into_iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_history_process_integrates_with_manifest_actions() {
        // The child owns an isolated HOME. Never open the user's real history.
        let script = r#"
import os, pathlib, runpy, sqlite3, sys, tempfile, time
with tempfile.TemporaryDirectory() as home:
    os.environ['HOME'] = home
    os.environ['XDG_CONFIG_HOME'] = str(pathlib.Path(home) / '.config')
    profile = pathlib.Path(home) / '.zen' / 'fixture'
    profile.mkdir(parents=True)
    connection = sqlite3.connect(profile / 'places.sqlite')
    connection.executescript('CREATE TABLE moz_places(url TEXT, title TEXT, last_visit_date INTEGER, hidden INTEGER); CREATE INDEX recent ON moz_places(last_visit_date);')
    connection.execute('INSERT INTO moz_places VALUES (?, ?, ?, 0)', ('https://example.test/docs', 'Fixture Docs', int(time.time() * 1000000) - 1000000))
    connection.commit()
    connection.close()
    plugin_path = sys.argv[2]
    sys.argv = [plugin_path, sys.argv[1]]
    runpy.run_path(plugin_path, run_name='__main__')
"#;
        let mut plugin = ExternalCommandPlugin::from_manifest(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/browser-history.toml"),
        )
        .unwrap();
        assert!(plugin.preserve_order());
        plugin.manifest.command = "python3".into();
        plugin.manifest.args = Some(vec![
            "-c".into(),
            script.into(),
            "{query}".into(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("plugins/browser_history.py")
                .to_string_lossy()
                .into_owned(),
        ]);
        let results = plugin.query("docs");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Fixture Docs");
        assert!(results[0].subtitle.contains("Zen"));
        assert_eq!(
            results[0].action,
            PluginAction::OpenUri("https://example.test/docs".into())
        );
        assert_eq!(
            results[0].buttons[0].action,
            PluginAction::CopyText("https://example.test/docs".into())
        );
        assert!(plugin.query("missing").is_empty());
    }

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn captures_stdout_with_closed_stdin_and_discarded_stderr() {
        let output = bounded_output(
            &mut shell("read line || printf 'stdin closed\\n'; printf 'ignored' >&2"),
            Duration::from_secs(2),
            1024,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"stdin closed\n");
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn preserves_unsuccessful_exit_status() {
        let output = bounded_output(&mut shell("exit 7"), Duration::from_secs(2), 1024).unwrap();
        assert_eq!(output.status.code(), Some(7));
    }

    #[test]
    fn stops_a_hung_plugin() {
        let start = Instant::now();
        let error = bounded_output(&mut shell("exec sleep 5"), Duration::from_millis(50), 1024)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn stops_descendants_that_keep_stdout_open() {
        let start = Instant::now();
        let error = bounded_output(
            &mut shell("sleep 5 & printf 'parent finished'"),
            Duration::from_millis(50),
            1024,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn rejects_unbounded_output() {
        let error = bounded_output(&mut shell("exec yes output"), Duration::from_secs(2), 1024)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn allows_output_exactly_at_the_limit() {
        let output =
            bounded_output(&mut shell("printf '1234'"), Duration::from_secs(2), 4).unwrap();
        assert_eq!(output.stdout, b"1234");
    }
}
