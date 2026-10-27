//! The control channel's per-launch token (docs/control-protocol.md). `solidcraft --control PORT`
//! makes a random token, writes it to a file only the user can read
//! ([`token_path`]) and refuses every connection whose first request doesn't carry it, so other
//! users on the same machine (loopback is shared, on Windows across remote-desktop sessions too)
//! can't drive the app. Clients find it with [`client_token`]. Next to it, `<port>.pid` names
//! the process that wrote it, so a client refused with a leftover token can say the file is stale.

use std::io::{Error, ErrorKind, Read, Result, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Shortest token accepted from `SOLIDCRAFT_CONTROL_TOKEN` (hex characters).
pub const MIN_TOKEN_LEN: usize = 32;

/// The port the control channel tries first when `--control` and `SOLIDCRAFT_CONTROL_PORT` give none.
/// Below the ephemeral ranges of Linux (32768+) and Windows/macOS (49152+), so the system never
/// hands it to an outgoing connection.
pub const DEFAULT_PORT: u16 = 27878;

/// Ports tried from [`DEFAULT_PORT`] upward when it is taken (a second window, another program).
pub const PORT_TRIES: u16 = 16;

/// Where the control channel listens, from the launch's `--control`/environment port and the
/// "Let AI agents connect" preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    /// Exactly this port (`--control PORT`, `SOLIDCRAFT_CONTROL_PORT`); no fallback.
    Exact(u16),
    /// [`DEFAULT_PORT`], or the next free one of [`PORT_TRIES`].
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

/// The folder of the token files: `<data folder>/control` (`SOLIDCRAFT_CONTROL_DIR` replaces it).
fn control_dir() -> Option<PathBuf> {
    match solidcraft_geom::env::var_os("CONTROL_DIR").filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(d)),
        None => Some(crate::recovery::data_dir()?.join("control")),
    }
}

/// The token file of the app listening on `port`: `<data folder>/control/<port>.token`
/// (`SOLIDCRAFT_CONTROL_DIR` replaces the folder).
pub fn token_path(port: u16) -> Option<PathBuf> {
    Some(control_dir()?.join(format!("{port}.token")))
}

/// The discovery file: the port of the most recently started app, so a client with no port
/// (`solidcraft-cli mcp`) finds it. It holds only the port; the token stays in its own file.
pub fn current_port_path() -> Option<PathBuf> {
    Some(control_dir()?.join("current.port"))
}

/// Record `port` as the running app's, readable only by the user like the token files.
pub fn write_current_port(port: u16) -> Result<PathBuf> {
    let path = current_port_path().ok_or_else(|| Error::new(ErrorKind::NotFound, "no data folder"))?;
    write_token_file(&path, &port.to_string())?;
    Ok(path)
}

/// The port the discovery file names, if there is one.
pub fn read_current_port() -> Option<u16> {
    parse_port(&std::fs::read_to_string(current_port_path()?).ok().filter(|t| t.len() <= 64)?)
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|p| *p != 0)
}

/// Remove the discovery file at exit, unless a later app has taken it over (it names another port).
pub fn remove_current_port(port: u16) {
    if read_current_port() == Some(port)
        && let Some(path) = current_port_path()
    {
        let _ = std::fs::remove_file(path);
    }
}

/// A token set on purpose with `SOLIDCRAFT_CONTROL_TOKEN` (test harnesses, sandboxed packages
/// whose data folder the client can't see), if it is long enough.
pub fn env_token() -> Option<String> {
    solidcraft_geom::env::var("CONTROL_TOKEN").map(|t| t.trim().to_string()).filter(|t| t.len() >= MIN_TOKEN_LEN)
}

/// The token a client sends to the app on `port`: `explicit` (`--token`), else
/// `SOLIDCRAFT_CONTROL_TOKEN`, else the app's token file.
pub fn client_token(port: u16, explicit: Option<&str>) -> Option<ClientToken> {
    if let Some(t) = explicit.map(str::trim).filter(|t| !t.is_empty()) {
        return Some(ClientToken { token: t.to_string(), file: None });
    }
    if let Some(t) = env_token() {
        return Some(ClientToken { token: t, file: None });
    }
    // Read directly, not through `vfs`: the app's own file, outside any agent's folders.
    read_token_file(&token_path(port)?)
}

/// A client token and, when it came from a token file, where that file is.
pub struct ClientToken {
    pub token: String,
    pub file: Option<TokenSource>,
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
            .map(|t| crate::crash::utc(t.as_secs()))
            .unwrap_or_else(|| "at an unknown time".into());
        let age = src.modified.and_then(|t| t.elapsed().ok()).map(|d| format!(" ({} s ago)", d.as_secs())).unwrap_or_default();
        let path = src.path.display();
        match src.writer.filter(|(pid, start)| writer_exited(*pid, *start)) {
            Some((pid, _)) => format!(
                "{error}; stale token file from an instance that exited (process {pid}): {path}, written {written}{age}; delete that file or restart SolidCraft"
            ),
            None => format!("{error}; token from {path}, written {written}{age}; if SolidCraft was force-quit, restart it or delete that file"),
        }
    }
}

/// Files bigger than this are not token files.
const MAX_TOKEN_FILE: u64 = 4096;

fn read_token_file(path: &Path) -> Option<ClientToken> {
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
    Some(ClientToken { token: token.to_string(), file: Some(TokenSource { path, modified, writer }) })
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

/// Write `token` to `path`, readable and writable only by the user: the folder is made `0700`
/// and the file `0600` on Unix; on Windows the file's inherited permissions are replaced by one
/// entry for the user (`icacls`) before the token goes in. A file left by an earlier launch is
/// replaced. The process file beside it ([`pid_path`]) records this process, best effort.
pub fn write_token_file(path: &Path, token: &str) -> Result<()> {
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
    let mut f = opts.open(path)?;
    #[cfg(windows)]
    restrict_to_user(path)?;
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    let pid = std::process::id();
    let line = process_start(pid).map_or_else(|| format!("{pid}\n"), |start| format!("{pid} {start}\n"));
    let _ = std::fs::write(pid_path(path), line);
    Ok(())
}

/// Remove a token file this launch wrote, and its process file.
pub fn remove_token_file(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(pid_path(path));
}

/// How long `icacls` may take (#6145). It finishes at once; on some computers the new process
/// is left suspended (security software holding it), and waiting on it without a limit froze
/// the app.
#[cfg(windows)]
const ICACLS_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Windows: drop inherited entries and grant only the current user full access. An `icacls`
/// that doesn't finish in time is ended and the file kept as made: it is in the user's own
/// local app data folder, which other users can't read by default.
#[cfg(windows)]
fn restrict_to_user(path: &Path) -> Result<()> {
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
    match wait_within(child, ICACLS_LIMIT)? {
        Some(status) if !status.success() => Err(Error::other(format!("icacls failed to restrict {}", path.display()))),
        _ => Ok(()),
    }
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

/// Removes the token file when the app ends.
pub struct TokenFile(pub PathBuf);

impl Drop for TokenFile {
    fn drop(&mut self) {
        remove_token_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let first = bind_with_fallback(DEFAULT_PORT, PORT_TRIES).unwrap();
        let a = first.local_addr().unwrap();
        assert!(a.ip().is_loopback() && (DEFAULT_PORT..DEFAULT_PORT + PORT_TRIES).contains(&a.port()), "{a}");
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
        write_token_file(&path, "27881").unwrap();
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
        let client = read_token_file(&path).unwrap();
        assert_eq!(client.token, "legacy-token");
        let src = client.file.as_ref().unwrap();
        assert_eq!(src.path, std::fs::canonicalize(&path).unwrap());
        assert!(src.modified.is_some() && src.writer.is_none());
        std::fs::write(pid_path(&path), "not a pid\n").unwrap();
        assert!(read_token_file(&path).unwrap().file.unwrap().writer.is_none());
        std::fs::write(&path, "x".repeat(4097)).unwrap();
        assert!(read_token_file(&path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refused token names its file and when it was written, never the token.
    #[test]
    fn unauthorized_error_names_the_token_file() {
        let path = PathBuf::from("control/7878.token");
        let modified = Some(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000));
        let client = ClientToken { token: "secret-token".into(), file: Some(TokenSource { path: path.clone(), modified, writer: None }) };
        let error = client.explain_error("unauthorized: send the control token");
        assert!(error.contains(&format!("token from {}, written 2023-11-14T22:13:20Z (", path.display())), "{error}");
        assert!(error.contains("restart it or delete that file") && !error.contains("secret-token"), "{error}");
        assert_eq!(client.explain_error("unknown method"), "unknown method");
        let explicit = client_token(0, Some("explicit-token")).unwrap();
        assert!(explicit.token == "explicit-token" && explicit.file.is_none());
        assert_eq!(explicit.explain_error("unauthorized"), "unauthorized");
    }

    /// A known exit marks a token stale. An unobservable process stays unknown, as
    /// it does in a sandbox which cannot run the platform's process inspector.
    #[cfg(any(unix, windows))]
    #[test]
    fn a_token_file_from_an_exited_process_is_stale_when_its_exit_is_observable() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "control_auth::tests::tokens_compare_whole"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        assert!(child.wait().unwrap().success());
        let source = |writer| Some(TokenSource { path: PathBuf::from("7878.token"), modified: None, writer });
        let stale = ClientToken { token: "secret-token".into(), file: source(Some((pid, None))) };
        let running = process_running(pid);
        let error = stale.explain_error("unauthorized");
        match running {
            Some(false) => assert!(error.contains(&format!("stale token file from an instance that exited (process {pid}): 7878.token")), "{error}"),
            None => {
                assert!(!error.contains("stale"), "unknown liveness must not claim an exit: {error}");
                assert!(error.contains("token from 7878.token, written at an unknown time"), "{error}");
                assert!(error.contains("restart it or delete that file"), "{error}");
            }
            Some(true) => panic!("the process inspector reports the reaped child {pid} as running"),
        }
        assert!(!error.contains("secret-token"));
        let live = ClientToken { token: "t".into(), file: source(Some((std::process::id(), process_start(std::process::id())))) };
        assert!(!live.explain_error("unauthorized").contains("stale"));
        #[cfg(target_os = "linux")]
        {
            let reused = process_start(std::process::id()).map(|s| s + 1);
            let reused = ClientToken { token: "t".into(), file: source(Some((std::process::id(), reused))) };
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
        write_token_file(&path, "0123456789abcdef0123456789abcdef").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "0123456789abcdef0123456789abcdef");
        let writer = read_token_file(&path).unwrap().file.unwrap().writer;
        assert_eq!(writer.map(|(pid, _)| pid), Some(std::process::id()));
        drop(TokenFile(path.clone()));
        assert!(!path.exists() && !pid_path(&path).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
