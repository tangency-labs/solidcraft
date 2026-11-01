//! Network folders for Send to Slicer (#2304): a folder the user picked once, usually a mounted
//! SMB or NFS share that a slicer on another computer reads (OrcaSlicer in a container on a home
//! server, say), or a folder on that computer reached over SFTP (`crate::remote_sftp`). Sending to
//! one copies the mesh there under a name a person can read, never replacing a file that is
//! already there, and starts no program.
//!
//! The list belongs to the session (`Session::network_folders`), like the parameter checks: the
//! application keeps it in the user's preferences and puts it there, the
//! `file.slicers.add_remote` and `file.slicers.remove_remote` commands change it, and the
//! application stores the result. Run without an app (CLI, scripts), a folder lasts as long as
//! the session. Nothing here scans the network: for a folder the operating system does the share
//! and SolidCraft writes files; for SFTP the user types the server, and its key must be one they
//! trust (`~/.ssh/known_hosts`, or the fingerprint they confirmed).

#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Most network folders kept.
pub const MAX_TARGETS: usize = 32;
/// Most characters in a network folder's name.
pub const MAX_NAME: usize = 64;
/// Most bytes in a folder's path.
pub const MAX_FOLDER: usize = 4096;
/// Largest file copied to a network folder.
pub const MAX_BYTES: u64 = 1 << 30;
/// Longest a copy to a network folder may take before the command answers without it: a share
/// whose server has gone can keep a file call waiting for minutes.
pub const TIME_LIMIT: Duration = Duration::from_secs(30);
/// Most numbered names (`Part (2).3mf`, …) tried before giving up.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_NUMBERED: u32 = 999;
/// Most characters of a file name taken from the design and body names.
const MAX_STEM: usize = 80;
/// What a network folder's id starts with in `file.slicers`.
pub const ID_PREFIX: &str = "remote:";
/// The SSH port when none is given.
pub const SSH_PORT: u16 = 22;
/// What the browser build answers: it reaches no network folder.
#[cfg(target_arch = "wasm32")]
const BROWSER: &str = "network folders can't be reached from the browser";

/// A network folder a mesh can be sent to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTarget {
    /// The name the user gave it, shown in the slicer list.
    pub name: String,
    /// Where the file goes.
    #[serde(flatten)]
    pub dest: Dest,
}

/// Where a network folder's files go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Dest {
    /// A folder on this computer, usually a mounted share.
    Folder { folder: String },
    /// A folder on another computer, reached over SFTP with the user's own SSH keys. No password
    /// is asked for or kept.
    Sftp {
        /// Who to log in as.
        user: String,
        /// The computer: a name or an IP address.
        host: String,
        /// Its SSH port.
        #[serde(default = "ssh_port")]
        port: u16,
        /// The folder there: a whole path, or one from the login folder.
        folder: String,
        /// A private key file to log in with; without one, the keys of the user's ssh-agent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
        /// The SHA256 fingerprint of the server's key that the user confirmed, for a server
        /// `~/.ssh/known_hosts` doesn't list.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_key: Option<String>,
    },
}

fn ssh_port() -> u16 {
    SSH_PORT
}

/// An SFTP server as typed: `user@host`, with `:port` when it isn't 22 (`user@[::1]:2222` for an
/// IPv6 address).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SftpAddress {
    pub user: String,
    pub host: String,
    pub port: u16,
}

impl SftpAddress {
    /// Read `user@host[:port]` (an `sftp://` in front is allowed).
    pub fn parse(text: &str) -> Result<SftpAddress, String> {
        let text = text.trim();
        let text = text.strip_prefix("sftp://").unwrap_or(text);
        let text = text.strip_suffix('/').unwrap_or(text);
        let Some((user, rest)) = text.rsplit_once('@') else {
            return Err("give the server as user@host, or user@host:port".into());
        };
        let (host, port) = if let Some(inner) = rest.strip_prefix('[') {
            let Some((host, after)) = inner.split_once(']') else { return Err(format!("{rest}: a ] is missing")) };
            match after {
                "" => (host, None),
                _ => (host, Some(after.strip_prefix(':').ok_or_else(|| format!("{rest}: the port goes after a colon"))?)),
            }
        } else {
            match rest.split_once(':') {
                Some((host, port)) if !port.contains(':') => (host, Some(port)),
                // More than one colon and no brackets: an IPv6 address on the usual port.
                _ => (rest, None),
            }
        };
        let port = match port {
            None => SSH_PORT,
            Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("{p}: a port is a number from 1 to 65535"))?,
        };
        SftpAddress::checked(user, host, port)
    }

    /// The parts, checked: a user and a host without spaces or control characters.
    pub fn checked(user: &str, host: &str, port: u16) -> Result<SftpAddress, String> {
        let (user, host) = (user.trim(), host.trim());
        if user.is_empty() {
            return Err("give the user to log in as: user@host".into());
        }
        if user.chars().count() > MAX_NAME || user.chars().any(|c| c.is_control() || c.is_whitespace() || c == ':' || c == '/') {
            return Err(format!("{user}: not a user name"));
        }
        let host_ok =
            !host.is_empty() && host.len() <= 253 && !host.starts_with('-') && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-_:%".contains(c));
        if !host_ok {
            return Err(format!("{host}: not a computer's name or address"));
        }
        if port == 0 {
            return Err("a port is a number from 1 to 65535".into());
        }
        Ok(SftpAddress { user: user.to_string(), host: host.to_string(), port })
    }
}

impl std::fmt::Display for SftpAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == SSH_PORT { write!(f, "{}@{host}", self.user) } else { write!(f, "{}@{host}:{}", self.user, self.port) }
    }
}

/// An SFTP server's key, as a Test or a send saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostKey {
    /// `SHA256:…`, as `ssh-keygen -lf` prints it on the server.
    pub fingerprint: String,
    /// The key's type, `ssh-ed25519` say.
    pub algorithm: String,
    /// `~/.ssh/known_hosts` lists it, or it is the fingerprint the user confirmed.
    pub trusted: bool,
    /// `~/.ssh/known_hosts`, or the fingerprint kept with the target, names another key for this
    /// server: it was set up again, or someone is in between.
    pub changed: bool,
    /// `~/.ssh/known_hosts` lists this server: with this key when `trusted`, else with another one,
    /// which trusting this key in SolidCraft never overrides (its line has to go first).
    pub known_hosts: bool,
}

/// What testing a network folder found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tested {
    /// A file could be written there and deleted again.
    pub ok: bool,
    /// What went wrong, in words.
    pub error: Option<String>,
    /// The SFTP server's key, once it answered.
    pub host_key: Option<HostKey>,
}

impl RemoteTarget {
    /// A folder target.
    pub fn folder_target(name: &str, folder: &str) -> RemoteTarget {
        RemoteTarget { name: name.to_string(), dest: Dest::Folder { folder: folder.to_string() } }
    }

    /// An SFTP target: `folder` on `address`, logged in to with `key` (else the ssh-agent's keys),
    /// trusting the server key of fingerprint `host_key` when `~/.ssh/known_hosts` doesn't list it.
    pub fn sftp_target(name: &str, address: &SftpAddress, folder: &str, key: Option<&str>, host_key: Option<&str>) -> RemoteTarget {
        RemoteTarget {
            name: name.to_string(),
            dest: Dest::Sftp {
                user: address.user.clone(),
                host: address.host.clone(),
                port: address.port,
                folder: folder.to_string(),
                key: key.map(str::to_string),
                host_key: host_key.map(str::to_string),
            },
        }
    }

    /// The SFTP server, for an SFTP target.
    pub fn address(&self) -> Option<SftpAddress> {
        match &self.dest {
            Dest::Folder { .. } => None,
            Dest::Sftp { user, host, port, .. } => Some(SftpAddress { user: user.clone(), host: host.clone(), port: *port }),
        }
    }

    /// Where the files go, for people: the folder, or `sftp://user@host/folder`.
    pub fn shown(&self) -> String {
        match (&self.dest, self.address()) {
            (Dest::Sftp { folder, .. }, Some(address)) => sftp_url(&address, folder),
            _ => self.folder().to_string(),
        }
    }

    /// How `file.slicers` and `file.slicers.add_remote` describe it.
    pub fn listing(&self) -> serde_json::Value {
        let mut out = serde_json::json!({"id": self.id(), "name": self.name, "remote": true, "kind": self.kind(), "folder": self.folder()});
        if let Dest::Sftp { key, host_key, .. } = &self.dest {
            out["sftp"] = serde_json::json!(self.address().map(|a| a.to_string()));
            if let Some(key) = key {
                out["key"] = serde_json::json!(key);
            }
            if let Some(host_key) = host_key {
                out["host_key"] = serde_json::json!(host_key);
            }
        }
        out
    }

    /// Its id in `file.slicers`: `remote:<name>`.
    pub fn id(&self) -> String {
        format!("{ID_PREFIX}{}", self.name)
    }

    /// The kind, as `file.slicers` names it.
    pub fn kind(&self) -> &'static str {
        match self.dest {
            Dest::Folder { .. } => "folder",
            Dest::Sftp { .. } => "sftp",
        }
    }

    /// The folder the files go to (on the server, for SFTP).
    pub fn folder(&self) -> &str {
        match &self.dest {
            Dest::Folder { folder } | Dest::Sftp { folder, .. } => folder,
        }
    }
}

/// A network folder's name as given: trimmed, 1 to [`MAX_NAME`] characters, no control characters.
pub fn check_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        Err("give the network folder a name".into())
    } else if name.chars().count() > MAX_NAME {
        Err(format!("a network folder's name has {MAX_NAME} characters at most"))
    } else if name.chars().any(char::is_control) {
        Err("a network folder's name can't hold control characters".into())
    } else {
        Ok(name)
    }
}

/// A folder's path as given: trimmed, whole (absolute), at most [`MAX_FOLDER`] bytes, no control
/// characters. Whether it is there is not looked at: a share may be mounted later.
pub fn check_folder(folder: &str) -> Result<&str, String> {
    let folder = folder.trim();
    if folder.is_empty() {
        Err("give the folder".into())
    } else if folder.len() > MAX_FOLDER {
        Err(format!("a folder's path has {MAX_FOLDER} bytes at most"))
    } else if folder.chars().any(char::is_control) {
        Err("a folder's path can't hold control characters".into())
    } else if !Path::new(folder).is_absolute() {
        Err(format!("{folder}: give the folder's whole path"))
    } else {
        Ok(folder)
    }
}

/// A folder on an SFTP server as given: trimmed, not empty, at most [`MAX_FOLDER`] bytes, no control
/// characters. A path that doesn't start with `/` is taken from the login folder.
pub fn check_remote_folder(folder: &str) -> Result<&str, String> {
    let folder = folder.trim();
    if folder.is_empty() {
        Err("give the folder on the server".into())
    } else if folder.len() > MAX_FOLDER {
        Err(format!("a folder's path has {MAX_FOLDER} bytes at most"))
    } else if folder.chars().any(char::is_control) {
        Err("a folder's path can't hold control characters".into())
    } else {
        Ok(folder)
    }
}

/// A private key file's path as given: trimmed, whole (or from the home folder, `~/.ssh/id_ed25519`),
/// at most [`MAX_FOLDER`] bytes.
pub fn check_key_file(key: &str) -> Result<&str, String> {
    let key = key.trim();
    if key.is_empty() {
        Err("give the key file".into())
    } else if key.len() > MAX_FOLDER || key.chars().any(char::is_control) {
        Err("a key file's path is at most 4096 bytes, without control characters".into())
    } else if !(Path::new(key).is_absolute() || key.starts_with("~/") || key.starts_with("~\\")) {
        Err(format!("{key}: give the key file's whole path, or one from ~/"))
    } else {
        Ok(key)
    }
}

/// A server key's fingerprint as `ssh-keygen -lf` prints it: `SHA256:` and 43 base64 characters.
pub fn check_fingerprint(text: &str) -> Result<&str, String> {
    let text = text.trim();
    let base64 = text.strip_prefix("SHA256:").unwrap_or_default();
    if (43..=44).contains(&base64.len()) && base64.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)) {
        Ok(text)
    } else {
        Err(format!("{text}: a server key's fingerprint looks like SHA256:… (Test shows it)"))
    }
}

/// `sftp://user@host[:port]/folder`; a folder from the login folder (`prints`, `~/prints`) shows
/// as `/~/prints`.
pub fn sftp_url(address: &SftpAddress, path: &str) -> String {
    if path.starts_with('/') {
        format!("sftp://{address}{path}")
    } else if path == "~" || path.starts_with("~/") {
        format!("sftp://{address}/{path}")
    } else {
        format!("sftp://{address}/~/{path}")
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The network folders of `list` the commands may use (the application's preferences, which a
/// person can edit): one that is not valid, or repeats a name, is left out, and so is any past
/// [`MAX_TARGETS`].
pub fn sanitized(list: &[RemoteTarget]) -> Vec<RemoteTarget> {
    let mut kept: Vec<RemoteTarget> = Vec::new();
    for t in list.iter().filter_map(checked) {
        if kept.len() < MAX_TARGETS && !kept.iter().any(|k| same_name(&k.name, &t.name)) {
            kept.push(t);
        }
    }
    kept
}

/// `t` with every part checked and trimmed, or nothing when a part is not valid.
fn checked(t: &RemoteTarget) -> Option<RemoteTarget> {
    let name = check_name(&t.name).ok()?;
    match &t.dest {
        Dest::Folder { folder } => Some(RemoteTarget::folder_target(name, check_folder(folder).ok()?)),
        Dest::Sftp { user, host, port, folder, key, host_key } => {
            let address = SftpAddress::checked(user, host, *port).ok()?;
            let folder = check_remote_folder(folder).ok()?;
            let key = match key {
                Some(key) => Some(check_key_file(key).ok()?),
                None => None,
            };
            let host_key = match host_key {
                Some(fingerprint) => Some(check_fingerprint(fingerprint).ok()?),
                None => None,
            };
            Some(RemoteTarget::sftp_target(name, &address, folder, key, host_key))
        }
    }
}

/// Add a network folder to `list`, replacing the one of the same name (true when it did).
pub fn add(list: &mut Vec<RemoteTarget>, target: RemoteTarget) -> Result<bool, String> {
    if let Some(old) = list.iter_mut().find(|t| same_name(&t.name, &target.name)) {
        *old = target;
        return Ok(true);
    }
    if list.len() >= MAX_TARGETS {
        return Err(format!("there are {MAX_TARGETS} network folders already: remove one first"));
    }
    list.push(target);
    Ok(false)
}

/// Remove the network folder `spec` names (its name or id) from `list`; the one removed.
pub fn remove(list: &mut Vec<RemoteTarget>, spec: &str) -> Option<RemoteTarget> {
    let name = spec.trim().strip_prefix(ID_PREFIX).unwrap_or(spec);
    let at = list.iter().position(|t| same_name(&t.name, name))?;
    Some(list.remove(at))
}

/// The network folder of `list` that `spec` names: its id (`remote:<name>`) or its name, in any case.
pub fn resolve<'a>(list: &'a [RemoteTarget], spec: &str) -> Option<&'a RemoteTarget> {
    let name = spec.trim().strip_prefix(ID_PREFIX).unwrap_or(spec);
    list.iter().find(|t| same_name(&t.name, name))
}

/// Whether `spec` is written as a network folder's id, so it names nothing else.
pub fn is_id(spec: &str) -> bool {
    spec.trim().starts_with(ID_PREFIX)
}

/// A file name (no extension) a person can read for `bodies` of a design: the design's name, and
/// the body's after it when one body other than the design's namesake is sent.
pub fn readable_stem(design: &str, bodies: &[String]) -> String {
    let design = design.trim();
    let name = match bodies {
        [one] if !one.trim().is_empty() && one.trim() != design => {
            if design.is_empty() {
                one.trim().to_string()
            } else {
                format!("{design} - {}", one.trim())
            }
        }
        _ => design.to_string(),
    };
    safe_stem(&name)
}

/// `name` as a file name every system takes: the characters Windows and SMB refuse and control
/// characters become `_`, leading and trailing dots and spaces go (no hidden files), at most
/// [`MAX_STEM`] characters, `design` when nothing is left, and an `_` after a Windows device name.
pub fn safe_stem(name: &str) -> String {
    let cleaned: String = name.chars().take(MAX_STEM).map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let stem = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let stem = if stem.is_empty() { "design" } else { stem };
    let device = stem.split('.').next().unwrap_or_default().trim().to_ascii_uppercase();
    let numbered_device = (device.starts_with("COM") || device.starts_with("LPT"))
        && device.len() == 4
        && device.as_bytes().get(3).is_some_and(|b| (b'1'..=b'9').contains(b));
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered_device { format!("{stem}_") } else { stem.to_string() }
}

/// `<stem>.<ext>` for the first name, then `<stem> (2).<ext>` and so on.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn numbered(stem: &str, ext: &str, n: u32) -> String {
    if n <= 1 { format!("{stem}.{ext}") } else { format!("{stem} ({n}).{ext}") }
}

/// Where a file went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered {
    /// Its path, or `sftp://user@host/folder/name` on an SFTP server.
    pub path: PathBuf,
    /// A file of the first name was there, so a number was added.
    pub numbered: bool,
}

/// Copy the file at `src` into the network folder as `<stem>.<ext>` (numbered when that name is
/// taken), within [`TIME_LIMIT`] (more for a big file over SFTP) and [`MAX_BYTES`]. The error says
/// what went wrong in words.
pub fn deliver(target: &RemoteTarget, src: &Path, stem: &str, ext: &str) -> Result<Delivered, String> {
    match &target.dest {
        Dest::Folder { folder } => deliver_within(Path::new(folder), src, stem, ext, max_bytes(), TIME_LIMIT),
        #[cfg(target_arch = "wasm32")]
        Dest::Sftp { .. } => {
            let _ = (src, stem, ext);
            Err(BROWSER.into())
        }
        #[cfg(not(target_arch = "wasm32"))]
        Dest::Sftp { .. } => {
            let server = crate::remote_sftp::Server::of(target).ok_or_else(|| "not an SFTP server".to_string())?;
            server.deliver(src, stem, ext, max_bytes(), &crate::remote_sftp::Setup::user())
        }
    }
}

/// Test that a mesh can be sent to `target`: write a small file there and delete it again. For an
/// SFTP server, `confirmed` is a fingerprint the user accepted just now, trusted for this test
/// only; the answer carries the server's key either way.
pub fn test_target(target: &RemoteTarget, confirmed: Option<&str>) -> Tested {
    match &target.dest {
        Dest::Folder { folder } => match probe(folder) {
            Ok(()) => Tested { ok: true, ..Tested::default() },
            Err(e) => Tested { ok: false, error: Some(e), host_key: None },
        },
        #[cfg(target_arch = "wasm32")]
        Dest::Sftp { .. } => {
            let _ = confirmed;
            Tested { ok: false, error: Some(BROWSER.into()), host_key: None }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Dest::Sftp { .. } => match crate::remote_sftp::Server::of(target) {
            Some(server) => {
                let mut setup = crate::remote_sftp::Setup::user();
                setup.confirmed = confirmed.map(str::to_string);
                server.test(&setup)
            }
            None => Tested { ok: false, error: Some("not an SFTP server".into()), host_key: None },
        },
    }
}

/// The size cap of [`deliver`]; tests of the commands make it small.
fn max_bytes() -> u64 {
    #[cfg(test)]
    if let Some(made_up) = *TEST_MAX_BYTES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
        return made_up;
    }
    MAX_BYTES
}

/// Tests of the commands set the size cap to one they made up.
#[cfg(test)]
pub(crate) static TEST_MAX_BYTES: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
/// The size cap is process-wide: tests that change it, or send to a network folder, take turns.
#[cfg(test)]
pub(crate) static TEST_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// [`deliver`] into `folder` with a size cap and a time limit of its own.
pub fn deliver_within(folder: &Path, src: &Path, stem: &str, ext: &str, max: u64, limit: Duration) -> Result<Delivered, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (folder, src, stem, ext, max, limit);
        Err(BROWSER.into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (folder, src, stem, ext) = (folder.to_path_buf(), src.to_path_buf(), stem.to_string(), ext.to_string());
        let shown = folder.display().to_string();
        within(limit, &shown, move || copy_into(&folder, &src, &stem, &ext, max))
    }
}

/// Copies going on in the background, by ticket (`file.send_to_slicer` with `wait: "return"`, which
/// the window uses so a share that does not answer never holds it up).
#[cfg(not(target_arch = "wasm32"))]
type Copying = std::sync::mpsc::Receiver<Result<Delivered, String>>;
#[cfg(not(target_arch = "wasm32"))]
static SENDING: std::sync::Mutex<Vec<(u64, Copying)>> = std::sync::Mutex::new(Vec::new());
/// Most copies kept going at once; past it, the oldest one's answer is dropped.
#[cfg(not(target_arch = "wasm32"))]
const MAX_SENDING: usize = 16;

/// [`deliver`] on a thread of its own; the ticket to ask [`poll`] about it.
pub fn deliver_in_background(target: &RemoteTarget, src: &Path, stem: &str, ext: &str) -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let ticket = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_arch = "wasm32")]
    let _ = (target, src, stem, ext);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let (target, src, stem, ext) = (target.clone(), src.to_path_buf(), stem.to_string(), ext.to_string());
        let started = std::thread::Builder::new().name("network-folder-send".into()).spawn(move || {
            let _ = tx.send(deliver(&target, &src, &stem, &ext));
        });
        let mut sending = SENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match started {
            Ok(_) => {
                if sending.len() >= MAX_SENDING {
                    sending.remove(0);
                }
                sending.push((ticket, rx));
            }
            // No thread: the answer is that it could not start (the receiver is dropped with its sender).
            Err(e) => {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = tx.send(Err(format!("couldn't start the copy ({e})")));
                sending.push((ticket, rx));
            }
        }
    }
    ticket
}

/// How the copy of `ticket` went: `None` while it goes on, then its answer, once.
pub fn poll(ticket: u64) -> Option<Result<Delivered, String>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ticket;
        Some(Err(BROWSER.into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::mpsc::TryRecvError;
        let mut sending = SENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(at) = sending.iter().position(|(t, _)| *t == ticket) else {
            return Some(Err("the copy is no longer followed".into()));
        };
        let answer = match sending.get(at).map(|(_, rx)| rx.try_recv()) {
            Some(Err(TryRecvError::Empty)) => return None,
            Some(Ok(answer)) => answer,
            Some(Err(TryRecvError::Disconnected)) | None => Err("the copy stopped unexpectedly".into()),
        };
        sending.remove(at);
        Some(answer)
    }
}

/// Write a small file in `folder` and delete it again, within [`TIME_LIMIT`]: whether a mesh can be
/// sent there. The error says what went wrong in words.
pub fn probe(folder: &str) -> Result<(), String> {
    probe_within(Path::new(folder), TIME_LIMIT)
}

/// [`probe`] with a time limit of its own.
pub fn probe_within(folder: &Path, limit: Duration) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (folder, limit);
        Err(BROWSER.into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let folder = folder.to_path_buf();
        let shown = folder.display().to_string();
        within(limit, &shown, move || probe_now(&folder))
    }
}

/// Run `work` on a thread of its own and wait at most `limit` for it. A file call on a share whose
/// server has gone can block for minutes; the thread is left to finish on its own then.
#[cfg(not(target_arch = "wasm32"))]
fn within<T: Send + 'static>(limit: Duration, folder: &str, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("network-folder".into())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| format!("couldn't start writing to {folder} ({e})"))?;
    match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("{folder} did not answer within {} s (is its server on?); the file may still arrive", limit.as_secs()))
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(format!("writing to {folder} stopped unexpectedly")),
    }
}

/// What went wrong with `folder`, in words.
#[cfg(not(target_arch = "wasm32"))]
fn explain(folder: &str, e: &io::Error) -> String {
    match e.kind() {
        ErrorKind::NotFound => format!("{folder} is not there (is the share mounted?)"),
        ErrorKind::PermissionDenied => format!("{folder}: no permission to write there"),
        ErrorKind::StorageFull => format!("{folder} is full"),
        ErrorKind::ReadOnlyFilesystem => format!("{folder} is read-only"),
        ErrorKind::NotADirectory => format!("{folder} is not a folder"),
        _ => format!("{folder}: {e}"),
    }
}

/// A folder that is there and is a folder. A folder is never made: at an unmounted share's mount
/// point that would put the files on this computer instead.
#[cfg(not(target_arch = "wasm32"))]
fn existing_folder(folder: &Path) -> Result<String, String> {
    let shown = folder.display().to_string();
    match std::fs::metadata(folder) {
        Ok(m) if m.is_dir() => Ok(shown),
        Ok(_) => Err(format!("{shown} is not a folder")),
        Err(e) => Err(explain(&shown, &e)),
    }
}

/// A suffix for temporary names that no other send, in this process or another, uses at once.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn unique() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or_default();
    format!("{}-{n}-{nanos}", std::process::id())
}

#[cfg(not(target_arch = "wasm32"))]
fn copy_into(folder: &Path, src: &Path, stem: &str, ext: &str, max: u64) -> Result<Delivered, String> {
    let shown = existing_folder(folder)?;
    let size = std::fs::metadata(src).map_err(|e| format!("{}: {e}", src.display()))?.len();
    if size > max {
        let mb = |b: u64| b.div_ceil(1_000_000);
        return Err(format!("the file is {} MB, more than the {} MB a network folder takes", mb(size), mb(max)));
    }
    // Copied under a hidden temporary name first, so a slicer watching the folder never reads half a file.
    let tmp = folder.join(format!(".{stem}.{ext}.{}.part", unique()));
    if let Err(e) = std::fs::copy(src, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(explain(&shown, &e));
    }
    let placed = place(&tmp, folder, stem, ext);
    if placed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    placed.map_err(|e| explain(&shown, &e))
}

/// Give the temporary file `tmp` the first free name, never replacing a file. A hard link takes a
/// name only when it is free, in one step; a share without hard links gets the name reserved with
/// an empty file first, which the temporary file then replaces.
#[cfg(not(target_arch = "wasm32"))]
fn place(tmp: &Path, folder: &Path, stem: &str, ext: &str) -> io::Result<Delivered> {
    let mut link = true;
    for n in 1..=MAX_NUMBERED {
        let path = folder.join(numbered(stem, ext, n));
        if link {
            match std::fs::hard_link(tmp, &path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(tmp);
                    return Ok(Delivered { path, numbered: n > 1 });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => link = false,
            }
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => {
                if let Err(e) = std::fs::rename(tmp, &path) {
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
                return Ok(Delivered { path, numbered: n > 1 });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(ErrorKind::AlreadyExists, format!("{MAX_NUMBERED} files named {stem} are there already")))
}

#[cfg(not(target_arch = "wasm32"))]
fn probe_now(folder: &Path) -> Result<(), String> {
    use std::io::Write;
    let shown = existing_folder(folder)?;
    let file = folder.join(format!(".solidcraft-test-{}.tmp", unique()));
    let data = b"SolidCraft checks that it can write here; this file is deleted at once.\n";
    let written =
        std::fs::OpenOptions::new().write(true).create_new(true).open(&file).and_then(|mut f| f.write_all(data).and_then(|()| f.sync_all()));
    let read = written.and_then(|()| std::fs::metadata(&file)).map(|m| m.len());
    let _ = std::fs::remove_file(&file);
    match read {
        Ok(len) if len == data.len() as u64 => Ok(()),
        Ok(len) => Err(format!("{shown}: wrote {} bytes and found {len}", data.len())),
        Err(e) => Err(explain(&shown, &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-remote-slicers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn file_names_are_readable_and_safe_everywhere() {
        let one = |b: &str| vec![b.to_string()];
        assert_eq!(readable_stem("Bracket", &one("Arm")), "Bracket - Arm");
        assert_eq!(readable_stem("Bracket", &one("Bracket")), "Bracket", "the body named after the design adds nothing");
        assert_eq!(readable_stem("Bracket", &[]), "Bracket");
        assert_eq!(readable_stem("Bracket", &["Arm".to_string(), "Base".to_string()]), "Bracket");
        assert_eq!(readable_stem("", &one("Arm")), "Arm");
        assert_eq!(safe_stem("a/b:c*d?\"e<f>g|h\\i"), "a_b_c_d__e_f_g_h_i");
        assert_eq!(safe_stem("  ..hidden.  "), "hidden", "no hidden files, no trailing dots Windows drops");
        assert_eq!(safe_stem("tab\there"), "tab_here");
        assert_eq!(safe_stem(""), "design");
        assert_eq!(safe_stem("..."), "design");
        assert_eq!(safe_stem("con"), "con_");
        assert_eq!(safe_stem("LPT1.part"), "LPT1.part_");
        assert_eq!(safe_stem("COM0"), "COM0", "COM0 is not a device");
        assert_eq!(safe_stem("Console"), "Console");
        assert_eq!(safe_stem(&"x".repeat(200)).chars().count(), MAX_STEM);
        assert_eq!(safe_stem("Peça"), "Peça", "letters of any language are kept");
    }

    #[test]
    fn names_and_folders_are_checked() {
        assert_eq!(check_name("  Orca on homelab "), Ok("Orca on homelab"));
        assert!(check_name("   ").is_err());
        assert!(check_name(&"n".repeat(MAX_NAME + 1)).is_err());
        assert!(check_name("a\u{7}b").is_err());
        assert!(check_folder("relative/folder").is_err());
        assert!(check_folder("").is_err());
        assert!(check_folder(&format!("/{}", "a".repeat(MAX_FOLDER))).is_err());
        let abs = std::env::temp_dir();
        assert!(check_folder(&abs.to_string_lossy()).is_ok());
    }

    #[test]
    fn the_list_adds_replaces_resolves_and_removes_by_name_or_id() {
        let folder = std::env::temp_dir().to_string_lossy().into_owned();
        let mut list = sanitized(&[
            RemoteTarget::folder_target(" Orca ", &folder),
            RemoteTarget::folder_target("orca", &folder),
            RemoteTarget::folder_target("", &folder),
            RemoteTarget::folder_target("Relative", "not/whole"),
        ]);
        assert_eq!(list, [RemoteTarget::folder_target("Orca", &folder)], "trimmed; a repeated name and invalid entries are left out");
        assert_eq!(resolve(&list, "ORCA").map(|t| t.name.as_str()), Some("Orca"));
        assert_eq!(resolve(&list, "remote:Orca").map(RemoteTarget::id), Some("remote:Orca".into()));
        assert!(resolve(&list, "Cura").is_none());
        assert_eq!(add(&mut list, RemoteTarget::folder_target("Cura", &folder)), Ok(false));
        assert_eq!(add(&mut list, RemoteTarget::folder_target("CURA", "/elsewhere")), Ok(true));
        assert_eq!(resolve(&list, "cura").map(RemoteTarget::folder), Some("/elsewhere"));
        assert_eq!(remove(&mut list, "remote:cura").map(|t| t.name), Some("CURA".into()));
        assert!(remove(&mut list, "cura").is_none());
        let mut full = sanitized(&(0..MAX_TARGETS + 5).map(|i| RemoteTarget::folder_target(&format!("T{i}"), &folder)).collect::<Vec<_>>());
        assert_eq!(full.len(), MAX_TARGETS);
        assert!(add(&mut full, RemoteTarget::folder_target("One more", &folder)).is_err());
    }

    #[test]
    fn a_target_round_trips_through_the_preferences_json() {
        let t = RemoteTarget::folder_target("Orca", "/mnt/orca");
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v, serde_json::json!({"name": "Orca", "kind": "folder", "folder": "/mnt/orca"}));
        assert_eq!(serde_json::from_value::<RemoteTarget>(v).unwrap(), t);

        let address = SftpAddress::parse("maker@printbox:2222").unwrap();
        let t = RemoteTarget::sftp_target("Orca box", &address, "/srv/prints", None, Some(&format!("SHA256:{}", "A".repeat(43))));
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["kind"], "sftp");
        assert!(v.get("key").is_none(), "no key file: the ssh-agent's keys, and nothing about it is kept");
        assert_eq!(serde_json::from_value::<RemoteTarget>(v).unwrap(), t);
        let old = serde_json::json!({"name": "Box", "kind": "sftp", "user": "maker", "host": "printbox", "folder": "prints"});
        assert_eq!(serde_json::from_value::<RemoteTarget>(old).unwrap().address().unwrap().port, SSH_PORT, "the port defaults to 22");
    }

    #[test]
    fn sftp_servers_are_read_shown_and_checked() {
        let parse = |t: &str| SftpAddress::parse(t).map(|a| a.to_string());
        assert_eq!(parse(" maker@printbox "), Ok("maker@printbox".into()));
        assert_eq!(parse("maker@printbox:22"), Ok("maker@printbox".into()), "port 22 is not written");
        assert_eq!(parse("sftp://maker@192.168.1.20:2222/"), Ok("maker@192.168.1.20:2222".into()));
        assert_eq!(parse("maker@[fe80::1]:2222"), Ok("maker@[fe80::1]:2222".into()));
        assert_eq!(parse("maker@fe80::1"), Ok("maker@[fe80::1]".into()), "an IPv6 address without brackets is on port 22");
        assert_eq!(SftpAddress::parse("me@work@printbox").unwrap().user, "me@work", "the last @ separates the host");
        for bad in [
            "printbox",
            "@printbox",
            "maker@",
            "maker@print box",
            "maker@-oProxyCommand=x",
            "maker@box:0",
            "maker@box:99999",
            "maker@box:ssh",
            "ma ker@box",
            "maker@[::1",
        ] {
            assert!(SftpAddress::parse(bad).is_err(), "{bad}");
        }
        let a = SftpAddress::parse("maker@printbox").unwrap();
        assert_eq!(sftp_url(&a, "/srv/prints/Part.3mf"), "sftp://maker@printbox/srv/prints/Part.3mf");
        assert_eq!(sftp_url(&a, "prints"), "sftp://maker@printbox/~/prints");
        assert_eq!(sftp_url(&a, "~/prints"), "sftp://maker@printbox/~/prints");
        assert_eq!(sftp_url(&a, "~"), "sftp://maker@printbox/~");
        assert_eq!(RemoteTarget::sftp_target("Box", &a, "prints", None, None).shown(), "sftp://maker@printbox/~/prints");

        let fp = format!("SHA256:{}", "x".repeat(43));
        assert_eq!(check_fingerprint(&format!(" {fp} ")), Ok(fp.as_str()));
        for bad in ["", "SHA256:", "MD5:aa:bb", "SHA256:short", &format!("SHA256:{}", "!".repeat(43))] {
            assert!(check_fingerprint(bad).is_err(), "{bad}");
        }
        assert!(check_remote_folder("  ").is_err() && check_remote_folder("a\nb").is_err() && check_remote_folder(" ~/prints ") == Ok("~/prints"));
        assert!(check_key_file("id_ed25519").is_err(), "a key file's whole path");
        assert_eq!(check_key_file(" ~/.ssh/id_ed25519 "), Ok("~/.ssh/id_ed25519"));

        // A target that comes back from the preferences broken is dropped, the rest kept.
        let good = RemoteTarget::sftp_target("Box", &a, "prints", None, None);
        let mut broken = good.clone();
        broken.name = "Broken".into();
        if let Dest::Sftp { host, .. } = &mut broken.dest {
            *host = "bad host".into();
        }
        let mut bad_key = good.clone();
        bad_key.name = "Bad key".into();
        if let Dest::Sftp { key, .. } = &mut bad_key.dest {
            *key = Some("relative".into());
        }
        assert_eq!(sanitized(&[broken, good.clone(), bad_key]), vec![good]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_replaced_or_left_behind() {
        let root = scratch("numbered");
        let (src, share) = (root.join("part.3mf"), root.join("share"));
        std::fs::create_dir_all(&share).unwrap();
        std::fs::write(share.join("Bracket.3mf"), b"theirs").unwrap();
        std::fs::write(&src, b"ours").unwrap();
        let send = || deliver_within(&share, &src, "Bracket", "3mf", 1 << 20, Duration::from_secs(10));
        let first = send().unwrap();
        let second = send().unwrap();
        assert_eq!((first.path.file_name().unwrap(), first.numbered), (std::ffi::OsStr::new("Bracket (2).3mf"), true));
        assert_eq!(second.path.file_name().unwrap(), "Bracket (3).3mf");
        assert_eq!(std::fs::read(share.join("Bracket.3mf")).unwrap(), b"theirs", "the file that was there is untouched");
        assert_eq!(std::fs::read(&second.path).unwrap(), b"ours");
        assert_eq!(names(&share), ["Bracket (2).3mf", "Bracket (3).3mf", "Bracket.3mf"], "no temporary file is left");
        let fresh = deliver_within(&share, &src, "Base", "stl", 1 << 20, Duration::from_secs(10)).unwrap();
        assert_eq!((fresh.path.file_name().unwrap(), fresh.numbered), (std::ffi::OsStr::new("Base.stl"), false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_missing_folder_a_file_and_a_big_file_are_refused_in_words() {
        let root = scratch("refused");
        let src = root.join("part.3mf");
        std::fs::write(&src, vec![0u8; 2_000]).unwrap();
        let gone = root.join("not-mounted");
        let e = deliver_within(&gone, &src, "Part", "3mf", 1 << 20, Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("is not there") && e.contains("mounted"), "{e}");
        assert!(!gone.exists(), "a missing folder is never made");
        let e = deliver_within(&src, &src, "Part", "3mf", 1 << 20, Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("not a folder"), "{e}");
        let share = root.join("share");
        std::fs::create_dir_all(&share).unwrap();
        let e = deliver_within(&share, &src, "Part", "3mf", 1_000, Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("MB"), "{e}");
        assert!(names(&share).is_empty(), "nothing was written");
        assert!(probe_within(&gone, Duration::from_secs(10)).unwrap_err().contains("is not there"));
        probe_within(&share, Duration::from_secs(10)).unwrap();
        assert!(names(&share).is_empty(), "the test file is deleted");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_without_write_permission_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("readonly");
        let (src, share) = (root.join("part.3mf"), root.join("share"));
        std::fs::write(&src, b"ours").unwrap();
        std::fs::create_dir_all(&share).unwrap();
        std::fs::set_permissions(&share, std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root may write anywhere; the check means nothing then.
        if std::fs::write(share.join("probe"), b"x").is_err() {
            let e = deliver_within(&share, &src, "Part", "3mf", 1 << 20, Duration::from_secs(10)).unwrap_err();
            assert!(e.contains("no permission"), "{e}");
            assert!(probe_within(&share, Duration::from_secs(10)).unwrap_err().contains("no permission"));
        }
        std::fs::set_permissions(&share, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_copy_in_the_background_is_answered_once() {
        let root = scratch("background");
        let (src, share) = (root.join("part.3mf"), root.join("share"));
        std::fs::create_dir_all(&share).unwrap();
        std::fs::write(&src, b"ours").unwrap();
        let target = RemoteTarget::folder_target("Share", &share.to_string_lossy());
        let ticket = deliver_in_background(&target, &src, "Bracket", "3mf");
        let answer = (0..500)
            .find_map(|_| {
                let answer = poll(ticket);
                if answer.is_none() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                answer
            })
            .unwrap();
        assert_eq!(answer.unwrap().path, share.join("Bracket.3mf"));
        assert!(poll(ticket).unwrap().is_err(), "answered once");
        assert!(poll(u64::MAX).unwrap().is_err(), "a ticket nobody was given");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_folder_that_does_not_answer_is_given_up_on() {
        let e = within(Duration::from_millis(50), "/mnt/gone", || {
            std::thread::sleep(Duration::from_millis(500));
            Ok(())
        })
        .unwrap_err();
        assert!(e.contains("did not answer") && e.contains("/mnt/gone"), "{e}");
    }
}
