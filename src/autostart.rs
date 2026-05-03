use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

const AUTOSTART_FILE: &str = "io.github.gpotlight.Gpotlight.desktop";

pub fn is_enabled() -> bool {
    autostart_path().is_ok_and(|path| path.exists())
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let path = autostart_path()?;
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, desktop_entry()).context("failed to write autostart desktop entry")?;
    } else if path.exists() {
        fs::remove_file(path).context("failed to remove autostart desktop entry")?;
    }

    Ok(())
}

fn autostart_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("XDG config directory is unavailable")?;
    Ok(base.join("autostart").join(AUTOSTART_FILE))
}

fn desktop_entry() -> &'static str {
    r#"[Desktop Entry]
Type=Application
Name=Gpotlight
Comment=Spotlight-style launcher for GNOME
Exec=sh -c 'setsid gpotlight >/dev/null 2>&1 &'
Icon=io.github.gpotlight.Gpotlight
Categories=Utility;GTK;
StartupNotify=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
"#
}
