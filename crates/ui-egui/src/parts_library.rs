//! The parts library: a shared panel (docked at the view's left by default; it docks, floats and
//! pops out as the Browser does) with the designs and parts in a folder on this computer or a
//! server share, each shown by its picture and name. Drag one into the view to put it in the open
//! design: a SolidCraft design becomes a linked component (an occurrence placed where it is
//! dropped, as Insert Design makes), a STEP or mesh file is imported (as Insert STEP or Insert
//! Mesh). Folders open in place; double-click a part to open it on its own. The app bar's
//! documents button (`|||`, left of File) shows and hides the panel in the left-hand place the
//! old documents browser had; it remembers its folder, the subfolder last browsed into and the card
//! size. The shared panel host owns its title, close, collapse and placement. Built from kit pieces
//! only (#8206).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::prefs::LibrarySize;
use crate::ui_kit::{Container, PartCard, PartCardSize, PartFace, Surface};
use solidcraft_theme::Tokens;

/// Most entries listed from one folder (a share can hold thousands).
const MAX_ENTRIES: usize = 2000;
/// A folder's listing is read again after this long (ms), so a share's new parts show up.
const RESCAN_MS: f64 = 15_000.0;
/// The panel's default width, and the room its list keeps clear on the right for the scroll bar.
pub(crate) const CARD_WIDTH: f32 = 324.0;
const SCROLL_BAR: f32 = 14.0;
const CARD_STRETCH: f32 = 1.25;
/// Placeholder cards drawn while a folder is read.
const PLACEHOLDERS: usize = 6;

/// How long the cards slide when the columns change (s).
const SLIDE: f64 = 0.22;

/// The cards' grid in a list `room` wide: the size's own card width, as many columns as fit,
/// centred.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    size: PartCardSize,
    columns: usize,
    width: f32,
    /// The first column's left, from the list's left.
    left: f32,
    /// The gap between rows.
    rows_gap: f32,
}

impl Grid {
    fn new(size: PartCardSize, room: f32, rows_gap: f32) -> Self {
        let columns = size.columns_in(room);
        // Cards widen a little to take up the room left beside the last column, so a careless panel
        // width doesn't leave wide side margins; past that much, the next column comes in instead.
        let fill = (room - PartCardSize::gap() * (columns - 1) as f32) / columns as f32;
        let width = fill.min(size.width() * CARD_STRETCH).max(Tokens::get().space_24.min(size.width()));
        let span = width * columns as f32 + PartCardSize::gap() * (columns - 1) as f32;
        Grid { size, columns, width, left: ((room - span) / 2.0).max(0.0), rows_gap }
    }

    fn row(&self) -> f32 {
        self.size.height() + self.rows_gap
    }

    /// Card `i`'s top left, from the list's top left.
    fn at(&self, i: usize) -> egui::Vec2 {
        let column = i % self.columns;
        egui::vec2(self.left + column as f32 * (self.width + PartCardSize::gap()), (i / self.columns) as f32 * self.row())
    }

    /// The same cards across, whatever the room left over.
    fn same_columns(&self, other: &Grid) -> bool {
        self.size == other.size && self.columns == other.columns
    }
}

/// Where card `i` is drawn `t` (0..1) into a slide from `from` to `to`, and how opaque. A card that
/// keeps its column moves from its old place to its new one; a card that changes column slides
/// in from the right at its new row (so a new column comes in from the right, and the column
/// that goes slides back into the first).
fn slid(from: &Grid, to: &Grid, i: usize, t: f32, room: f32) -> (egui::Vec2, f32) {
    let e = 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3);
    let end = to.at(i);
    if i % from.columns == i % to.columns { (from.at(i) + (end - from.at(i)) * e, 1.0) } else { (egui::vec2(room + (end.x - room) * e, end.y), e) }
}

/// The card size of `size`, `None` for List.
fn card_size(size: LibrarySize) -> Option<PartCardSize> {
    match size {
        LibrarySize::Small => Some(PartCardSize::Small),
        LibrarySize::Medium => Some(PartCardSize::Medium),
        LibrarySize::Large => Some(PartCardSize::Large),
        LibrarySize::List => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    Design,
    Step,
    Mesh,
}

impl Kind {
    fn of(path: &Path) -> Option<Kind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() {
            e if solidcraft_engine::io::DESIGN_EXTS.contains(&e) => Kind::Design,
            "step" | "stp" => Kind::Step,
            "stl" | "3mf" | "obj" | "glb" | "gltf" => Kind::Mesh,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
    /// A folder's designs and parts (not counting its subfolders).
    pub count: usize,
}

/// What a card carries while it is dragged.
#[derive(Clone, Debug)]
pub struct Dragged(pub PathBuf);

type Listing = (PathBuf, Result<Vec<Entry>, String>);

#[derive(Default)]
pub struct LibraryState {
    /// The folder being shown: the library folder or one inside it.
    pub current: Option<PathBuf>,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
    pub filter: String,
    /// The card picked last (a click), drawn with the accent edge.
    pub selected: Option<PathBuf>,
    /// Where the panel's body was painted this frame (cleared each tick): a part let go over it is
    /// not dropped in the view it lies over, and Ctrl+F or `/` over it focuses the search.
    pub(crate) shown: Option<egui::Rect>,
    listed: Option<(PathBuf, f64)>,
    #[cfg(not(target_arch = "wasm32"))]
    listing: Option<std::sync::mpsc::Receiver<Listing>>,
    thumbs: HashMap<String, Option<crate::ui_kit::PreviewTextures>>,
    queued: Vec<String>,
    /// The card grid drawn last frame, and the slide from the grid before it.
    grid: Option<Grid>,
    slide: Option<(Grid, f64)>,
    #[cfg(not(target_arch = "wasm32"))]
    pictures: Option<std::sync::mpsc::Receiver<(String, Option<crate::ui_kit::PreviewImage>)>>,
    #[cfg(not(target_arch = "wasm32"))]
    slow_pictures: Vec<std::sync::mpsc::Receiver<(String, Option<crate::ui_kit::PreviewImage>)>>,
}

/// Read a folder: its subfolders, then its designs and parts, by name (hidden files left out).
pub fn list(dir: &Path) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for item in read.flatten().take(MAX_ENTRIES * 4) {
        let path = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with('~') {
            continue;
        }
        // A linked folder (a symlink, a junction) is listed as the folder it points to.
        let is_dir = item.file_type().map(|t| if t.is_symlink() { path.is_dir() } else { t.is_dir() }).unwrap_or(false);
        let kind = if is_dir { Some(Kind::Folder) } else { Kind::of(&path) };
        if let Some(kind) = kind {
            let shown = if kind == Kind::Folder { name } else { path.file_stem().map_or(name.clone(), |s| s.to_string_lossy().to_string()) };
            let count = if kind == Kind::Folder { parts_in(&path) } else { 0 };
            out.push(Entry { path, name: shown, kind, count });
        }
        if out.len() >= MAX_ENTRIES {
            break;
        }
    }
    out.sort_by(|a, b| (a.kind != Kind::Folder).cmp(&(b.kind != Kind::Folder)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// How many designs and parts a folder holds directly (0 when it can't be read).
fn parts_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |read| {
        read.flatten().take(MAX_ENTRIES).filter(|i| !i.file_name().to_string_lossy().starts_with('.') && Kind::of(&i.path()).is_some()).count()
    })
}

fn root(app: &SolveApp) -> Option<PathBuf> {
    let f = app.preferences.library_folder.trim();
    (!f.is_empty()).then(|| PathBuf::from(f))
}

/// Show the library from `folder` (kept in the preferences).
pub fn set_folder(app: &mut SolveApp, folder: &str) {
    app.preferences.library_folder = folder.to_string();
    app.preferences.library_open = true;
    app.preferences.library_subfolder.clear();
    app.parts_library.current = None;
    app.parts_library.listed = None;
    app.parts_library.entries.clear();
    app.parts_library.error = None;
}

/// Ask for the folder with the host's dialog.
pub fn choose_folder(app: &mut SolveApp) {
    if app.services.pick_folder.is_none() {
        app.set_status(solidcraft_ui_text::tr("parts_library.no_picker").to_string(), true);
        return;
    }
    let current = app.preferences.library_folder.clone();
    app.pick_folder(&current, |app, path| {
        if let Some(p) = path {
            set_folder(app, &p);
        }
    });
}

/// The folder shown: the one browsed into, else the remembered subfolder, else the library folder.
/// Nothing here touches the disk (a share can be slow): a remembered subfolder that is gone shows
/// the library folder once its listing fails.
fn current(app: &SolveApp) -> Option<PathBuf> {
    app.parts_library.current.clone().or_else(|| {
        let root = root(app)?;
        let sub = app.preferences.library_subfolder.trim();
        let plain = Path::new(sub).components().all(|c| matches!(c, std::path::Component::Normal(_)));
        Some(if !sub.is_empty() && plain { root.join(sub) } else { root })
    })
}

/// Browse into `dir` (the library folder or one inside it) and remember it.
fn enter(app: &mut SolveApp, ctx: &egui::Context, dir: PathBuf) {
    app.preferences.library_subfolder =
        root(app).and_then(|r| dir.strip_prefix(&r).ok().map(|rel| rel.to_string_lossy().replace('\\', "/"))).unwrap_or_default();
    app.parts_library.current = Some(dir);
    app.parts_library.entries.clear();
    app.parts_library.error = None;
    refresh(app, ctx, true);
}

/// The folder is being read and nothing from it has come in yet.
fn reading(app: &SolveApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.parts_library.listing.is_some() && app.parts_library.entries.is_empty()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        false
    }
}

/// Read the shown folder again when it changed or the listing is old.
fn refresh(app: &mut SolveApp, ctx: &egui::Context, force: bool) {
    let Some(dir) = current(app) else { return };
    let now = crate::now_ms();
    let fresh = app.parts_library.listed.as_ref().is_some_and(|(d, at)| *d == dir && now - at < RESCAN_MS);
    if fresh && !force {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !force && app.parts_library.listing.is_some() && app.parts_library.listed.as_ref().is_some_and(|(d, _)| *d == dir) {
        return;
    }
    app.parts_library.listed = Some((dir.clone(), now));
    #[cfg(not(target_arch = "wasm32"))]
    {
        // A share can be slow: list it off the window thread.
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("library".into()).spawn(move || {
            let listing = list(&dir);
            let _ = tx.send((dir, listing));
            ctx.request_repaint();
        });
        if spawned.is_ok() {
            app.parts_library.listing = Some(rx);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        take_listing(app, (dir.clone(), list(&dir)));
    }
}

fn take_listing(app: &mut SolveApp, (dir, listing): Listing) {
    if current(app).as_deref() != Some(dir.as_path()) {
        return;
    }
    match listing {
        Ok(entries) => {
            app.parts_library.error = None;
            app.parts_library.entries = entries;
        }
        Err(_) if app.parts_library.current.is_none() && !app.preferences.library_subfolder.is_empty() => {
            // The remembered subfolder is gone: the library folder instead.
            app.preferences.library_subfolder.clear();
            app.parts_library.listed = None;
        }
        Err(e) => {
            app.parts_library.error = Some(e);
            app.parts_library.entries.clear();
        }
    }
}

/// Per frame: listings and pictures that came in.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    // Escape over the library lets the selected card go. It is read here, at the start of the frame,
    // over where the card was drawn last frame: a handler drawn before the panel can consume the key.
    let last = app.parts_library.shown.take();
    let over = last.is_some_and(|rect| ctx.input(|i| i.pointer.latest_pos()).is_some_and(|at| rect.contains(at)));
    if app.parts_library.selected.is_some() && over && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.parts_library.selected = None;
    }
    if !app.preferences.library_open {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(rx) = &app.parts_library.listing
            && let Ok(l) = rx.try_recv()
        {
            app.parts_library.listing = None;
            take_listing(app, l);
        }
        let mut done = false;
        while let Some(rx) = &app.parts_library.pictures {
            match rx.try_recv() {
                Ok((key, img)) => crate::home::install_texture(&mut app.parts_library.thumbs, ctx, &key, img),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        if done {
            app.parts_library.pictures = None;
        }
        let thumbs = &mut app.parts_library.thumbs;
        app.parts_library.slow_pictures.retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok((key, img)) => crate::home::install_texture(thumbs, ctx, &key, img),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break true,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break false,
                }
            }
        });
        start_pictures(&mut app.parts_library);
    }
    refresh(app, ctx, false);
}

#[cfg(not(target_arch = "wasm32"))]
fn start_pictures(l: &mut LibraryState) {
    if l.pictures.is_some() || l.queued.is_empty() {
        return;
    }
    // A big import can take minutes: it gets its own thread so the other cards don't wait on it.
    let (slow, keys): (Vec<String>, Vec<String>) =
        std::mem::take(&mut l.queued).into_iter().partition(|k| std::fs::metadata(k).is_ok_and(|m| m.len() > SLOW_PICTURE_BYTES));
    if !slow.is_empty() {
        // Held as blank until drawn, so a card doesn't queue it again.
        for key in &slow {
            l.thumbs.insert(key.clone(), None);
        }
        if let Some(rx) = spawn_pictures(slow) {
            l.slow_pictures.push(rx);
        }
    }
    if !keys.is_empty() {
        for key in &keys {
            l.thumbs.insert(key.clone(), None);
        }
        l.pictures = spawn_pictures(keys);
    }
}

#[cfg(not(target_arch = "wasm32"))]
const SLOW_PICTURE_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
fn spawn_pictures(keys: Vec<String>) -> Option<std::sync::mpsc::Receiver<(String, Option<crate::ui_kit::PreviewImage>)>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("library pictures".into()).spawn(move || {
        for key in keys {
            let img = std::panic::catch_unwind(|| crate::home::part_picture(&key)).ok().flatten();
            if tx.send((key, img)).is_err() {
                return;
            }
        }
    });
    spawned.is_ok().then_some(rx)
}

fn thumb(l: &mut LibraryState, key: &str) -> Option<crate::ui_kit::PreviewArt> {
    match l.thumbs.get(key) {
        Some(t) => t.as_ref().map(crate::ui_kit::PreviewTextures::art),
        None => {
            if !l.queued.iter().any(|k| k == key) {
                l.queued.push(key.to_string());
            }
            None
        }
    }
}

/// Where a part dropped at `pos` goes: the model point under it, else the ground plane there.
fn drop_point(app: &SolveApp, proj: &crate::viewport::Proj, pos: egui::Pos2) -> Vec3 {
    for h in crate::viewport::pick_3d(app, proj, pos) {
        if let crate::viewport::Hit::Face { point, .. } = h {
            return point;
        }
    }
    let (o, d) = proj.ray(pos);
    let up = if app.cam.up == solidcraft_viewport::UpAxis::Y { Vec3::Y } else { Vec3::Z };
    solidcore_record::Plane::from_normal(Vec3::ZERO, up)
        .and_then(|ground| ground.intersect_ray(o, d))
        .filter(|p| proj.to_screen(*p).is_some())
        .unwrap_or(Vec3::ZERO)
}

/// Put the part at `path` in the open design at `at` (mm): a card the user dragged or picked, so
/// a design on a server share is the user's choice and its server is allowed for the insert,
/// as Insert Design's file dialog allows it.
pub fn insert(app: &mut SolveApp, path: &Path, at: Vec3) -> Result<(), String> {
    let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(&path.to_string_lossy()).into_iter().collect();
    insert_allowing(app, path, at, &hosts)
}

/// [`insert`], allowing only the servers in `hosts` (none for `ui.library`, whose folder the
/// caller may have set: a share then needs the server allowed as any other path does).
fn insert_allowing(app: &mut SolveApp, path: &Path, at: Vec3, hosts: &[String]) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    match Kind::of(path) {
        Some(Kind::Design) => solidcraft_engine::io::vfs::allowing_hosts(hosts, || {
            app.run("component.insert_design", json!({"path": p, "translate": [at.x, at.y, at.z], "reuse": true}))
        })
        .map(|_| ()),
        Some(Kind::Step | Kind::Mesh) => {
            app.insert_path_async(&p);
            Ok(())
        }
        _ => Err(solidcraft_ui_text::format("parts_library.not_a_part", &[("path", &p)])),
    }
}

/// The view: a card dropped on it inserts its part there. Returns where it will land, for
/// [`paint_drop`] once the model is drawn.
pub fn view_drop(app: &mut SolveApp, ui: &egui::Ui, rect: egui::Rect, proj: &crate::viewport::Proj) -> Option<egui::Pos2> {
    let ctx = ui.ctx();
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx) {
        return None;
    }
    // Over the view, not the panel lying over it: a card clicked, or let go where it was picked
    // up, inserts nothing.
    let over_panel = |p: &egui::Pos2| app.parts_library.shown.is_some_and(|r| r.contains(*p)) || crate::shell::covered(app, *p);
    let pos = ctx.pointer_hover_pos().filter(|p| rect.contains(*p) && !over_panel(p))?;
    let at = drop_point(app, proj, pos);
    if ctx.input(|i| i.pointer.primary_released())
        && let Some(d) = egui::DragAndDrop::take_payload::<Dragged>(ctx)
    {
        match insert(app, &d.0, at) {
            Ok(()) => {
                let name = d.0.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_string());
                app.set_status(solidcraft_ui_text::format("parts_library.inserted", &[("name", &name)]), false);
            }
            Err(e) => app.set_status(e, true),
        }
    }
    proj.to_screen(at)
}

/// The mark where a dragged part will land.
pub fn paint_drop(painter: &egui::Painter, at: egui::Pos2) {
    painter.circle_stroke(at, Tokens::get().space_6, egui::Stroke::new(2.0, crate::workspace::tab_accent(&Tokens::get(), "MODEL").color));
}

/// While a part is dragged: its name in a chip by the pointer, so it is clear what will be
/// inserted. Tokens: the tooltip frame.
fn drag_chip(ui: &egui::Ui, e: &Entry) {
    let Some(at) = ui.ctx().pointer_interact_pos() else { return };
    let t = Tokens::get();
    let container = Container::new(ui.ctx(), Surface::Popup);
    container
        .area(egui::Id::unique("sc_library_drag"))
        .order(egui::Order::Tooltip)
        .fixed_pos(at + egui::vec2(t.space_14, t.space_10))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            container.tooltip_frame(ui.ctx()).show(ui, |ui| {
                ui.label(egui::RichText::new(&e.name).color(t.form_ink).size(t.font_small));
            });
        });
}

/// What the card asked for this frame, done once it is drawn.
#[derive(Default)]
struct Acts {
    close: bool,
    refresh: bool,
    choose: bool,
    design_folder: bool,
    enter: Option<PathBuf>,
    open: Option<PathBuf>,
    insert: Option<PathBuf>,
    select: Option<PathBuf>,
}

/// The shared panel's body: the folder's breadcrumb, the search, the size, the cards and the hint.
/// The list scrolls inside the room the host gives it; the panel reports its natural height, so
/// a short folder makes a short panel.
pub(crate) fn shared_content(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.parts_library.shown = Some(ui.max_rect());
    let mut acts = Acts::default();
    let top = ui.cursor().top();
    let t = Tokens::get();
    let inset = egui::Margin::symmetric(t.space_12 as i8, t.space_6 as i8);
    let natural = egui::Frame::NONE.inner_margin(inset).show(ui, |ui| crate::ui_kit::catalog_scope(ui, |ui| contents(app, ui, &mut acts))).inner;
    crate::panel_runtime::report_content_extent(ui, top, natural + 2.0 * t.space_6, false);
    take(app, &ctx, acts);
}

/// The app bar's documents button (`|||`) is there: a desktop build with a folder picker, outside
/// the Start page.
pub(crate) fn available(app: &SolveApp) -> bool {
    !cfg!(target_arch = "wasm32") && app.services.pick_folder.is_some() && !app.home.open
}

/// Show or hide the panel, as the app bar's documents button and `view.parts_library` do.
pub(crate) fn toggle(app: &mut SolveApp) {
    app.preferences.library_open = !app.preferences.library_open;
}

/// The button's name, for its tooltip and the app bar's overflow menu.
pub(crate) fn toggle_label(app: &SolveApp) -> &'static str {
    solidcraft_ui_text::tr(if app.preferences.library_open { "parts_library.hide" } else { "parts_library.show" })
}

/// The app bar's documents button (`|||`): shows and hides the parts library. Returns whether it is
/// there; the shared bar renderer paints it from the response this leaves.
pub(crate) fn app_bar_toggle(app: &mut SolveApp, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    if !available(app) {
        return false;
    }
    let t = Tokens::get();
    let id = ui.scope_id().with("document_browser_toggle");
    let response = ui.interact(rect, id, egui::Sense::click());
    let label = toggle_label(app);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    crate::scenario::publish_handle("library:toggle", rect.center());
    crate::scenario::publish_rect("library:toggle", rect);
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 4.0, t.app_bar_hover);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect.shrink(1.0), 4.0, egui::Stroke::new(1.0, t.app_bar_text), egui::StrokeKind::Inside);
    }
    crate::icons::paint_app_bar(ui.painter(), rect.shrink(4.0), "document_browser", 1.0);
    if response.on_hover_text(label).clicked() {
        ui.memory_mut(|memory| memory.request_focus(id));
        toggle(app);
    }
    true
}

/// A header action of the shared panel: read the folder again, choose another, or the open
/// design's folder.
pub(crate) fn header_action(app: &mut SolveApp, id: &str) {
    match id {
        // The next tick reads it again.
        "refresh" => app.parts_library.listed = None,
        "choose-folder" => choose_folder(app),
        "design-folder" => {
            if let Some(dir) = app.session.path.as_deref().and_then(|p| Path::new(p).parent()).map(|d| d.to_string_lossy().to_string()) {
                set_folder(app, &dir);
            }
        }
        _ => {}
    }
}

fn take(app: &mut SolveApp, ctx: &egui::Context, acts: Acts) {
    if acts.close {
        app.preferences.library_open = false;
    }
    if acts.refresh {
        refresh(app, ctx, true);
    }
    if acts.choose {
        choose_folder(app);
    }
    if acts.design_folder
        && let Some(dir) = app.session.path.as_deref().and_then(|p| Path::new(p).parent()).map(|d| d.to_string_lossy().to_string())
    {
        set_folder(app, &dir);
    }
    if let Some(p) = acts.select {
        // A second click on the selected card lets it go.
        app.parts_library.selected = if app.parts_library.selected.as_ref() == Some(&p) { None } else { Some(p) };
    }
    if let Some(dir) = acts.enter {
        enter(app, ctx, dir);
    }
    if let Some(p) = acts.insert
        && let Err(e) = insert(app, &p, Vec3::ZERO)
    {
        app.set_status(e, true);
    }
    if let Some(p) = acts.open {
        crate::home::open_recent(app, &p.to_string_lossy());
    }
}

/// The shown folder's entries the search leaves.
fn filtered(app: &SolveApp) -> Vec<Entry> {
    let filter = app.parts_library.filter.to_lowercase();
    app.parts_library.entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter)).cloned().collect()
}

/// Returns the body's natural height: its fixed bands plus the whole list, however much of it
/// shows.
fn contents(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Acts) -> f32 {
    let t = Tokens::get();
    let tr = solidcraft_ui_text::tr;
    let top = ui.cursor().top();
    let Some(root) = root(app) else {
        no_folder(app, ui, acts);
        footer(ui);
        return ui.cursor().top() - top;
    };
    let here = current(app).unwrap_or_else(|| root.clone());
    breadcrumb(app, ui, &root, &here, acts);
    search(app, ui, acts);
    let mut size = LibrarySize::ALL.iter().position(|s| *s == app.preferences.library_size).unwrap_or(1);
    let labels: Vec<&str> = LibrarySize::ALL.iter().map(|s| tr(s.label_id())).collect();
    if crate::ui_kit::segmented(
        ui,
        &mut size,
        &labels,
        crate::ui_kit::Segments {
            targets: &["library:size_small", "library:size_medium", "library:size_large", "library:size_list"],
            ..Default::default()
        },
    ) {
        app.preferences.library_size = LibrarySize::ALL[size];
    }
    let room = ui.available_height() - crate::ui_kit::catalog_footer_height(ui) - t.space_8;
    let list = egui::ScrollArea::vertical().id_salt("library_list").max_height(room.max(0.0)).auto_shrink([false, true]);
    // How much taller the whole list is than the part of it that shows.
    let mut hidden = 0.0;
    if let Some(error) = app.parts_library.error.clone() {
        unreadable(ui, &error, acts);
    } else if reading(app) {
        let out = list.show(ui, |ui| placeholders(app, ui));
        hidden = (out.content_size.y - out.inner_rect.height()).max(0.0);
    } else if app.parts_library.entries.is_empty() {
        state(ui, "folder", None, tr("parts_library.empty"));
        if crate::ui_kit::button(ui, tr("parts_library.choose_another"), crate::ui_kit::Button::Normal).clicked() {
            acts.choose = true;
        }
    } else {
        let entries = filtered(app);
        if entries.is_empty() {
            state(ui, "folder", Some(t.form_dim), tr("parts_library.no_match"));
            let clear = crate::ui_kit::button(ui, tr("parts_library.clear_search"), crate::ui_kit::Button::Normal);
            crate::scenario::publish_handle("library:clear_search", clear.rect.center());
            if clear.clicked() {
                app.parts_library.filter.clear();
            }
        } else {
            // Only the rows in sight are laid out, and only their pictures drawn.
            let grid =
                card_size(app.preferences.library_size).map(|size| Grid::new(size, ui.available_width() - SCROLL_BAR, ui.spacing().item_spacing.y));
            let now = ui.input(|i| i.time);
            if let (Some(was), Some(grid)) = (app.parts_library.grid, grid)
                && !was.same_columns(&grid)
            {
                app.parts_library.slide = Some((was, now));
            }
            app.parts_library.grid = grid;
            if app.parts_library.slide.is_some_and(|(_, at)| now - at >= SLIDE) || grid.is_none() {
                app.parts_library.slide = None;
            }
            let columns = grid.map_or(1, |g| g.columns);
            let row = grid.map_or(t.control_height, |g| g.size.height());
            let out = list.show_rows(ui, row, entries.len().div_ceil(columns), |ui, rows| list_view(app, ui, acts, &entries, rows));
            hidden = (out.content_size.y - out.inner_rect.height()).max(0.0);
            if hidden - out.state.clamped_offset().y > 1.0 {
                fade_out_below(ui, out.inner_rect, t.panel);
            }
        }
    }
    footer(ui);
    ui.cursor().top() - top + hidden
}

/// The cards fade out at the list's foot while more of them lie below.
fn fade_out_below(ui: &egui::Ui, list: egui::Rect, panel: egui::Color32) {
    let height = Tokens::get().space_24.min(list.height() / 3.0);
    let band = egui::Rect::from_min_max(egui::pos2(list.left(), list.bottom() - height), list.max);
    let mut mesh = egui::Mesh::default();
    let clear = egui::Color32::TRANSPARENT;
    mesh.colored_vertex(band.left_top(), clear);
    mesh.colored_vertex(band.right_top(), clear);
    mesh.colored_vertex(band.right_bottom(), panel);
    mesh.colored_vertex(band.left_bottom(), panel);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// No folder chosen yet.
fn no_folder(app: &SolveApp, ui: &mut egui::Ui, acts: &mut Acts) {
    let tr = solidcraft_ui_text::tr;
    state(ui, "folder", None, tr("parts_library.no_folder"));
    let b = crate::ui_kit::button(ui, tr("parts_library.choose"), crate::ui_kit::Button::Primary);
    crate::scenario::publish_handle("library:choose", b.rect.center());
    if b.clicked() {
        acts.choose = true;
    }
    if crate::ui_kit::button(ui, tr("parts_library.design_folder"), crate::ui_kit::Button::Enabled(app.session.path.is_some())).clicked() {
        acts.design_folder = true;
    }
}

/// A folder that can't be read: its message, Try again and another folder. The breadcrumb keeps
/// its path.
fn unreadable(ui: &mut egui::Ui, error: &str, acts: &mut Acts) {
    let tr = solidcraft_ui_text::tr;
    let t = Tokens::get();
    state(ui, "folder", Some(t.error), tr("parts_library.unreadable"));
    ui.label(egui::RichText::new(error).size(t.font_small).color(t.text_dim));
    ui.horizontal(|ui| {
        if crate::ui_kit::button(ui, tr("parts_library.try_again"), crate::ui_kit::Button::Primary).clicked() {
            acts.refresh = true;
        }
        if crate::ui_kit::button(ui, tr("parts_library.choose_another"), crate::ui_kit::Button::Normal).clicked() {
            acts.choose = true;
        }
    });
}

/// A state's icon (in `ink`, else the accent) and its line of text.
fn state(ui: &mut egui::Ui, icon: &str, ink: Option<egui::Color32>, text: &str) {
    let t = Tokens::get();
    let side = t.space_24 + t.space_8;
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), side), egui::Sense::hover());
    let at = egui::Rect::from_center_size(r.center(), egui::vec2(side, side));
    crate::icons::paint_ink(ui.painter(), at, icon, ink.unwrap_or(crate::workspace::tab_accent(&t, "MODEL").color));
    ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).size(t.font_small).color(t.form_ink)));
}

/// The shared panel header's status: the shown folder's item count, once a folder is chosen.
pub(crate) fn status(app: &SolveApp) -> Option<String> {
    root(app).map(|_| item_count(app))
}

/// "N items" ("1 item"); while searching, those the search leaves.
fn item_count(app: &SolveApp) -> String {
    let n = if app.parts_library.filter.is_empty() { app.parts_library.entries.len() } else { filtered(app).len() };
    solidcraft_ui_text::format(if n == 1 { "parts_library.items_one" } else { "parts_library.items" }, &[("n", &n.to_string())])
}

/// Up a level inside a subfolder, the folder button (another folder), the folder shown (cut short;
/// an ellipsis stands for the levels between, named in its tooltip) and the item count on the right.
fn breadcrumb(app: &SolveApp, ui: &mut egui::Ui, root: &Path, here: &Path, acts: &mut Acts) {
    let t = Tokens::get();
    let tr = solidcraft_ui_text::tr;
    let name = |p: &Path| p.file_name().map_or_else(|| p.to_string_lossy().to_string(), |n| n.to_string_lossy().to_string());
    let count = item_count(app);
    let levels: Vec<String> =
        here.strip_prefix(root).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_4;
        let counted = ui.painter().layout_no_wrap(count.clone(), egui::FontId::proportional(t.font_small), t.text_dim).size().x;
        if here != root {
            let up =
                crate::ui_kit::compact_button(ui, crate::ui_kit::MOVE_UP, tr("parts_library.up"), true, false).on_hover_text(tr("parts_library.up"));
            crate::scenario::publish_handle("library:up", up.rect.center());
            if up.clicked() {
                acts.enter = here.parent().map(Path::to_path_buf).filter(|p| p.starts_with(root)).or_else(|| Some(root.to_path_buf()));
            }
        }
        let room = ui.available_width() - counted - t.space_8;
        let folder = crate::ui_kit::folder_button(
            ui,
            &name(root),
            if levels.is_empty() { room } else { room * 0.5 },
            crate::workspace::tab_accent(&t, "MODEL"),
        )
        .on_hover_text(format!("{}\n{}", tr("parts_library.choose_another"), root.to_string_lossy()));
        crate::scenario::publish_handle("library:folder", folder.rect.center());
        if folder.clicked() {
            acts.choose = true;
        }
        let dim = |s: &str| egui::RichText::new(s).size(t.font_small).color(t.text_dim);
        if levels.len() > 1 {
            ui.label(dim("/"));
            ui.label(dim("…")).on_hover_text(levels[..levels.len() - 1].join("/"));
        }
        if let Some(last) = levels.last() {
            ui.label(dim("/"));
            let left = (ui.available_width() - counted - t.space_8).max(t.space_24);
            ui.scope(|ui| {
                ui.set_max_width(left);
                ui.add(egui::Label::new(egui::RichText::new(last).size(t.font_small).color(t.form_ink)).truncate());
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(dim(&count));
        });
    });
}

/// The search field. Esc clears it, then closes the panel; Ctrl+F or `/` over the panel focuses it;
/// F5 reads the folder again.
fn search(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Acts) {
    let id = egui::Id::unique("library_search");
    let r = crate::ui_kit::search_field(ui, id, &mut app.parts_library.filter, solidcraft_ui_text::tr("parts_library.search"));
    crate::scenario::publish_handle("library:search", r.rect.center());
    let over = app.parts_library.shown.is_some_and(|c| ui.ctx().pointer_hover_pos().is_some_and(|p| c.contains(p)));
    let nothing_focused = ui.ctx().memory(|m| m.focused().is_none());
    if over && nothing_focused && ui.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::F)) || i.key_pressed(egui::Key::Slash)) {
        r.request_focus();
    }
    if nothing_focused && ui.input(|i| i.key_pressed(egui::Key::F5)) {
        acts.refresh = true;
    }
    if (r.has_focus() || r.lost_focus()) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        // The search's own Esc: no tool or selection is cancelled with it.
        app.esc_handled = true;
        if app.parts_library.filter.is_empty() {
            acts.close = true;
        } else {
            app.parts_library.filter.clear();
            r.request_focus();
        }
    }
}

/// Cards at their final size while the folder is read.
fn placeholders(app: &SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let room = ui.available_width() - SCROLL_BAR;
    match card_size(app.preferences.library_size) {
        Some(size) => {
            let grid = Grid::new(size, room, ui.spacing().item_spacing.y);
            for row in 0..PLACEHOLDERS.div_ceil(grid.columns) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = PartCardSize::gap();
                    ui.add_space(grid.left);
                    for _ in 0..grid.columns.min(PLACEHOLDERS - row * grid.columns) {
                        crate::ui_kit::part_card(
                            ui,
                            PartCard {
                                name: "",
                                face: PartFace::Placeholder,
                                size,
                                width: grid.width,
                                selected: false,
                                dragging: false,
                                accent: crate::workspace::tab_accent(&t, "MODEL"),
                            },
                        );
                    }
                });
            }
        }
        None => {
            for _ in 0..PLACEHOLDERS {
                let (r, _) = ui.allocate_exact_size(egui::vec2(room, t.control_height), egui::Sense::hover());
                ui.painter().rect_filled(r.shrink2(egui::vec2(0.0, t.space_6)), t.radius_sm, t.dialog_footer);
            }
        }
    }
}

/// The `rows` of `entries` in sight: cards in the size's columns, or names.
fn list_view(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Acts, entries: &[Entry], rows: std::ops::Range<usize>) {
    let t = Tokens::get();
    let tr = solidcraft_ui_text::tr;
    let grid = app.parts_library.grid;
    let dragging = egui::DragAndDrop::payload::<Dragged>(ui.ctx()).map(|d| d.0.clone());
    let columns = grid.map_or(1, |g| g.columns);
    let room = ui.available_width() - SCROLL_BAR;
    let accent = crate::workspace::tab_accent(&t, "MODEL");
    let slide = app.parts_library.slide.map(|(from, at)| (from, ((ui.input(|i| i.time) - at) / SLIDE) as f32));
    if slide.is_some() {
        ui.ctx().request_repaint();
    }
    for row in rows {
        let first = row * columns;
        let row = &entries[first..((row + 1) * columns).min(entries.len())];
        // A row of cards takes its full height in the list; each card is drawn at its place in
        // the centred grid, or on its way there while the columns change.
        let band = grid.map(|g| {
            let (band, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), g.size.height()), egui::Sense::hover());
            egui::pos2(band.left(), band.top() - (first / g.columns) as f32 * g.row())
        });
        {
            for (k, e) in row.iter().enumerate() {
                let badge = match e.kind {
                    Kind::Step => Some("STEP"),
                    Kind::Mesh => Some("MESH"),
                    _ => None,
                };
                let selected = app.parts_library.selected.as_ref() == Some(&e.path);
                let r = match grid {
                    Some(grid) => {
                        let size = grid.size;
                        let i = first + k;
                        let (offset, opacity) = match slide {
                            Some((from, p)) => slid(&from, &grid, i, p, room),
                            None => (grid.at(i), 1.0),
                        };
                        let place = egui::Rect::from_min_size(band.unwrap_or_default() + offset, egui::vec2(grid.width, size.height()));
                        let id = if e.count == 1 { "parts_library.folder_parts_one" } else { "parts_library.folder_parts" };
                        let parts = solidcraft_ui_text::format(id, &[("n", &e.count.to_string())]);
                        let face = if e.kind == Kind::Folder {
                            PartFace::Folder { count: &parts }
                        } else {
                            let key = e.path.to_string_lossy().to_string();
                            PartFace::Part { picture: thumb(&mut app.parts_library, &key), badge }
                        };
                        let dragging = dragging.as_ref() == Some(&e.path);
                        let mut card = ui.new_child(egui::UiBuilder::new().max_rect(place).id_salt(&e.path));
                        card.set_opacity(opacity);
                        crate::ui_kit::part_card(&mut card, PartCard { name: &e.name, face, size, width: grid.width, selected, dragging, accent })
                    }
                    None => {
                        ui.set_width(room);
                        let icon = match e.kind {
                            Kind::Folder => "folder",
                            Kind::Mesh => "mesh_body",
                            _ => "body",
                        };
                        crate::ui_kit::part_row(ui, &e.name, icon, badge, selected, accent)
                    }
                };
                crate::scenario::publish_handle(&format!("library:{}", e.name), r.rect.center());
                if e.kind == Kind::Folder {
                    if r.clicked() {
                        acts.enter = Some(e.path.clone());
                    }
                    continue;
                }
                if r.clicked() {
                    acts.select = Some(e.path.clone());
                }
                // The card's own response carries the drag (an overlaid drag source took the
                // pointer from it: no right click, no double click, #4432): a primary drag carries
                // the part to the view, which inserts it where it is let go; a double click or the
                // menu's Open opens it as its own document (a STEP or mesh file too).
                if r.drag_started_by(egui::PointerButton::Primary) {
                    egui::DragAndDrop::set_payload(ui.ctx(), Dragged(e.path.clone()));
                }
                if r.dragged_by(egui::PointerButton::Primary) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    drag_chip(ui, e);
                }
                if r.double_clicked() {
                    acts.open = Some(e.path.clone());
                }
                let insert = if e.kind == Kind::Design { tr("parts_library.insert") } else { tr("parts_library.import") };
                let mut insert_item = crate::ui_kit::Item::action("library:menu_insert", insert, "");
                insert_item.aliases.push("library:menu_insert".into());
                let mut open_item = crate::ui_kit::Item::action("library:menu_open", tr("parts_library.open"), "");
                open_item.aliases.push("library:menu_open".into());
                if let Some(picked) = crate::ui_kit::Menu::context_items(&r, &[insert_item, open_item]) {
                    match picked.id.as_str() {
                        "library:menu_insert" => acts.insert = Some(e.path.clone()),
                        "library:menu_open" => acts.open = Some(e.path.clone()),
                        _ => {}
                    }
                }
            }
        }
    }
}

/// The kit's catalog footer: the hint's first sentence, the whole hint on hover.
fn footer(ui: &mut egui::Ui) {
    let tr = solidcraft_ui_text::tr;
    crate::ui_kit::catalog_footer_with_tip(ui, tr("parts_library.hint_short"), tr("parts_library.hint"));
}

/// Esc while the search has the focus is the search's own, whatever runs before the card this frame.
pub(crate) fn takes_esc(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.has_focus(egui::Id::unique("library_search"))) && ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

/// `ui.library`: show or hide the card (`open`), set its card `size`, its `folder`, browse into a
/// `subfolder` of it, or `insert` a listed part (`at` mm, default the origin). Returns the card's
/// state and what the shown folder lists.
pub fn control(app: &mut SolveApp, p: &serde_json::Value) -> Result<serde_json::Value, String> {
    if let Some(folder) = p.get("folder").and_then(serde_json::Value::as_str) {
        if !Path::new(folder).is_dir() {
            return Err(format!("{folder} is not a folder"));
        }
        set_folder(app, folder);
        let listing = list(Path::new(folder));
        take_listing(app, (PathBuf::from(folder), listing));
    }
    if let Some(b) = p.get("open").and_then(serde_json::Value::as_bool) {
        app.preferences.library_open = b;
    }
    if let Some(v) = p.get("size") {
        app.preferences.library_size = v.as_str().and_then(LibrarySize::from_id).ok_or("`size` must be small, medium, large or list")?;
    }
    if let Some(sub) = p.get("subfolder").and_then(serde_json::Value::as_str) {
        let root = root(app).ok_or("no library folder set")?;
        // Only names of folders inside it: no `..`, no absolute or drive paths.
        let plain = Path::new(sub).components().all(|c| matches!(c, std::path::Component::Normal(_)));
        let dir = root.join(sub);
        if !plain || !dir.is_dir() {
            return Err(format!("{sub} is not a folder in the library"));
        }
        app.preferences.library_subfolder = sub.replace('\\', "/");
        app.parts_library.current = Some(dir.clone());
        take_listing(app, (dir.clone(), list(&dir)));
    }
    if let Some(name) = p.get("insert").and_then(serde_json::Value::as_str) {
        let e = app
            .parts_library
            .entries
            .iter()
            .find(|e| e.name == name && e.kind != Kind::Folder)
            .cloned()
            .ok_or_else(|| format!("no part `{name}` in the shown folder"))?;
        let at = p
            .get("at")
            .and_then(|v| v.as_array())
            .map(|a| {
                Vec3::new(
                    a.first().and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                    a.get(1).and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                    a.get(2).and_then(serde_json::Value::as_f64).unwrap_or(0.0),
                )
            })
            .unwrap_or(Vec3::ZERO);
        insert_allowing(app, &e.path, at, &[])?;
    }
    let kind = |k: Kind| match k {
        Kind::Folder => "folder",
        Kind::Design => "design",
        Kind::Step => "step",
        Kind::Mesh => "mesh",
    };
    Ok(json!({
        "open": app.preferences.library_open,
        "size": app.preferences.library_size.id(),
        "folder": app.preferences.library_folder,
        "shown": current(app).map(|p| p.to_string_lossy().to_string()),
        "entries": app.parts_library.entries.iter().map(|e| json!({"name": e.name, "kind": kind(e.kind), "path": e.path.to_string_lossy()})).collect::<Vec<_>>(),
        "error": app.parts_library.error,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of parts: two designs, a STEP file, a subfolder, and files the library leaves out.
    fn parts(dir: &Path) {
        let mut s = solidcraft_engine::Session::default();
        s.execute("solid.box", &json!({"length": 10, "width": 20, "height": 30})).unwrap();
        s.execute("file.save", &json!({"path": dir.join("bracket.solidcraft")})).unwrap();
        s.execute("file.export", &json!({"path": dir.join("adapter.step")})).unwrap();
        let mut b = solidcraft_engine::Session::default();
        b.execute("solid.cylinder", &json!({"radius": 3, "height": 12})).unwrap();
        std::fs::create_dir_all(dir.join("Fasteners")).unwrap();
        b.execute("file.save", &json!({"path": dir.join("Fasteners").join("pin.solidcraft")})).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a part").unwrap();
        std::fs::write(dir.join(".hidden.step"), "x").unwrap();
    }

    #[test]
    fn a_folder_lists_its_subfolders_then_its_designs_and_parts() {
        let h = crate::scenario::Harness::new();
        parts(&h.dir);
        let entries = list(&h.dir).unwrap();
        let named: Vec<(&str, Kind)> = entries.iter().map(|e| (e.name.as_str(), e.kind)).collect();
        assert_eq!(named.first(), Some(&("Fasteners", Kind::Folder)), "{named:?}");
        assert_eq!(entries[0].count, 1, "the folder card says how many parts it holds");
        assert!(named.contains(&("bracket", Kind::Design)) && named.contains(&("adapter", Kind::Step)), "{named:?}");
        assert!(!named.iter().any(|(n, _)| n.contains("notes") || n.contains("hidden")), "{named:?}");
        assert!(list(&h.dir.join("missing")).is_err());
    }

    /// The card through `ui.library`: a folder chosen, shown, browsed into (and the subfolder
    /// remembered), a part inserted where asked as a linked component, the size changed; hidden
    /// again. The folder stays in the preferences.
    #[test]
    fn the_library_lists_a_folder_and_inserts_its_parts() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        let dir = h.dir.to_string_lossy().to_string();
        let r = control(&mut h.app, &json!({"folder": dir})).unwrap();
        assert_eq!(r["open"], true);
        assert!(r["entries"].as_array().unwrap().iter().any(|e| e["name"] == "bracket" && e["kind"] == "design"), "{r}");
        h.frames(3);
        assert!(h.app.parts_library.shown.is_some(), "the card shows");
        assert!(h.app.prefs().contains("library_folder"), "the folder is kept with the preferences");
        control(&mut h.app, &json!({"insert": "bracket", "at": [50, 0, 0]})).unwrap();
        let links = h.app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["name"], "bracket", "{links}");
        let b = solidcraft_engine::view::bounds(&h.app.session).unwrap();
        assert!((b.min.x - 50.0).abs() < 1e-6, "placed where asked: {b:?}");
        let sub = control(&mut h.app, &json!({"subfolder": "Fasteners"})).unwrap();
        assert_eq!(sub["entries"][0]["name"], "pin", "{sub}");
        assert_eq!(h.app.preferences.library_subfolder, "Fasteners", "the subfolder is remembered");
        assert!(control(&mut h.app, &json!({"subfolder": ".."})).is_err(), "never above the library folder");
        assert!(control(&mut h.app, &json!({"insert": "nope"})).is_err());
        assert_eq!(control(&mut h.app, &json!({"size": "list"})).unwrap()["size"], "list");
        assert!(control(&mut h.app, &json!({"size": "huge"})).is_err());
        control(&mut h.app, &json!({"open": false})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.shown.is_none(), "hidden");
        assert!(control(&mut h.app, &json!({"folder": h.dir.join("bracket.solidcraft").to_string_lossy()})).is_err(), "a file is not a folder");
    }

    /// Frames until a file being opened has opened (or a minute has passed).
    fn opened(h: &mut crate::scenario::Harness) -> Option<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        h.frames(2);
        while h.app.file_load.busy() && std::time::Instant::now() < deadline {
            h.frames(1);
        }
        h.frames(2);
        h.app.session.path.clone()
    }

    /// A double click on a card opens its part as its own document, and a right click offers
    /// Insert and Open: the card's own response carries the drag, so nothing takes the pointer
    /// from it (an overlaid drag source did, and neither worked).
    #[test]
    fn a_double_click_or_the_right_click_menu_opens_a_part_on_its_own() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        // Past the cards' slide to their places, so the click lands on the card it aims at.
        h.frames(30);
        let at = crate::scenario::handle_at("library:bracket").expect("the bracket's card");
        h.step(&json!({"click": [at.x, at.y], "double": true})).unwrap();
        let path = opened(&mut h).expect("a double click opens it");
        assert!(path.ends_with("bracket.solidcraft"), "{path}");
        assert_eq!(h.app.session.execute("component.links", &json!({})).unwrap()["links"].as_array().map_or(0, Vec::len), 0, "opened, not inserted");

        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        // Past the cards' slide to their places, so the click lands on the card it aims at.
        h.frames(30);
        let at = crate::scenario::handle_at("library:adapter").expect("the STEP file's card");
        h.step(&json!({"click": [at.x, at.y], "button": "right"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("library:menu_insert").is_some(), "the right click shows the menu");
        let open = crate::scenario::handle_at("library:menu_open").expect("its Open");
        h.step(&json!({"click": [open.x, open.y]})).unwrap();
        // A STEP file asks its import options first: the defaults. The window opens once the
        // load reaches it.
        for _ in 0..600 {
            if crate::scenario::handle_at("import_options:ok").is_some() {
                break;
            }
            h.frames(1);
        }
        h.step(&json!({"click": {"handle": "import_options:ok"}})).unwrap();
        // A STEP file opens as a new design holding it as a component named after it.
        opened(&mut h);
        let occurrences = h.app.session.execute("inspect.measure", &json!({})).unwrap();
        let named = serde_json::to_string(&occurrences).unwrap_or_default().to_lowercase();
        assert!(named.contains("adapter") && !h.app.session.world_state().unwrap().bodies.is_empty(), "Open opened the STEP file: {occurrences}");
    }

    /// The size sets the columns: three Small, two Medium, one Large across; List is one name per
    /// row. Its segment picks it.
    #[test]
    fn the_size_lays_the_cards_out_in_its_columns() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        // Folders first: Fasteners, then adapter and bracket.
        let at = |h: &mut crate::scenario::Harness, size: LibrarySize| {
            let segment = crate::scenario::handle_at(&format!("library:size_{}", size.id())).expect("the size's segment");
            h.step(&json!({"click": [segment.x, segment.y]})).unwrap();
            // Past the cards' slide to their new places.
            h.frames(30);
            assert_eq!(h.app.preferences.library_size, size);
            let f = crate::scenario::handle_at("library:Fasteners").unwrap();
            let a = crate::scenario::handle_at("library:adapter").unwrap();
            let b = crate::scenario::handle_at("library:bracket").unwrap();
            (f, a, b)
        };
        let (f, a, b) = at(&mut h, LibrarySize::Small);
        assert!((f.y - a.y).abs() < 1.0 && (a.y - b.y).abs() < 1.0, "small: three across {f:?} {a:?} {b:?}");
        let (f, a, b) = at(&mut h, LibrarySize::Medium);
        assert!((f.y - a.y).abs() < 1.0 && b.y - a.y > PartCardSize::Medium.picture(), "medium: two across {f:?} {a:?} {b:?}");
        let (f, a, _) = at(&mut h, LibrarySize::Large);
        assert!((f.x - a.x).abs() < 1.0 && a.y - f.y > PartCardSize::Large.picture(), "large: one across {f:?} {a:?}");
        let (f, a, _) = at(&mut h, LibrarySize::List);
        let row = Tokens::get().control_height;
        assert!((f.x - a.x).abs() < 1.0 && (a.y - f.y - row).abs() < 9.0, "list: a short row each {f:?} {a:?}");
    }

    #[test]
    fn a_second_click_or_escape_lets_the_selected_card_go() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        h.app.home.open = false;
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        // Past the cards' slide to their places, so the click lands on the card it aims at.
        h.frames(30);
        let at = crate::scenario::handle_at("library:adapter").expect("the STEP file's card");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.selected.as_ref().is_some_and(|p| p.ends_with("adapter.step")), "a click selects the card");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.selected.is_none(), "a second click lets it go");
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.selected.is_some(), "a third click selects it again");
        h.step(&json!({"move": [at.x, at.y]})).unwrap();
        h.step(&json!({"key": "Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.selected.is_none(), "Escape over the library lets it go");
    }

    #[test]
    fn cards_widen_a_little_to_close_the_side_margins() {
        for size in [PartCardSize::Small, PartCardSize::Medium, PartCardSize::Large] {
            let gap = PartCardSize::gap();
            for room in (0..60).map(|step| size.width() + step as f32 * 7.0) {
                let grid = Grid::new(size, room, 0.0);
                let span = grid.width * grid.columns as f32 + gap * (grid.columns - 1) as f32;
                assert!(span <= room + 0.5, "{size:?} in {room}: {span} overflows");
                assert!(grid.width <= size.width() * CARD_STRETCH + 0.01, "{size:?} in {room}: {} too wide", grid.width);
                if grid.width < size.width() * CARD_STRETCH - 0.01 {
                    assert!(grid.left < 0.5, "{size:?} in {room}: margin {} left while the cards could still widen", grid.left);
                }
            }
        }
    }

    /// A wider panel fits more columns, centred, and the cards slide to their new places; a
    /// narrower one takes the columns away again. Cards widen at most a quarter.
    #[test]
    fn a_wider_panel_adds_columns_of_the_same_cards() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        h.app.home.open = false;
        h.app.preferences.library_size = LibrarySize::Medium;
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(30);
        let cards = || ["Fasteners", "adapter", "bracket"].map(|n| crate::scenario::handle_at(&format!("library:{n}")).expect(n));
        let gap = PartCardSize::gap();
        let pitch_ok =
            |pitch: f32| pitch >= PartCardSize::Medium.width() + gap - 1.0 && pitch <= PartCardSize::Medium.width() * CARD_STRETCH + gap + 1.0;
        let [f, a, b] = cards();
        assert!((f.y - a.y).abs() < 1.0 && b.y > a.y + PartCardSize::Medium.picture(), "two across at the default width {f:?} {a:?} {b:?}");
        assert!(pitch_ok(a.x - f.x), "the cards widen at most a little {f:?} {a:?}");
        let resize = |h: &mut crate::scenario::Harness, width: f32| {
            h.app
                .run_panel_command(
                    "panel.resize",
                    &json!({"target": {"type": "instance", "id": "parts-library"}, "size": {"width": width, "height": 400}, "thickness_only": true}),
                )
                .unwrap();
        };
        resize(&mut h, 700.0);
        h.frames(4);
        // bracket moves to the new third column: it slides in from the right along its new row.
        let [_, _, mid] = cards();
        h.frames(30);
        let [f2, a2, b2] = cards();
        assert!(mid.x > b2.x + 1.0 && (mid.y - b2.y).abs() < 1.0, "the new column slides in, not jumps {mid:?} -> {b2:?}");
        let (_, panel) = placed(&h, "parts-library").expect("the library's section");
        assert!((f2.y - a2.y).abs() < 1.0 && (a2.y - b2.y).abs() < 1.0, "three across when wide {f2:?} {a2:?} {b2:?}");
        assert!(
            pitch_ok(a2.x - f2.x) && ((b2.x - a2.x) - (a2.x - f2.x)).abs() < 1.0,
            "even columns, cards widened at most a little {f2:?} {a2:?} {b2:?}"
        );
        // Four Medium cards fit across 700 px, but there are three: the grid is centred on its
        // columns, so the row sits left of the middle by half a column at most.
        let columns = PartCardSize::Medium.columns_in(panel.width() - 2.0 * Tokens::get().space_8 - SCROLL_BAR);
        assert!(columns >= 4, "{columns} columns in {panel:?}");
        let middle = (f2.x + b2.x) / 2.0;
        assert!(middle < panel.center().x + 1.0 && middle > panel.left() + PartCardSize::Medium.width(), "centred columns {middle} in {panel:?}");
        resize(&mut h, CARD_WIDTH);
        h.frames(30);
        let [f3, a3, b3] = cards();
        assert!((f3.y - a3.y).abs() < 1.0 && b3.y > a3.y + PartCardSize::Medium.picture(), "back to two across {f3:?} {a3:?} {b3:?}");
    }

    /// Where the shared host placed `instance` this frame: its zone and outer rect.
    fn placed(h: &crate::scenario::Harness, instance: &str) -> Option<(solidcraft_engine::panels::Zone, egui::Rect)> {
        let frame = h.app.universal_panel_frame()?;
        frame.layout.sections.iter().find(|s| s.instance.0 == instance).map(|s| (s.zone, crate::panel_runtime::egui_rect(s.outer)))
    }

    /// Whether the layout asks for the library panel to be open.
    fn requested_open(h: &crate::scenario::Harness) -> bool {
        h.app.panel_runtime.as_ref().is_some_and(|integration| {
            integration.runtime().layout().layout().instances.iter().any(|instance| instance.id.0 == "parts-library" && instance.requested_open)
        })
    }

    /// The library is a shared panel: docked left, in its own column at the window's edge outside
    /// the Browser, hugging its contents (a shorter folder, a shorter panel), with its controls
    /// inside it.
    #[test]
    fn the_library_is_a_shared_panel_left_of_the_browser() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        h.app.home.open = false;
        h.app.ui.show_browser = true;
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(6);
        assert!(requested_open(&h), "the toggle opens the shared panel");
        let (zone, card) = placed(&h, "parts-library").expect("the library's section");
        assert_eq!(zone, solidcraft_engine::panels::Zone::Left);
        let (_, browser) = placed(&h, "browser").expect("the Browser's section");
        assert!(card.right() <= browser.left() + 0.5, "the library is outside the Browser: {card:?} {browser:?}");
        assert!(h.app.parts_library.shown.is_some_and(|body| card.contains(body.center())), "its body paints inside its section");
        for what in ["folder", "search"] {
            let at = crate::scenario::handle_at(&format!("library:{what}")).expect(what);
            assert!(card.contains(at), "{what} inside the panel");
        }
        let bottom = card.bottom();
        control(&mut h.app, &json!({"subfolder": "Fasteners"})).unwrap();
        h.frames(6);
        let (_, card) = placed(&h, "parts-library").expect("the library's section");
        assert!(card.bottom() < bottom, "fewer parts, a shorter panel: {bottom} -> {}", card.bottom());
        assert!(crate::scenario::handle_at("library:up").is_some_and(|at| card.contains(at)), "Up inside the panel");
    }

    /// `view.parts_library` opens and closes the panel; closing it from the shared host clears
    /// the preference too, and the header actions are the library's. It is no longer on the Insert
    /// tab: the app bar's documents button shows it.
    #[test]
    fn the_command_opens_and_closes_the_card() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let on_insert = crate::workspace::SECTIONS.iter().any(|s| s.ids.contains(&"view.parts_library"));
        assert!(!on_insert, "not on the ribbon");
        h.app.run("view.parts_library", json!({})).unwrap();
        h.frames(4);
        assert!(h.app.preferences.library_open && requested_open(&h) && h.app.parts_library.shown.is_some(), "opened");
        assert!(crate::scenario::handle_at("library:choose").is_some(), "with no folder it offers Choose folder…");
        h.app.run("view.parts_library", json!({})).unwrap();
        h.frames(4);
        assert!(!h.app.preferences.library_open && !requested_open(&h) && h.app.parts_library.shown.is_none(), "closed");
        h.app.run("view.parts_library", json!({"visible": true})).unwrap();
        h.frames(4);
        assert!(requested_open(&h), "open again");
        let instance = solidcraft_engine::panels::InstanceId::new("parts-library").unwrap();
        h.app
            .dispatch_panel_layout(solidcraft_engine::panels::LayoutAction::Apply {
                expected_revision: None,
                mutations: vec![solidcraft_engine::panels::Mutation::Close { instance }],
            })
            .unwrap();
        h.frames(4);
        assert!(!h.app.preferences.library_open, "the host's close clears the preference");
        assert!(h.app.parts_library.shown.is_none(), "and the panel is gone");
    }

    /// The app bar's documents button (`|||`, left of Home and File) shows and hides the parts
    /// library, in the left-hand place the documents browser had; the keyboard works it too, and
    /// the Start page has no button.
    #[test]
    fn the_app_bar_documents_button_shows_and_hides_the_library() {
        let mut h = crate::scenario::Harness::new();
        h.app.services.pick_folder = Some(Box::new(|_: &str| Box::pin(std::future::ready(None))));
        h.app.home.open = false;
        h.frames(3);
        assert!(!h.app.preferences.library_open, "hidden until asked for");
        let toggle = crate::scenario::rect_of("library:toggle").expect("the documents button");
        let home = crate::scenario::rect_of("appbar:home").unwrap();
        let file = crate::scenario::rect_of("appbar:file").unwrap();
        assert!(toggle.right() < home.left() && home.right() <= file.left(), "{toggle:?} {home:?} {file:?}");
        assert!(h.widget("Show parts library").is_some(), "it is named for what it does");
        h.step(&json!({"click": {"handle": "library:toggle"}})).unwrap();
        h.frames(4);
        assert!(h.app.preferences.library_open && requested_open(&h), "shown");
        let (zone, card) = placed(&h, "parts-library").expect("the library's section");
        assert_eq!(zone, solidcraft_engine::panels::Zone::Left);
        let viewport = h.app.viewport.rect.unwrap();
        assert!(card.left() <= viewport.left() + 16.0 && card.right() < viewport.center().x, "at the view's left edge: {card:?} {viewport:?}");
        assert!(toggle.bottom() <= card.top(), "under the app bar");
        assert!(h.widget("Hide parts library").is_some());
        h.step(&json!({"key": "Enter"})).unwrap();
        h.frames(4);
        assert!(!h.app.preferences.library_open && !requested_open(&h), "Enter on the focused button hides it");
        assert_eq!(crate::scenario::rect_of("library:toggle"), Some(toggle), "the button stays put");
        h.app.home.open = true;
        h.frames(3);
        assert!(crate::scenario::rect_of("library:toggle").is_none(), "no button on the Start page");
    }

    /// A search that leaves nothing says so and clears with one click; the count follows it.
    #[test]
    fn a_search_with_no_match_clears_with_one_click() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        assert_eq!(item_count(&h.app), "3 items");
        h.app.parts_library.filter = "brack".into();
        assert_eq!(item_count(&h.app), "1 item", "the search's count, singular");
        h.app.parts_library.filter = "zzz".into();
        h.frames(2);
        let clear = crate::scenario::handle_at("library:clear_search").expect("Clear search");
        h.step(&json!({"click": [clear.x, clear.y]})).unwrap();
        h.frames(2);
        assert!(h.app.parts_library.filter.is_empty() && crate::scenario::handle_at("library:bracket").is_some(), "cleared");
    }

    /// A folder that can't be read keeps its path and offers Try again.
    #[test]
    fn a_folder_that_cannot_be_read_says_so() {
        let mut h = crate::scenario::Harness::new();
        let gone = h.dir.join("gone");
        std::fs::create_dir_all(&gone).unwrap();
        control(&mut h.app, &json!({"folder": gone.to_string_lossy()})).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        let ctx = h.ctx.clone();
        refresh(&mut h.app, &ctx, true);
        for _ in 0..50 {
            h.frames(1);
            if h.app.parts_library.error.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(h.app.parts_library.error.is_some(), "unreadable");
        h.frames(2);
        assert!(h.widget(solidcraft_ui_text::tr("parts_library.try_again")).is_some(), "Try again");
        assert_eq!(h.app.preferences.library_folder, gone.to_string_lossy(), "the path is kept");
    }

    /// A card let go over the library card (a click, or a drag back to where it started) inserts
    /// nothing, though the view lies under it; let go over the view, it inserts.
    #[test]
    fn a_card_inserts_only_when_let_go_over_the_view() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        let card = h.app.parts_library.shown.expect("the card is drawn");
        let links =
            |h: &mut crate::scenario::Harness| h.app.session.execute("component.links", &json!({})).unwrap()["links"].as_array().map_or(0, Vec::len);
        let bracket = h.dir.join("bracket.solidcraft");
        let right = h.app.shell.browser.map_or(card.right(), |b| b.right());
        for (at, inserted) in [(card.center(), 0), (egui::pos2(right + 200.0, card.center().y), 1)] {
            egui::DragAndDrop::set_payload(&h.ctx, Dragged(bracket.clone()));
            h.step(&json!({"click": [at.x, at.y]})).unwrap();
            h.frames(2);
            assert_eq!(links(&mut h), inserted, "let go at {at:?} (card {card:?})");
        }
    }
}
