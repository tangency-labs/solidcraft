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
    let Some(source) = crate::open_meshing::session_world(&app.session).body(body).map(|selected| selected.definition_name().to_string()) else {
        return;
    };
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
    for g in app.session.doc.browser_groups.iter().filter(|g| g.component == component && g.folder == "bodies" && g.items.contains(&source)) {
        app.tree.collapsed.remove(&format!("{scope}/g{}", g.id));
    }
}

#[cfg(test)]
#[path = "browser_model_tests.rs"]
mod model_tests;

use crate::icons::glyphs::badge;
use std::collections::BTreeSet;

use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::FeatureKind;

use crate::context_menu::{Item, Target};
use crate::theme::Tokens;

use crate::{SolveApp, icons};

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
    /// The floating browser card is folded to its header.
    pub panel_collapsed: bool,
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
    /// The Edit Canvas / Calibrate panel.
    pub canvas_panel: Option<CanvasPanel>,
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

impl TreeState {
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
    let present: Vec<String> = crate::open_meshing::session_world(&app.session).bodies.iter().map(|b| b.name.clone()).collect();
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
    if !dialog_takes_body(app) {
        return false;
    }
    let world = crate::open_meshing::session_world(&app.session);
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
    let st = crate::open_meshing::session_world(&app.session);
    find_group(app, id)
        .filter(|g| g.folder == "bodies")
        .map(|g| g.items.iter().filter(|b| st.body(b).is_some()).cloned().collect())
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
    v.extend([
        Item::action("ui.rename", "Rename", "rename").with(json!({ "group": id })),
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
    }
    v.push(
        Item::action("ui.newGroup", if folder == "sketches" { "New Sketch Group" } else { "New Group" }, "folder")
            .with(json!({ "component": component, "folder": folder })),
    );
    if sel.len() > 1 {
        v.push(Item::action("ui.groupSelected", "Group Selected", "folder").with(json!({ "component": component, "folder": folder, "items": sel })));
    }
    v.push(Item::sep());
    v.push(Item::action("ui.folderVisible", "Show/Hide", "eye").key("V").with(json!({ "key": format!("{scope}/{folder}") })));
    v.push(Item::action("ui.showAll", "Show All", "eye"));
    v
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

/// Selected browser items of the folder of a body or sketch, when more than one (for Group
/// Selected in their menus).
pub fn selection_group(app: &SolveApp, target: &Target) -> Option<Value> {
    let (comp, folder) = match target {
        Target::Body { name } => {
            let st = crate::open_meshing::session_world(&app.session);
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
            if let Ok(r) = app.run("browser.group", json!({ "component": comp, "folder": folder, "items": items }))
                && let Some(g) = r.get("group").and_then(Value::as_u64)
            {
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

const ROW_H: f32 = crate::ui_kit::TREE_ROW_HEIGHT;
/// The floating card's inner margin: the header highlight sits this far from the card's top, left
/// and right edges, so its corners are concentric with the card's.
const CARD_INSET: f32 = 4.0;
const HEADER_H: f32 = 26.0;
const INDENT: f32 = 14.0;
/// Opacity of a dimmed row's icon (a hidden body, sketch, origin item, …).
const DIM_ICON_OPACITY: f32 = 0.4;

fn row_rect(ui: &egui::Ui) -> Rect {
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW_H))
}

/// One row's look.
#[derive(Default)]
pub(crate) struct Row<'a> {
    pub(crate) depth: usize,
    /// Some(open) for rows that fold.
    pub(crate) fold: Option<bool>,
    /// Some(visible) for rows with an eye.
    pub(crate) eye: Option<bool>,
    pub(crate) icon: &'a str,
    pub(crate) label: &'a str,
    /// The label is a name someone gave (a body, sketch, component, …): it is shown as typed,
    /// not translated when it happens to match a UI label.
    pub(crate) name: bool,
    pub(crate) selected: bool,
    pub(crate) dim: bool,
    pub(crate) color: Option<Color32>,
    /// Component rows: is it the active one?
    pub(crate) radio: Option<bool>,
    /// Trailing badges: "lock", "pin".
    pub(crate) badges: &'a [&'a str],
    /// A button at the row's right end: (icon, tooltip label), e.g. a feature's profile sketch.
    pub(crate) action: Option<(&'a str, &'a str)>,
    /// Lit by the selection or the 3D view's pointer: the item's own row, or a row leading to it.
    pub(crate) mark: Mark,
}

/// How a row relates to what the selection and the 3D view's pointer light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mark {
    #[default]
    None,
    /// A row a lit item is inside (its component, placement, folder or group): a faint bar.
    Branch,
    /// A lit item's own row: a solid bar, and the hover colour unless it is selected.
    Here,
}

/// What happened on a row.
#[derive(Default)]
pub(crate) struct RowResp {
    pub(crate) rect: Option<Rect>,
    pub(crate) clicked: bool,
    pub(crate) double: bool,
    pub(crate) secondary: Option<Pos2>,
    pub(crate) fold: bool,
    pub(crate) eye: bool,
    pub(crate) radio: bool,
    /// The row's [`Row::action`] button was clicked.
    pub(crate) action: bool,
    pub(crate) hovered: bool,
    /// The row's response (drag and drop).
    pub(crate) resp: Option<egui::Response>,
    /// The press began on the fold arrow, eye or radio (not a drag of the row).
    pub(crate) on_control: bool,
}

pub(crate) fn draw_row(ui: &mut egui::Ui, id: egui::Id, row: &Row) -> RowResp {
    let t = Tokens::get();
    // Rows scrolled out of view only take their space: no widgets, no painting.
    let r = row_rect(ui);
    if !ui.is_rect_visible(r) {
        ui.allocate_space(r.size());
        return RowResp { rect: Some(r), ..Default::default() };
    }
    // A stable id per row (not the layout position), so a press and its release stay on the
    // same row while rows above open or close.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = ui.interact(r, id, Sense::click_and_drag());
    // While something is dragged (an appearance swatch, drawn under the pointer) `hovered` is
    // off; the row under the pointer still counts, so a drop lands on it.
    let dragged_over = egui::DragAndDrop::has_any_payload(ui.ctx()) && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p));
    let mut out = RowResp { rect: Some(r), hovered: resp.hovered() || dragged_over, ..Default::default() };
    if row.selected {
        ui.painter().rect_filled(r, t.radius_sm.saturating_sub(1), crate::theme::TabAccent::current(ui.ctx()).soft);
    } else if resp.hovered() || row.mark == Mark::Here {
        ui.painter().rect_filled(r, t.radius_sm.saturating_sub(1), t.hover);
    }
    // A lit item (selected, or under the pointer in the 3D view): a bar down the left of its row,
    // and a fainter, thinner one on each row leading to it, so the bars trace its branch.
    let bar = match row.mark {
        Mark::None => None,
        Mark::Branch => Some((2.0, t.accent.gamma_multiply(0.4))),
        Mark::Here => Some((3.0, t.accent)),
    };
    if let Some((w, colour)) = bar {
        ui.painter().rect_filled(Rect::from_min_size(r.left_top() + vec2(0.0, 2.0), vec2(w, r.height() - 4.0)), t.radius_sm / 4, colour);
    }
    let mut x = r.left() + 2.0 + row.depth as f32 * INDENT;
    let cy = r.center().y;
    // The row is one widget: where a click lands decides what it does (the fold arrow with
    // padding, the eye, the radio, or the row itself), so a click on the arrow never also
    // selects the row or starts a drag.
    let fold_r = Rect::from_min_max(pos2(x - 4.0, r.top()), pos2(x + 15.0, r.bottom()));
    let eye_r = Rect::from_center_size(pos2(x + 14.0 + 8.0, cy), vec2(19.0, r.height()));
    let radio_r = Rect::from_center_size(pos2(x + 32.0 + 6.0, cy), vec2(15.0, r.height()));
    // The label follows the fold arrow, eye, radio and icon (drawn below in that order). It is
    // laid out first, so the action button can sit right after the name and read as the row's.
    let label_x = x + 14.0 + 18.0 + if row.radio.is_some() { 15.0 } else { 0.0 } + if row.icon.is_empty() { 0.0 } else { 21.0 };
    let col =
        if row.selected { crate::theme::TabAccent::current(ui.ctx()).ink } else { row.color.unwrap_or(if row.dim { t.text_dim } else { t.text }) };
    let label_display = if row.name { crate::i18n::tr_name(row.label) } else { crate::i18n::tr_dyn(row.label) };
    let font = if row.selected { crate::fonts::semibold(t.font_label) } else { FontId::proportional(t.font_label) };
    // The label stops before the action, the badges and the card's edge; a longer name ends in
    // an ellipsis and its full text is in the tooltip.
    let badges_width = row.badges.len() as f32 * 15.0;
    let action_width = if row.action.is_some() { 22.0 } else { 0.0 };
    let room = (r.right() - 4.0 - badges_width - action_width - label_x).max(0.0);
    let mut job = egui::text::LayoutJob::simple(label_display.to_string(), font, col, room);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = ui.painter().layout_job(job);
    let lw = galley.size().x;
    let elided = galley.elided;
    let action_r = Rect::from_min_size(pos2(label_x + lw + 2.0, r.top()), vec2(20.0, r.height()));
    let at = |p: Pos2| -> u8 {
        if row.action.is_some() && action_r.contains(p) {
            4
        } else if row.fold.is_some() && fold_r.contains(p) {
            1
        } else if row.eye.is_some() && eye_r.contains(p) {
            2
        } else if row.radio.is_some() && radio_r.contains(p) {
            3
        } else {
            0
        }
    };
    DRAWN.with(|d| d.borrow_mut().push((row.label.to_string(), r, row.fold.map(|_| fold_r), row.fold)));
    let press_on = ui.input(|i| i.pointer.press_origin()).map_or(0, at);
    let click_on = resp.interact_pointer_pos().map_or(0, at);
    let hover_on = resp.hover_pos().map_or(0, at);
    // Fold arrow.
    if let Some(open) = row.fold {
        crate::ui_kit::disclosure(ui.painter(), pos2(x + 6.0, cy), open, if hover_on == 1 { t.text } else { t.text_dim });
        out.fold = resp.clicked() && click_on == 1;
        crate::scenario::publish_handle(&format!("fold:{}", row.label), fold_r.center());
    }
    x += 14.0;
    // Eye.
    if let Some(vis) = row.eye {
        let er = Rect::from_center_size(pos2(x + 8.0, cy), vec2(15.0, 15.0));
        // Hidden or faded rows keep main's dimming, with the rounded suppression asset.
        icons::paint_icon(ui.painter(), er, if vis { "eye" } else { "suppress" }, if vis && !row.dim { 0.85 } else { DIM_ICON_OPACITY });
        out.eye = resp.clicked() && click_on == 2;
        crate::scenario::publish_handle(&format!("eye:{}", row.label), er.center());
    }
    x += 18.0;
    // Activation radio (components).
    if let Some(active) = row.radio {
        let rc = pos2(x + 6.0, cy);
        ui.painter().circle_stroke(rc, 5.0, Stroke::new(1.2, if active { t.accent } else { t.text_dim }));
        if active {
            ui.painter().circle_filled(rc, 2.8, t.accent);
        }
        out.radio = resp.clicked() && click_on == 3;
        x += 15.0;
    }
    if !row.icon.is_empty() {
        let ir = Rect::from_min_size(pos2(x, cy - 8.0), vec2(16.0, 16.0));
        // Icons are full-colour images (a tint colour only sets their opacity): a hidden or
        // faded row draws its icon faint, like its label.
        icons::paint_icon(ui.painter(), ir, row.icon, if row.dim { DIM_ICON_OPACITY } else { 1.0 });
        x += 21.0;
    }
    crate::scenario::publish_rect(&format!("row_label:{}", row.label), Rect::from_min_size(pos2(x, cy - galley.size().y / 2.0), galley.size()));
    ui.painter().with_clip_rect(ui.painter().clip_rect().intersect(r)).galley(pos2(x, cy - galley.size().y / 2.0), galley, col);
    let mut bx = (x + lw + 6.0 + action_width).min(r.right() - 16.0);
    for b in row.badges {
        let br = Rect::from_center_size(pos2(bx + 6.0, cy), vec2(12.0, 12.0));
        badge(ui.painter(), br, b, t.text_dim);
        bx += 15.0;
    }
    if let Some((icon, _)) = row.action {
        if hover_on == 4 {
            ui.painter().rect_filled(action_r.shrink2(vec2(1.0, 3.0)), t.radius_sm.saturating_sub(1), t.hover);
        }
        let ir = Rect::from_center_size(action_r.center(), vec2(14.0, 14.0));
        icons::paint_icon(ui.painter(), ir, icon, if hover_on == 4 { 1.0 } else { 0.6 });
        out.action = resp.clicked() && click_on == 4;
        crate::scenario::publish_handle(&format!("action:{}", row.label), action_r.center());
    }
    crate::scenario::publish_handle(&format!("row:{}", row.label), r.center());
    let tip = match hover_on {
        1 => Some(if row.fold == Some(true) { crate::i18n::tr("ui.collapse") } else { crate::i18n::tr("ui.expand") }),
        2 => Some(if row.eye == Some(true) { crate::i18n::tr("ui.hide") } else { crate::i18n::tr("ui.show") }),
        3 => Some(if row.radio == Some(true) { crate::i18n::tr("ui.active_component") } else { crate::i18n::tr("ui.activate") }),
        4 => row.action.map(|(_, tip)| crate::i18n::tr_label(tip)),
        _ => None,
    };
    let resp = match tip {
        Some(t) => resp.on_hover_text(t),
        None if elided => resp.on_hover_text(label_display.to_string()),
        None => resp,
    };
    out.on_control = press_on != 0;
    out.clicked = resp.clicked() && click_on == 0;
    out.double = resp.double_clicked() && click_on == 0;
    out.resp = Some(resp.clone());
    if resp.secondary_clicked() {
        out.secondary = resp.interact_pointer_pos().or(Some(r.left_bottom()));
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

/// Whether the body being revealed is in this placement or one below it: it opens.
fn reveals_in_occurrence(app: &SolveApp, path: &[u64]) -> bool {
    app.tree.reveal.as_ref().is_some_and(|r| app.session.occurrence_bodies(path, true).is_ok_and(|bodies| bodies.contains(r)))
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
            selected.insert(match s {
                Sel::Body { name } | Sel::Face { body: name, .. } | Sel::Edge { body: name, .. } | Sel::Vertex { body: name, .. } => {
                    Thing::Body(name.clone())
                }
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
            });
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

/// One content-hugging browser; the shared container supplies floating or flush compact chrome.
pub fn browser(app: &mut SolveApp, ui: &mut egui::Ui) {
    let available = ui.available_rect_before_wrap();
    let t = Tokens::get();
    std::mem::swap(&mut app.tree.order_prev, &mut app.tree.order);
    app.tree.order_len = 0;
    DRAWN.with(|d| d.borrow_mut().clear());
    OCCURRENCES.with(|d| d.borrow_mut().clear());
    let mut acts = Actions { hover_bodies: std::mem::take(&mut app.viewport.hover_bodies), ..Default::default() };
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
    let collapsed = vertical && !stacked && app.preferences.timeline_left_browser_collapsed;
    let bounds = if stacked {
        crate::timeline_left::stacked_rects(app, available).0
    } else {
        panel_bounds(available, if vertical { None } else { app.shell.history }, if collapsed { t.space_24 + t.space_10 } else { width }, gap)
    };
    app.shell.browser = None;
    let min_height = HEADER_H + 2.0 * (CARD_INSET + 1.0);
    if bounds.is_positive() && bounds.height() >= min_height {
        let fill = if container.floating() && app.preferences.browser_float { t.panel.gamma_multiply(BROWSER_FLOAT_ALPHA) } else { t.panel };
        let frame = container.frame(ui.ctx(), fill, crate::frame::Side::Left).inner_margin(CARD_INSET as i8);
        let inner_width = (bounds.width() - frame.total_margin().sum().x).max(0.0);
        let inner_height = (bounds.height() - frame.total_margin().sum().y).max(0.0);
        let card = Container::new(ui.ctx(), Surface::Panel)
            .area(egui::Id::new("sc_browser_card"))
            .fixed_pos(bounds.min)
            .order(egui::Order::Middle)
            .show(ui.ctx(), |ui| {
                let limits = Rect::from_min_max(available.min, pos2(available.right(), bounds.bottom()));
                let response = container.show_frame(ui, frame, limits, |ui| {
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
                });
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
    browser_header(app, ui, collapsible);
    let stacked = crate::timeline_left::stacked(app);
    if collapsible
        && (if stacked {
            (app.preferences.timeline_left_browser_collapsed && app.shell.history_folding.is_none_or(|p| p[0] >= 1.0))
                || ui.available_height() <= t.space_3
        } else {
            app.tree.panel_collapsed && !(app.ui.show_timeline && app.preferences.timeline_left)
        })
    {
        return;
    }
    if crate::render_workspace::browser_mode(app, ui) {
        return;
    }
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top() + 1.0, t.toolbar.lerp_to_gamma(t.border, 0.5));
    ui.add_space(t.space_3);
    if collapsible && !stacked {
        browser_rows_hugging(app, ui, acts);
    } else {
        browser_rows(app, ui, acts);
    }
}

/// The header: the title, the body count and (on the floating card) a chevron that folds the
/// card to just this row. The whole row is the button.
fn browser_header(app: &mut SolveApp, ui: &mut egui::Ui, collapsible: bool) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let response = ui.interact(rect, ui.id().with("browser_header"), if collapsible { Sense::click() } else { Sense::hover() });
    crate::scenario::publish_target("browser:header", rect);
    let vertical = app.ui.show_timeline && app.preferences.timeline_left;
    let stacked = crate::timeline_left::stacked(app);
    let collapsed = collapsible && if stacked { app.preferences.timeline_left_browser_collapsed } else { !vertical && app.tree.panel_collapsed };
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
                app.tree.panel_collapsed = !app.tree.panel_collapsed;
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
            component_rows(app, ui, 0, 0, acts, &mut cache);
        });
    // The row was reached and scrolled to, or isn't in the tree (a body a later feature removes):
    // either way the reveal is done.
    app.tree.reveal = None;
    app.tree.cache = Some(cache);
}

fn finish_browser(app: &mut SolveApp, ui: &mut egui::Ui, mut acts: Actions) {
    app.tree.order.truncate(app.tree.order_len);
    if !acts.hovered {
        acts.hover_bodies.clear();
    }
    app.viewport.hover_bodies = acts.hover_bodies;
    if let Some(d) = acts.drop {
        apply_drop(app, d);
    }
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
    crate::browser_prompts::occurrence_panel(app, ui);
    crate::browser_prompts::capture_prompt(app, ui.ctx());
    crate::browser_canvas::panel(app, ui.ctx());
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
    let key = &root.key;
    if let Some(body) = app.tree.reveal.as_ref()
        && (cache.root_bodies.contains(body) || root.children.iter().any(|path| reveals_in_occurrence(app, path)))
    {
        app.tree.collapsed.remove(key);
    }
    let open = is_open(app, key, true);
    let active = app.session.active_component == id;
    let bodies = &cache.root_bodies;
    let visible = id == 0 || bodies.is_empty() || bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b));
    let faded = app.session.active_component != 0 && !app.session.doc.component_within(id, app.session.active_component) && !active;
    let name = &root.name;
    let badges: &[&str] = if root.grounded { &["pin"] } else { &[] };
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
            let _ = app.run("component.activate", json!({ "component": id }));
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
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp != id && d.folder != "construction", DropMark::Box)
    {
        acts.drop = Some(DropAction::Component { to: id, folder: d.folder, keys: d.keys.clone() });
    }
    if !open {
        return;
    }
    let d = depth + 1;
    if id == 0 {
        doc_settings(app, ui, d);
        named_view_rows(app, ui, d, acts);
        origin_rows(app, ui, d, acts);
        canvas_rows(app, ui, d, acts);
        analysis_rows(app, ui, d, acts);
    }
    for (slot, folder) in root.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        let data = slot.get_or_insert_with(|| cache.folder(app, id, None, folder));
        folder_rows_at(app, ui, id, None, folder, d, acts, data);
    }
    features::rows(app, ui, id, &[], d, acts);
    if id == 0 {
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
    if reveals_in_occurrence(app, path) {
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
    let badges: &[&str] = if placement.grounded { &["pin"] } else { &[] };
    app.tree.remember(key);
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row {
            depth,
            fold: Some(open),
            eye: Some(own_visible),
            icon: "component",
            label: &placement.label,
            name: true,
            selected,
            // A cut component is faded until Paste moves it.
            dim: faded || !visible || app.cut_placements.iter().any(|p| p.as_slice() == path),
            radio: Some(active),
            badges,
            mark: acts.lit.placement(&app.session.doc, path, placement.component),
            ..Default::default()
        },
    );
    if let Some(rect) = r.rect.filter(|_| r.resp.is_some()) {
        OCCURRENCES.with(|d| d.borrow_mut().push((path.to_vec(), placement.name.clone(), rect, open)));
        crate::scenario::publish_handle(&format!("row:occurrence:{}", key.trim_start_matches("o:")), pos2(rect.right() - 12.0, rect.center().y));
        let x = rect.left() + 2.0 + depth as f32 * INDENT;
        for (control, offset) in [("fold", 6.0), ("eye", 22.0), ("activate", 38.0)] {
            crate::scenario::publish_handle(&format!("{control}:occurrence:{}", key.trim_start_matches("o:")), pos2(x + offset, rect.center().y));
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
    }
    if r.fold {
        toggle(app, key, true);
    }
    if r.eye {
        let _ = app.run("browser.visibility", json!({"visible": app.session.occurrence_hidden(path), "paths": [path]}));
    }
    if r.radio || (r.double && !app.session.doc.parts.contains_key(&placement.component)) {
        let _ = app.run("component.activate", json!({"component": placement.component, "occurrence": placement.occurrence, "path": path}));
    }
    if r.double && app.session.doc.parts.contains_key(&placement.component) {
        crate::dialogs_part_size::open(app, placement.component);
    }
    if r.clicked && !r.double {
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
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp != placement.component && d.folder != "construction", DropMark::Box)
    {
        acts.drop = Some(DropAction::Component { to: placement.component, folder: d.folder, keys: d.keys.clone() });
    }
    if !open {
        return;
    }
    for (slot, folder) in placement.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        let data = slot.get_or_insert_with(|| cache.folder(app, placement.component, Some(path), folder));
        folder_rows_at(app, ui, placement.component, Some(path), folder, depth + 1, acts, data);
    }
    features::rows(app, ui, placement.component, path, depth + 1, acts);
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

fn origin_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize, acts: &Actions) {
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
        let label = k;
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
            // Faded when hidden itself or with the whole Origin.
            &Row { depth: depth + 1, eye: Some(visible), icon, label, selected, dim: !visible || !app.origin_shown(), mark, ..Default::default() },
        );
        if r.eye {
            // Through the command, so the Origin row's eye is an undo step (#2219).
            let _ = app.run("browser.visibility", json!({ "items": [k], "visible": !visible }));
        }
        if let Some(p) = r.secondary
            && icon == "plane"
        {
            crate::context_menu::open_for(app, p, Target::OriginPlane { name: k.into() });
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

/// The menu of a canvas.
pub fn canvas_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = app.session.doc.canvases.iter().find(|c| c.id == id).is_some_and(|c| c.visible);
    vec![
        Item::action("ui.editCanvas", "Edit Canvas", "canvas").with(json!({ "canvas": id })),
        Item::action("ui.calibrateCanvas", "Calibrate", "measure").with(json!({ "canvas": id })),
        Item::action("canvas.to_sketch", "Canvas to Sketch", "canvas").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.delete", "Delete", "delete").with(json!({ "canvases": [id] })),
        Item::action("ui.rename", "Rename", "rename").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.canvasVisible", "Show/Hide", "eye").key("V").with(json!({ "canvas": id, "visible": !visible })),
    ]
}

/// Editing a canvas (position, size, angle, opacity, flip) or calibrating it (two points on
/// the image and their true distance). Live edits of one opening are one undo step.
#[derive(Clone, Debug)]
pub struct CanvasPanel {
    pub canvas: u64,
    pub calibrate: bool,
    /// Undo depth when the panel opened.
    pub(crate) depth: usize,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub distance: f64,
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
        CanvasPanel { canvas, calibrate, depth: app.session.undo.len(), a, b, distance: d }
    }
}

/// Does the open dialog's active input take bodies (Combine's target and tools, #4452;
/// Move/Copy's bodies, #4771)? Then a body clicked in the Browser goes there, as a click
/// on it in the view does, rather than replacing the selection: overlapping bodies are
/// often only told apart in the Browser.
fn dialog_takes_body(app: &SolveApp) -> bool {
    app.dialog.as_ref().and_then(|d| d.active_input()).is_some_and(|i| i.accept & crate::selection::BODIES != 0)
}

/// Browser picks go only into compatible active dialog inputs. Outside a dialog, planes and
/// axes become the selection; body rows use the ordinary Ctrl/Shift-aware selection path.
fn pick_from_browser(app: &mut SolveApp, x: Sel) {
    if let Sel::Body { .. } = &x
        && dialog_takes_body(app)
        && let Some(mut d) = app.dialog.take()
    {
        d.pick(&app.session, x);
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
    visible: bool,
    selected: bool,
    color: Option<Color32>,
    locked: bool,
    sel: Option<Sel>,
    target: Option<Target>,
    /// Bodies lit when the row is hovered.
    bodies: Vec<String>,
}

impl Entry {
    fn body(app: &SolveApp, b: &solidcraft_engine::doc::ModelBody, world: String) -> Self {
        Self {
            thing: Thing::Body(world.clone()),
            key: b.name.clone(),
            row_key: format!("b:{world}"),
            patterns: None,
            // A part the modelling kernel refuses says so (Repair Part is in its menu).
            label: if b.body.smlib_refused().is_some() { crate::i18n::format("browser.needs_repair", &[("name", &b.name)]) } else { b.name.clone() },
            // An open shell is a surface body, not a solid.
            icon: if b.is_surface() { "surface" } else { "body" },
            visible: !app.session.visibility.hidden_bodies.contains(&world),
            selected: app.session.selection.iter().any(|s| matches!(s, Sel::Body { name } if name == &world)),
            color: b.body.smlib_refused().map(|_| Tokens::get().warning),
            locked: app.session.visibility.locked_bodies.contains(&world),
            sel: Some(Sel::Body { name: world.clone() }),
            target: Some(Target::Body { name: world.clone() }),
            bodies: vec![world],
        }
    }

    fn sketch(app: &SolveApp, f: &solidcraft_engine::doc::Feature) -> Self {
        let t = Tokens::get();
        let active = app.session.active_sketch == Some(f.id);
        let lost = app.session.model.result(f.id).is_some_and(|result| result.error.is_some())
            && matches!(app.session.doc.sketch_face_attachment(f.id, &app.session.model), solidcraft_engine::doc::FaceAttachmentStatus::Lost { .. });
        Self {
            key: f.id.to_string(),
            row_key: format!("s:{}", f.id),
            thing: Thing::Sketch(f.id),
            patterns: crate::sketch_pattern_tree::has_patterns(app, f.id).then(|| format!("s:{}/patterns", f.id)),
            label: if lost {
                format!("{}  (attachment lost)", f.name)
            } else if active {
                format!("{}  (editing)", f.name)
            } else {
                f.name.clone()
            },
            icon: "sketch",
            visible: sketch_visible(app, f.id),
            selected: active || app.session.selection.iter().any(|s| matches!(s, Sel::Feature { id } if *id == f.id)),
            color: if lost { Some(t.error) } else { active.then_some(t.sketch_accent) },
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
            visible: !app.session.visibility.hidden_origin.contains(&f.name),
            selected: app.session.selection.contains(&sel),
            color: None,
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

/// A folder's groups (with their items) and the items in no group, in browser order.
fn arrange<'a>(
    doc: &'a solidcraft_engine::doc::Document,
    comp: u64,
    folder: &str,
    all: Vec<Entry>,
) -> (Vec<(&'a solidcraft_engine::doc::BrowserGroup, Vec<Entry>)>, Vec<Entry>) {
    let mut rest: Vec<Option<Entry>> = all.into_iter().map(Some).collect();
    let mut take = |key: &str| rest.iter_mut().find(|e| e.as_ref().is_some_and(|e| e.key == key)).and_then(Option::take);
    let grouped: Vec<_> = doc
        .browser_groups
        .iter()
        .filter(|g| g.component == comp && g.folder == folder)
        .map(|g| {
            let items = g.items.iter().filter_map(|k| take(k)).collect();
            (g, items)
        })
        .collect();
    let mut loose: Vec<Entry> = rest.into_iter().flatten().collect();
    if let Some(order) = doc.browser_order.get(&format!("{comp}/{folder}")) {
        let mut positions = std::collections::HashMap::with_capacity(order.len());
        for (i, key) in order.iter().enumerate() {
            positions.entry(key.as_str()).or_insert(i);
        }
        let pos = |k: &str| positions.get(k).copied().unwrap_or(usize::MAX);
        loose.sort_by_key(|e| pos(&e.key));
    }
    (grouped, loose)
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
        for g in app.session.doc.browser_groups.iter().filter(|g| g.items.contains(&e.key)) {
            app.tree.collapsed.remove(&format!("g{}", g.id));
        }
    }
    let open = is_open(app, key, true);
    // The folder's eye is its own: hiding it hides its items without changing theirs, and its
    // items never change it. It and everything in it fade while it or a placement above is
    // hidden, whatever their own eyes say.
    let shown = !app.session.folder_hidden(key);
    let folder_faded = !shown || path.is_some_and(|p| !app.session.occurrence_visible(p));
    let r = draw_row(
        ui,
        ui.id().with(key),
        &Row {
            depth,
            fold: Some(open),
            eye: Some(shown),
            icon: "folder",
            label: folder_label(folder),
            dim: folder_faded,
            mark: acts.lit.branch(app, data.entries()),
            ..Default::default()
        },
    );
    if r.fold {
        toggle(app, key, true);
    } else if r.clicked {
        // The Bodies label is the whole set while a command is asking for bodies. The arrow
        // still folds it. With no such command, the label folds, as it always has.
        let names: Vec<String> = data.entries().flat_map(|e| e.bodies.clone()).collect();
        if !offer_bodies(app, &names, Take::Toggle) {
            toggle(app, key, true);
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
        && let Some(d) = drop_target(ui, resp, |d: &Drag| d.folder == fstatic && (d.comp == comp || d.folder != "construction"), DropMark::Box)
    {
        acts.drop = Some(if d.comp == comp {
            DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), group: None, before: None }
        } else {
            DropAction::Component { to: comp, folder: d.folder, keys: d.keys.clone() }
        });
    }
    if !open {
        return;
    }
    let loose_keys = &data.loose_keys;
    for group in &data.groups {
        let (id, items, gkey) = (group.id, &group.items, &group.key);
        let gopen = is_open(app, gkey, true);
        let gshown = !app.session.folder_hidden(gkey);
        let group_faded = folder_faded || !gshown;
        let r = draw_row(
            ui,
            ui.id().with(gkey),
            &Row {
                depth: depth + 1,
                fold: Some(gopen),
                eye: Some(gshown),
                icon: "folder",
                label: &group.label,
                name: true,
                dim: group_faded,
                mark: acts.lit.branch(app, items),
                ..Default::default()
            },
        );
        if r.fold {
            toggle(app, gkey, true);
        } else if r.clicked {
            let names: Vec<String> = items.iter().flat_map(|e| e.bodies.clone()).collect();
            if !offer_bodies(app, &names, Take::Toggle) {
                toggle(app, gkey, true);
            }
        }
        if r.eye {
            toggle_folder(app, gkey);
        }
        if r.hovered {
            acts.hover(items.iter().flat_map(|e| &e.bodies));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Group { id, scope: prefix.to_owned() }));
        }
        if let Some(resp) = &r.resp
            && let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == fstatic, DropMark::Box)
        {
            acts.drop = Some(DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), group: Some(id), before: None });
        }
        if gopen {
            for e in items {
                entry_row(app, ui, comp, path, fstatic, e, depth + 2, Some(id), loose_keys, group_faded, acts);
            }
        }
    }
    for e in &data.loose {
        entry_row(app, ui, comp, path, fstatic, e, depth + 1, None, loose_keys, folder_faded, acts);
    }
}

/// The body's name edited in its row (started by a double-click): Enter or leaving the box keeps
/// the name, Escape cancels.
fn inline_rename(app: &mut SolveApp, ui: &mut egui::Ui, row: Rect, e: &Entry, depth: usize) {
    let editing = app.menu.rename.as_ref().is_some_and(|r| r.inline && matches!(&r.what, crate::context_menu::RenameWhat::Body(b) if *b == e.key));
    if !editing {
        return;
    }
    let Some(mut rename) = app.menu.rename.take() else { return };
    // Where the label starts: the fold arrow, eye and icon come first.
    let left = row.left() + 2.0 + depth as f32 * INDENT + 14.0 + 18.0 + 21.0;
    let edit_rect = Rect::from_min_max(pos2(left, row.top() + 1.0), pos2(row.right() - 3.0, row.bottom() - 1.0));
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
    loose: &[String],
    // A placement or folder above is hidden: the row fades, its eye stays its own.
    parent_hidden: bool,
    acts: &mut Actions,
) {
    app.tree.remember(&e.row_key);
    let pattern_key = e.patterns.as_deref();
    let patterns_open = pattern_key.is_some_and(|key| is_open(app, key, true));
    let rect = row_rect(ui);
    if !ui.is_rect_visible(rect) && !patterns_open && !(folder == "bodies" && app.tree.reveal.as_deref() == Some(e.key.as_str())) {
        ui.allocate_space(rect.size());
        return;
    }
    let (selected, mark) = lit_row(acts.lit.entry(app, e));
    let visible = e.is_visible(app);
    let locked = match &e.sel {
        Some(Sel::Body { name }) => app.session.visibility.locked_bodies.contains(name),
        _ => e.locked,
    };
    let badges: &[&str] = if locked { &["lock"] } else { &[] };
    let r = draw_row(
        ui,
        ui.id().with(("item", &e.row_key)),
        &Row {
            depth,
            fold: pattern_key.map(|_| patterns_open),
            eye: Some(visible),
            icon: e.icon,
            label: &e.label,
            name: true,
            selected,
            // A cut body is faded until Paste moves it.
            dim: !e.visible || parent_hidden || (folder == "bodies" && app.cut_bodies.contains(&e.key)),
            color: e.color,
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
            if let Some(path) = path {
                let _ = app.run("component.activate", json!({"component": comp,"occurrence":path.last(),"path":path}));
            }
            app.edit_sketch(id);
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
        && let Some(rect) = r.rect
    {
        inline_rename(app, ui, rect, e, depth);
    }
    if r.hovered && !e.bodies.is_empty() {
        acts.hover(e.bodies.iter());
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
        resp.dnd_set_drag_payload(Drag { comp, folder, keys });
    }
    if let Some(d) = drop_target(ui, resp, |d: &Drag| d.comp == comp && d.folder == folder && !d.keys.contains(&e.key), DropMark::Above) {
        acts.drop = Some(match group {
            Some(g) => DropAction::Move { comp, folder, keys: d.keys.clone(), group: Some(g), before: Some(e.key.clone()) },
            None => {
                let mut order: Vec<String> = loose.iter().filter(|k| !d.keys.contains(k)).cloned().collect();
                let at = order.iter().position(|k| *k == e.key).unwrap_or(order.len());
                for (i, k) in d.keys.iter().enumerate() {
                    order.insert(at + i, k.clone());
                }
                DropAction::Order { comp, folder, keys: d.keys.clone(), order }
            }
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

/// A drop, applied after the tree is drawn.
enum DropAction {
    /// Into a group (before an item of it), or out of groups.
    Move { comp: u64, folder: &'static str, keys: Vec<String>, group: Option<u64>, before: Option<String> },
    /// Out of groups, in this order of the folder's loose items.
    Order { comp: u64, folder: &'static str, keys: Vec<String>, order: Vec<String> },
    /// Into another component (bodies and sketches).
    Component { to: u64, folder: &'static str, keys: Vec<String> },
}

fn apply_drop(app: &mut SolveApp, d: DropAction) {
    match d {
        DropAction::Move { comp, folder, keys, group, before } => {
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": keys, "group": group, "before": before }));
        }
        DropAction::Order { comp, folder, keys, order } => {
            let _ = app.run("browser.move", json!({ "component": comp, "folder": folder, "items": keys }));
            let _ = app.run("browser.order", json!({ "component": comp, "folder": folder, "order": order }));
        }
        DropAction::Component { to, folder, keys } => {
            let _ = match folder {
                "bodies" => app.run("component.move_bodies", json!({ "bodies": keys, "component": to })),
                "sketches" => app.run("component.move_sketches", json!({ "sketches": keys, "component": to })),
                _ => Ok(serde_json::Value::Null),
            };
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
    let st = crate::open_meshing::session_world(&app.session);
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
            "component.capture_position" | "component.revert_position" | "occurrence.move" | "occurrence.drag" | "edit.undo" | "edit.redo"
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
