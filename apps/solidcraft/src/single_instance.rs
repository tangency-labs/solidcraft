//! Opening a design while SolidCraft already runs (#18055): the second launch hands the file to the
//! first and ends, and the first opens it as File > Open does.
//!
//! The channel is local to the user: a Unix domain socket in the settings folder (Linux, macOS) or a
//! named pipe (Windows). No network listener. A random token in the settings folder (readable by the
//! user only) must come with every request, so a pipe that other accounts could reach still takes
//! nothing from them.
//!
//! Only file paths travel, and whether they open read-only (`solidcraft --read-only`): those go under
//! `read_only_files` in place of `files` and are answered `ok read-only`. An instance from before the
//! option reads only `files`, so it refuses such a request (`no files`) and opens nothing; the launch
//! takes nothing but `ok read-only` and then opens them itself. The running instance accepts a path
//! only when it names an existing file, then queues it for the same open the File menu uses; nothing
//! it receives is run or interpreted beyond that.
//!
//! Workshop links (`solidcraft://workshop/…`, docs/workshop-links.md) travel the same way, under
//! `links`, answered `ok links`. An instance from before them refuses such a request (`no files`)
//! and the launch opens the link itself. The running instance only passes on text that looks like
//! a Workshop link; the window parses it strictly and asks before it opens or installs anything.
//!
//! A launch that gets no answer within [`ANSWER_WITHIN`] (a stale socket, a window that hangs) starts
//! normally, so the channel can never keep SolidCraft from opening. `--new-instance` skips the hand-off.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use interprocess::local_socket::{ListenerOptions, Stream, prelude::*};
use serde_json::{Value, json};

/// How long a launch waits for the running instance before it starts normally.
pub const ANSWER_WITHIN: Duration = Duration::from_millis(1500);
/// The most files one request carries.
const MAX_FILES: usize = 64;
/// The longest request line, in bytes.
const MAX_LINE: u64 = 256 * 1024;
/// The longest path accepted, in bytes.
const MAX_PATH: usize = 4096;
/// Connections served at once; more wait for the next accept.
const MAX_CONNECTIONS: usize = 8;
/// The reply to an accepted request.
const OK: &str = "ok";
/// The reply to an accepted read-only request: an instance from before `--read-only` never sends it.
const OK_READ_ONLY: &str = "ok read-only";
/// The reply to accepted Workshop links: an instance from before them never sends it.
const OK_LINKS: &str = "ok links";

/// What a request hands over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handed {
    /// Files to open, and whether they open read-only.
    Files(Vec<String>, bool),
    /// `solidcraft://` links for the window to look up and ask about.
    Links(Vec<String>),
}

impl Handed {
    fn reply(&self) -> &'static str {
        match self {
            Handed::Files(_, false) => OK,
            Handed::Files(_, true) => OK_READ_ONLY,
            Handed::Links(_) => OK_LINKS,
        }
    }
}

/// Where the running instance listens and the token it expects.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// The socket file (Unix) or the pipe's name source (Windows).
    socket: PathBuf,
    /// The file holding the token.
    token: PathBuf,
}

impl Endpoint {
    /// The endpoint of the instance that uses `settings`: two instances with different settings
    /// folders (`SOLIDCRAFT_SETTINGS_DIR`) are separate apps. `None` where a socket path would be
    /// too long for the system.
    pub fn in_settings(settings: &Path) -> Option<Self> {
        let socket = settings.join("open.sock");
        // sockaddr_un holds about 104 bytes on macOS and 108 on Linux.
        if cfg!(unix) && socket.as_os_str().len() > 100 {
            return None;
        }
        Some(Self { socket, token: settings.join("open.token") })
    }

    fn name(&self) -> std::io::Result<interprocess::local_socket::Name<'static>> {
        #[cfg(unix)]
        {
            use interprocess::local_socket::{GenericFilePath, ToFsName};
            self.socket.clone().to_fs_name::<GenericFilePath>()
        }
        #[cfg(not(unix))]
        {
            use interprocess::local_socket::{GenericNamespaced, ToNsName};
            use std::hash::{Hash, Hasher};
            // The pipe's name is flat: one per settings folder.
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            self.socket.hash(&mut hasher);
            format!("solidcraft-open-{:016x}", hasher.finish()).to_ns_name::<GenericNamespaced>()
        }
    }
}

/// The paths of a request that name existing files, each made absolute. Anything else is dropped:
/// a path that is relative, has a NUL, is too long or is not a file.
pub fn valid_files(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .take(MAX_FILES)
        .filter(|p| !p.is_empty() && p.len() <= MAX_PATH && !p.contains('\0'))
        .filter(|p| {
            let path = Path::new(p.as_str());
            path.is_absolute() && path.is_file()
        })
        .cloned()
        .collect()
}

/// The Workshop links of a request that could be links at all; anything else is dropped. The
/// window parses each one strictly before it does anything with it.
pub fn valid_links(links: &[String]) -> Vec<String> {
    links
        .iter()
        .take(MAX_FILES)
        .filter(|l| solidcraft_ui_egui::workshop::link::looks_like_link(l) && !l.chars().any(char::is_control))
        .cloned()
        .collect()
}

/// A request line: the token and the files, and whether they open read-only (`read_only_files` in
/// place of `files`), or Workshop links (`links`). `Err` says why it is refused.
pub fn parse_request(line: &str, token: &str) -> Result<Handed, &'static str> {
    let value: Value = serde_json::from_str(line).map_err(|_| "not a request")?;
    let given = value.get("token").and_then(Value::as_str).ok_or("no token")?;
    if !solidcraft_control::auth::tokens_match(given, token) {
        return Err("wrong token");
    }
    let (files, kind) = match (value.get("files"), value.get("read_only_files"), value.get("links")) {
        (Some(files), None, None) => (files, 0),
        (None, Some(files), None) => (files, 1),
        (None, None, Some(links)) => (links, 2),
        (None, None, None) => return Err("no files"),
        (_, _, Some(_)) => return Err("files and links together"),
        _ => return Err("files and read-only files together"),
    };
    let files = files.as_array().ok_or("no files")?;
    if files.len() > MAX_FILES {
        return Err("too many files");
    }
    let files = files.iter().map(|f| f.as_str().map(str::to_string).ok_or("a file is not text")).collect::<Result<Vec<_>, _>>()?;
    Ok(match kind {
        0 => Handed::Files(files, false),
        1 => Handed::Files(files, true),
        _ => Handed::Links(files),
    })
}

/// Make a relative path absolute against the working directory, as the second launch sees it.
pub fn absolute(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        return path.to_string();
    }
    std::env::current_dir().map(|dir| dir.join(p).to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_string())
}

/// Hand `files` to the instance listening at `endpoint`. `true`: it took them, and this launch
/// should end. `false`: nobody answered in time, or it refused; start normally. No files only asks
/// whether an instance is there.
pub fn forward(endpoint: &Endpoint, files: &[String]) -> bool {
    send(endpoint, Handed::Files(files.to_vec(), false))
}

/// [`forward`] for files that open read-only (`solidcraft --read-only`). `true` only when the
/// instance answered that it opens them read-only; an older one refuses, and this launch starts.
pub fn forward_read_only(endpoint: &Endpoint, files: &[String]) -> bool {
    send(endpoint, Handed::Files(files.to_vec(), true))
}

/// Hand Workshop links to the running instance. `true` only when it answered that it takes
/// links; an older one refuses, and this launch starts and opens them itself.
pub fn forward_links(endpoint: &Endpoint, links: &[String]) -> bool {
    send(endpoint, Handed::Links(links.to_vec()))
}

fn send(endpoint: &Endpoint, handed: Handed) -> bool {
    let Ok(token) = std::fs::read_to_string(&endpoint.token) else { return false };
    let request = request_line(token.trim(), &handed);
    let expected = handed.reply();
    let endpoint = endpoint.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    // A thread so that an instance that accepts and never answers cannot hold this launch.
    let spawned = std::thread::Builder::new().name("single-instance-forward".into()).spawn(move || {
        let _ = tx.send(ask(&endpoint, &request, expected));
    });
    spawned.is_ok() && rx.recv_timeout(ANSWER_WITHIN).unwrap_or(false)
}

/// The request line. Read-only files go under `read_only_files` and links under `links`, which an
/// instance from before them does not read: it refuses (`no files`) instead of opening them some
/// other way.
fn request_line(token: &str, handed: &Handed) -> String {
    match handed {
        Handed::Files(files, false) => json!({ "token": token, "files": files }),
        Handed::Files(files, true) => json!({ "token": token, "read_only_files": files }),
        Handed::Links(links) => json!({ "token": token, "links": links }),
    }
    .to_string()
}

/// Whether `reply` accepts the request: the answer for its kind ([`Handed::reply`]).
fn accepted(reply: &str, expected: &str) -> bool {
    reply.trim() == expected
}

fn ask(endpoint: &Endpoint, request: &str, expected: &str) -> bool {
    let Ok(name) = endpoint.name() else { return false };
    let Ok(stream) = Stream::connect(name) else { return false };
    let mut stream = BufReader::new(stream);
    if writeln!(stream.get_mut(), "{request}").is_err() || stream.get_mut().flush().is_err() {
        return false;
    }
    let mut reply = String::new();
    stream.read_line(&mut reply).is_ok() && accepted(&reply, expected)
}

/// What the listener does with what a request hands over.
pub type Sink = Arc<dyn Fn(Handed) + Send + Sync>;

/// Listen at `endpoint` on a thread of its own, handing the files of each valid request to `sink`.
/// Does nothing (`false`) when another instance already answers there, so a `--new-instance` run
/// never takes the channel from the first one, or when the channel cannot be made.
#[cfg(test)]
pub fn listen(endpoint: &Endpoint, sink: Sink) -> bool {
    listen_while(endpoint, sink, Arc::new(AtomicBool::new(true)))
}

fn listen_while(endpoint: &Endpoint, sink: Sink, open: Arc<AtomicBool>) -> bool {
    if forward(endpoint, &[]) {
        return false;
    }
    let Some(token) = solidcraft_control::server::new_token() else { return false };
    if write_token(&endpoint.token, &token).is_err() {
        return false;
    }
    let Ok(listener) = create_listener(endpoint) else { return false };
    let busy = Arc::new(AtomicUsize::new(0));
    std::thread::Builder::new()
        .name("single-instance-listen".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if busy.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    continue;
                }
                busy.fetch_add(1, Ordering::SeqCst);
                let (busy, sink, token, open) = (busy.clone(), sink.clone(), token.clone(), open.clone());
                let served = std::thread::Builder::new().name("single-instance-serve".into()).spawn({
                    let busy = busy.clone();
                    move || {
                        serve(stream, &token, &sink, &open);
                        busy.fetch_sub(1, Ordering::SeqCst);
                    }
                });
                if served.is_err() {
                    busy.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })
        .is_ok()
}

/// The listener at `endpoint`, replacing a socket file left by a crash (a live one was ruled out
/// before). On Unix only its owner may connect: the socket is made with mode 0600 where the system
/// lets a socket's mode be set before it is bound (Linux, the BSDs), and otherwise (macOS)
/// is given that mode just after. A request still needs the token, which only the owner can
/// read, so the moment between the two lets nobody in.
fn create_listener(endpoint: &Endpoint) -> std::io::Result<interprocess::local_socket::Listener> {
    let options = || -> std::io::Result<ListenerOptions<'static>> { Ok(ListenerOptions::new().name(endpoint.name()?).try_overwrite(true)) };
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        use std::os::unix::fs::PermissionsExt;
        match options()?.mode(0o600).create_sync() {
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
                let listener = options()?.create_sync()?;
                std::fs::set_permissions(&endpoint.socket, std::fs::Permissions::from_mode(0o600))?;
                Ok(listener)
            }
            made => made,
        }
    }
    #[cfg(not(unix))]
    options()?.create_sync()
}

/// One connection: read the request, answer, and pass the files on.
fn serve(stream: Stream, token: &str, sink: &Sink, open: &AtomicBool) {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    let read = (&mut stream).take(MAX_LINE).read_line(&mut line);
    let reply = match read {
        _ if !open.load(Ordering::SeqCst) => "refused: closing".to_string(),
        Ok(n) if n > 0 && line.ends_with('\n') => match parse_request(&line, token) {
            Ok(handed) => {
                let answer = handed.reply();
                let handed = match handed {
                    Handed::Files(files, read_only) => Handed::Files(valid_files(&files), read_only),
                    Handed::Links(links) => Handed::Links(valid_links(&links)),
                };
                if !matches!(&handed, Handed::Files(list, _) | Handed::Links(list) if list.is_empty()) {
                    sink(handed);
                }
                answer.to_string()
            }
            Err(why) => format!("refused: {why}"),
        },
        _ => "refused: not a request".to_string(),
    };
    let _ = writeln!(stream.get_mut(), "{reply}");
}

/// Write the token so that only the user reads it.
fn write_token(path: &Path, token: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(token.as_bytes())
}

/// Remove the files this instance made (at exit). A later instance replaces them anyway.
pub fn cleanup(endpoint: &Endpoint) {
    #[cfg(unix)]
    let _ = std::fs::remove_file(&endpoint.socket);
    let _ = std::fs::remove_file(&endpoint.token);
}

/// Files that arrived and have not been opened yet.
static PENDING: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());
/// Workshop links that arrived (or were named at launch) and have not been looked up yet.
static PENDING_LINKS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The endpoint this instance listens at, for [`stop`].
static LISTENING: OnceLock<(Endpoint, Arc<AtomicBool>)> = OnceLock::new();

/// Keep handed-over files until the window opens them ([`take`]).
fn hold(files: Vec<String>, read_only: bool) {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).extend(files.into_iter().map(|f| (f, read_only)));
}

/// Keep Workshop links until the window looks them up ([`take_links`]): links named on this
/// launch's command line, or handed over by a later launch.
pub fn hold_links(links: Vec<String>) {
    PENDING_LINKS.lock().unwrap_or_else(PoisonError::into_inner).extend(links);
}

/// The Workshop links that arrived since the last call.
pub fn take_links() -> Vec<String> {
    std::mem::take(&mut *PENDING_LINKS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Start listening for files other launches hand over; the window comes forward and opens them.
pub fn start(settings: Option<&Path>, ctx: &egui::Context) {
    let Some(endpoint) = settings.and_then(Endpoint::in_settings) else { return };
    let ctx = ctx.clone();
    let sink: Sink = Arc::new(move |handed: Handed| {
        match handed {
            Handed::Files(files, read_only) => hold(files, read_only),
            Handed::Links(links) => hold_links(links),
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    });
    let open = Arc::new(AtomicBool::new(true));
    if listen_while(&endpoint, sink, open.clone()) {
        let _ = LISTENING.set((endpoint, open));
    }
}

/// Stop advertising this instance (at exit).
pub fn stop() {
    if let Some((endpoint, open)) = LISTENING.get() {
        // Only once: a second call at exit would delete the token the next launch just wrote.
        if open.swap(false, Ordering::SeqCst) {
            cleanup(endpoint);
        }
    }
}

/// The paths that arrived since the last call, each with whether it opens read-only.
pub fn take() -> Vec<(String, bool)> {
    std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-single-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn collecting() -> (Sink, Arc<Mutex<Vec<String>>>) {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        (
            Arc::new(move |handed: Handed| {
                if let Handed::Files(files, _) = handed {
                    sink.lock().unwrap().extend(files)
                }
            }),
            got,
        )
    }

    fn wait_for(got: &Arc<Mutex<Vec<String>>>, count: usize) -> Vec<String> {
        for _ in 0..200 {
            if got.lock().unwrap().len() >= count {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        got.lock().unwrap().clone()
    }

    #[test]
    fn request_needs_the_token_and_text_files() {
        assert_eq!(parse_request(r#"{"token":"t","files":["/a.sc","/b.sc"]}"#, "t"), Ok(Handed::Files(vec!["/a.sc".into(), "/b.sc".into()], false)));
        assert_eq!(parse_request(r#"{"token":"x","files":["/a.sc"]}"#, "t"), Err("wrong token"));
        assert_eq!(parse_request(r#"{"files":["/a.sc"]}"#, "t"), Err("no token"));
        assert_eq!(parse_request(r#"{"token":"t"}"#, "t"), Err("no files"));
        assert_eq!(parse_request(r#"{"token":"t","files":[1]}"#, "t"), Err("a file is not text"));
        assert_eq!(parse_request("open /etc/passwd", "t"), Err("not a request"));
        let many = format!(r#"{{"token":"t","files":[{}]}}"#, vec!["\"/a\""; MAX_FILES + 1].join(","));
        assert_eq!(parse_request(&many, "t"), Err("too many files"));
    }

    #[test]
    fn only_existing_absolute_files_pass() {
        let dir = temp("valid");
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let file = file.to_string_lossy().into_owned();
        let paths = vec![
            file.clone(),
            dir.to_string_lossy().into_owned(),
            dir.join("missing.sc").to_string_lossy().into_owned(),
            "part.sc".into(),
            String::new(),
            format!("{file}\0.txt"),
            "x".repeat(MAX_PATH + 1),
        ];
        assert_eq!(valid_files(&paths), vec![file]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_paths_become_absolute() {
        let made = absolute("part.sc");
        assert!(Path::new(&made).is_absolute());
        assert!(made.ends_with("part.sc"));
        assert_eq!(
            absolute(&std::env::current_dir().unwrap().join("a.sc").to_string_lossy()),
            std::env::current_dir().unwrap().join("a.sc").to_string_lossy()
        );
    }

    #[test]
    fn a_file_reaches_the_running_instance() {
        let dir = temp("roundtrip");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let (sink, got) = collecting();
        assert!(listen(&endpoint, sink));
        // Only its owner may connect, on macOS too, where the mode is set after binding.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&endpoint.socket).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let file = file.to_string_lossy().into_owned();
        assert!(forward(&endpoint, std::slice::from_ref(&file)));
        assert_eq!(wait_for(&got, 1), vec![file]);
        // Files that do not exist are dropped, though the request is answered.
        assert!(forward(&endpoint, &[dir.join("none.sc").to_string_lossy().into_owned()]));
        assert_eq!(got.lock().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_listener_does_not_take_the_channel() {
        let dir = temp("second");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let (first, got) = collecting();
        assert!(listen(&endpoint, first));
        let (second, other) = collecting();
        assert!(!listen(&endpoint, second));
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        assert!(forward(&endpoint, &[file.to_string_lossy().into_owned()]));
        assert_eq!(wait_for(&got, 1).len(), 1);
        assert!(other.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nobody_listening_starts_normally() {
        let dir = temp("nobody");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        // No token file, no socket.
        assert!(!forward(&endpoint, &["/a.sc".into()]));
        // A token file and a stale socket file left by a crash: refused at once, and a new
        // instance replaces the socket.
        std::fs::write(&endpoint.token, "old").unwrap();
        #[cfg(unix)]
        {
            drop(std::os::unix::net::UnixListener::bind(&endpoint.socket).unwrap());
            assert!(endpoint.socket.exists());
        }
        let started = std::time::Instant::now();
        assert!(!forward(&endpoint, &["/a.sc".into()]));
        assert!(started.elapsed() < ANSWER_WITHIN);
        let (sink, got) = collecting();
        assert!(listen(&endpoint, sink));
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        assert!(forward(&endpoint, &[file.to_string_lossy().into_owned()]));
        assert_eq!(wait_for(&got, 1).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wrong_token_is_refused() {
        let dir = temp("token");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let (sink, got) = collecting();
        assert!(listen(&endpoint, sink));
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let request = json!({ "token": "guess", "files": [file.to_string_lossy()] }).to_string();
        assert!(!ask(&endpoint, &request, OK));
        std::thread::sleep(Duration::from_millis(100));
        assert!(got.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #19340: after `stop`, a successor this window starts must not hand its designs back to it.
    #[test]
    fn a_stopped_instance_takes_no_files() {
        let dir = temp("stopped");
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        start(Some(&dir), &egui::Context::default());
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let token = std::fs::read_to_string(&endpoint.token).unwrap();
        stop();
        let request = json!({ "token": token.trim(), "files": [file.to_string_lossy()] }).to_string();
        assert!(!ask(&endpoint, &request, OK), "a stopped instance took a hand-off");
        std::thread::sleep(Duration::from_millis(100));
        assert!(take().is_empty());
        std::fs::write(&endpoint.token, "next launch").unwrap();
        stop();
        assert!(endpoint.token.exists(), "a second stop removed the next launch's token");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_only_request_names_its_files_apart() {
        let files = vec!["/a.sc".to_string()];
        assert_eq!(parse_request(r#"{"token":"t","read_only_files":["/a.sc"]}"#, "t"), Ok(Handed::Files(vec!["/a.sc".into()], true)));
        assert_eq!(parse_request(r#"{"token":"t","files":["/a.sc"],"read_only_files":["/a.sc"]}"#, "t"), Err("files and read-only files together"));
        assert_eq!(parse_request(r#"{"token":"t","read_only_files":[1]}"#, "t"), Err("a file is not text"));
        let many = format!(r#"{{"token":"t","read_only_files":[{}]}}"#, vec!["\"/a\""; MAX_FILES + 1].join(","));
        assert_eq!(parse_request(&many, "t"), Err("too many files"));
        let plain: Value = serde_json::from_str(&request_line("t", &Handed::Files(files.clone(), false))).unwrap();
        assert_eq!(plain, json!({"token": "t", "files": ["/a.sc"]}));
        let read_only: Value = serde_json::from_str(&request_line("t", &Handed::Files(files.clone(), true))).unwrap();
        assert_eq!(read_only, json!({"token": "t", "read_only_files": ["/a.sc"]}));
        assert!(read_only.get("files").is_none());
        assert_eq!(parse_request(&request_line("t", &Handed::Files(files.clone(), true)), "t"), Ok(Handed::Files(files, true)));
    }

    #[test]
    fn only_the_read_only_answer_takes_a_read_only_request() {
        assert!(accepted("ok\n", OK));
        assert!(accepted("ok read-only\n", OK_READ_ONLY));
        assert!(!accepted("ok\n", OK_READ_ONLY));
        assert!(!accepted("ok read-only\n", OK));
        assert!(!accepted("refused: no files\n", OK_READ_ONLY));
        assert!(accepted("ok links\n", OK_LINKS));
        assert!(!accepted("ok\n", OK_LINKS));
    }

    #[test]
    fn a_plain_ok_to_a_read_only_request_starts_a_window_of_its_own() {
        use interprocess::local_socket::{ListenerOptions, prelude::*};
        // A stand-in that answers `ok` to whatever it reads: even that does not take read-only files.
        let dir = temp("old");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        std::fs::write(&endpoint.token, "t").unwrap();
        let listener = ListenerOptions::new().name(endpoint.name().unwrap()).try_overwrite(true).create_sync().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_ok() {
                    let _ = writeln!(reader.get_mut(), "ok");
                }
            }
        });
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let file = file.to_string_lossy().into_owned();
        assert!(forward(&endpoint, std::slice::from_ref(&file)));
        assert!(!forward_read_only(&endpoint, std::slice::from_ref(&file)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_instance_from_before_the_option_opens_no_read_only_file() {
        use interprocess::local_socket::{ListenerOptions, prelude::*};
        // `serve` as it was before `--read-only`: it reads only `files`, and a request without them
        // is refused before anything is opened.
        let dir = temp("before");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        std::fs::write(&endpoint.token, "t").unwrap();
        let listener = ListenerOptions::new().name(endpoint.name().unwrap()).try_overwrite(true).create_sync().unwrap();
        let (sink, got) = collecting();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let request: Value = serde_json::from_str(&line).unwrap_or_default();
                let reply = match request.get("files").and_then(Value::as_array) {
                    Some(files) => {
                        sink(Handed::Files(files.iter().filter_map(Value::as_str).map(str::to_string).collect(), false));
                        OK.to_string()
                    }
                    None => "refused: no files".to_string(),
                };
                let _ = writeln!(reader.get_mut(), "{reply}");
            }
        });
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let file = file.to_string_lossy().into_owned();
        assert!(!forward_read_only(&endpoint, std::slice::from_ref(&file)));
        assert!(got.lock().unwrap().is_empty(), "nothing is opened for writing");
        assert!(forward(&endpoint, std::slice::from_ref(&file)));
        assert_eq!(wait_for(&got, 1), vec![file]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_read_only_file_reaches_the_running_instance_read_only() {
        let dir = temp("readonly");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let got: Arc<Mutex<Vec<(String, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_got = got.clone();
        let sink: Sink = Arc::new(move |handed: Handed| {
            if let Handed::Files(files, read_only) = handed {
                sink_got.lock().unwrap().extend(files.into_iter().map(|f| (f, read_only)));
            }
        });
        assert!(listen(&endpoint, sink));
        let file = dir.join("part.sc");
        std::fs::write(&file, b"x").unwrap();
        let file = file.to_string_lossy().into_owned();
        assert!(forward_read_only(&endpoint, std::slice::from_ref(&file)));
        assert!(forward(&endpoint, std::slice::from_ref(&file)));
        for _ in 0..200 {
            if got.lock().unwrap().len() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*got.lock().unwrap(), vec![(file.clone(), true), (file, false)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn held_files_keep_whether_they_open_read_only() {
        hold(vec!["/a.sc".into()], false);
        hold(vec!["/b.sc".into(), "/c.sc".into()], true);
        assert_eq!(take(), vec![("/a.sc".to_string(), false), ("/b.sc".to_string(), true), ("/c.sc".to_string(), true)]);
        assert!(take().is_empty());
    }

    #[test]
    fn a_workshop_link_reaches_the_running_instance() {
        let dir = temp("links");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        let got: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink_got = got.clone();
        let sink: Sink = Arc::new(move |handed: Handed| match handed {
            Handed::Links(links) => sink_got.lock().unwrap().extend(links),
            Handed::Files(..) => panic!("a link arrived as a file"),
        });
        assert!(listen(&endpoint, sink));
        let link = "solidcraft://workshop/model/bherbruck/wall-mount-shelf-bracket@1.2.0".to_string();
        assert!(forward_links(&endpoint, &[link.clone(), "/etc/passwd".into(), "solidcraft://workshop/x\u{7}".into()]));
        assert_eq!(wait_for(&got, 1), vec![link.clone()]);
        assert_eq!(
            parse_request(r#"{"token":"t","links":["solidcraft://workshop/plugin/maker/gears"]}"#, "t"),
            Ok(Handed::Links(vec!["solidcraft://workshop/plugin/maker/gears".into()]))
        );
        assert_eq!(parse_request(r#"{"token":"t","links":["a"],"files":["/a.sc"]}"#, "t"), Err("files and links together"));
        assert_eq!(valid_links(&[link.clone(), "https://workshop.solidcraft.io/model/a/b".into()]), vec![link]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_instance_from_before_links_leaves_the_link_to_this_launch() {
        use interprocess::local_socket::{ListenerOptions, prelude::*};
        // `serve` as it was before links: no `files`, so it refuses and opens nothing.
        let dir = temp("before-links");
        let endpoint = Endpoint::in_settings(&dir).unwrap();
        std::fs::write(&endpoint.token, "t").unwrap();
        let listener = ListenerOptions::new().name(endpoint.name().unwrap()).try_overwrite(true).create_sync().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let request: Value = serde_json::from_str(&line).unwrap_or_default();
                let reply = if request.get("files").is_some() { OK } else { "refused: no files" };
                let _ = writeln!(reader.get_mut(), "{reply}");
            }
        });
        assert!(!forward_links(&endpoint, &["solidcraft://workshop/plugin/maker/gears".into()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn held_links_wait_for_the_window() {
        hold_links(vec!["solidcraft://workshop/plugin/maker/gears".into()]);
        assert_eq!(take_links(), vec!["solidcraft://workshop/plugin/maker/gears".to_string()]);
        assert!(take_links().is_empty());
    }

    #[test]
    fn long_socket_paths_are_not_used() {
        let long = PathBuf::from(format!("/{}", "a".repeat(200)));
        assert_eq!(Endpoint::in_settings(&long).is_none(), cfg!(unix));
    }
}
