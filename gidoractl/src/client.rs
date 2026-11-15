use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};

const DEFAULT_SOCKET_RELATIVE: &str =
    "Library/Application Support/space.pkarpovich.gidora/control.sock";
const READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketPath(PathBuf);

impl SocketPath {
    pub fn resolve(explicit: Option<PathBuf>) -> anyhow::Result<SocketPath> {
        if let Some(path) = explicit {
            return Ok(SocketPath(path));
        }
        let Some(home) = std::env::var_os("HOME") else {
            bail!("HOME is not set, so the control socket cannot be located; pass --socket");
        };
        Ok(SocketPath(
            PathBuf::from(home).join(DEFAULT_SOCKET_RELATIVE),
        ))
    }
}

pub fn round_trip(socket: &SocketPath, request: &str) -> anyhow::Result<String> {
    let SocketPath(path) = socket;
    let Ok(mut stream) = UnixStream::connect(path) else {
        bail!(
            "Gidora control socket is not available at {}. Turn on \"Agent control socket\" in Settings > Gidora.",
            path.display()
        );
    };
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .context("setting the read timeout")?;
    stream
        .write_all(request.as_bytes())
        .context("sending the request")?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("reading the response")?;
    if response.is_empty() {
        bail!("the browser closed the connection without a response");
    }
    Ok(response)
}
