use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

pub const APP_ID: &str = "io.github.gpotlight.Gpotlight";

pub fn ensure_user_desktop_entry() -> Result<()> {
    let Some(data_dir) = dirs::data_local_dir() else {
        return Ok(());
    };

    let desktop_dir = data_dir.join("applications");
    let desktop_path = desktop_dir.join(format!("{APP_ID}.desktop"));
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let content = desktop_entry_content(&current_exe);

    if fs::read_to_string(&desktop_path).ok().as_deref() == Some(content.as_str()) {
        return Ok(());
    }

    fs::create_dir_all(&desktop_dir)
        .with_context(|| format!("failed to create {}", desktop_dir.display()))?;
    fs::write(&desktop_path, content)
        .with_context(|| format!("failed to write {}", desktop_path.display()))?;

    tracing::info!(
        desktop_entry = %desktop_path.display(),
        "updated user desktop entry for portal app id"
    );
    Ok(())
}

fn desktop_entry_content(exec: &PathBuf) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Gpotlight\n\
         Comment=Spotlight-style launcher for GNOME\n\
         Exec={}\n\
         Icon=system-search\n\
         Categories=Utility;\n\
         StartupNotify=false\n\
         DBusActivatable=false\n",
        shell_escape(exec)
    )
}

fn shell_escape(path: &PathBuf) -> String {
    let raw = path.to_string_lossy();
    format!("'{}'", raw.replace('\'', "'\\''"))
}
