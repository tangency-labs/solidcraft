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
}

enum Job {
    Lookup(Receiver<Result<Resolved, Refusal>>),
    Download { item: Box<Resolved>, done: Receiver<Result<PathBuf, Refusal>> },
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

/// Download `item`'s file into `dir` and check its size and sha256; nothing is kept on failure.
pub(crate) fn fetch(download: &crate::update::install::HttpDownload, item: &Resolved, dir: &Path) -> Result<PathBuf, Refusal> {
    std::fs::create_dir_all(dir).map_err(|e| Refusal::Download(e.to_string()))?;
    let path = dir.join(download_name(item));
    let limit = item.file.size;
    let checked = download(&item.file.url, &path, &|done, _| done <= limit)
        .map_err(|e| Refusal::Download(e.to_string()))
        .and_then(|()| crate::update::install::sha256_file(&path).map_err(Refusal::Download))
        .and_then(|sum| if sum.eq_ignore_ascii_case(&item.file.sha256) { Ok(()) } else { Err(Refusal::Checksum) });
    match checked {
        Ok(()) => Ok(path),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

/// A folder of its own for one download, in the system's temporary folder.
fn download_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    std::env::temp_dir().join("solidcraft-workshop").join(std::format!("{}-{nanos}", std::process::id()))
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
            Ok(Ok(path)) => downloaded(app, &item, &path),
        },
    }
}

/// Hand the checked download to the path that opens or installs its kind.
fn downloaded(app: &mut SolveApp, item: &Resolved, path: &Path) {
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
        let dir = download_dir();
        let result = fetch(&download, &worker, &dir).and_then(|path| match worker.kind {
            Kind::Part => {
                let root = library_root.ok_or_else(|| Refusal::Download("no data folder".into()))?;
                let folder = library::install(&path, &root, &worker.id, &worker.version.to_string());
                let _ = std::fs::remove_file(&path);
                folder
            }
            _ => Ok(path),
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
    let Some(item) = app.workshop.ask.take() else { return };
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
}
