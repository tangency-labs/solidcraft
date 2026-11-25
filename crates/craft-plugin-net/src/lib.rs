//! Connections for Craft plugins (capability `net@0.1`). The app opens them on a plugin's behalf:
//! plain TCP; TLS, with the server's certificate checked against the system's trusted roots as the
//! app's update check does; or an SSH session that checks the server's key against the user's
//! known hosts, signs in with the user's agent or keys and runs one command. A plugin reaches only
//! the endpoints its manifest names, and only once the user allowed them. App-independent: an app
//! owns the grant, this crate owns the sockets.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use craft_plugin::{AllowedHost, ConnectPermission, HostError, Result, Transport};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;

/// Connections one plugin call may hold open at once.
pub const MAX_CONNECTIONS: usize = 16;
/// The most one read returns and one write takes.
pub const MAX_CHUNK_BYTES: usize = 1 << 20;
/// What an SSH command printed on its error output, kept for the error a failed command gives.
const MAX_STDERR_BYTES: usize = 16 * 1024;

/// What a plugin asks to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Tcp,
    Tls,
    /// Sign in as `user` and run `command` (for example `git-upload-pack '/me/designs.git'`).
    Ssh {
        user: String,
        command: String,
    },
}

impl Request {
    pub fn transport(&self) -> Transport {
        match self {
            Self::Tcp => Transport::Tcp,
            Self::Tls => Transport::Tls,
            Self::Ssh { .. } => Transport::Ssh,
        }
    }
}

/// Where SSH finds the user's known hosts and keys, and the extra certificate authorities TLS
/// trusts besides the system's. The defaults are the user's own; tests use their own files.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// `None`: `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    /// `None`: `~/.ssh/id_ed25519` and `~/.ssh/id_ecdsa`.
    pub ssh_keys: Option<Vec<PathBuf>>,
    /// Ask the user's SSH agent (`SSH_AUTH_SOCK`) for keys first.
    pub ssh_agent: bool,
    /// Certificates (DER) TLS trusts instead of the system's roots, for a test server.
    pub tls_roots: Vec<Vec<u8>>,
    /// The user's allowlist: when it names any server, a connection must be to one of them too.
    pub allowed_hosts: Vec<AllowedHost>,
}

impl Options {
    /// The user's own known hosts, keys and agent, and the system's trusted roots.
    pub fn user() -> Self {
        Self { ssh_agent: true, ..Self::default() }
    }
}

/// Called while a connection waits for the network, every fraction of a second: an error (the
/// call was cancelled or ran out of time) stops the wait and is returned.
pub type Wait<'a> = &'a mut dyn FnMut() -> Result<()>;

/// The open connections of one plugin call. Dropping it closes them all.
pub struct Connections {
    endpoints: Vec<ConnectPermission>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    options: Options,
    open: BTreeMap<u32, Box<dyn Stream>>,
    next: u32,
}

/// One open connection.
trait Stream: Send {
    /// Up to `max` bytes as soon as some arrive; empty at the end.
    fn read(&mut self, max: usize, wait: Wait) -> Result<Vec<u8>>;
    fn write(&mut self, data: &[u8], wait: Wait) -> Result<()>;
    fn finish(&mut self, wait: Wait) -> Result<()>;
}

impl Connections {
    /// Connections limited to `endpoints`, the ones the manifest names and the user allowed.
    pub fn new(endpoints: Vec<ConnectPermission>, options: Options) -> Self {
        Self { endpoints, options, open: BTreeMap::new(), next: 1 }
    }

    /// Opens a connection and returns its handle.
    pub fn open(&mut self, host: &str, port: u16, request: Request, wait: Wait) -> Result<u32> {
        let transport = request.transport();
        let name = host.trim_end_matches('.');
        if name.is_empty() || name.len() > 253 || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(HostError::InvalidInput { field: "host".into(), reason: "a host name or an address".into() });
        }
        if port == 0 {
            return Err(HostError::InvalidInput { field: "port".into(), reason: "a port is 1 to 65535".into() });
        }
        if !self.endpoints.iter().any(|endpoint| endpoint.allows(name, port, transport)) {
            return Err(HostError::PermissionDenied {
                permission: format!("net: {name}:{port} over {} is not among the plugin's permissions.connect", transport.name()),
            });
        }
        if !self.options.allowed_hosts.is_empty() && !self.options.allowed_hosts.iter().any(|allowed| allowed.allows(name, port)) {
            return Err(HostError::PermissionDenied {
                permission: format!("net: {name}:{port} is not on your list of allowed servers (Preferences › Plugins › Allowed servers)"),
            });
        }
        if let Request::Ssh { user, command } = &request {
            let plain = |text: &str, max: usize| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control);
            if !plain(user, 64) || !plain(command, 4096) {
                return Err(HostError::InvalidInput { field: "transport".into(), reason: "an SSH user and command are plain text".into() });
            }
        }
        if self.open.len() >= MAX_CONNECTIONS {
            return Err(HostError::LimitExceeded { limit: format!("open connections (at most {MAX_CONNECTIONS})") });
        }
        let stream = self.connect(name, port, request, wait)?;
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        self.open.insert(id, stream);
        Ok(id)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn connect(&self, host: &str, port: u16, request: Request, wait: Wait) -> Result<Box<dyn Stream>> {
        native::connect(host, port, request, &self.options, wait)
    }

    #[cfg(target_arch = "wasm32")]
    fn connect(&self, _host: &str, _port: u16, _request: Request, _wait: Wait) -> Result<Box<dyn Stream>> {
        Err(HostError::Unsupported { reason: "this build of the app makes no connections for plugins".into() })
    }

    pub fn read(&mut self, id: u32, max: usize, wait: Wait) -> Result<Vec<u8>> {
        if max == 0 || max > MAX_CHUNK_BYTES {
            return Err(HostError::InvalidInput { field: "max".into(), reason: format!("read 1 to {MAX_CHUNK_BYTES} bytes at a time") });
        }
        self.stream(id)?.read(max, wait)
    }

    pub fn write(&mut self, id: u32, data: &[u8], wait: Wait) -> Result<()> {
        if data.len() > MAX_CHUNK_BYTES {
            return Err(HostError::InvalidInput { field: "data".into(), reason: format!("write at most {MAX_CHUNK_BYTES} bytes at a time") });
        }
        self.stream(id)?.write(data, wait)
    }

    pub fn finish(&mut self, id: u32, wait: Wait) -> Result<()> {
        self.stream(id)?.finish(wait)
    }

    pub fn close(&mut self, id: u32) {
        self.open.remove(&id);
    }

    fn stream(&mut self, id: u32) -> Result<&mut Box<dyn Stream>> {
        self.open.get_mut(&id).ok_or_else(|| HostError::InvalidInput { field: "connection".into(), reason: format!("no open connection {id}") })
    }
}

/// A connection that failed: `what` is the net import (`net.open`, `net.read`...).
fn failed(what: &str, reason: impl Into<String>) -> HostError {
    HostError::CommandFailed { command: what.into(), reason: reason.into() }
}
