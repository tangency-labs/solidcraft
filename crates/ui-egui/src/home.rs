//! The start page: New Design, Open, the recent designs and the built-in samples, each with a
//! picture of the design rendered from it. Shown on launch without a file and from the house
//! button or the first document tab; selecting a design returns to its view.

use std::collections::HashMap;

use egui::vec2;
use solidcraft_engine::Session;
use solidcraft_engine::render::{Camera, StandardView};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::ui_kit::{self, FileView, RecoveryMessage};

/// Recent designs kept.
pub const MAX_RECENT: usize = 12;
const THUMB: [usize; 2] = solidcraft_engine::io::THUMBNAIL_SIZE;
const SAMPLE_PICTURES: [&[u8]; 6] = [
    include_bytes!("../../../assets/samples/exports/0.png"),
    include_bytes!("../../../assets/samples/exports/1.png"),
    include_bytes!("../../../assets/samples/exports/2.png"),
    include_bytes!("../../../assets/samples/exports/3.png"),
    include_bytes!("../../../assets/samples/exports/4.png"),
    include_bytes!("../../../assets/samples/exports/5.png"),
];

/// What a background job makes: a key ("sample:N" or a path), the picture, and for samples the
/// built design (opened at once when picked).
type Made = (String, Option<egui::ColorImage>, Option<Session>);

#[derive(Default)]
pub struct HomeState {
    section: Section,
    pub open: bool,
    /// Recent design paths, newest first (kept in the preferences).
    pub recent: Vec<String>,
    pub opened_at: std::collections::BTreeMap<String, u64>,
    /// Designs the user removed from the list. Kept in the preferences so a host that rebuilds
    /// the recent list from its stored designs (the web build, see `sync_recents`) does not add
    /// them back. Opening or saving a design clears it from here.
    pub forgotten: std::collections::BTreeSet<String>,
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
    /// Keys whose picture could not be made (an empty or unreadable design). Terminal: the card
    /// shows a placeholder instead of spinning forever. Cleared when the design is opened or saved
    /// (see `add_recent`), which asks for the picture again.
    failed: std::collections::HashSet<String>,
    samples: HashMap<usize, Session>,
    /// Keys asked for and not made yet.
    queued: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: Option<std::sync::mpsc::Receiver<Made>>,
    /// Building a selected sample must not wait for a cold recent file.
    #[cfg(not(target_arch = "wasm32"))]
    sample_rx: Option<std::sync::mpsc::Receiver<Made>>,
    #[cfg(not(target_arch = "wasm32"))]
    remembering: Option<std::sync::mpsc::Receiver<Made>>,
    /// A sample picked before its design was ready.
    wanted: Option<usize>,
    /// Whether each recent file is there, found off the window thread (a stat of a slow or
    /// disconnected drive must not hold up the page); unknown counts as there.
    present: HashMap<String, bool>,
    #[cfg(not(target_arch = "wasm32"))]
    present_asked: std::collections::HashSet<String>,
    #[cfg(not(target_arch = "wasm32"))]
    present_rx: Vec<std::sync::mpsc::Receiver<(String, bool)>>,
}

impl HomeState {
    /// Wait for the background check of the recent files to answer (tests only).
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn wait_for_existence_checks(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.recent.iter().any(|p| !self.present.contains_key(p)) && std::time::Instant::now() < deadline {
            for rx in &self.present_rx {
                while let Ok((p, there)) = rx.try_recv() {
                    self.present.insert(p, there);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    pub fn ready_recents(&self) -> usize {
        self.recent.iter().filter(|p| self.thumbs.get(*p).is_some_and(Option::is_some)).count()
    }

    /// Whether a design's picture could not be made, so its card shows a placeholder rather than
    /// a spinner that would otherwise turn forever.
    fn thumb_failed(&self, key: &str) -> bool {
        self.failed.contains(key)
    }

    pub fn ready_samples(&self) -> usize {
        (0..solidcraft_engine::sample::samples().len()).filter(|i| self.thumbs.get(&format!("sample:{i}")).is_some_and(Option::is_some)).count()
    }

    pub fn show(&mut self, _rev: u64) {
        self.open = true;
        self.section = Section::Start;
        // Returning from another document cancels an earlier, unfinished sample selection.
        self.wanted = None;
    }

    /// Put a design at the top of the recent list.
    pub fn add_recent(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(MAX_RECENT);
        self.opened_at.insert(path.to_string(), web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        self.opened_at.retain(|p, _| self.recent.contains(p));
        self.thumbs.remove(path);
        // Opening or saving a design undoes an earlier removal from the list.
        self.forgotten.remove(path);
        // A fresh open or save earns another attempt at the picture.
        self.failed.remove(path);
    }

    /// Drop a design from the recent list and remember that the user did so, so a host that
    /// rebuilds the list from its stored designs (the web build) does not add it back.
    pub fn forget(&mut self, path: &str) {
        self.recent.retain(|p| p != path);
        self.opened_at.remove(path);
        self.thumbs.remove(path);
        self.forgotten.insert(path.to_string());
    }
}

/// A picture of a design: iso view, fitted.
pub fn thumbnail(s: &Session) -> Option<egui::ColorImage> {
    let mut cam = Camera::default();
    cam.set_view(StandardView::Iso);
    let b = solidcraft_engine::view::bounds(s);
    if b.is_empty() {
        return None;
    }
    cam.fit(&b);
    // Fill the card: the fit leaves room for a whole window's margins.
    cam.distance *= 0.7;
    let scene = solidcraft_engine::view::scene(s, &cam);
    // Drawn on the GPU by the renderer the app installs; none (a browser): no picture.
    let picture = solidcraft_engine::capture::rgba(&scene, &cam, THUMB[0] as u32, THUMB[1] as u32).ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(THUMB, &picture.rgba))
}

/// File identity, not just its path: saving or replacing a design invalidates its picture.
#[cfg(not(target_arch = "wasm32"))]
fn cache_path(key: &str) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(key).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    meta.len().hash(&mut h);
    meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.hash(&mut h);
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    Some(root.join("SolidCraft").join("thumbnails").join(format!("{:016x}.png", h.finish())))
}

#[cfg(not(target_arch = "wasm32"))]
fn cached(key: &str) -> Option<egui::ColorImage> {
    let path = cache_path(key)?;
    if std::fs::metadata(&path).ok()?.len() > 512 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    decode_thumbnail(bytes)
}

fn decode_thumbnail(bytes: Vec<u8>) -> Option<egui::ColorImage> {
    if bytes.len() > 512 * 1024 {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(THUMB[0] as u32);
    limits.max_image_height = Some(THUMB[1] as u32);
    limits.max_alloc = Some((THUMB[0] * THUMB[1] * 8) as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?.to_rgba8();
    (image.width() as usize == THUMB[0] && image.height() as usize == THUMB[1])
        .then(|| egui::ColorImage::from_rgba_unmultiplied(THUMB, image.as_raw()))
}

/// Imported STEP/mesh files open as unsaved designs, with no session path. Cache their
/// pictures under the source file identity too, so Home need not reimport them on each launch.
pub(crate) fn remember_file(s: &Session, key: &str) -> Option<egui::ColorImage> {
    #[cfg(not(target_arch = "wasm32"))]
    let identity = cache_path(key);
    remember_at(
        s,
        key,
        #[cfg(not(target_arch = "wasm32"))]
        identity,
    )
}

fn remember_at(s: &Session, key: &str, #[cfg(not(target_arch = "wasm32"))] identity: Option<std::path::PathBuf>) -> Option<egui::ColorImage> {
    #[cfg(target_arch = "wasm32")]
    let _ = key;
    let img = thumbnail(s)?;
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = identity.filter(|p| cache_path(key).as_ref() == Some(p)) {
        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        if let Some(bytes) = solidcraft_engine::render::png(THUMB[0], THUMB[1], rgba)
            && let Some(dir) = path.parent()
            && std::fs::create_dir_all(dir).is_ok()
        {
            let _ = solidcraft_engine::io::write_atomic(&path, &bytes, false);
        }
    }
    Some(img)
}

/// Waits up to `within` for the threads drawing Home pictures, and drops what they made: the
/// window is going, and a picture still on the GPU as the process exits can crash the driver.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn finish_pictures(h: &mut HomeState, within: std::time::Duration) {
    let end = std::time::Instant::now() + within;
    for rx in [h.rx.take(), h.sample_rx.take(), h.remembering.take()].into_iter().flatten() {
        // Each thread sends what it made and ends; its channel closes when it has.
        while let Some(left) = end.checked_duration_since(std::time::Instant::now()) {
            if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(left) {
                break;
            }
        }
    }
}

/// A Home picture is being drawn off the UI thread.
#[cfg(test)]
pub(crate) fn remembering(h: &HomeState) -> bool {
    h.remembering.is_some()
}

pub(crate) fn remember_session(h: &mut HomeState, s: &Session) {
    if let Some(path) = s.path.as_deref() {
        remember_source(h, s, path);
    }
}

/// Draw an opened file's picture after adoption, including imports with no save path.
pub(crate) fn remember_source(h: &mut HomeState, s: &Session, key: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let identity = cache_path(key);
        let snapshot = s.scratch();
        let source = key.to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        if std::thread::Builder::new()
            .name("saved-thumbnail".into())
            .spawn(move || {
                let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| remember_at(&snapshot, &source, identity))).ok().flatten();
                let _ = tx.send((source, img, None));
            })
            .is_ok()
        {
            h.remembering = Some(rx);
            h.queued.retain(|k| k != key);
            h.thumbs.insert(key.to_owned(), None);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (h, s, key);
    }
}

pub(crate) fn install(h: &mut HomeState, ctx: &egui::Context, key: &str, img: Option<egui::ColorImage>) {
    match img {
        Some(i) => {
            let tex = ctx.load_texture(format!("thumb:{key}"), i, egui::TextureOptions::LINEAR);
            h.thumbs.insert(key.to_string(), Some(tex));
            h.failed.remove(key);
        }
        // The job finished with no picture: an empty design, or one that could not be read. Mark
        // it so the card stops spinning and shows a placeholder (#4655).
        None => {
            h.thumbs.insert(key.to_string(), None);
            h.failed.insert(key.to_string());
        }
    }
}

/// Read a disposable picture without evaluating, importing or meshing a model.
fn picture(key: &str) -> Option<egui::ColorImage> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(img) = cached(key) {
        return Some(img);
    }
    solidcraft_engine::io::read_design_thumbnail_file(key).and_then(decode_thumbnail)
}

/// A sample's shipped picture. Its model is built only when the sample is selected.
fn sample_picture(i: usize) -> Option<egui::ColorImage> {
    decode_thumbnail(SAMPLE_PICTURES.get(i)?.to_vec())
}

/// Build what a key stands for (runs off the UI thread where threads exist).
fn make(key: &str) -> Made {
    let sample = key.strip_prefix("sample:").and_then(|i| i.parse::<usize>().ok());
    if sample.is_none()
        && let Some(img) = picture(key)
    {
        return (key.to_string(), Some(img), None);
    }
    let mut s = Session::default();
    let built = match sample {
        Some(i) => solidcraft_engine::sample::samples().get(i).is_some_and(|smp| {
            let ok = s.run_script(&(smp.script)()).is_ok();
            s.doc_mut().name = smp.name.to_string();
            s.undo.clear();
            s.mark_saved();
            ok
        }),
        None => s.execute("doc.open", &serde_json::json!({ "path": key })).is_ok(),
    };
    if !built {
        return (key.to_string(), None, None);
    }
    if sample.is_some() {
        // Keep the first viewport frame warm even though the preview needs no new render.
        s.world_state().warm_meshes(|| true);
    }
    let img = match sample {
        Some(i) => sample_picture(i),
        None => remember_file(&s, key),
    };
    (key.to_string(), img, sample.map(|_| s))
}

/// Send every readable picture before starting any cold model work. A portable picture in a
/// later file must not wait for an earlier STEP import or legacy design rebuild.
#[cfg(not(target_arch = "wasm32"))]
fn read_pictures(keys: Vec<String>, tx: &std::sync::mpsc::Sender<Made>) -> Option<Vec<String>> {
    let mut cold = Vec::new();
    for key in keys {
        if let Some(i) = key.strip_prefix("samplepic:").and_then(|i| i.parse::<usize>().ok()) {
            tx.send((format!("sample:{i}"), sample_picture(i), None)).ok()?;
            continue;
        }
        match std::panic::catch_unwind(|| picture(&key)).ok().flatten() {
            Some(img) => tx.send((key, Some(img), None)).ok()?,
            None => cold.push(key),
        }
    }
    Some(cold)
}

#[cfg(not(target_arch = "wasm32"))]
fn start_jobs(h: &mut HomeState) {
    if h.rx.is_some() || h.queued.is_empty() {
        return;
    }
    let mut keys = std::mem::take(&mut h.queued);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("thumbnails".into()).spawn(move || {
        // Prefer local PNGs; even the metadata checks belong off the window thread.
        keys.sort_by_key(|k| !cache_path(k).is_some_and(|p| p.exists()));
        let Some(cold) = read_pictures(keys, &tx) else { return };
        for k in cold {
            let made = std::panic::catch_unwind(|| make(&k)).unwrap_or((k, None, None));
            if tx.send(made).is_err() {
                return;
            }
        }
    });
    if spawned.is_ok() {
        h.rx = Some(rx);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn start_sample(h: &mut HomeState) {
    let Some(i) = h.wanted.filter(|i| !h.samples.contains_key(i)) else { return };
    if h.sample_rx.is_some() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if std::thread::Builder::new()
        .name("selected-sample".into())
        .spawn(move || {
            let key = format!("sample:{i}");
            let made = std::panic::catch_unwind(|| make(&key)).unwrap_or((key, None, None));
            let _ = tx.send(made);
        })
        .is_ok()
    {
        h.sample_rx = Some(rx);
    }
}

/// Take finished pictures and designs (one job per frame on the web, where there are no
/// threads).
fn poll(app: &mut SolveApp, ctx: &egui::Context) {
    let mut done: Vec<Made> = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    let mut remembered = None;
    #[cfg(not(target_arch = "wasm32"))]
    {
        start_sample(&mut app.home);
        if let Some(rx) = &app.home.sample_rx {
            match rx.try_recv() {
                Ok(m) => {
                    done.push(m);
                    app.home.sample_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => app.home.sample_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(16)),
            }
        }
        if let Some(rx) = &app.home.remembering {
            match rx.try_recv() {
                Ok(m) => {
                    remembered = Some(m);
                    app.home.remembering = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => app.home.remembering = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
            }
        }
        start_jobs(&mut app.home);
        if let Some(rx) = &app.home.rx {
            loop {
                match rx.try_recv() {
                    Ok(m) => {
                        // A cold job started before a save/open cannot overwrite its fresh picture.
                        if m.0.starts_with("sample:") || !app.home.thumbs.get(&m.0).is_some_and(Option::is_some) {
                            done.push(m);
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        app.home.rx = None;
                        break;
                    }
                }
            }
        }
        if let Some(m) = remembered {
            done.push(m);
        }
        if app.home.rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(i) = app.home.wanted {
        done.push(make(&format!("sample:{i}")));
        ctx.request_repaint();
    } else if !app.home.queued.is_empty() {
        let key = app.home.queued.remove(0);
        done.push(make(&key));
        ctx.request_repaint();
    }
    for (key, img, session) in done {
        // A picture sent ahead of the build has no session either; only a build with nothing failed.
        if session.is_none() && img.is_none() && key.starts_with("sample:") && app.home.wanted.is_some_and(|w| key == format!("sample:{w}")) {
            app.home.wanted = None;
            app.set_status(format!("the sample could not be built ({key})"), true);
        }
        // A failed sample build keeps its shipped preview, so never overwrite a sample that
        // already has a picture with an empty result. A recent with no picture, though, must
        // settle into a terminal "failed" state so its card stops spinning (#4655); `install`
        // records that.
        let is_sample = key.starts_with("sample:");
        let already_ready = app.home.thumbs.get(&key).is_some_and(Option::is_some);
        if img.is_some() || (!is_sample && !already_ready) || !app.home.thumbs.contains_key(&key) {
            install(&mut app.home, ctx, &key, img);
        }
        if let (Some(s), Some(i)) = (session, key.strip_prefix("sample:").and_then(|i| i.parse::<usize>().ok())) {
            app.home.samples.insert(i, s);
        }
    }
    if let Some(i) = app.home.wanted
        && app.home.samples.contains_key(&i)
    {
        app.home.wanted = None;
        open_sample(app, i);
    } else if app.home.wanted.is_some() {
        // A selection can change while another sample finishes; schedule the next build.
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Ask for a key's picture (once: an empty entry marks it as asked for).
fn want(h: &mut HomeState, key: &str) {
    if !h.thumbs.contains_key(key) {
        h.queued.push(key.to_string());
        h.thumbs.insert(key.to_string(), None);
    }
}

/// Open a built-in sample (a copy of the design made for its picture).
pub fn open_sample(app: &mut SolveApp, i: usize) {
    match app.home.samples.get(&i) {
        Some(s) => {
            let mut copy = s.scratch();
            copy.mark_saved();
            crate::documents::adopt(app, copy);
            app.home.open = false;
        }
        None if i < solidcraft_engine::sample::samples().len() => {
            app.home.open = true;
            app.home.wanted = Some(i);
        }
        None => {}
    }
}

/// Is this recent design there to open: in memory, or something the host can bring back?
pub fn available(app: &SolveApp, path: &str) -> bool {
    app.home.present.get(path).copied().unwrap_or(true) || app.services.can_restore.as_ref().is_some_and(|can| can(path))
}

pub fn open_recent(app: &mut SolveApp, path: &str) {
    if !solidcraft_engine::io::vfs::exists(path) {
        if let Some(restore) = app.services.restore_file.as_ref().filter(|_| available(app, path)) {
            restore(path);
            return;
        }
        app.set_status(format!("{path} is no longer there"), true);
        return;
    }
    app.home.open = false;
    app.open_path_async(path);
}

/// Find out whether the recent files are there: on a thread, so the page never waits on a drive.
fn ask_present(h: &mut HomeState, ctx: &egui::Context, recent: &[String]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let new: Vec<String> = recent.iter().filter(|p| h.present_asked.insert((*p).clone())).cloned().collect();
        if !new.is_empty() {
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ctx.clone();
            let spawned = std::thread::Builder::new().name("recent-files".into()).spawn(move || {
                for p in new {
                    let there = solidcraft_engine::io::vfs::exists(&p);
                    if tx.send((p, there)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
            if spawned.is_ok() {
                h.present_rx.push(rx);
            }
        }
        for rx in &h.present_rx {
            while let Ok((p, there)) = rx.try_recv() {
                h.present.insert(p, there);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    for p in recent {
        h.present.insert(p.clone(), solidcraft_engine::io::vfs::exists(p));
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Section {
    #[default]
    Start,
    Recovery,
    Export,
}

enum FileAction {
    Back,
    Section(Section),
    New(&'static str),
    Open,
    Import,
    Save,
    SaveAs,
    Export(&'static str),
    Mesh,
    ViewDxf,
    Properties,
    Settings,
    Shortcuts,
    Crashes,
    About,
    Recovery(solidcraft_engine::recovery::Entry, bool),
}

/// File view replaces the workspace while keeping the open design and its history intact.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    poll(app, ui.ctx());
    let file_menu_open = ui.ctx().data(|data| data.get_temp::<bool>(egui::Id::new("sc_file_menu"))).unwrap_or(false);
    let interactive = !app.file_picker_busy() && !app.file_load.busy() && !file_menu_open;
    // Help windows draw before Home; remember their prior visibility so one Escape closes only the window.
    let had_help_window = ui.ctx().memory(|memory| {
        ["sc_about", "sc_crash_reports"]
            .iter()
            .any(|id| memory.areas().visible_last_frame(&egui::LayerId::new(egui::Order::Middle, egui::Id::new(id))))
    });

    if interactive
        && !had_help_window
        && !app.prefs_window.open
        && !app.help.about
        && !app.crashes.open
        && !app.help.menu
        && !egui::Popup::is_any_open(ui.ctx())
        && app.issue_report.draft.is_none()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        app.home.open = false;
        return;
    }
    if interactive
        && !had_help_window
        && !app.prefs_window.open
        && !app.help.about
        && !app.crashes.open
        && !egui::Popup::is_any_open(ui.ctx())
        && app.issue_report.draft.is_none()
        && !crate::documents::active_is_blank(app)
    {
        let save_as = egui::Modifiers { command: true, shift: true, ..Default::default() };
        let action = if ui.input_mut(|input| input.consume_key(save_as, egui::Key::S)) {
            Some(FileAction::SaveAs)
        } else if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            Some(FileAction::Save)
        } else {
            None
        };
        if let Some(action) = action {
            dispatch(app, ui.ctx(), action);
            return;
        }
    }
    let t = Tokens::get();
    let samples = solidcraft_engine::sample::samples();
    for i in 0..samples.len() {
        let key = format!("sample:{i}");
        if !app.home.thumbs.contains_key(&key) {
            // Decoded on the thumbnail thread where there is one; the card shows its placeholder
            // until the picture arrives.
            #[cfg(not(target_arch = "wasm32"))]
            {
                app.home.queued.push(format!("samplepic:{i}"));
                app.home.thumbs.insert(key, None);
            }
            #[cfg(target_arch = "wasm32")]
            install(&mut app.home, ui.ctx(), &key, sample_picture(i));
        }
    }
    let recent = app.home.recent.clone();
    ask_present(&mut app.home, ui.ctx(), &recent);
    for path in &recent {
        if app.home.present.get(path).copied().unwrap_or(true) {
            want(&mut app.home, path);
        }
    }
    let mut action = None;
    FileView::page(ui, |ui| {
        ui.add_enabled_ui(interactive, |ui| {
            let full = ui.available_rect_before_wrap();
            crate::scenario::publish_rect("home:page", full);
            let narrow = full.width() < t.file_nav_width + FileView::tile_min_width() + t.file_content_inset_x * 2.0;
            if narrow {
                FileView::navigation(ui, |ui| {
                    ui_kit::dropdown_menu(ui, "home_navigation", app.tr("ui.start_page"), |ui| {
                        ui.set_min_width(t.file_nav_width);
                        FileView::navigation(ui, |ui| {
                            navigation(app, ui, &mut action);
                            if action.is_some() {
                                ui.close();
                            }
                        });
                    });
                });
                content(app, ui, &mut action, true);
            } else {
                let rail = egui::Rect::from_min_size(full.min, vec2(t.file_nav_width, full.height()));
                FileView::rail(ui, rail, |ui| {
                    egui::ScrollArea::vertical().id_salt("home_navigation").auto_shrink([false, false]).show(ui, |ui| {
                        navigation(app, ui, &mut action);
                    });
                });
                let body = egui::Rect::from_min_max(egui::pos2(rail.right(), full.top()), full.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| content(app, ui, &mut action, false));
            }
        });
    });
    if let Some(action) = action {
        dispatch(app, ui.ctx(), action);
    }
}

fn navigation(app: &SolveApp, ui: &mut egui::Ui, action: &mut Option<FileAction>) {
    let busy = app.file_picker_busy();
    let design = !crate::documents::active_is_blank(app);
    let response = FileView::back_to(ui, "home:back", app.tr("home.back"), &app.session.doc.name, "Esc");
    publish("home:back", &response);
    if response.clicked() {
        *action = Some(FileAction::Back);
    }
    let primary = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
    let shift = if cfg!(target_os = "macos") { "⇧" } else { "Shift+" };
    let recover = if app.recoverable.is_empty() {
        app.tr_label("Recover").to_string()
    } else {
        format!("{} ({})", app.tr_label("Recover"), app.recoverable.len())
    };
    let nav = [
        ("home:start", app.tr_label("Start"), String::new(), Some(Section::Start), true, FileAction::Section(Section::Start)),
        ("home:new", app.tr_label("New"), format!("{primary}N"), None, true, FileAction::New("preferred")),
        ("home:open", app.tr("ui.open"), format!("{primary}O"), None, true, FileAction::Open),
        ("home:recover", recover.as_str(), String::new(), Some(Section::Recovery), true, FileAction::Section(Section::Recovery)),
        ("home:save", app.tr("ui.save"), format!("{primary}S"), None, design, FileAction::Save),
        ("home:save_as", app.tr("ui.save_as"), format!("{primary}{shift}S"), None, design, FileAction::SaveAs),
        ("home:export", app.tr("home.export"), String::new(), Some(Section::Export), design, FileAction::Section(Section::Export)),
        ("home:properties", app.tr("home.properties"), String::new(), None, design, FileAction::Properties),
        ("home:settings", app.tr("home.settings"), format!("{primary},"), None, true, FileAction::Settings),
        ("home:shortcuts", app.tr("home.shortcuts"), String::new(), None, true, FileAction::Shortcuts),
        ("home:crashes", app.tr("home.crashes"), String::new(), None, true, FileAction::Crashes),
        ("home:about", app.tr_label("About SolidCraft"), String::new(), None, true, FileAction::About),
    ];
    for (index, (id, label, key, section, enabled, picked)) in nav.into_iter().enumerate() {
        if index == 4 || index == 8 {
            FileView::separator(ui);
        }
        let response = FileView::nav_row(ui, id, label, &key, section == Some(app.home.section), enabled && !busy);
        publish(id, &response);
        if response.clicked() {
            *action = Some(picked);
        }
    }
}

fn publish(id: &str, response: &egui::Response) {
    crate::scenario::publish_handle(id, response.rect.center());
    crate::scenario::publish_rect(id, response.rect);
}

fn content(app: &mut SolveApp, ui: &mut egui::Ui, action: &mut Option<FileAction>, narrow: bool) {
    let t = Tokens::get();
    FileView::content(ui, narrow, |ui| {
        egui::ScrollArea::vertical().id_salt("home_content").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let (title, hint) = match app.home.section {
                Section::Start => (app.tr_label("Start"), app.tr("home.start_hint")),
                Section::Recovery => (app.tr_label("Recover"), app.tr("home.recovery_hint")),
                Section::Export => (app.tr("home.export"), app.tr("home.export_hint")),
            };
            FileView::title(ui, title);
            ui.add_space(t.space_6);
            FileView::subtitle(ui, hint);
            ui.add_space(t.file_section_gap);
            if let Some((message, _, true)) = &app.status {
                ui_kit::text(ui, message, ui_kit::Text::Error("home_status"));
            }
            match app.home.section {
                Section::Start => start_content(app, ui, action),
                Section::Recovery => recovery_content(app, ui, action),
                Section::Export => export_content(app, ui, action),
            }
        });
    });
}

fn start_content(app: &mut SolveApp, ui: &mut egui::Ui, action: &mut Option<FileAction>) {
    let t = Tokens::get();
    if let Some(entry) = app.recoverable.first().cloned() {
        let title = crate::i18n::format("home.recovery_count", &[("count", &app.recoverable.len().to_string())]);
        let filename = entry
            .path
            .as_deref()
            .and_then(|path| std::path::Path::new(path).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.name.clone());
        let metadata = format!(" · {}", crate::recovery_ui::ago(entry.saved_at));
        let (recover, discard) = FileView::recovery_banner(
            ui,
            "home:recovery",
            RecoveryMessage { title: &title, before_filename: " · ", filename: &filename, after_filename: &metadata },
            app.tr_label("Recover"),
            (app.recoverable.len() == 1).then(|| app.tr_label("Discard")),
        );
        publish("home:recovery:recover", &recover);
        if recover.clicked() {
            *action =
                Some(if app.recoverable.len() == 1 { FileAction::Recovery(entry.clone(), true) } else { FileAction::Section(Section::Recovery) });
        }
        if let Some(discard) = discard {
            publish("home:recovery:discard", &discard);
            if discard.clicked() {
                *action = Some(FileAction::Recovery(entry, false));
            }
        }
        ui.add_space(t.file_section_gap);
    }
    let shortcut = if cfg!(target_os = "macos") { "⌘O" } else { "Ctrl+O" };
    let open_hint = crate::i18n::format("home.open_hint", &[("shortcut", shortcut)]);
    let tiles = [
        ("home:new_mm", "new", app.tr("home.new_mm"), app.tr("home.new_mm_hint"), true),
        ("home:new_inches", "new", app.tr("home.new_inches"), app.tr("home.new_inches_hint"), false),
        ("home:open_tile", "open", app.tr("home.open"), open_hint.as_str(), false),
        ("home:import", "import", app.tr("home.import"), app.tr("home.import_hint"), false),
    ];
    FileView::section(ui, app.tr_label("New"), t.space_12, |ui| {
        FileView::grid(ui, "home_new", FileView::tile_min_width(), tiles.len(), |ui, index, width| {
            let Some((id, icon, title, hint, primary)) = tiles.get(index) else { return };
            let response = FileView::action_tile(ui, id, width, icon, title, hint, *primary);
            publish(id, &response);
            if response.clicked() {
                *action = Some(match index {
                    0 => FileAction::New("mm"),
                    1 => FileAction::New("in"),
                    2 => FileAction::Open,
                    _ => FileAction::Import,
                });
            }
        });
    });
    ui.add_space(t.file_section_gap);
    FileView::section(ui, app.tr("home.recent"), t.space_10, |ui| {
        if app.home.recent.is_empty() {
            FileView::detail(ui, app.tr("home.recent_empty"));
        }
        if !app.home.recent.is_empty() {
            FileView::recent_list(ui, |ui| {
                for (index, path) in app.home.recent.clone().iter().enumerate() {
                    let name = std::path::Path::new(path).file_stem().map(|name| name.to_string_lossy()).unwrap_or_else(|| path.as_str().into());
                    let missing = !available(app, path);
                    let when = if app.session.path.as_ref() == Some(path) { app.tr("home.open_now").to_string() } else { recent_time(app, path) };
                    let shown_path = if missing {
                        format!("{} · {}", app.tr("home.missing"), friendly_path(path))
                    } else if path.starts_with("/disk/") {
                        app.tr_label("On your computer").to_string()
                    } else if path.starts_with("/opfs/") {
                        app.tr("home.in_this_browser").to_string()
                    } else {
                        friendly_path(path)
                    };
                    let response = FileView::recent_row(
                        ui,
                        &format!("home:recent:{index}"),
                        ui_kit::RecentRow {
                            title: &name,
                            path: &shown_path,
                            when: &when,
                            workspace: app.tr("home.design_workspace"),
                            thumbnail: texture(app.home.thumbs.get(path).and_then(Option::as_ref)),
                            // A design whose picture could not be made shows a dimmed placeholder,
                            // distinct from the one shown while a picture is still loading (#4655).
                            thumbnail_failed: app.home.thumb_failed(path),
                        },
                    );
                    publish(&format!("home:recent:{index}"), &response);
                    ui_kit::Menu::context(&response, |ui| {
                        if crate::context_menu::button(ui, app.tr_label("Remove from list"), "delete").clicked() {
                            app.home.forget(path);
                            ui.close();
                        }
                        if !cfg!(target_arch = "wasm32") && crate::context_menu::button(ui, app.tr_label("Show in folder"), "folder").clicked() {
                            if let Some(show) = &app.services.show_in_folder {
                                show(path);
                            }
                            ui.close();
                        }
                    });
                    if response.clicked() {
                        open_recent(app, path);
                    }
                }
            });
        }
    });
    ui.add_space(t.file_section_gap);
    FileView::section(ui, app.tr("home.samples"), t.space_10, |ui| {
        let samples: Vec<_> = solidcraft_engine::sample::samples()
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let hint = if app.home.wanted == Some(index) { app.tr("home.opening") } else { app.tr_label(sample.about) };
                ui_kit::SampleCard {
                    title: app.tr_label(sample.name),
                    description: hint,
                    icon: "box",
                    thumbnail: texture(app.home.thumbs.get(&format!("sample:{index}")).and_then(Option::as_ref)),
                }
            })
            .collect();
        for (index, response) in FileView::sample_grid(ui, "home:sample", &samples).into_iter().enumerate() {
            publish(&format!("home:sample:{index}"), &response);
            if response.clicked() {
                open_sample(app, index);
            }
        }
    });
}

fn texture(texture: Option<&egui::TextureHandle>) -> Option<(egui::TextureId, egui::Vec2)> {
    texture.map(|texture| (texture.id(), texture.size_vec2()))
}

/// Abbreviate only an actual home-directory ancestor; the source path is kept for actions.
pub(crate) fn friendly_path(path: &str) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    abbreviated_path(path, home.as_deref().map(std::path::Path::new))
}

fn abbreviated_path(path: &str, home: Option<&std::path::Path>) -> String {
    if let Some(relative) = home.filter(|home| !home.as_os_str().is_empty()).and_then(|home| std::path::Path::new(path).strip_prefix(home).ok()) {
        if relative.as_os_str().is_empty() { "~".into() } else { format!("~/{}", relative.to_string_lossy()) }
    } else {
        path.to_owned()
    }
}

fn recent_time(app: &SolveApp, path: &str) -> String {
    let Some(time) = app.home.opened_at.get(path) else { return String::new() };
    let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(*time);
    recent_time_at(app, *time, now)
}

fn recent_time_at(app: &SolveApp, time: u64, now: u64) -> String {
    let seconds = now.saturating_sub(time);
    if seconds < 60 {
        return app.tr("home.opened_just_now").to_string();
    }
    let (key, count) = if seconds < 3600 {
        ("home.opened_minutes_ago", seconds / 60)
    } else if seconds < 86400 {
        ("home.opened_hours_ago", seconds / 3600)
    } else if seconds < 172800 {
        return app.tr("home.yesterday").to_string();
    } else {
        let date = solidcraft_engine::crash::utc(time);
        let date = date.split_once('T').map_or(date.as_str(), |(date, _)| date);
        let mut parts = date.rsplitn(3, '-');
        let day = parts.next().unwrap_or("").trim_start_matches('0');
        let month = parts.next().unwrap_or("");
        return crate::i18n::format("home.short_date", &[("day", day), ("month", app.tr(&format!("home.month_{month}")))]);
    };
    crate::i18n::format(key, &[("count", &count.to_string())])
}

fn recovery_content(app: &SolveApp, ui: &mut egui::Ui, action: &mut Option<FileAction>) {
    if app.recoverable.is_empty() {
        ui_kit::text(ui, app.tr("home.recovery_empty"), ui_kit::Text::Inline);
    }
    for (index, entry) in app.recoverable.iter().take(20).enumerate() {
        let detail = crate::i18n::format(
            "recovery.detail",
            &[
                ("file", entry.path.as_deref().unwrap_or(app.tr("recovery.never_saved"))),
                ("features", &entry.features.to_string()),
                ("when", &crate::recovery_ui::ago(entry.saved_at)),
            ],
        );
        let (recover, discard) =
            FileView::recovery_review(ui, &format!("home:recovery:{index}"), &entry.name, &detail, app.tr_label("Recover"), app.tr_label("Discard"));
        for (id, response, recover) in [("recover", Some(recover), true), ("discard", discard, false)] {
            let Some(response) = response else { continue };
            publish(&format!("home:recovery:{index}:{id}"), &response);
            if response.clicked() {
                *action = Some(FileAction::Recovery(entry.clone(), recover));
            }
        }
        ui.add_space(Tokens::get().space_12);
    }
}

fn export_content(app: &SolveApp, ui: &mut egui::Ui, action: &mut Option<FileAction>) {
    let formats = [
        ("step", "STEP", Some("home.export_step_hint")),
        ("mesh", "Mesh", Some("home.export_mesh_hint")),
        ("view_dxf", "DXF", None),
        ("obj", "OBJ", None),
        ("urdf", "URDF", None),
        ("json", "JSON", Some("home.export_json_hint")),
    ];
    FileView::grid(ui, "home_exports", FileView::tile_min_width(), formats.len(), |ui, index, width| {
        let Some((id, format, hint)) = formats.get(index) else { return };
        let label = if *id == "view_dxf" {
            app.tr_label("Export Current View as DXF…").to_string()
        } else {
            format!("{} {}…", app.tr("home.export"), app.tr_label(format))
        };
        let subtitle = hint.map(|key| app.tr(key)).unwrap_or(format);
        let response = FileView::action_tile(ui, &format!("home:export:{id}"), width, "export", &label, subtitle, false);
        publish(&format!("home:export:{id}"), &response);
        if response.clicked() {
            *action = Some(match *id {
                "step" => FileAction::Export("step"),
                "mesh" => FileAction::Mesh,
                "view_dxf" => FileAction::ViewDxf,
                "obj" => FileAction::Export("obj"),
                "urdf" => FileAction::Export("urdf"),
                _ => FileAction::Export("json"),
            });
        }
    });
}

fn dispatch(app: &mut SolveApp, ctx: &egui::Context, action: FileAction) {
    match action {
        FileAction::Back => app.home.open = false,
        FileAction::Section(section) => {
            app.home.section = section;
            if section == Section::Recovery && app.recoverable.is_empty() {
                let dir = app.autosave.as_ref().map(|autosave| autosave.dir.clone()).or_else(solidcraft_engine::recovery::default_dir);
                if let Some(dir) = dir {
                    app.recoverable = solidcraft_engine::recovery::orphans(&dir);
                }
            }
        }
        FileAction::New(units) => {
            crate::documents::new_design(app);
            if units != "preferred" && !app.home.open && crate::documents::active_is_blank(app) {
                crate::prefs::apply_new_design_units(app, units);
            }
        }
        FileAction::Open => app.pick_open(|app, path| {
            if let Some(path) = path {
                app.open_path_async(&path);
            }
        }),
        FileAction::Import => app.pick_open_for(&["step", "stp", "iges", "igs", "stl", "obj", "3mf", "gltf", "glb"], |app, path| {
            if let Some(path) = path {
                app.open_path_async(&path);
            }
        }),
        FileAction::Save => {
            app.home.open = false;
            crate::toolbar::save(app);
        }
        FileAction::SaveAs => {
            app.home.open = false;
            crate::toolbar::save_as(app);
        }
        FileAction::Export(ext) => {
            app.home.open = false;
            crate::toolbar::export(app, ext);
        }
        FileAction::Mesh => {
            app.home.open = false;
            app.start("file.save_mesh");
        }
        FileAction::ViewDxf => {
            app.home.open = false;
            app.start("file.export_view_dxf");
        }
        FileAction::Properties => {
            app.home.open = false;
            app.start("document.metadata");
        }
        FileAction::Settings => crate::prefs::open_at(app, 0),
        FileAction::Shortcuts => {
            app.keymap.open = true;
            app.keymap.read_only = true;
        }
        FileAction::Crashes => crate::help::run(app, ctx, "crashes"),
        FileAction::About => crate::help::run(app, ctx, "about"),
        FileAction::Recovery(entry, recover) => crate::recovery_ui::apply(app, entry, recover),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_paths_only_abbreviate_complete_home_ancestors() {
        let home = Some(std::path::Path::new("/Users/sample"));
        assert_eq!(abbreviated_path("/Users/sample/Designs/plate.solidcraft", home), "~/Designs/plate.solidcraft");
        assert_eq!(abbreviated_path("/Users/sample", home), "~");
        assert_eq!(abbreviated_path("/Users/sample-other/plate.solidcraft", home), "/Users/sample-other/plate.solidcraft");
        assert_eq!(abbreviated_path("/disk/design.solidcraft", None), "/disk/design.solidcraft");
    }

    #[test]
    fn recent_times_use_compact_labels_and_calendar_dates() {
        let app = crate::test_support::sample_app();
        let now = 1_792_569_600; // 2026-10-21T08:00:00Z.
        assert_eq!(recent_time_at(&app, now + 60, now), "Just now");
        assert_eq!(recent_time_at(&app, now - 120, now), "2 min ago");
        assert_eq!(recent_time_at(&app, now - 7200, now), "2 h ago");
        assert_eq!(recent_time_at(&app, now - 86400, now), "Yesterday");
        assert_eq!(recent_time_at(&app, now - 172800, now), "19 Oct");
    }

    #[test]
    fn shipped_sample_pictures_are_bounded_and_match_the_current_scripts() {
        use sha2::{Digest, Sha256};
        let hashes: Vec<String> = serde_json::from_str(include_str!("../../../assets/samples/exports/scripts.json")).unwrap();
        let samples = solidcraft_engine::sample::samples();
        assert_eq!(samples.len(), SAMPLE_PICTURES.len());
        assert_eq!(hashes.len(), samples.len());
        for (i, sample) in samples.iter().enumerate() {
            let script = serde_json::to_vec(&(sample.script)()).unwrap();
            assert_eq!(hashes.get(i).unwrap(), &format!("{:x}", Sha256::digest(script)), "regenerate the preview for {}", sample.name);
            assert_eq!(sample_picture(i).unwrap().size, THUMB);
        }
        assert!(sample_picture(samples.len()).is_none());
    }

    /// A recent design the host can bring back (the browser build's remembered files) is not
    /// "missing": choosing it asks the host to restore it, and one the host doesn't know still is.
    #[test]
    fn a_recent_design_the_host_can_restore_is_asked_for_not_reported_missing() {
        let mut h = crate::scenario::Harness::new();
        let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sink = asked.clone();
        h.app.services.can_restore = Some(Box::new(|p| p.starts_with("/disk/known/")));
        h.app.services.restore_file = Some(Box::new(move |p| sink.borrow_mut().push(p.to_string())));
        assert!(available(&h.app, "/disk/known/Bracket.solidcraft"));
        // A file is taken to be there until the background check says it is not.
        assert!(available(&h.app, "/disk/other/Bracket.solidcraft"));
        h.app.home.present.insert("/disk/other/Bracket.solidcraft".into(), false);
        assert!(!available(&h.app, "/disk/other/Bracket.solidcraft"));
        h.app.home.add_recent("/disk/known/Bracket.solidcraft");
        h.app.home.show(h.app.session.revision);
        click_file(&mut h, "home:recent:0");
        assert_eq!(*asked.borrow(), ["/disk/known/Bracket.solidcraft"]);
        open_recent(&mut h.app, "/disk/other/Bracket.solidcraft");
        assert_eq!(asked.borrow().len(), 1, "a file the host doesn't remember is not asked for");
        assert!(h.app.status.as_ref().is_some_and(|(text, _, error)| *error && text.contains("no longer there")));
    }

    /// Rebuild the shipped pictures from the authoritative sample command scripts.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "asset authoring: writes assets/samples/exports"]
    fn regenerate_sample_thumbnails() {
        use sha2::{Digest, Sha256};
        assert!(crate::gpu::test_picture_renderer());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/samples/exports");
        std::fs::create_dir_all(&dir).unwrap();
        let mut hashes = Vec::new();
        for (i, sample) in solidcraft_engine::sample::samples().into_iter().enumerate() {
            let began = std::time::Instant::now();
            let script = (sample.script)();
            let mut session = Session::default();
            session.run_script(&script).unwrap();
            let image = thumbnail(&session).unwrap();
            let rgba = image.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect();
            let png = solidcraft_engine::render::png(THUMB[0], THUMB[1], rgba).unwrap();
            std::fs::write(dir.join(format!("{i}.png")), png).unwrap();
            hashes.push(format!("{:x}", Sha256::digest(serde_json::to_vec(&script).unwrap())));
            eprintln!("{}: {:.3} s", sample.name, began.elapsed().as_secs_f64());
        }
        std::fs::write(dir.join("scripts.json"), serde_json::to_vec_pretty(&hashes).unwrap()).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cached_recent_uses_a_separate_png_without_reading_or_changing_design_json() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-thumb-{}-{nonce}.solidcraft", std::process::id()));
        // Deliberately invalid design: a cached recent must not parse/evaluate it.
        let bytes = b"{\"thumbnail_test\":true}";
        std::fs::write(&path, bytes).unwrap();
        let key = path.to_string_lossy().to_string();
        let mut s = Session::default();
        s.execute("solid.box", &serde_json::json!({"length":10,"width":10,"height":10})).unwrap();
        s.path = Some(key.clone());
        let image = remember_file(&s, &key).unwrap();
        let png = cache_path(&key).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let (_, cached_image, session) = make(&key);
        assert!(session.is_none());
        assert_eq!(cached_image.unwrap().pixels, image.pixels);
        std::fs::write(&path, b"changed file identity").unwrap();
        assert!(cached(&key).is_none());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(png).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn portable_thumbnail_loads_without_a_local_picture_or_model_rebuild() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let mut session = Session::default();
        session.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        for body in &session.world_state().bodies {
            body.mesh();
        }
        let bytes = session.design_container(solidcraft_engine::io::DesignCompression::Fast, false, true).unwrap();
        let png = solidcraft_engine::io::read_design_thumbnail(&bytes).unwrap();
        let image = decode_thumbnail(png).unwrap();
        assert_eq!(image.size, THUMB);
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-portable-thumb-{}-{nonce}.solidcraft", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let key = path.to_string_lossy().to_string();
        assert!(cached(&key).is_none());
        let (_, picture, rebuilt) = make(&key);
        assert!(rebuilt.is_none());
        assert_eq!(picture.unwrap().pixels, image.pixels);
        assert!(!cache_path(&key).unwrap().exists(), "fallback rebuilding would create a local picture");
        std::fs::remove_file(path).unwrap();
        assert!(decode_thumbnail(vec![0; 600_000]).is_none());
        assert!(decode_thumbnail(vec![0; 24]).is_none());
    }

    /// STEP opens deliberately have no save path, but their source still gets a cache entry.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn imported_step_picture_is_cached_under_its_source_without_a_save_path() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-import-thumb-{}-{nonce}.step", std::process::id()));
        let key = path.to_string_lossy().into_owned();
        let mut part = Session::default();
        part.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        part.execute("file.export", &serde_json::json!({"path":key})).unwrap();
        let original = std::fs::read(&path).unwrap();
        let mut opened = Session::default();
        opened.execute("doc.open", &serde_json::json!({"path":key})).unwrap();
        assert!(opened.path.is_none(), "an imported file is an unsaved design");
        let mut home = HomeState::default();
        remember_source(&mut home, &opened, &key);
        let (made_key, image, rebuilt) = home.remembering.take().unwrap().recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(made_key, key);
        assert!(rebuilt.is_none());
        let image = image.unwrap();
        assert!(opened.path.is_none(), "caching must not turn Save into an overwrite of the STEP source");
        let png = cache_path(&key).unwrap();
        assert_eq!(cached(&key).unwrap().pixels, image.pixels);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let (_, picture, rebuilt) = make(&key);
        assert!(rebuilt.is_none());
        assert_eq!(picture.unwrap().pixels, image.pixels);
        std::fs::write(&path, b"file replaced").unwrap();
        assert!(cached(&key).is_none());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(png).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn portable_pictures_are_sent_before_any_cold_recent_is_built() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("solidcraft-fast-thumb-{}-{nonce}.solidcraft", std::process::id()));
        let key = path.to_string_lossy().into_owned();
        let mut s = Session::default();
        s.execute("solid.box", &serde_json::json!({"length":10,"width":20,"height":30})).unwrap();
        for b in &s.world_state().bodies {
            b.mesh();
        }
        let bytes = s.design_container(solidcraft_engine::io::DesignCompression::Fast, false, true).unwrap();
        std::fs::write(&path, bytes).unwrap();
        let cold = format!("missing-cold-file-{nonce}.step");
        let (tx, rx) = std::sync::mpsc::channel();
        let misses = read_pictures(vec![cold.clone(), key.clone()], &tx).unwrap();
        assert_eq!(misses, vec![cold]);
        let (ready, picture, rebuilt) = rx.try_recv().unwrap();
        assert_eq!(ready, key);
        assert!(picture.is_some() && rebuilt.is_none());
        assert!(cached(&key).is_none(), "the portable picture did not rebuild the design");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn first_home_frame_shows_placeholders_and_the_sample_pictures_arrive_without_building_any_model() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.show(0);
        h.frames(1);
        assert!(h.app.home.ready_samples() < solidcraft_engine::sample::samples().len(), "the first frame decodes nothing");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while h.app.home.ready_samples() < solidcraft_engine::sample::samples().len() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frames(1);
        }
        assert_eq!(h.app.home.ready_samples(), solidcraft_engine::sample::samples().len());
        assert!(h.app.home.samples.is_empty(), "no sample model was built");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn selecting_a_sample_does_not_wait_for_a_running_cold_recent() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let (_blocked_worker, rx) = std::sync::mpsc::channel();
        app.home.rx = Some(rx);
        app.home.open = false;
        let ctx = egui::Context::default();
        open_sample(&mut app, 1);
        assert!(app.home.open, "a programmatic selection from a document must start the sample job too");
        let began = std::time::Instant::now();
        while app.home.open && began.elapsed() < std::time::Duration::from_secs(30) {
            poll(&mut app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!app.home.open, "the recent worker is still blocked, but the sample must open");
        assert!(app.home.rx.is_some());
        assert_eq!(app.session.doc.name, "Sheet Metal Bracket");
        assert!(!app.session.is_dirty());
    }

    /// The shipped picture of the sample being opened can arrive after it was asked for: that is
    /// not its build failing, and the sample still opens (#4349 fixed it; it had made the
    /// `s15_start_page_sample_save_reopen` scenario fail depending on timing).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_sample_picture_arriving_while_the_sample_builds_is_not_a_failed_build() {
        let mut app = SolveApp::new(Session::default(), Default::default());
        let (pictures, rx) = std::sync::mpsc::channel();
        app.home.rx = Some(rx);
        let ctx = egui::Context::default();
        open_sample(&mut app, 0);
        assert!(app.home.open && app.home.wanted == Some(0));
        pictures.send(("sample:0".into(), sample_picture(0), None)).unwrap();
        let began = std::time::Instant::now();
        while app.home.open && began.elapsed() < std::time::Duration::from_secs(30) {
            poll(&mut app, &ctx);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.status.as_ref().is_none_or(|(_, _, error)| !error), "{:?}", app.status);
        assert!(!app.home.open, "the sample opens once built");
        assert_eq!(app.session.doc.name, "Sample Plate");
    }

    #[test]
    fn recent_list_is_newest_first_and_bounded() {
        let mut h = HomeState::default();
        for i in 0..20 {
            h.add_recent(&format!("/d/{i}.solidcraft"));
        }
        h.add_recent("/d/5.solidcraft");
        assert_eq!(h.recent.len(), MAX_RECENT);
        assert_eq!(h.recent.first().map(String::as_str), Some("/d/5.solidcraft"));
        assert_eq!(h.recent.iter().filter(|p| p.as_str() == "/d/5.solidcraft").count(), 1);
    }

    /// Samples build in the background and make a picture; picking one opens its design.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sample_card_makes_a_picture_and_opens() {
        if !crate::gpu::test_picture_renderer() {
            return;
        }
        let (key, img, session) = make("sample:0");
        assert_eq!(key, "sample:0");
        let img = img.unwrap();
        assert_eq!(img.size, THUMB);
        let s = session.unwrap();
        assert_eq!(s.doc.name, "Sample Plate");
        assert!(!s.is_dirty() && s.undo.is_empty());
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        app.home.samples.insert(0, s);
        app.home.show(app.session.revision);
        open_sample(&mut app, 0);
        assert!(!app.home.open);
        assert_eq!(app.session.doc.name, "Sample Plate");
        assert!(!app.session.is_dirty());
    }

    /// The gear, Ctrl+, and Home's own links open Preferences over Home, not after leaving it.
    #[test]
    fn preferences_and_shortcuts_open_over_home() {
        let mut h = crate::scenario::Harness::new();
        let rev = h.app.session.revision;
        h.app.home.show(rev);
        h.frames(2);
        assert!(h.app.home.open);
        let gear = crate::scenario::handle_at("titlebar:preferences").unwrap();
        h.call("ui.click", serde_json::json!({"x": gear.x, "y": gear.y}));
        assert!(h.app.prefs_window.open && h.app.home.open);
        let shown = |h: &crate::scenario::Harness| {
            h.ctx.memory(|m| m.areas().visible_last_frame(&egui::LayerId::new(egui::Order::Middle, egui::Id::new("sc_prefs"))))
        };
        assert!(shown(&h), "the Preferences window must be drawn on Home");
        h.call("ui.prefs", serde_json::json!({"button": "cancel"}));
        assert!(!shown(&h));

        // Home › Keyboard Shortcuts opens the same window at its page.
        h.app.keymap.open = true;
        h.frames(2);
        assert!(shown(&h) && h.app.prefs_window.section == crate::prefs::PAGE_SHORTCUTS && h.app.home.open);
    }

    fn click_file(h: &mut crate::scenario::Harness, id: &str) {
        h.frames(3);
        h.step(&serde_json::json!({"click": {"handle": id}})).unwrap();
        h.frames(3);
    }

    #[test]
    fn file_view_new_tiles_choose_units_without_changing_preferences_or_other_tabs() {
        for (id, units) in [("home:new_mm", "mm"), ("home:new_inches", "in")] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
            let original = h.app.session.doc.to_json();
            h.app.preferences.default_units = "ft".into();
            h.app.home.show(0);
            click_file(&mut h, id);
            assert!(!h.app.home.open);
            assert_eq!(h.app.session.doc.units, units);
            assert_eq!(h.app.preferences.default_units, "ft");
            assert_eq!(h.app.docs.count(), 2);
            assert!(!h.app.session.is_dirty(), "a new empty design is initially clean");
            crate::documents::switch(&mut h.app, 0);
            assert_eq!(h.app.session.doc.to_json(), original, "the previously open design is untouched");
        }
    }

    #[test]
    fn file_view_open_and_import_cancel_preserve_the_current_design_and_history() {
        for id in ["home:open_tile", "home:import"] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
            let original = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            h.app.home.show(0);
            click_file(&mut h, id);
            assert!(h.app.home.open, "cancel keeps the File view open");
            assert_eq!(h.app.session.doc.to_json(), original);
            assert_eq!(h.app.session.undo.len(), undo);
            assert_eq!(h.app.docs.count(), 1);
        }
    }

    #[test]
    fn file_view_save_as_and_export_act_only_after_an_explicit_file_action() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
        h.app.home.show(0);
        h.frames(3);
        assert!(h.app.session.path.is_none());
        click_file(&mut h, "home:save_as");
        h.settle();
        assert!(!h.app.home.open);
        assert!(h.app.session.path.as_ref().is_some_and(|path| std::path::Path::new(path).is_file()));
        assert!(!h.app.session.is_dirty());
        let original = h.app.session.doc.to_json();
        h.app.home.show(0);
        click_file(&mut h, "home:export");
        assert!(h.app.home.open && h.app.home.section == Section::Export);
        click_file(&mut h, "home:export:json");
        h.settle();
        assert!(!h.app.home.open);
        assert!(h.dir.join(format!("{}.json", h.app.session.doc.name)).is_file());
        assert_eq!(h.app.session.doc.to_json(), original, "export leaves the design unchanged");
    }

    #[test]
    fn file_view_recovery_reuses_dirty_design_protection_and_keeps_the_entry() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
        let original = h.app.session.doc.to_json();
        h.app.recoverable =
            vec![solidcraft_engine::recovery::Entry { id: "file-test".into(), name: "Bracket".into(), path: None, saved_at: 0, features: 3 }];
        h.app.home.show(0);
        h.frames(3);
        assert!(crate::scenario::handle_at("home:recovery:recover").is_some());
        click_file(&mut h, "home:recovery:recover");
        assert_eq!(h.app.session.doc.to_json(), original);
        assert_eq!(h.app.recoverable.len(), 1);
        click_file(&mut h, "home:recover");
        click_file(&mut h, "home:recovery:0:recover");
        assert_eq!(h.app.session.doc.to_json(), original);
        assert_eq!(h.app.recoverable.len(), 1);
        assert!(h.app.status.as_ref().is_some_and(|(message, _, error)| *error && message.contains("save or discard")));
        assert!(crate::scenario::rect_of("dialog:report:home_status").is_some(), "the refusal is visible on File");
    }

    #[test]
    fn file_view_escape_returns_to_design_but_does_not_close_file_behind_preferences() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.show(0);
        click_file(&mut h, "home:settings");
        h.step(&serde_json::json!({"key":"Escape"})).unwrap();
        assert!(h.app.home.open);
        h.frames(2);
        click_file(&mut h, "home:about");
        h.step(&serde_json::json!({"key":"Escape"})).unwrap();
        assert!(h.app.home.open, "Escape must close About without closing File behind it");
        h.frames(3);
        h.step(&serde_json::json!({"key":"Escape"})).unwrap();
        assert!(!h.app.home.open);
    }

    #[test]
    fn file_view_narrow_navigation_and_content_stay_inside_the_window() {
        for dark in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.call("ui.resize", serde_json::json!({"width":420,"height":800}));
            h.app.ui.dark = dark;
            h.app.home.show(0);
            h.frames(4);
            let page = crate::scenario::rect_of("home:page").unwrap();
            let tile = crate::scenario::rect_of("home:new_mm").unwrap();
            assert!(page.contains_rect(tile), "{dark}: the new tile escapes the narrow page");
            h.step(&serde_json::json!({"widget":"Start page"})).unwrap();
            h.frames(2);
            assert!(crate::scenario::handle_at("home:settings").is_some(), "all File actions remain available through the narrow menu");
        }
    }

    #[test]
    fn file_view_desktop_cards_align_in_rows_and_remain_visible() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.resize", serde_json::json!({"width":1400,"height":900}));
        h.app.home.show(0);
        h.frames(5);
        let page = crate::scenario::rect_of("home:page").unwrap();
        let first = crate::scenario::rect_of("home:new_mm").unwrap();
        for id in ["home:new_mm", "home:new_inches", "home:open_tile", "home:import"] {
            let rect = crate::scenario::rect_of(id).unwrap();
            assert!((rect.top() - first.top()).abs() < 0.5, "{id}: action tiles must share a row");
            assert!(page.contains_rect(rect), "{id}: card is off screen");
        }
        let first = crate::scenario::rect_of("home:sample:0").unwrap();
        for i in 0..6 {
            let rect = crate::scenario::rect_of(&format!("home:sample:{i}")).unwrap();
            assert!((rect.top() - first.top()).abs() < 0.5, "sample {i}: thumbnails must share a row");
            assert!(page.contains_rect(rect), "sample {i}: card is off screen");
        }
    }

    #[test]
    fn file_view_save_as_shortcut_uses_the_picker_without_overwriting_the_source() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
        let source = h.dir.join("source.solidcraft");
        h.app.run("file.save_as", serde_json::json!({"path":source})).unwrap();
        let bytes = std::fs::read(&source).unwrap();
        h.app.session.doc = std::sync::Arc::new({
            let mut doc = (*h.app.session.doc).clone();
            doc.name = "Copy".into();
            doc
        });
        h.app.home.show(0);
        h.frames(3);
        h.step(&serde_json::json!({"key":"S","cmd":true,"shift":true})).unwrap();
        h.frames(3);
        h.settle();
        assert!(!h.app.home.open);
        assert_eq!(h.app.session.path.as_deref(), h.dir.join("Copy.solidcraft").to_str());
        assert_eq!(std::fs::read(source).unwrap(), bytes);
    }

    #[test]
    fn document_plus_opens_file_before_creating_a_design() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", serde_json::json!({"length":10,"width":12,"height":14})).unwrap();
        let original = h.app.session.doc.to_json();
        click_file(&mut h, "tab:new");
        assert!(h.app.home.open);
        assert_eq!(h.app.docs.count(), 1);
        assert_eq!(h.app.session.doc.to_json(), original);
        click_file(&mut h, "home:new_inches");
        assert_eq!(h.app.docs.count(), 2);
        assert_eq!(h.app.session.doc.units, "in");
    }
}
