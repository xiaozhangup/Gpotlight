# Gpotlight

Gpotlight is a Spotlight-style launcher for GNOME 50 on Wayland, built with Rust and GTK4.

The app uses a large transparent GTK window as a centered container. GNOME centers that window, while the visible launcher surface is rendered near the upper center inside it. This avoids compositor-specific positioning hacks on Wayland.

## Current Architecture

- GTK4 UI with a transparent host window and visible launcher panel.
- Config file stored under `$XDG_CONFIG_HOME/gpotlight/config.toml`.
- Plugin registry with built-in plugins and an extension point for external plugins.
- Global shortcut portal adapter for `org.freedesktop.portal.GlobalShortcuts`.
- Status notifier tray entry for opening settings and toggling the launcher.
- Lightweight i18n resources in `app/resources/locale`.

## Build

```sh
cargo build
```

Runtime dependencies expected on GNOME:

- GTK 4
- xdg-desktop-portal with GlobalShortcuts support
- A StatusNotifierItem/AppIndicator shell extension if your GNOME session does not show tray icons by default

## Run

```sh
cargo run
```

Toggle a running instance without starting a new UI:

```sh
gpotlight toggle
```

For GNOME portal permissions, install the desktop entry after copying the binary
to a directory on your `PATH`:

```sh
install -Dm755 target/debug/gpotlight ~/.local/bin/gpotlight
install -Dm644 data/io.github.gpotlight.Gpotlight.desktop ~/.local/share/applications/io.github.gpotlight.Gpotlight.desktop
update-desktop-database ~/.local/share/applications
```

## Plugin Manifests

External plugins can be placed in either:

- `plugins/*.toml` during development
- `$XDG_CONFIG_HOME/gpotlight/plugins/*.toml` for user-installed plugins

Each plugin is a command that receives the query and prints JSON lines:

```toml
id = "example.web"
name = "Example Web Plugin"
description = "Searches a custom service"
command = "my-gpotlight-plugin"
args = ["--query", "{query}"]
```

Each output line should look like:

```json
{"title":"Result","subtitle":"Details","icon":"system-search-symbolic","action":{"type":"open-uri","uri":"https://example.com"}}
```
