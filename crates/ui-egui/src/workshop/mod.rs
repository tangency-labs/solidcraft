//! Open in SolidCraft from the Workshop (workshop.solidcraft.io): the `solidcraft://workshop/…`
//! link a Workshop page hands the desktop app (docs/workshop-links.md).
//!
//! The link names an item only. The app looks it up in the Workshop's signed index, fetched
//! fresh from workshop.solidcraft.io and trusted only when its Ed25519 signature verifies with a
//! key built in (`index`). Delisted items and revoked versions are refused. Then it **always
//! asks**, showing the name, author, kind, tier, version, size, licence and, for a plugin, the
//! permissions it asks for; nothing downloads, opens or installs before the user says yes. Then
//! it downloads the one file the index pins and checks its sha256:
//!
//! - a model opens as File › Open opens a design (read-only, so Save asks where to keep it,
//!   never into the download folder), with the usual newer-format warning;
//! - a plugin goes to Install plugin…'s review, which asks for its permissions as for any `.zip`;
//! - a parts library unpacks into the data folder (`library`) and the Parts Library panel shows it.
//!
//! Lookups and downloads run on worker threads; the window never waits on the network.

pub mod index;
pub mod library;
pub mod link;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use solidcraft_ui_text::{format, tr};

use crate::{SolveApp, ui_kit};
use index::{Refusal, Resolved, Tier};
use link::{Kind, Link};

/// One HTTPS answer from workshop.solidcraft.io, with its signature headers.
#[derive(Clone, Debug, Default)]
pub struct Signed {
    pub body: Vec<u8>,
    /// `X-Workshop-Signature`.
    pub signature: Option<String>,
    /// `X-Workshop-Key-Id`.
    pub key_id: Option<String>,
}

/// HTTPS GET to workshop.solidcraft.io only, no redirects; the host refuses any other address.
pub type HttpGet = Arc<dyn Fn(&str) -> Result<Signed, String> + Send + Sync>;

/// The link being looked up or downloaded, and the question waiting for an answer.
#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    pub(crate) ask: Option<Resolved>,
    /// When the question first drew (`egui::InputState::time`), for [`KEY_DELAY`].
    asked_at: Option<f64>,
}

/// How long, in seconds, the question ignores keys that would answer it (Enter, a button's first
/// letter): a keypress meant for the window as the question appears must not say yes. A click
/// counts at once.
pub(crate) const KEY_DELAY: f64 = 1.0;

/// Drop this frame's keys that could answer the question while it is newer than [`KEY_DELAY`]:
/// every key and typed text but Escape, which only cancels.
fn hold_keys(ctx: &egui::Context, shown_for: f64) {
    if shown_for >= KEY_DELAY {
        return;
    }
    ctx.input_mut(|i| {
        i.events
            .retain(|e| matches!(e, egui::Event::Key { key: egui::Key::Escape, .. }) || !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
    });
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(KEY_DELAY - shown_for));
}

enum Job {
    Lookup(Receiver<Result<Resolved, Refusal>>),
    /// The file to open (with what it looked like when checked), or a library's unpacked folder.
    Download {
        item: Box<Resolved>,
        done: Receiver<Result<(PathBuf, Option<Seen>), Refusal>>,
    },
}

/// Whether this build opens Workshop links: the desktop app, with its network services.
pub fn available(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && app.services.workshop_get.is_some() && app.services.http_download.is_some()
}

/// Open a `solidcraft://` link: look it up, then ask. A link that doesn't parse is refused at once.
pub fn open_link(app: &mut SolveApp, ctx: &egui::Context, text: &str) {
    let Some(link) = link::parse(text.trim()) else {
        refuse(app, &Refusal::BadLink);
        return;
    };
    if !available(app) {
        return;
    }
    if app.workshop.job.is_some() || app.workshop.ask.is_some() {
        app.set_status(tr("workshop.busy").to_string(), true);
        return;
    }
    let Some(get) = app.services.workshop_get.clone() else { return };
    let seen = sequence_file();
    let (send, done) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("workshop-lookup".into()).spawn(move || {
        let app_version = crate::update::parse_version(crate::help::VERSION).unwrap_or_else(|| semver::Version::new(0, 0, 0));
        let _ = send.send(lookup(index::KEYS, &|url: &str| get(url), &link, seen.as_deref(), unix_now(), &app_version));
        ctx.request_repaint();
    });
    if spawned.is_ok() {
        app.workshop.job = Some(Job::Lookup(done));
        app.set_status(tr("workshop.looking_up").to_string(), false);
    }
}

/// Fetch the index, check it and find the item. `seen` keeps the highest sequence verified, so a
/// replayed older snapshot is refused next time too.
pub(crate) fn lookup(
    keys: &[(&str, &str)],
    get: &dyn Fn(&str) -> Result<Signed, String>,
    link: &Link,
    seen: Option<&Path>,
    now: i64,
    app_version: &semver::Version,
) -> Result<Resolved, Refusal> {
    let answer = get(index::INDEX_URL).map_err(Refusal::Unreachable)?;
    let index = index::verify_with(keys, &answer.body, answer.signature.as_deref(), answer.key_id.as_deref())?;
    let highest = seen.and_then(|path| std::fs::read_to_string(path).ok()).and_then(|s| s.trim().parse::<u64>().ok());
    index::check_fresh(&index, highest, now)?;
    if let Some(path) = seen
        && highest.is_none_or(|h| index.sequence > h)
    {
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = solidcraft_engine::io::safe_write::write_atomic(path, index.sequence.to_string().as_bytes(), false);
    }
    index::resolve(&index, link, app_version)
}

fn sequence_file() -> Option<PathBuf> {
    solidcraft_engine::recovery::data_dir().map(|d| d.join("workshop").join("index-sequence"))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The file name a download is kept under: made from the item's checked id, never the index's
/// file name.
fn download_name(item: &Resolved) -> String {
    let id = if link::segment(&item.slug) {
        item.slug.as_str()
    } else if link::segment(&item.id) {
        item.id.as_str()
    } else {
        "item"
    };
    let ext = if item.kind == Kind::Model { "sc" } else { "zip" };
    std::format!("{id}-{}.{ext}", item.version)
}

/// Download `item`'s file into `dir` (a fresh folder from [`download_dir`]) and check its size
/// and sha256 through one open handle; nothing is kept on failure. Gives the path and that handle,
/// rewound, so a parts library unpacks from the very file that was checked.
pub(crate) fn fetch(download: &crate::update::install::HttpDownload, item: &Resolved, dir: &Path) -> Result<(PathBuf, std::fs::File), Refusal> {
    let path = dir.join(download_name(item));
    let limit = item.file.size;
    let checked = download(&item.file.url, &path, &|done, _| done <= limit)
        .map_err(|e| Refusal::Download(e.to_string()))
        .and_then(|()| open_checked(&path, &item.file.sha256));
    match checked {
        Ok(file) => Ok((path, file)),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

/// Open the plain file at `path` (not a link) and check its sha256 through that handle; the file
/// is made read-only and the handle is rewound for its reader.
fn open_checked(path: &Path, sha256: &str) -> Result<std::fs::File, Refusal> {
    use std::io::{Read, Seek};
    let fail = |e: std::io::Error| Refusal::Download(e.to_string());
    if !std::fs::symlink_metadata(path).map_err(fail)?.file_type().is_file() {
        return Err(Refusal::Download(std::format!("{} is not a file", path.display())));
    }
    let mut file = std::fs::File::open(path).map_err(fail)?;
    if seen(&file.metadata().map_err(fail)?) != seen(&std::fs::symlink_metadata(path).map_err(fail)?) {
        return Err(Refusal::Checksum);
    }
    let mut hash = <sha2::Sha256 as sha2::Digest>::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(fail)?;
        if n == 0 {
            break;
        }
        sha2::Digest::update(&mut hash, buf.get(..n).unwrap_or_default());
    }
    let sum: String = sha2::Digest::finalize(hash).iter().map(|b| std::format!("{b:02x}")).collect();
    if !sum.eq_ignore_ascii_case(sha256) {
        return Err(Refusal::Checksum);
    }
    // Read-only for its owner too (unix; on Windows a read-only file couldn't be removed after).
    #[cfg(unix)]
    let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o400));
    file.rewind().map_err(fail)?;
    Ok(file)
}

/// What a checked file looked like: its size, modification time and (on unix) device and inode.
/// Another file put in its place, or the same one changed, looks different.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seen(u64, Option<std::time::SystemTime>, Option<(u64, u64)>);

fn seen(metadata: &std::fs::Metadata) -> Seen {
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    };
    #[cfg(not(unix))]
    let id = None;
    Seen(metadata.len(), metadata.modified().ok(), id)
}

/// Whether `path` is still the plain file that was checked, right before it is handed on.
pub(crate) fn still_checked(path: &Path, checked: &Seen) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file() && &seen(&m) == checked)
}

/// A folder of its own for one download: a new folder with a random name in the system's
/// temporary folder, readable only by this user (0700 on unix). It is made with one `mkdir`
/// that fails when the name exists, so nobody can have prepared it or a link in its place.
pub(crate) fn download_dir() -> Result<PathBuf, Refusal> {
    use std::hash::{BuildHasher, Hasher};
    let base = std::env::temp_dir();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut last = String::new();
    for attempt in 0..16u32 {
        // RandomState's keys come from the operating system's random source.
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(nanos);
        hasher.write_u32(std::process::id());
        hasher.write_u32(attempt);
        let dir = base.join(std::format!("solidcraft-workshop-{:016x}", hasher.finish()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = e.to_string(),
            Err(e) => return Err(Refusal::Download(e.to_string())),
        }
    }
    Err(Refusal::Download(last))
}

/// The question's text: what is asked, in plain lines, so it can be checked without a window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub title: String,
    pub ok: String,
    pub heading: String,
    pub summary: String,
    /// Label and value, one row each.
    pub rows: Vec<(String, String)>,
    /// The permissions a plugin asks for (empty for models and libraries).
    pub permissions: Vec<String>,
    /// Warnings shown above the buttons.
    pub warnings: Vec<String>,
    /// What happens after Yes.
    pub next: String,
    pub tone: Tone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Info,
    Warning,
}

pub(crate) fn tier_text(tier: Tier) -> &'static str {
    tr(match tier {
        Tier::Official => "workshop.tier_official",
        Tier::Community => "workshop.tier_community",
        Tier::Unverified => "workshop.tier_unverified",
    })
}

/// A size as people read it.
pub(crate) fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1024 {
        std::format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        std::format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        std::format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        std::format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

/// The question for `item`.
pub(crate) fn question(item: &Resolved) -> Question {
    let (title, ok, kind, next) = match item.kind {
        Kind::Model => ("workshop.title_model", "workshop.open", "workshop.kind_model", "workshop.next_model"),
        Kind::Plugin => ("workshop.title_plugin", "workshop.install", "workshop.kind_plugin", "workshop.next_plugin"),
        Kind::Part => ("workshop.title_part", "workshop.install", "workshop.kind_part", "workshop.next_part"),
    };
    let mut rows = vec![
        (tr("workshop.kind").to_string(), tr(kind).to_string()),
        (tr("workshop.author").to_string(), item.author.clone()),
        (tr("workshop.tier").to_string(), tier_text(item.tier).to_string()),
        (tr("workshop.version").to_string(), item.version.to_string()),
        (tr("workshop.size").to_string(), size_text(item.file.size)),
    ];
    if !item.licence.is_empty() {
        rows.push((tr("workshop.licence").to_string(), item.licence.clone()));
    }
    let permissions = match item.kind {
        Kind::Plugin if item.permissions.is_empty() => vec![tr("workshop.permissions_none").to_string()],
        Kind::Plugin => item.permissions.clone(),
        _ => Vec::new(),
    };
    let mut warnings = Vec::new();
    if item.tier == Tier::Unverified {
        warnings.push(tr("workshop.unverified_note").to_string());
    }
    if let Some(reason) = &item.warning {
        let reason = if reason.trim().is_empty() { tr("workshop.no_reason") } else { reason.as_str() };
        warnings.push(format("workshop.warning", &[("reason", reason)]));
    }
    Question {
        title: tr(title).to_string(),
        ok: tr(ok).to_string(),
        heading: std::format!("{} {}", item.name, item.version),
        summary: item.summary.clone(),
        rows,
        permissions,
        tone: if warnings.is_empty() { Tone::Info } else { Tone::Warning },
        warnings,
        next: tr(next).to_string(),
    }
}

/// What a refusal says.
pub(crate) fn refusal_text(refusal: &Refusal) -> String {
    let reason = |r: &Option<String>| r.clone().filter(|r| !r.trim().is_empty()).unwrap_or_else(|| tr("workshop.no_reason").to_string());
    match refusal {
        Refusal::BadLink => tr("workshop.bad_link").to_string(),
        Refusal::Unreachable(error) => format("workshop.unreachable", &[("error", error)]),
        Refusal::Untrusted => tr("workshop.untrusted").to_string(),
        Refusal::BadIndex(error) => format("workshop.bad_index", &[("error", error)]),
        Refusal::Replayed { .. } | Refusal::Expired => tr("workshop.stale_index").to_string(),
        Refusal::NotListed(link) => format("workshop.not_listed", &[("link", link)]),
        Refusal::Delisted(why) => format("workshop.delisted", &[("reason", &reason(why))]),
        Refusal::Revoked(why) => format("workshop.revoked", &[("reason", &reason(why))]),
        Refusal::NoSuchVersion(version) => format("workshop.no_version", &[("version", version)]),
        Refusal::NeedsNewerApp(version) => format("workshop.needs_newer", &[("version", version)]),
        Refusal::BadItem(error) => format("workshop.bad_item", &[("error", error)]),
        Refusal::Download(error) => format("workshop.download_failed", &[("error", error)]),
        Refusal::Checksum => tr("workshop.checksum").to_string(),
        Refusal::LibraryContents(error) => format("workshop.library_contents", &[("error", error)]),
    }
}

fn refuse(app: &mut SolveApp, refusal: &Refusal) {
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let text = refusal_text(refusal);
    log::warn!("workshop link refused: {text}");
    app.set_status(text, true);
}

/// Per frame: collect a finished lookup or download.
pub(crate) fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(job) = app.workshop.job.take() else { return };
    match job {
        Job::Lookup(done) => match done.try_recv() {
            Err(TryRecvError::Empty) => app.workshop.job = Some(Job::Lookup(done)),
            Err(TryRecvError::Disconnected) => refuse(app, &Refusal::Unreachable("the lookup stopped".into())),
            Ok(Err(refusal)) => refuse(app, &refusal),
            Ok(Ok(item)) => {
                app.set_status(String::new(), false);
                app.workshop.ask = Some(item);
                ctx.request_repaint();
            }
        },
        Job::Download { item, done } => match done.try_recv() {
            Err(TryRecvError::Empty) => app.workshop.job = Some(Job::Download { item, done }),
            Err(TryRecvError::Disconnected) => refuse(app, &Refusal::Download("the download stopped".into())),
            Ok(Err(refusal)) => refuse(app, &refusal),
            Ok(Ok((path, checked))) => downloaded(app, &item, &path, checked.as_ref()),
        },
    }
}

/// Hand the checked download to the path that opens or installs its kind, once it is sure to be
/// the file that was checked.
fn downloaded(app: &mut SolveApp, item: &Resolved, path: &Path, checked: Option<&Seen>) {
    if checked.is_some_and(|checked| !still_checked(path, checked)) {
        let _ = std::fs::remove_file(path);
        refuse(app, &Refusal::Checksum);
        return;
    }
    let shown = path.to_string_lossy().into_owned();
    app.set_status(String::new(), false);
    match item.kind {
        Kind::Model => app.open_chosen_file_read_only(&shown),
        Kind::Plugin => crate::plugin_install::review(app, shown),
        Kind::Part => {
            crate::parts_library::set_folder(app, &shown);
            let name = std::format!("{} {}", item.name, item.version);
            app.set_status(format("workshop.library_installed", &[("name", &name)]), false);
        }
    }
}

/// Yes: download on a worker, then (for a library) unpack it there too.
fn start_download(app: &mut SolveApp, ctx: &egui::Context, item: Resolved) {
    let Some(download) = app.services.http_download.clone() else { return };
    let library_root = library::root();
    let (send, done) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    let worker = item.clone();
    let spawned = std::thread::Builder::new().name("workshop-download".into()).spawn(move || {
        let result = download_dir().and_then(|dir| {
            let fetched = fetch(&download, &worker, &dir);
            let result = fetched.and_then(|(path, file)| match worker.kind {
                Kind::Part => {
                    let root = library_root.ok_or_else(|| Refusal::Download("no data folder".into()))?;
                    let folder = library::install_file(file, &root, &worker.id, &worker.version.to_string());
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_dir(&dir);
                    // Unpacked from the checked handle: nothing left to check before use.
                    folder.map(|folder| (folder, None))
                }
                _ => file.metadata().map(|m| (path, Some(seen(&m)))).map_err(|e| Refusal::Download(e.to_string())),
            });
            if result.is_err() {
                let _ = std::fs::remove_dir(&dir);
            }
            result
        });
        let _ = send.send(result);
        ctx.request_repaint();
    });
    if spawned.is_ok() {
        let name = std::format!("{} {}", item.name, item.version);
        app.set_status(format("workshop.downloading", &[("name", &name)]), false);
        app.workshop.job = Some(Job::Download { item: Box::new(item), done });
    }
}

/// The question, while an item waits for the answer.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(item) = app.workshop.ask.take() else {
        app.workshop.asked_at = None;
        return;
    };
    let now = ctx.input(|i| i.time);
    let asked_at = *app.workshop.asked_at.get_or_insert(now);
    hold_keys(ctx, now - asked_at);
    let _language = solidcraft_ui_text::scope(&app.preferences.language);
    let q = question(&item);
    let cancel = tr("workshop.cancel").to_string();
    let tone = match q.tone {
        Tone::Info => ui_kit::Tone::Info,
        Tone::Warning => ui_kit::Tone::Warning,
    };
    let (closed, action) = ui_kit::Confirm::new(egui::Id::unique("sc_workshop_open"), "import", &q.title)
        .tone(tone)
        .actions(&q.ok, &cancel, None)
        .show(ctx, true, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.55).show(ui, |ui| {
                ui_kit::text(ui, &q.heading, ui_kit::Text::Heading("workshop_name"));
                if !q.summary.is_empty() {
                    ui_kit::text(ui, &q.summary, ui_kit::Text::Report("workshop_summary", false));
                }
                for (i, (label, value)) in q.rows.iter().enumerate() {
                    ui.push_id(i, |ui| {
                        ui_kit::row(ui, label, |ui| ui_kit::text(ui, value, ui_kit::Text::Inline));
                    });
                }
                if !q.permissions.is_empty() {
                    ui_kit::text(ui, tr("workshop.permissions"), ui_kit::Text::Report("workshop_permissions", false));
                    for (i, line) in q.permissions.iter().enumerate() {
                        ui.push_id(("permission", i), |ui| ui_kit::text(ui, &std::format!("• {line}"), ui_kit::Text::Hint("workshop_permission")));
                    }
                }
                for (i, warning) in q.warnings.iter().enumerate() {
                    ui.push_id(("warning", i), |ui| ui_kit::status_line(ui, warning, ui_kit::StatusTone::Warning));
                }
                ui_kit::text(ui, &q.next, ui_kit::Text::Hint("workshop_next"));
            });
        });
    match action {
        ui_kit::Action::Ok => start_download(app, ctx, item),
        ui_kit::Action::Cancel => {}
        _ if closed => {}
        _ => app.workshop.ask = Some(item),
    }
    if app.workshop.ask.is_none() {
        app.workshop.asked_at = None;
    }
}
