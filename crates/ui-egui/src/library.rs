//! The parts library: a floating card at the view's left, beside the Browser, with the designs and
//! parts in a folder on this computer or a server share, each shown by its picture and name. Drag
//! one into the view to put it in the open design: a SolidCraft design becomes a linked component
//! (an occurrence placed where it is dropped, as Insert Design makes), a STEP or mesh file is
//! imported (as Insert STEP or Insert Mesh). Folders open in place; double-click a part to open it
//! on its own. Insert > Parts library shows and hides the card; it remembers its folder, the
//! subfolder last browsed into and the card size. Built from kit pieces only (#8206).

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::prefs::LibrarySize;
use crate::theme::{TabAccent, Tokens};
use crate::ui_kit::{Container, PartCard, PartCardSize, PartFace, Surface};

/// Most entries listed from one folder (a share can hold thousands).
const MAX_ENTRIES: usize = 2000;
/// A folder's listing is read again after this long (ms), so a share's new parts show up.
const RESCAN_MS: f64 = 15_000.0;
/// The card's width, and the room its list keeps clear on the right for the scroll bar.
pub(crate) const CARD_WIDTH: f32 = 300.0;
const SCROLL_BAR: f32 = 14.0;
/// Placeholder cards drawn while a folder is read.
const PLACEHOLDERS: usize = 6;
/// The offset its title band drags the card by.
const DRAG_KEY: &str = "sc_library_card";

/// The room the open card takes at the view's left, with its gap: the Browser, a left History and
/// the dialog column start beside it. A card dragged away from there takes none.
pub(crate) fn left_inset(app: &SolveApp) -> f32 {
    if shown(app) && app.library.moved == egui::Vec2::ZERO { CARD_WIDTH + Container::for_layout(app.ui.layout, Surface::Panel).gap() } else { 0.0 }
}

/// The card shows in the modelling workspaces, not over Render's own panels.
fn shown(app: &SolveApp) -> bool {
    app.preferences.library_open && !crate::render_workspace::active(app)
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
    /// Where the card was drawn last frame: a part let go over it is not dropped in the view it
    /// lies over, and the view takes no clicks there.
    pub(crate) shown: Option<egui::Rect>,
    /// Folded to its title band.
    pub collapsed: bool,
    /// How far its title band dragged the card from its place beside the Browser.
    pub(crate) moved: egui::Vec2,
    /// Snapped under the Browser: it follows the tree as that grows and shrinks.
    pub(crate) under_browser: bool,
    listed: Option<(PathBuf, f64)>,
    #[cfg(not(target_arch = "wasm32"))]
    listing: Option<std::sync::mpsc::Receiver<Listing>>,
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
    queued: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pictures: Option<std::sync::mpsc::Receiver<(String, Option<egui::ColorImage>)>>,
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
    app.library.current = None;
    app.library.listed = None;
    app.library.entries.clear();
    app.library.error = None;
}

/// Ask for the folder with the host's dialog.
pub fn choose_folder(app: &mut SolveApp) {
    if app.file_picker_busy() {
        return;
    }
    let future = app.services.pick_folder.as_ref().map(|pick| pick());
    let Some(future) = future else {
        app.set_status(crate::i18n::tr("library.no_picker").to_string(), true);
        return;
    };
    app.begin_folder_picker(
        future,
        Box::new(|app, path| {
            if let Some(p) = path {
                set_folder(app, &p);
            }
        }),
    );
}

/// The folder shown: the one browsed into, else the remembered subfolder, else the library folder.
/// Nothing here touches the disk (a share can be slow): a remembered subfolder that is gone shows
/// the library folder once its listing fails.
fn current(app: &SolveApp) -> Option<PathBuf> {
    app.library.current.clone().or_else(|| {
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
    app.library.current = Some(dir);
    app.library.entries.clear();
    app.library.error = None;
    refresh(app, ctx, true);
}

/// The folder is being read and nothing from it has come in yet.
fn reading(app: &SolveApp) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.library.listing.is_some() && app.library.entries.is_empty()
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
    let fresh = app.library.listed.as_ref().is_some_and(|(d, at)| *d == dir && now - at < RESCAN_MS);
    if fresh && !force {
        return;
    }
    app.library.listed = Some((dir.clone(), now));
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
            app.library.listing = Some(rx);
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
            app.library.error = None;
            app.library.entries = entries;
        }
        Err(_) if app.library.current.is_none() && !app.preferences.library_subfolder.is_empty() => {
            // The remembered subfolder is gone: the library folder instead.
            app.preferences.library_subfolder.clear();
            app.library.listed = None;
        }
        Err(e) => {
            app.library.error = Some(e);
            app.library.entries.clear();
        }
    }
}

/// Per frame: listings and pictures that came in.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.preferences.library_open {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(rx) = &app.library.listing
            && let Ok(l) = rx.try_recv()
        {
            app.library.listing = None;
            take_listing(app, l);
        }
        let mut done = false;
        while let Some(rx) = &app.library.pictures {
            match rx.try_recv() {
                Ok((key, img)) => crate::home::install_texture(&mut app.library.thumbs, ctx, &key, img),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
        if done {
            app.library.pictures = None;
        }
        start_pictures(&mut app.library);
    }
    refresh(app, ctx, false);
}

#[cfg(not(target_arch = "wasm32"))]
fn start_pictures(l: &mut LibraryState) {
    if l.pictures.is_some() || l.queued.is_empty() {
        return;
    }
    let keys = std::mem::take(&mut l.queued);
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("library pictures".into()).spawn(move || {
        for key in keys {
            let img = std::panic::catch_unwind(|| crate::home::part_picture(&key)).ok().flatten();
            if tx.send((key, img)).is_err() {
                return;
            }
        }
    });
    if spawned.is_ok() {
        l.pictures = Some(rx);
    }
}

fn thumb(l: &mut LibraryState, key: &str) -> Option<egui::TextureHandle> {
    match l.thumbs.get(key) {
        Some(t) => t.clone(),
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
    let up = if app.cam.up == solidcraft_engine::render::UpAxis::Y { Vec3::Y } else { Vec3::Z };
    let den = d.dot(up);
    if den.abs() > 1e-9 {
        let t = -o.dot(up) / den;
        if t > 0.0 && t.is_finite() {
            return o + d * t;
        }
    }
    Vec3::ZERO
}

/// Put the part at `path` in the open design at `at` (mm).
pub fn insert(app: &mut SolveApp, path: &Path, at: Vec3) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();
    match Kind::of(path) {
        Some(Kind::Design) => app.run("component.insert_design", json!({"path": p, "translate": [at.x, at.y, at.z], "reuse": true})).map(|_| ()),
        Some(Kind::Step | Kind::Mesh) => {
            app.insert_path_async(&p);
            Ok(())
        }
        _ => Err(crate::i18n::format("library.not_a_part", &[("path", &p)])),
    }
}

/// The view: a card dropped on it inserts its part there.
pub fn view_drop(app: &mut SolveApp, ui: &egui::Ui, rect: egui::Rect, proj: &crate::viewport::Proj) {
    let ctx = ui.ctx();
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx) {
        return;
    }
    // Over the view, not the panel lying over it: a card clicked, or let go where it was picked
    // up, inserts nothing.
    let over_panel = |p: &egui::Pos2| app.library.shown.is_some_and(|r| r.contains(*p));
    let Some(pos) = ctx.pointer_hover_pos().filter(|p| rect.contains(*p) && !over_panel(p)) else { return };
    // Where it will land.
    let at = drop_point(app, proj, pos);
    if let Some(s) = proj.to_screen(at) {
        ui.painter().circle_stroke(s, Tokens::get().space_6, egui::Stroke::new(2.0, TabAccent::for_tab(&Tokens::get(), "MODEL").color));
    }
    if ctx.input(|i| i.pointer.primary_released())
        && let Some(d) = egui::DragAndDrop::take_payload::<Dragged>(ctx)
    {
        match insert(app, &d.0, at) {
            Ok(()) => {
                let name = d.0.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_string());
                app.set_status(crate::i18n::format("library.inserted", &[("name", &name)]), false);
            }
            Err(e) => app.set_status(e, true),
        }
    }
}

/// While a part is dragged: its name in a chip by the pointer, so it is clear what will be
/// inserted. Tokens: the tooltip frame.
fn drag_chip(ui: &egui::Ui, e: &Entry) {
    let Some(at) = ui.ctx().pointer_interact_pos() else { return };
    let t = Tokens::get();
    let container = Container::new(ui.ctx(), Surface::Popup);
    container
        .area(egui::Id::new("sc_library_drag"))
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
    close: Cell<bool>,
    refresh: Cell<bool>,
    choose: bool,
    design_folder: bool,
    enter: Option<PathBuf>,
    open: Option<PathBuf>,
    insert: Option<PathBuf>,
    select: Option<PathBuf>,
}

/// The card, left of the Browser and top-aligned with it: as tall as its contents, up to the
/// room above a History strip along the bottom; then its list scrolls while the header and footer
/// stay. Its title band drags it anywhere in the view; let go near its place it snaps back, let go
/// near the Browser's bottom edge it snaps under the tree. Too short for its list, it shows its
/// title band alone.
pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    app.library.shown = None;
    if !shown(app) {
        return;
    }
    let t = Tokens::get();
    let container = Container::new(ui.ctx(), Surface::Panel);
    let gap = container.gap();
    let mut available = ui.available_rect_before_wrap();
    available.min.x += crate::agent_chat::left_inset(app, available);
    let along_bottom = app.ui.show_timeline && !app.preferences.timeline_left;
    let bounds = crate::browser::panel_bounds(available, app.shell.history.filter(|_| along_bottom), CARD_WIDTH, gap);
    let header = crate::ui_kit::header_height();
    if !bounds.is_positive() || bounds.height() < header * 2.0 {
        return;
    }
    let ctx = ui.ctx().clone();
    let dragged = crate::frame::dock_offset(&ctx, DRAG_KEY);
    let far = (egui::pos2(available.right() - CARD_WIDTH, bounds.bottom() - header * 2.0) - bounds.min).max(egui::Vec2::ZERO);
    let mut moved = egui::vec2(dragged.x.clamp(available.left() - bounds.left(), far.x), dragged.y.clamp(available.top() - bounds.top(), far.y));
    let down = ctx.input(|i| i.pointer.any_down());
    // Under the tree: only a Browser already out of the card's way has that spot.
    let under = app.shell.browser.filter(|_| app.library.moved != egui::Vec2::ZERO).map(|b| {
        let spot = egui::pos2(b.left(), b.bottom() + gap) - bounds.min;
        egui::vec2(spot.x.clamp(0.0, far.x), spot.y.clamp(0.0, far.y))
    });
    if down && dragged != app.library.moved {
        app.library.under_browser = false;
    }
    if !down {
        if moved.length() < t.space_24 {
            // Let go near its place: back beside the Browser.
            app.library.under_browser = false;
            moved = egui::Vec2::ZERO;
            crate::frame::set_dock_offset(&ctx, DRAG_KEY, moved);
        } else if under.is_some_and(|spot| (moved - spot).length() < t.space_24) {
            app.library.under_browser = true;
        }
    }
    if app.library.under_browser
        && let Some(spot) = under
    {
        moved = spot;
        crate::frame::set_dock_offset(&ctx, DRAG_KEY, moved);
    }
    app.library.moved = moved;
    let at = bounds.min + moved;
    let short = bounds.bottom() - at.y < t.form_header_height * 3.0;
    let frame = container.frame(&ctx, t.panel, crate::frame::Side::Left).inner_margin(t.space_12 as i8);
    let inner = (egui::vec2(bounds.width(), bounds.bottom() - at.y) - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    let mut acts = Acts::default();
    let card = container.area(egui::Id::new("sc_library_card")).fixed_pos(at).movable(false).show(&ctx, |ui| {
        container.show_frame(ui, frame, available, |ui| {
            ui.set_width(inner.x);
            ui.set_max_height(inner.y);
            crate::ui_kit::catalog_scope(ui, |ui| contents(app, ui, &mut acts, (dragged, short)));
        })
    });
    let rect = card.response.rect;
    app.library.shown = Some(rect);
    crate::scenario::publish_rect("library:card", rect);
    take(app, &ctx, acts);
}

fn take(app: &mut SolveApp, ctx: &egui::Context, acts: Acts) {
    if acts.close.get() {
        app.preferences.library_open = false;
    }
    if acts.refresh.get() {
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
        app.library.selected = Some(p);
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
    let filter = app.library.filter.to_lowercase();
    app.library.entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter)).cloned().collect()
}

/// `(dragged, short)`: the drag offset as it was before this frame, and whether the card has room
/// for no more than its title band.
fn contents(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Acts, (dragged, short): (egui::Vec2, bool)) {
    let t = Tokens::get();
    let tr = crate::i18n::tr;
    let accent = TabAccent::for_tab(&t, "MODEL");
    // The title band drags the card; its buttons, added after, keep their clicks.
    let band = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), crate::ui_kit::header_height()));
    crate::frame::drag_dock(ui, band, DRAG_KEY, dragged, app.library.moved);
    let more_menu = Cell::new(None::<egui::Response>);
    let collapsed = (!short).then_some(&mut app.library.collapsed);
    crate::ui_kit::card_header(ui, "body", accent.color, tr("library.title"), collapsed, || acts.close.set(true), &mut |ui| {
        let r = ui.max_rect();
        let half = egui::vec2(r.width() / 2.0, r.height());
        let again = crate::ui_kit::icon_button_at(ui, egui::Rect::from_min_size(r.min, half), "update", tr("library.refresh"));
        crate::scenario::publish_handle("library:refresh", again.rect.center());
        if again.clicked() {
            acts.refresh.set(true);
        }
        let more = crate::ui_kit::icon_button_at(ui, egui::Rect::from_min_size(r.min + egui::vec2(half.x, 0.0), half), "dots", tr("library.more"));
        crate::scenario::publish_handle("library:more", more.rect.center());
        more_menu.set(Some(more));
    });
    if let Some(more) = more_menu.take() {
        more_actions(app, ui, &more, acts);
    }
    if short || app.library.collapsed {
        return;
    }
    let Some(root) = root(app) else {
        no_folder(app, ui, acts);
        footer(ui);
        return;
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
    if let Some(error) = app.library.error.clone() {
        unreadable(ui, &error, acts);
    } else if reading(app) {
        list.show(ui, |ui| placeholders(app, ui));
    } else if app.library.entries.is_empty() {
        state(ui, "folder", None, tr("library.empty"));
        if crate::ui_kit::button(ui, tr("library.choose_another"), crate::ui_kit::Button::Normal).clicked() {
            acts.choose = true;
        }
    } else {
        let entries = filtered(app);
        if entries.is_empty() {
            state(ui, "folder", Some(t.form_dim), tr("library.no_match"));
            let clear = crate::ui_kit::button(ui, tr("library.clear_search"), crate::ui_kit::Button::Normal);
            crate::scenario::publish_handle("library:clear_search", clear.rect.center());
            if clear.clicked() {
                app.library.filter.clear();
            }
        } else {
            // Only the rows in sight are laid out, and only their pictures drawn.
            let size = card_size(app.preferences.library_size);
            let columns = size.map_or(1, PartCardSize::columns);
            let row = size.map_or(t.control_height, PartCardSize::height);
            list.show_rows(ui, row, entries.len().div_ceil(columns), |ui, rows| list_view(app, ui, acts, &entries, rows));
        }
    }
    footer(ui);
}

/// The ⋯ menu: another folder, the open design's folder, read the folder again.
fn more_actions(app: &SolveApp, ui: &egui::Ui, more: &egui::Response, acts: &mut Acts) {
    let tr = crate::i18n::tr;
    Container::new(ui.ctx(), Surface::Popup).menu(more).show(|ui| {
        if crate::ui_kit::MenuItem::new("library:menu_choose", tr("library.choose_another"), "folder").show(ui).clicked() {
            acts.choose = true;
            ui.close();
        }
        let mut item = crate::ui_kit::MenuItem::new("library:menu_design_folder", tr("library.design_folder"), "");
        item.enabled = app.session.path.is_some();
        if item.show(ui).clicked() {
            acts.design_folder = true;
            ui.close();
        }
        crate::ui_kit::Menu::separator(ui);
        let mut again = crate::ui_kit::MenuItem::new("library:menu_refresh", tr("library.refresh"), "update");
        again.shortcut = "F5";
        if again.show(ui).clicked() {
            acts.refresh.set(true);
            ui.close();
        }
    });
}

/// No folder chosen yet.
fn no_folder(app: &SolveApp, ui: &mut egui::Ui, acts: &mut Acts) {
    let tr = crate::i18n::tr;
    state(ui, "folder", None, tr("library.no_folder"));
    let b = crate::ui_kit::button(ui, tr("library.choose"), crate::ui_kit::Button::Primary);
    crate::scenario::publish_handle("library:choose", b.rect.center());
    if b.clicked() {
        acts.choose = true;
    }
    if crate::ui_kit::button(ui, tr("library.design_folder"), crate::ui_kit::Button::Enabled(app.session.path.is_some())).clicked() {
        acts.design_folder = true;
    }
}

/// A folder that can't be read: its message, Try again and another folder. The breadcrumb keeps
/// its path.
fn unreadable(ui: &mut egui::Ui, error: &str, acts: &mut Acts) {
    let tr = crate::i18n::tr;
    let t = Tokens::get();
    state(ui, "folder", Some(t.error), tr("library.unreadable"));
    ui.label(egui::RichText::new(error).size(t.font_small).color(t.text_dim));
    ui.horizontal(|ui| {
        if crate::ui_kit::button(ui, tr("library.try_again"), crate::ui_kit::Button::Primary).clicked() {
            acts.refresh.set(true);
        }
        if crate::ui_kit::button(ui, tr("library.choose_another"), crate::ui_kit::Button::Normal).clicked() {
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
    crate::icons::paint_ink(ui.painter(), at, icon, ink.unwrap_or(TabAccent::for_tab(&t, "MODEL").color));
    ui.vertical_centered(|ui| ui.label(egui::RichText::new(text).size(t.font_small).color(t.form_ink)));
}

/// "N items" ("1 item"); while searching, those the search leaves.
fn item_count(app: &SolveApp) -> String {
    let n = if app.library.filter.is_empty() { app.library.entries.len() } else { filtered(app).len() };
    crate::i18n::format(if n == 1 { "library.items_one" } else { "library.items" }, &[("n", &n.to_string())])
}

/// Up a level inside a subfolder, the folder button (another folder), the folder shown (cut short;
/// an ellipsis stands for the levels between, named in its tooltip) and the item count on the right.
fn breadcrumb(app: &SolveApp, ui: &mut egui::Ui, root: &Path, here: &Path, acts: &mut Acts) {
    let t = Tokens::get();
    let tr = crate::i18n::tr;
    let name = |p: &Path| p.file_name().map_or_else(|| p.to_string_lossy().to_string(), |n| n.to_string_lossy().to_string());
    let count = item_count(app);
    let levels: Vec<String> =
        here.strip_prefix(root).map(|r| r.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_4;
        let counted = ui.painter().layout_no_wrap(count.clone(), egui::FontId::proportional(t.font_small), t.text_dim).size().x;
        if here != root {
            let up = crate::ui_kit::compact_button(ui, crate::ui_kit::MOVE_UP, tr("library.up"), true, false).on_hover_text(tr("library.up"));
            crate::scenario::publish_handle("library:up", up.rect.center());
            if up.clicked() {
                acts.enter = here.parent().map(Path::to_path_buf).filter(|p| p.starts_with(root)).or_else(|| Some(root.to_path_buf()));
            }
        }
        let room = ui.available_width() - counted - t.space_8;
        let folder = crate::ui_kit::folder_button(ui, &name(root), if levels.is_empty() { room } else { room * 0.5 }).on_hover_text(format!(
            "{}\n{}",
            tr("library.choose_another"),
            root.to_string_lossy()
        ));
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

/// The search field. Esc clears it, then closes the card; Ctrl+F or `/` over the card focuses it;
/// F5 reads the folder again.
fn search(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Acts) {
    let id = egui::Id::new("library_search");
    let r = crate::ui_kit::search_field(ui, id, &mut app.library.filter, crate::i18n::tr("library.search"));
    crate::scenario::publish_handle("library:search", r.rect.center());
    let over = app.library.shown.is_some_and(|c| ui.ctx().pointer_hover_pos().is_some_and(|p| c.contains(p)));
    let nothing_focused = ui.ctx().memory(|m| m.focused().is_none());
    if over && nothing_focused && ui.input(|i| (i.modifiers.command && i.key_pressed(egui::Key::F)) || i.key_pressed(egui::Key::Slash)) {
        r.request_focus();
    }
    if nothing_focused && ui.input(|i| i.key_pressed(egui::Key::F5)) {
        acts.refresh.set(true);
    }
    if (r.has_focus() || r.lost_focus()) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        // The search's own Esc: no tool or selection is cancelled with it.
        app.esc_handled = true;
        if app.library.filter.is_empty() {
            acts.close.set(true);
        } else {
            app.library.filter.clear();
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
            for row in 0..PLACEHOLDERS.div_ceil(size.columns()) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.space_8;
                    for _ in 0..size.columns().min(PLACEHOLDERS - row * size.columns()) {
                        crate::ui_kit::part_card(
                            ui,
                            PartCard { name: "", face: PartFace::Placeholder, size, room, selected: false, dragging: false },
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
    let tr = crate::i18n::tr;
    let size = card_size(app.preferences.library_size);
    let dragging = egui::DragAndDrop::payload::<Dragged>(ui.ctx()).map(|d| d.0.clone());
    let columns = size.map_or(1, PartCardSize::columns);
    let room = ui.available_width() - SCROLL_BAR;
    for row in rows {
        let row = &entries[row * columns..((row + 1) * columns).min(entries.len())];
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for e in row {
                let badge = match e.kind {
                    Kind::Step => Some("STEP"),
                    Kind::Mesh => Some("MESH"),
                    _ => None,
                };
                let selected = app.library.selected.as_ref() == Some(&e.path);
                let r = match size {
                    Some(size) => {
                        let id = if e.count == 1 { "library.folder_parts_one" } else { "library.folder_parts" };
                        let parts = crate::i18n::format(id, &[("n", &e.count.to_string())]);
                        let face = if e.kind == Kind::Folder {
                            PartFace::Folder { count: &parts }
                        } else {
                            let key = e.path.to_string_lossy().to_string();
                            PartFace::Part { picture: thumb(&mut app.library, &key).map(|tex| tex.id()), badge }
                        };
                        let dragging = dragging.as_ref() == Some(&e.path);
                        crate::ui_kit::part_card(ui, PartCard { name: &e.name, face, size, room, selected, dragging })
                    }
                    None => {
                        ui.set_width(room);
                        let icon = match e.kind {
                            Kind::Folder => "folder",
                            Kind::Mesh => "mesh_body",
                            _ => "body",
                        };
                        crate::ui_kit::part_row(ui, &e.name, icon, badge, selected)
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
                crate::ui_kit::Menu::context(&r, |ui| {
                    let insert = if e.kind == Kind::Design { tr("library.insert") } else { tr("library.import") };
                    let i = crate::ui_kit::MenuItem::new("library:menu_insert", insert, "").show(ui);
                    crate::scenario::publish_handle("library:menu_insert", i.rect.center());
                    if i.clicked() {
                        acts.insert = Some(e.path.clone());
                        ui.close();
                    }
                    let o = crate::ui_kit::MenuItem::new("library:menu_open", tr("library.open"), "").show(ui);
                    crate::scenario::publish_handle("library:menu_open", o.rect.center());
                    if o.clicked() {
                        acts.open = Some(e.path.clone());
                        ui.close();
                    }
                });
            }
        });
    }
}

/// The kit's catalog footer: the hint's first sentence, the whole hint on hover.
fn footer(ui: &mut egui::Ui) {
    let tr = crate::i18n::tr;
    crate::ui_kit::catalog_footer_with_tip(ui, tr("library.hint_short"), tr("library.hint"));
}

/// Esc while the search has the focus is the search's own, whatever runs before the card this frame.
pub(crate) fn takes_esc(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.has_focus(egui::Id::new("library_search"))) && ctx.input(|i| i.key_pressed(egui::Key::Escape))
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
        app.library.current = Some(dir.clone());
        take_listing(app, (dir.clone(), list(&dir)));
    }
    if let Some(name) = p.get("insert").and_then(serde_json::Value::as_str) {
        let e = app
            .library
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
        insert(app, &e.path, at)?;
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
        "entries": app.library.entries.iter().map(|e| json!({"name": e.name, "kind": kind(e.kind), "path": e.path.to_string_lossy()})).collect::<Vec<_>>(),
        "error": app.library.error,
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
        assert!(h.app.library.shown.is_some(), "the card shows");
        assert!(h.app.prefs().contains("library_folder"), "the folder is kept with the preferences");
        control(&mut h.app, &json!({"insert": "bracket", "at": [50, 0, 0]})).unwrap();
        let links = h.app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["name"], "bracket", "{links}");
        let b = solidcraft_engine::view::bounds(&h.app.session);
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
        assert!(h.app.library.shown.is_none(), "hidden");
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
        h.frames(3);
        let at = crate::scenario::handle_at("library:bracket").expect("the bracket's card");
        h.step(&json!({"click": [at.x, at.y], "double": true})).unwrap();
        let path = opened(&mut h).expect("a double click opens it");
        assert!(path.ends_with("bracket.solidcraft"), "{path}");
        assert_eq!(h.app.session.execute("component.links", &json!({})).unwrap()["links"].as_array().map_or(0, Vec::len), 0, "opened, not inserted");

        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        let at = crate::scenario::handle_at("library:adapter").expect("the STEP file's card");
        h.step(&json!({"click": [at.x, at.y], "button": "right"})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("library:menu_insert").is_some(), "the right click shows the menu");
        let open = crate::scenario::handle_at("library:menu_open").expect("its Open");
        h.step(&json!({"click": [open.x, open.y]})).unwrap();
        // A STEP file opens as a new design holding it as a component named after it.
        opened(&mut h);
        let occurrences = h.app.session.execute("inspect.measure", &json!({})).unwrap();
        let named = serde_json::to_string(&occurrences).unwrap_or_default().to_lowercase();
        assert!(named.contains("adapter") && !h.app.session.world_state().bodies.is_empty(), "Open opened the STEP file: {occurrences}");
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
            h.frames(3);
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

    /// The card is 300 pt wide, left of the Browser (the card gap apart) and top-aligned with it,
    /// hugs its contents and ends above a History strip along the bottom, which keeps its width.
    #[test]
    fn the_card_sits_left_of_the_browser_and_above_the_history_strip() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        h.app.ui.show_browser = true;
        h.frames(3);
        let history_before = h.app.shell.history;
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(4);
        let card = h.app.library.shown.expect("the card");
        assert!((card.width() - CARD_WIDTH).abs() < 1.5, "{card:?}");
        let browser = h.app.shell.browser.expect("the Browser");
        let gap = Container::for_layout(h.app.ui.layout, Surface::Panel).gap();
        assert!((browser.left() - card.right() - gap).abs() < 1.5, "the Browser starts beside the card: {card:?} {browser:?}");
        assert!((browser.top() - card.top()).abs() < 1.5, "top-aligned: {card:?} {browser:?}");
        if let Some(history) = h.app.shell.history.filter(|r| r.width() > r.height()) {
            assert!(card.bottom() <= history.top() + 0.5, "above History: {card:?} {history:?}");
            assert_eq!(Some(history.left()), history_before.map(|r| r.left()), "History keeps its width");
        }
        let bottom = card.bottom();
        control(&mut h.app, &json!({"subfolder": "Fasteners"})).unwrap();
        h.frames(3);
        assert!(h.app.library.shown.expect("the card").bottom() < bottom, "fewer parts, a shorter card");
        for what in ["refresh", "more", "folder", "search", "up"] {
            let at = crate::scenario::handle_at(&format!("library:{what}")).expect(what);
            assert!(h.app.library.shown.unwrap().contains(at), "{what} inside the card");
        }
    }

    /// Insert > Parts library opens and closes the card; × closes it too.
    #[test]
    fn the_insert_entry_opens_and_closes_the_card() {
        let mut h = crate::scenario::Harness::new();
        let on_insert = crate::workspace::SECTIONS.iter().any(|s| s.tab == "INSERT" && s.ids.contains(&"view.parts_library"));
        assert!(on_insert, "on the Insert tab");
        h.app.run("view.parts_library", json!({})).unwrap();
        h.frames(3);
        assert!(h.app.preferences.library_open && h.app.library.shown.is_some(), "opened");
        assert!(crate::scenario::handle_at("library:choose").is_some(), "with no folder it offers Choose folder…");
        h.app.run("view.parts_library", json!({})).unwrap();
        h.frames(2);
        assert!(!h.app.preferences.library_open && h.app.library.shown.is_none(), "closed");
        h.app.run("view.parts_library", json!({"visible": true})).unwrap();
        h.frames(2);
        let close = crate::scenario::handle_at("dialog:close").expect("the card's ×");
        let card = h.app.library.shown.expect("the card");
        assert!(card.contains(close), "the × is the card's: {close:?} {card:?}");
        h.step(&json!({"click": [close.x, close.y]})).unwrap();
        h.frames(2);
        assert!(!h.app.preferences.library_open, "× closes it");
    }

    /// The fold button leaves the title band; its title band drags the card off its place (the
    /// Browser takes the room back), and let go near its place it snaps back beside the Browser.
    #[test]
    fn the_card_folds_and_drags_and_snaps_back() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        h.app.ui.show_browser = true;
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(4);
        let card = h.app.library.shown.expect("the card");
        let fold = crate::scenario::handle_at("dialog:collapse").expect("the fold button");
        assert!(card.contains(fold), "the fold button is the card's");
        h.step(&json!({"click": [fold.x, fold.y]})).unwrap();
        h.frames(2);
        let folded = h.app.library.shown.expect("the folded card");
        assert!(h.app.library.collapsed && folded.height() < card.height() / 2.0, "the title band alone: {folded:?}");
        assert!(crate::scenario::handle_at("library:search").is_none(), "no list while folded");
        let fold = crate::scenario::handle_at("dialog:collapse").unwrap();
        h.step(&json!({"click": [fold.x, fold.y]})).unwrap();
        h.frames(2);
        assert!(!h.app.library.collapsed, "unfolded");

        let title = egui::pos2(card.left() + 80.0, card.top() + crate::ui_kit::header_height() / 2.0);
        let to = title + egui::vec2(400.0, 60.0);
        h.step(&json!({"drag": [[title.x, title.y], [to.x, to.y]]})).unwrap();
        h.frames(3);
        let moved = h.app.library.shown.expect("the card");
        assert!((moved.left() - card.left() - 400.0).abs() < 2.0 && (moved.top() - card.top() - 60.0).abs() < 2.0, "{card:?} → {moved:?}");
        assert_eq!(left_inset(&h.app), 0.0, "moved away, it leaves the Browser its place");

        let title = egui::pos2(moved.left() + 80.0, moved.top() + crate::ui_kit::header_height() / 2.0);
        let near = title - egui::vec2(390.0, 55.0);
        h.step(&json!({"drag": [[title.x, title.y], [near.x, near.y]]})).unwrap();
        h.frames(3);
        let back = h.app.library.shown.expect("the card");
        assert!((back.min - card.min).length() < 1.0, "snapped back: {card:?} {back:?}");
        assert!(left_inset(&h.app) > 0.0, "beside the Browser again");

        // Dragged off, then let go near the Browser's bottom edge: it snaps under the tree.
        let title = egui::pos2(back.left() + 80.0, back.top() + crate::ui_kit::header_height() / 2.0);
        let away = title + egui::vec2(500.0, 200.0);
        h.step(&json!({"drag": [[title.x, title.y], [away.x, away.y]]})).unwrap();
        h.frames(3);
        let tree = h.app.shell.browser.expect("the Browser");
        let gap = Container::for_layout(h.app.ui.layout, Surface::Panel).gap();
        let spot = egui::pos2(tree.left(), tree.bottom() + gap);
        let near = away + ((spot + egui::vec2(10.0, 10.0)) - (back.min + egui::vec2(500.0, 200.0)));
        h.step(&json!({"drag": [[away.x, away.y], [near.x, near.y]]})).unwrap();
        h.frames(3);
        let under = h.app.library.shown.expect("the card");
        assert!(h.app.library.under_browser && (under.min - spot).length() < 1.0, "under the tree: {spot:?} {under:?}");
        assert_eq!(left_inset(&h.app), 0.0, "the Browser keeps the left edge");

        let title = egui::pos2(under.left() + 80.0, under.top() + crate::ui_kit::header_height() / 2.0);
        let off = title + egui::vec2(400.0, 0.0);
        h.step(&json!({"drag": [[title.x, title.y], [off.x, off.y]]})).unwrap();
        h.frames(3);
        assert!(!h.app.library.under_browser, "dragged away, it stays where it is let go");
    }

    /// A search that leaves nothing says so and clears with one click; the count follows it.
    #[test]
    fn a_search_with_no_match_clears_with_one_click() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        assert_eq!(item_count(&h.app), "3 items");
        h.app.library.filter = "brack".into();
        assert_eq!(item_count(&h.app), "1 item", "the search's count, singular");
        h.app.library.filter = "zzz".into();
        h.frames(2);
        let clear = crate::scenario::handle_at("library:clear_search").expect("Clear search");
        h.step(&json!({"click": [clear.x, clear.y]})).unwrap();
        h.frames(2);
        assert!(h.app.library.filter.is_empty() && crate::scenario::handle_at("library:bracket").is_some(), "cleared");
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
            if h.app.library.error.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(h.app.library.error.is_some(), "unreadable");
        h.frames(2);
        assert!(h.widget(crate::i18n::tr("library.try_again")).is_some(), "Try again");
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
        let card = h.app.library.shown.expect("the card is drawn");
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
