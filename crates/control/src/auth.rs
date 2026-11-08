//! The control channel's per-launch token (docs/control-protocol.md). An app listening for
//! control makes a random token, writes it to a file only the user can read
//! ([`App::token_path`]) and refuses every connection whose first request doesn't carry it, so
//! other users on the same machine (loopback is shared, on Windows across remote-desktop sessions
//! too) can't drive the app. Clients find it with [`App::client_token`]. Next to it, `<port>.pid`
//! names the process that wrote it, so a client refused with a leftover token can say the file is
//! stale.

use std::ffi::OsString;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Shortest token accepted from the app's `CONTROL_TOKEN` variable (hex characters).
pub const MIN_TOKEN_LEN: usize = 32;

/// Ports tried from the app's default port upward when it is taken (a second window, another
/// program).
pub const PORT_TRIES: u16 = 16;

/// Which app a control channel belongs to: where its token files live, its environment and its
/// name in messages.
#[derive(Clone, Copy)]
pub struct App {
    /// The app's name in messages ("restart SolidCraft").
    pub name: &'static str,
    /// The per-user data folder; the token files go in its `control` folder.
    pub data_dir: fn() -> Option<PathBuf>,
    /// The app's environment variable `name` (`CONTROL_DIR`, `CONTROL_TOKEN`), with the app's prefix.
    pub env: fn(&str) -> Option<OsString>,
    /// A time in seconds since the Unix epoch, as the app writes UTC times in messages.
    pub utc: fn(u64) -> String,
    /// The port the control channel tries first when the launch gives none. Below the ephemeral
    /// ranges of Linux (32768+) and Windows/macOS (49152+), so the system never hands it to an
    /// outgoing connection.
    pub default_port: u16,
}

/// Where the control channel listens, from the launch's `--control`/environment port and the
/// "Let AI agents connect" preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    /// Exactly this port (`--control PORT`, the app's `CONTROL_PORT`); no fallback.
    Exact(u16),
    /// The app's [`App::default_port`], or the next free one of [`PORT_TRIES`].
    Default,
}

/// What a launch listens on. An explicit port wins over the preference, so a scripted launch
/// works with the setting off; with the setting off and no port, nothing listens.
pub fn plan(explicit: Option<u16>, enabled: bool) -> Option<Listen> {
    match explicit {
        Some(port) => Some(Listen::Exact(port)),
        None if enabled => Some(Listen::Default),
        None => None,
    }
}

/// Bind a loopback-only listener on `first`, or on the next free port when it is taken, up to
/// `tries` ports in all.
pub fn bind_with_fallback(first: u16, tries: u16) -> Result<std::net::TcpListener> {
    let mut last = Error::new(ErrorKind::AddrInUse, "no port to try");
    for port in (first..=u16::MAX).take(usize::from(tries.max(1))) {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok(l),
            // In use; Windows reports a port another program holds exclusively as denied.
            Err(e) if matches!(e.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied) => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

impl App {
    /// The folder of the token files: `<data folder>/control` (the app's `CONTROL_DIR` replaces it).
    fn control_dir(&self) -> Option<PathBuf> {
        match self.custom_control_dir() {
            Some(d) => Some(PathBuf::from(d)),
            None => Some((self.data_dir)()?.join("control")),
        }
    }

    /// The folder the app's `CONTROL_DIR` names in place of the default one.
    fn custom_control_dir(&self) -> Option<OsString> {
        (self.env)("CONTROL_DIR").filter(|d| !d.is_empty())
    }

    /// The token file of the app listening on `port`: `<data folder>/control/<port>.token`
    /// (the app's `CONTROL_DIR` replaces the folder).
    pub fn token_path(&self, port: u16) -> Option<PathBuf> {
        Some(self.control_dir()?.join(format!("{port}.token")))
    }

    /// The discovery file: the port of the most recently started app, so a client with no port
    /// finds it. It holds only the port; the token stays in its own file.
    pub fn current_port_path(&self) -> Option<PathBuf> {
        Some(self.control_dir()?.join("current.port"))
    }

    /// Record `port` as the running app's, readable only by the user like the token files.
    pub fn write_current_port(&self, port: u16) -> Result<PathBuf> {
        let path = self.current_port_path().ok_or_else(|| Error::new(ErrorKind::NotFound, "no data folder"))?;
        self.write_token_file(&path, &port.to_string())?;
        Ok(path)
    }

    /// The port the discovery file names, if there is one.
    pub fn read_current_port(&self) -> Option<u16> {
        parse_port(&std::fs::read_to_string(self.current_port_path()?).ok().filter(|t| t.len() <= 64)?)
    }

    /// Remove the discovery file (and its process file) at exit, unless a later app has taken it
    /// over: it names another port, or another launch now holds this port's token file. Under
    /// Flatpak every instance has its own loopback, so two windows can listen on the same port
    /// number and write the same file names; the one closing first must leave the other's (#8794).
    /// Run it after [`remove_token_file`], which takes this launch's own token file away first.
    pub fn remove_current_port(&self, port: u16, token: &str) {
        if let (Some(current), Some(file)) = (self.current_port_path(), self.token_path(port)) {
            remove_discovery(&current, &file, port, token);
        }
    }

    /// A token set on purpose with the app's `CONTROL_TOKEN` (test harnesses, sandboxed packages
    /// whose data folder the client can't see), if it is long enough.
    pub fn env_token(&self) -> Option<String> {
        (self.env)("CONTROL_TOKEN").and_then(|v| v.into_string().ok()).map(|t| t.trim().to_string()).filter(|t| t.len() >= MIN_TOKEN_LEN)
    }

    /// The token a client sends to the app on `port`: `explicit` (`--token`), else the app's
    /// `CONTROL_TOKEN`, else the app's token file.
    pub fn client_token(&self, port: u16, explicit: Option<&str>) -> Option<ClientToken> {
        if let Some(t) = explicit.map(str::trim).filter(|t| !t.is_empty()) {
            return Some(ClientToken { token: t.to_string(), file: None, app: *self });
        }
        if let Some(t) = self.env_token() {
            return Some(ClientToken { token: t, file: None, app: *self });
        }
        // Read directly, not through any agent's file scope: the app's own file.
        read_token_file(&self.token_path(port)?, *self)
    }

    /// Write `token` to `path`, readable and writable only by the user: the folder is made `0700`
    /// and the file `0600` on Unix; on Windows the file's inherited permissions are replaced by one
    /// entry for the user (`icacls`) before the token goes in; when that fails, the still empty file
    /// is removed and no token is written. A file left by an earlier launch is replaced. The process
    /// file beside it ([`pid_path`]) records this process, best effort.
    pub fn write_token_file(&self, path: &Path, token: &str) -> Result<()> {
        #[cfg(windows)]
        let restrict = |p: &Path| restrict_to_user(p, self.custom_control_dir().is_some());
        #[cfg(not(windows))]
        let restrict = |_: &Path| Ok(());
        write_token_file_with(path, token, restrict)
    }
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|p| *p != 0)
}

/// [`App::remove_current_port`] on given files: `current` is the discovery file, `file` the token
/// file of `port`.
fn remove_discovery(current: &Path, file: &Path, port: u16, token: &str) {
    let named = std::fs::read_to_string(current).ok().filter(|t| t.len() <= 64).and_then(|t| parse_port(&t));
    if named != Some(port) || held_by_another(file, token) {
        return;
    }
    let _ = std::fs::remove_file(current);
    let _ = std::fs::remove_file(pid_path(current));
}

/// A client token and, when it came from a token file, where that file is.
pub struct ClientToken {
    pub token: String,
    pub file: Option<TokenSource>,
    /// The app it is for, named in an explanation.
    pub app: App,
}

/// The token file a client read: its path, when it was written and the process that wrote it.
pub struct TokenSource {
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    /// From `<port>.pid`: the writer's process id and, on Linux, its start time.
    pub writer: Option<(u32, Option<u64>)>,
}

impl ClientToken {
    /// The app's `error`, and for a refused token read from a file, which file it was, when it
    /// was written and whether the instance that wrote it has exited. Never the token itself.
    pub fn explain_error(&self, error: &str) -> String {
        let Some(src) = &self.file else { return error.to_string() };
        if !error.starts_with("unauthorized") {
            return error.to_string();
        }
        let written = src
            .modified
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|t| (self.app.utc)(t.as_secs()))
            .unwrap_or_else(|| "at an unknown time".into());
        let age = src.modified.and_then(|t| t.elapsed().ok()).map(|d| format!(" ({} s ago)", d.as_secs())).unwrap_or_default();
        let path = src.path.display();
        let name = self.app.name;
        match src.writer.filter(|(pid, start)| writer_exited(*pid, *start)) {
            Some((pid, _)) => format!(
                "{error}; stale token file from an instance that exited (process {pid}): {path}, written {written}{age}; delete that file or restart {name}"
            ),
            None => format!("{error}; token from {path}, written {written}{age}; if {name} was force-quit, restart it or delete that file"),
        }
    }
}

/// Files bigger than this are not token files.
const MAX_TOKEN_FILE: u64 = 4096;

fn read_token_file(path: &Path, app: App) -> Option<ClientToken> {
    let f = std::fs::File::open(path).ok()?;
    let modified = f.metadata().ok().and_then(|m| m.modified().ok());
    let mut text = String::new();
    f.take(MAX_TOKEN_FILE + 1).read_to_string(&mut text).ok()?;
    if text.len() as u64 > MAX_TOKEN_FILE {
        return None;
    }
    let token = text.trim();
    if token.is_empty() {
        return None;
    }
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let writer = read_pid_file(&pid_path(&path));
    Some(ClientToken { token: token.to_string(), file: Some(TokenSource { path, modified, writer }), app })
}

/// The process file beside the token file `path`: `7878.token` → `7878.pid`.
pub fn pid_path(path: &Path) -> PathBuf {
    path.with_extension("pid")
}

/// `<pid> [<start time>]`; anything else (or no file) is no information.
fn read_pid_file(path: &Path) -> Option<(u32, Option<u64>)> {
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(256).read_to_string(&mut text).ok()?;
    let mut fields = text.split_whitespace();
    let pid = fields.next()?.parse::<u32>().ok().filter(|&pid| pid > 0)?;
    Some((pid, fields.next().and_then(|s| s.parse().ok())))
}

/// Whether the process that wrote a token file is known to be gone: not running, or its id now
/// belongs to a process that started at another time. Unknown counts as running.
fn writer_exited(pid: u32, start: Option<u64>) -> bool {
    process_running(pid) == Some(false) || start.zip(process_start(pid)).is_some_and(|(saved, now)| saved != now)
}

/// Whether process `pid` runs; `None` when that can't be found out.
fn process_running(pid: u32) -> Option<bool> {
    #[cfg(target_os = "linux")]
    {
        if !Path::new("/proc/self").exists() {
            return None;
        }
        match std::fs::metadata(format!("/proc/{pid}")) {
            Ok(_) => Some(true),
            Err(e) if e.kind() == ErrorKind::NotFound => Some(false),
            Err(_) => None,
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let output = std::process::Command::new("ps").args(["-p", &pid.to_string(), "-o", "pid="]).output().ok()?;
        if output.status.success() {
            return Some(true);
        }
        (output.status.code() == Some(1) && output.stdout.iter().all(u8::is_ascii_whitespace)).then_some(false)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.is_empty() {
            return None;
        }
        // `"image","pid",…` per process; with no match, a message line without a pid.
        let text = String::from_utf8_lossy(&output.stdout);
        Some(text.lines().any(|line| line.split("\",\"").nth(1).and_then(|s| s.trim_matches('"').parse::<u32>().ok()) == Some(pid)))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

/// Linux: the process's start time (clock ticks after boot), which tells a reused id apart.
fn process_start(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let mut stat = String::new();
        std::fs::File::open(format!("/proc/{pid}/stat")).ok()?.take(4096).read_to_string(&mut stat).ok()?;
        stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// `a == b`, taking the same time wherever they differ.
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// [`App::write_token_file`], with `restrict` limiting the new file to the user (Windows).
fn write_token_file_with(path: &Path, token: &str, restrict: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::new(ErrorKind::InvalidInput, "no folder"))?;
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        opts.mode(0o600);
    }
    let opened = opts.open(path)?;
    // On a refusal the file goes with the closure, so it is closed before it is removed.
    let mut f = match restrict(path).map(|()| opened) {
        Ok(f) => f,
        Err(e) => {
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
    };
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    let pid = std::process::id();
    let line = process_start(pid).map_or_else(|| format!("{pid}\n"), |start| format!("{pid} {start}\n"));
    let _ = std::fs::write(pid_path(path), line);
    Ok(())
}

/// Remove a token file this launch wrote, and its process file, while it still holds `token`.
/// Another launch may have written the same file since (a second window on the same port,
/// #8794); its file stays. The process id can't tell the two apart, as every Flatpak instance
/// has the same one, but each launch's token is its own.
pub fn remove_token_file(path: &Path, token: &str) {
    if read_token(path).is_some_and(|held| tokens_match(&held, token.trim())) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(pid_path(path));
    }
}

/// The token a token file holds, trimmed; `None` when there is no readable token file.
fn read_token(path: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(MAX_TOKEN_FILE + 1).read_to_string(&mut text).ok()?;
    let token = text.trim();
    (text.len() as u64 <= MAX_TOKEN_FILE && !token.is_empty()).then(|| token.to_string())
}

/// Whether the token file at `path` holds a token other than `token`: another launch's.
fn held_by_another(path: &Path, token: &str) -> bool {
    read_token(path).is_some_and(|held| !tokens_match(&held, token.trim()))
}

/// How long `icacls` may take (#6145). It finishes at once; on some computers the new process
/// is left suspended (security software holding it), and waiting on it without a limit froze
/// the app.
#[cfg(windows)]
const ICACLS_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// How a run of `icacls` ended.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IcaclsEnd {
    Succeeded,
    Failed,
    /// It didn't finish within [`ICACLS_LIMIT`] and was ended.
    TimedOut,
}

/// Whether a token may go into a file after `icacls` ended this way. `custom_folder`: the file is
/// in the folder the app's `CONTROL_DIR` names. A timeout is accepted only in the default folder,
/// the user's own local app data, which other users can't read by default; nothing is known about
/// the permissions of any other folder.
#[cfg(any(windows, test))]
fn restriction_result(end: IcaclsEnd, custom_folder: bool, path: &Path) -> Result<()> {
    match end {
        IcaclsEnd::Succeeded => Ok(()),
        IcaclsEnd::TimedOut if !custom_folder => Ok(()),
        IcaclsEnd::TimedOut => Err(Error::new(ErrorKind::TimedOut, format!("icacls did not finish in time to restrict {}", path.display()))),
        IcaclsEnd::Failed => Err(Error::other(format!("icacls failed to restrict {}", path.display()))),
    }
}

/// Windows: drop inherited entries and grant only the current user full access. An `icacls`
/// that doesn't finish in time is ended, and [`restriction_result`] decides what that means.
#[cfg(windows)]
fn restrict_to_user(path: &Path, custom_folder: bool) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let user = std::env::var("USERNAME").map_err(|_| Error::other("USERNAME is not set"))?;
    let domain = std::env::var("USERDOMAIN").ok().filter(|d| !d.is_empty());
    let who = domain.map_or(user.clone(), |d| format!("{d}\\{user}"));
    let child = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("{who}:F")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()?;
    let end = match wait_within(child, ICACLS_LIMIT)? {
        Some(status) if status.success() => IcaclsEnd::Succeeded,
        Some(_) => IcaclsEnd::Failed,
        None => IcaclsEnd::TimedOut,
    };
    restriction_result(end, custom_folder, path)
}

/// Wait for `child` at most `limit`: its exit status, or `None` (and the child ended) when it
/// hasn't finished by then.
#[cfg(any(windows, test))]
fn wait_within(mut child: std::process::Child, limit: std::time::Duration) -> Result<Option<std::process::ExitStatus>> {
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= limit {
            // Not waited for afterwards: a suspended process may not end at once.
            let _ = child.kill();
            return Ok(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Removes the token file (path, token) when the app ends, while it still holds that token.
pub struct TokenFile(pub PathBuf, pub String);

impl Drop for TokenFile {
    fn drop(&mut self) {
        remove_token_file(&self.0, &self.1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with no data folder and no environment, whose times read `t<seconds>`.
    const TEST: App = App { name: "TestCraft", data_dir: || None, env: |_| None, utc: |secs| format!("t{secs}"), default_port: 27878 };

    /// #6145: a child that doesn't finish in time is given up on, not waited for forever.
    #[cfg(unix)]
    #[test]
    fn a_slow_child_is_waited_for_only_so_long() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let slow = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        assert!(wait_within(slow, Duration::from_millis(200)).unwrap().is_none());
        assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
        let quick = std::process::Command::new("true").spawn().unwrap();
        assert!(wait_within(quick, Duration::from_secs(5)).unwrap().is_some_and(|s| s.success()));
        let failing = std::process::Command::new("false").spawn().unwrap();
        assert!(wait_within(failing, Duration::from_secs(5)).unwrap().is_some_and(|s| !s.success()));
    }

    /// #6145's freeze fix stays in the default folder, the user's own; a folder picked with
    /// the app's `CONTROL_DIR` gets no token unless the file was really restricted.
    #[test]
    fn only_the_default_folder_accepts_an_icacls_that_timed_out() {
        let path = Path::new("control/7878.token");
        for custom in [false, true] {
            assert!(restriction_result(IcaclsEnd::Succeeded, custom, path).is_ok());
            assert!(restriction_result(IcaclsEnd::Failed, custom, path).is_err());
        }
        assert!(restriction_result(IcaclsEnd::TimedOut, false, path).is_ok());
        let e = restriction_result(IcaclsEnd::TimedOut, true, path).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::TimedOut);
        assert!(e.to_string().contains("7878.token"), "{e}");
    }

    /// A file that couldn't be restricted is removed, empty and with no process file beside it, and
    /// the error goes to the caller. A restricted one gets its token.
    #[test]
    fn a_token_file_that_could_not_be_restricted_is_removed() {
        let dir = std::env::temp_dir().join(format!("solidcraft-control-restrict-{}", std::process::id()));
        let path = dir.join("7879.token");
        let e = write_token_file_with(&path, "secret-token", |p| {
            assert_eq!(std::fs::read(p).unwrap(), b"", "restricted before the token goes in");
            restriction_result(IcaclsEnd::TimedOut, true, p)
        })
        .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::TimedOut);
        assert!(!path.exists() && !pid_path(&path).exists());
        write_token_file_with(&path, "secret-token", |p| restriction_result(IcaclsEnd::TimedOut, false, p)).unwrap();
        assert_eq!(read_token(&path).as_deref(), Some("secret-token"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tokens_compare_whole() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc123", "abc12"));
        assert!(!tokens_match("", "a"));
    }

    /// The preference starts the listener by default, off it doesn't, and a port given on the
    /// command line or in the environment wins over it either way.
    #[test]
    fn the_preference_decides_unless_a_port_is_given() {
        assert_eq!(plan(None, true), Some(Listen::Default));
        assert_eq!(plan(None, false), None);
        assert_eq!(plan(Some(7878), false), Some(Listen::Exact(7878)));
        assert_eq!(plan(Some(7878), true), Some(Listen::Exact(7878)));
    }

    /// A taken port falls through to the next free one, on loopback only; a run of taken ports
    /// is an error, not a hang.
    #[test]
    fn a_taken_port_falls_back_to_the_next_free_one() {
        let first = bind_with_fallback(TEST.default_port, PORT_TRIES).unwrap();
        let a = first.local_addr().unwrap();
        assert!(a.ip().is_loopback() && (TEST.default_port..TEST.default_port + PORT_TRIES).contains(&a.port()), "{a}");
        let second = bind_with_fallback(a.port(), PORT_TRIES).unwrap();
        assert!(second.local_addr().unwrap().port() > a.port());
        assert!(bind_with_fallback(a.port(), 1).is_err());
    }

    #[test]
    fn the_discovery_file_holds_a_port() {
        assert_eq!(parse_port("27879\n"), Some(27879));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("70000"), None);
        assert_eq!(parse_port("nope"), None);
        let dir = std::env::temp_dir().join(format!("solidcraft-current-port-{}", std::process::id()));
        let path = dir.join("current.port");
        TEST.write_token_file(&path, "27881").unwrap();
        assert_eq!(parse_port(&std::fs::read_to_string(&path).unwrap()), Some(27881));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A one-line token file (and one with no process file) still reads.
    #[test]
    fn a_token_file_reads_with_and_without_its_process_file() {
        let dir = std::env::temp_dir().join(format!("solidcraft-control-read-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("7879.token");
        std::fs::write(&path, "legacy-token\n").unwrap();
        let client = read_token_file(&path, TEST).unwrap();
        assert_eq!(client.token, "legacy-token");
        let src = client.file.as_ref().unwrap();
        assert_eq!(src.path, std::fs::canonicalize(&path).unwrap());
        assert!(src.modified.is_some() && src.writer.is_none());
        std::fs::write(pid_path(&path), "not a pid\n").unwrap();
        assert!(read_token_file(&path, TEST).unwrap().file.unwrap().writer.is_none());
        std::fs::write(&path, "x".repeat(4097)).unwrap();
        assert!(read_token_file(&path, TEST).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused token's explanation names the app and writes the time as the app does.
    #[test]
    fn an_explanation_names_the_app() {
        let file = Some(TokenSource { path: PathBuf::from("7878.token"), modified: Some(UNIX_EPOCH), writer: None });
        let client = ClientToken { token: "secret-token".into(), file, app: TEST };
        let error = client.explain_error("unauthorized");
        assert!(error.contains("written t0 (") && error.contains("if TestCraft was force-quit"), "{error}");
        assert!(!error.contains("secret-token"), "{error}");
        let explicit = TEST.client_token(0, Some("explicit-token")).unwrap();
        assert!(explicit.token == "explicit-token" && explicit.file.is_none());
        assert_eq!(explicit.explain_error("unauthorized"), "unauthorized");
    }

    /// A process file naming a process that has exited marks the token file stale.
    #[cfg(any(unix, windows))]
    #[test]
    fn a_token_file_from_an_exited_process_is_stale() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "auth::tests::tokens_compare_whole"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        let source = |writer| Some(TokenSource { path: PathBuf::from("7878.token"), modified: None, writer });
        let stale = ClientToken { token: "secret-token".into(), file: source(Some((pid, None))), app: TEST };
        let error = stale.explain_error("unauthorized");
        assert!(error.contains(&format!("stale token file from an instance that exited (process {pid}): 7878.token")), "{error}");
        assert!(!error.contains("secret-token"));
        let live = ClientToken { token: "t".into(), file: source(Some((std::process::id(), process_start(std::process::id())))), app: TEST };
        assert!(!live.explain_error("unauthorized").contains("stale"));
        #[cfg(target_os = "linux")]
        {
            let reused = process_start(std::process::id()).map(|s| s + 1);
            let reused = ClientToken { token: "t".into(), file: source(Some((std::process::id(), reused))), app: TEST };
            assert!(reused.explain_error("unauthorized").contains("stale token file"));
        }
    }

    /// The token file is the user's alone, and a client reads it back.
    #[cfg(unix)]
    #[test]
    fn the_token_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("solidcraft-control-auth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("control/7878.token");
        std::fs::create_dir_all(dir.join("control")).unwrap();
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        TEST.write_token_file(&path, "0123456789abcdef0123456789abcdef").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "0123456789abcdef0123456789abcdef");
        let writer = read_token_file(&path, TEST).unwrap().file.unwrap().writer;
        assert_eq!(writer.map(|(pid, _)| pid), Some(std::process::id()));
        drop(TokenFile(path.clone(), "0123456789abcdef0123456789abcdef".into()));
        assert!(!path.exists() && !pid_path(&path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #8794: under Flatpak two windows can listen on the same port number (each has its own
    /// loopback) and write the same files. The one closing first leaves the other's token, process
    /// and discovery files; the last one takes them all away, `current.pid` too.
    #[test]
    fn a_closing_launch_leaves_another_launchs_files() {
        let dir = std::env::temp_dir().join(format!("solidcraft-control-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (file, current) = (dir.join("27878.token"), dir.join("current.port"));
        let (first, second) = ("a".repeat(64), "b".repeat(64));
        // The first window, then the second on the same port, each writing both files.
        for token in [&first, &second] {
            TEST.write_token_file(&file, token).unwrap();
            TEST.write_token_file(&current, "27878").unwrap();
        }
        // The first closes: the second's files stay.
        remove_token_file(&file, &first);
        remove_discovery(&current, &file, 27878, &first);
        assert_eq!(read_token(&file).as_deref(), Some(second.as_str()));
        assert!(pid_path(&file).exists() && current.exists() && pid_path(&current).exists());
        // Another port named in the discovery file is another app's too.
        remove_discovery(&current, &file, 27879, &second);
        assert!(current.exists());
        // The second closes: nothing of either is left.
        remove_token_file(&file, &format!(" {second}\n"));
        remove_discovery(&current, &file, 27878, &second);
        assert!(!file.exists() && !pid_path(&file).exists() && !current.exists() && !pid_path(&current).exists());
        // With no token file left (removed by hand, or never written), the discovery file of the
        // port still goes; a missing file is no error.
        TEST.write_token_file(&current, "27878").unwrap();
        remove_token_file(&file, &second);
        remove_discovery(&current, &file, 27878, &second);
        assert!(!current.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
