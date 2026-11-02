//! SFTP network folders for Send to Slicer (#2304): a folder on another computer, reached over SSH
//! with the user's own keys, the ssh-agent's or one key file. No password is asked for or kept.
//! The server must be one the user trusts: listed in `~/.ssh/known_hosts`, or its key's fingerprint
//! confirmed once under Test. A server whose key is not the one trusted is refused.
//!
//! A send connects, writes the mesh under a hidden temporary name, gives it the first free readable
//! name (`Part.3mf`, `Part (2).3mf`, …) and leaves: nothing on the server is replaced. Every step
//! has a time limit, and the work runs on a thread of its own with a runtime of its own, so neither
//! a server that does not answer nor the caller's own runtime can hold it up. Native builds only.

use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use russh::client;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{OpenFlags, StatusCode};
use tokio::io::AsyncWriteExt;

use crate::remote_slicers::{self, Delivered, Dest, HostKey, RemoteTarget, SftpAddress, Tested};

/// Most keys of the ssh-agent offered to a server: servers drop a client after about six refused.
const MAX_AGENT_KEYS: usize = 5;
/// Bytes read from the mesh at a time.
const CHUNK: usize = 256 * 1024;
/// A key file is a few kilobytes; a bigger file is not one.
const MAX_KEY_FILE: u64 = 64 * 1024;
/// The slowest upload waited for, in bytes a second: a 1 GiB file may take about four hours.
const MIN_RATE: u64 = 64 * 1024;

/// An SFTP target, ready to connect to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: SftpAddress,
    /// The folder there: a whole path, or one from the login folder (`~/` is allowed).
    pub folder: String,
    /// The private key file to log in with; without one, the ssh-agent's keys.
    pub key: Option<PathBuf>,
    /// The fingerprint of the server key the user trusted for this target.
    pub host_key: Option<String>,
}

/// What of the user's own SSH setup a connection uses, and how long it may wait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// The known hosts file, `~/.ssh/known_hosts`.
    pub known_hosts: Option<PathBuf>,
    /// The ssh-agent's socket (`SSH_AUTH_SOCK`), or its pipe on Windows.
    pub agent: Option<PathBuf>,
    /// A fingerprint the user confirmed just now (Test), trusted for this connection only.
    pub confirmed: Option<String>,
    /// How long connecting, logging in, and each answer of the server may take.
    pub limit: Duration,
}

impl Setup {
    /// The user's: their known hosts file and ssh-agent, and [`remote_slicers::TIME_LIMIT`].
    pub fn user() -> Setup {
        #[cfg(any(test, feature = "sftp-test-server"))]
        if let Some(made_up) = TEST_SETUP.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            return made_up;
        }
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty()).map(PathBuf::from);
        let agent = std::env::var_os("SSH_AUTH_SOCK").filter(|s| !s.is_empty()).map(PathBuf::from);
        #[cfg(windows)]
        let agent = agent.or_else(|| Some(PathBuf::from(r"\\.\pipe\openssh-ssh-agent")));
        Setup { known_hosts: home.map(|h| h.join(".ssh").join("known_hosts")), agent, confirmed: None, limit: remote_slicers::TIME_LIMIT }
    }
}

/// Tests set the SSH setup to one of their own (a known hosts file and an agent they made), so
/// the user's own `~/.ssh` plays no part.
#[cfg(any(test, feature = "sftp-test-server"))]
pub static TEST_SETUP: Mutex<Option<Setup>> = Mutex::new(None);

impl Server {
    /// The server of an SFTP target.
    pub fn of(target: &RemoteTarget) -> Option<Server> {
        match &target.dest {
            Dest::Sftp { folder, key, host_key, .. } => {
                Some(Server { address: target.address()?, folder: folder.clone(), key: key.as_deref().map(from_home), host_key: host_key.clone() })
            }
            Dest::Folder { .. } => None,
        }
    }

    /// The folder as the server takes it: `.` for the login folder.
    fn folder_path(&self) -> String {
        let folder = self.folder.trim();
        if folder.starts_with('/') {
            let trimmed = folder.trim_end_matches('/');
            return if trimmed.is_empty() { "/".into() } else { trimmed.into() };
        }
        let folder = if folder == "~" { "" } else { folder.strip_prefix("~/").unwrap_or(folder) };
        let folder = folder.trim_end_matches('/');
        if folder.is_empty() { ".".into() } else { folder.into() }
    }

    /// `name` in the folder, as the server takes it.
    fn path(&self, name: &str) -> String {
        match self.folder_path().as_str() {
            "." => name.to_string(),
            "/" => format!("/{name}"),
            folder => format!("{folder}/{name}"),
        }
    }

    /// The folder for people: `sftp://user@host/folder`.
    fn shown(&self) -> String {
        match self.folder_path().as_str() {
            "." => remote_slicers::sftp_url(&self.address, "~"),
            folder => remote_slicers::sftp_url(&self.address, folder),
        }
    }

    /// Copy the file at `src` into the folder as `<stem>.<ext>`, numbered when that name is taken,
    /// refusing a file over `max` bytes. The error says what went wrong in words.
    pub fn deliver(&self, src: &Path, stem: &str, ext: &str, max: u64, setup: &Setup) -> Result<Delivered, String> {
        let size = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?.len();
        if size > max {
            let mb = |b: u64| b.div_ceil(1_000_000);
            return Err(format!("the file is {} MB, more than the {} MB a network folder takes", mb(size), mb(max)));
        }
        let total = setup.limit.saturating_mul(4).saturating_add(Duration::from_secs(size / MIN_RATE));
        let (server, setup, src, stem, ext) = (self.clone(), setup.clone(), src.to_path_buf(), stem.to_string(), ext.to_string());
        let seen = Arc::new(Mutex::new(None));
        let shown = self.address.to_string();
        run(total, &shown, move || async move {
            let (handle, sftp) = connect(&server, &setup, seen).await?;
            let sent = put(&sftp, &server, &src, &stem, &ext, size, max).await;
            leave(handle, sftp).await;
            sent
        })
    }

    /// Log in, write a small file in the folder and delete it again: whether a mesh can be sent.
    /// The answer carries the server's key once it showed it, trusted or not.
    pub fn test(&self, setup: &Setup) -> Tested {
        let seen: Arc<Mutex<Option<Seen>>> = Arc::new(Mutex::new(None));
        let (server, setup_, seen_) = (self.clone(), setup.clone(), seen.clone());
        let total = setup.limit.saturating_mul(4);
        let result = run(total, &self.address.to_string(), move || async move {
            let (handle, sftp) = connect(&server, &setup_, seen_).await?;
            let probed = probe(&sftp, &server).await;
            leave(handle, sftp).await;
            probed
        });
        let host_key = seen.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|s| s.key.clone());
        match result {
            Ok(()) => Tested { ok: true, error: None, host_key },
            Err(e) => Tested { ok: false, error: Some(e), host_key },
        }
    }
}

/// `~/x` in the user's home folder; any other path as it is.
fn from_home(path: &str) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).filter(|h| !h.is_empty());
    match (path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")), home) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// Run `work` on a thread of its own, in a runtime of its own, for at most `total`.
fn run<T, F>(total: Duration, shown: &str, work: impl FnOnce() -> F + Send + 'static) -> Result<T, String>
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>>,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let gave_up = format!("{shown} did not finish within {} s", total.as_secs());
    let gave_up_ = gave_up.clone();
    std::thread::Builder::new()
        .name("sftp".into())
        .spawn(move || {
            let answer = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => {
                    let answer = runtime.block_on(async move { tokio::time::timeout(total, work()).await.unwrap_or(Err(gave_up_)) });
                    // A name lookup still going on (a resolver that does not answer) is left behind.
                    runtime.shutdown_background();
                    answer
                }
                Err(e) => Err(format!("couldn't start the connection ({e})")),
            };
            let _ = tx.send(answer);
        })
        .map_err(|e| format!("couldn't start the connection to {shown} ({e})"))?;
    match rx.recv_timeout(total.saturating_add(Duration::from_secs(5))) {
        Ok(answer) => answer,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(gave_up),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(format!("the connection to {shown} stopped unexpectedly")),
    }
}

/// `future`, or the words for a server that did not answer within `limit`.
async fn within<T>(limit: Duration, what: &str, future: impl Future<Output = T>) -> Result<T, String> {
    tokio::time::timeout(limit, future).await.map_err(|_| format!("{what} did not answer within {} s (is it on?)", limit.as_secs()))
}

/// The server's key as the connection saw it.
#[derive(Clone, Debug)]
struct Seen {
    key: HostKey,
}

/// Checks the server's key; nothing else of a connection is handled here.
struct Checker {
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
    trusted: Option<String>,
    confirmed: Option<String>,
    seen: Arc<Mutex<Option<Seen>>>,
}

impl client::Handler for Checker {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let key = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::new(cert.public_key().clone(), ""),
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        let listed = match &self.known_hosts {
            Some(path) => russh::keys::check_known_hosts_path(&self.host, self.port, &key, path),
            None => Ok(false),
        };
        let (trusted, changed, known_hosts) = match listed {
            Ok(true) => (true, false, true),
            // The known hosts file names another key for this server: never trusted, whatever was confirmed.
            Err(russh::keys::Error::KeyChanged { .. }) => (false, true, true),
            // Not listed, or the file can't be read: trusted only with the fingerprint the user confirmed.
            _ => {
                let mine = [self.trusted.as_deref(), self.confirmed.as_deref()].into_iter().flatten().any(|f| f == fingerprint);
                (mine, !mine && self.trusted.is_some(), false)
            }
        };
        let key = HostKey { fingerprint, algorithm: key.algorithm().to_string(), trusted, changed, known_hosts };
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(Seen { key });
        Ok(trusted)
    }
}

/// Connect, check the server's key, log in and start SFTP, each step within the limit.
async fn connect(server: &Server, setup: &Setup, seen: Arc<Mutex<Option<Seen>>>) -> Result<(client::Handle<Checker>, SftpSession), String> {
    let address = &server.address;
    let limit = setup.limit;
    let shown = address.to_string();
    let found = within(limit, &address.host, tokio::net::lookup_host((address.host.as_str(), address.port)))
        .await?
        .map_err(|e| format!("couldn't find {} on the network ({e})", address.host))?;
    let addrs: Vec<std::net::SocketAddr> = found.collect();
    if addrs.is_empty() {
        return Err(format!("couldn't find {} on the network", address.host));
    }
    let config = client::Config { inactivity_timeout: Some(limit), ..client::Config::default() };
    let checker = Checker {
        host: address.host.clone(),
        port: address.port,
        known_hosts: setup.known_hosts.clone(),
        trusted: server.host_key.clone(),
        confirmed: setup.confirmed.clone(),
        seen: seen.clone(),
    };
    let mut handle = match within(limit, &shown, client::connect(Arc::new(config), &addrs[..], checker)).await? {
        Ok(handle) => handle,
        Err(e) => return Err(refused(address, setup, &e, seen.lock().unwrap_or_else(PoisonError::into_inner).clone())),
    };
    within(limit, &shown, log_in(&mut handle, server, setup)).await??;
    let channel = within(limit, &shown, handle.channel_open_session()).await?.map_err(|e| format!("{shown}: {e}"))?;
    within(limit, &shown, channel.request_subsystem(true, "sftp")).await?.map_err(|e| format!("{shown}: {e}"))?;
    let sftp = within(limit, &shown, SftpSession::new(channel.into_stream())).await?.map_err(|e| format!("{shown} has no SFTP server ({e})"))?;
    sftp.set_timeout(limit.as_secs().max(1));
    Ok((handle, sftp))
}

/// What went wrong connecting to `address`, in words.
fn refused(address: &SftpAddress, setup: &Setup, e: &russh::Error, seen: Option<Seen>) -> String {
    let host = &address.host;
    match (e, seen) {
        (russh::Error::UnknownKey, Some(Seen { key })) if key.known_hosts => {
            let file = setup.known_hosts.as_ref().map_or_else(|| "known_hosts".to_string(), |p| p.display().to_string());
            let listed = if address.port == remote_slicers::SSH_PORT { host.clone() } else { format!("\"[{host}]:{}\"", address.port) };
            format!(
                "the key of {host} ({}) is not the one {file} lists for it: if the server was set up again, remove its line (ssh-keygen -R {listed}); otherwise someone may be in between",
                key.fingerprint
            )
        }
        (russh::Error::UnknownKey, Some(Seen { key, .. })) if key.changed => format!(
            "the key of {host} is now {}, not the one you trusted: if the server was set up again, test it and trust the new key; otherwise someone may be in between",
            key.fingerprint
        ),
        (russh::Error::UnknownKey, Some(Seen { key, .. })) => {
            format!("{host} is not a server you trust yet: its key is {} ({}). Check it on the server, then trust it", key.fingerprint, key.algorithm)
        }
        (russh::Error::IO(io), _) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused => format!("nothing answers on {host} port {} (is its SSH server on?)", address.port),
            std::io::ErrorKind::HostUnreachable | std::io::ErrorKind::NetworkUnreachable => format!("{host} can't be reached from here"),
            std::io::ErrorKind::TimedOut => format!("{address} did not answer (is it on?)"),
            _ => format!("{address}: {io}"),
        },
        (russh::Error::Disconnect | russh::Error::HUP, _) => format!("{address} closed the connection"),
        _ => format!("{address}: {e}"),
    }
}

/// Log in with the key file, or else with the keys of the ssh-agent.
async fn log_in(handle: &mut client::Handle<Checker>, server: &Server, setup: &Setup) -> Result<(), String> {
    let address = &server.address;
    let user = address.user.as_str();
    let Some(path) = &server.key else { return agent_log_in(handle, address, setup).await };
    let key = read_key(path)?;
    let hash = rsa_hash(handle, key.algorithm().is_rsa()).await;
    let answer = handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(|e| format!("{address}: {e}"))?;
    if answer.success() { Ok(()) } else { Err(format!("{address} refused the key {}", path.display())) }
}

/// The hash for an RSA key, as the server prefers.
async fn rsa_hash(handle: &client::Handle<Checker>, rsa: bool) -> Option<HashAlg> {
    if rsa { handle.best_supported_rsa_hash().await.ok().flatten().flatten() } else { None }
}

/// A private key file without a passphrase, read with a size cap.
fn read_key(path: &Path) -> Result<PrivateKey, String> {
    let shown = path.display();
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => format!("{shown}: no such key file"),
        std::io::ErrorKind::PermissionDenied => format!("{shown}: no permission to read the key file"),
        _ => format!("{shown}: {e}"),
    })?;
    let mut text = String::new();
    file.take(MAX_KEY_FILE + 1).read_to_string(&mut text).map_err(|_| format!("{shown} is not a private key file"))?;
    if text.len() as u64 > MAX_KEY_FILE {
        return Err(format!("{shown} is not a private key file"));
    }
    russh::keys::decode_secret_key(&text, None).map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => {
            format!("{shown} has a passphrase: add it to ssh-agent (ssh-add) and leave the key file out, so SolidCraft never asks for it")
        }
        _ => format!("{shown} is not a private key SolidCraft can use (an Ed25519 or ECDSA key in OpenSSH format is)"),
    })
}

/// Log in with the keys the ssh-agent holds, a few at most.
async fn agent_log_in(handle: &mut client::Handle<Checker>, address: &SftpAddress, setup: &Setup) -> Result<(), String> {
    let no_agent = "no ssh-agent is running: start one and add your key (ssh-add), or give a key file";
    let Some(socket) = &setup.agent else { return Err(no_agent.into()) };
    let mut agent = open_agent(socket).await.map_err(|_| no_agent.to_string())?;
    let keys = agent.request_identities().await.map_err(|e| format!("ssh-agent did not list its keys ({e})"))?;
    if keys.is_empty() {
        return Err("ssh-agent holds no keys: add yours with ssh-add, or give a key file".into());
    }
    for identity in keys.iter().take(MAX_AGENT_KEYS) {
        let key = identity.public_key().into_owned();
        let hash = rsa_hash(handle, key.algorithm().is_rsa()).await;
        if let Ok(answer) = handle.authenticate_publickey_with(address.user.as_str(), key, hash, &mut agent).await
            && answer.success()
        {
            return Ok(());
        }
    }
    Err(format!("{address} refused the keys of ssh-agent"))
}

type Agent = russh::keys::agent::client::AgentClient<Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'static>>;

#[cfg(unix)]
async fn open_agent(socket: &Path) -> Result<Agent, russh::keys::Error> {
    Ok(russh::keys::agent::client::AgentClient::connect_uds(socket).await?.dynamic())
}

#[cfg(windows)]
async fn open_agent(socket: &Path) -> Result<Agent, russh::keys::Error> {
    Ok(russh::keys::agent::client::AgentClient::connect_named_pipe(socket.as_os_str()).await?.dynamic())
}

#[cfg(not(any(unix, windows)))]
async fn open_agent(_socket: &Path) -> Result<Agent, russh::keys::Error> {
    Err(russh::keys::Error::AgentFailure)
}

/// Close SFTP and the connection; what the server says to that no longer matters.
async fn leave(handle: client::Handle<Checker>, sftp: SftpSession) {
    let _ = sftp.close().await;
    let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
}

/// What went wrong with `shown` on the server, in words.
fn explain(shown: &str, e: &SftpError) -> String {
    match e {
        SftpError::Status(status) => match status.status_code {
            StatusCode::NoSuchFile => format!("{shown} is not there"),
            StatusCode::PermissionDenied => format!("{shown}: no permission to write there"),
            StatusCode::NoConnection | StatusCode::ConnectionLost => format!("{shown}: the connection broke"),
            _ if status.error_message.trim().is_empty() => format!("{shown}: the server refused ({:?})", status.status_code),
            _ => format!("{shown}: the server refused ({})", status.error_message.trim()),
        },
        SftpError::Timeout => format!("{shown} stopped answering"),
        _ => format!("{shown}: {e}"),
    }
}

/// The folder must be there, and be a folder.
async fn folder_there(sftp: &SftpSession, server: &Server) -> Result<(), String> {
    let shown = server.shown();
    match sftp.metadata(server.folder_path()).await {
        Ok(attrs) if attrs.is_dir() => Ok(()),
        Ok(_) => Err(format!("{shown} is not a folder")),
        Err(e) => Err(explain(&shown, &e)),
    }
}

/// Write the mesh under a hidden temporary name, then give it the first free name.
async fn put(sftp: &SftpSession, server: &Server, src: &Path, stem: &str, ext: &str, size: u64, max: u64) -> Result<Delivered, String> {
    let shown = server.shown();
    folder_there(sftp, server).await?;
    // A server that says how much room it has is asked first, so a full disk is said plainly.
    if let Ok(Some(room)) = sftp.fs_info(server.folder_path()).await {
        let free = room.blocks_avail.saturating_mul(room.fragment_size.max(1));
        if free < size {
            let mb = |b: u64| b.div_ceil(1_000_000);
            return Err(format!("{shown} is full: {} MB free, the file needs {} MB", free / 1_000_000, mb(size)));
        }
    }
    let part = server.path(&format!(".{stem}.{ext}.{}.part", remote_slicers::unique()));
    let mut file = sftp.open_with_flags(&part, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE).await.map_err(|e| explain(&shown, &e))?;
    let written = copy(&mut file, src, max).await;
    let closed = file.shutdown().await;
    let placed = match (written, closed) {
        (Ok(()), Ok(())) => place(sftp, server, &part, stem, ext).await,
        (Err(e), _) => Err(e),
        (Ok(()), Err(e)) => Err(stopped(&shown, &e)),
    };
    if placed.is_err() {
        let _ = sftp.remove_file(&part).await;
    }
    placed
}

/// The server stopped taking the file, in words.
fn stopped(shown: &str, e: &std::io::Error) -> String {
    format!("{shown}: the server stopped taking the file ({e}); is its disk full?")
}

/// Copy the file at `src` into `file`, at most `max` bytes.
async fn copy(file: &mut russh_sftp::client::fs::File, src: &Path, max: u64) -> Result<(), String> {
    let shown_src = src.display().to_string();
    let mut from = std::fs::File::open(src).map_err(|e| format!("{shown_src}: {e}"))?;
    let mut buf = vec![0u8; CHUNK];
    let mut sent = 0u64;
    loop {
        let n = from.read(&mut buf).map_err(|e| format!("{shown_src}: {e}"))?;
        if n == 0 {
            return file.flush().await.map_err(|e| stopped("the server", &e));
        }
        sent = sent.saturating_add(n as u64);
        if sent > max {
            return Err("the file grew past the size a network folder takes".into());
        }
        file.write_all(buf.get(..n).unwrap_or_default()).await.map_err(|e| stopped("the server", &e))?;
    }
}

/// Rename `part` to the first free name; SFTP's rename never replaces a file.
async fn place(sftp: &SftpSession, server: &Server, part: &str, stem: &str, ext: &str) -> Result<Delivered, String> {
    let shown = server.shown();
    for n in 1..=remote_slicers::MAX_NUMBERED {
        let path = server.path(&remote_slicers::numbered(stem, ext, n));
        if sftp.try_exists(path.as_str()).await.map_err(|e| explain(&shown, &e))? {
            continue;
        }
        match sftp.rename(part, path.as_str()).await {
            Ok(()) => return Ok(Delivered { path: PathBuf::from(remote_slicers::sftp_url(&server.address, &path)), numbered: n > 1 }),
            // Taken between the look and the rename: the next number.
            Err(_) if matches!(sftp.try_exists(path.as_str()).await, Ok(true)) => continue,
            Err(e) => return Err(explain(&shown, &e)),
        }
    }
    Err(format!("{shown}: {} files named {stem} are there already", remote_slicers::MAX_NUMBERED))
}

/// Write a small file in the folder and delete it again.
async fn probe(sftp: &SftpSession, server: &Server) -> Result<(), String> {
    let shown = server.shown();
    folder_there(sftp, server).await?;
    let path = server.path(&format!(".solidcraft-test-{}.tmp", remote_slicers::unique()));
    let data = b"SolidCraft checks that it can write here; this file is deleted at once.\n";
    let mut file = sftp.open_with_flags(&path, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE).await.map_err(|e| explain(&shown, &e))?;
    let written = file.write_all(data).await;
    let closed = file.shutdown().await;
    let size = match (written, closed) {
        (Ok(()), Ok(())) => sftp.metadata(path.as_str()).await.map(|m| m.len()).map_err(|e| explain(&shown, &e)),
        (Err(e), _) | (Ok(()), Err(e)) => Err(stopped(&shown, &e)),
    };
    let removed = sftp.remove_file(path.as_str()).await;
    match (size, removed) {
        (Ok(len), Ok(())) if len == data.len() as u64 => Ok(()),
        (Ok(len), Ok(())) => Err(format!("{shown}: wrote {} bytes and found {len}", data.len())),
        (Ok(_), Err(e)) => Err(format!("{shown}: the test file could not be deleted ({})", explain(&shown, &e))),
        (Err(e), _) => Err(e),
    }
}

/// An SFTP server in the test process: russh's SSH server with a small SFTP handler on a real
/// folder, and an ssh-agent, so logging in, trusting keys and sending are tested end to end (here,
/// and in the app's tests through the `sftp-test-server` feature).
#[cfg(any(test, feature = "sftp-test-server"))]
pub mod test_server {
    use std::collections::HashMap;
    use std::io::{Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use russh::keys::{PrivateKey, PublicKey, ssh_key};
    use russh::server::{self, Auth, ChannelOpenHandle, Msg, Session, VerifiedPublicKey};
    use russh::{Channel, ChannelId};
    use russh_sftp::protocol::{Attrs, ExtendedReply, FileAttributes, Handle, OpenFlags, Packet, Status, StatusCode, Version};

    /// A key made from `seed`, the same in every run.
    pub fn key(seed: u8) -> PrivateKey {
        PrivateKey::from(ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]))
    }

    /// How the server behaves.
    #[derive(Clone, Default)]
    pub struct Options {
        /// Keys that may log in.
        pub authorized: Vec<PublicKey>,
        /// Room on its disk, said through statvfs@openssh.com; `None`: it doesn't say.
        pub free: Option<u64>,
        /// Bytes taken before writing fails, as on a full disk that doesn't say so beforehand.
        pub fail_after: Option<u64>,
        /// The port on 127.0.0.1; 0 for any free one.
        pub port: u16,
    }

    /// A server that runs until the tests end.
    pub struct Running {
        pub port: u16,
        pub host_key: PrivateKey,
    }

    /// Start a server whose key is made from `host_seed` and whose login folder is `home`, on
    /// 127.0.0.1 (`options.port`, or a free port). It runs until the process ends.
    pub fn start(host_seed: u8, options: Options, home: &Path) -> Result<Running, String> {
        let host_key = key(host_seed);
        let (tx, rx) = std::sync::mpsc::channel();
        let (config_key, home) = (host_key.clone(), home.to_path_buf());
        std::thread::Builder::new()
            .name("sftp-test-server".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let bound = tokio::net::TcpListener::bind(("127.0.0.1", options.port)).await;
                    let listener = match bound.and_then(|l| l.local_addr().map(|a| (l, a.port()))) {
                        Ok((listener, port)) => {
                            let _ = tx.send(Ok(port));
                            listener
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    let config = Arc::new(server::Config {
                        keys: vec![config_key],
                        auth_rejection_time: Duration::from_millis(10),
                        auth_rejection_time_initial: Some(Duration::ZERO),
                        ..server::Config::default()
                    });
                    while let Ok((stream, _)) = listener.accept().await {
                        let ssh = Ssh { options: options.clone(), home: home.clone(), channels: Arc::default() };
                        let config = config.clone();
                        tokio::spawn(async move {
                            if let Ok(session) = server::run_stream(config, stream, ssh).await {
                                let _ = session.await;
                            }
                        });
                    }
                });
            })
            .map_err(|e| e.to_string())?;
        let port = rx.recv().map_err(|e| e.to_string())??;
        Ok(Running { port, host_key })
    }

    struct Ssh {
        options: Options,
        home: PathBuf,
        channels: Arc<Mutex<HashMap<ChannelId, Channel<Msg>>>>,
    }

    impl server::Handler for Ssh {
        type Error = russh::Error;

        async fn auth_publickey(&mut self, _user: &str, key: VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
            let known = self.options.authorized.iter().any(|k| k.key_data() == key.public_key.key_data());
            Ok(if known { Auth::Accept } else { Auth::reject() })
        }

        async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: ChannelOpenHandle, _session: &mut Session) -> Result<(), Self::Error> {
            self.channels.lock().unwrap_or_else(PoisonError::into_inner).insert(channel.id(), channel);
            reply.accept().await;
            Ok(())
        }

        async fn subsystem_request(&mut self, id: ChannelId, name: &str, session: &mut Session) -> Result<(), Self::Error> {
            let channel = self.channels.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
            match channel {
                Some(channel) if name == "sftp" => {
                    session.channel_success(id)?;
                    let sftp = Sftp { options: self.options.clone(), home: self.home.clone(), files: HashMap::new(), next: 0, taken: 0 };
                    russh_sftp::server::run(channel.into_stream(), sftp).await;
                }
                _ => session.channel_failure(id)?,
            }
            Ok(())
        }
    }

    struct Sftp {
        options: Options,
        home: PathBuf,
        files: HashMap<String, std::fs::File>,
        next: u64,
        taken: u64,
    }

    impl Sftp {
        fn local(&self, path: &str) -> PathBuf {
            if path.starts_with('/') { PathBuf::from(path) } else { self.home.join(path) }
        }
    }

    fn ok(id: u32) -> Status {
        Status { id, status_code: StatusCode::Ok, error_message: "Ok".into(), language_tag: "en-US".into() }
    }

    fn status(e: &std::io::Error) -> StatusCode {
        match e.kind() {
            std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
            std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
            _ => StatusCode::Failure,
        }
    }

    impl russh_sftp::server::Handler for Sftp {
        type Error = StatusCode;

        fn unimplemented(&self) -> Self::Error {
            StatusCode::OpUnsupported
        }

        async fn init(&mut self, _version: u32, _extensions: HashMap<String, String>) -> Result<Version, Self::Error> {
            let mut version = Version::new();
            if self.options.free.is_some() {
                version.extensions.insert(russh_sftp::extensions::STATVFS.into(), "2".into());
            }
            Ok(version)
        }

        async fn open(&mut self, id: u32, filename: String, flags: OpenFlags, _attrs: FileAttributes) -> Result<Handle, Self::Error> {
            let mut open = std::fs::OpenOptions::new();
            open.read(flags.contains(OpenFlags::READ)).write(flags.contains(OpenFlags::WRITE)).truncate(flags.contains(OpenFlags::TRUNCATE));
            if flags.contains(OpenFlags::CREATE) {
                if flags.contains(OpenFlags::EXCLUDE) {
                    open.create_new(true);
                } else {
                    open.create(true);
                }
            }
            let file = open.open(self.local(&filename)).map_err(|e| status(&e))?;
            self.next += 1;
            let handle = format!("h{}", self.next);
            self.files.insert(handle.clone(), file);
            Ok(Handle { id, handle })
        }

        async fn write(&mut self, id: u32, handle: String, offset: u64, data: Vec<u8>) -> Result<Status, Self::Error> {
            self.taken += data.len() as u64;
            if self.options.fail_after.is_some_and(|room| self.taken > room) {
                return Err(StatusCode::Failure);
            }
            let file = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
            file.seek(SeekFrom::Start(offset)).and_then(|_| file.write_all(&data)).map_err(|e| status(&e))?;
            Ok(ok(id))
        }

        async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
            self.files.remove(&handle);
            Ok(ok(id))
        }

        async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            let meta = std::fs::metadata(self.local(&path)).map_err(|e| status(&e))?;
            Ok(Attrs { id, attrs: FileAttributes::from(&meta) })
        }

        async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            let meta = std::fs::symlink_metadata(self.local(&path)).map_err(|e| status(&e))?;
            Ok(Attrs { id, attrs: FileAttributes::from(&meta) })
        }

        async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
            std::fs::remove_file(self.local(&filename)).map_err(|e| status(&e))?;
            Ok(ok(id))
        }

        async fn rename(&mut self, id: u32, oldpath: String, newpath: String) -> Result<Status, Self::Error> {
            // SFTP version 3: renaming onto a file that is there fails.
            if self.local(&newpath).exists() {
                return Err(StatusCode::Failure);
            }
            std::fs::rename(self.local(&oldpath), self.local(&newpath)).map_err(|e| status(&e))?;
            Ok(ok(id))
        }

        async fn extended(&mut self, id: u32, request: String, _data: Vec<u8>) -> Result<Packet, Self::Error> {
            let (Some(free), true) = (self.options.free, request == russh_sftp::extensions::STATVFS) else {
                return Err(StatusCode::OpUnsupported);
            };
            let room = russh_sftp::extensions::Statvfs {
                block_size: 4096,
                fragment_size: 4096,
                blocks: 1 << 20,
                blocks_free: free / 4096,
                blocks_avail: free / 4096,
                inodes: 1 << 16,
                inodes_free: 1 << 15,
                inodes_avail: 1 << 15,
                fs_id: 1,
                flags: 0,
                name_max: 255,
            };
            let data = russh_sftp::ser::to_bytes(&room).map_err(|_| StatusCode::Failure)?.to_vec();
            Ok(Packet::ExtendedReply(ExtendedReply { id, data }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{self, Options};
    use super::*;
    use std::time::Instant;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-sftp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    /// Start an ssh-agent on `socket` holding `keys`.
    #[cfg(unix)]
    fn start_agent(socket: &Path, keys: Vec<russh::keys::PrivateKey>) {
        #[derive(Clone)]
        struct Agent;
        impl russh::keys::agent::server::Agent for Agent {}

        struct Incoming(tokio::net::UnixListener);
        impl futures::Stream for Incoming {
            type Item = std::io::Result<tokio::net::UnixStream>;
            fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::Item>> {
                self.get_mut().0.poll_accept(cx).map(|r| Some(r.map(|(stream, _)| stream)))
            }
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::UnixListener::bind(&socket).unwrap();
                let serving = tokio::spawn(russh::keys::agent::server::serve(Incoming(listener), Agent));
                let mut client = russh::keys::agent::client::AgentClient::connect_uds(&socket).await.unwrap();
                for key in &keys {
                    client.add_identity(key, &[]).await.unwrap();
                }
                tx.send(()).unwrap();
                let _ = serving.await;
            });
        });
        rx.recv().unwrap();
    }

    fn fingerprint(key: &PrivateKey) -> String {
        key.public_key().fingerprint(HashAlg::Sha256).to_string()
    }

    /// The user's key, seed 7, in a file of its own outside the folder sent to.
    fn key_file(tag: &str, seed: u8) -> PathBuf {
        let dir = scratch(&format!("{tag}-keys"));
        let path = dir.join("id_ed25519");
        let text = test_server::key(seed).to_openssh(russh::keys::ssh_key::LineEnding::LF).unwrap();
        std::fs::write(&path, text.as_bytes()).unwrap();
        path
    }

    /// A server that lets the key of seed 7 in, with its login folder `home`.
    fn serving(home: &Path, options: Options) -> test_server::Running {
        let authorized = vec![test_server::key(7).public_key().clone()];
        test_server::start(1, Options { authorized, ..options }, home).unwrap()
    }

    fn setup() -> Setup {
        Setup { known_hosts: None, agent: None, confirmed: None, limit: Duration::from_secs(10) }
    }

    fn server(port: u16, folder: &Path, key: Option<PathBuf>, host_key: Option<String>) -> Server {
        Server { address: SftpAddress::checked("maker", "127.0.0.1", port).unwrap(), folder: folder.to_string_lossy().into_owned(), key, host_key }
    }

    fn mesh(dir: &Path, bytes: usize) -> PathBuf {
        let path = dir.join("mesh.3mf");
        std::fs::write(&path, vec![b'm'; bytes]).unwrap();
        path
    }

    #[test]
    fn an_unknown_server_is_refused_until_its_key_is_confirmed() {
        let home = scratch("unknown");
        let running = serving(&home, Options::default());
        let fp = fingerprint(&running.host_key);
        let folder = home.join("prints");
        std::fs::create_dir_all(&folder).unwrap();
        let target = server(running.port, &folder, Some(key_file("unknown", 7)), None);

        let tested = target.test(&setup());
        assert!(!tested.ok, "{tested:?}");
        let seen = tested.host_key.clone().unwrap();
        assert_eq!(
            (seen.fingerprint.as_str(), seen.algorithm.as_str(), seen.trusted, seen.changed, seen.known_hosts),
            (fp.as_str(), "ssh-ed25519", false, false, false)
        );
        assert!(tested.error.as_deref().unwrap().contains(&fp), "the fingerprint is shown to compare: {tested:?}");
        assert!(target.deliver(&mesh(&home, 10), "Part", "3mf", 1 << 20, &setup()).is_err(), "nothing is sent to it either");
        assert!(names(&folder).is_empty(), "nothing was written: {:?}", names(&folder));

        let confirmed = Setup { confirmed: Some(fp.clone()), ..setup() };
        let tested = target.test(&confirmed);
        assert!(tested.ok && tested.host_key.as_ref().is_some_and(|k| k.trusted), "{tested:?}");
        assert!(names(&folder).is_empty(), "the test file is gone: {:?}", names(&folder));

        let trusted = Server { host_key: Some(fp), ..target };
        let sent = trusted.deliver(&mesh(&home, 10), "Part", "3mf", 1 << 20, &setup()).unwrap();
        assert_eq!(names(&folder), ["Part.3mf"]);
        assert_eq!(sent.path, PathBuf::from(format!("sftp://maker@127.0.0.1:{}{}/Part.3mf", running.port, folder.display())));
    }

    #[test]
    fn a_server_in_known_hosts_is_trusted_and_one_whose_key_changed_never_is() {
        let home = scratch("known");
        let running = serving(&home, Options::default());
        let fp = fingerprint(&running.host_key);
        let target = server(running.port, &home, Some(key_file("known", 7)), None);
        let listed = |key: &PrivateKey| {
            let path = home.join("known_hosts");
            let line = format!("# a comment\n[127.0.0.1]:{} {}\n", running.port, key.public_key().to_openssh().unwrap());
            std::fs::write(&path, line).unwrap();
            Setup { known_hosts: Some(path), ..setup() }
        };

        let tested = target.test(&listed(&running.host_key));
        assert!(tested.ok && tested.host_key.as_ref().is_some_and(|k| k.trusted && !k.changed && k.known_hosts), "{tested:?}");

        let other = test_server::key(99);
        let changed = Setup { confirmed: Some(fp.clone()), ..listed(&other) };
        let tested = target.test(&changed);
        assert!(!tested.ok && tested.host_key.as_ref().is_some_and(|k| !k.trusted && k.changed), "{tested:?}");
        let error = tested.error.unwrap();
        assert!(error.contains(&format!("ssh-keygen -R \"[127.0.0.1]:{}\"", running.port)), "confirming never overrides known_hosts: {error}");
        assert!(tested.host_key.as_ref().is_some_and(|k| k.known_hosts));

        let moved = Server { host_key: Some(fingerprint(&other)), ..target.clone() };
        let tested = moved.test(&setup());
        assert!(!tested.ok && tested.host_key.as_ref().is_some_and(|k| k.changed && k.fingerprint == fp), "{tested:?}");
        assert!(tested.error.unwrap().contains("not the one you trusted"));
    }

    #[test]
    fn sending_writes_a_readable_name_and_never_replaces_a_file() {
        let home = scratch("send");
        let running = serving(&home, Options::default());
        let folder = home.join("prints");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Bracket.3mf"), b"theirs").unwrap();
        let src = mesh(&home, 300_000);
        let fp = fingerprint(&running.host_key);
        let target = server(running.port, &folder, Some(key_file("send", 7)), Some(fp.clone()));

        let first = target.deliver(&src, "Bracket", "3mf", 1 << 20, &setup()).unwrap();
        let second = target.deliver(&src, "Bracket", "3mf", 1 << 20, &setup()).unwrap();
        assert!(first.numbered && second.numbered);
        assert!(first.path.to_string_lossy().ends_with("/prints/Bracket (2).3mf"), "{first:?}");
        assert!(second.path.to_string_lossy().ends_with("/prints/Bracket (3).3mf"), "{second:?}");
        assert_eq!(names(&folder), ["Bracket (2).3mf", "Bracket (3).3mf", "Bracket.3mf"], "no temporary file is left");
        assert_eq!(std::fs::read(folder.join("Bracket.3mf")).unwrap(), b"theirs");
        assert_eq!(std::fs::read(folder.join("Bracket (2).3mf")).unwrap(), std::fs::read(&src).unwrap());

        // A folder from the login folder, with or without `~/`.
        for given in ["~/prints/", "prints"] {
            let relative = Server { folder: given.into(), ..target.clone() };
            let sent = relative.deliver(&src, "Lid", "stl", 1 << 20, &setup()).unwrap();
            assert!(sent.path.to_string_lossy().contains(":") && sent.path.to_string_lossy().contains("/~/prints/Lid"), "{sent:?}");
        }
        assert!(names(&folder).contains(&"Lid.stl".to_string()) && names(&folder).contains(&"Lid (2).stl".to_string()));
    }

    #[test]
    fn a_wrong_key_and_a_missing_agent_are_refused_in_words() {
        let home = scratch("auth");
        let running = serving(&home, Options::default());
        let trusted = Some(fingerprint(&running.host_key));
        let error = |key: Option<PathBuf>| server(running.port, &home, key, trusted.clone()).test(&setup()).error.unwrap_or_default();

        assert!(error(Some(key_file("auth-wrong", 8))).contains("refused the key"), "{}", error(Some(key_file("auth-wrong", 8))));
        assert!(error(None).contains("no ssh-agent is running"));
        assert!(error(Some(home.join("missing"))).contains("no such key file"));
        let junk = home.join("junk");
        std::fs::write(&junk, b"not a key").unwrap();
        assert!(error(Some(junk)).contains("is not a private key"));
        assert!(names(&home).iter().all(|n| !n.starts_with(".solidcraft-test")));
    }

    #[cfg(unix)]
    #[test]
    fn the_keys_of_the_ssh_agent_log_in_without_a_key_file() {
        let home = scratch("agent");
        let running = serving(&home, Options::default());
        let socket = home.join("agent.sock");
        start_agent(&socket, vec![test_server::key(9), test_server::key(7)]);
        let target = server(running.port, &home, None, Some(fingerprint(&running.host_key)));
        let tested = target.test(&Setup { agent: Some(socket.clone()), ..setup() });
        assert!(tested.ok, "the agent's second key is the one: {tested:?}");

        let empty = home.join("empty.sock");
        start_agent(&empty, Vec::new());
        let tested = target.test(&Setup { agent: Some(empty), ..setup() });
        assert!(tested.error.unwrap_or_default().contains("holds no keys"));
    }

    #[test]
    fn a_server_that_does_not_answer_or_is_not_there_is_given_up_on_in_time() {
        // Takes the connection and never says a word.
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = silent.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let held: Vec<_> = silent.incoming().take(4).collect();
            std::thread::sleep(Duration::from_secs(60));
            drop(held);
        });
        let quick = Setup { limit: Duration::from_secs(1), ..setup() };
        let started = Instant::now();
        let tested = server(port, Path::new("/srv"), None, None).test(&quick);
        assert!(tested.error.as_deref().unwrap_or_default().contains("did not answer within 1 s"), "{tested:?}");
        assert!(started.elapsed() < Duration::from_secs(8), "given up on in time: {:?}", started.elapsed());

        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let tested = server(port, Path::new("/srv"), None, None).test(&quick);
        assert!(tested.error.as_deref().unwrap_or_default().contains("nothing answers on 127.0.0.1"), "{tested:?}");
        assert!(tested.host_key.is_none());
    }

    #[test]
    fn a_full_disk_a_missing_folder_and_a_big_file_are_refused_in_words() {
        let home = scratch("refused");
        let key = Some(key_file("refused", 7));
        let src = mesh(&home, 200_000);

        let full = serving(&home, Options { free: Some(4096), ..Options::default() });
        let target = server(full.port, &home, key.clone(), Some(fingerprint(&full.host_key)));
        let error = target.deliver(&src, "Part", "3mf", 1 << 20, &setup()).unwrap_err();
        assert!(error.contains("is full"), "{error}");

        let stops = serving(&home, Options { fail_after: Some(50_000), ..Options::default() });
        let target = server(stops.port, &home, key.clone(), Some(fingerprint(&stops.host_key)));
        let error = target.deliver(&src, "Part", "3mf", 1 << 20, &setup()).unwrap_err();
        assert!(error.contains("stopped taking the file"), "{error}");
        assert_eq!(names(&home), ["mesh.3mf"], "the half-written file is deleted");

        let gone = Server { folder: home.join("nas").to_string_lossy().into_owned(), ..target.clone() };
        assert!(gone.deliver(&src, "Part", "3mf", 1 << 20, &setup()).unwrap_err().contains("is not there"));
        let a_file = Server { folder: src.to_string_lossy().into_owned(), ..target.clone() };
        assert!(a_file.deliver(&src, "Part", "3mf", 1 << 20, &setup()).unwrap_err().contains("is not a folder"));
        let error = target.deliver(&src, "Part", "3mf", 1000, &setup()).unwrap_err();
        assert!(error.contains("more than"), "refused before connecting: {error}");
        assert_eq!(names(&home), ["mesh.3mf"]);
    }
}
