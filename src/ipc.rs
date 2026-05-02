use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

const SOCKET_NAME: &str = "gpotlight.sock";
const COMMAND_TOGGLE: &[u8] = b"toggle\n";

pub fn send_toggle_if_running() {
    let Ok(path) = socket_path() else {
        return;
    };

    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };

    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
    let _ = stream.write_all(COMMAND_TOGGLE);
}

pub fn spawn_toggle_server<F>(on_toggle: F) -> Result<()>
where
    F: Fn() + 'static,
{
    let path = socket_path()?;
    if path.exists() {
        let _ = fs::remove_file(&path);
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind IPC socket {}", path.display()))?;
    listener
        .set_nonblocking(false)
        .context("failed to configure IPC socket")?;

    let (sender, receiver) = mpsc::channel::<IpcCommand>();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            match connection {
                Ok(mut stream) => {
                    let mut buffer = [0_u8; 64];
                    if let Ok(size) = stream.read(&mut buffer) {
                        if buffer[..size].starts_with(COMMAND_TOGGLE) {
                            let _ = sender.send(IpcCommand::Toggle);
                        }
                    }
                }
                Err(err) => tracing::warn!(error = ?err, "IPC socket accept failed"),
            }
        }
    });

    glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(command) = receiver.try_recv() {
            match command {
                IpcCommand::Toggle => on_toggle(),
            }
        }
        glib::ControlFlow::Continue
    });

    Ok(())
}

enum IpcCommand {
    Toggle,
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::runtime_dir())
        .context("XDG_RUNTIME_DIR is unavailable")?;
    Ok(runtime_dir.join(SOCKET_NAME))
}
