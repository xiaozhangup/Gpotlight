mod app_launcher;
mod calculator;
mod system_actions;
mod web_search;

use crate::plugin::manifest::register_manifest_plugins;
use crate::plugin::PluginRegistry;

use app_launcher::AppLauncherPlugin;
use calculator::CalculatorPlugin;
use system_actions::SystemActionsPlugin;
use web_search::WebSearchPlugin;

pub fn register_builtin_plugins(registry: &mut PluginRegistry) {
    registry.register(SystemActionsPlugin);
    registry.register(AppLauncherPlugin::load());
    registry.register(CalculatorPlugin);
    registry.register(WebSearchPlugin);
    register_manifest_plugins(registry);
}
