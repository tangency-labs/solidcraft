//! The browser (left): the design as a tree of components, each with its bodies, sketches and
//! construction planes, plus document settings and the origin.
//!
//! Rows are drawn in code (icons from [`icons`]) and act through commands. A click selects
//! (Ctrl adds, Shift extends over the visible rows), hovering a body row lights it up in the
//! viewport, a double-click on a sketch edits it, and a right-click opens the item's menu
//! ([`crate::context_menu`]).

use crate::ui_kit::{Container, Surface};

#[path = "browser_cache.rs"]
mod cache;
#[cfg(test)]
#[path = "occurrence_ui_tests.rs"]
mod occurrence_ui_tests;

#[cfg(test)]
#[path = "occurrence_gizmo_tests.rs"]
mod occurrence_gizmo_tests;

#[path = "browser_model.rs"]
mod model;

mod features;
mod point_clouds;
pub use point_clouds::CloudPanel;
pub(crate) use point_clouds::point_cloud_tracks;

pub(crate) fn reveal_feature(app: &mut SolveApp, id: u64, body: &str) {
    features::reveal(app, id, body);
}

/// Find in Browser for a body: open every row that holds it (the document, the placements down
/// to it, its Bodies folder and any group around it) and scroll its row into view. The rows
/// are opened here, not while drawing, because a collapsed ancestor never draws its children.
pub(crate) fn reveal_body(app: &mut SolveApp, body: &str) {
    app.ui.show_browser = true;
    app.preferences.timeline_left_browser_collapsed = false;
    app.tree.reveal_feature = None;
    app.tree.reveal = Some(body.to_string());
    let state = app.session.model.state();
    let Some(source) = app.session.world_state().body(body).map(|selected| selected.definition_name().to_string()) else { return };
    let Some(component) = state.body(&source).map(|definition| definition.component) else { return };
    // Every component's first placement keeps the definition's name, so the owner must match too.
    let Some((_, path, _)) = app
        .session
        .doc
        .placements()
        .into_iter()
        .find(|(owner, path, _)| *owner == component && app.session.doc.world_body_name(&state, &source, *owner, path) == body)
    else {
        return;
    };
    app.tree.collapsed.remove("c0");
    for depth in 1..=path.len() {
        app.tree.collapsed.remove(&occurrence_key(&path[..depth]));
    }
    let scope = if path.is_empty() { format!("c{component}") } else { occurrence_key(&path) };
    app.tree.collapsed.remove(&scope);
    app.tree.collapsed.remove(&format!("{scope}/bodies"));
    open_group_chain(app, &scope, component, "bodies", &source);
}

/// Open the group holding an item, and the groups holding that one.
fn open_group_chain(app: &mut SolveApp, scope: &str, component: u64, folder: &str, key: &str) {
    let mut at = app.session.doc.group_of_item(component, folder, key).map(|g| g.id);
    for _ in 0..=solidcraft_engine::doc::MAX_GROUP_NESTING {
        let Some(id) = at else { return };
        app.tree.collapsed.remove(&if scope.is_empty() { format!("g{id}") } else { format!("{scope}/g{id}") });
        at = app.session.doc.browser_group(id).and_then(|g| g.parent);
    }
}

pub(crate) fn reveal_in_history(app: &mut SolveApp, id: u64) {
    features::reveal_in_history(app, id);
}

#[cfg(test)]
#[path = "browser_model_tests.rs"]
mod model_tests;

use std::collections::BTreeSet;
use std::sync::{Arc, Weak};

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::{BrowserEntry, Document, FeatureKind};

use crate::context_menu::{Item, Target};
use crate::theme::Tokens;

use crate::SolveApp;

thread_local! {
    /// The rows drawn this frame: (label, row, fold arrow area, open). For automation
    /// (`ui.at {browser | chevron}`, `ui.browser`).
    static DRAWN: std::cell::RefCell<Vec<(String, Rect, Option<Rect>, Option<bool>)>> = const { std::cell::RefCell::new(Vec::new()) };
    static OCCURRENCES: std::cell::RefCell<Vec<(Vec<u64>, String, Rect, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The rows on screen: (label, row rect, fold arrow area, open).
pub fn drawn_rows() -> Vec<(String, Rect, Option<Rect>, Option<bool>)> {
    DRAWN.with(|d| d.borrow().clone())
}

pub fn drawn_occurrences() -> Vec<(Vec<u64>, String, Rect, bool)> {
    OCCURRENCES.with(|d| d.borrow().clone())
}

/// Browser state kept between frames.
#[derive(Default)]
pub struct TreeState {
    /// Geometry and assembly queries, independent of the camera, selection and row visibility.
    model: Option<model::Model>,
    /// Folders closed by the user (keys like `c0/bodies`).
    pub collapsed: BTreeSet<String>,
    /// Folders opened by the user (folders that start closed).
    pub expanded: BTreeSet<String>,
    /// Scroll to this body's row (by the name it has in the world, which tells the placements
    /// of a part apart) and open the components, placements and folders above it: Find in
    /// Browser, and a body picked in the viewport (#3005).
    pub reveal: Option<String>,
    /// A native feature to reveal in its placed component's Features folder (Find in Timeline).
    pub(super) reveal_feature: Option<features::Reveal>,
    /// The Move/Copy panel of an occurrence.
    pub occurrence_move: Option<OccMove>,
    /// The Redefine Sketch Plane panel (sketch id).
    pub redefine: Option<u64>,
    pub redefine_draft: Option<crate::sketch_attachment_ui::Draft>,
    /// Components picked in the tree (they have no viewport selection).
    pub picked_components: Vec<u64>,
    /// The last folder label clicked, for V; independent of the geometry selection.
    picked_folder: Option<FolderPick>,
    /// Canvases picked in the tree.
    pub picked_canvases: Vec<u64>,
    /// Joints picked in the tree (a hidden one shows highlighted while it is picked).
    pub picked_joints: Vec<u64>,
    /// Folders picked in the feature history (features use the engine selection).
    pub picked_timeline_groups: Vec<u64>,
    /// Last feature-history item clicked, for Shift ranges (`f:<id>` or `g:<id>`).
    pub timeline_anchor: Option<String>,
    /// Find in Timeline: the feature the history scrolls to the next time it draws its items, in
    /// the bottom strip or in the history on the left (#5850).
    pub timeline_reveal: Option<u64>,
    /// Show components marked out of the History bar anyway (#10393). Held by the session, like
    /// the bar's Show All toggle; the History controls' menu carries the switch.
    pub history_show_hidden: bool,
    /// The feature of the face or edge last picked in the view, so a new pick scrolls the
    /// history to it once.
    pub view_pick: Option<u64>,
    /// The Edit Canvas / Calibrate panel.
    pub canvas_panel: Option<CanvasPanel>,
    /// The bodies a row at the top of the history lights this frame (`history_rows`), for the
    /// browser, drawn after it, to keep.
    history_hover: Vec<String>,
    /// The Edit Point Cloud panel.
    pub cloud_panel: Option<CloudPanel>,
    /// The plane a row at the top of the history lights this frame, kept the same way (#6975).
    history_plane: Option<String>,
    /// A Section Analysis hidden with its eye in the Analysis folder: its plane (origin,
    /// normal), so the eye can bring it back (#822).
    pub hidden_section: Option<(solidcraft_engine::geom::Vec3, solidcraft_engine::geom::Vec3)>,
    /// What the Section Analysis dialog's OK last made, for Edit to start from.
    pub section_dialog: Option<crate::dialogs::SectionDraft>,
    /// The Analysis folder hidden with its own eye, and the Section Analysis hidden with its
    /// own: the section shows only while neither is, and each eye keeps its state.
    pub analysis_hidden: bool,
    pub section_hidden: bool,
    /// What waits for the Capture Position question: a command id, or [`SAVE`].
    pub capture_prompt: Option<String>,
    /// The design, undo depth and visibility when the button last went down: a drag over the eyes
    /// merges the steps it made into one (`eye_drag_frame`).
    eye_press: Option<(Arc<Document>, usize, solidcraft_engine::Visibility)>,
    /// The last row clicked (for Shift ranges).
    anchor: Option<String>,
    /// Row keys in drawing order this frame (for Shift ranges).
    order: Vec<String>,
    /// The previous frame's complete row order: a Shift click resolves its range here, since
    /// this frame's `order` does not yet hold the rows below the clicked one (#4991).
    order_prev: Vec<String>,
    order_len: usize,
    cache: Option<cache::Cache>,
}

struct FolderPick {
    key: String,
    session: u64,
    doc: Weak<Document>,
}

impl FolderPick {
    fn current(&self, app: &SolveApp) -> bool {
        self.session == app.session.uid && std::ptr::eq(self.doc.as_ptr(), Arc::as_ptr(&app.session.doc))
    }
}

impl TreeState {
    pub(crate) fn clear_folder_pick(&mut self) {
        self.picked_folder = None;
    }

    fn remember(&mut self, key: &str) {
        if let Some(slot) = self.order.get_mut(self.order_len) {
            if slot != key {
                slot.clear();
                slot.push_str(key);
            }
        } else {
            self.order.push(key.to_owned());
        }
        self.order_len += 1;
    }
}

/// Moving an occurrence: the offsets typed so far (pending until Capture Position).
#[derive(Clone, Debug)]
pub struct OccMove {
    pub occurrence: u64,
    /// Exact assembly placement used for world-space movement and the pivot.
    pub path: Vec<u64>,
    /// Effective local placement at dialog opening, including its pre-existing pending move.
    pub baseline: Option<solidcraft_engine::doc::Mat>,
    pub had_pending: bool,
    /// Distances along X, Y, Z (mm) and turns about X, Y, Z (degrees) from where it was.
    pub translate: [f64; 3],
    pub angles: [f64; 3],
    /// The occurrence's origin when the move began: what it turns about.
    pub pivot: Option<solidcraft_engine::geom::Vec3>,
    /// The values when the current triad drag began.
    drag0: Option<([f64; 3], [f64; 3])>,
}

impl OccMove {
    pub fn new(occurrence: u64) -> Self {
        OccMove { occurrence, path: Vec::new(), baseline: None, had_pending: false, translate: [0.0; 3], angles: [0.0; 3], pivot: None, drag0: None }
    }
}

// ---------------------------------------------------------------------------------------------
// Queries used by the menus
// ---------------------------------------------------------------------------------------------

/// Is a sketch shown itself (its eye)? A hidden Sketches folder or component still keeps it out
/// of the viewport (`solidcraft_engine::view::sketch_shown`).
pub fn sketch_visible(app: &SolveApp, id: u64) -> bool {
    if app.session.visibility.hidden_sketches.contains(&id) {
        return false;
    }
    app.session.visibility.shown_sketches.contains(&id) || !solidcraft_engine::view::sketch_consumed(&app.session, id)
}

pub fn set_sketch_visible(app: &mut SolveApp, id: u64, on: bool) {
    // Through the command, so showing or hiding a sketch is an undo step too (#2219).
    let _ = app.run("browser.visibility", json!({ "items": [id], "visible": on }));
}

pub fn redefine_sketch(app: &mut SolveApp, id: u64) {
    app.tree.redefine = Some(id);
    app.tree.redefine_draft = Some(crate::sketch_attachment_ui::Draft::new(app, id));
}

fn find_group(app: &SolveApp, id: u64) -> Option<&solidcraft_engine::doc::BrowserGroup> {
    app.session.doc.browser_groups.iter().find(|g| g.id == id)
}

/// World bodies of a component picked in the tree. The root is the whole design; a nested
/// component is its own placement, including bodies inside it.
pub(crate) fn bodies_of_component(app: &SolveApp, id: u64) -> Vec<String> {
    let present: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let mut names = if id == 0 {
        present.clone()
    } else {
        let mut names = Vec::new();
        let path = app.session.doc.first_path(id);
        if !path.is_empty()
            && let Ok(more) = app.session.occurrence_bodies(&path, true)
        {
            names.extend(more);
        }
        names.extend(crate::context_menu::component_bodies(app, id));
        names
    };
    names.retain(|n| present.contains(n) && !app.session.body_removed_later(n));
    let mut kept = Vec::new();
    for name in names {
        if !kept.contains(&name) {
            kept.push(name);
        }
    }
    kept
}

/// How a browser click changes a dialog input that takes bodies.
enum Take {
    /// A second click on the same set removes it.
    Toggle,
    /// A Shift range only adds.
    Add,
}

/// Put these bodies into the open dialog's active input, the same way as clicking each one
/// ([`pick_from_browser`]). Returns false when that input does not take bodies, so the click
/// keeps its ordinary meaning. A Shift range only adds; a group or component toggles.
fn offer_bodies(app: &mut SolveApp, names: &[String], take: Take) -> bool {
    if !dialog_takes_body(app) || app.dialog.as_ref().is_some_and(|d| d.takes_components()) {
        return false;
    }
    let world = app.session.world_state();
    let picked: Vec<Sel> =
        names.iter().filter(|n| world.body(n).is_some() && !app.session.body_removed_later(n)).map(|n| Sel::Body { name: n.clone() }).collect();
    if picked.is_empty() {
        return false;
    }
    let Some(mut d) = app.dialog.take() else { return false };
    for body in picked {
        if !d.active_input().is_some_and(|i| i.accept & crate::selection::BODIES != 0) {
            break;
        }
        if matches!(take, Take::Add) && d.active_input().is_some_and(|i| i.items.contains(&body)) {
            continue;
        }
        d.pick(&app.session, body);
    }
    app.dialog = Some(d);
    true
}

/// The bodies in a group (none for sketch and plane groups).
pub fn group_bodies(app: &SolveApp, id: u64) -> Vec<String> {
    let st = app.session.world_state();
    find_group(app, id)
        .filter(|g| g.folder == "bodies")
        .map(|_| app.session.doc.group_item_keys(id).into_iter().filter(|b| st.body(b).is_some()).collect())
        .unwrap_or_default()
}

pub fn group_name(app: &SolveApp, id: u64) -> Option<String> {
    find_group(app, id).map(|g| g.name.clone())
}

/// The placement a folder's scope key names: `c0` the design, `o:<ids>` a placement's path.
fn scope_path(scope: &str) -> Vec<u64> {
    scope.strip_prefix("o:").map(|ids| ids.split('/').filter_map(|id| id.parse().ok()).collect()).unwrap_or_default()
}

/// Paste on a Bodies or Sketches folder or group: copied (or cut) bodies, or copied sketches and
/// features, go into its component (that placement), and into the group.
fn paste_item(app: &SolveApp, component: u64, scope: &str, folder: &str, group: Option<u64>) -> Item {
    let mut item = Item::action("ui.paste", "Paste", "paste")
        .key("Ctrl+V")
        .with(json!({ "component": component, "path": scope_path(scope), "group": group, "folder": folder }));
    item.enabled = if folder == "sketches" { crate::edit_clipboard::pasting_features(app) } else { crate::edit_clipboard::pasting_bodies(app) };
    item
}

/// The menu of a group row.
pub fn group_items(app: &SolveApp, id: u64, scope: &str) -> Vec<Item> {
    let Some(g) = find_group(app, id) else { return Vec::new() };
    let mut v = Vec::new();
    // Paste beside Rename, as the clipboard sits beside Delete and Rename in every menu.
    if g.folder == "bodies" || g.folder == "sketches" {
        v.push(paste_item(app, g.component, scope, &g.folder, Some(id)));
    }
    // A group inside this one, unless it is already as deep as groups go.
    if app.session.doc.group_level(id) < solidcraft_engine::doc::MAX_GROUP_NESTING {
        v.push(Item::action("ui.newGroup", "New Group", "folder").with(json!({ "component": g.component, "folder": g.folder, "parent": id })));
    }
    v.extend([
        Item::action("ui.rename", "Rename", "rename").key("F2").with(json!({ "group": id })),
        Item::action("ui.ungroup", "Ungroup", "folder").with(json!({ "group": id })),
        Item::sep(),
        // The group itself: its items keep their own state.
        Item::action("ui.folderVisible", "Show/Hide", "eye").key("V").with(json!({ "key": format!("{scope}/g{id}") })),
    ]);
    if g.folder == "bodies" {
        v.push(Item::action("ui.isolate", "Isolate", "body").with(json!({ "bodies": group_bodies(app, id) })));
    }
    v
}

/// Move Sketches to Their Parts (`component.move_sketches.to_parts`) for `sketches` (ids).
pub(crate) fn sketches_to_parts_item(sketches: Vec<String>) -> Item {
    Item::action("component.move_sketches.to_parts", "Move Sketches to Their Parts", "move").with(json!({ "sketches": sketches }))
}

/// The menu of a folder row (Bodies, Sketches, Construction).
pub fn folder_items(app: &SolveApp, component: u64, folder: &str, scope: &str) -> Vec<Item> {
    let sel: Vec<String> = selected_keys(app, component, folder);
    let all = entries(app, component, folder);
    let mut v = Vec::new();
    if folder == "bodies" {
        let bodies: Vec<String> = all.iter().map(|e| e.key.clone()).collect();
        v.push(paste_item(app, component, scope, folder, None));
        v.push(Item::action("component.from_bodies", "Create Components from Bodies", "component").with(json!({ "bodies": bodies })));
    }
    if folder == "sketches" {
        v.push(paste_item(app, component, scope, folder, None));
        // The folder's selected sketches, or all of them, go to the part whose face they are on (#10218).
        if !app.session.doc.components.is_empty() && !all.is_empty() {
            let sketches = if sel.is_empty() { all.iter().map(|e| e.key.clone()).collect() } else { sel.clone() };
            v.push(sketches_to_parts_item(sketches));
        }
    }
    v.push(Item::action("ui.newGroup", "New Group", "folder").with(json!({ "component": component, "folder": folder })));
    if sel.len() > 1 {
        v.push(Item::action("ui.groupSelected", "Group Selected", "folder").with(json!({ "component": component, "folder": folder, "items": sel })));
    }
    v.push(Item::sep());
    v.push(Item::action("ui.folderVisible", "Show/Hide", "eye").key("V").with(json!({ "key": format!("{scope}/{folder}") })));
    v.push(Item::action("ui.showAll", "Show All", "eye"));
    v
}

/// A component's or placement's menu, while its Bodies folder is folded away (#4217), takes the
/// folder's items it lacks: Create Components from Bodies and Group Selected (New Group and Show
/// All are on every component's menu).
pub fn folded_folder_items(app: &SolveApp, target: &Target) -> Vec<Item> {
    let (comp, scope) = match target {
        Target::Component { id } => (*id, format!("c{id}")),
        Target::Occurrence { path } => match app.session.occurrence_placement(path) {
            Ok(placement) => (placement.component, occurrence_key(path)),
            Err(_) => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    if !bodies_folded(app, comp, &scope, entries(app, comp, "bodies").len()) {
        return Vec::new();
    }
    let items =
        folder_items(app, comp, "bodies", &scope).into_iter().filter(|i| matches!(i.id.as_str(), "component.from_bodies" | "ui.groupSelected"));
    std::iter::once(Item::sep()).chain(items).collect()
}

/// The menu of the Origin folder.
pub fn origin_items(app: &SolveApp) -> Vec<Item> {
    let planes = ["XY", "XZ", "YZ"].iter().all(|p| !app.session.visibility.hidden_origin.iter().any(|h| h == p));
    let axes = ["X", "Y", "Z"].iter().all(|p| !app.session.visibility.hidden_origin.iter().any(|h| h == p));
    vec![
        Item::action("ui.origin", "Show/Hide", "eye").key("V"),
        Item::action("ui.originAll", "Show All", "eye"),
        Item::action("ui.originPart", if planes { "Hide Planes" } else { "Show Planes" }, "plane")
            .with(json!({ "keys": ["XY", "XZ", "YZ"], "show": !planes })),
        Item::action("ui.originPart", if axes { "Hide Axes" } else { "Show Axes" }, "axis").with(json!({ "keys": ["X", "Y", "Z"], "show": !axes })),
    ]
}

/// Show or hide a browser folder or group itself, by tree key (`c0/bodies`, `o:4/g7`): its
/// items keep their own state, and hiding or showing them never changes it.
pub fn toggle_folder(app: &mut SolveApp, key: &str) {
    let _ = app.run("browser.visibility", json!({ "tree_folders": [key], "visible": app.session.folder_hidden(key) }));
}

/// A drag over the eyes is one undo step (#9680). When the button goes down the undo depth and the
/// hidden items are noted; when it comes up, the steps the drag made are merged into the first of
/// them. Runs every frame, whatever else is open, so a release is never missed.
pub(crate) fn eye_drag_frame(app: &mut SolveApp, ctx: &egui::Context) {
    let (pressed, down) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
    if pressed {
        app.tree.eye_press = Some((app.session.doc.clone(), app.session.undo.len(), without_origin(app.session.visibility.clone())));
    }
    if down {
        return;
    }
    let dragged = ctx.data_mut(|d| d.remove_temp::<bool>(crate::ui_kit::eye_drag_id())).is_some();
    let Some((doc, depth, visibility)) = app.tree.eye_press.take().filter(|_| dragged) else { return };
    // shortcut: once the history is full (200 steps) its length stops growing, so the drag is left as
    // separate steps, like the point-cloud and canvas drags; the visibility test keeps a trimmed front
    // from being merged wrongly.
    let undo = &mut app.session.undo;
    if undo.len() > depth + 1 && undo.get(depth).is_some_and(|s| Arc::ptr_eq(&s.doc, &doc) && without_origin(s.visibility.clone()) == visibility) {
        undo.truncate(depth + 1);
    }
}

/// The Origin's own eye is not an undo step (`set_origin_shown`), so the drag's steps do not hold it:
/// it is left out of the comparison.
fn without_origin(mut visibility: solidcraft_engine::Visibility) -> solidcraft_engine::Visibility {
    visibility.origin_hidden = None;
    visibility
}

/// A folder shortcut target lasts until another click, Escape, or a change of design/model.
/// Visibility changes keep the document snapshot, so V can toggle the same folder again.
pub(crate) fn update_folder_pick(app: &mut SolveApp, ctx: &egui::Context) {
    if ctx.input(|i| i.pointer.any_pressed() || i.key_pressed(egui::Key::Escape))
        || app.tree.picked_folder.as_ref().is_some_and(|pick| !pick.current(app))
    {
        app.tree.clear_folder_pick();
    }
}

fn pick_folder(app: &mut SolveApp, key: &str) {
    app.tree.picked_folder = Some(FolderPick { key: key.into(), session: app.session.uid, doc: Arc::downgrade(&app.session.doc) });
}

/// The group whose row was clicked last (its key ends in `/g<id>`), while that pick still holds
/// and the group exists: F2 renames it (#10005).
pub(crate) fn picked_group(app: &SolveApp) -> Option<u64> {
    let pick = app.tree.picked_folder.as_ref().filter(|pick| pick.current(app))?;
    let id = pick.key.rsplit_once("/g")?.1.parse().ok()?;
    find_group(app, id).map(|g| g.id)
}

pub(crate) fn toggle_picked_folder(app: &mut SolveApp) -> bool {
    let Some(pick) = app.tree.picked_folder.as_ref().filter(|pick| pick.current(app)) else { return false };
    let key = pick.key.clone();
    toggle_folder(app, &key);
    true
}

/// Selected browser items of the folder of a body or sketch, when more than one (for Group
/// Selected in their menus).
pub fn selection_group(app: &SolveApp, target: &Target) -> Option<Value> {
    let (comp, folder) = match target {
        Target::Body { name } => {
            let st = app.session.world_state();
            let b = st.body(name)?;
            (b.component, "bodies")
        }
        Target::Sketch { id } => (app.session.doc.feature(*id)?.component, "sketches"),
        Target::Plane { id } => (app.session.doc.feature(*id)?.component, "construction"),
        _ => return None,
    };
    let sel = selected_keys(app, comp, folder);
    (sel.len() > 1).then(|| json!({ "component": comp, "folder": folder, "items": sel }))
}

/// Group actions from the menus.
pub fn group_action(app: &mut SolveApp, action: &str, p: &Value) {
    match action {
        "ui.newGroup" | "ui.groupSelected" => {
            let folder = p.get("folder").and_then(Value::as_str).unwrap_or("bodies");
            let comp = p.get("component").and_then(Value::as_u64).unwrap_or(0);
            let items = p.get("items").cloned().unwrap_or(json!([]));
            let parent = p.get("parent").and_then(Value::as_u64);
            if let Ok(r) = app.run("browser.group", json!({ "component": comp, "folder": folder, "items": items, "parent": parent }))
                && let Some(g) = r.get("group").and_then(Value::as_u64)
            {
                if let Some(parent) = parent {
                    app.tree.collapsed.remove(&format!("c{comp}/g{parent}"));
                }
                app.tree.collapsed.remove(&format!("c{comp}/{folder}"));
                app.tree.collapsed.remove(&format!("c{comp}/g{g}"));
            }
        }
        "ui.ungroup" => {
            if let Some(g) = p.get("group") {
                let _ = app.run("browser.ungroup", json!({ "group": g }));
            }
        }
        "ui.renameGroup" => {
            let _ = app.run("browser.rename_group", p.clone());
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// The floating card's inner margin: the header highlight sits this far from the card's top, left
/// and right edges, so its corners are concentric with the card's.
const CARD_INSET: f32 = 4.0;
const HEADER_H: f32 = 26.0;
const INDENT: f32 = 14.0;
use crate::ui_kit::RowResp;

fn row_rect(ui: &egui::Ui) -> Rect {
    crate::ui_kit::tree_row_rect(ui, &Row { compact: compact_tree(ui), ..Default::default() })
}

/// One row's look.
pub(crate) use crate::ui_kit::{Mark, Row};

fn compact_tree(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("sc_compact_tree"))).unwrap_or(false)
}

/// Where tree rows put their eye this frame (Preferences › Interface, #8718).
pub(crate) fn publish_row_prefs(app: &SolveApp, ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_tree_eye_left"), app.preferences.browser_eye_left));
}

fn eye_left(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::new("sc_tree_eye_left"))).unwrap_or(false)
}

pub(crate) fn draw_row(ui: &mut egui::Ui, id: egui::Id, row: &Row<'_>) -> RowResp {
    let compact = compact_tree(ui);
    let out = crate::ui_kit::tree_row(ui, id, &Row { compact, eye_left: eye_left(ui), ..*row });
    if let Some(r) = out.rect.filter(|r| ui.is_rect_visible(*r)) {
        let x = r.left() + if compact { Tokens::get().space_8 } else { 2.0 } + row.depth as f32 * INDENT;
        let fold = row.fold.map(|_| Rect::from_min_max(pos2(x - 4.0, r.top()), pos2(x + 15.0, r.bottom())));
        DRAWN.with(|d| d.borrow_mut().push((row.label.to_string(), r, fold, row.fold)));
    }
    out
}

/// A Ctrl/Shift-aware click on a selectable row.
fn click_select(app: &mut SolveApp, ui: &egui::Ui, key: String, sel: Option<Sel>) {
    let mods = ui.input(|i| i.modifiers);
    if mods.shift
        && let Some(a) = app.tree.anchor.as_deref()
    {
        let order = app.tree.order.get(..app.tree.order_len).unwrap_or(&[]);
        let items = shift_range(&app.tree.order_prev, a, &key).or_else(|| shift_range(order, a, &key));
        if let Some(items) = items {
            // Move/Copy (and any open command whose active input takes bodies) gets every body
            // in the range. Otherwise the range is an ordinary selection.
            let names: Vec<String> = items
                .iter()
                .filter_map(|s| match s {
                    Sel::Body { name } => Some(name.clone()),
                    _ => None,
                })
                .collect();
            if offer_bodies(app, &names, Take::Add) {
                return;
            }
            let _ = app.run("select.set", json!({ "items": items, "add": mods.command || mods.ctrl }));
            return;
        }
    }
    app.tree.anchor = Some(key);
    let Some(sel) = sel else { return };
    if mods.command || mods.ctrl {
        if app.session.selection.contains(&sel) {
            let rest: Vec<Sel> = app.session.selection.iter().filter(|x| **x != sel).cloned().collect();
            let _ = app.run("select.set", json!({ "items": rest }));
        } else {
            let _ = app.run("select.set", json!({ "items": [sel], "add": true }));
        }
    } else {
        app.tree.picked_components.clear();
        app.tree.picked_joints.clear();
        let _ = app.run("select.set", json!({ "items": [sel] }));
    }
}

/// The selectable rows from `anchor` to `key` (either way round) in a row order, or `None`
/// when the order lacks either of them.
fn shift_range(order: &[String], anchor: &str, key: &str) -> Option<Vec<Sel>> {
    let i = order.iter().position(|k| k == anchor)?;
    let j = order.iter().position(|k| k == key)?;
    let rows = order.get(i.min(j)..=i.max(j))?;
    Some(rows.iter().filter_map(|k| sel_of_key(k)).collect())
}

fn sel_of_key(k: &str) -> Option<Sel> {
    if let Some((_, id)) = k.rsplit_once("/s:") {
        return id.parse().ok().map(|id| Sel::Feature { id });
    }
    if let Some(p) = k.strip_prefix("o:") {
        let path: Option<Vec<u64>> = p.split('/').map(|id| id.parse().ok()).collect();
        return path.filter(|p| !p.is_empty()).map(|path| Sel::Occurrence { path });
    }
    if let Some(b) = k.strip_prefix("b:") {
        return Some(Sel::Body { name: b.to_string() });
    }
    if let Some(s) = k.strip_prefix("s:").or_else(|| k.strip_prefix("f:")) {
        return s.parse().ok().map(|id| Sel::Feature { id });
    }
    None
}

/// The body a selection item is on: a body, or a face or edge of one (by its name in the
/// world).
pub(crate) fn picked_body(sel: &Sel) -> Option<&str> {
    match sel {
        Sel::Body { name } => Some(name),
        Sel::Face { body, .. } | Sel::Edge { body, .. } => Some(body),
        _ => None,
    }
}

/// A pick in the viewport: the Browser, when shown, opens down to the picked body's row and
/// scrolls to it (#3005). The body of the last item picked.
pub(crate) fn reveal_pick(app: &mut SolveApp) {
    if app.ui.show_browser
        && let Some(body) = app.session.selection.last().and_then(picked_body)
    {
        app.tree.reveal = Some(body.to_string());
    }
}

/// The placement path of the body being revealed, resolved once per model: a row opens when
/// the body's path runs through it, instead of every row expanding its subtree to test
/// containment (#7502). `None` when nothing is revealed or the body is a top-level design body.
fn reveal_path(app: &mut SolveApp) -> Option<Vec<u64>> {
    let body = app.tree.reveal.clone()?;
    prepare_model(app);
    app.tree.model.as_mut()?.body_path(&app.session, &body)
}

pub(crate) fn is_open(app: &SolveApp, key: &str, default: bool) -> bool {
    if default { !app.tree.collapsed.contains(key) } else { app.tree.expanded.contains(key) }
}

pub(crate) fn toggle(app: &mut SolveApp, key: &str, default: bool) {
    if default {
        if !app.tree.collapsed.remove(key) {
            app.tree.collapsed.insert(key.to_string());
        }
    } else if !app.tree.expanded.remove(key) {
        app.tree.expanded.insert(key.to_string());
    }
}

/// Apply Expand All / Collapse All: open-by-default keys live in `collapsed` when closed;
/// closed-by-default keys live in `expanded` when open.
pub fn apply_fold(app: &mut SolveApp, open: bool, open_by_default: &[String], closed_by_default: &[String]) {
    if open {
        for key in open_by_default {
            app.tree.collapsed.remove(key);
        }
        for key in closed_by_default {
            app.tree.expanded.insert(key.clone());
        }
    } else {
        for key in open_by_default {
            app.tree.collapsed.insert(key.clone());
        }
        for key in closed_by_default {
            app.tree.expanded.remove(key);
        }
    }
}

/// Everything a frame's rows want done, applied after drawing.
#[derive(Default)]
struct Actions {
    menu: Option<(Pos2, Target)>,
    hover_bodies: Vec<String>,
    hovered: bool,
    /// The plane whose row is under the pointer, lit in the view (#6975).
    hover_plane: Option<String>,
    drop: Option<DropAction>,
    /// What the selection and the 3D view's pointer light in the tree.
    lit: Lit,
}

/// Something in the 3D view with a row in the tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Thing {
    Body(String),
    /// A sketch, by feature id.
    Sketch(u64),
    /// An origin or construction plane, by name.
    Plane(String),
    /// An origin or construction axis.
    Axis(String),
    /// A construction point.
    Point(String),
}

/// What the tree lights for the selection and the 3D view's pointer: each lit item's row (in
/// the selection colour when selected, the hover colour when under the pointer, with a bar
/// down its left), and a faint bar on each row leading to it.
#[derive(Default)]
struct Lit {
    /// Selected, or part of something selected: a body for its faces, edges and vertices, a
    /// sketch for its curves, points and profiles.
    selected: std::collections::HashSet<Thing>,
    /// Under the pointer in the 3D view.
    hovered: Option<Thing>,
    /// The placements holding a lit body, and the components holding a lit sketch or plane.
    paths: Vec<Vec<u64>>,
    components: Vec<u64>,
}

impl Lit {
    fn of(app: &mut SolveApp) -> Lit {
        use crate::viewport::Hit;
        let mut selected = std::collections::HashSet::new();
        // Bodies chosen in an open dialog light up even when the click was in the 3D view,
        // which does not rewrite the browser selection.
        let dialog_picks = app.dialog.as_ref().map(|d| d.items()).unwrap_or_default();
        for s in app.session.selection.iter().chain(&dialog_picks) {
            let thing = match s {
                Sel::Body { name } => Thing::Body(name.clone()),
                // A picked face, edge or vertex lights the feature it belongs to (see
                // `browser::features`), not the whole body as if the body were selected.
                Sel::Face { .. } | Sel::Edge { .. } | Sel::Vertex { .. } => continue,
                Sel::Feature { id } => Thing::Sketch(*id),
                Sel::Profile { sketch, .. } | Sel::ExternalSketchCurve { sketch, .. } | Sel::ExternalSketchPoint { sketch, .. } => {
                    Thing::Sketch(*sketch)
                }
                Sel::SketchCurve { .. } | Sel::SketchPoint { .. } | Sel::SketchMidpoint { .. } | Sel::SketchConstraint { .. } => {
                    match app.session.active_sketch {
                        Some(id) => Thing::Sketch(id),
                        None => continue,
                    }
                }
                Sel::Plane { name } => Thing::Plane(name.clone()),
                Sel::Axis { name } => Thing::Axis(name.clone()),
                Sel::ConstructionPoint { name } => Thing::Point(name.clone()),
                Sel::Occurrence { .. } => continue,
            };
            selected.insert(thing);
        }
        let hovered = match &app.viewport.hover {
            Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => Some(Thing::Body(body.clone())),
            Some(
                Hit::SketchPoint { sketch, .. } | Hit::SketchMidpoint { sketch, .. } | Hit::SketchCurve { sketch, .. } | Hit::Profile { sketch, .. },
            ) => Some(Thing::Sketch(*sketch)),
            Some(Hit::Plane { name, .. }) => Some(Thing::Plane(name.clone())),
            Some(Hit::Axis { name }) => Some(Thing::Axis(name.clone())),
            Some(Hit::ConstructionPoint { name }) => Some(Thing::Point(name.clone())),
            None => None,
        };
        let doc = app.session.doc.clone();
        let (mut paths, mut components) = (Vec::new(), Vec::new());
        for thing in selected.iter().chain(&hovered) {
            match thing {
                Thing::Body(b) => {
                    if let Some(path) = app.tree.model.as_mut().and_then(|model| model.body_path(&app.session, b)) {
                        paths.push(path);
                    }
                }
                Thing::Sketch(id) => {
                    components.extend(doc.features.iter().find(|f| f.id == *id && matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.component))
                }
                Thing::Plane(name) | Thing::Axis(name) | Thing::Point(name) => components.extend(
                    doc.features
                        .iter()
                        .find(|f| {
                            f.name == *name
                                && matches!(
                                    f.kind,
                                    FeatureKind::ConstructionPlane { .. }
                                        | FeatureKind::ConstructionAxis { .. }
                                        | FeatureKind::ConstructionPoint { .. }
                                )
                        })
                        .map(|f| f.component),
                ),
            }
        }
        Lit { selected, hovered, paths, components }
    }

    fn any(&self) -> bool {
        !self.selected.is_empty() || self.hovered.is_some()
    }

    /// Whether `thing` is lit: (selected, under the pointer).
    fn thing(&self, thing: &Thing) -> (bool, bool) {
        (self.selected.contains(thing), self.hovered.as_ref() == Some(thing))
    }

    /// Whether a folder entry is lit: (selected, under the pointer). Its own `selected` (picked
    /// in the tree, or the sketch being edited) counts as selected.
    fn entry(&self, app: &SolveApp, e: &Entry) -> (bool, bool) {
        let (selected, hovered) = self.thing(&e.thing);
        (e.is_selected(app) || selected, hovered)
    }

    /// The mark of a row holding these entries: a faint bar when any of them is lit.
    fn branch<'e>(&self, app: &SolveApp, entries: impl IntoIterator<Item = &'e Entry>) -> Mark {
        if entries.into_iter().any(|e| self.entry(app, e) != (false, false)) { Mark::Branch } else { Mark::None }
    }

    /// The mark of a placement row: a faint bar when a lit body is placed in it or below it, or
    /// a lit sketch or plane belongs to its component or one inside it.
    fn placement(&self, doc: &solidcraft_engine::doc::Document, path: &[u64], component: u64) -> Mark {
        let below = self.paths.iter().any(|p| p.starts_with(path)) || self.components.iter().any(|c| doc.component_within(*c, component));
        if below { Mark::Branch } else { Mark::None }
    }
}

/// A lit row's look: `(selected, under the pointer)` to the row's selection flag and mark.
fn lit_row((selected, hovered): (bool, bool)) -> (bool, Mark) {
    (selected, if selected || hovered { Mark::Here } else { Mark::None })
}

impl Actions {
    fn hover<'a>(&mut self, bodies: impl Iterator<Item = &'a String>) {
        let mut len = 0;
        for body in bodies {
            if let Some(slot) = self.hover_bodies.get_mut(len) {
                if slot != body {
                    slot.clone_from(body);
                }
            } else {
                self.hover_bodies.push(body.clone());
            }
            len += 1;
        }
        self.hover_bodies.truncate(len);
        self.hovered = true;
    }
}

/// How narrow the browser sidebar may be dragged, and how wide (the viewport keeps the rest).
const MIN_BROWSER_WIDTH: f32 = 170.0;
const MAX_BROWSER_WIDTH: f32 = 620.0;
/// How wide a grip the sidebar's right edge offers, counted inwards from the edge.
const BROWSER_RAIL: f32 = 6.0;
/// How opaque the floating browser's card is; the workspace shows through the rest.
pub(crate) const BROWSER_FLOAT_ALPHA: f32 = 0.4;

pub(crate) fn browser_width(remembered: f32) -> f32 {
    if remembered.is_finite() { remembered.clamp(MIN_BROWSER_WIDTH, MAX_BROWSER_WIDTH) } else { crate::prefs::DEFAULT_BROWSER_WIDTH }
}

/// Either card in a stacked column changes the same remembered width.
pub(crate) fn resize_column(app: &mut SolveApp, ui: &egui::Ui, rect: Rect, width: f32, handle: &'static str) {
    let rail = Rect::from_x_y_ranges(rect.right() - BROWSER_RAIL..=rect.right(), rect.y_range());
    let response =
        ui.interact(rail, egui::Id::new(("sc_shared_column_rail", handle)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged() {
        let delta = ui.input(|input| input.pointer.delta().x);
        if delta.is_finite() {
            app.preferences.browser_width = (width + delta).clamp(MIN_BROWSER_WIDTH, MAX_BROWSER_WIDTH);
        }
    }
    crate::scenario::publish_handle(handle, pos2(rect.right() - BROWSER_RAIL / 2.0, rect.center().y));
}

/// The width of the browser folded to its rail beside a left History.
pub(crate) fn collapsed_width() -> f32 {
    let t = Tokens::get();
    t.space_24 + t.space_10
}

/// How the browser card shows (Preferences › Interface › Layout, #8692).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserShow {
    /// Always on screen (folded to its header when the user folds it).
    #[default]
    Shown,
    /// Out of sight until the pointer comes over its place or the strip at its left edge, as the
    /// navigation bar's "Hide until hovered"; never folded, so the whole card is the target.
    Hover,
}

/// Does the browser hide until hovered? Only its own card does: not the one sharing its column
/// with History, nor the Render page's.
pub(crate) fn reveals(app: &SolveApp) -> bool {
    app.preferences.browser_show == BrowserShow::Hover && !crate::timeline_left::stacked(app) && !crate::render_workspace::active(app)
}

/// How much of the browser set to reveal on hover shows this frame (0 hidden, 1 shown). The
/// pointer over last frame's card or the strip at its left edge shows it; a menu, popup, drag or
/// text edit started while it showed keeps it shown. A drag started elsewhere (orbiting the view)
/// passes over it without showing it.
fn reveal_opacity(app: &SolveApp, ctx: &egui::Context, previous: Option<Rect>, edge: Rect) -> f32 {
    let card = egui::Id::new("sc_browser_card");
    let memory = card.with("reveal");
    let was = ctx.data(|d| d.get_temp::<bool>(memory)).unwrap_or(false);
    let over = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
        let layer = ctx.layer_id_at(p);
        let on_card = previous.is_some_and(|r| r.contains(p)) && layer.is_some_and(|l| l.id == card);
        let on_edge = edge.contains(p) && layer.is_none_or(|l| l.order == egui::Order::Background);
        on_card || on_edge
    });
    let dragging = ctx.dragged_id().is_some();
    let busy = app.viewport.context_menu.is_some() || egui::Popup::is_any_open(ctx) || dragging || ctx.egui_wants_keyboard_input();
    let shown = (over && (was || !dragging)) || (was && busy);
    ctx.data_mut(|d| d.insert_temp(memory, shown));
    ctx.animate_value_with_time(memory.with("opacity"), f32::from(u8::from(shown)), crate::ui_kit::NAV_FADE_SECONDS)
}

/// The thin tab at the hidden browser's left edge that says where it is.
fn reveal_tab(ui: &egui::Ui, card: Rect, opacity: f32) {
    let t = Tokens::get();
    let x = card.left() + t.space_3;
    let tab = Rect::from_min_max(pos2(x, card.top() + t.space_8), pos2(x + t.space_3, card.top() + t.space_8 + t.space_24 * 2.0));
    ui.painter().rect_filled(tab, t.space_3 / 2.0, t.text_dim.gamma_multiply(opacity));
}

/// One content-hugging browser; the shared container supplies floating or flush compact chrome.
pub fn browser(app: &mut SolveApp, ui: &mut egui::Ui) {
    let available = ui.available_rect_before_wrap();
    let t = Tokens::get();
    std::mem::swap(&mut app.tree.order_prev, &mut app.tree.order);
    app.tree.order_len = 0;
    DRAWN.with(|d| d.borrow_mut().clear());
    OCCURRENCES.with(|d| d.borrow_mut().clear());
    let mut acts = Actions { hover_bodies: std::mem::take(&mut app.viewport.hover_bodies), ..Default::default() };
    if crate::render_workspace::active(app) {
        render_card(app, ui, &mut acts);
        finish_browser(app, ui, acts);
        return;
    }
    let stacked = crate::timeline_left::stacked(app);
    let container =
        Container::new(ui.ctx(), Surface::Panel).joined(crate::ui_kit::DockEdges { left: true, top: true, bottom: stacked, ..Default::default() });
    let gap = container.gap();
    let width = browser_width(app.preferences.browser_width);
    let vertical = app.ui.show_timeline && app.preferences.timeline_left;
    let mut available = available;
    if !stacked {
        available.min.x += crate::agent_chat::left_inset(app, available);
    }
    if vertical
        && !stacked
        && let Some(history) = app.shell.history
    {
        available.min.x = (history.right() + gap).min(available.right()) - gap;
    }
    let reveals = reveals(app);
    let collapsed = vertical && !stacked && !reveals && app.preferences.timeline_left_browser_collapsed;
    let bounds = if stacked {
        crate::timeline_left::stacked_rects(app, available).0
    } else {
        panel_bounds(available, if vertical { None } else { app.shell.history }, if collapsed { collapsed_width() } else { width }, gap)
    };
    let opacity = if reveals {
        let edge = Rect::from_min_max(pos2(available.left(), bounds.top()), pos2(bounds.left() + t.space_10, bounds.bottom()));
        crate::scenario::publish_target("browser:edge", edge);
        reveal_opacity(app, ui.ctx(), app.shell.browser, edge)
    } else {
        1.0
    };
    app.shell.browser_hidden = reveals && opacity < 0.5;
    app.shell.browser = None;
    let min_height = HEADER_H + 2.0 * (CARD_INSET + 1.0);
    // A dialog in the left column holds the browser's place (`dialog_column`).
    if !crate::dialog_column::holds(app) && bounds.is_positive() && bounds.height() >= min_height {
        let fill = if container.floating() && app.preferences.browser_float { t.panel.gamma_multiply(BROWSER_FLOAT_ALPHA) } else { t.panel };
        let frame = container.frame(ui.ctx(), fill, crate::frame::Side::Left).inner_margin(CARD_INSET as i8);
        let inner_width = (bounds.width() - frame.total_margin().sum().x).max(0.0);
        let inner_height = (bounds.height() - frame.total_margin().sum().y).max(0.0);
        let card = Container::new(ui.ctx(), Surface::Panel).area(egui::Id::new("sc_browser_card")).fixed_pos(bounds.min).show(ui.ctx(), |ui| {
            let limits = Rect::from_min_max(available.min, pos2(available.right(), bounds.bottom()));
            let response = ui
                .scope(|ui| {
                    ui.set_opacity(opacity);
                    container.show_frame(ui, frame, limits, |ui| {
                        ui.set_width(inner_width);
                        ui.set_max_height(inner_height);
                        if stacked {
                            ui.set_min_height(inner_height);
                            ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, vec2(inner_width, inner_height))));
                        }
                        if collapsed {
                            browser_collapsed_rail(app, ui);
                        } else {
                            browser_contents(app, ui, &mut acts, true);
                        }
                    })
                })
                .inner;
            if reveals && opacity < 1.0 {
                reveal_tab(ui, response.response.rect, 1.0 - opacity);
            }
            if !collapsed {
                resize_column(app, ui, response.response.rect, width, "left-browser-rail");
            }
        });
        let rect = card.response.rect;
        app.shell.browser = Some(rect);
        crate::scenario::publish_rect("browser:card", rect);
        // Preserve the overlay handle used by main's automation.
        if app.preferences.browser_float {
            crate::scenario::publish_rect("browser:float", rect);
        }
    }
    finish_browser(app, ui, acts);
}

fn render_card(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions) {
    let bounds = crate::shell::panel_bounds(app, ui.available_rect_before_wrap());
    let narrow = bounds.width() < 800.0;
    crate::shell::register_panel(app, "render:browser:show", None, None);
    if narrow && !app.shell.render_browser_open {
        app.shell.browser = None;
        crate::shell::register_panel(app, "render:browser", None, None);
        crate::shell::register_panel(app, "render:appearances", None, None);
        crate::render_ui::dock_chip(app, ui, "render:browser:show", "ui.browser_title", "view.render_browser", json!({"open":true}));
        return;
    }
    let mut side = app.preferences.render_browser_side.clone();
    let mut width = app.preferences.render_browser_width;
    let title = app.tr("render.page_scene").to_string();
    let labels = [app.tr("render.tree_by_object").to_string(), app.tr("render.tree_by_look").to_string()];
    let search_label = app.tr("render.tree_search").to_string();
    let mut lens = usize::from(app.render_workspace.view.by_look);
    let mut search_clicked = false;
    let content = crate::ui_kit::Panel::workspace_with_header(
        ui,
        crate::ui_kit::WorkspaceOptions {
            id: "render:browser",
            title: &title,
            icon: "list",
            bounds: crate::ui_kit::Panel::stack_first_bounds(bounds),
            fill_height: true,
            compact: crate::render_ui::compact(app, bounds.width()),
            editor: false,
            enabled: true,
            header_actions: None,
        },
        &mut side,
        &mut width,
        true,
        |ui, options| {
            search_clicked =
                crate::ui_kit::tree_header(ui, options.title, &mut lens, &labels.iter().map(String::as_str).collect::<Vec<_>>(), &search_label);
        },
        |ui| {
            if narrow {
                let hide = crate::ui_kit::button(ui, app.tr("render.fold_browser"), crate::ui_kit::Button::Normal);
                crate::scenario::publish_target("render:browser:hide", hide.rect);
                if hide.clicked() {
                    let _ = app.run("view.render_browser", json!({"open":false}));
                }
            }
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("sc_compact_tree"), true));
            browser_contents(app, ui, acts, true);
            ui.ctx().data_mut(|d| d.remove::<bool>(egui::Id::new("sc_compact_tree")));
        },
    );
    if (lens == 1) != app.render_workspace.view.by_look {
        let _ = app.run("view.render_browser", json!({"by_look":lens == 1}));
    }
    if search_clicked {
        let _ = app.run("view.render_browser", json!({"search":!app.render_workspace.view.search_objects}));
    }
    app.preferences.render_browser_width = width;
    let requested =
        content.as_ref().and_then(|c| c.inner.1).map(str::to_string).or_else(|| (app.preferences.render_browser_side != side).then(|| side.clone()));
    if let Some(side) = requested {
        let _ = app.run("view.render_dock", serde_json::json!({"panel":"browser","side":side}));
    }
    let rect = content.map(|c| c.response.rect);
    app.shell.browser = rect;
    crate::shell::register_panel(
        app,
        "render:browser",
        rect,
        if narrow { None } else { Some(if side == "left" { crate::ui_kit::Edge::Left } else { crate::ui_kit::Edge::Right }) },
    );
    if let Some(rect) = rect {
        crate::scenario::publish_rect("browser:card", rect);
        crate::render_workspace::appearances_card(app, ui, crate::ui_kit::Panel::stack_next_bounds(bounds, rect));
    } else {
        crate::shell::register_panel(app, "render:appearances", None, None);
    }
}

fn browser_collapsed_rail(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let title_top = t.space_24 + ui.spacing().item_spacing.y;
    let height = title_top + t.space_24 * 4.0 + t.space_8;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    // Include the card margin in the hit target; keep the tint inside that margin.
    let response =
        ui.interact(rect.expand(CARD_INSET + 1.0), ui.id().with("browser_header"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() || response.has_focus() {
        crate::ui_kit::panel_header_hover(ui.painter(), rect);
    }
    let disclosure = Rect::from_min_size(rect.min, vec2(rect.width(), t.space_24));
    crate::ui_kit::disclosure(ui.painter(), disclosure.center(), false, t.text_dim);
    crate::scenario::publish_handle("left-browser-toggle", disclosure.center());
    crate::scenario::publish_target("browser:header", disclosure);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), crate::i18n::tr("ui.browser_title")));
    if response.on_hover_text(crate::i18n::tr("ui.browser_expand")).clicked() {
        app.preferences.timeline_left_browser_collapsed = false;
    }
    let galley = ui.painter().layout_no_wrap(crate::i18n::tr("ui.browser_title").to_owned(), crate::fonts::semibold(t.font_body), t.text);
    let title = Rect::from_min_max(rect.min + vec2(0.0, title_top), rect.max);
    ui.painter().add(
        egui::epaint::TextShape::new(title.center() - galley.size() / 2.0, galley, t.text)
            .with_angle_and_anchor(std::f32::consts::FRAC_PI_2, egui::Align2::CENTER_CENTER),
    );
}

pub(crate) fn panel_bounds(available: Rect, history: Option<Rect>, width: f32, gap: f32) -> Rect {
    let min = available.min + vec2(gap, 0.0);
    let right = (available.right() - gap).max(min.x);
    let bottom = history.map_or(available.bottom() - gap, |r| r.top() - gap).max(min.y);
    Rect::from_min_max(min, pos2((min.x + width).min(right), bottom))
}

fn browser_contents(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions, collapsible: bool) {
    let t = Tokens::get();
    // Only the floating card folds; the docked column keeps its full height.
    let render = crate::render_workspace::active(app);
    if !render {
        browser_header(app, ui, collapsible);
    }
    let stacked = !render && crate::timeline_left::stacked(app);
    if !render
        && collapsible
        && (if stacked {
            (app.preferences.timeline_left_browser_collapsed && app.shell.history_folding.is_none_or(|p| p[0] >= 1.0))
                || ui.available_height() <= t.space_3
        } else {
            app.preferences.browser_collapsed && !reveals(app) && !(app.ui.show_timeline && app.preferences.timeline_left)
        })
    {
        return;
    }
    if crate::render_workspace::browser_mode(app, ui) {
        return;
    }
    if !render {
        t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top() + 1.0, t.toolbar.lerp_to_gamma(t.border, 0.5));
        ui.add_space(t.space_3);
    }
    crate::edit_in_place::browser_strip(app, ui);
    if collapsible && !stacked {
        browser_rows_hugging(app, ui, acts);
    } else {
        browser_rows(app, ui, acts);
    }
}

/// The header: the title, the body count and (on the floating card) a chevron that folds the
/// card to just this row. The whole row is the button. A browser that reveals on hover never
/// folds: the whole card is what the pointer finds.
fn browser_header(app: &mut SolveApp, ui: &mut egui::Ui, collapsible: bool) {
    let collapsible = collapsible && !reveals(app);
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let response = ui.interact(rect, ui.id().with("browser_header"), if collapsible { Sense::click() } else { Sense::hover() });
    crate::scenario::publish_target("browser:header", rect);
    let vertical = app.ui.show_timeline && app.preferences.timeline_left;
    let stacked = crate::timeline_left::stacked(app);
    let collapsed =
        collapsible && if stacked { app.preferences.timeline_left_browser_collapsed } else { !vertical && app.preferences.browser_collapsed };
    if vertical {
        crate::scenario::publish_handle("left-browser-toggle", rect.center());
    }
    if collapsible && (response.hovered() || response.has_focus()) {
        crate::ui_kit::panel_header_hover(ui.painter(), rect);
    }
    let title = crate::i18n::tr("ui.browser_title").to_string();
    let kind = if collapsible { egui::WidgetType::Button } else { egui::WidgetType::Label };
    response.widget_info(|| egui::WidgetInfo::labeled(kind, true, &title));
    let galley = ui.painter().layout_no_wrap(title, crate::fonts::semibold(t.font_heading), t.text);
    ui.painter().galley(pos2(rect.left() + t.space_6, rect.center().y - galley.size().y / 2.0), galley, t.text);
    let mut right = rect.right() - t.space_6;
    if collapsible {
        crate::ui_kit::disclosure(ui.painter(), pos2(right - t.form_section_icon_size / 2.0, rect.center().y), !collapsed, t.text_dim);
        right -= 18.0;
    }
    let bodies = app.session.model.state().bodies.len();
    let key = if bodies == 1 { "ui.browser_bodies_one" } else { "ui.browser_bodies_other" };
    let count = crate::i18n::format(key, &[("count", &bodies.to_string())]);
    let galley = ui.painter().layout_no_wrap(count, FontId::proportional(t.font_body), t.text_dim);
    ui.painter().galley(pos2(right - galley.size().x, rect.center().y - galley.size().y / 2.0), galley, t.text_dim);
    if collapsible {
        let tip = crate::i18n::tr(if collapsed { "ui.browser_expand" } else { "ui.browser_collapse" });
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() {
            if stacked {
                app.preferences.timeline_left_browser_collapsed = !app.preferences.timeline_left_browser_collapsed;
            } else if vertical {
                app.preferences.timeline_left_browser_collapsed = true;
            } else {
                app.preferences.browser_collapsed = !app.preferences.browser_collapsed;
            }
        }
    }
}

fn prepare_model(app: &mut SolveApp) {
    if app.tree.model.as_ref().is_none_or(|model| !model.matches(&app.session)) {
        app.tree.model = Some(model::Model::new(&app.session));
    }
}

fn model_placement(app: &mut SolveApp, path: &[u64]) -> Option<std::sync::Arc<solidcraft_engine::occurrences::OccurrencePlacement>> {
    prepare_model(app);
    app.tree.model.as_mut()?.placement(&app.session, path)
}

#[cfg(test)]
fn model_children(app: &SolveApp, component: u64) -> Vec<u64> {
    match app.tree.model.as_ref().filter(|model| model.matches(&app.session)) {
        Some(model) => model.children.get(&component).cloned().unwrap_or_default(),
        None => app.session.doc.occurrences.iter().filter(|o| o.parent == component).map(|o| o.id).collect(),
    }
}

fn model_body_indices(app: &SolveApp, component: u64) -> Vec<usize> {
    match app.tree.model.as_ref().filter(|model| model.matches(&app.session)) {
        Some(model) => model.bodies.get(&component).cloned().unwrap_or_default(),
        None => app
            .session
            .model
            .state()
            .bodies
            .iter()
            .enumerate()
            .filter(|(_, b)| app.session.doc.model_body_component(b) == component)
            .map(|(i, _)| i)
            .collect(),
    }
}

fn model_feature_indices(app: &SolveApp, component: u64, sketches: bool) -> Vec<usize> {
    match app.tree.model.as_ref().filter(|model| model.matches(&app.session)) {
        Some(model) => (if sketches { &model.sketches } else { &model.planes }).get(&component).cloned().unwrap_or_default(),
        None => app
            .session
            .doc
            .features
            .iter()
            .take(app.session.doc.marker.unwrap_or(app.session.doc.features.len()))
            .enumerate()
            .filter_map(|(i, f)| {
                let kind = if sketches {
                    matches!(f.kind, FeatureKind::Sketch { .. })
                } else {
                    matches!(
                        f.kind,
                        FeatureKind::ConstructionPlane { .. } | FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. }
                    )
                };
                (f.component == component && kind).then_some(i)
            })
            .collect(),
    }
}

fn browser_rows(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions) {
    rows_scrolling(app, ui, acts, false);
}

/// As [browser_rows], but the area hugs the rows it drew (the floating card), up to the height
/// the caller allowed.
fn browser_rows_hugging(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions) {
    rows_scrolling(app, ui, acts, true);
}

fn rows_scrolling(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions, shrink_height: bool) {
    prepare_model(app);
    acts.lit = Lit::of(app);
    let mut cache = app.tree.cache.take().unwrap_or_default();
    cache.prepare(app);
    egui::ScrollArea::vertical()
        // The card may animate all the way down to its header.
        .min_scrolled_height(0.0)
        .auto_shrink([false, shrink_height])
        .scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, ..egui::scroll_area::ScrollSource::NONE })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if crate::render_workspace::active(app) {
                crate::render_workspace::scene::sections(app, ui);
                if !crate::render_workspace::scene::objects(app, ui, &mut acts.hover_bodies, &mut acts.hovered) {
                    component_rows(app, ui, 0, 1, acts, &mut cache);
                }
                crate::render_workspace::scene::output(app, ui);
            } else {
                component_rows(app, ui, 0, 0, acts, &mut cache);
            }
        });
    // The row was reached and scrolled to, or isn't in the tree (a body a later feature removes):
    // either way the reveal is done.
    app.tree.reveal = None;
    app.tree.cache = Some(cache);
}

/// The Bodies folder and the Origin at the top of the vertical History (Preferences › Interface:
/// Show origin and bodies at the top, #6490): the browser's own rows, with their eyes, selection,
/// menus and fold state. The bodies are the active component's, in the placement being edited,
/// as that component's folder in the browser lists them. The rows stay out of the browser's row
/// order, automation list and reveal, which the browser keeps for its own rows.
pub(crate) fn history_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    prepare_model(app);
    let mut acts = Actions { lit: Lit::of(app), ..Default::default() };
    let reveal = app.tree.reveal.take();
    let order = app.tree.order_len;
    let drawn = DRAWN.with(|d| d.borrow().len());
    let component = app.session.active_component;
    let path = (component != 0)
        .then(|| app.session.active_occurrence_path.clone().unwrap_or_else(|| app.session.doc.first_path(component)))
        .filter(|path| !path.is_empty());
    // The Bodies folder as the browser draws it for that component or placement (`browser_cache`).
    let mut cache = app.tree.cache.take().unwrap_or_default();
    cache.prepare(app);
    let bodies = cache.folder(app, component, path.as_deref(), "bodies");
    app.tree.cache = Some(cache);
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        folder_rows_at(app, ui, component, path.as_deref(), "bodies", 0, &mut acts, &bodies);
        origin_rows(app, ui, 0, &mut acts);
    });
    app.tree.reveal = reveal;
    // The browser's own rows reuse these order slots (`TreeState::remember`).
    app.tree.order_len = order;
    DRAWN.with(|d| d.borrow_mut().truncate(drawn));
    app.viewport.hover_bodies = acts.hover_bodies.clone();
    app.tree.history_hover = acts.hover_bodies;
    app.viewport.hover_plane = acts.hover_plane.clone();
    app.tree.history_plane = acts.hover_plane;
    if let Some(d) = acts.drop {
        apply_drop(app, d);
    }
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
}

fn finish_browser(app: &mut SolveApp, ui: &mut egui::Ui, mut acts: Actions) {
    app.tree.order.truncate(app.tree.order_len);
    if !acts.hovered {
        acts.hover_bodies.clear();
    }
    // The history's rows, drawn before the browser, light bodies too (`history_rows`).
    let history = std::mem::take(&mut app.tree.history_hover);
    app.viewport.hover_bodies = if acts.hover_bodies.is_empty() { history } else { acts.hover_bodies };
    let history_plane = app.tree.history_plane.take();
    app.viewport.hover_plane = acts.hover_plane.or(history_plane);
    if let Some(d) = acts.drop {
        apply_drop(app, d);
    }
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
    crate::browser_prompts::occurrence_panel(app, ui);
    crate::browser_prompts::capture_prompt(app, ui.ctx());
    crate::browser_canvas::panel(app, ui.ctx());
    point_clouds::panel(app, ui.ctx());
    crate::sketch_attachment_ui::panel(app, ui.ctx());
}

fn component_name(app: &SolveApp, id: u64) -> String {
    if id == 0 {
        return app.session.doc.name.clone();
    }
    let doc = &app.session.doc;
    let name = doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    let n = doc.occurrences.iter().filter(|o| o.component == id).count();
    let name = if n > 1 { format!("{name} ({n} instances)") } else { format!("{name}:1") };
    crate::dialogs_part_size::browser_label(app, id, &name)
}

fn component_rows(app: &mut SolveApp, ui: &mut egui::Ui, id: u64, depth: usize, acts: &mut Actions, cache: &mut cache::Cache) {
    let mut root = std::mem::take(&mut cache.root);
    component_rows_inner(app, ui, id, depth, acts, cache, &mut root);
    cache.root = root;
}

fn component_rows_inner(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    id: u64,
    depth: usize,
    acts: &mut Actions,
    cache: &mut cache::Cache,
    root: &mut cache::Node,
) {
    if id == 0 && crate::render_workspace::active(app) {
        let Some(slot) = root.folders.first_mut() else { return };
        let data = slot.get_or_insert_with(|| cache.folder(app, id, None, "bodies"));
        folder_rows_at(app, ui, id, None, "bodies", depth.saturating_sub(1), acts, data);
        for path in &root.children {
            occurrence_rows(app, ui, path, depth, acts, cache);
        }
        return;
    }
    let key = &root.key;
    let reveal = app.tree.reveal.clone();
    if let Some(body) = reveal.as_ref() {
        let reveals =
            cache.root_bodies.contains(body) || reveal_path(app).is_some_and(|rp| root.children.iter().any(|path| rp.starts_with(path.as_slice())));
        if reveals {
            app.tree.collapsed.remove(key);
        }
    }
    let open = is_open(app, key, true);
    let active = app.session.active_component == id;
    let bodies = &cache.root_bodies;
    let visible = id == 0 || bodies.is_empty() || bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b));
    let faded = app.session.active_component != 0 && !app.session.doc.component_within(id, app.session.active_component) && !active;
    let name = &root.name;
    let badges: &[&str] = if root.grounded { &["pin"] } else { &[] };
    let appearance = crate::render_workspace::scene::decoration(app, bodies);
    let row = Row {
        depth,
        fold: Some(open),
        eye: (id != 0).then_some(visible),
        icon: "component",
        label: name,
        name: true,
        selected: app.tree.picked_components.contains(&id),
        dim: faded,
        radio: Some(active),
        badges,
        swatch: appearance.swatch,
        secondary: &appearance.name,
        count: &appearance.count,
        // The design holds everything the tree lists.
        mark: if id == 0 && acts.lit.any() { Mark::Branch } else { Mark::None },
        ..Default::default()
    };
    let r = draw_row(ui, ui.id().with(("comp", id)), &row);
    if r.fold {
        toggle(app, key, true);
    }
    if r.radio {
        let _ = app.run("component.activate", json!({ "component": id }));
    }
    if r.eye {
        if visible {
            app.session
                .visibility
                .hidden_bodies
                .extend(bodies.iter().filter(|b| !app.session.visibility.hidden_bodies.contains(b)).cloned().collect::<Vec<_>>());
        } else {
            app.session.visibility.hidden_bodies.retain(|b| !bodies.contains(b));
        }
    }
    if r.clicked && !r.double {
        let names = bodies_of_component(app, id);
        if !offer_bodies(app, &names, Take::Toggle) {
            let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            if !ctrl {
                app.tree.picked_components.clear();
                let _ = app.run("select.clear", json!({}));
            }
            if app.tree.picked_components.contains(&id) {
                app.tree.picked_components.retain(|c| *c != id);
            } else {
                app.tree.picked_components.push(id);
            }
        }
    }
    if r.double {
        if app.session.doc.parts.contains_key(&id) {
            crate::dialogs_part_size::open(app, id);
        } else {
            crate::edit_in_place::double_click_component(app, json!({ "component": id }));
        }
    }
    if r.hovered {
        acts.hover(bodies.iter());
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::Component { id }));
    }
    // Bodies and sketches dropped on a component move into it.
    if let Some(resp) = &r.resp
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp != id && d.folder != "construction" && d.groups.is_empty(), DropMark::Box)
    {
        acts.drop = Some(DropAction::Component { to: id, folder: d.folder, keys: d.keys.clone() });
    }

    // A placement row dropped on the design row moves out to the design.
    if let Some(resp) = &r.resp
        && let Some(d) = occurrence_drop_target(ui, resp, |d| occurrence_can_land(app, d, id))
    {
        let occurrence = d.path.last().copied().unwrap_or(0);
        acts.drop = Some(DropAction::Reparent { occurrence, path: d.path.clone(), parent: id, parent_path: None });
    }
    if !open {
        return;
    }
    let d = depth + 1;
    if id == 0 && !crate::render_workspace::active(app) {
        doc_settings(app, ui, d);
        named_view_rows(app, ui, d, acts);
        origin_rows(app, ui, d, acts);
        canvas_rows(app, ui, d, acts);
        point_clouds::rows(app, ui, d, acts);
        analysis_rows(app, ui, d, acts);
    }
    let render = crate::render_workspace::active(app);
    for (slot, folder) in root.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        if render && folder != "bodies" {
            continue;
        }
        let data = slot.get_or_insert_with(|| cache.folder(app, id, None, folder));
        folder_rows_at(app, ui, id, None, folder, d, acts, data);
    }
    if !render {
        features::rows(app, ui, id, &[], d, acts);
    }
    if id == 0 && !render {
        crate::dialogs_assembly::browser_joints(app, ui, d);
    }
    for path in &root.children {
        occurrence_rows(app, ui, path, d, acts, cache);
    }
}

fn occurrence_key(path: &[u64]) -> String {
    use std::fmt::Write;
    let mut key = String::from("o:");
    for (i, id) in path.iter().enumerate() {
        if i != 0 {
            key.push('/');
        }
        let _ = write!(key, "{id}");
    }
    key
}

/// Show a placement's row closed: a pasted (or moved) component arrives folded, whatever a row
/// at that path showed before.
pub(crate) fn close_placement(app: &mut SolveApp, path: &[u64]) {
    app.tree.collapsed.insert(occurrence_key(path));
}

/// A placement, rather than a component definition: repeated child IDs remain distinct
/// because their complete paths are the selection and row identity.
fn occurrence_rows(app: &mut SolveApp, ui: &mut egui::Ui, path: &[u64], depth: usize, acts: &mut Actions, cache: &mut cache::Cache) {
    // Own this cached row while drawing: children can populate the cache without copying it.
    let Some((path, mut placement)) = cache.take_occurrence(app, path) else { return };
    occurrence_rows_inner(app, ui, &path, depth, acts, cache, &mut placement);
    cache.occurrences.insert(path, placement);
}

fn occurrence_rows_inner(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    path: &[u64],
    depth: usize,
    acts: &mut Actions,
    cache: &mut cache::Cache,
    placement: &mut cache::Node,
) {
    let key = &placement.key;
    if reveal_path(app).is_some_and(|rp| rp.starts_with(path)) {
        app.tree.collapsed.remove(key);
    }
    let open = is_open(app, key, true);
    let on_screen = ui.is_rect_visible(row_rect(ui));
    let active = app.session.active_occurrence_path.as_deref() == Some(path)
        || (app.session.active_occurrence_path.is_none() && app.session.active_component == placement.component && placement.first);
    let visible = !on_screen || app.session.occurrence_visible(path);
    let own_visible = !on_screen || !app.session.occurrence_hidden(path);
    let selected = on_screen && app.session.selection.iter().any(|s| matches!(s, Sel::Occurrence { path: selected } if selected == path));
    let faded = app.session.active_occurrence_path.as_ref().is_some_and(|p| !path.starts_with(p) && !p.starts_with(path));
    // A cut component has a dashed outline until Paste moves it.
    let cut = app.cut.placements.iter().any(|p| p.as_slice() == path);
    let badges: &[&str] = if placement.grounded { &["pin"] } else { &[] };
    app.tree.remember(key);
    let appearance = if crate::render_workspace::active(app) {
        let bodies = placement.hover_bodies.get_or_insert_with(|| app.session.occurrence_bodies(path, true).unwrap_or_default());
        crate::render_workspace::scene::decoration(app, bodies)
    } else {
        crate::render_workspace::scene::Decoration::default()
    };
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row {
            depth,
            fold: Some(open),
            eye: Some(own_visible),
            icon: "component",
            label: &placement.label,
            swatch: appearance.swatch,
            secondary: &appearance.name,
            count: &appearance.count,
            name: true,
            selected,
            dim: faded || !visible,
            cut,
            radio: Some(active),
            badges,
            mark: acts.lit.placement(&app.session.doc, path, placement.component),
            ..Default::default()
        },
    );
    if let Some(rect) = r.rect.filter(|_| r.resp.is_some()) {
        OCCURRENCES.with(|d| d.borrow_mut().push((path.to_vec(), placement.name.clone(), rect, open)));
        // Near the row's end, clear of an eye there.
        let end = r.eye_at.filter(|e| e.x > rect.center().x).map_or(rect.right() - 12.0, |e| e.x - 24.0);
        crate::scenario::publish_handle(&format!("row:occurrence:{}", key.trim_start_matches("o:")), pos2(end, rect.center().y));
        for (control, at) in [("fold", r.fold_at), ("eye", r.eye_at), ("activate", r.radio_at)] {
            if let Some(at) = at {
                crate::scenario::publish_handle(&format!("{control}:occurrence:{}", key.trim_start_matches("o:")), at);
            }
        }
    }
    if let Some(resp) = &r.resp {
        let inherited = if own_visible && !visible { "\nHidden by a parent placement. Show the parent to reveal this instance." } else { "" };
        resp.clone().on_hover_ui(|ui| {
            ui.label(&placement.tooltip);
            if !inherited.is_empty() {
                ui.label(inherited.trim_start());
            }
        });
        // Dragging the row moves the placement it stands for. A press on its fold arrow, eye or
        // radio is not a drag of the row, so those controls keep their own jobs.
        if resp.drag_started() && !r.on_control {
            resp.dnd_set_drag_payload(OccurrenceDrag { path: path.to_vec(), component: placement.component });
        }
    }
    if r.fold {
        toggle(app, key, true);
    }
    if r.eye {
        let _ = app.run("browser.visibility", json!({"visible": app.session.occurrence_hidden(path), "paths": [path]}));
    }
    let takes_components = app.dialog.as_ref().is_some_and(|d| d.takes_components());
    let params = json!({"component": placement.component, "occurrence": placement.occurrence, "path": path});
    if r.radio {
        let _ = app.run("component.activate", params);
    } else if r.double && !takes_components && !app.session.doc.parts.contains_key(&placement.component) {
        crate::edit_in_place::double_click_component(app, params);
    }
    if r.double && !takes_components && app.session.doc.parts.contains_key(&placement.component) {
        crate::dialogs_part_size::open(app, placement.component);
    }
    if r.clicked && takes_components {
        if let Some(mut d) = app.dialog.take() {
            d.pick(&app.session, Sel::Occurrence { path: path.to_vec() });
            app.dialog = Some(d);
        }
    } else if r.clicked && !r.double {
        let names = app.session.occurrence_bodies(path, true).unwrap_or_default();
        if !offer_bodies(app, &names, Take::Toggle) {
            click_select(app, ui, key.clone(), Some(Sel::Occurrence { path: path.to_vec() }));
        }
    }
    if r.hovered {
        let bodies = placement.hover_bodies.get_or_insert_with(|| app.session.occurrence_bodies(path, true).unwrap_or_default());
        acts.hover(bodies.iter());
    }
    if let Some(p) = r.secondary {
        if !selected {
            let _ = app.run("select.set", json!({"items": [Sel::Occurrence { path: path.to_vec() }]}));
        }
        acts.menu = Some((p, Target::Occurrence { path: path.to_vec() }));
    }
    if let Some(resp) = &r.resp
        && let Some(d) =
            drop_target(ui, resp, |d: &Drag| d.comp != placement.component && d.folder != "construction" && d.groups.is_empty(), DropMark::Box)
    {
        acts.drop = Some(DropAction::Component { to: placement.component, folder: d.folder, keys: d.keys.clone() });
    }

    // The top of a sibling's row puts the dragged placement before it; the rest of the row, and
    // any other row, moves the placement into this row's component.
    let top = r.resp.as_ref().is_some_and(|resp| pointer_in_top(ui, resp.rect));
    if let Some(resp) = &r.resp
        && top
        && let Some(d) = occurrence_reorder_target(ui, resp, |d| occurrence_sibling(app, d, placement.occurrence).is_some())
        && let Some(parent) = occurrence_sibling(app, &d, placement.occurrence)
    {
        let dragged = d.path.last().copied().unwrap_or(0);
        let mut order: Vec<u64> = app.session.doc.occurrences.iter().filter(|o| o.parent == parent && o.id != dragged).map(|o| o.id).collect();
        app.session.doc.sort_placements(parent, &mut order);
        let at = order.iter().position(|o| *o == placement.occurrence).unwrap_or(order.len());
        order.insert(at, dragged);
        let order: Vec<String> = order.iter().map(u64::to_string).collect();
        let _ = app.run("browser.order", json!({ "folder": "components", "component": parent, "order": order }));
    }
    // A placement row dropped on this one moves into this row's component.
    if let Some(resp) = &r.resp
        && let Some(d) = occurrence_drop_target(ui, resp, |d| {
            occurrence_can_land(app, d, placement.component) && !(top && occurrence_sibling(app, d, placement.occurrence).is_some())
        })
    {
        let occurrence = d.path.last().copied().unwrap_or(0);
        acts.drop = Some(DropAction::Reparent { occurrence, path: d.path.clone(), parent: placement.component, parent_path: Some(path.to_vec()) });
    }
    if !open {
        return;
    }
    let render = crate::render_workspace::active(app);
    for (slot, folder) in placement.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        if render && folder != "bodies" {
            continue;
        }
        let data = slot.get_or_insert_with(|| cache.folder(app, placement.component, Some(path), folder));
        folder_rows_at(app, ui, placement.component, Some(path), folder, depth + 1, acts, data);
    }
    if !render {
        features::rows(app, ui, placement.component, path, depth + 1, acts);
        // The joints that attach this placement, as the root's Joints folder lists them (#9130).
        crate::dialogs_assembly::component_joints(app, ui, path, key, depth + 1);
    }
    if path.len() < 64 {
        for child in &placement.children {
            occurrence_rows(app, ui, child, depth + 1, acts, cache);
        }
    }
}

/// The Document Settings group. A second click on an entry closes the window it opened, the way
/// that window's own × does (#4150).
fn doc_settings(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    let key = "docsettings";
    let open = is_open(app, key, false);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), icon: "settings", label: "Document Settings", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if !open {
        return;
    }
    let units = format!("Units: {}", app.session.doc.units);
    let r = draw_row(ui, ui.id().with("units"), &Row { depth: depth + 1, icon: "settings", label: &units, ..Default::default() });
    if let Some(p) = r.secondary {
        crate::context_menu::open_for(app, p, Target::Units);
    } else if r.clicked
        && let Some(rect) = r.rect
    {
        crate::context_menu::toggle_for(app, rect, rect.left_bottom(), Target::Units);
    }
    let prec = format!(
        "Precision: {} {}, {}°",
        crate::prefs::show_mm_bare(app, 0.0),
        crate::prefs::unit_scale(&app.session.doc.units).1,
        format_args!("{:.*}", usize::from(app.preferences.angle_decimals), 0.0)
    );
    if draw_row(ui, ui.id().with("precision"), &Row { depth: depth + 1, icon: "settings", label: &prec, ..Default::default() }).clicked {
        if app.prefs_window.open {
            crate::prefs::close(app, true);
        } else {
            app.prefs_window.open = true;
        }
    }
    let params = format!("Parameters ({})", app.session.doc.params.len());
    if draw_row(ui, ui.id().with("params"), &Row { depth: depth + 1, icon: "params", label: &params, ..Default::default() }).clicked {
        if app.params.is_some() {
            app.params = None;
        } else {
            app.start("parameters.change");
        }
    }
    if draw_row(ui, ui.id().with("design_properties"), &Row { depth: depth + 1, icon: "settings", label: "Design Properties", ..Default::default() })
        .clicked
    {
        if app.design_properties.is_some() {
            app.design_properties = None;
        } else {
            app.start("document.metadata");
        }
    }
}

/// The origin point, axes and planes, as `(key, icon, thing lit for it)`.
fn origin_things() -> [(&'static str, &'static str, Option<Thing>); 7] {
    let (axis, plane) = (|k: &str| Some(Thing::Axis(k.into())), |k: &str| Some(Thing::Plane(k.into())));
    [
        ("O", "point", None),
        ("X", "axis", axis("X")),
        ("Y", "axis", axis("Y")),
        ("Z", "axis", axis("Z")),
        ("XY", "plane", plane("XY")),
        ("XZ", "plane", plane("XZ")),
        ("YZ", "plane", plane("YZ")),
    ]
}

fn origin_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let key = "origin";
    let open = is_open(app, key, false);
    let lit = |thing: &Option<Thing>| thing.as_ref().map_or((false, false), |t| acts.lit.thing(t));
    let items = origin_things();
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row {
            depth,
            fold: Some(open),
            eye: Some(app.origin_shown()),
            icon: "origin",
            label: "Origin",
            dim: !app.origin_shown(),
            mark: if items.iter().any(|(_, _, thing)| lit(thing) != (false, false)) { Mark::Branch } else { Mark::None },
            ..Default::default()
        },
    );
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if r.eye {
        app.set_origin_shown(!app.origin_shown());
    }
    if let Some(p) = r.secondary {
        crate::context_menu::open_for(app, p, Target::Origin);
    }
    if !open {
        return;
    }
    for (k, icon, thing) in items {
        // An origin plane by the name the preferences give it; its key stays the id.
        let label = app.preferences.origin_plane_names.label(k);
        let visible = !app.session.visibility.hidden_origin.iter().any(|h| h == k);
        let sel = match icon {
            "plane" => Some(Sel::Plane { name: k.into() }),
            "axis" => Some(Sel::Axis { name: k.into() }),
            _ => Some(Sel::Vertex { body: String::new(), point: solidcraft_engine::geom::Vec3::ZERO }),
        };
        let (selected, mark) = lit_row(lit(&thing));
        let r = draw_row(
            ui,
            ui.id().with(("o", k)),
            // Faded when hidden itself or with the whole Origin. A renamed plane's label is already
            // in the interface's language (`name`).
            &Row {
                depth: depth + 1,
                eye: Some(visible),
                icon,
                label,
                name: label != k,
                selected,
                dim: !visible || !app.origin_shown(),
                mark,
                ..Default::default()
            },
        );
        if r.eye {
            // Through the command, so the Origin row's eye is an undo step (#2219).
            let _ = app.run("browser.visibility", json!({ "items": [k], "visible": !visible }));
        }
        if let Some(p) = r.secondary {
            let target = match icon {
                "plane" => Some(Target::OriginPlane { name: k.into() }),
                "axis" => Some(Target::OriginAxis { name: k.into() }),
                _ => None,
            };
            if let Some(target) = target {
                crate::context_menu::open_for(app, p, target);
            }
        }
        if r.hovered && icon == "plane" {
            acts.hover_plane = Some(k.into());
        }
        if r.clicked
            && let Some(x) = sel
        {
            pick_from_browser(app, x);
        }
    }
}

/// The standard views listed under Named Views: (label, view).
pub const STANDARD_VIEWS: [(&str, &str); 4] = [("Top", "top"), ("Front", "front"), ("Right", "right"), ("Home", "home")];

/// Named Views: the standard views, then the design's saved cameras. A double-click restores
/// one (animated).
fn named_view_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let key = "namedviews";
    let open = is_open(app, key, false);
    let r = draw_row(ui, ui.id().with(key), &Row { depth, fold: Some(open), icon: "folder", label: "Named Views", ..Default::default() });
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::NamedViews));
    }
    if !open {
        return;
    }
    for (label, view) in STANDARD_VIEWS {
        let r = draw_row(ui, ui.id().with(("stdview", view)), &Row { depth: depth + 1, icon: "home", label, ..Default::default() });
        if r.double {
            app.animate_view(view);
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::NamedView { name: label.to_string() }));
        }
    }
    let names: Vec<String> = app.session.doc.named_views.iter().map(|v| v.name.clone()).collect();
    for name in names {
        let r = draw_row(
            ui,
            ui.id().with(("view", &name)),
            &Row { depth: depth + 1, icon: "perspective", label: &name, name: true, ..Default::default() },
        );
        if r.double {
            restore_view(app, &name);
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::NamedView { name }));
        }
    }
}

/// Turn the camera to a named view (a saved one or a standard one), animated.
pub fn restore_view(app: &mut SolveApp, name: &str) {
    if let Some((_, v)) = STANDARD_VIEWS.iter().find(|(l, _)| *l == name) {
        app.animate_view(v);
        return;
    }
    if app.run("view.apply", json!({ "view": name })).is_ok() {
        app.follow_view_request(true);
    }
}

/// Save the current camera as a named view.
pub fn save_view(app: &mut SolveApp, name: Option<&str>) -> Option<String> {
    let cam = serde_json::to_value(app.cam).ok()?;
    let r = app.run("view.save", json!({ "name": name, "camera": cam })).ok()?;
    app.tree.collapsed.remove("namedviews");
    app.tree.expanded.insert("namedviews".into());
    r.get("view").and_then(Value::as_str).map(str::to_string)
}

/// The menu of a named view.
pub fn view_items(app: &SolveApp, name: &str) -> Vec<Item> {
    let saved = app.session.doc.named_views.iter().any(|v| v.name == name);
    vec![
        Item::action("ui.restoreView", "Restore", "home").with(json!({ "view": name })),
        Item::action("ui.updateView", "Update to Current View", "perspective").with(json!({ "view": name })).on_if(saved),
        Item::sep(),
        Item::action("ui.rename", "Rename", "rename").with(json!({ "view": name })).on_if(saved),
        Item::action("view.delete", "Delete", "delete").with(json!({ "view": name })).on_if(saved),
    ]
}

/// The Analysis folder: the Section Analysis, while there is one (shown or hidden), with an eye
/// to hide and show it, a double-click to edit it and a menu to delete it (#822).
fn analysis_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    if app.session.section.is_some() {
        // A section made (or remade) while an older one was hidden replaces it, shown.
        app.tree.hidden_section = None;
        app.tree.analysis_hidden = false;
        app.tree.section_hidden = false;
    }
    if app.session.section.is_none() && app.tree.hidden_section.is_none() {
        app.tree.analysis_hidden = false;
        app.tree.section_hidden = false;
        return;
    }
    // Each eye is its own: the folder's never follows the section's, nor the other way round.
    let (folder_shown, section_shown) = (!app.tree.analysis_hidden, !app.tree.section_hidden);
    let key = "analysis";
    let open = is_open(app, key, true);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(folder_shown), icon: "folder", label: "Analysis", dim: !folder_shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if r.eye {
        app.tree.analysis_hidden = folder_shown;
        sync_section(app);
    }
    if !open {
        return;
    }
    let r = draw_row(
        ui,
        ui.id().with("section_analysis"),
        &Row {
            depth: depth + 1,
            eye: Some(section_shown),
            icon: "section",
            label: "Section Analysis",
            dim: !section_shown || !folder_shown,
            ..Default::default()
        },
    );
    if r.eye {
        set_section_shown(app, !section_shown);
    }
    if r.double {
        crate::dialogs::edit_section(app);
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::Section));
    }
}

/// Show or hide the Section Analysis itself (its eye): a hidden Analysis folder still keeps it
/// off.
pub fn set_section_shown(app: &mut SolveApp, shown: bool) {
    app.tree.section_hidden = !shown;
    sync_section(app);
}

/// The section is on while both the Analysis folder and the section itself are shown.
fn sync_section(app: &mut SolveApp) {
    let on = !app.tree.analysis_hidden && !app.tree.section_hidden;
    set_section_visible(app, on);
}

/// Hide the Section Analysis (keeping its plane to show it again) or show the hidden one.
pub fn set_section_visible(app: &mut SolveApp, visible: bool) {
    if visible {
        if let Some((origin, normal)) = app.tree.hidden_section {
            let plane = json!({"origin": [origin.x, origin.y, origin.z], "normal": [normal.x, normal.y, normal.z]});
            if app.run("inspect.section", json!({"plane": plane, "hatch": app.session.section_hatch})).is_ok() {
                app.tree.hidden_section = None;
            }
        }
    } else if let Some(section) = app.session.section
        && app.run("inspect.section", json!({"clear": true})).is_ok()
    {
        app.tree.hidden_section = Some(section);
    }
}

/// The menu of the Section Analysis row.
pub fn section_items(app: &SolveApp) -> Vec<Item> {
    let visible = !app.tree.section_hidden;
    vec![
        Item::action("ui.editSection", "Edit Section Analysis", "section"),
        Item::sep(),
        Item::action("ui.unsection", "Delete", "delete"),
        Item::sep(),
        Item::action("ui.sectionVisible", "Show/Hide", "eye").key("V").with(json!({ "visible": !visible })),
    ]
}

/// The Canvases folder's tree key (`Visibility::hidden_folders`).
pub const CANVASES: &str = "c0/canvases";

/// The Canvases folder (reference images) with an eye per canvas.
fn canvas_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &mut Actions) {
    let canvases: Vec<(u64, String, bool)> = app.session.doc.canvases.iter().map(|c| (c.id, c.name.clone(), c.visible)).collect();
    if canvases.is_empty() {
        return;
    }
    let key = "canvases";
    let open = is_open(app, key, true);
    // The folder's eye is its own: each canvas keeps its eye while the folder hides them all.
    let shown = !app.session.folder_hidden(CANVASES);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row { depth, fold: Some(open), eye: Some(shown), icon: "folder", label: "Canvases", dim: !shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, true);
    }
    if r.eye {
        toggle_folder(app, CANVASES);
    }
    if !open {
        return;
    }
    for (id, name, visible) in canvases {
        let selected = app.tree.picked_canvases.contains(&id);
        let r = draw_row(
            ui,
            ui.id().with(("canvas", id)),
            &Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "canvas",
                label: &name,
                name: true,
                selected,
                dim: !visible || !shown,
                ..Default::default()
            },
        );
        if r.eye {
            let _ = app.run("canvas.edit", json!({ "canvas": id, "visible": !visible }));
        }
        if r.clicked {
            if !ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                app.tree.picked_canvases.clear();
                app.tree.picked_components.clear();
                app.tree.picked_joints.clear();
                let _ = app.run("select.clear", json!({}));
            }
            if selected {
                app.tree.picked_canvases.retain(|c| *c != id);
            } else {
                app.tree.picked_canvases.push(id);
            }
        }
        if r.double {
            app.tree.canvas_panel = Some(CanvasPanel::new(app, id, false));
        }
        if let Some(p) = r.secondary {
            if !selected {
                app.tree.picked_canvases = vec![id];
            }
            acts.menu = Some((p, Target::Canvas { id }));
        }
    }
}

/// The menu of a point cloud.
pub fn point_cloud_items(app: &SolveApp, id: u64) -> Vec<Item> {
    point_clouds::items(app, id)
}

/// The menu of a canvas.
pub fn canvas_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = app.session.doc.canvases.iter().find(|c| c.id == id).is_some_and(|c| c.visible);
    vec![
        Item::action("ui.editCanvas", "Edit Canvas", "canvas").with(json!({ "canvas": id })),
        Item::action("ui.calibrateCanvas", "Calibrate", "measure").with(json!({ "canvas": id })),
        Item::action("canvas.to_sketch", "Canvas to Sketch", "canvas").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.delete", "Delete", "delete").with(json!({ "canvases": [id] })),
        Item::action("ui.rename", "Rename", "rename").key("F2").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.canvasVisible", "Show/Hide", "eye").key("V").with(json!({ "canvas": id, "visible": !visible })),
    ]
}

/// Editing a canvas (position, size, angle, opacity, flip) or calibrating it (two points on
/// the image and their true distance). Live edits of one opening are one undo step.
#[derive(Clone, Debug)]
pub(crate) struct CanvasEditDrag {
    /// 0 move, 1 scale, 2 rotate, 3 opacity.
    pub(crate) kind: u8,
    pub(crate) pointer: [f64; 2],
    pub(crate) center: [f64; 2],
    pub(crate) width: f64,
    pub(crate) angle: f64,
}

#[derive(Clone, Debug)]
pub struct CanvasPanel {
    pub canvas: u64,
    pub calibrate: bool,
    /// Undo depth when the panel opened.
    pub(crate) depth: usize,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub distance: f64,
    /// Next viewport click sets Point A (`0`) or Point B (`1`) (#9108).
    pub next: u8,
    /// Calibration marker currently dragged with the primary pointer.
    pub(crate) dragging: Option<u8>,
    /// Direct manipulation active while Edit Canvas is open.
    pub(crate) edit_drag: Option<CanvasEditDrag>,
}

impl CanvasPanel {
    pub fn new(app: &SolveApp, canvas: u64, calibrate: bool) -> Self {
        // Calibration starts from the image's left and right edge midpoints.
        let (a, b, d) = app
            .session
            .doc
            .canvases
            .iter()
            .find(|c| c.id == canvas)
            .map(|c| ([c.center.x - c.width / 2.0, c.center.y], [c.center.x + c.width / 2.0, c.center.y], c.width))
            .unwrap_or(([0.0, 0.0], [100.0, 0.0], 100.0));
        CanvasPanel { canvas, calibrate, depth: app.session.undo.len(), a, b, distance: d, next: 0, dragging: None, edit_drag: None }
    }
}

/// Does the open dialog's active input take bodies (Combine's target and tools, #4452;
/// Move/Copy's bodies, #4771)? Then a body clicked in the Browser goes there, as a click
/// on it in the view does, rather than replacing the selection: overlapping bodies are
/// often only told apart in the Browser.
fn dialog_takes_body(app: &SolveApp) -> bool {
    app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::BODIES != 0)
}

/// A plain click on a sketch (a Browser row or a History item) while Extrude, Revolve or Emboss
/// is open gives the dialog every profile of that sketch (#6106). False when it is not one: the
/// click then selects as usual.
pub(crate) fn pick_sketch_into_dialog(app: &mut SolveApp, id: u64) -> bool {
    let Some(mut d) = app.dialog.take() else { return false };
    let taken = d.pick_sketch(&app.session, id);
    app.dialog = Some(d);
    taken
}

/// Browser picks go only into compatible active dialog inputs. Outside a dialog, planes and
/// axes become the selection; body rows use the ordinary Ctrl/Shift-aware selection path.
fn pick_from_browser(app: &mut SolveApp, x: Sel) {
    if let Sel::Body { name } = &x
        && dialog_takes_body(app)
        && let Some(mut d) = app.dialog.take()
    {
        let pick = if d.takes_components() {
            crate::dialogs::occurrence_path_of_body(&app.session, name).map(|path| Sel::Occurrence { path })
        } else {
            Some(x)
        };
        if let Some(pick) = pick {
            d.pick(&app.session, pick);
        }
        app.dialog = Some(d);
        return;
    }
    let hit = match &x {
        Sel::Plane { name } => crate::viewport::Hit::Plane { name: name.clone(), point: solidcraft_engine::geom::Vec3::ZERO },
        Sel::Axis { name } => crate::viewport::Hit::Axis { name: name.clone() },
        Sel::ConstructionPoint { name } => crate::viewport::Hit::ConstructionPoint { name: name.clone() },
        // The origin point goes only into a dialog input that takes it.
        Sel::Vertex { body, point } if body.is_empty() && app.dialog.is_some() => crate::viewport::Hit::Vertex { body: String::new(), point: *point },
        _ => return,
    };
    if let Some(mut d) = app.dialog.take() {
        if let Some(sel) = d.candidate(&app.session, &hit) {
            d.pick(&app.session, sel);
        }
        app.dialog = Some(d);
        return;
    }
    let _ = app.run("select.set", json!({ "items": [x] }));
}

/// One item of a component folder.
struct Entry {
    /// The highlight lookup key, owned once with the cached row.
    thing: Thing,
    /// The browser key (body name, or the feature id of a sketch or plane).
    key: String,
    /// The key in [`TreeState`] row order (`b:`, `s:`, `p:`).
    row_key: String,
    patterns: Option<String>,
    label: String,
    icon: &'static str,
    /// The tooltip's message id, if the row has one ([`Row::tip`]).
    tip: Option<&'static str>,
    visible: bool,
    selected: bool,
    color: Option<Color32>,
    /// A sketch's solve state: its icon's colour and its tooltip (#7245).
    sketch_state: Option<crate::sketch_state::SketchState>,
    locked: bool,
    sel: Option<Sel>,
    target: Option<Target>,
    /// Bodies lit when the row is hovered.
    bodies: Vec<String>,
}

impl Entry {
    fn body(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, world: String) -> Self {
        // A mesh body (an imported STL, 3MF, OBJ, …) has no faces to model on; an open shell is a
        // surface body, not a solid.
        let (icon, tip) = if b.body.is_mesh() {
            ("mesh_body", "ui.body_kind_mesh")
        } else if b.is_surface() {
            ("surface", "ui.body_kind_surface")
        } else {
            ("body", "ui.body_kind_solid")
        };
        Self {
            thing: Thing::Body(world.clone()),
            key: b.name.clone(),
            row_key: format!("b:{world}"),
            patterns: None,
            label: b.name.clone(),
            icon,
            tip: Some(tip),
            visible: app.body_visible(&world),
            selected: app.session.selection.iter().any(|s| matches!(s, Sel::Body { name } if name == &world)),
            color: None,
            sketch_state: None,
            locked: app.session.visibility.locked_bodies.contains(&world),
            sel: Some(Sel::Body { name: world.clone() }),
            target: Some(Target::Body { name: world.clone() }),
            bodies: vec![world],
        }
    }

    fn sketch(app: &SolveApp, f: &solidcraft_engine::doc::Feature) -> Self {
        let t = Tokens::get();
        let active = app.session.active_sketch == Some(f.id);
        let sketch_state = app.session.model.state().sketch(f.id).map(crate::sketch_state::SketchState::of);
        let lost = app.session.model.result(f.id).is_some_and(|result| result.error.is_some())
            && matches!(app.session.doc.sketch_face_attachment(f.id, &app.session.model), solidcraft_engine::doc::FaceAttachmentStatus::Lost { .. });
        Self {
            key: f.id.to_string(),
            row_key: format!("s:{}", f.id),
            thing: Thing::Sketch(f.id),
            patterns: crate::sketch_pattern_tree::has_patterns(app, f.id).then(|| format!("s:{}/patterns", f.id)),
            label: if lost {
                crate::i18n::format("browser.sketch_attachment_lost", &[("name", &crate::i18n::tr_name(&f.name))])
            } else if active {
                crate::i18n::format("browser.sketch_editing", &[("name", &crate::i18n::tr_name(&f.name))])
            } else {
                f.name.clone()
            },
            icon: "sketch",
            tip: None,
            visible: sketch_visible(app, f.id),
            selected: active || app.session.selection.iter().any(|s| matches!(s, Sel::Feature { id } if *id == f.id)),
            color: if lost { Some(t.error) } else { active.then_some(t.sketch_accent) },
            sketch_state,
            locked: false,
            sel: Some(Sel::Feature { id: f.id }),
            target: Some(Target::Sketch { id: f.id }),
            bodies: Vec::new(),
        }
    }

    fn plane(app: &SolveApp, f: &solidcraft_engine::doc::Feature) -> Self {
        let sel = construction_sel(f);
        let thing = match &sel {
            Sel::Axis { name } => Thing::Axis(name.clone()),
            Sel::ConstructionPoint { name } => Thing::Point(name.clone()),
            _ => Thing::Plane(f.name.clone()),
        };
        Self {
            key: f.id.to_string(),
            row_key: format!("p:{}", f.id),
            thing,
            patterns: None,
            label: f.name.clone(),
            icon: crate::timeline::icon_of(&f.kind),
            tip: None,
            visible: !app.session.visibility.hidden_origin.contains(&f.name),
            selected: app.session.selection.contains(&sel),
            color: None,
            sketch_state: None,
            locked: false,
            sel: Some(sel),
            target: Some(Target::Plane { id: f.id }),
            bodies: Vec::new(),
        }
    }

    fn is_visible(&self, app: &SolveApp) -> bool {
        match &self.sel {
            Some(Sel::Body { name }) => !app.session.visibility.hidden_bodies.contains(name),
            Some(Sel::Feature { id }) => sketch_visible(app, *id),
            Some(Sel::Plane { name }) => !app.session.visibility.hidden_origin.contains(name),
            _ => self.visible,
        }
    }

    fn is_selected(&self, app: &SolveApp) -> bool {
        self.sel
            .as_ref()
            .is_some_and(|s| app.session.selection.contains(s) || matches!(s, Sel::Feature { id } if app.session.active_sketch == Some(*id)))
    }
}

/// What is being dragged in the tree: items of one component folder.
#[derive(Clone, Debug)]
struct Drag {
    comp: u64,
    folder: &'static str,
    keys: Vec<String>,
    /// Groups dragged by their rows (alone: a drag carries items or groups).
    groups: Vec<u64>,
}

impl Drag {
    /// What is dragged, as the browser commands name it.
    fn entries(&self) -> Vec<BrowserEntry> {
        self.keys.iter().map(|k| BrowserEntry::item(k)).chain(self.groups.iter().map(|g| BrowserEntry::Group(*g))).collect()
    }
}

/// A placement row dragged in the tree: the exact path it sits at, and the component it places.
#[derive(Clone, Debug)]
struct OccurrenceDrag {
    /// The placement's path from the design root (its own occurrence id last).
    path: Vec<u64>,
    /// The component the placement is an instance of.
    component: u64,
}

/// The items of a component folder, in model order.
fn entries(app: &SolveApp, comp: u64, folder: &str) -> Vec<Entry> {
    let doc = &app.session.doc;
    let state = app.session.model.state();
    match folder {
        "bodies" => model_body_indices(app, comp)
            .into_iter()
            .filter_map(|i| state.bodies.get(i))
            .filter(|b| !app.session.body_removed_later(&b.name))
            .map(|b| Entry::body(app, b, b.name.clone()))
            .collect(),
        "sketches" => model_feature_indices(app, comp, true).into_iter().filter_map(|i| doc.features.get(i)).map(|f| Entry::sketch(app, f)).collect(),
        _ => model_feature_indices(app, comp, false).into_iter().filter_map(|i| doc.features.get(i)).map(|f| Entry::plane(app, f)).collect(),
    }
}

fn construction_sel(f: &solidcraft_engine::doc::Feature) -> Sel {
    let name = f.name.clone();
    match f.kind {
        FeatureKind::ConstructionPoint { .. } => Sel::ConstructionPoint { name },
        FeatureKind::ConstructionAxis { .. } => Sel::Axis { name },
        _ => Sel::Plane { name },
    }
}

/// A folder's rows in browser order (see `Document::ordered_entries`), then the items in no list
/// in their natural order. A group's rows are ordered the same way by its own list. `prefix` is
/// the scope the groups' tree keys start with.
fn arrange(doc: &solidcraft_engine::doc::Document, comp: u64, folder: &str, prefix: &str, all: Vec<Entry>) -> Vec<cache::Row> {
    use cache::Row;
    use solidcraft_engine::doc::BrowserEntry;
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::with_capacity(all.len());
    for (i, e) in all.iter().enumerate() {
        index.entry(e.key.clone()).or_insert(i);
    }
    let mut pool: Vec<Option<Entry>> = all.into_iter().map(Some).collect();
    let mut take = |key: &str| index.get(key).and_then(|i| pool[*i].take()).map(|e| Row::Item(Box::new(e)));
    fn build(
        doc: &solidcraft_engine::doc::Document,
        comp: u64,
        folder: &str,
        prefix: &str,
        id: u64,
        left: usize,
        take: &mut dyn FnMut(&str) -> Option<Row>,
    ) -> Option<cache::Group> {
        let g = doc.browser_group(id)?;
        let mut rows = Vec::new();
        for e in doc.ordered_entries(comp, folder, Some(id)) {
            match e {
                BrowserEntry::Item(k) => rows.extend(take(&k)),
                BrowserEntry::Group(c) if left > 0 => rows.extend(build(doc, comp, folder, prefix, c, left - 1, take).map(Row::Group)),
                BrowserEntry::Group(_) => {}
            }
        }
        let count = rows.iter().map(Row::count).sum();
        Some(cache::Group { id, key: format!("{prefix}/g{id}"), label: format!("{} ({count})", g.name), count, rows })
    }
    let order = doc.ordered_entries(comp, folder, None);
    // Groups first, so an item in a group is never also listed loose.
    let mut groups: std::collections::HashMap<u64, cache::Group> = std::collections::HashMap::new();
    for e in &order {
        if let BrowserEntry::Group(id) = e {
            groups.extend(build(doc, comp, folder, prefix, *id, solidcraft_engine::doc::MAX_GROUP_NESTING, &mut take).map(|g| (*id, g)));
        }
    }
    let mut rows = Vec::new();
    for e in order {
        match e {
            BrowserEntry::Group(id) => rows.extend(groups.remove(&id).map(Row::Group)),
            BrowserEntry::Item(k) => rows.extend(take(&k)),
        }
    }
    rows.extend(pool.into_iter().flatten().map(|e| Row::Item(Box::new(e))));
    rows
}

/// Show or hide items of a folder, through `browser.visibility` so each is an undo step (#2219).
fn set_visible(app: &mut SolveApp, folder: &str, items: &[&Entry], on: bool) {
    for e in items {
        match folder {
            "bodies" => {
                if !e.bodies.is_empty() {
                    let _ = app.run("browser.visibility", json!({ "items": e.bodies.clone(), "visible": on }));
                }
            }
            "sketches" => {
                if let Ok(id) = e.key.parse() {
                    set_sketch_visible(app, id, on);
                }
            }
            _ => {
                let _ = app.run("browser.visibility", json!({ "items": [e.label], "visible": on }));
            }
        }
    }
}

fn folder_label(folder: &str) -> &'static str {
    match folder {
        "bodies" => "Bodies",
        "sketches" => "Sketches",
        _ => "Construction",
    }
}

fn folder_static(folder: &str) -> &'static str {
    match folder {
        "bodies" => "bodies",
        "sketches" => "sketches",
        _ => "construction",
    }
}

/// Whether a component's Bodies folder is folded away, its bodies (and groups) listed straight
/// under the component or placement row (Preferences › Interface › Bodies folders, #4217).
/// `scope` is that row's tree key (`c{id}`, `o:{path}`) and `bodies` how many items the folder
/// holds. The design's own folder always stays, and so does a hidden one: its eye is the way back.
fn bodies_folded(app: &SolveApp, comp: u64, scope: &str, bodies: usize) -> bool {
    use crate::prefs::BodiesFolders;
    if comp == 0 || bodies == 0 || app.session.folder_hidden(&format!("{scope}/bodies")) {
        return false;
    }
    match app.preferences.browser_bodies_folders {
        BodiesFolders::Always => false,
        BodiesFolders::Several => bodies == 1 && !app.session.doc.browser_groups.iter().any(|g| g.component == comp && g.folder == "bodies"),
        BodiesFolders::Never => true,
    }
}

/// A component's folder: its row, its groups and its items. Rows are drag sources; items,
/// groups and the folder row are drop targets (reorder, into a group, out of groups).
#[allow(clippy::too_many_arguments)]
fn folder_rows_at(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    comp: u64,
    path: Option<&[u64]>,
    folder: &str,
    depth: usize,
    acts: &mut Actions,
    data: &cache::Folder,
) {
    if data.key.is_empty() {
        return;
    }
    let fstatic = folder_static(folder);
    let key = &data.key;
    let prefix = key.rsplit_once('/').map_or(key.as_str(), |(prefix, _)| prefix);
    if let Some(r) = app.tree.reveal.as_deref()
        && folder == "bodies"
        && let Some(e) = data.entries().find(|e| e.row_key.strip_prefix("b:") == Some(r))
    {
        app.tree.collapsed.remove(key);
        app.tree.collapsed.remove(&format!("c{comp}"));
        open_group_chain(app, "", comp, "bodies", &e.key);
    }
    // A folded folder draws no row: its groups and items move up into its place, and their own
    // eyes, hover and drops do its jobs. It is always open, as nothing could open it again.
    let render = crate::render_workspace::active(app);
    let folded = !render && folder == "bodies" && bodies_folded(app, comp, prefix, data.count);
    // With the history in the browser the sketches are there too: their folder starts closed.
    let default_open = !(folder == "sketches" && app.preferences.history_in_browser);
    let open = folded || is_open(app, key, default_open);
    let inner = if folded { depth } else { depth + 1 };
    // The folder's eye is its own: hiding it hides its items without changing theirs, and its
    // items never change it. It and everything in it fade while it or a placement above is
    // hidden, whatever their own eyes say.
    let shown = !app.session.folder_hidden(key);
    let folder_faded = !shown || path.is_some_and(|p| !app.session.occurrence_visible(p));
    if !render {
        let r = if folded {
            RowResp::default()
        } else {
            draw_row(
                ui,
                ui.id().with(key),
                &Row {
                    depth,
                    fold: Some(open),
                    eye: Some(shown),
                    icon: "folder",
                    label: folder_label(folder),
                    dim: folder_faded,
                    mark: if acts.lit.any() { acts.lit.branch(app, data.entries()) } else { Mark::None },
                    ..Default::default()
                },
            )
        };
        if r.fold {
            toggle(app, key, default_open);
        } else if r.clicked {
            // The Bodies label is the whole set while a command is asking for bodies. The arrow
            // still folds it. With no such command, the label folds, as it always has.
            let names: Vec<String> = data.entries().flat_map(|e| e.bodies.clone()).collect();
            if !offer_bodies(app, &names, Take::Toggle) {
                pick_folder(app, key);
                toggle(app, key, default_open);
            }
        }
        if r.eye {
            toggle_folder(app, key);
        }
        if r.hovered {
            acts.hover(data.entries().flat_map(|e| &e.bodies));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Folder { component: comp, folder: fstatic.to_string(), scope: prefix.to_owned() }));
        }
        // Dropping on the folder row takes items out of their groups, to the end. Bodies or sketches
        // from another component's folder move into this component, as a drop on its row does
        // (#3270: the Bodies folder looked like the place to drop a body, and took nothing).
        if let Some(resp) = &r.resp
            && let Some(d) = drop_target(
                ui,
                resp,
                |d: &Drag| d.folder == fstatic && (d.comp == comp || (d.folder != "construction" && d.groups.is_empty())),
                DropMark::Box,
            )
        {
            acts.drop = Some(if d.comp == comp {
                DropAction::Order {
                    comp,
                    folder: fstatic,
                    keys: d.keys.clone(),
                    groups: d.groups.clone(),
                    order: order_before(&data.top_order, &d.entries(), None),
                }
            } else {
                DropAction::Component { to: comp, folder: d.folder, keys: d.keys.clone() }
            });
        }
        if !open {
            return;
        }
    }
    folder_level(app, ui, comp, path, fstatic, prefix, &data.rows, None, inner, &data.top_order, folder_faded, acts);
}

/// The rows of a folder's top level, or of one group (`holder`), at `depth`; groups draw their
/// own rows the same way one level deeper. `top` is the folder's top-level order as drawn.
#[allow(clippy::too_many_arguments)]
fn folder_level(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    comp: u64,
    path: Option<&[u64]>,
    fstatic: &'static str,
    prefix: &str,
    rows: &[cache::Row],
    holder: Option<u64>,
    depth: usize,
    top: &[BrowserEntry],
    faded: bool,
    acts: &mut Actions,
) {
    let render = crate::render_workspace::active(app);
    // The render workspace has always listed a folder's loose items above its groups.
    let items_first = render && holder.is_none();
    let rows = rows
        .iter()
        .filter(|r| items_first && matches!(r, cache::Row::Item(_)))
        .chain(rows.iter().filter(|r| !items_first || matches!(r, cache::Row::Group(_))));
    for row in rows {
        let group = match row {
            cache::Row::Item(e) => {
                entry_row(app, ui, comp, path, fstatic, e, depth, holder, top, faded, acts);
                continue;
            }
            cache::Row::Group(g) => g,
        };
        let (id, gkey) = (group.id, &group.key);
        let gopen = is_open(app, gkey, true);
        let gshown = !app.session.folder_hidden(gkey);
        let group_faded = faded || !gshown;
        let label = if render {
            app.session.doc.browser_groups.iter().find(|g| g.id == id).map_or(group.label.as_str(), |g| g.name.as_str())
        } else {
            &group.label
        };
        let count = if render && !gopen { crate::i18n::format("render.tree_more", &[("count", &group.count.to_string())]) } else { String::new() };
        let r = draw_row(
            ui,
            ui.id().with(gkey),
            &Row {
                depth,
                fold: Some(gopen),
                eye: (!render || gopen).then_some(gshown),
                icon: if render { "" } else { "folder" },
                label,
                name: true,
                dim: group_faded || (render && !gopen),
                count: &count,
                mark: if acts.lit.any() { acts.lit.branch(app, group.entries()) } else { Mark::None },
                ..Default::default()
            },
        );
        if r.fold {
            toggle(app, gkey, true);
        } else if r.clicked {
            let names: Vec<String> = group.entries().into_iter().flat_map(|e| e.bodies.clone()).collect();
            if !offer_bodies(app, &names, Take::Toggle) {
                pick_folder(app, gkey);
                toggle(app, gkey, true);
            }
        }
        if r.eye {
            toggle_folder(app, gkey);
        }
        if r.hovered {
            acts.hover(group.entries().into_iter().flat_map(|e| &e.bodies));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Group { id, scope: prefix.to_owned() }));
        }
        if let Some(resp) = &r.resp {
            // Dragging a group row moves the group; a press on its arrow or eye is not a drag.
            if resp.drag_started() && !r.on_control {
                resp.dnd_set_drag_payload(Drag { comp, folder: fstatic, keys: Vec::new(), groups: vec![id] });
            }
            // The top of the row puts the drop before the group; the rest puts it inside.
            let level = app.session.doc.group_level(id);
            let fits = |d: &Drag, parent_level: usize| {
                let doc = &app.session.doc;
                d.groups.iter().all(|g| !doc.group_within(id, *g) && parent_level + doc.group_height(*g) <= solidcraft_engine::doc::MAX_GROUP_NESTING)
            };
            if let Some((d, zone)) = drop_target_zone(ui, resp, |d: &Drag, zone| {
                d.comp == comp && d.folder == fstatic && fits(d, if zone == DropMark::Above { level - 1 } else { level })
            }) {
                let before = BrowserEntry::Group(id);
                acts.drop = Some(match (zone, holder) {
                    (DropMark::Box, _) => {
                        DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), groups: d.groups.clone(), group: Some(id), before: None }
                    }
                    (DropMark::Above, Some(h)) => DropAction::Move {
                        comp,
                        folder: fstatic,
                        keys: d.keys.clone(),
                        groups: d.groups.clone(),
                        group: Some(h),
                        before: Some(before),
                    },
                    (DropMark::Above, None) => DropAction::Order {
                        comp,
                        folder: fstatic,
                        order: order_before(top, &d.entries(), Some(&before)),
                        keys: d.keys.clone(),
                        groups: d.groups.clone(),
                    },
                });
            }
        }
        if gopen {
            folder_level(app, ui, comp, path, fstatic, prefix, &group.rows, Some(id), depth + 1, top, group_faded, acts);
        }
    }
}

/// `top` with the dragged entries moved to just before `before` (to the end without it).
fn order_before(top: &[BrowserEntry], dragged: &[BrowserEntry], before: Option<&BrowserEntry>) -> Vec<BrowserEntry> {
    let mut order: Vec<BrowserEntry> = top.iter().filter(|e| !dragged.contains(e)).cloned().collect();
    let at = before.and_then(|b| order.iter().position(|e| e == b)).unwrap_or(order.len());
    for (i, e) in dragged.iter().enumerate() {
        order.insert(at + i, e.clone());
    }
    order
}

/// The body's name edited in its row (started by a double-click): Enter or leaving the box keeps
/// the name, Escape cancels.
fn inline_rename(app: &mut SolveApp, ui: &mut egui::Ui, row: Rect, label_left: f32, e: &Entry) {
    let editing = app.menu.rename.as_ref().is_some_and(|r| r.inline && matches!(&r.what, crate::context_menu::RenameWhat::Body(b) if *b == e.key));
    if !editing {
        return;
    }
    let Some(mut rename) = app.menu.rename.take() else { return };
    let edit_rect = Rect::from_min_max(pos2(label_left, row.top() + 1.0), pos2(row.right() - 3.0, row.bottom() - 1.0));
    ui.painter().rect_filled(edit_rect, 3.0, Tokens::get().field);
    let id = ui.id().with(("browser_rename", &e.row_key));
    let response = crate::ui_kit::inline_name(ui, edit_rect, id, &mut rename.text);
    crate::scenario::publish_handle(&format!("rename:{}", e.key), edit_rect.center());
    let was_focused = rename.focused;
    if !rename.focused {
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
            let end = rename.text.chars().count();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
            state.store(ui.ctx(), id);
        }
        response.request_focus();
        rename.focused = true;
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let commit = ui.input(|i| i.key_pressed(egui::Key::Enter)) || (was_focused && response.lost_focus());
    if escape {
        // Dropping the state cancels the edit.
    } else if commit {
        crate::context_menu::commit_rename(app, &rename);
    } else {
        app.menu.rename = Some(rename);
    }
}

/// One item row of a folder.
#[allow(clippy::too_many_arguments)]
fn entry_row(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    comp: u64,
    path: Option<&[u64]>,
    folder: &'static str,
    e: &Entry,
    depth: usize,
    group: Option<u64>,
    top: &[BrowserEntry],
    // A placement or folder above is hidden: the row fades, its eye stays its own.
    parent_hidden: bool,
    acts: &mut Actions,
) {
    let appearance = crate::render_workspace::scene::decoration(app, &e.bodies);
    if crate::render_workspace::active(app) && !crate::render_workspace::scene::matches_query(app, &e.label, &appearance.name) {
        return;
    }
    app.tree.remember(&e.row_key);
    let pattern_key = e.patterns.as_deref();
    let patterns_open = pattern_key.is_some_and(|key| is_open(app, key, true));
    let rect = row_rect(ui);
    let revealed = folder == "bodies" && app.tree.reveal.as_ref().is_some_and(|b| e.row_key.strip_prefix("b:") == Some(b.as_str()));
    if !ui.is_rect_visible(rect) && !patterns_open && !revealed {
        ui.allocate_space(rect.size());
        return;
    }
    let (selected, mark) = lit_row(acts.lit.entry(app, e));
    let visible = e.is_visible(app);
    let locked = match &e.sel {
        Some(Sel::Body { name }) => app.session.visibility.locked_bodies.contains(name),
        _ => e.locked,
    };
    // What is cut (a body, a sketch, or a body made by a cut feature) has a dashed
    // outline until Paste moves it.
    let cut = (folder == "bodies" && crate::edit_clipboard::body_is_cut(app, &e.key))
        || matches!(&e.sel, Some(Sel::Feature { id }) if app.cut.features.contains(id));
    let badges: &[&str] = if locked { &["lock"] } else { &[] };
    // Cached with the evaluated model; the preference is read as the row is drawn.
    let sketch_state = e.sketch_state.filter(|_| app.preferences.sketch_state_icons);
    let state_tip = sketch_state.map(crate::sketch_state::SketchState::text);
    let r = draw_row(
        ui,
        ui.id().with(("item", &e.row_key)),
        &Row {
            depth,
            fold: pattern_key.map(|_| patterns_open),
            eye: Some(visible),
            icon: e.icon,
            kind_tip: e.tip,
            label: &e.label,
            name: true,
            selected,
            dim: !visible || parent_hidden,
            cut,
            color: e.color,
            swatch: appearance.swatch,
            secondary: &appearance.name,
            count: &appearance.count,
            icon_ink: sketch_state.and_then(|state| state.ink(&Tokens::get())),
            tip: state_tip.as_deref(),
            badges,
            mark,
            ..Default::default()
        },
    );
    if r.fold
        && let Some(key) = pattern_key
    {
        toggle(app, key, true);
    }
    if folder == "bodies"
        && app.tree.reveal.as_ref().is_some_and(|b| e.row_key.strip_prefix("b:") == Some(b.as_str()))
        && let Some(rect) = r.rect
    {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
        app.tree.reveal = None;
    }
    if r.eye {
        set_visible(app, folder, &[e], !visible);
    }
    if r.double && folder == "sketches" {
        if let Ok(id) = e.key.parse() {
            app.edit_sketch_in(id, path.map(|path| (comp, path)));
        }
    } else if r.clicked {
        let shift = ui.input(|i| i.modifiers.shift);
        // A body row with a dialog open is a pick, as Combine does (#4452), unless Shift is
        // asking for the rows between this one and the anchor.
        if folder == "construction" || (folder == "bodies" && !shift && app.dialog.is_some()) {
            if let Some(s) = e.sel.clone() {
                if matches!(s, Sel::Body { .. }) {
                    app.tree.anchor = Some(e.row_key.clone());
                }
                pick_from_browser(app, s);
            }
        } else if folder == "sketches"
            && !ui.input(|i| i.modifiers.any())
            && let Some(Sel::Feature { id }) = e.sel
            && pick_sketch_into_dialog(app, id)
        {
            // Extrude, Revolve or Emboss took all the sketch's profiles (#6106).
        } else {
            click_select(app, ui, e.row_key.clone(), e.sel.clone());
        }
        // A double-click renames the row (sketch rows open for editing instead), like F2. A body
        // is edited in its own row.
        if r.double {
            crate::context_menu::rename_selection(app, r.rect.map(|x| x.left_bottom()).unwrap_or_default());
            if folder == "bodies"
                && let Some(rename) = app.menu.rename.as_mut()
                && matches!(&rename.what, crate::context_menu::RenameWhat::Body(b) if *b == e.key)
            {
                rename.inline = true;
            }
        }
    }
    if folder == "bodies"
        && let (Some(rect), Some(label_left)) = (r.rect, r.label_left)
    {
        inline_rename(app, ui, rect, label_left, e);
    }
    if r.hovered && !e.bodies.is_empty() {
        acts.hover(e.bodies.iter());
    }
    if r.hovered
        && let Thing::Plane(name) = &e.thing
    {
        acts.hover_plane = Some(name.clone());
    }
    if let Some(p) = r.secondary
        && let Some(t) = e.target.clone()
    {
        if !selected && let Some(s) = &e.sel {
            let _ = app.run("select.set", json!({ "items": [s] }));
        }
        acts.menu = Some((p, t));
    }
    if patterns_open && let Some(Sel::Feature { id }) = &e.sel {
        crate::sketch_pattern_tree::rows(app, ui, *id, depth + 1, "browser", path.map(|path| (comp, path)), !visible || parent_hidden);
    }
    let Some(resp) = &r.resp else { return };
    // Drag: the selected items of this folder when this one is selected, else just this one.
    if resp.drag_started() && !r.on_control {
        let keys: Vec<String> = if selected { selected_keys(app, comp, folder) } else { vec![e.key.clone()] };
        resp.dnd_set_drag_payload(Drag { comp, folder, keys, groups: Vec::new() });
    }
    let fits = |d: &Drag| {
        let doc = &app.session.doc;
        let level = group.map_or(0, |g| doc.group_level(g));
        d.groups
            .iter()
            .all(|g| group.is_none_or(|h| !doc.group_within(h, *g)) && level + doc.group_height(*g) <= solidcraft_engine::doc::MAX_GROUP_NESTING)
    };
    if let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == folder && !d.keys.contains(&e.key) && fits(d), DropMark::Above) {
        let before = BrowserEntry::item(&e.key);
        acts.drop = Some(match group {
            Some(g) => DropAction::Move { comp, folder, keys: d.keys.clone(), groups: d.groups.clone(), group: Some(g), before: Some(before) },
            None => DropAction::Order {
                comp,
                folder,
                order: order_before(top, &d.entries(), Some(&before)),
                keys: d.keys.clone(),
                groups: d.groups.clone(),
            },
        });
    }
}

/// Keys of the selected items of a component folder.
fn selected_keys(app: &SolveApp, comp: u64, folder: &str) -> Vec<String> {
    entries(app, comp, folder).into_iter().filter(|e| e.selected).map(|e| e.key).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum DropMark {
    /// A line above the row: the items go before it.
    Above,
    /// A box around the row: the items go into it.
    Box,
}

/// While a matching payload hovers the row, mark where it would land; on release, the payload.
fn drop_target(ui: &egui::Ui, resp: &egui::Response, accept: impl Fn(&Drag) -> bool, mark: DropMark) -> Option<std::sync::Arc<Drag>> {
    let t = Tokens::get();
    let hovering = resp.dnd_hover_payload::<Drag>().filter(|d| accept(d));
    if hovering.is_some() {
        let r = resp.rect;
        match mark {
            DropMark::Above => ui.painter().hline(r.x_range(), r.top(), Stroke::new(2.0, t.accent)),
            DropMark::Box => {
                ui.painter().rect_stroke(r.shrink(0.5), t.radius_sm.saturating_sub(1), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside)
            }
        };
    }
    // Taking a payload takes it whatever its type: leave other drags (appearance swatches) be.
    if !egui::DragAndDrop::has_payload_of_type::<Drag>(ui.ctx()) {
        return None;
    }
    resp.dnd_release_payload::<Drag>().filter(|d| accept(d))
}

/// Like [`drop_target`] for a row that takes a drop above it (its top part) or inside it.
fn drop_target_zone(ui: &egui::Ui, resp: &egui::Response, accept: impl Fn(&Drag, DropMark) -> bool) -> Option<(std::sync::Arc<Drag>, DropMark)> {
    let t = Tokens::get();
    let r = resp.rect;
    let at = ui.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos()));
    let zone = if at.is_some_and(|p| p.y < r.top() + r.height() * 0.3) { DropMark::Above } else { DropMark::Box };
    if resp.dnd_hover_payload::<Drag>().is_some_and(|d| accept(&d, zone)) {
        match zone {
            DropMark::Above => ui.painter().hline(r.x_range(), r.top(), Stroke::new(2.0, t.accent)),
            DropMark::Box => {
                ui.painter().rect_stroke(r.shrink(0.5), t.radius_sm.saturating_sub(1), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside)
            }
        };
    }
    if !egui::DragAndDrop::has_payload_of_type::<Drag>(ui.ctx()) {
        return None;
    }
    resp.dnd_release_payload::<Drag>().filter(|d| accept(d, zone)).map(|d| (d, zone))
}

/// Whether a placement dragged from `d` may land in component `parent`: not where it already is,
/// and not in itself or a component it holds. The command refuses those too; this keeps the row
/// from showing a drop mark it would refuse.
fn occurrence_can_land(app: &SolveApp, d: &OccurrenceDrag, parent: u64) -> bool {
    let Some(id) = d.path.last().copied() else { return false };
    let Some(o) = app.session.doc.occurrences.iter().find(|o| o.id == id) else { return false };
    if parent == 0 {
        // The design row takes any placement not already at the design root: there is no cycle
        // to make at the top, and everything is inside the design.
        return o.parent != 0;
    }
    o.parent != parent && !app.session.doc.inside_component(parent, d.component)
}

/// Whether the pointer is over the top part of a row (where a drop goes before it).
fn pointer_in_top(ui: &egui::Ui, row: Rect) -> bool {
    ui.input(|i| i.pointer.hover_pos().or(i.pointer.latest_pos())).is_some_and(|p| p.y < row.top() + row.height() * 0.3)
}

/// The component a placement shares with the placement under it, when both sit in the same one
/// (and are two placements): the parent whose sub-components they are ordered among.
fn occurrence_sibling(app: &SolveApp, d: &OccurrenceDrag, target: u64) -> Option<u64> {
    let dragged = d.path.last().copied().filter(|o| *o != target)?;
    let occurrences = &app.session.doc.occurrences;
    let parent = occurrences.iter().find(|o| o.id == dragged)?.parent;
    occurrences.iter().any(|o| o.id == target && o.parent == parent).then_some(parent)
}

/// While a matching placement drag hovers the row, mark a drop before it; on release, the payload.
fn occurrence_reorder_target(
    ui: &egui::Ui,
    resp: &egui::Response,
    accept: impl Fn(&OccurrenceDrag) -> bool,
) -> Option<std::sync::Arc<OccurrenceDrag>> {
    if resp.dnd_hover_payload::<OccurrenceDrag>().is_some_and(|d| accept(&d)) {
        ui.painter().hline(resp.rect.x_range(), resp.rect.top(), Stroke::new(2.0, Tokens::get().accent));
    }
    if !egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(ui.ctx()) {
        return None;
    }
    resp.dnd_release_payload::<OccurrenceDrag>().filter(|d| accept(d))
}

/// While a matching placement drag hovers the row, mark where it would land; on release, the payload.
fn occurrence_drop_target(ui: &egui::Ui, resp: &egui::Response, accept: impl Fn(&OccurrenceDrag) -> bool) -> Option<std::sync::Arc<OccurrenceDrag>> {
    let t = Tokens::get();
    let hovering = resp.dnd_hover_payload::<OccurrenceDrag>().filter(|d| accept(d));
    if hovering.is_some() {
        let r = resp.rect;
        ui.painter().rect_stroke(r.shrink(0.5), t.radius_sm.saturating_sub(1), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
    }
    // Taking a payload takes it whatever its type: leave other drags (appearance swatches) be.
    if !egui::DragAndDrop::has_payload_of_type::<OccurrenceDrag>(ui.ctx()) {
        return None;
    }
    resp.dnd_release_payload::<OccurrenceDrag>().filter(|d| accept(d))
}

/// A drop, applied after the tree is drawn.
enum DropAction {
    /// Into a group (before an item of it), or out of groups.
    Move { comp: u64, folder: &'static str, keys: Vec<String>, groups: Vec<u64>, group: Option<u64>, before: Option<BrowserEntry> },
    /// Out of groups, in this order of the folder's top level.
    Order { comp: u64, folder: &'static str, keys: Vec<String>, groups: Vec<u64>, order: Vec<BrowserEntry> },
    /// Into another component (bodies and sketches).
    Component { to: u64, folder: &'static str, keys: Vec<String> },
    /// A placement moved into another component (or the design, `parent` 0).
    Reparent { occurrence: u64, path: Vec<u64>, parent: u64, parent_path: Option<Vec<u64>> },
}

fn entry_json(e: &BrowserEntry) -> Value {
    match e {
        BrowserEntry::Group(id) => json!(id),
        BrowserEntry::Item(k) => json!(k),
    }
}

/// Items (strings) and groups (numbers), as the browser commands take them.
fn entries_json(keys: &[String], groups: &[u64]) -> Vec<Value> {
    keys.iter().map(|k| json!(k)).chain(groups.iter().map(|g| json!(g))).collect()
}

fn apply_drop(app: &mut SolveApp, d: DropAction) {
    match d {
        DropAction::Move { comp, folder, keys, groups, group, before } => {
            let items = entries_json(&keys, &groups);
            let before = before.map(|b| entry_json(&b));
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": items, "group": group, "before": before }));
        }
        DropAction::Order { comp, folder, keys, groups, order } => {
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": entries_json(&keys, &groups) }));
            let order: Vec<Value> = order.iter().map(entry_json).collect();
            let _ = app.run("browser.order", json!({ "component": comp, "folder": folder, "order": order }));
        }
        DropAction::Component { to, folder, keys } => {
            let _ = match folder {
                "bodies" => app.run("component.move_bodies", json!({ "bodies": keys, "component": to })),
                "sketches" => app.run("component.move_sketches", json!({ "sketches": keys, "component": to })),
                _ => Ok(serde_json::Value::Null),
            };
        }
        DropAction::Reparent { occurrence, path, parent, parent_path } => {
            let mut payload = json!({ "occurrence": occurrence, "path": path, "parent": parent });
            if let Some(pp) = parent_path {
                payload["parent_path"] = json!(pp);
            }
            match app.run("occurrence.reparent", payload) {
                Ok(out) => {
                    let to: Vec<u64> = out
                        .get("path")
                        .and_then(serde_json::Value::as_array)
                        .map(|a| a.iter().filter_map(serde_json::Value::as_u64).collect())
                        .unwrap_or_default();
                    if !to.is_empty() {
                        close_placement(app, &to);
                    }
                }
                Err(e) => app.set_status(e, true),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Panels opened from the menus
// ---------------------------------------------------------------------------------------------

pub(crate) fn revert_occurrence_dialog(app: &mut SolveApp, m: &OccMove) {
    if m.had_pending {
        let _ = app.run("occurrence.move", json!({"occurrence":m.occurrence,"base_transform":m.baseline,"capture":false}));
    } else {
        let _ = app.run("component.revert_position", json!({"occurrences":[m.occurrence]}));
    }
}

/// The pending move: turned about X, then Y, then Z through the starting origin, then moved.
pub fn apply_occurrence_move(app: &mut SolveApp, m: &OccMove) {
    use crate::gizmo;
    use solidcraft_engine::geom::Vec3;
    let a = m.angles.map(f64::to_radians);
    let r = gizmo::mul(&gizmo::mul(&gizmo::rotation(Vec3::Z, a[2]), &gizmo::rotation(Vec3::Y, a[1])), &gizmo::rotation(Vec3::X, a[0]));
    let (axis, angle) = gizmo::axis_angle(&r);
    let o = m.pivot.unwrap_or(Vec3::ZERO);
    let baseline = m.baseline.or_else(|| app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.transform));
    let _ = app.run(
        "occurrence.move",
        if m.path.is_empty() {
            json!({ "occurrence": m.occurrence, "base_transform": baseline, "translate": m.translate, "axis": [axis.x, axis.y, axis.z], "angle": angle, "origin": [o.x, o.y, o.z], "capture": false })
        } else {
            json!({ "occurrence": m.occurrence, "path": m.path, "space": "world", "base_transform": baseline, "translate": m.translate, "axis": [axis.x, axis.y, axis.z], "angle": angle, "origin": [o.x, o.y, o.z], "capture": false })
        },
    );
}

/// The shared move triad at the occurrence's origin: arrows and squares move it, rings turn it.
pub(crate) fn occurrence_triad_model(app: &SolveApp) -> Option<crate::gizmo::Triad> {
    let movement = app.tree.occurrence_move.as_ref()?;
    let center = movement.pivot? + solidcraft_engine::geom::Vec3::new(movement.translate[0], movement.translate[1], movement.translate[2]);
    Some(crate::gizmo::Triad { center, translate: true, rotate: true })
}

/// Occurrence handles belong to the viewport, not the clipped Move panel's child UI.
pub(crate) fn occurrence_triad(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    use crate::gizmo::{self, Change};
    let Some(mut m) = app.tree.occurrence_move.clone() else { return };
    let before = (m.translate, m.angles);
    let Some(t) = occurrence_triad_model(app) else { return };
    let step = crate::canvas::snap_step(app.cam.half_height());
    let Some(g) = gizmo::show(ui, painter, proj, egui::Id::new("sc_occ_triad"), &t, step) else { return };
    if g.started || m.drag0.is_none() {
        m.drag0 = Some((m.translate, m.angles));
    }
    let (t0, a0) = m.drag0.unwrap_or((m.translate, m.angles));
    match g.change {
        Change::Translate(v) => m.translate = [t0[0] + v.x, t0[1] + v.y, t0[2] + v.z],
        Change::Rotate { axis, angle } => {
            if let Some(slot) = m.angles.get_mut(axis) {
                *slot = a0.get(axis).copied().unwrap_or(0.0) + angle.to_degrees();
            }
        }
    }
    if g.done {
        m.drag0 = None;
    }
    if (m.translate, m.angles) != before {
        apply_occurrence_move(app, &m);
    }
    app.tree.occurrence_move = Some(m);
}

/// Dimensions of finished sketches whose dimensions are shown (Show Dimensions), drawn over the
/// viewport like the sketch being edited draws its own (`crate::dim_view`). A double-click on a
/// value edits it in place without editing the sketch (`crate::dim_view::model_double_click`).
pub fn dims_overlay(app: &SolveApp, ctx: &egui::Context) {
    use solidcraft_engine::geom::Vec2;
    use solidcraft_engine::sketch::{DimFrame, chain_centres, default_text, dim_frame, dim_layout_reaching, dim_reach};
    crate::dim_view::set_model_drawn(Vec::new());
    let Some(rect) = app.viewport.rect else { return };
    if app.session.visibility.shown_dimensions.is_empty() {
        return;
    }
    let mut drawn = Vec::new();
    let tk = Tokens::get();
    let proj = crate::viewport::projection(app, rect);
    let painter = ctx.layer_painter(egui::LayerId::background()).with_clip_rect(rect);
    let st = app.session.world_state();
    for id in app
        .session
        .visibility
        .shown_dimensions
        .iter()
        .filter(|id| app.session.active_sketch != Some(**id) && solidcraft_engine::view::sketch_shown(&app.session, **id))
    {
        let Some(ss) = st.sketch(*id) else { continue };
        let sk = &ss.sketch;
        let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
        // Unplaced dimensions sit away from the middle of the sketch.
        let centres = chain_centres(sk);
        let n = centres.len().max(1) as f64;
        let middle = centres.iter().fold(Vec2::ZERO, |a, p| a + *p) * (1.0 / n);
        for c in &sk.constraints {
            let Some(frame) = dim_frame(sk, &c.kind) else { continue };
            let anchor = match frame {
                DimFrame::Linear { p0, p1, .. } => (p0 + p1) * 0.5,
                DimFrame::Radial { center, .. } | DimFrame::ArcLength { center, .. } => center,
                DimFrame::Angular { vertex, .. } => vertex,
            };
            let (Some(a), Some(b)) = (to(anchor), to(anchor + Vec2::X)) else { continue };
            let px = 1.0 / f64::from(a.distance(b)).max(1e-9);
            if !px.is_finite() {
                continue;
            }
            let expr = c.param.as_ref().and_then(|p| app.session.doc.param(p)).map(|p| p.expr.as_str());
            let col = if c.driven { tk.dimension_driven } else { tk.dimension };
            let galley = painter.layout_no_wrap(
                crate::dim_view::label(&c.kind, expr, c.driven, &app.session.doc.units, app.preferences.length_decimals),
                FontId::proportional(tk.font_caption),
                col,
            );
            let size = galley.size() + vec2(6.0, 2.0);
            let half = Vec2::new(f64::from(size.x) * 0.5 * px, f64::from(size.y) * 0.5 * px);
            let lay = dim_layout_reaching(&frame, c.text.or_else(|| default_text(&frame, middle, px)), px, half, dim_reach(sk, &c.kind));
            for line in &lay.lines {
                painter.add(egui::Shape::line(line.iter().filter_map(|q| to(*q)).collect(), Stroke::new(1.0, col)));
            }
            for (tip, dir) in &lay.arrows {
                if let (Some(t), Some(back)) = (to(*tip), to(*tip - *dir * px)) {
                    crate::icons::markers::dimension_arrow(&painter, t, back, col);
                }
            }
            let Some(tc) = to(lay.text) else { continue };
            let r = Rect::from_center_size(tc, size);
            painter.rect_filled(r, tk.radius_sm / 2, tk.viewport_top.gamma_multiply(0.85));
            painter.galley(r.min + vec2(3.0, 1.0), galley, col);
            if rect.contains(tc) {
                drawn.push(crate::dim_view::ModelDim { sketch: *id, id: c.id.clone(), param: c.param.clone(), text: r });
            }
        }
    }
    crate::dim_view::set_model_drawn(drawn);
}

/// Keep browser panels beside the browser and below the ribbon, including the floating shell.
pub(crate) fn panel_pos(app: &SolveApp) -> Pos2 {
    app.viewport
        .rect
        .map(|r| crate::shell::content_rect(app, r).left_top() + vec2(Tokens::get().space_16, Tokens::get().space_16))
        .unwrap_or(pos2(270.0, 140.0))
}

/// The Capture Position question stands for saving.
pub const SAVE: &str = "__save";

/// Does starting `what` (a command id or [`SAVE`]) need the Capture Position question first?
/// Yes while components were moved without capturing their positions, except for the commands
/// that capture, revert or keep moving.
pub fn needs_capture(app: &SolveApp, what: &str) -> bool {
    !app.session.pending_moves.is_empty()
        && !matches!(
            what,
            "component.capture_position"
                | "component.revert_position"
                | "occurrence.move"
                | "occurrence.align"
                | "occurrence.drag"
                | "edit.undo"
                | "edit.redo"
        )
        && app.tree.capture_prompt.is_none()
}

/// Answer the Capture Position question: `capture`, `revert` or `cancel`. The waiting command
/// (or save) then goes ahead, unless cancelled.
pub fn answer_capture(app: &mut SolveApp, action: &str) -> Result<(), String> {
    let Some(what) = app.tree.capture_prompt.clone() else { return Err("no Capture Position question is open".into()) };
    match action {
        "capture" => drop(app.run("component.capture_position", json!({}))),
        "revert" => drop(app.run("component.revert_position", json!({}))),
        "cancel" => {
            app.tree.capture_prompt = None;
            return Ok(());
        }
        other => return Err(format!("unknown answer `{other}` (capture, revert, cancel)")),
    }
    app.tree.capture_prompt = None;
    app.tree.occurrence_move = None;
    if what == SAVE {
        crate::toolbar::save(app);
    } else {
        app.start(&what);
    }
    Ok(())
}

/// Where an occurrence's origin is now (with its pending move).
pub(crate) fn occurrence_origin(app: &SolveApp, occurrence: u64) -> Option<solidcraft_engine::geom::Vec3> {
    let o = app.session.doc.occurrences.iter().find(|o| o.id == occurrence)?;
    let parent = app.session.doc.component_transform(o.parent);
    let own = app.session.pending_moves.get(&occurrence).copied().unwrap_or(o.transform);
    Some(solidcraft_engine::doc::apply_point(&solidcraft_engine::doc::mat_mul(&parent, &own), solidcraft_engine::geom::Vec3::ZERO))
}

/// The names the Bodies folder lists at the top level, for tests.
#[cfg(test)]
pub(crate) fn body_rows(app: &SolveApp) -> Vec<String> {
    entries(app, 0, "bodies").into_iter().map(|e| e.key).collect()
}

#[cfg(test)]
#[path = "section_browser_tests.rs"]
mod section_browser_tests;

#[cfg(test)]
#[path = "browser_card_tests.rs"]
mod card_tests;
