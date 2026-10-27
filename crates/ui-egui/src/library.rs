//! The parts library: a panel on the window's left with the designs and parts in a folder on this
//! computer or a server share, each shown by its picture and name. Drag one into the view to put
//! it in the open design: a SolidCraft design becomes a linked component (an occurrence placed
//! where it is dropped, as Insert Design makes), a STEP or mesh file is imported (as Insert STEP
//! or Insert Mesh). Folders open in place; double-click a design to open it. The panel narrows
//! to a strip or hides (File > Parts Library), and remembers its folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Color32, RichText, Sense, vec2};
use serde_json::json;
use solidcraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;

/// Most entries listed from one folder (a share can hold thousands).
const MAX_ENTRIES: usize = 2000;
/// A folder's listing is read again after this long (ms), so a share's new parts show up.
const RESCAN_MS: f64 = 15_000.0;
/// The card sizes (`library_size` 1, 2 and 3): the first library's 104, half as large again, and
/// twice as large.
const CARD_SMALL: f32 = 104.0;
const CARD_NORMAL: f32 = 156.0;
const CARD_XL: f32 = 208.0;
/// How far a card may narrow to keep one more in its row.
const CARD_SHRINK: f32 = 0.8;

/// The card width of `library_size` (0, names only: none).
fn card_size(size: u8) -> Option<f32> {
    match size {
        0 => None,
        1 => Some(CARD_SMALL),
        2 => Some(CARD_NORMAL),
        _ => Some(CARD_XL),
    }
}
/// The gap between cards, and a names-only row's height.
const GAP: f32 = 6.0;
const NAME_ROW: f32 = 22.0;

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
            e if e == solidcraft_engine::io::DESIGN_EXT || e == solidcraft_engine::io::LEGACY_DESIGN_EXT => Kind::Design,
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
    /// Where the panel was drawn last frame: a card let go over it is not dropped in the view
    /// the panel lies over.
    shown: Option<egui::Rect>,
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
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let kind = if is_dir { Some(Kind::Folder) } else { Kind::of(&path) };
        if let Some(kind) = kind {
            let shown = if kind == Kind::Folder { name } else { path.file_stem().map_or(name.clone(), |s| s.to_string_lossy().to_string()) };
            out.push(Entry { path, name: shown, kind });
        }
        if out.len() >= MAX_ENTRIES {
            break;
        }
    }
    out.sort_by(|a, b| (a.kind != Kind::Folder).cmp(&(b.kind != Kind::Folder)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

fn root(app: &SolveApp) -> Option<PathBuf> {
    let f = app.preferences.library_folder.trim();
    (!f.is_empty()).then(|| PathBuf::from(f))
}

/// Show the library from `folder` (kept in the preferences).
pub fn set_folder(app: &mut SolveApp, folder: &str) {
    app.preferences.library_folder = folder.to_string();
    app.preferences.library_open = true;
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
        app.set_status("this build has no folder picker; set the folder with ui.library {folder}".to_string(), true);
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

fn current(app: &SolveApp) -> Option<PathBuf> {
    app.library.current.clone().or_else(|| root(app))
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
            let img = std::panic::catch_unwind(|| crate::home::picture_of(&key)).ok().flatten();
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
        _ => Err(format!("{p} is not a design or a part file")),
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
        ui.painter().circle_stroke(s, 7.0, egui::Stroke::new(2.0, Tokens::get().accent));
    }
    if ctx.input(|i| i.pointer.primary_released())
        && let Some(d) = egui::DragAndDrop::take_payload::<Dragged>(ctx)
    {
        match insert(app, &d.0, at) {
            Ok(()) => app.set_status(format!("inserted {}", d.0.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_string())), false),
            Err(e) => app.set_status(e, true),
        }
    }
}

/// A card `width` wide: the part's picture, its kind and its name.
fn card(ui: &mut egui::Ui, l: &mut LibraryState, e: &Entry, width: f32) -> egui::Response {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, width * 0.75 + 18.0), Sense::click_and_drag());
    let pic = egui::Rect::from_min_size(rect.min, vec2(width, width * 0.75));
    let painter = ui.painter_at(rect);
    let fill = if resp.hovered() { t.panel.gamma_multiply(1.3) } else { t.panel.gamma_multiply(1.12) };
    painter.rect_filled(pic, 4.0, fill);
    match e.kind {
        Kind::Folder => {
            let c = pic.center();
            let tab = egui::Rect::from_center_size(c + vec2(-10.0, -14.0), vec2(22.0, 8.0));
            let body = egui::Rect::from_center_size(c + vec2(0.0, 4.0), vec2(46.0, 32.0));
            painter.rect_filled(tab, 2.0, t.accent.gamma_multiply(0.8));
            painter.rect_filled(body, 3.0, t.accent);
        }
        _ => {
            let key = e.path.to_string_lossy().to_string();
            match thumb(l, &key) {
                Some(tex) => {
                    painter.image(tex.id(), pic.shrink(2.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    painter.text(pic.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(t.font_label), t.text_dim);
                }
            }
            // What the file is, where it is not a design (a STEP and a design can share a name).
            let badge = match e.kind {
                Kind::Step => Some("STEP"),
                Kind::Mesh => Some("MESH"),
                _ => None,
            };
            if let Some(b) = badge {
                let r = egui::Rect::from_min_size(pic.left_top() + vec2(4.0, 4.0), vec2(34.0, 14.0));
                painter.rect_filled(r, 3.0, t.accent);
                painter.text(r.center(), egui::Align2::CENTER_CENTER, b, egui::FontId::proportional(t.font_badge), Color32::WHITE);
            }
        }
    }
    let name = shortened(&e.name, width);
    painter.text(
        egui::pos2(rect.center().x, pic.bottom() + 9.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(t.font_small),
        t.text,
    );
    crate::scenario::publish_handle(&format!("library:{}", e.name), rect.center());
    resp.on_hover_text(e.path.to_string_lossy().to_string())
}

/// `name` cut to what fits `width` at the small font (about 6.5 px a character), with an ellipsis.
fn shortened(name: &str, width: f32) -> String {
    let fits = ((width / 6.5).floor() as usize).max(4);
    if name.chars().count() > fits { format!("{}…", name.chars().take(fits - 1).collect::<String>()) } else { name.to_owned() }
}

/// A names-only row `width` wide: a folder mark or the file's kind, and the name.
fn name_row(ui: &mut egui::Ui, e: &Entry, width: f32) -> egui::Response {
    let t = Tokens::get();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, NAME_ROW), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    if resp.hovered() {
        painter.rect_filled(rect, 3.0, t.panel.gamma_multiply(1.3));
    }
    let tag = match e.kind {
        Kind::Folder => "▸",
        Kind::Design => "",
        Kind::Step => "STEP",
        Kind::Mesh => "MESH",
    };
    let mut x = rect.left() + 4.0;
    if !tag.is_empty() {
        let r = egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 7.0), vec2(if e.kind == Kind::Folder { 14.0 } else { 34.0 }, 14.0));
        painter.rect_filled(r, 3.0, t.accent);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, tag, egui::FontId::proportional(t.font_badge), Color32::WHITE);
        x = r.right() + 6.0;
    }
    painter.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        shortened(&e.name, rect.right() - x),
        egui::FontId::proportional(t.font_small),
        t.text,
    );
    crate::scenario::publish_handle(&format!("library:{}", e.name), rect.center());
    resp.on_hover_text(e.path.to_string_lossy().to_string())
}

/// While a card is dragged: its name by the pointer, so it is clear what will be inserted.
fn drag_ghost(ui: &egui::Ui, e: &Entry) {
    let Some(at) = ui.ctx().pointer_interact_pos() else { return };
    let t = Tokens::get();
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("sc_library_drag")));
    let font = egui::FontId::proportional(t.font_small);
    let galley = p.layout_no_wrap(e.name.clone(), font, t.text);
    let r = egui::Rect::from_min_size(at + vec2(14.0, 10.0), galley.size() + vec2(12.0, 8.0));
    p.rect_filled(r, 4.0, t.panel);
    p.rect_stroke(r, 4.0, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(6.0, 4.0), galley, t.text);
}

/// The panel (or its strip) on the window's left.
pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    if !app.preferences.library_open {
        app.library.shown = None;
        return;
    }
    let t = Tokens::get();
    let tr = crate::i18n::tr;
    if app.preferences.library_collapsed {
        app.library.shown = None;
        egui::Panel::left("sc_library_strip").exact_size(30.0).resizable(false).frame(egui::Frame::NONE.fill(t.panel).inner_margin(4)).show(
            ui,
            |ui| {
                let b = crate::ui_kit::button(ui, "»", crate::ui_kit::Button::Small).on_hover_text(tr("library.expand"));
                crate::scenario::publish_handle("library:expand", b.rect.center());
                if b.clicked() {
                    app.preferences.library_collapsed = false;
                }
            },
        );
        return;
    }
    let mut open: Option<PathBuf> = None;
    let mut enter: Option<PathBuf> = None;
    let mut insert_now: Option<PathBuf> = None;
    let mut action: Option<&'static str> = None;
    let shown = egui::Panel::left("sc_library")
        .resizable(true)
        // Two small cards side by side.
        .default_size(3.0 * CARD_SMALL + 2.0 * GAP + 16.0)
        .size_range(180.0..=720.0)
        .frame(egui::Frame::NONE.fill(t.panel).inner_margin(8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("library.title")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (label, tip, what) in [
                        ("×", "library.hide", "hide"),
                        ("«", "library.collapse", "collapse"),
                        ("⟳", "library.refresh", "refresh"),
                        (tr("library.folder"), "library.choose", "choose"),
                    ] {
                        let b = crate::ui_kit::button(ui, label, crate::ui_kit::Button::Small).on_hover_text(tr(tip));
                        crate::scenario::publish_handle(&format!("library:{what}"), b.rect.center());
                        if b.clicked() {
                            action = Some(what);
                        }
                    }
                });
            });
            let Some(root) = root(app) else {
                ui.add_space(8.0);
                ui.label(RichText::new(tr("library.no_folder")).color(t.text_dim));
                if crate::ui_kit::button(ui, tr("library.choose"), crate::ui_kit::Button::Normal).clicked() {
                    action = Some("choose");
                }
                return;
            };
            let here = current(app).unwrap_or_else(|| root.clone());
            ui.horizontal(|ui| {
                if here != root && crate::ui_kit::button(ui, "⬆", crate::ui_kit::Button::Small).on_hover_text(tr("library.up")).clicked() {
                    enter = here.parent().map(Path::to_path_buf).filter(|p| p.starts_with(&root)).or_else(|| Some(root.clone()));
                }
                let shown = here.strip_prefix(&root).ok().filter(|r| !r.as_os_str().is_empty()).map_or_else(
                    || root.file_name().map_or_else(|| root.to_string_lossy().to_string(), |n| n.to_string_lossy().to_string()),
                    |r| {
                        format!(
                            "{} / {}",
                            root.file_name().map_or_else(String::new, |n| n.to_string_lossy().to_string()),
                            r.to_string_lossy().replace('\\', " / ")
                        )
                    },
                );
                ui.label(RichText::new(shown).small().color(t.text_dim)).on_hover_text(here.to_string_lossy().to_string());
            });
            crate::ui_kit::text_field(ui, &mut app.library.filter, tr("library.search"));
            // How many columns at most, and how large: names only, small, normal or extra large cards.
            ui.horizontal(|ui| {
                let half = (ui.available_width() - GAP) / 2.0;
                let mut columns = usize::from(app.preferences.library_columns.clamp(1, 3)) - 1;
                let mut size = usize::from(app.preferences.library_size.min(3));
                ui.allocate_ui(vec2(half, 24.0), |ui| {
                    ui.label(RichText::new(tr("library.columns")).small().color(t.text_dim));
                    crate::ui_kit::dropdown(ui, "library_columns", &mut columns, &["1", "2", "3"]);
                });
                ui.allocate_ui(vec2(half, 24.0), |ui| {
                    ui.label(RichText::new(tr("library.size")).small().color(t.text_dim));
                    let sizes = [tr("library.size_names"), tr("library.size_small"), tr("library.size_normal"), tr("library.size_xl")];
                    crate::ui_kit::dropdown(ui, "library_size", &mut size, &sizes);
                });
                app.preferences.library_columns = columns as u8 + 1;
                app.preferences.library_size = size as u8;
            });
            if let Some(e) = &app.library.error {
                ui.label(RichText::new(e).small().color(t.warning));
            }
            ui.small(RichText::new(tr("library.hint")).color(t.text_dim));
            ui.add_space(4.0);
            let filter = app.library.filter.to_lowercase();
            let entries: Vec<Entry> =
                app.library.entries.iter().filter(|e| filter.is_empty() || e.name.to_lowercase().contains(&filter)).cloned().collect();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let columns = usize::from(app.preferences.library_columns.clamp(1, 3));
                let size = card_size(app.preferences.library_size);
                let names_only = size.is_none();
                let room = ui.available_width();
                // Cards keep their size, give or take a fifth: a row holds as many as fit at
                // four fifths of it, up to Columns (one at least, narrowed to the panel when even
                // one doesn't fit). Names fill their column.
                let per_row = size.map_or(columns, |w| (((room + GAP) / (w * CARD_SHRINK + GAP)).floor() as usize).clamp(1, columns));
                let fill = ((room - GAP * (per_row as f32 - 1.0)) / per_row as f32).max(40.0);
                let width = size.map_or(fill, |w| fill.min(w));
                for row in entries.chunks(per_row) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for e in row {
                            let r = if names_only { name_row(ui, e, width) } else { card(ui, &mut app.library, e, width) };
                            if e.kind == Kind::Folder {
                                if r.clicked() {
                                    enter = Some(e.path.clone());
                                }
                                continue;
                            }
                            // The card's own response carries the drag (an overlaid drag source
                            // took the pointer from it: no right click, no double click, #4432):
                            // a primary drag carries the part to the view, which inserts it where
                            // it is let go; a double click or the menu's Open opens it as its own
                            // document (a STEP or mesh file too).
                            if r.drag_started_by(egui::PointerButton::Primary) {
                                egui::DragAndDrop::set_payload(ui.ctx(), Dragged(e.path.clone()));
                            }
                            if r.dragged_by(egui::PointerButton::Primary) {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                drag_ghost(ui, e);
                            }
                            if r.double_clicked() {
                                open = Some(e.path.clone());
                            }
                            r.context_menu(|ui| {
                                let o = crate::ui_kit::button(ui, tr("library.open"), crate::ui_kit::Button::Normal);
                                crate::scenario::publish_handle("library:menu_open", o.rect.center());
                                if o.clicked() {
                                    open = Some(e.path.clone());
                                    ui.close();
                                }
                                let i = crate::ui_kit::button(ui, tr("library.insert"), crate::ui_kit::Button::Normal);
                                crate::scenario::publish_handle("library:menu_insert", i.rect.center());
                                if i.clicked() {
                                    insert_now = Some(e.path.clone());
                                    ui.close();
                                }
                            });
                        }
                    });
                    ui.add_space(if names_only { 1.0 } else { 4.0 });
                }
                if entries.is_empty() && app.library.error.is_none() {
                    ui.label(RichText::new(tr("library.empty")).color(t.text_dim));
                }
            });
        });
    app.library.shown = Some(shown.response.rect);
    match action {
        Some("hide") => {
            app.preferences.library_open = false;
        }
        Some("collapse") => {
            app.preferences.library_collapsed = true;
        }
        Some("refresh") => refresh(app, ui.ctx(), true),
        Some("choose") => choose_folder(app),
        _ => {}
    }
    if let Some(dir) = enter {
        app.library.current = Some(dir);
        app.library.entries.clear();
        refresh(app, ui.ctx(), true);
    }
    if let Some(p) = insert_now
        && let Err(e) = insert(app, &p, Vec3::ZERO)
    {
        app.set_status(e, true);
    }
    if let Some(p) = open {
        crate::home::open_recent(app, &p.to_string_lossy());
    }
}

/// `ui.library`: show or hide the panel (`open`), narrow it (`collapsed`), set its `folder`,
/// browse into a `subfolder` of it, or `insert` a listed part (`at` mm, default the origin).
/// Returns the panel's state and what the shown folder lists.
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
    if let Some(b) = p.get("collapsed").and_then(serde_json::Value::as_bool) {
        app.preferences.library_collapsed = b;
    }
    if let Some(sub) = p.get("subfolder").and_then(serde_json::Value::as_str) {
        let root = root(app).ok_or("no library folder set")?;
        // Only names of folders inside it: no `..`, no absolute or drive paths.
        let plain = Path::new(sub).components().all(|c| matches!(c, std::path::Component::Normal(_)));
        let dir = root.join(sub);
        if !plain || !dir.is_dir() {
            return Err(format!("{sub} is not a folder in the library"));
        }
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
        "collapsed": app.preferences.library_collapsed,
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
        assert!(named.contains(&("bracket", Kind::Design)) && named.contains(&("adapter", Kind::Step)), "{named:?}");
        assert!(!named.iter().any(|(n, _)| n.contains("notes") || n.contains("hidden")), "{named:?}");
        assert!(list(&h.dir.join("missing")).is_err());
    }

    /// The panel through `ui.library`: a folder chosen, shown, browsed into, a part inserted where
    /// asked as a linked component; narrowed to a strip and hidden again. The folder stays in the
    /// preferences.
    #[test]
    fn the_library_lists_a_folder_and_inserts_its_parts() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        let dir = h.dir.to_string_lossy().to_string();
        let r = control(&mut h.app, &json!({"folder": dir})).unwrap();
        assert_eq!(r["open"], true);
        assert!(r["entries"].as_array().unwrap().iter().any(|e| e["name"] == "bracket" && e["kind"] == "design"), "{r}");
        h.frames(3);
        assert!(h.widget(crate::i18n::tr("library.title")).is_some(), "the panel shows");
        assert!(h.app.prefs().contains("library_folder"), "the folder is kept with the preferences");
        control(&mut h.app, &json!({"insert": "bracket", "at": [50, 0, 0]})).unwrap();
        let links = h.app.session.execute("component.links", &json!({})).unwrap();
        assert_eq!(links["links"][0]["name"], "bracket", "{links}");
        let b = solidcraft_engine::view::bounds(&h.app.session);
        assert!((b.min.x - 50.0).abs() < 1e-6, "placed where asked: {b:?}");
        let sub = control(&mut h.app, &json!({"subfolder": "Fasteners"})).unwrap();
        assert_eq!(sub["entries"][0]["name"], "pin", "{sub}");
        assert!(control(&mut h.app, &json!({"subfolder": ".."})).is_err(), "never above the library folder");
        assert!(control(&mut h.app, &json!({"insert": "nope"})).is_err());
        control(&mut h.app, &json!({"collapsed": true})).unwrap();
        h.frames(2);
        assert!(h.widget(crate::i18n::tr("library.title")).is_none(), "narrowed to its strip");
        control(&mut h.app, &json!({"open": false, "collapsed": false})).unwrap();
        h.frames(2);
        assert!(h.widget(crate::i18n::tr("library.title")).is_none(), "hidden");
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
    /// Open and Insert: the card's own response carries the drag, so nothing takes the pointer
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

    /// Columns (1 to 3, at most) and the card size: names only, small (104 px), normal (156) or
    /// extra large (208). Cards keep their size, so a larger size fits fewer to a row.
    #[test]
    fn the_columns_and_the_card_size_lay_the_cards_out() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        let row = |h: &mut crate::scenario::Harness, columns: u8, size: u8| {
            h.app.preferences.library_columns = columns;
            h.app.preferences.library_size = size;
            h.frames(3);
            let a = crate::scenario::handle_at("library:adapter").unwrap();
            let b = crate::scenario::handle_at("library:bracket").unwrap();
            (a, b)
        };
        // Listed folders first: Fasteners, then adapter and bracket.
        let (a, b) = row(&mut h, 3, 1);
        assert!((a.y - b.y).abs() < 1.0, "three columns: side by side {a:?} {b:?}");
        let (a, b) = row(&mut h, 1, 1);
        assert!(b.y - a.y > 0.75 * CARD_SMALL.min(150.0), "one column: one under the other {a:?} {b:?}");
        let (a, b) = row(&mut h, 1, 0);
        assert!((b.y - a.y - NAME_ROW).abs() < 8.0, "names only: one short row each {a:?} {b:?}");
        // Each size is its own size: the gap from one card to the next grows with it, and a size
        // too large for two in a row puts the second card under the first.
        let pitch = |h: &mut crate::scenario::Harness, size: u8| {
            let (a, b) = row(h, 3, size);
            (b.x - a.x, b.y - a.y)
        };
        let (small, normal, xl) = (pitch(&mut h, 1), pitch(&mut h, 2), pitch(&mut h, 3));
        let panel = h.app.library.shown.expect("the panel");
        assert!((small.0 - (CARD_SMALL + GAP)).abs() < 1.0 || panel.width() < 2.0 * CARD_SMALL, "small cards side by side {small:?}");
        assert!(small != normal && normal != xl, "three different sizes: {small:?} {normal:?} {xl:?} in {panel:?}");
    }

    /// A card let go over the library panel (a click, or a drag back to where it started) inserts
    /// nothing, though the view lies under the panel; let go over the view, it inserts.
    #[test]
    fn a_card_inserts_only_when_let_go_over_the_view() {
        let mut h = crate::scenario::Harness::new();
        parts(&h.dir);
        control(&mut h.app, &json!({"folder": h.dir.to_string_lossy()})).unwrap();
        h.frames(3);
        let panel = h.app.library.shown.expect("the panel is drawn");
        let links =
            |h: &mut crate::scenario::Harness| h.app.session.execute("component.links", &json!({})).unwrap()["links"].as_array().map_or(0, Vec::len);
        let bracket = h.dir.join("bracket.solidcraft");
        for (at, inserted) in [(panel.center(), 0), (panel.right_center() + egui::vec2(200.0, 0.0), 1)] {
            egui::DragAndDrop::set_payload(&h.ctx, Dragged(bracket.clone()));
            h.step(&json!({"click": [at.x, at.y]})).unwrap();
            h.frames(2);
            assert_eq!(links(&mut h), inserted, "let go at {at:?} (panel {panel:?})");
        }
    }
}
