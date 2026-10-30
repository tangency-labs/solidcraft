//! File access for the engine. Natively it is the disk. In the browser (wasm32) there is no
//! file system to call synchronously, so files live in memory here: the web host loads them
//! (from the browser's private storage, a file picker or a drop) before a command reads them,
//! and takes what commands wrote ([`take_writes`]) to store it or hand it out as a download.

use std::io::{Error, ErrorKind, Result};

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::*;

    /// At most `max` bytes of a regular file. A character device such as `/dev/zero` reports a
    /// length of 0, so checking the metadata and then reading to the end is unbounded; a FIFO
    /// blocks the caller instead. Refuse anything that is not a regular file, and never read
    /// more than the caller's cap plus the byte that proves it was exceeded.
    pub fn read(path: &str, max: usize) -> Result<Vec<u8>> {
        read_with_progress(path, max, |done, total| solidcraft_doc::progress::work_step("reading", done, total))
    }

    /// Read a regular file, reporting bytes read and file size. Returning true stops the read.
    pub(super) fn read_with_progress(path: &str, max: usize, mut stop: impl FnMut(usize, usize) -> bool) -> Result<Vec<u8>> {
        use std::io::Read;

        check(path)?;
        touched();
        // Cheap and non-blocking: refuse before opening, in case the open itself would block.
        let meta = std::fs::metadata(path)?;
        if !meta.is_file() {
            return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"));
        }
        if meta.len() > max as u64 {
            return Err(Error::new(ErrorKind::InvalidData, "file too large"));
        }
        let f = std::fs::File::open(path)?;
        // The path may have been swapped between the two calls; the handle decides.
        let meta = f.metadata()?;
        if !meta.is_file() {
            return Err(Error::new(ErrorKind::InvalidInput, "not a regular file"));
        }
        if meta.len() > max as u64 {
            return Err(Error::new(ErrorKind::InvalidData, "file too large"));
        }
        let total = meta.len() as usize;
        if stop(0, total) {
            return Err(Error::new(ErrorKind::Interrupted, "file read cancelled"));
        }
        let mut out = Vec::with_capacity(meta.len().min(16 << 20) as usize);
        let mut reader = f.take((max as u64).saturating_add(1));
        loop {
            let n = reader.by_ref().take(256 << 10).read_to_end(&mut out)?;
            if out.len() > max {
                return Err(Error::new(ErrorKind::InvalidData, "file too large"));
            }
            if n == 0 {
                break;
            }
            if stop(out.len(), total) {
                return Err(Error::new(ErrorKind::Interrupted, "file read cancelled"));
            }
        }
        Ok(out)
    }
    pub fn write(path: &str, bytes: &[u8]) -> Result<()> {
        check(path)?;
        touched();
        std::fs::write(path, bytes)
    }
    pub fn len(path: &str) -> Result<u64> {
        check(path)?;
        touched();
        std::fs::metadata(path).map(|m| m.len())
    }
    pub fn exists(path: &str) -> bool {
        check(path).is_ok() && {
            touched();
            std::path::Path::new(path).exists()
        }
    }
    /// Whether the path names a regular file the caller may access.
    pub fn is_file(path: &str) -> bool {
        check(path).is_ok() && {
            touched();
            std::path::Path::new(path).is_file()
        }
    }
    pub fn remove(path: &str) -> Result<()> {
        check(path)?;
        touched();
        std::fs::remove_file(path)
    }
    /// A folder and the folders above it, where missing.
    pub fn create_dir_all(path: &str) -> Result<()> {
        check(path)?;
        touched();
        std::fs::create_dir_all(path)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    thread_local! {
        static FILES: RefCell<BTreeMap<String, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
        static WRITES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn missing(path: &str) -> Error {
        Error::new(ErrorKind::NotFound, format!("{path}: no such file in this browser"))
    }

    /// At most `max` bytes, to match the native signature.
    pub fn read(path: &str, max: usize) -> Result<Vec<u8>> {
        FILES.with(|f| {
            let files = f.borrow();
            let bytes = files.get(path).ok_or_else(|| missing(path))?;
            let total = bytes.len();
            if total > max {
                return Err(Error::new(ErrorKind::InvalidData, "file too large"));
            }
            if solidcraft_doc::progress::work_step("reading", 0, total) {
                return Err(Error::new(ErrorKind::Interrupted, "file read cancelled"));
            }
            let out = bytes.clone();
            if solidcraft_doc::progress::work_step("reading", total, total) {
                return Err(Error::new(ErrorKind::Interrupted, "file read cancelled"));
            }
            Ok(out)
        })
    }
    pub fn write(path: &str, bytes: &[u8]) -> Result<()> {
        FILES.with(|f| f.borrow_mut().insert(path.to_string(), bytes.to_vec()));
        WRITES.with(|w| {
            let mut w = w.borrow_mut();
            w.retain(|p| p != path);
            w.push(path.to_string());
        });
        Ok(())
    }
    pub fn len(path: &str) -> Result<u64> {
        FILES.with(|f| f.borrow().get(path).map(|b| b.len() as u64)).ok_or_else(|| missing(path))
    }
    pub fn exists(path: &str) -> bool {
        FILES.with(|f| f.borrow().contains_key(path))
    }
    /// Every entry in browser storage is a regular file.
    pub fn is_file(path: &str) -> bool {
        exists(path)
    }
    pub fn remove(path: &str) -> Result<()> {
        FILES.with(|f| f.borrow_mut().remove(path)).map(|_| ()).ok_or_else(|| missing(path))
    }
    /// Files are keyed by their whole path here: a folder needs no making.
    pub fn create_dir_all(_path: &str) -> Result<()> {
        Ok(())
    }
    pub fn insert(path: &str, bytes: Vec<u8>) {
        FILES.with(|f| f.borrow_mut().insert(path.to_string(), bytes));
    }
    pub fn take_writes() -> Vec<(String, Vec<u8>)> {
        let paths = WRITES.with(|w| std::mem::take(&mut *w.borrow_mut()));
        paths.into_iter().filter_map(|p| FILES.with(|f| f.borrow().get(&p).cloned()).map(|b| (p, b))).collect()
    }
    pub fn list(prefix: &str) -> Vec<(String, u64)> {
        FILES.with(|f| f.borrow().iter().filter(|(p, _)| p.starts_with(prefix)).map(|(p, b)| (p.clone(), b.len() as u64)).collect())
    }
}

pub use imp::*;

/// The folders an agent's commands may read, write and delete in (an MCP server's, see
/// docs/mcp.md). While a scope is active on this thread ([`scoped`]), every file call here
/// refuses paths outside it; without one (the user's own file dialogs, the CLI) nothing changes.
///
/// Two kinds of folder: those named on purpose (`--allow-path`, the app's own recovery folder),
/// where everything is reachable, and design folders (the open design's, and those an agent
/// opened or saved a design in, #7181), which count only when [`design_folder_refusal`] has
/// nothing against them and never reach hidden files or folders (`.env`, `.git`) inside them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathScope {
    roots: Vec<std::path::PathBuf>,
    design_roots: Vec<std::path::PathBuf>,
}

/// At most this many design folders in a scope; the oldest gives way.
pub const MAX_DESIGN_FOLDERS: usize = 16;

thread_local! {
    static SCOPE: std::cell::RefCell<Option<PathScope>> = const { std::cell::RefCell::new(None) };
}

/// `dir` resolved, if it exists.
fn canonical(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(dir).ok()
}

/// The user's home folder, resolved.
fn home_dir() -> Option<std::path::PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|h| !h.is_empty()).and_then(|h| canonical(std::path::Path::new(&h)))
}

/// A folder or file name an agent never reaches by default: hidden on Unix (`.ssh`, `.env`),
/// and on Windows also the per-user `AppData` folder.
fn hidden(name: &std::ffi::OsStr) -> bool {
    let n = name.to_string_lossy();
    n.starts_with('.') || (cfg!(windows) && n.eq_ignore_ascii_case("AppData"))
}

/// Why `dir` (an open design's folder) can't be one of an agent's folders by default, or `None`
/// when it can: it must lie strictly inside the home folder (or the temporary folder) and not
/// in a hidden folder. So a design saved in home itself, in `/`, on a drive root or in a system
/// folder never puts `~/.ssh`, `~/.bashrc` or anything else of the user's in reach.
pub fn design_folder_refusal(dir: &std::path::Path) -> Option<String> {
    design_folder_refusal_in(dir, home_dir().as_deref(), Some(&temp_dir()))
}

/// [`design_folder_refusal`] with the home and temporary folders given (tests).
pub fn design_folder_refusal_in(dir: &std::path::Path, home: Option<&std::path::Path>, temp: Option<&std::path::Path>) -> Option<String> {
    let Some(d) = canonical(dir) else { return Some("it doesn't exist".into()) };
    if d.parent().is_none() {
        return Some("it is a drive or file-system root".into());
    }
    let home = home.and_then(canonical);
    if home.as_ref().is_some_and(|h| h.starts_with(&d)) {
        return Some("it is your home folder or a folder above it".into());
    }
    let temp = temp.and_then(canonical).filter(|t| t.parent().is_some());
    // The closer of the two (a temporary folder may lie inside home, in a hidden folder).
    let Some(base) = [home, temp].into_iter().flatten().filter(|b| d.starts_with(b) && d != *b).max_by_key(|b| b.components().count()) else {
        return Some("it is not inside your home folder".into());
    };
    let rel = d.strip_prefix(&base).ok()?;
    if rel.components().any(|c| hidden(c.as_os_str())) {
        return Some("it is inside a hidden folder".into());
    }
    None
}

impl PathScope {
    /// Confine file access to `roots` (resolved now; folders that don't exist are left out, so an
    /// empty scope refuses every path).
    pub fn new(roots: impl IntoIterator<Item = std::path::PathBuf>) -> PathScope {
        let mut out = PathScope::default();
        for r in roots.into_iter().filter_map(|r| canonical(&r)) {
            if !out.roots.contains(&r) {
                out.roots.push(r);
            }
        }
        out
    }

    /// Also the folder of the design at `path` (the open design), when
    /// [`design_folder_refusal`] allows it; hidden entries inside it stay out of reach. It is the
    /// folder of the file as [`PathScope::check`] resolves it: a link to a design elsewhere puts
    /// the folder it leads to in reach, never the folder the link lies in.
    pub fn with_design(self, path: Option<&str>) -> PathScope {
        let file = path.and_then(|p| resolve(std::path::Path::new(p)).ok());
        match file.as_deref().and_then(std::path::Path::parent) {
            Some(d) => self.with_design_root(d),
            None => self,
        }
    }

    /// Also the folder of `path`, a design file an agent opened or saved (#7181), on the same
    /// terms as the open design's. On Windows its text is judged first, as [`PathScope::check`]
    /// judges a path: resolving a UNC path would already make Windows connect out.
    pub fn with_design_file(self, path: &str) -> PathScope {
        if cfg!(windows) && windows_refusal(path).is_some() {
            return self;
        }
        self.with_design(Some(path))
    }

    /// Also `dir`, a folder an agent opened or saved a design in (a request's `design_folders`),
    /// on the same terms as the open design's folder, its text judged first on Windows.
    pub fn with_design_folder(self, dir: &std::path::Path) -> PathScope {
        if cfg!(windows) && windows_refusal(&dir.to_string_lossy()).is_some() {
            return self;
        }
        self.with_design_root(dir)
    }

    fn with_design_root(mut self, dir: &std::path::Path) -> PathScope {
        if design_folder_refusal(dir).is_none()
            && let Some(d) = canonical(dir)
            && !self.design_roots.contains(&d)
        {
            if self.design_roots.len() >= MAX_DESIGN_FOLDERS {
                self.design_roots.remove(0);
            }
            self.design_roots.push(d);
        }
        self
    }

    /// The folders named on purpose.
    pub fn roots(&self) -> &[std::path::PathBuf] {
        &self.roots
    }

    /// The design folders that count.
    pub fn design_roots(&self) -> &[std::path::PathBuf] {
        &self.design_roots
    }

    /// Whether `path` lies inside one of the folders, after following symlinks and `..`.
    pub fn check(&self, path: &str) -> Result<()> {
        let refused = || {
            let roots: Vec<String> = self.roots.iter().chain(&self.design_roots).map(|r| r.display().to_string()).collect();
            Error::new(
                ErrorKind::PermissionDenied,
                format!(
                    "{path}: outside the folders this agent may use ({}); allow another folder with `--allow-path DIR`",
                    if roots.is_empty() { "none".into() } else { roots.join(", ") }
                ),
            )
        };
        let windows = |why: &str| Error::new(ErrorKind::PermissionDenied, format!("{path}: not allowed for this agent: {why}"));
        // Judged from the text before any filesystem call: resolving a UNC path would already
        // make Windows connect out, and a device name opens the device.
        if cfg!(windows)
            && let Some(why) = windows_refusal(path)
        {
            return Err(windows(why));
        }
        touched();
        let resolved = match resolve(std::path::Path::new(path)) {
            Ok(r) => r,
            Err(e) if e.kind() == ErrorKind::NotFound => return Err(self.missing_folder(path).unwrap_or_else(refused)),
            Err(_) => return Err(refused()),
        };
        // Judged again once resolved: a link or junction inside an allowed folder may lead to a
        // network share or a device even though the path given looked local.
        if cfg!(windows) && !local_drive_path(&resolved) {
            return Err(windows("only paths on a local drive"));
        }
        if self.contains(&resolved) { Ok(()) } else { Err(refused()) }
    }

    /// Whether the resolved path `p` lies inside one of the folders.
    fn contains(&self, p: &std::path::Path) -> bool {
        let inside_design = || self.design_roots.iter().any(|r| p.strip_prefix(r).is_ok_and(|rel| !rel.components().any(|c| hidden(c.as_os_str()))));
        self.roots.iter().any(|r| p.starts_with(r)) || inside_design()
    }

    /// The "folder does not exist" error for a path whose folder is missing, but only when the
    /// nearest folder that does exist is one of the scope's own: telling the user which folder is
    /// missing must not become a way to probe the disk outside it.
    fn missing_folder(&self, path: &str) -> Option<Error> {
        let given = std::path::Path::new(path);
        // A dangling link (the path itself, or in its folders) is not a missing folder: it leads
        // somewhere, and that is refused.
        let mut missing = given.parent()?;
        let existing = loop {
            let here = if missing.as_os_str().is_empty() { std::path::Path::new(".") } else { missing };
            if let Ok(c) = std::fs::canonicalize(here) {
                break c;
            }
            if std::fs::symlink_metadata(here).is_ok() {
                return None;
            }
            missing = missing.parent()?;
        };
        if std::fs::symlink_metadata(given).is_ok() {
            return None;
        }
        self.contains(&existing).then(|| {
            Error::new(
                ErrorKind::NotFound,
                format!("{path}: its folder does not exist (nothing exists below {}); create the folder first", existing.display()),
            )
        })
    }
}

/// Whether a resolved path is on a local drive (`C:\…` or `\\?\C:\…`) with no `:` or device
/// name in it. Always true off Windows, where these rules don't apply.
fn local_drive_path(p: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        let drive = matches!(
            p.components().next(),
            Some(std::path::Component::Prefix(x)) if matches!(x.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_))
        );
        drive && windows_refusal(&p.to_string_lossy()).is_none()
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        true
    }
}

/// Why Windows would not treat `path` as a plain file on a local drive, judged by Windows rules
/// from the text alone (so it is tested on every platform): UNC, device (`\\.\`), NT (`\??\`) and
/// verbatim paths other than a verbatim drive (`\\?\C:\`), drive-relative paths (`C:file`), a `:`
/// inside a name (alternate data streams), reserved device names (`NUL`, `COM1.txt`, …) and
/// short (8.3) names.
pub fn windows_refusal(path: &str) -> Option<&'static str> {
    let norm = path.replace('/', "\\");
    let drive = |s: &str| matches!(s.as_bytes(), [d, b':', ..] if d.is_ascii_alphabetic());
    let drive_abs = |s: &str| matches!(s.as_bytes(), [d, b':', b'\\', ..] if d.is_ascii_alphabetic());
    let body = if let Some(v) = norm.strip_prefix("\\\\?\\") {
        if !drive_abs(v) {
            return Some("only paths on a local drive (no UNC, device or volume paths)");
        }
        v.get(2..).unwrap_or("")
    } else if norm.starts_with("\\\\") || norm.starts_with("\\??\\") {
        return Some("network (UNC), device and NT namespace paths");
    } else if drive(&norm) {
        if !drive_abs(&norm) {
            return Some("drive-relative paths (`C:file`)");
        }
        norm.get(2..).unwrap_or("")
    } else {
        &norm
    };
    for name in body.split('\\').filter(|n| !n.is_empty()) {
        if name.contains(':') {
            return Some("`:` in a file name (alternate data streams)");
        }
        // A short (8.3) name such as `PROGRA~1` or `SSH~1` is another spelling of a folder whose
        // real name could be anything; canonicalising expands existing ones, and a new file's
        // name must not be one.
        if short_name(name) {
            return Some("short (8.3) names such as `PROGRA~1`");
        }
        if device_name(name) {
            return Some("reserved device names (CON, NUL, COM1, …)");
        }
    }
    None
}

/// Whether a file or folder name is one Windows keeps for a device (`NUL`, `com1.txt`, `Con .x`).
fn device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches([' ', '.']).to_uppercase();
    let numbered =
        |p: &str| stem.strip_prefix(p).is_some_and(|n| matches!(n, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

/// Where a path would lead other than to a file on this computer, judged from its text by Windows
/// rules on every platform (a design made on one system is opened on another).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Remote {
    /// A file on another computer (`\\host\share\…`, `\\?\UNC\host\…`): the host as written.
    Network(String),
    /// A device or NT namespace path (`\\.\pipe\x`, `\\?\Volume{…}`, `\??\…`) or a reserved
    /// device name (`NUL`, `COM1.txt`).
    Device,
}

/// Where `path` leads when it is not to a file on this computer (`None`: a local path, absolute
/// or relative). Text only: nothing on disk or the network is asked.
pub fn remote(path: &str) -> Option<Remote> {
    let norm = path.trim().replace('/', "\\");
    let host = |rest: &str| -> Remote {
        let h = rest.split('\\').next().unwrap_or("").trim();
        if h.is_empty() || h == "." || h == "?" { Remote::Device } else { Remote::Network(h.to_string()) }
    };
    let starts = |p: &str| norm.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p));
    let body = if starts("\\\\?\\UNC\\") || starts("\\??\\UNC\\") {
        return Some(host(norm.get(8..).unwrap_or("")));
    } else if let Some(v) = norm.strip_prefix("\\\\?\\") {
        if !matches!(v.as_bytes(), [d, b':', b'\\', ..] if d.is_ascii_alphabetic()) {
            return Some(Remote::Device);
        }
        v
    } else if norm.starts_with("\\\\.\\") || norm.starts_with("\\??\\") || norm == "\\\\." || norm == "\\\\?" {
        return Some(Remote::Device);
    } else if let Some(rest) = norm.strip_prefix("\\\\") {
        return Some(host(rest));
    } else {
        norm.as_str()
    };
    body.split('\\').filter(|n| !n.is_empty()).any(device_name).then_some(Remote::Device)
}

/// Which network hosts the paths a design names may lead to. Empty by default: a design that
/// names `\\host\share\part.solidcraft` reaches nothing on the network until the user allows that
/// host, since on Windows merely asking whether the file exists connects to the host and sends
/// it the user's sign-in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetworkPolicy {
    /// Every host (a managed organisation that keeps its designs on file servers).
    pub allow_all: bool,
    /// Hosts allowed by name (case does not matter; a leading `\\` is ignored).
    pub hosts: Vec<String>,
}

impl NetworkPolicy {
    const NONE: NetworkPolicy = NetworkPolicy { allow_all: false, hosts: Vec::new() };

    /// Whether `host` is allowed.
    pub fn allows(&self, host: &str) -> bool {
        let host = host_key(host);
        self.allow_all || (!host.is_empty() && self.hosts.iter().any(|h| host_key(h) == host))
    }
}

/// A host name as compared: without `\\` around it, lower case.
fn host_key(host: &str) -> String {
    host.trim().trim_matches('\\').trim_matches('/').to_ascii_lowercase()
}

static NETWORK: std::sync::RwLock<NetworkPolicy> = std::sync::RwLock::new(NetworkPolicy::NONE);

thread_local! {
    static ONCE_HOSTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static FILE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The hosts allowed for every design (the app's preferences).
pub fn set_network_policy(policy: NetworkPolicy) {
    if let Ok(mut p) = NETWORK.write()
        && *p != policy
    {
        *p = policy;
    }
}

/// The hosts allowed for every design.
pub fn network_policy() -> NetworkPolicy {
    NETWORK.read().map(|p| p.clone()).unwrap_or_default()
}

/// Run `f` with `hosts` also allowed on this thread: the user allowed them for this one action
/// ("Allow once"), or picked the file on them in a file dialog.
pub fn allowing_hosts<T>(hosts: &[String], f: impl FnOnce() -> T) -> T {
    struct Restore(usize);
    impl Drop for Restore {
        fn drop(&mut self) {
            let n = self.0;
            ONCE_HOSTS.with(|h| h.borrow_mut().truncate(n));
        }
    }
    let _restore = Restore(ONCE_HOSTS.with(|h| {
        let mut h = h.borrow_mut();
        let n = h.len();
        h.extend(hosts.iter().cloned());
        n
    }));
    f()
}

/// The network host of a path, when it names one (`\\host\share\x` → `host`).
pub fn network_host(path: &str) -> Option<String> {
    match remote(path) {
        Some(Remote::Network(h)) => Some(h),
        _ => None,
    }
}

/// Why a path a design names may not be reached, or `None` when it may: a device path never; a
/// network path when its host is allowed (the preferences, [`allowing_hosts`], or the host the
/// design itself was opened from, `design`, which the user already connected to).
pub fn design_path_refusal(path: &str, design: Option<&str>) -> Option<Remote> {
    let r = remote(path)?;
    let Remote::Network(host) = &r else { return Some(r) };
    let own = design.and_then(network_host).is_some_and(|d| host_key(&d) == host_key(host));
    let once = || ONCE_HOSTS.with(|h| h.borrow().iter().any(|x| host_key(x) == host_key(host)));
    if own || once() || network_policy().allows(host) { None } else { Some(r) }
}

/// The error for a refused design path.
fn design_path_error(path: &str, why: &Remote) -> Error {
    let text = match why {
        Remote::Network(host) => format!(
            "{path}: on the network location \\\\{host}, which designs may not reach until you allow that server \
             (Update Links or Open Linked Design asks; or Preferences › Design › Network locations)"
        ),
        Remote::Device => format!("{path}: names a device, not a file"),
    };
    Error::new(ErrorKind::PermissionDenied, text)
}

/// Check a path a design names (a linked design, a font file) before anything reads it, judged
/// from its text first: a network path whose host isn't allowed, or a device path, is refused
/// without a filesystem call. An agent's scope ([`check`]) applies on top.
pub fn check_design_path(path: &str, design: Option<&str>) -> Result<()> {
    if let Some(why) = design_path_refusal(path, design) {
        return Err(design_path_error(path, &why));
    }
    check(path)
}

/// [`read`] of a path a design names ([`check_design_path`] first).
pub fn read_design_path(path: &str, design: Option<&str>, max: usize) -> Result<Vec<u8>> {
    check_design_path(path, design)?;
    read(path, max)
}

/// Whether a path a design names is a file; `None` (unknown) when [`check_design_path`] refuses
/// it, which asks nothing of the disk or the network.
pub fn design_path_is_file(path: &str, design: Option<&str>) -> Option<bool> {
    design_path_refusal(path, design).is_none().then(|| is_file(path))
}

/// The checks the layers below ask before reading a file a design names (sketch text's font
/// files). Done once; every session and agent scope does it.
pub fn install_checks() {
    solidcraft_doc::sketch::set_font_file_check(|p| check_design_path(&p.to_string_lossy(), None).map_err(|e| e.to_string()));
}

/// File system calls made on this thread through this module (tests: a refused path made none).
#[doc(hidden)]
pub fn file_calls() -> usize {
    FILE_CALLS.with(|c| c.get())
}

/// Count one file system call (native).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn touched() {
    FILE_CALLS.with(|c| c.set(c.get().saturating_add(1)));
}

/// Whether a file or folder name looks like a short (8.3) name: a `~` and a digit (`PROGRA~1`).
fn short_name(name: &str) -> bool {
    name.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit())
}

/// `path` spelled with long names, for the folders SolidCraft derives from the environment
/// itself (the temporary folder, the recovery folder). On Windows those may hold a short (8.3)
/// name: the temporary folder is often `…\NAME~1.XYZ\AppData\Local\Temp`, with the user folder's
/// short name, when that folder's name is long, and [`windows_refusal`] refuses short names in
/// every path an agent's commands reach, the server's own folders included. A path with a short
/// name is resolved as far as it exists (the rest is appended as given) and loses the `\\?\`
/// prefix resolving adds. It stays as it is when it has no short name, can't be resolved or
/// doesn't resolve to a local drive, and off Windows.
pub fn long_names(path: &std::path::Path) -> std::path::PathBuf {
    if !cfg!(windows) || !path.iter().any(|n| short_name(&n.to_string_lossy())) {
        return path.to_path_buf();
    }
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(full) = std::fs::canonicalize(at) {
            let local = full
                .to_str()
                .and_then(|f| f.strip_prefix(r"\\?\"))
                .filter(|f| matches!(f.as_bytes(), [d, b':', b'\\', ..] if d.is_ascii_alphabetic()));
            return local.map_or_else(|| path.to_path_buf(), |f| rest.iter().rev().fold(std::path::PathBuf::from(f), |p, n| p.join(n)));
        }
        let (Some(parent), Some(name)) = (at.parent(), at.file_name()) else { return path.to_path_buf() };
        rest.push(name);
        at = parent;
    }
}

/// The temporary folder in the browser. std has none there (`std::env::temp_dir` panics on
/// wasm32-unknown-unknown), so it is a folder of the in-memory files: what is written in it stays
/// in memory, neither stored nor downloaded (#7192).
pub const BROWSER_TEMP: &str = "/tmp";

/// The temporary folder spelled with long names ([`long_names`]), for the folders SolidCraft
/// makes in it; [`BROWSER_TEMP`] in the browser.
pub fn temp_dir() -> std::path::PathBuf {
    if cfg!(target_arch = "wasm32") {
        return std::path::PathBuf::from(BROWSER_TEMP);
    }
    long_names(&std::env::temp_dir())
}

/// `path` with symlinks and `..` resolved. A file that doesn't exist yet resolves through its
/// folder, which must exist; a symlink whose target is missing is refused (writing through it
/// would land wherever it points).
fn resolve(path: &std::path::Path) -> Result<std::path::PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == ErrorKind::NotFound && std::fs::symlink_metadata(path).is_err() => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return Err(e) };
            let parent = if parent.as_os_str().is_empty() { std::path::Path::new(".") } else { parent };
            Ok(std::fs::canonicalize(parent)?.join(name))
        }
        Err(e) => Err(e),
    }
}

/// Run `f` with file access confined to `scope` on this thread (`None`: unconfined).
pub fn scoped<T>(scope: Option<PathScope>, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathScope>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            SCOPE.with(|s| *s.borrow_mut() = prev);
        }
    }
    // Sketch text reads the font files a design names, below this crate: it asks here first.
    install_checks();
    let _restore = Restore(SCOPE.with(|s| s.replace(scope)));
    f()
}

/// The scope active on this thread.
pub fn current_scope() -> Option<PathScope> {
    SCOPE.with(|s| s.borrow().clone())
}

/// Refuse `path` if a scope is active and it lies outside. Code that reaches the disk without
/// going through this module calls it first. In the browser files are the host's, never the
/// disk, so there is nothing to confine.
// ponytail: checked then opened, so a local process racing a symlink swap in an allowed folder
// could still redirect one call; opening by handle (openat/O_NOFOLLOW) would close that.
pub fn check(path: &str) -> Result<()> {
    if cfg!(target_arch = "wasm32") {
        return Ok(());
    }
    SCOPE.with(|s| s.borrow().as_ref().map_or(Ok(()), |scope| scope.check(path)))
}

/// A file's text, at most `max` bytes.
pub fn read_to_string(path: &str, max: usize) -> Result<String> {
    String::from_utf8(read(path, max)?).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_file_read_reports_bytes_before_completion_and_can_be_cancelled_between_chunks() {
        let path = std::env::temp_dir().join(format!("solidcraft-vfs-progress-{}", std::process::id()));
        let path = path.to_string_lossy();
        let bytes: Vec<_> = (0..1_000_003).map(|i| (i % 251) as u8).collect();
        super::write(&path, &bytes).unwrap();
        let mut reports = Vec::new();
        let read = super::imp::read_with_progress(&path, bytes.len(), |done, total| {
            reports.push((done, total));
            false
        })
        .unwrap();
        assert_eq!(read, bytes);
        assert_eq!(reports.first(), Some(&(0, bytes.len())));
        assert_eq!(reports.last(), Some(&(bytes.len(), bytes.len())));
        assert!(reports.iter().any(|&(done, total)| done > 0 && done < total));
        assert!(reports.windows(2).all(|p| p[0].0 < p[1].0 && p[1].1 == bytes.len()));

        let error = super::imp::read_with_progress(&path, bytes.len(), |done, _| done > 0).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        let progress = std::sync::Arc::new(solidcraft_doc::progress::Progress::default());
        solidcraft_doc::progress::watch(progress.clone(), || super::read(&path, bytes.len())).unwrap();
        assert_eq!(progress.work(), Some(("reading", bytes.len(), bytes.len())));
        progress.cancel();
        let error = solidcraft_doc::progress::watch(progress, || super::read(&path, bytes.len())).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        super::remove(&path).unwrap();
    }

    #[test]
    fn files_round_trip() {
        let dir = std::env::temp_dir().join(format!("solidcraft-vfs-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.txt").to_string_lossy().to_string();
        assert!(!super::exists(&p));
        assert!(!super::is_file(&p));
        assert!(!super::is_file(&dir.to_string_lossy()));
        assert!(super::read(&p, 16).is_err());
        super::write(&p, b"hello").unwrap();
        assert!(super::exists(&p));
        assert!(super::is_file(&p));
        assert_eq!(super::len(&p).unwrap(), 5);
        assert_eq!(super::read_to_string(&p, 16).unwrap(), "hello");
        // The cap is the caller's, not the file's.
        assert!(super::read(&p, 4).is_err());
        assert_eq!(super::read(&p, 5).unwrap(), b"hello");
        super::remove(&p).unwrap();
        assert!(!super::exists(&p));
        assert!(!super::is_file(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `metadata().len()` is 0 for a character device and for a FIFO, so a cap checked against
    /// it passes and the read then runs to an end that never comes. Refuse what is not a
    /// regular file, and do it before opening: opening a FIFO blocks until a writer appears.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn special_files_are_refused_without_blocking() {
        let e = super::read("/dev/zero", 1024).unwrap_err();
        assert!(e.to_string().contains("not a regular file"), "{e}");
        assert!(super::read_to_string("/dev/zero", 1024).is_err());

        let dir = std::env::temp_dir().join(format!("solidcraft-vfs-fifo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fifo = dir.join("p");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status().map(|s| s.success()).unwrap_or(false);
        if made {
            let path = fifo.to_string_lossy().to_string();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(super::read(&path, 1024).is_err());
            });
            let refused = rx.recv_timeout(std::time::Duration::from_secs(5));
            assert_eq!(refused, Ok(true), "reading a FIFO must be refused, not block");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An agent's scope (#205): inside its folders files work as before; outside, through `..`
    /// or through a symlink that leads out, every call is refused with a clear error and the
    /// disk is left alone. Without a scope nothing is confined.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn a_scope_confines_every_call_to_its_folders() {
        use super::{PathScope, scoped};
        let base = std::env::temp_dir().join(format!("solidcraft-vfs-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(&out, root.join("link")).unwrap();
        std::os::unix::fs::symlink(out.join("missing.txt"), root.join("dangling")).unwrap();
        let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
        let scope = PathScope::new([root.clone()]);
        scoped(Some(scope.clone()), || {
            let inside = s(root.join("sub/a.txt"));
            super::write(&inside, b"ok").unwrap();
            assert_eq!(super::read(&inside, 16).unwrap(), b"ok");
            assert_eq!(super::len(&inside).unwrap(), 2);
            assert!(super::exists(&inside));
            super::remove(&inside).unwrap();
            for escape in [out.join("secret.txt"), root.join("../out/secret.txt"), root.join("link/secret.txt"), root.join("dangling")] {
                let p = s(escape);
                let e = super::read(&p, 16).unwrap_err();
                assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{p}: {e}");
                assert!(e.to_string().contains("outside the folders this agent may use"), "{e}");
                assert!(super::write(&p, b"x").is_err() && super::remove(&p).is_err() && super::len(&p).is_err() && !super::exists(&p), "{p}");
            }
            assert!(super::write(&s(out.join("new.txt")), b"x").is_err());
            let e = super::check(&s(root.join("dangling/x.txt"))).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
            assert!(super::check(&s(root.join("sub/../sub/b.txt"))).is_ok(), "`..` that stays inside is fine");
        });
        assert_eq!(std::fs::read_to_string(out.join("secret.txt")).unwrap(), "secret");
        assert!(!out.join("new.txt").exists() && !out.join("missing.txt").exists());
        // The scope ends with the call; an empty one refuses everything.
        assert!(super::current_scope().is_none());
        assert_eq!(super::read(&s(out.join("secret.txt")), 16).unwrap(), b"secret");
        assert!(PathScope::new([base.join("nonexistent")]).check(&s(root.join("sub/a.txt"))).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Review of #205: a root is a folder, not a string prefix (`a` doesn't admit `ab`); a new
    /// file needs an existing folder, and one whose folder is a symlink that leads out is refused.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn roots_are_folders_and_new_files_resolve_through_their_folder() {
        use super::PathScope;
        let base = std::env::temp_dir().join(format!("solidcraft-vfs-prefix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, ab, out) = (base.join("a"), base.join("ab"), base.join("out"));
        for d in [&a, &ab, &out] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink(&out, a.join("link")).unwrap();
        let scope = PathScope::new([a.clone()]);
        let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
        assert!(scope.check(&s(a.join("new.txt"))).is_ok());
        for refused in [ab.join("x.txt"), a.join("link/new.txt"), base.join("missing/x.txt")] {
            let e = scope.check(&s(refused.clone())).unwrap_err();
            assert!(e.to_string().contains("outside the folders this agent may use"), "{}: {e}", refused.display());
        }
        // A folder that is missing inside an allowed one is reported as missing (#1077), not as
        // out of scope: widening `--allow-path` would not help. Nothing is created.
        for missing in [a.join("missing/x.txt"), a.join("missing/deeper/x.txt")] {
            let e = scope.check(&s(missing.clone())).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::NotFound, "{}: {e}", missing.display());
            assert!(e.to_string().contains("its folder does not exist") && !e.to_string().contains("--allow-path"), "{e}");
        }
        super::scoped(Some(scope), || assert!(super::write(&s(a.join("link/new.txt")), b"x").is_err()));
        assert!(!out.join("new.txt").exists() && !a.join("missing").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Windows rules, judged from the text alone, on every platform (#205 review).
    #[test]
    fn windows_paths_that_are_not_plain_local_files_are_refused() {
        use super::windows_refusal as w;
        for ok in [
            r"C:\work\part.step",
            "C:/work/part.step",
            r"\\?\C:\work\part.step",
            r"work\part.step",
            "part.step",
            r"\work\a.b.step",
            r"C:\com10\x.txt",
            r"C:\console.txt",
            r"\\?\c:\work\part.step",
            r"C:\work\draft~v2.step",
        ] {
            assert_eq!(w(ok), None, "{ok}");
        }
        for bad in [
            r"\\server\share\x.step",
            "//server/share/x.step",
            r"\\?\UNC\server\share\x.step",
            r"\??\UNC\server\share\x.step",
            r"\??\C:\x.step",
            "/??/UNC/server/share/x.step",
            r"\\.\COM1",
            r"\\?\Volume{0}\x",
            r"\\?\GLOBALROOT\Device\x",
            "C:foo.step",
            r"C:\work\a.step:stream",
            r"C:\work\NUL",
            r"C:\work\nul.txt",
            r"C:\work\COM1.step",
            r"C:\work\lpt9",
            r"C:\work\Con .txt",
            r"C:\work\COM¹",
            r"C:\work\CONOUT$",
            r"work\AUX\x.step",
            // Every spelling of the NT, device and UNC prefixes.
            r"\\?\unc\server\share\x.step",
            "//?/UNC/server/share/x.step",
            r"/\?\UNC\server\share\x.step",
            r"\??/UNC/server/share/x.step",
            r"\\.\C:\x.step",
            "//./pipe/x",
            r"\\?\C:",
            r"\\server",
            // Device names with any extension, in any case.
            r"C:\work\aux.tar.gz",
            r"C:\work\Prn.",
            r"C:\work\lPt1.step",
            r"C:\work\conin$.txt",
            // Alternate data streams, and short (8.3) names.
            r"C:\work\a.step::$DATA",
            r"C:\Users\me\SSH~1\id_ed25519",
            r"C:\PROGRA~1\x.step",
        ] {
            assert!(w(bad).is_some(), "{bad}");
        }
    }

    /// The same, through a scope on Windows itself: refused before any filesystem call.
    #[cfg(windows)]
    #[test]
    fn windows_unc_verbatim_drive_relative_and_device_paths_are_refused() {
        let root = super::temp_dir().join(format!("solidcraft-vfs-win-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let scope = super::PathScope::new([root.clone()]);
        let inside = |f: &str| root.join(f).to_string_lossy().to_string();
        assert!(scope.check(&inside("ok.step")).is_ok());
        for bad in [
            r"\\server\share\x.step".to_string(),
            r"\\?\UNC\server\share\x.step".to_string(),
            r"\??\UNC\server\share\x.step".to_string(),
            r"\\.\COM1".to_string(),
            "C:foo.step".to_string(),
            inside("NUL"),
            inside("COM1.txt"),
            inside("con"),
            inside("a.step:stream"),
        ] {
            let e = scope.check(&bad).unwrap_err();
            assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{bad}: {e}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The short (8.3) spelling of an existing `path` as `cmd` gives it (`NAME~1.XYZ`), or `None`
    /// when it has no short name (the volume makes none).
    #[cfg(windows)]
    fn short_spelling(path: &std::path::Path) -> Option<std::path::PathBuf> {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("cmd").raw_arg(format!("/d /c for %I in (\"{}\") do @echo %~sI", path.display())).output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| s.split('\\').any(super::short_name)).map(Into::into)
    }

    /// The folders SolidCraft derives from the environment are spelled with long names: on Windows
    /// the temporary folder's path often has a short (8.3) name (the user folder's `NAME~1.XYZ`
    /// when its name is long), and the server's own folders in it were refused like an agent's
    /// short names. An agent's own short spelling of a folder it may use stays refused.
    #[cfg(windows)]
    #[test]
    fn derived_folders_are_spelled_with_long_names_and_short_names_from_agents_stay_refused() {
        use super::{PathScope, long_names, short_name, temp_dir};
        assert!(!temp_dir().iter().any(|n| short_name(&n.to_string_lossy())), "{}", temp_dir().display());
        let long = temp_dir().join(format!("solidcraft-vfs-long-names-{}", std::process::id()));
        std::fs::create_dir_all(&long).unwrap();
        let Some(short) = short_spelling(&long) else {
            eprintln!("skipped: {} has no short (8.3) name on this volume", long.display());
            let _ = std::fs::remove_dir_all(&long);
            return;
        };
        let expanded = long_names(&short);
        assert_eq!(std::fs::canonicalize(&expanded).unwrap(), std::fs::canonicalize(&long).unwrap());
        assert!(super::windows_refusal(&expanded.to_string_lossy()).is_none(), "{}", expanded.display());
        // What doesn't exist yet is kept as given; a path without a short name is left alone.
        assert_eq!(long_names(&short.join("new folder").join("part.step")), expanded.join("new folder").join("part.step"));
        assert_eq!(long_names(&long), long);
        // The folder through a scope: its long spelling is in reach, an agent's short one is not.
        let scope = PathScope::new([long.clone()]);
        assert!(scope.check(&expanded.join("part.step").to_string_lossy()).is_ok());
        let e = scope.check(&short.join("part.step").to_string_lossy()).unwrap_err();
        assert!(e.to_string().contains("short (8.3) names"), "{e}");
        let _ = std::fs::remove_dir_all(&long);
    }

    /// The open design's folder counts only when it is safe (#205): strictly inside home (or the
    /// temporary folder), not home itself, a folder above it, a root or a hidden folder; and
    /// hidden files inside it stay out of reach. Folders named on purpose reach everything.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn the_designs_folder_counts_only_when_safe_and_never_reaches_hidden_files() {
        use super::{PathScope, design_folder_refusal_in as refusal};
        let base = std::env::temp_dir().join(format!("solidcraft-vfs-design-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let (proj, dotdir) = (home.join("work/proj"), home.join(".config/x"));
        for d in [&proj.join("sub"), &dotdir, &home.join(".ssh"), &proj.join(".git")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let sibling = base.join("homex");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(home.join(".ssh/id_ed25519"), "key").unwrap();
        std::fs::write(proj.join(".env"), "x").unwrap();
        std::os::unix::fs::symlink(home.join(".ssh"), proj.join("keys")).unwrap();
        let h = Some(home.as_path());
        assert_eq!(refusal(&proj, h, None), None);
        assert_eq!(refusal(&home.join("work/../work/proj"), h, None), None);
        for (dir, why) in [
            (home.clone(), "home"),
            (base.clone(), "above home"),
            ("/".into(), "root"),
            (sibling.clone(), "a name that only starts like home"),
            (dotdir.clone(), "hidden"),
            (base.join("missing"), "doesn't exist"),
        ] {
            assert!(refusal(&dir, h, None).is_some(), "{why}: {}", dir.display());
        }
        assert!(refusal(&proj, None, None).is_some(), "no home known: nothing implicit");
        assert_eq!(refusal(&base.join("homex"), None, Some(&base)), None, "inside the temporary folder");
        assert!(refusal(&base, None, Some(&base)).is_some(), "the temporary folder itself");

        // Through a scope: the design's folder reaches its own files, never hidden ones, and
        // never a link that leads to them.
        let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
        let mut scope = PathScope::new([]);
        // `with_design` judges with the real home; build the folder list directly instead.
        scope.design_roots.push(std::fs::canonicalize(&proj).unwrap());
        assert!(scope.check(&s(proj.join("part.step"))).is_ok());
        assert!(scope.check(&s(proj.join("sub/../part.step"))).is_ok());
        for bad in
            [proj.join(".env"), proj.join(".git/config"), proj.join("keys/id_ed25519"), proj.join("../../.ssh/id_ed25519"), home.join(".bashrc")]
        {
            assert!(scope.check(&s(bad.clone())).is_err(), "{}", bad.display());
        }
        // Named on purpose, a folder reaches its hidden files too.
        assert!(PathScope::new([proj.clone()]).check(&s(proj.join(".env"))).is_ok());
        // A design saved at `/` or in a missing folder adds nothing.
        assert!(PathScope::new([]).with_design(Some("/part.solidcraft")).design_roots().is_empty());
        assert!(PathScope::new([]).with_design(Some(&s(base.join("missing/p.solidcraft")))).design_roots().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Folders an agent opened or saved a design in (#7181) count as the open design's folder
    /// does, hidden ones never, and a scope keeps at most `MAX_DESIGN_FOLDERS`: the oldest gives
    /// way.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_scope_keeps_the_latest_design_folders() {
        use super::{MAX_DESIGN_FOLDERS, PathScope};
        let base = super::temp_dir().join(format!("solidcraft-vfs-design-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dirs: Vec<_> = (0..=MAX_DESIGN_FOLDERS).map(|i| base.join(format!("d{i}"))).collect();
        for d in dirs.iter().chain([&base.join(".hidden")]) {
            std::fs::create_dir_all(d).unwrap();
        }
        let scope = dirs
            .iter()
            .fold(PathScope::new([]), |s, d| s.with_design_folder(d))
            .with_design_folder(&dirs[1])
            .with_design_folder(&base.join(".hidden"));
        let canon = |d: &std::path::Path| std::fs::canonicalize(d).unwrap();
        assert_eq!(scope.design_roots().len(), MAX_DESIGN_FOLDERS, "{scope:?}");
        assert_eq!(scope.design_roots().last(), Some(&canon(&dirs[MAX_DESIGN_FOLDERS])), "{scope:?}");
        let file = |d: &std::path::Path| d.join("part.solidcraft").to_string_lossy().to_string();
        assert!(scope.check(&file(&dirs[1])).is_ok());
        assert!(scope.check(&file(&dirs[0])).is_err(), "the oldest gave way");
        assert!(scope.check(&file(&base.join(".hidden"))).is_err(), "a hidden folder never counts");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A save follows a symbolic link to the file it points to (#845), so the link's target is
    /// what a scope judges: a link inside an allowed folder that leads out is refused, a chain of
    /// links that stays inside is fine.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn a_link_is_judged_by_where_it_leads() {
        use super::PathScope;
        let base = std::env::temp_dir().join(format!("solidcraft-vfs-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, out) = (base.join("root"), base.join("out"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("part.solidcraft"), "outside").unwrap();
        std::fs::write(root.join("real.solidcraft"), "inside").unwrap();
        std::os::unix::fs::symlink(out.join("part.solidcraft"), root.join("part.solidcraft")).unwrap();
        std::os::unix::fs::symlink("real.solidcraft", root.join("a.solidcraft")).unwrap();
        std::os::unix::fs::symlink("a.solidcraft", root.join("b.solidcraft")).unwrap();
        std::os::unix::fs::symlink("../out/new.solidcraft", root.join("dangling.solidcraft")).unwrap();
        let scope = PathScope::new([root.clone()]);
        let s = |p: std::path::PathBuf| p.to_string_lossy().to_string();
        assert!(scope.check(&s(root.join("part.solidcraft"))).is_err());
        assert!(scope.check(&s(root.join("dangling.solidcraft"))).is_err());
        assert!(scope.check(&s(root.join("b.solidcraft"))).is_ok());
        super::scoped(Some(scope), || {
            let path = s(root.join("part.solidcraft"));
            let refused = super::check(&path).and_then(|()| crate::safe_write::write_atomic(std::path::Path::new(&path), b"agent", false));
            assert!(refused.is_err());
        });
        assert_eq!(std::fs::read_to_string(out.join("part.solidcraft")).unwrap(), "outside");
        assert!(!out.join("new.solidcraft").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A path a design names (a linked design, a font file) that leads to another computer or a
    /// device is refused from its text, on every platform, before any file call: on Windows even
    /// asking whether `\\host\share\x` exists connects to the host and may send it the user's
    /// sign-in. Local paths, absolute or relative, are unchanged.
    #[test]
    fn design_paths_on_network_locations_and_devices_are_refused_without_a_file_call() {
        use super::{Remote, check_design_path, design_path_is_file, design_path_refusal, read_design_path, remote};
        let net = |h: &str| Some(Remote::Network(h.into()));
        for (path, want) in [
            (r"\\server\share\part.solidcraft", net("server")),
            ("//server/share/part.solidcraft", net("server")),
            (r"\\Server.corp.example\share", net("Server.corp.example")),
            (r"\\10.0.0.5\c$\x.solidcraft", net("10.0.0.5")),
            (r"\\?\UNC\server\share\part.solidcraft", net("server")),
            (r"\\?\unc\server\share\part.solidcraft", net("server")),
            (r"\??\UNC\server\share\x", net("server")),
            ("//?/UNC/server/share/x", net("server")),
            (r"\\.\pipe\x", Some(Remote::Device)),
            ("//./pipe/x", Some(Remote::Device)),
            (r"\\.\COM1", Some(Remote::Device)),
            (r"\\?\Volume{0}\x", Some(Remote::Device)),
            (r"\\?\GLOBALROOT\Device\x", Some(Remote::Device)),
            (r"\??\C:\x", Some(Remote::Device)),
            (r"\\.", Some(Remote::Device)),
            ("CON", Some(Remote::Device)),
            ("nul", Some(Remote::Device)),
            ("NUL.solidcraft", Some(Remote::Device)),
            ("COM1", Some(Remote::Device)),
            ("com1.txt.solidcraft", Some(Remote::Device)),
            ("LPT9.ttf", Some(Remote::Device)),
            (r"C:\work\aux.step", Some(Remote::Device)),
            ("parts/Con .solidcraft", Some(Remote::Device)),
            (r"\\?\C:\work\prn", Some(Remote::Device)),
            // Local: unchanged.
            (r"C:\work\part.solidcraft", None),
            ("C:/work/part.solidcraft", None),
            (r"\\?\C:\work\part.solidcraft", None),
            ("/home/me/parts/plate.solidcraft", None),
            ("parts/plate.solidcraft", None),
            ("../parts/plate.solidcraft", None),
            ("console.solidcraft", None),
            (r"C:\com10\x.solidcraft", None),
        ] {
            assert_eq!(remote(path), want, "{path}");
            assert_eq!(design_path_refusal(path, None), want, "{path}");
            if want.is_some() {
                let calls = super::file_calls();
                let e = check_design_path(path, None).unwrap_err();
                assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{path}: {e}");
                assert!(read_design_path(path, None, 1 << 20).is_err(), "{path}");
                assert_eq!(design_path_is_file(path, None), None, "{path}");
                assert_eq!(super::file_calls(), calls, "{path}: refused, yet a file call was made");
                if let Some(Remote::Network(host)) = &want {
                    assert!(e.to_string().contains(&format!(r"network location \\{host}")), "{e}");
                } else {
                    assert!(e.to_string().contains("names a device"), "{e}");
                }
            }
        }
        // A local file is read as before.
        let dir = std::env::temp_dir().join(format!("solidcraft-vfs-design-path-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("part.solidcraft").to_string_lossy().to_string();
        super::write(&p, b"x").unwrap();
        assert_eq!(read_design_path(&p, None, 16).unwrap(), b"x");
        assert_eq!(design_path_is_file(&p, None), Some(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A server is reached once allowed: for one action ([`allowing_hosts`], "Allow once"), by
    /// the preferences ([`NetworkPolicy`]) or because the design itself was opened from it. A
    /// device path never is.
    #[test]
    fn an_allowed_network_host_passes_and_only_that_one() {
        use super::{NetworkPolicy, Remote, allowing_hosts, design_path_refusal};
        let unc = r"\\files.corp\cad\part.solidcraft";
        assert!(design_path_refusal(unc, None).is_some());
        allowing_hosts(&["FILES.corp".into()], || {
            assert_eq!(design_path_refusal(unc, None), None);
            assert_eq!(design_path_refusal(r"\\?\UNC\files.corp\cad\x", None), None);
            assert_eq!(design_path_refusal(r"\\evil\share\x", None), Some(Remote::Network("evil".into())));
            assert_eq!(design_path_refusal(r"\\.\pipe\x", None), Some(Remote::Device));
        });
        assert!(design_path_refusal(unc, None).is_some(), "allowed for that action only");
        // The design was opened from that server: its links beside it are reached.
        assert_eq!(design_path_refusal(unc, Some(r"\\Files.Corp\cad\asm.solidcraft")), None);
        assert!(design_path_refusal(unc, Some(r"\\other\cad\asm.solidcraft")).is_some());
        assert!(design_path_refusal(unc, Some("/home/me/asm.solidcraft")).is_some());
        let listed = NetworkPolicy { allow_all: false, hosts: vec![r"\\Files.corp".into()] };
        assert!(listed.allows("files.corp") && !listed.allows("evil") && !listed.allows(""));
        assert!(NetworkPolicy { allow_all: true, hosts: vec![] }.allows("anything"));
        assert!(!NetworkPolicy::default().allows("files.corp"), "empty by default");
    }

    /// The font file a design's sketch text names follows the same rule.
    #[test]
    fn a_design_font_file_on_a_network_location_is_refused() {
        super::install_checks();
        let calls = super::file_calls();
        let e = solidcraft_doc::sketch::font_data(Some(r"\\evil.example\fonts\x.ttf")).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(e.contains(r"network location \\evil.example"), "{e}");
        assert_eq!(super::file_calls(), calls);
    }

    /// A directory is not a file either.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn directories_are_not_files() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        assert!(super::read(&dir, 1024).is_err());
    }
}
