//! Opening a design while SolidCraft already runs (#18055): the second launch hands the file to the
//! first and ends, and the first opens it as File > Open does.
//!
//! The channel is local to the user: a Unix domain socket in the settings folder (Linux, macOS) or a
//! named pipe (Windows). No network listener. A random token in the settings folder (readable by the
//! user only) must come with every request, so a pipe that other accounts could reach still takes
//! nothing from them.
//!
//! Only file paths travel. The running instance accepts a path only when it names an existing file,
//! then queues it for the same open the File menu uses; nothing it receives is run or interpreted
//! beyond that.
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

/// A request line: the token and the files. `Err` says why it is refused.
pub fn parse_request(line: &str, token: &str) -> Result<Vec<String>, &'static str> {
    let value: Value = serde_json::from_str(line).map_err(|_| "not a request")?;
    let given = value.get("token").and_then(Value::as_str).ok_or("no token")?;
    if !solidcraft_control::auth::tokens_match(given, token) {
        return Err("wrong token");
    }
    let files = value.get("files").and_then(Value::as_array).ok_or("no files")?;
    if files.len() > MAX_FILES {
        return Err("too many files");
    }
    files.iter().map(|f| f.as_str().map(str::to_string).ok_or("a file is not text")).collect()
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
    let Ok(token) = std::fs::read_to_string(&endpoint.token) else { return false };
    let request = json!({ "token": token.trim(), "files": files }).to_string();
    let endpoint = endpoint.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    // A thread so that an instance that accepts and never answers cannot hold this launch.
    let spawned = std::thread::Builder::new().name("single-instance-forward".into()).spawn(move || {
        let _ = tx.send(ask(&endpoint, &request));
    });
    spawned.is_ok() && rx.recv_timeout(ANSWER_WITHIN).unwrap_or(false)
}

fn ask(endpoint: &Endpoint, request: &str) -> bool {
    let Ok(name) = endpoint.name() else { return false };
    let Ok(stream) = Stream::connect(name) else { return false };
    let mut stream = BufReader::new(stream);
    if writeln!(stream.get_mut(), "{request}").is_err() || stream.get_mut().flush().is_err() {
        return false;
    }
    let mut reply = String::new();
    stream.read_line(&mut reply).is_ok() && reply.trim() == OK
}

/// What the listener does with accepted files.
pub type Sink = Arc<dyn Fn(Vec<String>) + Send + Sync>;

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
            Ok(files) => {
                let files = valid_files(&files);
                if !files.is_empty() {
                    sink(files);
                }
                OK.to_string()
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
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The endpoint this instance listens at, for [`stop`].
static LISTENING: OnceLock<(Endpoint, Arc<AtomicBool>)> = OnceLock::new();

/// Start listening for files other launches hand over; the window comes forward and opens them.
pub fn start(settings: Option<&Path>, ctx: &egui::Context) {
    let Some(endpoint) = settings.and_then(Endpoint::in_settings) else { return };
    let ctx = ctx.clone();
    let sink: Sink = Arc::new(move |files| {
        PENDING.lock().unwrap_or_else(PoisonError::into_inner).extend(files);
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

/// The paths that arrived since the last call.
pub fn take() -> Vec<String> {
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
        (Arc::new(move |files| sink.lock().unwrap().extend(files)), got)
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
        assert_eq!(parse_request(r#"{"token":"t","files":["/a.sc","/b.sc"]}"#, "t"), Ok(vec!["/a.sc".into(), "/b.sc".into()]));
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
        assert!(!ask(&endpoint, &request));
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
        assert!(!ask(&endpoint, &request), "a stopped instance took a hand-off");
        std::thread::sleep(Duration::from_millis(100));
        assert!(take().is_empty());
        std::fs::write(&endpoint.token, "next launch").unwrap();
        stop();
        assert!(endpoint.token.exists(), "a second stop removed the next launch's token");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_socket_paths_are_not_used() {
        let long = PathBuf::from(format!("/{}", "a".repeat(200)));
        assert_eq!(Endpoint::in_settings(&long).is_none(), cfg!(unix));
    }
}
