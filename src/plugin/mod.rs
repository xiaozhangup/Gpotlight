pub mod builtin;
pub mod manifest;

#[cfg(test)]
use crate::config::ConfigStore;
use crate::config::{AppConfig, PluginConfig};
use gio::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub title: String,
    pub subtitle: String,
    pub icon: Option<String>,
    pub pinned: bool,
    pub action: PluginAction,
    pub buttons: Vec<SearchResultButton>,
    pub refresh_key: Option<String>,
    pub refresh_interval_ms: Option<u64>,
    pub source_plugin_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResultButton {
    pub title: String,
    pub icon: Option<String>,
    pub action: PluginAction,
    pub close_on_activate: bool,
    pub refresh_after_ms: Option<u64>,
}

pub struct PluginResults {
    pub plugin_id: String,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginAction {
    LaunchDesktopFile(String),
    OpenUri(String),
    CopyText(String),
    AppAction(String),
    LaunchCommand { command: String, args: Vec<String> },
    Noop,
}

pub trait SearchPlugin {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn query(&self, query: &str) -> Vec<SearchResult>;

    /// Whether queries may wait for processes, disk, or network I/O.
    /// In-memory plugins run immediately; blocking plugins run on the search worker.
    fn is_blocking(&self) -> bool {
        false
    }

    /// Preserve a provider's meaningful ordering, such as visit recency.
    fn preserve_order(&self) -> bool {
        false
    }

    fn query_with_config(
        &self,
        query: &str,
        config: &PluginConfig,
        app_config: &AppConfig,
    ) -> Vec<SearchResult> {
        let _ = (config, app_config);
        self.query(query)
    }

    fn config_items(&self) -> Vec<PluginConfigItem> {
        Vec::new()
    }

    fn query_cancellable(
        &self,
        query: &str,
        config: &PluginConfig,
        app_config: &AppConfig,
        _cancellable: &gio::Cancellable,
    ) -> Vec<SearchResult> {
        self.query_with_config(query, config, app_config)
    }
}

#[derive(Clone, Default)]
pub struct PluginRegistry {
    plugins: Vec<Arc<dyn SearchPlugin + Send + Sync>>,
}

impl PluginRegistry {
    pub fn register<P>(&mut self, plugin: P)
    where
        P: SearchPlugin + Send + Sync + 'static,
    {
        self.plugins.push(Arc::new(plugin));
    }

    pub fn plugin_metadata(&self) -> Vec<PluginMetadata> {
        self.plugins
            .iter()
            .map(|plugin| PluginMetadata {
                id: plugin.id().to_string(),
                name: plugin.name().to_string(),
                description: plugin.description().to_string(),
                config_items: plugin.config_items(),
            })
            .collect()
    }

    #[cfg(test)]
    pub fn search(&self, config: &ConfigStore, query: &str) -> Vec<SearchResult> {
        self.search_with_config(config.current(), query)
    }

    #[cfg(test)]
    pub fn search_with_config(&self, config: &AppConfig, query: &str) -> Vec<SearchResult> {
        let mut results = self.search_immediate(config, query);
        results.extend(self.search_blocking(config, query));
        self.rank_results(config, results)
    }

    /// Query only plugins that can answer from memory, without creating threads.
    pub fn search_immediate(&self, config: &AppConfig, query: &str) -> Vec<SearchResult> {
        let scoped = self.has_triggered_plugin(config, query);
        let query_is_empty = query.trim().is_empty();
        self.plugins
            .iter()
            .filter(|plugin| !plugin.is_blocking())
            .filter_map(|plugin| {
                let plugin_config = config.plugin_config(plugin.id());
                let plugin_query = if scoped {
                    plugin_config.prefixed_query(query)?
                } else {
                    query_for_plugin(plugin_config, query)?
                };
                Some((plugin, plugin_config, plugin_query))
            })
            .flat_map(|(plugin, plugin_config, plugin_query)| {
                self.query_plugin(plugin.as_ref(), plugin_query, plugin_config, config, None)
            })
            .filter(|result| !query_is_empty || result.pinned)
            .collect()
    }

    #[cfg(test)]
    pub fn search_blocking(&self, config: &AppConfig, query: &str) -> Vec<SearchResult> {
        let results = std::sync::Mutex::new(Vec::new());
        self.search_blocking_stream(config, query, &gio::Cancellable::new(), |batch| {
            results.lock().unwrap().extend(batch.results);
        });
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|result| {
            self.plugins
                .iter()
                .position(|plugin| Some(plugin.id()) == result.source_plugin_id.as_deref())
        });
        results
    }

    /// Called off the GTK main thread. Deliver each plugin as it finishes,
    /// keeping at most four commands active until cancellation is reaped.
    pub fn search_blocking_stream(
        &self,
        config: &AppConfig,
        query: &str,
        cancellable: &gio::Cancellable,
        publish: impl Fn(PluginResults) + Sync,
    ) {
        let scoped = self.has_triggered_plugin(config, query);
        let calls: Vec<_> = self
            .plugins
            .iter()
            .filter(|plugin| plugin.is_blocking())
            .filter_map(|plugin| {
                let plugin_config = config.plugin_config(plugin.id());
                let plugin_query = if scoped {
                    plugin_config.prefixed_query(query)?
                } else {
                    query_for_plugin(plugin_config, query)?
                };
                Some((plugin, plugin_config, plugin_query))
            })
            .collect();
        if calls.is_empty() {
            return;
        }

        // Bound CPU/process pressure even when many external plugins are installed.
        const MAX_CONCURRENT_PLUGINS: usize = 4;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let query_is_empty = query.trim().is_empty();
        let run = || loop {
            if cancellable.is_cancelled() {
                break;
            }
            let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some((plugin, plugin_config, plugin_query)) = calls.get(index) else {
                break;
            };
            let mut results = self.query_plugin(
                plugin.as_ref(),
                plugin_query,
                plugin_config,
                config,
                Some(cancellable),
            );
            if cancellable.is_cancelled() {
                break;
            }
            results.retain(|result| !query_is_empty || result.pinned);
            publish(PluginResults {
                plugin_id: plugin.id().to_string(),
                results,
            });
        };
        std::thread::scope(|scope| {
            for _ in 1..calls.len().min(MAX_CONCURRENT_PLUGINS) {
                scope.spawn(run);
            }
            run();
        });
    }

    fn has_triggered_plugin(&self, config: &AppConfig, query: &str) -> bool {
        self.plugins.iter().any(|plugin| {
            config
                .plugin_config(plugin.id())
                .prefixed_query(query)
                .is_some()
        })
    }

    pub fn rank_results(
        &self,
        config: &AppConfig,
        mut results: Vec<SearchResult>,
    ) -> Vec<SearchResult> {
        let plugin_order: std::collections::HashMap<_, _> = self
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| (plugin.id(), (index, plugin.preserve_order())))
            .collect();
        // Each usage key is allocated and looked up once, rather than twice per
        // sort comparison. Include plugin order so separately delivered batches
        // retain the original ordering when their ranking scores are tied.
        results.sort_by_cached_key(|result| {
            let (index, preserve_order) = result
                .source_plugin_id
                .as_deref()
                .and_then(|id| plugin_order.get(id).copied())
                .unwrap_or((usize::MAX, false));
            let usage =
                if !preserve_order && config.usage_ranking_enabled && !config.usage.is_empty() {
                    config.usage.get(&result.usage_key()).copied().unwrap_or(0)
                } else {
                    0
                };
            (
                std::cmp::Reverse(result.pinned),
                std::cmp::Reverse(usage),
                index,
            )
        });
        results.truncate(100);
        results
    }

    #[cfg(test)]
    pub fn search_plugin_with_config(
        &self,
        config: &AppConfig,
        plugin_id: &str,
        query: &str,
    ) -> Vec<SearchResult> {
        self.search_plugin_cancellable(config, plugin_id, query, &gio::Cancellable::new())
    }

    pub fn search_plugin_cancellable(
        &self,
        config: &AppConfig,
        plugin_id: &str,
        query: &str,
        cancellable: &gio::Cancellable,
    ) -> Vec<SearchResult> {
        let Some(plugin) = self.plugins.iter().find(|plugin| plugin.id() == plugin_id) else {
            return Vec::new();
        };
        let plugin_config = config.plugin_config(plugin.id());
        if !plugin_config.enabled {
            return Vec::new();
        }
        self.query_plugin(
            plugin.as_ref(),
            query,
            plugin_config,
            config,
            Some(cancellable),
        )
    }

    fn query_plugin(
        &self,
        plugin: &dyn SearchPlugin,
        query: &str,
        plugin_config: &PluginConfig,
        app_config: &AppConfig,
        cancellable: Option<&gio::Cancellable>,
    ) -> Vec<SearchResult> {
        let mut results =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match cancellable {
                Some(cancellable) => {
                    plugin.query_cancellable(query, plugin_config, app_config, cancellable)
                }
                None => plugin.query_with_config(query, plugin_config, app_config),
            }))
            .unwrap_or_else(|_| {
                tracing::warn!(
                    plugin_id = plugin.id(),
                    "plugin query panicked, skipping its results"
                );
                Vec::new()
            });
        for result in &mut results {
            result.source_plugin_id = Some(plugin.id().to_string());
        }
        results
    }
}

// Include empty global queries for pinned cards, while using the same prefix
// routing and per-plugin defaults as the settings window and refresh requests.
fn query_for_plugin<'a>(config: &PluginConfig, query: &'a str) -> Option<&'a str> {
    config.query(query).or_else(|| {
        (config.enabled && config.show_in_global_search && query.trim().is_empty()).then_some("")
    })
}

#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub description: String,
    pub config_items: Vec<PluginConfigItem>,
}

#[derive(Debug, Clone)]
pub struct PluginConfigItem {
    pub key: String,
    pub title: String,
    pub description: String,
    pub kind: PluginConfigKind,
    pub default_value: toml::Value,
}

#[derive(Debug, Clone)]
pub enum PluginConfigKind {
    Bool,
    Text,
    Choice { options: Vec<PluginConfigChoice> },
    Integer { min: i64, max: i64, step: i64 },
}

#[derive(Debug, Clone)]
pub struct PluginConfigChoice {
    pub value: String,
    pub label: String,
}

impl SearchResult {
    pub fn usage_key(&self) -> String {
        match &self.action {
            PluginAction::Noop => format!("noop:{}:{}", self.title, self.subtitle),
            _ => self.action.usage_key_fragment(),
        }
    }
}

impl SearchResultButton {
    pub fn usage_key(&self, result: &SearchResult) -> String {
        format!(
            "button:{}:{}:{}",
            result.usage_key(),
            self.title,
            self.action.usage_key_fragment()
        )
    }
}

impl PluginAction {
    fn usage_key_fragment(&self) -> String {
        match self {
            PluginAction::LaunchDesktopFile(desktop_id) => format!("desktop:{desktop_id}"),
            PluginAction::OpenUri(uri) => format!("uri:{uri}"),
            PluginAction::CopyText(text) => format!("copy:{text}"),
            PluginAction::AppAction(action) => format!("app-action:{action}"),
            PluginAction::LaunchCommand { command, args } => {
                format!("command:{}:{}", command, args.join("\u{1f}"))
            }
            PluginAction::Noop => "noop".to_string(),
        }
    }
}

pub fn activate_result(result: &SearchResult, window: &gtk::Window) {
    activate_action(&result.action, window, true);
}

pub fn activate_action(action: &PluginAction, window: &gtk::Window, close_window: bool) {
    match action {
        PluginAction::LaunchDesktopFile(desktop_id) => {
            let ctx = gio::AppLaunchContext::new();
            if let Some(info) = gio::AppInfo::all().into_iter().find(|app| {
                app.id()
                    .map(|id| id.as_str() == desktop_id)
                    .unwrap_or(false)
            }) {
                if let Err(err) = info.launch(&[], Some(&ctx)) {
                    tracing::warn!(error = ?err, desktop_id, "failed to launch desktop file");
                }
            }
        }
        PluginAction::OpenUri(uri) => {
            // Some URI launchers wait for the browser process. Keep that wait
            // off GTK so activating a history result dismisses the window promptly.
            let uri = uri.clone();
            std::thread::spawn(move || {
                if let Err(err) = open::that(&uri) {
                    tracing::warn!(error = ?err, uri, "failed to open uri");
                }
            });
        }
        PluginAction::CopyText(text) => {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(text);
            }
        }
        PluginAction::AppAction(action) => {
            if let Some(app) = window.application() {
                app.activate_action(action, None);
            }
        }
        PluginAction::LaunchCommand { command, args } => {
            if let Err(err) = Command::new("setsid").arg(command).args(args).spawn() {
                tracing::warn!(error = ?err, command, args = ?args, "failed to launch command");
            }
        }
        PluginAction::Noop => {}
    }

    if close_window {
        window.set_visible(false);
    }
}

pub type SharedRegistry = Rc<RefCell<PluginRegistry>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigStore;

    #[test]
    fn browser_history_is_only_queried_after_its_prefix_by_default() {
        let queries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "bundled.browser-history",
            blocking: true,
            queries: Some(queries.clone()),
            ..StaticPlugin::default()
        });
        let config = ConfigStore::from_config_for_test(AppConfig::default());
        for query in ["", "hello", "example.com"] {
            assert!(registry.search(&config, query).is_empty());
        }
        assert_eq!(queries.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(
            registry.search(&config, "h")[0].title,
            "bundled.browser-history:"
        );
        assert_eq!(
            registry.search(&config, "h 文档")[0].title,
            "bundled.browser-history:文档"
        );
        assert_eq!(queries.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    #[test]
    fn history_prefix_excludes_unrelated_global_results_and_processes() {
        let queries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "global",
            queries: Some(queries.clone()),
            ..StaticPlugin::default()
        });
        registry.register(StaticPlugin {
            id: "slow-global",
            blocking: true,
            queries: Some(queries.clone()),
            ..StaticPlugin::default()
        });
        registry.register(StaticPlugin {
            id: "bundled.browser-history",
            blocking: true,
            ..StaticPlugin::default()
        });
        let mut config = AppConfig::default();
        for query in ["h", "h docs"] {
            let results = registry.search_with_config(&config, query);
            assert_eq!(results.len(), 1);
            assert_eq!(
                results[0].source_plugin_id.as_deref(),
                Some("bundled.browser-history")
            );
        }
        assert_eq!(queries.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(registry.search_with_config(&config, "hello").len(), 2);
        config.plugin_config_mut("bundled.browser-history").enabled = false;
        assert_eq!(registry.search_with_config(&config, "h").len(), 2);
    }

    #[derive(Default)]
    struct StaticPlugin {
        id: &'static str,
        pinned: bool,
        blocking: bool,
        preserve_order: bool,
        queries: Option<Arc<std::sync::atomic::AtomicUsize>>,
    }

    impl SearchPlugin for StaticPlugin {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> &str {
            self.id
        }

        fn description(&self) -> &str {
            self.id
        }

        fn is_blocking(&self) -> bool {
            self.blocking
        }

        fn preserve_order(&self) -> bool {
            self.preserve_order
        }

        fn query(&self, query: &str) -> Vec<SearchResult> {
            if let Some(queries) = &self.queries {
                queries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            vec![SearchResult {
                title: format!("{}:{query}", self.id),
                subtitle: String::new(),
                icon: None,
                pinned: self.pinned,
                action: PluginAction::Noop,
                buttons: Vec::new(),
                refresh_key: None,
                refresh_interval_ms: None,
                source_plugin_id: None,
            }]
        }
    }

    #[test]
    fn empty_search_only_keeps_pinned_results() {
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "normal",
            pinned: false,
            ..Default::default()
        });
        registry.register(StaticPlugin {
            id: "pinned",
            pinned: true,
            ..Default::default()
        });
        let config = ConfigStore::from_config_for_test(AppConfig::default());

        let results = registry.search(&config, "");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "pinned:");
    }

    #[test]
    fn immediate_search_defers_blocking_work_and_merge_retains_plugin_order() {
        let queries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "blocking",
            blocking: true,
            queries: Some(queries.clone()),
            ..Default::default()
        });
        registry.register(StaticPlugin {
            id: "immediate",
            ..Default::default()
        });
        let config = AppConfig::default();
        let mut results = registry.search_immediate(&config, "abc");
        assert_eq!(results[0].title, "immediate:abc");
        assert_eq!(queries.load(std::sync::atomic::Ordering::Relaxed), 0);

        // A cloned registry is cheap to move to a worker and still shares plugins.
        let worker_registry = registry.clone();
        let worker_config = config.clone();
        results.extend(
            std::thread::spawn(move || worker_registry.search_blocking(&worker_config, "abc"))
                .join()
                .unwrap(),
        );
        let results = registry.rank_results(&config, results);
        assert_eq!(queries.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(results[0].title, "blocking:abc");
        assert_eq!(results[1].title, "immediate:abc");
    }

    #[test]
    fn split_search_respects_disabled_plugins_and_trigger_prefixes() {
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "disabled",
            ..Default::default()
        });
        registry.register(StaticPlugin {
            id: "prefixed",
            blocking: true,
            ..Default::default()
        });
        let mut config = AppConfig::default();
        config.plugins.insert(
            "disabled".into(),
            PluginConfig {
                enabled: false,
                ..Default::default()
            },
        );
        config.plugins.insert(
            "prefixed".into(),
            PluginConfig {
                show_in_global_search: false,
                trigger_prefix: "/".into(),
                ..Default::default()
            },
        );
        assert!(registry.search_with_config(&config, "abc").is_empty());
        assert!(registry.search_with_config(&config, "").is_empty());
        assert_eq!(
            registry.search_with_config(&config, "/")[0].title,
            "prefixed:"
        );
        assert_eq!(
            registry.search_with_config(&config, "/ abc")[0].title,
            "prefixed:abc"
        );
        assert!(registry
            .search_plugin_with_config(&config, "disabled", "abc")
            .is_empty());
    }

    #[test]
    fn ranking_preserves_pinning_usage_and_stable_ties() {
        let mut registry = PluginRegistry::default();
        for (id, pinned) in [("first", false), ("popular", false), ("pinned", true)] {
            registry.register(StaticPlugin {
                id,
                pinned,
                ..Default::default()
            });
        }
        let mut config = AppConfig::default();
        config.usage.insert("noop:popular:abc:".into(), 10);
        let results = registry.search_with_config(&config, "abc");
        let titles: Vec<_> = results.iter().map(|result| result.title.as_str()).collect();
        assert_eq!(titles, ["pinned:abc", "popular:abc", "first:abc"]);

        config.usage_ranking_enabled = false;
        let results = registry.search_with_config(&config, "abc");
        let titles: Vec<_> = results.iter().map(|result| result.title.as_str()).collect();
        assert_eq!(titles, ["pinned:abc", "first:abc", "popular:abc"]);
    }

    #[test]
    fn provider_recency_order_survives_global_usage_ranking() {
        let mut registry = PluginRegistry::default();
        registry.register(StaticPlugin {
            id: "bundled.browser-history",
            preserve_order: true,
            ..Default::default()
        });
        let mut recent = StaticPlugin::default().query("newest").remove(0);
        recent.source_plugin_id = Some("bundled.browser-history".into());
        let mut older = recent.clone();
        older.title = "older".into();
        let mut config = AppConfig::default();
        config.usage.insert(older.usage_key(), 100);
        assert_eq!(
            registry.rank_results(&config, vec![recent.clone(), older])[0],
            recent
        );
    }

    #[test]
    fn blocking_search_limits_parallel_work_without_dropping_results() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct BlockingPlugin {
            id: String,
            active: Arc<AtomicUsize>,
            peak: Arc<AtomicUsize>,
        }

        impl SearchPlugin for BlockingPlugin {
            fn id(&self) -> &str {
                &self.id
            }

            fn name(&self) -> &str {
                &self.id
            }

            fn description(&self) -> &str {
                &self.id
            }

            fn is_blocking(&self) -> bool {
                true
            }

            fn query(&self, query: &str) -> Vec<SearchResult> {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(active, Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(10));
                self.active.fetch_sub(1, Ordering::SeqCst);
                StaticPlugin::default().query(query)
            }
        }

        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut registry = PluginRegistry::default();
        for index in 0..12 {
            registry.register(BlockingPlugin {
                id: format!("plugin-{index}"),
                active: active.clone(),
                peak: peak.clone(),
            });
        }

        let results = registry.search_blocking(&AppConfig::default(), "abc");
        assert_eq!(results.len(), 12);
        assert!(peak.load(Ordering::SeqCst) <= 4);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result.source_plugin_id, Some(format!("plugin-{index}")));
        }
    }
}
