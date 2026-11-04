# Gpotlight

Gpotlight is a Spotlight-style launcher for GNOME 50 on Wayland, built with Rust, GTK4, and libadwaita.

The app uses a large transparent GTK window as a centered container. GNOME centers that window, while the visible launcher surface is rendered near the upper center inside it. This avoids compositor-specific positioning hacks on Wayland.

On GTK 4.24 and a Wayland compositor supporting `ext-background-effect-v1` (such as GNOME 51), the launcher requests background blur behind its translucent panel. GTK 4.22 uses a more opaque panel without blur.

## Current Architecture

- GTK4/libadwaita UI with a transparent host window and visible launcher panel.
- Config file stored under `$XDG_CONFIG_HOME/gpotlight/config.toml`.
- Plugin registry with built-in plugins and an extension point for external plugins.
- Global shortcut portal adapter for `org.freedesktop.portal.GlobalShortcuts`.
- Status notifier tray entry for opening settings and toggling the launcher.
- Lightweight i18n resources in `app/resources/locale`.

## Build

On Fedora 44 / GNOME 50, install the compiler and development libraries:

```sh
sudo dnf install cargo rust gcc pkgconf-pkg-config gtk4-devel libadwaita-devel glib2-devel
```

GTK **4.22+** and libadwaita **1.9+** are required. For the bundled Python
plugins and RPM packaging, also install:

```sh
sudo dnf install python3 python3-gobject rpm-build
```

Build the optimized executable for normal use:

```sh
cargo build --release --locked
./target/release/gpotlight
```

`cargo build --locked` is also available for development, but its unoptimized
binary is not suitable for performance comparisons.

Runtime dependencies expected on GNOME:

- GTK 4
- libadwaita
- A StatusNotifierItem/AppIndicator shell extension if your GNOME session does not show tray icons by default

## Run

```sh
cargo run --release --locked
```

Toggle a running instance without starting a new UI:

```sh
gpotlight toggle
```

To use a global keyboard shortcut, create a custom shortcut in GNOME Settings
and set its command to:

```sh
gpotlight toggle
```

Open settings in a running instance without starting a new UI:

```sh
gpotlight settings
```

For GNOME portal permissions when running a debug build manually, install the
desktop entry after copying the binary to a directory on your `PATH`:

```sh
install -Dm755 target/debug/gpotlight ~/.local/bin/gpotlight
install -Dm644 data/io.github.gpotlight.Gpotlight.desktop ~/.local/share/applications/io.github.gpotlight.Gpotlight.desktop
update-desktop-database ~/.local/share/applications
```

## RPM

Build a Fedora RPM:

```sh
./scripts/build-rpm.sh
```

The package is written under `target/rpmbuild/RPMS/`.

## Plugin Manifests

External plugins can be placed in either:

- `plugins/*.toml` during development
- `/usr/share/gpotlight/plugins/*.toml` for package-installed plugins
- `$XDG_CONFIG_HOME/gpotlight/plugins/*.toml` for user-installed plugins

Each plugin is a command that receives the query and prints JSON lines:

```toml
id = "example.web"
name = "Example Web Plugin"
description = "Searches a custom service"
command = "my-gpotlight-plugin"
args = ["--query", "{query}"]

[[config]]
key = "safe_mode"
title = "Safe mode"
description = "Filter potentially noisy results"
type = "bool"
default = true

[[config]]
key = "endpoint"
title = "Endpoint"
type = "text"
default = "https://example.com"

[[config]]
key = "provider"
title = "Provider"
type = "choice"
default = "primary"
options = [
  { value = "primary", label = "Primary" },
  { value = "backup", label = "Backup" },
]
```

Each output line should look like:

```json
{"title":"Result","subtitle":"Details","icon":"system-search-symbolic","pinned":false,"action":{"type":"open-uri","uri":"https://example.com"}}
```

Gpotlight-owned plugin settings are configured in the settings window. Every
plugin can be shown directly in global search or gated behind a trigger prefix
such as `/` or `{`. External plugin custom settings are passed to the plugin
process as JSON in `GPOTLIGHT_PLUGIN_CONFIG`.

## Performance

Use a release build for daily use. Built-in search updates immediately; external
plugins run in a bounded background queue. The launcher has no periodic IPC or
tray polling while idle. See [performance measurements and validation](PERFORMANCE.md)
for the changes, benchmark limitations, and reproducible checks.

## Browser history

Type `h` to show recently visited pages, or `h rust docs` to search titles and
URLs. Press Enter to open a page in the default browser; use its copy button to
copy the URL. The **Browser History** plugin supports Google Chrome, Chromium,
Microsoft Edge, Brave, Vivaldi, Opera, Firefox, and Zen Browser, including multiple
profiles and the supported native/Flatpak profile locations.

In Settings → Plugins → Browser History, choose the browsers to search, change
the trigger prefix, or enable matching in global search. History search defaults
to the last 30 days, up to 5,000 recent records per profile, with at most 20
results. These limits are configurable. Matching is case-insensitive and checks
both page titles and URLs; duplicate URLs are merged across profiles.

The plugin opens local history databases read-only. When a running browser holds
an exclusive lock, it falls back to a private temporary copy of the database and
its WAL, then removes the copy after querying. It never writes to the browser's
history or uploads it. Copies with detected source changes are retried within
the time budget; busy, oversized, inaccessible, or unsupported profiles may be
skipped so they do not block other results. Private browsing pages
that the browser did not record cannot appear. See [plugin details](plugins/README.md)
for profile discovery and validation.
