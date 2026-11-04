use anyhow::{Context, Result};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

const SOCKET_NAME: &str = "gpotlight.sock";
const COMMAND_TOGGLE: &[u8] = b"toggle\n";
const COMMAND_SETTINGS: &[u8] = b"settings\n";
const COMMAND_TIMEOUT: Duration = Duration::from_millis(250);

pub fn send_toggle_if_running() {
    send_command_if_running(COMMAND_TOGGLE);
}

pub fn send_settings_if_running() {
    send_command_if_running(COMMAND_SETTINGS);
}

pub fn is_running() -> bool {
    let Ok(path) = socket_path() else {
        return false;
    };

    UnixStream::connect(path).is_ok()
}

fn send_command_if_running(command: &[u8]) {
    let Ok(path) = socket_path() else {
        return;
    };

    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };

    let _ = stream.set_write_timeout(Some(COMMAND_TIMEOUT));
    let _ = stream.write_all(command);
}

pub fn spawn_toggle_server<T, S>(on_toggle: T, on_settings: S) -> Result<()>
where
    T: Fn() + 'static,
    S: Fn() + 'static,
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

    let (sender, receiver) = async_channel::unbounded::<IpcCommand>();
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            match connection {
                Ok(mut stream) => {
                    // A client that connects without sending a command must not
                    // hold up all subsequent shortcut invocations indefinitely.
                    if stream.set_read_timeout(Some(COMMAND_TIMEOUT)).is_err() {
                        continue;
                    }
                    if let Ok(Some(command)) = read_command(&mut stream) {
                        if sender.try_send(command).is_err() {
                            break;
                        }
                    }
                }
                Err(err) => tracing::warn!(error = ?err, "IPC socket accept failed"),
            }
        }
    });

    // Wake the GTK main context only when a command arrives. A periodic poll
    // both consumed idle CPU and added up to 50 ms to every shortcut press.
    glib::spawn_future_local(async move {
        while let Ok(command) = receiver.recv().await {
            match command {
                IpcCommand::Toggle => on_toggle(),
                IpcCommand::Settings => on_settings(),
            }
        }
    });

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum IpcCommand {
    Toggle,
    Settings,
}

fn read_command(reader: &mut impl Read) -> std::io::Result<Option<IpcCommand>> {
    let mut buffer = [0_u8; 64];
    let mut size = 0;
    while size < buffer.len() {
        let read = match reader.read(&mut buffer[size..]) {
            Ok(read) => read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        if read == 0 {
            break;
        }
        size += read;
        if buffer[..size].contains(&b'\n') {
            return Ok(if buffer[..size].starts_with(COMMAND_TOGGLE) {
                Some(IpcCommand::Toggle)
            } else if buffer[..size].starts_with(COMMAND_SETTINGS) {
                Some(IpcCommand::Settings)
            } else {
                None
            });
        }
    }
    Ok(None)
}

fn socket_path() -> Result<PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(dirs::runtime_dir)
        .context("XDG_RUNTIME_DIR is unavailable")?;
    Ok(runtime_dir.join(SOCKET_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_commands_split_across_socket_reads() {
        for (bytes, expected) in [
            (COMMAND_TOGGLE, IpcCommand::Toggle),
            (COMMAND_SETTINGS, IpcCommand::Settings),
        ] {
            let mut fragmented = OneByteAtATime(bytes);
            assert_eq!(read_command(&mut fragmented).unwrap(), Some(expected));
        }
    }

    #[test]
    fn rejects_unknown_incomplete_and_oversized_commands() {
        for bytes in [b"other\n".as_slice(), b"toggle", &[b'a'; 128]] {
            let mut input = bytes;
            assert_eq!(read_command(&mut input).unwrap(), None);
        }
    }

    #[test]
    fn idle_client_times_out() {
        let (mut server, _client) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        let error = read_command(&mut server).unwrap_err();
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
    }

    struct OneByteAtATime<'a>(&'a [u8]);

    impl Read for OneByteAtATime<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let size = buffer.len().min(1);
            self.0.read(&mut buffer[..size])
        }
    }
}
