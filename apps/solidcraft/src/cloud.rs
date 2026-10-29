//! Talking to CloudCraft from this application.
//!
//! [`service`] starts the worker that the cloud Open and Save pages run on.
//! SolidCraft holds no provider credential and speaks no provider protocol.
//! Everything goes through the CloudCraft service, which is the one place a
//! provider is configured. This file supplies the transport half of
//! `solidcraft_ui_egui::cloud` and the two things a desktop application has
//! to do that a library cannot: open a browser, and decide how long to keep
//! polling.
//!
//! Every call in `craft_cloud_client` blocks. The pages therefore never make
//! one. They state a [`Job`], this thread runs it, and the answer goes back
//! through the [`Post`] the job arrived with. The egui context is woken
//! afterwards, because a worker answering is not a local event and the window
//! would otherwise sit still until the mouse moved.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use craft_cloud_client::{Client, Error};
use solidcraft_ui_egui::cloud::{Account, CloudBackend, Event, Fault, GitView, Job, Post};

/// How long to keep polling a sign-in before giving up on it.
const SIGN_IN_WAIT: Duration = Duration::from_secs(300);

/// Where the CloudCraft service is.
///
/// Read from the environment and compiled in nowhere, because the service is
/// deployment-specific and a desktop binary must not carry one. Without it
/// there is no cloud worker, and the interface offers nothing cloud-shaped.
fn base_url() -> Option<String> {
    ["SOLIDCRAFT_CLOUD_URL", "CRAFT_CLOUD_URL", "COLLABCRAFT_URL"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .find(|value| !value.is_empty())
}

/// A handle to the worker, as the pages see it.
///
/// Every job is answered on that thread. Nothing a page does can block a
/// frame. One queue serves them all, so a large save holds up the jobs
/// behind it.
pub struct Service {
    jobs: Sender<(Job, Post)>,
}

impl CloudBackend for Service {
    fn submit(&self, job: Job, post: Post) {
        if self.jobs.send((job, post)).is_err() {
            log::warn!("the cloud worker is gone; the job was dropped");
        }
    }
}

/// Starts the worker, when a service is configured.
pub fn service(ctx: egui::Context) -> Option<Service> {
    let base = base_url()?;
    log::info!("cloud storage through {base}");
    let (jobs, receiver) = std::sync::mpsc::channel::<(Job, Post)>();
    let started = std::thread::Builder::new().name("cloudcraft".to_owned()).spawn(move || {
        let token = load_token();
        let mut client = Client::new(&base);
        if let Some(token) = token {
            client = client.with_token(token);
        }
        let mut worker = Worker { client, base, ctx, answering: None };
        while let Ok((job, post)) = receiver.recv() {
            worker.answering = Some(post);
            worker.run(job);
        }
    });
    if let Err(e) = started {
        log::error!("the cloud worker did not start: {e}");
        return None;
    }
    Some(Service { jobs })
}

struct Worker {
    client: Client,
    /// Kept beside the client so a rejected session can be replaced with a
    /// fresh one pointed at the same service.
    base: String,
    ctx: egui::Context,
    /// Where the job in hand sends its answers. Read more than once per job,
    /// because a sign-in reports the URL it wants a browser pointed at before
    /// the job that triggered it finishes.
    answering: Option<Post>,
}

impl Worker {
    fn post(&self, event: Event) {
        if let Some(post) = &self.answering {
            post.send(event);
        }
        self.ctx.request_repaint();
    }

    fn run(&mut self, job: Job) {
        match job {
            Job::Account => {
                let answer = self.call(|client| client.me()).map(|me| {
                    // The one moment this application learns who it is signed
                    // in as. The session file is written before the answer
                    // exists, so it is completed here.
                    remember_account(&me.id, &me.display);
                    Account {
                        display: me.display,
                        capabilities: me.capabilities,
                        provider_linked: me.provider_linked,
                        git: me.git.map(|g| GitView { branch: g.branch, remote: g.remote }),
                    }
                });
                self.post(Event::Account(answer));
            }
            Job::LinkStart => match self.call(|client| client.link_start()) {
                Ok(start) => self.post(Event::Browser { url: start.login_url, handle: Some(start.handle) }),
                Err(fault) => self.post(Event::Linked(Err(fault))),
            },
            Job::Conversation(id) => {
                let answer = self.call(|client| client.conversation(&id));
                self.post(Event::Conversation(answer));
            }
            Job::SignOut => {
                // The token on disk goes with the session. Leaving it
                // would sign the user back in on the next run.
                if let Some(path) = token_path() {
                    let _ = std::fs::remove_file(path);
                }
                self.client = Client::new(&self.base);
            }
            Job::LinkPoll(handle) => {
                let answer = self.call(|client| client.link_poll(&handle));
                self.post(Event::Linked(answer));
            }
            Job::List(path) => {
                let entries = self.call(|client| client.list(&path));
                self.post(Event::Listed { path, entries });
            }
            Job::SharedWithMe => {
                let answer = self.call(|client| client.shared_with_me());
                self.post(Event::Shared(answer));
            }
            Job::Read { entry, into } => {
                let bytes = self.call(|client| client.read(&entry.id));
                self.post(Event::Read { entry, into, bytes });
            }
            // The etag the document was read at is the condition. A stored
            // copy that has moved on refuses the write, and that refusal is
            // what stops one person's save from erasing another's work.
            Job::Write { uid, entry, bytes } => {
                let answer = self.call(|client| client.write(&entry.id, &bytes, entry.etag.as_deref()));
                self.post(Event::Saved { uid, entry: answer });
            }
            Job::Create { uid, folder, name, bytes } => {
                let answer = self.call(|client| client.create(&folder, &name, &bytes));
                self.post(Event::Saved { uid, entry: answer });
            }
        }
    }

    /// Runs one call with a session behind it.
    ///
    /// A rejected token is worth exactly one retry: the saved one has been
    /// revoked or has expired, and a fresh sign-in is the only recovery. A
    /// second rejection is reported.
    fn call<T>(&mut self, work: impl Fn(&Client) -> craft_cloud_client::Result<T>) -> Result<T, Fault> {
        self.sign_in()?;
        match work(&self.client) {
            Ok(value) => Ok(value),
            Err(Error::Unauthenticated) => {
                log::info!("the saved cloud session was rejected; signing in again");
                forget_token();
                self.client = Client::new(&self.base);
                self.sign_in()?;
                work(&self.client).map_err(fault)
            }
            Err(e) => Err(fault(e)),
        }
    }

    /// Makes sure there is a session token, signing in through the browser
    /// when there is not.
    ///
    /// A desktop application cannot host a redirect worth trusting, so this is
    /// the device flow: the service hands out a URL, the user finishes in
    /// their browser, and this polls until a token can be collected. Whatever
    /// happens in that browser is the identity provider's business.
    fn sign_in(&mut self) -> Result<(), Fault> {
        if self.client.is_signed_in() {
            return Ok(());
        }
        let start = self.client.sign_in_start().map_err(fault)?;
        if let Err(e) = open_browser(&start.verify_url) {
            // Not fatal. The interface shows the URL as a link, so the user
            // can still follow it by hand.
            log::warn!("could not open a browser: {e}");
        }
        self.post(Event::Browser { url: start.verify_url.clone(), handle: None });
        let interval = Duration::from_secs(start.interval.clamp(1, 10));
        let deadline = Instant::now() + SIGN_IN_WAIT.min(Duration::from_secs(start.expires_in));
        while Instant::now() < deadline {
            std::thread::sleep(interval);
            match self.client.sign_in_poll(&start.device_code) {
                Ok(Some(token)) => {
                    save_token(&token);
                    self.client = Client::new(&self.base).with_token(token);
                    self.post(Event::Linked(Ok(true)));
                    return Ok(());
                }
                // The ordinary answer while the user is still in the browser.
                Ok(None) => {}
                Err(e) => return Err(fault(e)),
            }
        }
        Err(Fault::Refused("the sign-in was not completed in time".to_owned()))
    }
}

/// A client error in the shapes the interface acts on differently.
///
/// CloudCraft answers a refused conditional write with HTTP 409 and the code
/// `conflict` (see its `api::fail`). The client crate carries that through as
/// an ordinary refusal, so the code is read here and given its own shape. That
/// is the one failure a save must never retry and never force: it means
/// somebody else's work is in the stored copy.
fn fault(e: Error) -> Fault {
    match e {
        Error::NotLinked => Fault::NotLinked,
        Error::Unauthenticated => Fault::Unauthenticated,
        Error::Unreachable(detail) => Fault::Unreachable(detail),
        Error::Refused { code, .. } if code == "conflict" || code == "409" || code == "412" => Fault::Conflict,
        Error::Refused { code, detail } => Fault::Refused(if detail.trim().is_empty() { code } else { detail }),
        Error::Malformed(detail) => Fault::Refused(format!("the cloud service gave an answer SolidCraft could not read: {detail}")),
        // A request this application built wrongly. Nothing the user did,
        // and nothing they can do, so it is said plainly and logged.
        Error::Unsendable(detail) => {
            log::error!("cloud request could not be encoded: {detail}");
            Fault::Refused("SolidCraft could not build that request".to_owned())
        }
    }
}

/// A client for the share dialog, built fresh for each of its requests.
///
/// Separate from the Open and Save worker, so a large save on that worker's
/// queue never holds up a share autocomplete. They share what matters, the
/// service and the saved session, so a sign-in through either reaches both.
pub fn client() -> Option<Client> {
    let mut client = Client::new(base_url()?);
    if let Some(token) = load_token() {
        client = client.with_token(token);
    }
    Some(client)
}

/// Where the session token lives.
///
/// Beside the application's own settings. It is a session the user created in
/// their own browser, scoped to this machine, and revocable on its own.
fn token_path() -> Option<PathBuf> {
    // `settings_dir` answers `Result<Option<_>, String>`: the outer result is a
    // bad override, the inner option is a platform with no settings home. Both
    // mean there is nowhere to keep a session, which is the same answer here.
    // The idiom matches `graphics_path` in `main.rs`.
    let dir = crate::settings_dir().ok().flatten()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("collabcraft-session.json"))
}

fn load_token() -> Option<String> {
    let raw = std::fs::read(token_path()?).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    value.get("token")?.as_str().map(str::to_owned).filter(|t| !t.is_empty())
}

fn save_token(token: &str) {
    let service = base_url().unwrap_or_default();
    write_session(serde_json::json!({"token": token, "service": service}));
}

/// Record which account this session belongs to, keeping the token.
///
/// A token alone identifies nothing. The suite launcher keeps one keychain
/// entry per account and keys it on the service and the subject the provider
/// issued, so an application that signs in says who signed in. Called once the
/// service has answered, because until then this application does not know.
fn remember_account(subject: &str, display: &str) {
    let Some(token) = load_token() else { return };
    let service = base_url().unwrap_or_default();
    write_session(serde_json::json!({
        "token": token,
        "service": service,
        "subject": subject,
        "display": display,
    }));
}

fn write_session(value: serde_json::Value) {
    let Some(path) = token_path() else { return };
    let Ok(text) = serde_json::to_vec(&value) else { return };
    if let Err(e) = std::fs::write(&path, text) {
        log::warn!("could not save the cloud session: {e}");
        return;
    }
    restrict(&path);
}

fn forget_token() {
    if let Some(path) = token_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// On Windows the per-user AppData directory is already owner-scoped, and
/// setting an ACL would need a Win32 call this application makes nowhere else.
#[cfg(not(unix))]
fn restrict(_path: &Path) {}

pub fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    // `cmd` splits its command line on `&` before anything else sees it, and an
    // authorize URL is mostly `&`. Passed unquoted the browser receives the
    // query up to the first one, and the identity provider then reports that
    // the client identifier is missing, which by then it is.
    //
    // The quotes have to survive Rust's own argument escaping, so the command
    // line goes through raw. The empty pair is the window title, which `start`
    // would otherwise take the URL to be. A quote inside the URL would end the
    // quoting early, so it is percent-encoded first.
    let mut command = {
        use std::os::windows::process::CommandExt as _;
        let mut c = std::process::Command::new("cmd");
        c.raw_arg(format!("/C start \"\" \"{}\"", url.replace('"', "%22")));
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    command.spawn().map(|_| ())
}
