//! The browser (left): the design as a tree of components, each with its bodies, sketches and
//! construction planes, plus document settings and the origin.
//!
//! Rows are drawn in code (icons from [`icons`]) and act through commands. A click selects
//! (Ctrl adds, Shift extends over the visible rows), a slower second click on a name starts
//! renaming it, hovering a body row lights it up in the viewport, a double-click on a sketch
//! edits it, and a right-click opens the item's menu ([`crate::context_menu`]).

use crate::ui_kit::BackingTooltipExt as _;
use solidcraft_engine::geom::prelude::*;

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

/// Find for a feature shows the whole tree, so it ends a search (#1413).
pub(crate) fn reveal_feature(app: &mut SolveApp, id: u64, body: &str) {
    app.tree.search.clear();
    features::reveal(app, id, body);
}

/// Whether folders start open: unless Preferences › Layout › Browser rows start closed is on,
/// when they, and placements whatever "Components start open" says ([`placement_open`]), open
/// with their arrow, Find in Browser or Expand All (#8768). The design's own row always starts
/// open.
pub(crate) fn rows_start_open(app: &SolveApp) -> bool {
    !app.preferences.browser_rows_start_closed
}

/// Whether the Browser follows the 3D view: a body picked there opens the Browser down to its row
/// (#3005), and the rows above what is selected or under the pointer are marked (#3009). It does
/// while the rows start open; while they start closed, only Find in Browser opens them.
fn follows_view(app: &SolveApp) -> bool {
    rows_start_open(app)
}

/// Reveal a production panel and its tab so an explicit Find can reach the real rows.
pub(crate) fn reveal_shared_panel(app: &mut SolveApp, name: &str) {
    use solidcraft_engine::panels::{LayoutAction, Mutation};

    let Some(layout) = app.panel_runtime().map(|runtime| runtime.layout().layout()) else { return };
    let Some(panel) = layout.instances.iter().find(|panel| panel.id.0 == name) else { return };
    let id = panel.id.clone();
    if !panel.requested_open {
        let mutation = Mutation::Open { kind: panel.kind.clone(), instance: id.clone(), destination: None };
        if app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: None, mutations: vec![mutation] }).is_err() {
            return;
        }
    }
    let Some(section) = app.panel_runtime().and_then(|runtime| runtime.layout().layout().containing(&id).ok()) else { return };
    let mut mutations = Vec::new();
    if section.collapsed {
        mutations.push(Mutation::Collapse { section: section.id.clone(), collapsed: false });
    }
    if section.active != id {
        mutations.push(Mutation::Activate { instance: id });
    }
    if !mutations.is_empty() {
        let _ = app.dispatch_panel_layout(LayoutAction::Apply { expected_revision: None, mutations });
    }
}

/// Find in Browser for a body: show the Browser and find the body there ([`find_in_tree`]). It
/// shows the whole tree, so it ends a search; a pick in the view keeps one ([`reveal_pick`], #1413).
pub(crate) fn reveal_body(app: &mut SolveApp, body: &str) {
    sync_reveals(app);
    app.tree.search.clear();
    app.ui.show_browser = true;
    reveal_shared_panel(app, "browser");
    find_in_tree(app, body);
}

/// What the last Find in Browser found.
struct Found {
    /// The body, by its name in the world.
    body: String,
    /// The placement it is in (empty for a body of the design itself).
    path: Vec<u64>,
    /// The selection the find left: the path lights the rows above the body until it changes.
    selection: Vec<Sel>,
}

/// Open the rows that hold a body and no others: the document, the placements down to it, its
/// Bodies folder and any group around it. While the rows start closed, the rows the last find
/// opened close again, unless they were opened or closed by hand since or are on this path. The
/// body's row is scrolled into view, and the path lights the rows leading to it. The path is
/// looked up once, here: the rows are opened before drawing, because a closed row never draws
/// what is inside it.
fn find_in_tree(app: &mut SolveApp, body: &str) {
    app.tree.reveal_feature = None;
    app.tree.reveal = Some(body.to_string());
    remember_reveal_source(app, body);
    let Some((component, source, path)) = body_placement(app, body) else { return };
    let start_open = rows_start_open(app);
    // A body of the design itself is in no placement: its folder is the design's (`c0`).
    let scope = if path.is_empty() { format!("c{component}") } else { occurrence_key(&path) };
    let mut rows = vec![("c0".to_string(), true)];
    rows.extend((1..=path.len()).map(|depth| (occurrence_key(&path[..depth]), start_open)));
    rows.push((format!("{scope}/bodies"), start_open));
    rows.extend(group_chain(app, component, "bodies", &source).into_iter().map(|id| (format!("{scope}/g{id}"), start_open)));
    // Rows that start open stay open after a find, as they always have: only rows that start
    // closed close again at the next one.
    if start_open {
        app.tree.found_rows.clear();
    }
    let mut opened = Vec::new();
    for (key, default) in std::mem::take(&mut app.tree.found_rows) {
        if rows.iter().any(|(row, _)| *row == key) {
            opened.push((key, default));
        } else {
            set_open(app, &key, default, false);
        }
    }
    for (key, default) in rows {
        if set_open(app, &key, default, true) && !start_open {
            opened.push((key, default));
        }
    }
    app.tree.found_rows = opened;
    app.tree.found = Some(Found { body: body.to_string(), path, selection: app.session.selection.clone() });
}

/// Another design is shown: the rows the last Find in Browser opened were that design's, and the
/// next Find leaves this one's rows as they are.
pub(crate) fn forget_found(app: &mut SolveApp) {
    app.tree.found_rows.clear();
    app.tree.found = None;
}

/// The placement a body (by its world name) is in: its component, its name in that component and
/// the placement's path. One look through the placements.
fn body_placement(app: &SolveApp, body: &str) -> Option<(u64, String, Vec<u64>)> {
    let source = crate::open_meshing::session_world(&app.session).body(body).map(|selected| selected.definition_name().to_string())?;
    let component = app.session.model.state().body(&source).map(|definition| definition.component)?;
    let path = placement_named(app, &source, component, body)?;
    Some((component, source, path))
}

/// The path of the placement of `component` in which its body `source` has the world name
/// `body`. Every component's first placement keeps the definition's name, so the owner must
/// match too. The design is indexed once for the look, so a component placed many times costs
/// its placements, not its placements times the design.
pub(crate) fn placement_named(app: &SolveApp, source: &str, component: u64, body: &str) -> Option<Vec<u64>> {
    let state = app.session.model.state();
    let doc = &app.session.doc;
    let names = solidcraft_engine::doc::WorldNames::new(doc, &state);
    doc.placements().into_iter().find(|(owner, path, _)| *owner == component && names.name(source, component, path) == body).map(|(_, path, _)| path)
}

pub(crate) fn reveal_in_history(app: &mut SolveApp, id: u64) {
    app.tree.search.clear();
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

use crate::context_menu::Target;
use crate::ui_kit::Item;
use solidcraft_theme::Tokens;

use crate::SolveApp;

/// The Browser's rows on screen: (label, row rect, fold arrow area, open).
/// Other presentations expose their geometry through `semantic_content::output`.
pub fn drawn_rows() -> Vec<crate::semantic_content::DrawnRow> {
    use crate::semantic_content::{ContentOwner, output};
    output(ContentOwner::BROWSER).rows
}

pub fn drawn_occurrences() -> Vec<crate::semantic_content::DrawnOccurrence> {
    crate::semantic_content::output(crate::semantic_content::ContentOwner::BROWSER).occurrences
}

/// Browser state kept between frames.
#[derive(Default)]
pub struct TreeState {
    /// Geometry and assembly queries, independent of the camera, selection and row visibility.
    model: Option<model::Model>,
    /// Rows closed by the user (keys like `c0/bodies`): read for rows that start open.
    pub collapsed: BTreeSet<String>,
    /// Rows opened by the user: read for rows that start closed. A row opened or closed by hand
    /// is written to both sets ([`set_open`]), so it keeps its state whichever way it starts.
    pub expanded: BTreeSet<String>,
    /// Scroll to this body's row (by the name it has in the world, which tells the placements
    /// of a part apart): Find in Browser, and a body picked in the viewport while the Browser
    /// follows the 3D view (#3005). [`find_in_tree`] opens the rows above it.
    pub reveal: Option<String>,
    /// Enough provenance to distinguish temporary rollback/suppression from target removal.
    reveal_source: Option<BodyRevealSource>,
    /// Runtime owner of the active pending requests, including requests with numeric IDs.
    reveal_session: Option<u64>,
    /// The rows the last Find in Browser opened (key, starts open), for the next one to close
    /// again. A row opened or closed by hand since leaves the list.
    found_rows: Vec<(String, bool)>,
    /// What the last Find in Browser found, until the selection changes.
    found: Option<Found>,
    /// A native feature to reveal in its placed component's Features folder (Find in Timeline).
    pub(super) reveal_feature: Option<features::Reveal>,
    /// The search above the tree (#1413).
    pub search: crate::browser_search::BrowserSearch,
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
    /// Which part of the design the History bar shows (`timeline_scope`).
    pub(crate) timeline_scope: crate::timeline_scope::Scope,
    /// The component edited or picked when the History bar last followed it, once it has
    /// (`timeline_scope`).
    pub(crate) timeline_followed: Option<Option<u64>>,
    /// A feature the History bar scrolls into view the next time it draws its items, in the
    /// bottom strip or in the history on the left: the edited component's last, or the one Find
    /// in Timeline found (#5850).
    pub timeline_reveal: Option<u64>,
    /// Show components marked out of the History bar anyway (#10393). Held by the session; the
    /// History controls' menu carries the switch.
    pub history_show_hidden: bool,
    /// The runs and folders opened and folded in the active design's History (`history_folds`).
    pub(crate) history_folds: crate::history_folds::Folds,
    /// Those of designs with a file, by file, kept between runs (`history_folds`).
    pub(crate) history_remembered: crate::history_folds::Remembered,
    /// The feature of the face or edge last picked in the view, so a new pick scrolls the
    /// history to it once.
    pub view_pick: Option<u64>,
    /// The Edit Canvas / Calibrate panel.
    pub canvas_panel: Option<CanvasPanel>,
    /// The Edit Point Cloud panel.
    pub cloud_panel: Option<CloudPanel>,
    /// A Section Analysis hidden with its eye in the Analysis folder: its plane (origin,
    /// normal), so the eye can bring it back (#822).
    pub hidden_section: Option<(solidcore_record::Vec3, solidcore_record::Vec3)>,
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
    eye_press: Option<(u64, Arc<Document>, usize, solidcraft_engine::Visibility)>,
    pub(crate) content: crate::semantic_content::Frame,
    cache: Option<cache::Cache>,
    /// The tree key of each item row the panel has seen, by its id: a Shift range comes back as ids.
    node_keys: std::collections::HashMap<NodeId, String>,
}

struct FolderPick {
    key: String,
    session: u64,
    doc: Weak<Document>,
}

struct BodyRevealSource {
    body: String,
    session: u64,
    creator: u64,
}

/// Pending presentation requests parked alongside their session in a bounded document slot.
/// Pointer input (`view_pick`) deliberately never travels with the slot.
#[derive(Default)]
pub(crate) struct PendingReveals {
    session: Option<u64>,
    body: Option<String>,
    source: Option<BodyRevealSource>,
    feature: Option<features::Reveal>,
    timeline: Option<u64>,
}

pub(crate) fn discard_reveals(app: &mut SolveApp) {
    app.tree.reveal = None;
    app.tree.reveal_source = None;
    app.tree.reveal_feature = None;
    app.tree.timeline_reveal = None;
    app.tree.view_pick = None;
    app.tree.reveal_session = Some(app.session.uid);
}

/// Guard request creation and presentation even when a session changes after frame begin.
pub(crate) fn sync_reveals(app: &mut SolveApp) {
    if app.tree.reveal_session.is_some_and(|session| session != app.session.uid) {
        discard_reveals(app);
    }
    if app.tree.reveal_source.as_ref().is_some_and(|source| source.session != app.session.uid) {
        app.tree.reveal = None;
        app.tree.reveal_source = None;
    }
    app.tree.reveal_session = Some(app.session.uid);
}

pub(crate) fn take_reveals(app: &mut SolveApp) -> PendingReveals {
    sync_reveals(app);
    app.tree.view_pick = None;
    PendingReveals {
        session: Some(app.session.uid),
        body: app.tree.reveal.take(),
        source: app.tree.reveal_source.take(),
        feature: app.tree.reveal_feature.take(),
        timeline: app.tree.timeline_reveal.take(),
    }
}

pub(crate) fn restore_reveals(app: &mut SolveApp, pending: PendingReveals) {
    discard_reveals(app);
    if pending.session == Some(app.session.uid) {
        app.tree.reveal = pending.body;
        app.tree.reveal_source = pending.source;
        app.tree.reveal_feature = pending.feature;
        app.tree.timeline_reveal = pending.timeline;
    }
}

pub(crate) fn request_timeline_reveal(app: &mut SolveApp, id: u64) {
    sync_reveals(app);
    app.tree.timeline_reveal = Some(id);
}

impl FolderPick {
    fn current(&self, app: &SolveApp) -> bool {
        self.session == app.session.uid && std::ptr::eq(self.doc.as_ptr(), Arc::as_ptr(&app.session.doc))
    }
}

impl TreeState {
    pub(crate) fn for_session(session: u64) -> Self {
        Self { reveal_session: Some(session), ..Default::default() }
    }

    pub(crate) fn clear_folder_pick(&mut self) {
        self.picked_folder = None;
    }

    /// Open a row. A placement's row is noted open in `expanded` as well, because it starts
    /// closed unless the "Components start open" preference is on (#9274).
    pub(crate) fn open_row(&mut self, key: &str) {
        self.collapsed.remove(key);
        if is_placement_key(key) && !self.expanded.contains(key) {
            self.expanded.insert(key.to_owned());
        }
    }

    /// Close a row, whichever way its placement starts.
    pub(crate) fn close_row(&mut self, key: &str) {
        if !self.collapsed.contains(key) {
            self.collapsed.insert(key.to_owned());
        }
        if is_placement_key(key) {
            self.expanded.remove(key);
        }
    }

    fn remember(&mut self, key: &str) {
        self.content.current_mut().remember(key);
        self.node_keys.entry(node_id(key)).or_insert_with(|| key.to_owned());
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
    /// The occurrence's centre when the move began: what it turns about.
    pub pivot: Option<solidcore_record::Vec3>,
    /// The order the angles apply in (`gizmo::turn_of`), so each stays what was dragged.
    order: [usize; 3],
    /// The placement the values start from, when not the baseline: a turn no sum of angles can
    /// make starts the values over from where the occurrence stands.
    base: Option<solidcraft_engine::doc::Mat>,
    /// The values when the current triad drag began, and whether its ring has settled its angle.
    drag0: Option<([f64; 3], [f64; 3])>,
    ring_set: bool,
}

impl OccMove {
    pub fn new(occurrence: u64) -> Self {
        OccMove {
            occurrence,
            path: Vec::new(),
            baseline: None,
            had_pending: false,
            translate: [0.0; 3],
            angles: [0.0; 3],
            pivot: None,
            order: crate::gizmo::ANGLE_ORDER,
            base: None,
            drag0: None,
            ring_set: false,
        }
    }

    /// The values go back to zero where the occurrence stands, which keeps its pending move.
    fn bake(&mut self, app: &SolveApp) {
        let t = self.translate;
        self.pivot = self.pivot.map(|p| p + solidcore_record::Vec3::new(t[0], t[1], t[2]));
        self.base = app.session.pending_moves.get(&self.occurrence).copied().or(self.base);
        (self.translate, self.angles, self.order) = ([0.0; 3], [0.0; 3], crate::gizmo::ANGLE_ORDER);
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
    let world = crate::open_meshing::session_world(&app.session);
    let picked: Vec<Sel> =
        names.iter().filter(|n| world.body(n).is_some() && !app.session.body_removed_later(n)).map(|n| Sel::Body { name: n.clone() }).collect();
    if picked.is_empty() {
        return false;
    }
    // The chat's picker takes them into Bodies whichever list is active (#11318).
    if let Some(d) = app.dialog.as_mut() {
        d.activate_input_taking(crate::selection::BODIES);
    }
    if !dialog_takes_body(app) || app.dialog.as_ref().is_some_and(|d| d.takes_components()) {
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

/// Whether a folder or group's scope key is a row the browser draws for `component`: the design's
/// own (`c0`, the root only), or a placement of it, spelt as the browser keys it (`o:12/3`, not
/// `o:012/3`: the rows' state is kept by that key).
pub(crate) fn scope_shows(app: &SolveApp, scope: &str, component: u64) -> bool {
    if component == 0 && scope == "c0" {
        return true;
    }
    let path = scope_path(scope);
    is_placement_key(scope) && occurrence_key(&path) == scope && app.session.occurrence_placement(&path).is_ok_and(|p| p.component == component)
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

/// Move Sketches to Their Parts (`component.move_sketches.to_parts`) for `sketches` (ids). They
/// go as numbers: a string is a name first, and a sketch may be named like another one's id.
pub(crate) fn sketches_to_parts_item(sketches: Vec<String>) -> Item {
    let ids: Vec<u64> = sketches.iter().filter_map(|k| k.parse().ok()).collect();
    Item::action("component.move_sketches.to_parts", "Move Sketches to Their Parts", "move").with(json!({ "sketches": ids }))
}

/// The menu of a folder row (Bodies, Sketches, Construction).
pub fn folder_items(app: &SolveApp, component: u64, folder: &str, scope: &str) -> Vec<Item> {
    if folder == "origin" {
        return vec![Item::action("ui.folderVisible", "Show/Hide", "eye").key("V").with(json!({"key":format!("{scope}/origin")}))];
    }
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
    let count = entries(app, comp, "bodies").len();
    if !bodies_folded(app, comp, &scope, count) {
        return Vec::new();
    }
    // Components from a component's only body would just nest it one level deeper.
    let items: Vec<_> = folder_items(app, comp, "bodies", &scope)
        .into_iter()
        .filter(|i| i.id == "ui.groupSelected" || (i.id == "component.from_bodies" && count > 1))
        .collect();
    if items.is_empty() {
        return items;
    }
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
    crate::semantic_content::ensure_session(app, ctx);
    let (pressed, down) = ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down()));
    if pressed && app.tree.eye_press.is_none() {
        app.tree.eye_press = Some((app.session.uid, app.session.doc.clone(), app.session.undo.len(), without_origin(app.session.visibility.clone())));
    }
    if down {
        return;
    }
    end_eye_drag(app, ctx);
}

/// Also called when the presentation holding the gesture disappears while the button is down.
pub(crate) fn end_eye_drag(app: &mut SolveApp, ctx: &egui::Context) {
    let capture = crate::semantic_content::release_eye(ctx);
    let dragged = ctx.data_mut(|d| d.remove_temp::<bool>(crate::ui_kit::eye_drag_id())).is_some();
    merge_eye_drag(app, dragged && capture.is_none_or(|source| source.session == app.session.uid));
}

/// An eye drag ended (`dragged`): the steps it made since the button went down become one.
pub(crate) fn merge_eye_drag(app: &mut SolveApp, dragged: bool) {
    let Some((_, doc, depth, visibility)) = app.tree.eye_press.take().filter(|(session, ..)| dragged && *session == app.session.uid) else {
        return;
    };
    // shortcut: once the history is full (200 steps) its length stops growing, so the drag is left as
    // separate steps, like the point-cloud and canvas drags; the visibility test keeps a trimmed front
    // from being merged wrongly.
    let undo = &mut app.session.undo;
    if undo.len() > depth + 1 && undo.get(depth).is_some_and(|s| Arc::ptr_eq(&s.doc, &doc) && without_origin(s.visibility.clone()) == visibility) {
        undo.truncate(depth + 1);
    }
}

pub(crate) fn discard_eye_press(app: &mut SolveApp) {
    app.tree.eye_press = None;
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

/// The group holding an item in a component's folder, then the groups holding that one, out to the
/// folder.
fn group_chain(app: &SolveApp, component: u64, folder: &str, key: &str) -> Vec<u64> {
    let doc = &app.session.doc;
    std::iter::successors(doc.group_of_item(component, folder, key).map(|g| g.id), |id| doc.browser_group(*id).and_then(|g| g.parent))
        .take(solidcraft_engine::doc::MAX_GROUP_NESTING + 1)
        .collect()
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
                // The new group shows open, in its open folder and group.
                let start_open = rows_start_open(app);
                if let Some(parent) = parent {
                    set_open(app, &format!("c{comp}/g{parent}"), start_open, true);
                }
                set_open(app, &format!("c{comp}/{folder}"), start_open, true);
                set_open(app, &format!("c{comp}/g{g}"), start_open, true);
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
const INDENT: f32 = 14.0;
use crate::ui_kit::browser::{self as panel, Dragged, How, NodeId, Payload, Rows, Selectable, Zone};

fn row_rect(ui: &egui::Ui) -> Rect {
    crate::ui_kit::tree_row_rect(ui, &Row { compact: compact_tree(ui), ..Default::default() })
}

/// One row's look.
pub(crate) use crate::ui_kit::{Mark, Row};

fn compact_tree(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("sc_compact_tree"))).unwrap_or(false)
}

/// Where tree rows put their eye this frame (Preferences › Interface, #8718).
pub(crate) fn publish_row_prefs(app: &SolveApp, ctx: &egui::Context) {
    ctx.data_mut(|d| {
        d.insert_temp(egui::Id::unique("sc_tree_eye_left"), app.preferences.browser_eye_left);
        d.insert_temp(egui::Id::unique("sc_tree_dialog_open"), app.dialog.is_some());
    });
}

/// With a dialog open a second click on a row picks again, so it never waits to rename.
fn dialog_open(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("sc_tree_dialog_open"))).unwrap_or(false)
}

fn eye_left(ui: &egui::Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(egui::Id::unique("sc_tree_eye_left"))).unwrap_or(false)
}

/// A Browser row's identity in the panel: its tree key, hashed the same way every frame. Tree keys
/// are unique in the Browser, so the hash is too.
pub(crate) fn node_id(key: &str) -> NodeId {
    egui::Id::unique(("sc_browser_row", key)).value()
}

/// The kinds of rows the Browser drags: a folder's items or groups, and placements.
const DRAG_ITEMS: u32 = 0;
const DRAG_PLACEMENTS: u32 = 1;

/// What a Browser row is beyond its look: its tree key, whether it is an item of the selection
/// (in the order a Shift range spans), the kind it drags as, the drops it takes and its rule for
/// them.
#[derive(Default)]
pub(crate) struct Spec<'a> {
    pub(crate) key: &'a str,
    pub(crate) item: bool,
    pub(crate) drag: Option<u32>,
    pub(crate) drops: panel::Drops,
    pub(crate) accept: Option<&'a dyn Fn(&Dragged, Zone) -> bool>,
}

/// Rows dropped on a row: where they land on it, the presentation they were dragged in and what
/// they are.
pub(crate) struct Dropped {
    pub(crate) zone: Zone,
    pub(crate) source: panel::Source,
    pub(crate) data: Option<Payload>,
}

/// A row's response as the Browser's handlers read it: the panel's events for the row.
#[derive(Default)]
pub(crate) struct Hit {
    pub(crate) rect: Option<Rect>,
    pub(crate) label_rect: Option<Rect>,
    pub(crate) row_id: Option<egui::Id>,
    /// A click on the row: the selection's (`how`), a plain one, the second of a double-click, or
    /// the slower second click that renames.
    pub(crate) clicked: bool,
    pub(crate) how: Option<How>,
    pub(crate) double: bool,
    pub(crate) slow_rename: bool,
    pub(crate) secondary: Option<Pos2>,
    pub(crate) fold: bool,
    pub(crate) eye: bool,
    pub(crate) radio: bool,
    pub(crate) action: bool,
    pub(crate) hovered: bool,
    pub(crate) resp: Option<egui::Response>,
    pub(crate) on_control: bool,
    pub(crate) drag_started: bool,
    pub(crate) fold_at: Option<Pos2>,
    pub(crate) eye_at: Option<Pos2>,
    pub(crate) eye_rect: Option<Rect>,
    pub(crate) radio_at: Option<Pos2>,
    /// The trailing actions clicked, by target.
    pub(crate) actions: Vec<String>,
    pub(crate) dropped: Option<Dropped>,
}

/// Where a row's name is edited in place.
#[derive(Clone, Copy, Default)]
pub(crate) struct NameAt {
    pub(crate) row_id: Option<egui::Id>,
    pub(crate) rect: Option<Rect>,
    pub(crate) label_rect: Option<Rect>,
}

impl Hit {
    pub(crate) fn name_at(&self) -> NameAt {
        NameAt { row_id: self.row_id, rect: self.rect, label_rect: self.label_rect }
    }

    /// The host data of what was dropped on the row, when it is a `T`.
    fn dropped_as<T: std::any::Any>(&self) -> Option<(&T, Zone, crate::semantic_content::ContentSource)> {
        let dropped = self.dropped.as_ref()?;
        let data = dropped.data.as_ref()?.get::<T>()?;
        Some((data, dropped.zone, crate::semantic_content::content_source(dropped.source)))
    }
}

/// Draw a Browser row through the panel and read its events. A renamable row asks to be renamed
/// (the host's editor, `context_menu`), unless a dialog is open: then a second click picks again.
pub(crate) fn node_row(rows: &mut Rows<'_>, spec: Spec<'_>, row: Row<'_>) -> Hit {
    let Spec { key, item, drag, drops, accept } = spec;
    let rename = if row.rename && !dialog_open(rows.ui()) { panel::Rename::Host } else { panel::Rename::Never };
    let out = rows.row(panel::Node {
        id: node_id(key),
        key,
        row,
        select: if item { Selectable::Yes(0) } else { Selectable::No },
        rename,
        drag,
        drops,
        menu: panel::NodeMenu::Host,
        accept,
        ..Default::default()
    });
    let mut hit = Hit {
        rect: out.rect,
        label_rect: out.label_rect,
        row_id: out.widget,
        hovered: out.hovered,
        resp: out.response,
        on_control: out.on_control,
        drag_started: out.drag_started,
        fold_at: out.fold_at,
        eye_at: out.eye_at,
        eye_rect: out.eye_rect,
        radio_at: out.radio_at,
        ..Default::default()
    };
    for event in out.events {
        match event {
            panel::Event::Select { how, .. } => {
                hit.clicked = true;
                hit.how = Some(how);
            }
            panel::Event::Click(_) => hit.clicked = true,
            // The second click of a double-click is a plain click too.
            panel::Event::Open(_) => {
                hit.clicked = true;
                hit.double = true;
                hit.how = item.then_some(How::Only);
            }
            // The slower second click, held back until the double-click delay passed.
            panel::Event::RenameRequested(_) => {
                hit.clicked = true;
                hit.slow_rename = true;
                hit.how = item.then_some(How::Only);
            }
            panel::Event::Fold { .. } => hit.fold = true,
            panel::Event::Eye { .. } => hit.eye = true,
            panel::Event::Activate(_) => hit.radio = true,
            panel::Event::NameAction(_) => hit.action = true,
            panel::Event::Menu { at, .. } => hit.secondary = Some(at),
            panel::Event::Move { zone, source, data, .. } => hit.dropped = Some(Dropped { zone, source, data }),
            panel::Event::Action { action, .. } => hit.actions.push(action),
            panel::Event::Rename { .. } | panel::Event::Revealed(_) | panel::Event::EyeGesture(_) => {}
        }
    }
    hit
}

/// The Browser's rows for the presentation being drawn, through the panel: in a scroll area
/// (`scroll`) or in `ui`'s own layout, each row's events handled as it is drawn. The rows drawn are
/// recorded for the presentation.
pub(crate) fn present(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    acts: &mut Actions,
    (scroll, shrink): (bool, bool),
    draw: impl FnOnce(&mut SolveApp, &mut Rows<'_>, &mut Actions),
) -> panel::Output {
    let owner = app.tree.content.active_owner();
    let mut state = std::mem::take(&mut app.tree.content.current_mut().browser);
    let style = panel::Style { compact: compact_tree(ui), eye_left: eye_left(ui) };
    let output = panel::Browser::new(&mut state, style, app.session.uid)
        .presentation(owner.0)
        .active(crate::semantic_content::presented(ui))
        .scroll(scroll)
        .shrink(shrink)
        .events_per_row(true)
        .show(ui, |rows| draw(app, rows, acts));
    app.tree.content.current_mut().browser = state;
    crate::semantic_content::record_drawn(ui, &output);
    for event in &output.events {
        if let panel::Event::EyeGesture(gesture) = event {
            crate::semantic_content::eye_gesture(ui, *gesture);
        }
    }
    output
}

/// Whether rows dragged in the panel come from this document.
fn same_document(app: &SolveApp, dragged: &Dragged) -> bool {
    dragged.source.scope == app.session.uid
}

/// A folder's items dragged, as the panel carries them.
fn item_drag(dragged: &Dragged) -> Option<&ItemDrag> {
    dragged.data.as_ref()?.get::<ItemDrag>()
}

/// A placement dragged, as the panel carries it.
fn placement_drag(dragged: &Dragged) -> Option<&PlacementDrag> {
    dragged.data.as_ref()?.get::<PlacementDrag>()
}

/// A click on a selectable row, as the panel read its modifiers: the row alone, toggled, or the
/// rows from the anchor (Shift), added with Ctrl.
fn click_select(app: &mut SolveApp, how: Option<&How>, sel: Option<Sel>) {
    if let Some(How::Range(ids) | How::RangeAdd(ids)) = how {
        let items: Vec<Sel> = ids.iter().filter_map(|id| app.tree.node_keys.get(id)).filter_map(|key| sel_of_key(key)).collect();
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
        let _ = app.run("select.set", json!({ "items": items, "add": matches!(how, Some(How::RangeAdd(_))) }));
        return;
    }
    let Some(sel) = sel else { return };
    if matches!(how, Some(How::Toggle)) {
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

/// A pick in the viewport, while the Browser is shown and follows the 3D view ([`follows_view`]):
/// the Browser finds the body of the last item picked, as Find in Browser does (#3005). A search
/// stays (#1413): a body it found opens in it, and one it didn't leaves the tree as it is.
pub(crate) fn reveal_pick(app: &mut SolveApp) {
    sync_reveals(app);
    if follows_view(app)
        && app.ui.show_browser
        && let Some(body) = app.session.selection.last().and_then(picked_body).map(str::to_string)
        && app.tree.search.hits().is_none_or(|hits| hits.found_body(&body))
    {
        find_in_tree(app, &body);
    }
}

fn remember_reveal_source(app: &mut SolveApp, body: &str) {
    app.tree.reveal_source = crate::open_meshing::session_world(&app.session).body(body).map(|selected| BodyRevealSource {
        body: body.to_owned(),
        session: app.session.uid,
        creator: selected.feature,
    });
}

/// Opens the design row and every placement down to `path` (Find in Timeline).
pub(crate) fn open_path(app: &mut SolveApp, path: &[u64]) {
    set_open(app, "c0", true, true);
    let start_open = rows_start_open(app);
    for depth in 1..=path.len() {
        set_open(app, &occurrence_key(&path[..depth]), start_open, true);
    }
}

/// A placement's own row (`o:1/2`), not a folder under it (`o:1/2/bodies`).
pub(crate) fn is_placement_key(key: &str) -> bool {
    key.strip_prefix("o:").is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit() || b == b'/'))
}

/// Whether a placement's row is open. Placements start closed, so a large assembly opens as a
/// short list (#9274); the "Components start open" preference keeps them open as before, unless
/// the Browser's rows start closed ([`rows_start_open`]).
pub(crate) fn placement_open(app: &SolveApp, key: &str) -> bool {
    !app.tree.collapsed.contains(key) && ((app.preferences.browser_components_open && rows_start_open(app)) || app.tree.expanded.contains(key))
}

/// Is the row with this fold key open? During a search every row is, unless it was closed in
/// the search; the tree's own folds come back when it ends.
pub(crate) fn is_open(app: &SolveApp, key: &str, default: bool) -> bool {
    if app.tree.search.on() {
        return !app.tree.search.closed(key);
    }
    fold_open(app, key, default)
}

/// Whether a row is open in the tree's own folds, whatever a search shows. The History, beside the
/// Browser or as the bar, reads its folds here: a search filters the Browser's rows, not the History's.
pub(crate) fn fold_open(app: &SolveApp, key: &str, default: bool) -> bool {
    if is_placement_key(key) {
        return placement_open(app, key);
    }
    if default { !app.tree.collapsed.contains(key) } else { app.tree.expanded.contains(key) }
}

/// Open or close a row in the tree's own folds, whichever way it starts; whether that changed it.
/// Opening it also lifts a close made during a search, so a revealed row shows either way.
fn set_open(app: &mut SolveApp, key: &str, default: bool, open: bool) -> bool {
    let was = fold_open(app, key, default);
    if open {
        app.tree.search.open(key);
        app.tree.collapsed.remove(key);
        app.tree.expanded.insert(key.to_string());
    } else {
        app.tree.collapsed.insert(key.to_string());
        app.tree.expanded.remove(key);
    }
    was != open
}

/// A row opened or closed by hand: the next Find in Browser leaves it as it is. During a search it
/// folds in the search only, and the tree's own folds come back when the search ends.
pub(crate) fn toggle(app: &mut SolveApp, key: &str, default: bool) {
    if app.tree.search.on() {
        app.tree.search.toggle_closed(key);
        return;
    }
    toggle_fold(app, key, default);
}

/// A row opened or closed by hand in the tree's own folds, whatever a search shows: the History's
/// folds, which a search leaves alone. The next Find in Browser leaves it as it is.
pub(crate) fn toggle_fold(app: &mut SolveApp, key: &str, default: bool) {
    app.tree.found_rows.retain(|(row, _)| row != key);
    let open = fold_open(app, key, default);
    set_open(app, key, default, !open);
}

/// Apply Expand All / Collapse All to the rows the command listed, whichever way each starts.
/// They count as folded by hand: the next Find in Browser leaves them as they are.
pub fn apply_fold(app: &mut SolveApp, open: bool, open_by_default: &[String], closed_by_default: &[String]) {
    app.tree.found_rows.retain(|(row, _)| !open_by_default.contains(row) && !closed_by_default.contains(row));
    for key in open_by_default.iter().chain(closed_by_default) {
        set_open(app, key, false, open);
    }
}

/// Everything a frame's rows want done, applied after drawing.
#[derive(Default)]
pub(crate) struct Actions {
    pub(crate) session: Option<u64>,
    menu: Option<(Pos2, Target)>,
    pub(crate) hover_bodies: Vec<String>,
    pub(crate) hovered: bool,
    /// The plane whose row is under the pointer, lit in the view (#6975).
    pub(crate) hover_plane: Option<String>,
    /// The feature or sketch whose row is under the pointer (#14263).
    pub(crate) hover_feature: Option<u64>,
    pub(crate) hover_joint: Option<u64>,
    drop: Option<(crate::semantic_content::ContentSource, DropAction)>,
    /// What the selection and the 3D view's pointer light in the tree.
    lit: Lit,
}

impl Actions {
    fn drop_from(&mut self, source: crate::semantic_content::ContentSource, action: DropAction) {
        self.drop = Some((source, action));
    }
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
    /// Selected, or part of something selected: a sketch for its curves, points and profiles.
    selected: std::collections::HashSet<Thing>,
    /// Bodies holding a selected face, edge or vertex. Each keeps the faint bar of a row with
    /// something lit inside it, so the tree still shows where the selection is once the pointer
    /// leaves the model (#11547), without lighting the body as if it were selected.
    holding: std::collections::HashSet<Thing>,
    /// Under the pointer in the 3D view.
    hovered: Option<Thing>,
    /// The placements holding a lit body, and the components holding a lit sketch or plane.
    paths: Vec<Vec<u64>>,
    components: Vec<u64>,
    /// The feature of what is under the pointer in the 3D view, and of the face or edge picked
    /// there ([`Lit::features_from_view`]), once worked out this frame.
    from_view: Option<[Option<u64>; 2]>,
}

#[cfg(test)]
thread_local! {
    /// How often the features lit from the 3D view were worked out (the tests count them).
    static FROM_VIEW_WORKED_OUT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Lit {
    /// The feature that made what is under the pointer in the 3D view, and the one that made the
    /// face or edge picked there, as the history lights them. Worked out for the first feature row
    /// that asks in a frame: every row and folder looked them up again, the pick naming every face
    /// of its body each time (#14850).
    fn features_from_view(&mut self, app: &SolveApp) -> [Option<u64>; 2] {
        *self.from_view.get_or_insert_with(|| {
            #[cfg(test)]
            FROM_VIEW_WORKED_OUT.with(|n| n.set(n.get() + 1));
            [crate::timeline::hovered_feature(app), crate::timeline::picked_feature(app)]
        })
    }

    fn of(app: &mut SolveApp) -> Lit {
        use crate::viewport::Hit;
        let mut selected = std::collections::HashSet::new();
        let mut holding = std::collections::HashSet::new();
        // Bodies chosen in an open dialog light up even when the click was in the 3D view,
        // which does not rewrite the browser selection.
        let dialog_picks = app.dialog.as_ref().map(|d| d.items()).unwrap_or_default();
        for s in app.session.selection.iter().chain(&dialog_picks) {
            let thing = match s {
                Sel::Body { name } => Thing::Body(name.clone()),
                // A picked face, edge or vertex lights the feature it belongs to (see
                // `browser::features`), not the whole body as if the body were selected; its body
                // keeps a faint bar. The origin's vertex belongs to no body.
                Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => {
                    if !body.is_empty() {
                        holding.insert(Thing::Body(body.clone()));
                    }
                    continue;
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
            };
            selected.insert(thing);
        }
        let hovered = match &app.viewport.hover {
            Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => Some(Thing::Body(body.clone())),
            Some(
                Hit::SketchPoint { sketch, .. }
                | Hit::SketchPoint3d { sketch, .. }
                | Hit::SketchMidpoint { sketch, .. }
                | Hit::SketchCurve { sketch, .. }
                | Hit::Profile { sketch, .. },
            ) => Some(Thing::Sketch(*sketch)),
            Some(Hit::Plane { name, .. }) => Some(Thing::Plane(name.clone())),
            Some(Hit::Axis { name }) => Some(Thing::Axis(name.clone())),
            Some(Hit::ConstructionPoint { name }) => Some(Thing::Point(name.clone())),
            None => None,
        };
        // Unless the Browser follows the 3D view, a body's placement is not looked up for the rows
        // above it (that names every placement again after each edit): only the one Find in
        // Browser found is known, until the selection changes.
        let follows = follows_view(app);
        if app.tree.found.as_ref().is_some_and(|found| found.selection != app.session.selection) {
            app.tree.found = None;
        }
        let doc = app.session.doc.clone();
        let (mut paths, mut components) = (Vec::new(), Vec::new());
        for thing in selected.iter().chain(&holding).chain(&hovered) {
            match thing {
                Thing::Body(b) if follows => {
                    if let Some(path) = app.tree.model.as_mut().and_then(|model| model.body_path(&app.session, b)) {
                        paths.push(path);
                    }
                }
                Thing::Body(b) => {
                    paths.extend(app.tree.found.as_ref().filter(|found| found.body == *b && !found.path.is_empty()).map(|found| found.path.clone()));
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
        // A body both holding the selection and under the pointer leads to its placement once.
        paths.sort_unstable();
        paths.dedup();
        Lit { selected, holding, hovered, paths, components, from_view: None }
    }

    fn any(&self) -> bool {
        !self.selected.is_empty() || !self.holding.is_empty() || self.hovered.is_some()
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

    /// A folder entry's row: its selection flag and mark. A body holding a selected face, edge or
    /// vertex, itself neither selected nor under the pointer, gets the faint bar.
    fn row(&self, app: &SolveApp, e: &Entry) -> (bool, Mark) {
        match lit_row(self.entry(app, e)) {
            (false, Mark::None) if self.holding.contains(&e.thing) => (false, Mark::Branch),
            row => row,
        }
    }

    /// The mark of a row holding these entries: a faint bar when any of them is lit.
    fn branch<'e>(&self, app: &SolveApp, entries: impl IntoIterator<Item = &'e Entry>) -> Mark {
        if entries.into_iter().any(|e| self.row(app, e) != (false, Mark::None)) { Mark::Branch } else { Mark::None }
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

/// Existing Browser content without placement, title or collapse chrome.
pub(crate) fn panel_contents(app: &mut SolveApp, ui: &mut egui::Ui) {
    sync_reveals(app);
    let render = crate::render_workspace::active(app);
    if render {
        // Render has its own search, and its tree never filters by this one.
        app.tree.search.clear();
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::unique("sc_compact_tree"), true));
    } else if ui.rect_contains_pointer(ui.max_rect())
        && !ui.ctx().text_edit_focused()
        && ui.input(|i| i.modifiers.command_only() && i.key_pressed(egui::Key::F))
        && crate::keymap::commands_for(app, &crate::keymap::key_name(egui::Key::F, egui::Modifiers::COMMAND)).is_empty()
    {
        // A field being typed in keeps the keyboard, and a command given this key keeps it: the
        // magnifier opens the search then.
        show_search(app);
    }
    if !crate::render_workspace::browser_mode(app, ui) {
        crate::edit_in_place::browser_strip(app, ui);
        browser_rows_hugging(app, ui);
    }
    if render {
        ui.ctx().data_mut(|data| data.remove::<bool>(egui::Id::unique("sc_compact_tree")));
    }
}

/// Gives the search's field the keyboard, showing its bar and opening and unfolding the Browser's
/// panel ([`reveal_shared_panel`]).
fn show_search(app: &mut SolveApp) {
    reveal_shared_panel(app, "browser");
    app.tree.search.show();
}

/// The panel header's magnifier: ends the search when its bar is shown, otherwise shows the bar.
pub(crate) fn magnifier(app: &mut SolveApp) {
    if crate::browser_search::bar_shown(app) {
        app.tree.search.clear();
    } else {
        show_search(app);
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
                let kind = if sketches { matches!(f.kind, FeatureKind::Sketch { .. }) } else { f.kind.is_construction() };
                (f.component == component && kind).then_some(i)
            })
            .collect(),
    }
}

/// The design's Origin, Bodies and Construction rows as the browser draws them (a click picks
/// for an open dialog as there), for the design tree over the view while a dialog holds the left
/// column (`dialog_tree`, #6710).
fn browser_rows_hugging(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::semantic_content::show(
        app,
        ui,
        crate::semantic_content::ContentOwner::BROWSER,
        crate::semantic_content::ContentRequest::Browser { shrink_height: true },
    );
}

fn rows_scrolling(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions, shrink_height: bool) {
    prepare_model(app);
    if crate::browser_search::bar_shown(app) {
        // The panel host draws the title row (#15184), so the field is the first row under it.
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), crate::ui_kit::row_height() + Tokens::get().space_8), Sense::hover());
        crate::browser_search::header_field(app, ui, rect);
    }
    if app.tree.search.on() {
        // Under the header and never scrolled with the rows, but clipped to the card's height, so it
        // folds away with the card as the rows do.
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical()
            .id_salt("browser_search_results")
            .max_height(ui.available_height())
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .scroll_source(egui::scroll_area::ScrollSource::NONE)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| crate::browser_search::results(app, ui));
        if app.tree.search.hits().is_some_and(|h| h.total() == 0) {
            return;
        }
    }
    let mut cache = app.tree.cache.take().unwrap_or_default();
    cache.prepare(app);
    let top = ui.cursor().top();
    // The card may animate all the way down to its header: the panel's scroll area shrinks to none.
    let output = present(app, ui, acts, (true, shrink_height), |app, rows, acts| {
        if crate::render_workspace::active(app) {
            crate::render_workspace::scene::sections(app, rows);
            if !crate::render_workspace::scene::objects(app, rows, &mut acts.hover_bodies, &mut acts.hovered) {
                component_rows(app, rows, 0, 1, acts, &mut cache);
            }
            crate::render_workspace::scene::output(app, rows);
        } else {
            component_rows(app, rows, 0, 0, acts, &mut cache);
        }
    });
    crate::panel_runtime::report_content_extent(ui, top, output.content_height, true);
    app.tree.cache = Some(cache);
}

/// Existing semantic rows, reusable by either presentation without a panel host.
pub(crate) fn render_content(app: &mut SolveApp, ui: &mut egui::Ui, request: crate::semantic_content::ContentRequest) -> Actions {
    crate::semantic_content::ensure_session(app, ui.ctx());
    let mut acts = Actions { session: Some(app.session.uid), lit: Lit::of(app), ..Default::default() };
    match request {
        crate::semantic_content::ContentRequest::Browser { shrink_height } => rows_scrolling(app, ui, &mut acts, shrink_height),
        // History's rows are never filtered by the Browser's search.
        crate::semantic_content::ContentRequest::ActiveBodiesOrigin => unsearched(app, |app| active_bodies_origin(app, ui, &mut acts)),
    }
    acts
}

/// `draw` with the Browser's search set aside, for rows outside the Browser that it never filters.
fn unsearched(app: &mut SolveApp, draw: impl FnOnce(&mut SolveApp)) {
    let search = std::mem::take(&mut app.tree.search);
    draw(app);
    app.tree.search = search;
}

/// History's optional prefix shares the Browser's cache, folds, row renderers and commands.
pub(crate) fn history_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::semantic_content::show(
        app,
        ui,
        crate::semantic_content::ContentOwner::HISTORY,
        crate::semantic_content::ContentRequest::ActiveBodiesOrigin,
    );
}

fn active_bodies_origin(app: &mut SolveApp, ui: &mut egui::Ui, acts: &mut Actions) {
    prepare_model(app);
    let component = app.session.active_component;
    let path = (component != 0)
        .then(|| app.session.active_occurrence_path.clone().unwrap_or_else(|| app.session.doc.first_path(component)))
        .filter(|path| !path.is_empty());
    // The Bodies folder as the browser draws it for that component or placement (`browser_cache`).
    let mut cache = app.tree.cache.take().unwrap_or_default();
    cache.prepare(app);
    let bodies = cache.folder(app, component, path.as_deref(), "bodies");
    app.tree.cache = Some(cache);
    // In the History's own layout, with no scroll area of their own.
    present(app, ui, acts, (false, true), |app, rows, acts| {
        folder_rows_at(app, rows, component, path.as_deref(), "bodies", 0, acts, &bodies);
        origin_rows(app, rows, 0, acts);
    });
}

pub(crate) fn apply_content_actions(app: &mut SolveApp, acts: Actions) {
    if acts.session != Some(app.session.uid) {
        return;
    }
    if let Some((_, d)) = acts.drop.filter(|(source, _)| crate::semantic_content::source_present(app, *source)) {
        // The target may have consumed a payload before its source's absence was known.
        // Commit only after all owners have reported, using the payload's original source.
        apply_drop(app, d);
    }
    if let Some((at, target)) = acts.menu {
        crate::context_menu::open_for(app, at, target);
    }
}

pub(crate) fn cancel_content_drag(ctx: &egui::Context, session: u64) {
    panel::cancel_drag(ctx, session);
}

/// App-owned editor lifecycle; these windows do not depend on any tree being presented.
pub(crate) fn editors(app: &mut SolveApp, ui: &mut egui::Ui) {
    crate::browser_prompts::occurrence_panel(app, ui);
    crate::browser_prompts::capture_prompt(app, ui.ctx());
    crate::browser_canvas::panel(app, ui.ctx());
    point_clouds::panel(app, ui.ctx());
    crate::sketch_attachment_ui::panel(app, ui.ctx());
}

/// History's prefix must neither open ancestors nor acknowledge a Browser reveal.
fn body_reveal(app: &SolveApp) -> Option<&str> {
    app.tree.content.browser_presentation().then_some(app.tree.reveal.as_deref()).flatten()
}

/// A hidden or collapsed presentation retains its request. Removed targets expire here.
pub(crate) fn prune_reveals(app: &mut SolveApp) {
    sync_reveals(app);
    if let Some(body) = app.tree.reveal.as_ref() {
        let world = crate::open_meshing::session_world(&app.session);
        let source = app.tree.reveal_source.as_ref().filter(|source| source.body == *body);
        let removed = source.is_some_and(|source| source.session != app.session.uid || app.session.doc.feature(source.creator).is_none());
        let evaluated = match source {
            Some(source) => app.session.doc.features.iter().enumerate().any(|(index, feature)| {
                feature.id == source.creator && !feature.suppressed && app.session.doc.marker.is_none_or(|marker| index < marker)
            }),
            None => app.session.doc.marker.is_none() && !app.session.doc.features.iter().any(|feature| feature.suppressed),
        };
        // A refused feature can temporarily remove a body from evaluation too. Its pending
        // reveal remains useful when the feature is repaired.
        let healthy =
            app.session.model.results.iter().all(|result| result.error.is_none()) && crate::open_meshing::placement_error(&app.session).is_none();
        if world.body(body).is_none() && (removed || (evaluated && healthy)) {
            app.tree.reveal = None;
            app.tree.reveal_source = None;
        }
    } else {
        app.tree.reveal_source = None;
    }
    if app.tree.reveal_feature.as_ref().is_some_and(|target| app.session.doc.feature(target.id).is_none()) {
        app.tree.reveal_feature = None;
    }
    if app.tree.timeline_reveal.is_some_and(|id| app.session.doc.feature(id).is_none()) {
        app.tree.timeline_reveal = None;
    }
}

fn component_name(app: &SolveApp, id: u64) -> String {
    if id == 0 {
        // The design's top row names it the way its tab does: after the file (#7267).
        return crate::documents::tab_name(app, &app.session);
    }
    let doc = &app.session.doc;
    let name = doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default();
    let n = doc.occurrences.iter().filter(|o| o.component == id).count();
    let name = if n > 1 { format!("{name} ({n} instances)") } else { format!("{name}:1") };
    crate::dialogs_part_size::browser_label(app, id, &name)
}

fn component_rows(app: &mut SolveApp, rows: &mut Rows<'_>, id: u64, depth: usize, acts: &mut Actions, cache: &mut cache::Cache) {
    let mut root = std::mem::take(&mut cache.root);
    component_rows_inner(app, rows, id, depth, acts, cache, &mut root);
    cache.root = root;
}

fn component_rows_inner(
    app: &mut SolveApp,
    rows: &mut Rows<'_>,
    id: u64,
    depth: usize,
    acts: &mut Actions,
    cache: &mut cache::Cache,
    root: &mut cache::Node,
) {
    if id == 0 && crate::render_workspace::active(app) {
        let Some(slot) = root.folders.first_mut() else { return };
        let data = slot.get_or_insert_with(|| cache.folder(app, id, None, "bodies"));
        folder_rows_at(app, rows, id, None, "bodies", depth.saturating_sub(1), acts, data);
        for path in &root.children {
            occurrence_rows(app, rows, path, depth, acts, cache);
        }
        return;
    }
    let key = &root.key;
    let open = is_open(app, key, true);
    let active = app.session.active_component == id;
    let bodies = &cache.root_bodies;
    let visible = id == 0 || bodies.is_empty() || bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b));
    let faded = app.session.active_component != 0 && !app.session.doc.component_within(id, app.session.active_component) && !active;
    let name = &root.name;
    let badges: &[&str] = if root.grounded { &["pin"] } else { &[] };
    let appearance = crate::render_workspace::scene::decoration(app, bodies);
    // A search never matches the design itself: it is the matches' parent, dimmed.
    let searching = app.tree.search.on();
    let row = Row {
        depth,
        fold: Some(open),
        eye: (id != 0).then_some(visible),
        icon: "component",
        label: name,
        name: true,
        rename: true,
        selected: app.tree.picked_components.contains(&id),
        dim: faded || searching,
        radio: Some(active),
        badges,
        swatch: appearance.swatch,
        secondary: &appearance.name,
        count: &appearance.count,
        // The design holds everything the tree lists.
        mark: if id == 0 && acts.lit.any() { Mark::Branch } else { Mark::None },
        ..Default::default()
    };
    // Bodies and sketches dropped on a component move into it; a placement dropped on it moves
    // into it (the design row: out to the design).
    let accept = |d: &Dragged, zone: Zone| {
        zone == Zone::Into
            && same_document(app, d)
            && (item_drag(d).is_some_and(|d| d.comp != id && d.folder != "construction" && d.groups.is_empty())
                || placement_drag(d).is_some_and(|d| occurrence_can_land(app, d, id)))
    };
    let r = node_row(rows, Spec { key, drops: panel::Drops { before: false, into: true }, accept: Some(&accept), ..Default::default() }, row);
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
    // With a dialog open a second click picks again: the dialog takes the component's bodies.
    let rename_here = r.slow_rename && app.dialog.is_none();
    if r.clicked && !r.double && !rename_here {
        let names = bodies_of_component(app, id);
        if !offer_bodies(app, &names, Take::Toggle) {
            let ctrl = rows.ui().input(|i| i.modifiers.command || i.modifiers.ctrl);
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
    if rename_here {
        start_inline_rename(app, crate::context_menu::RenameWhat::Component(id), r.name_at());
    } else if r.double {
        if app.session.doc.parts.contains_key(&id) {
            crate::dialogs_part_size::open(app, id);
        } else {
            crate::edit_in_place::double_click_component(app, json!({ "component": id }));
        }
    }
    inline_rename(app, rows.ui(), r.name_at(), name);
    if r.hovered {
        acts.hover(bodies.iter());
    }
    if let Some(p) = r.secondary {
        acts.menu = Some((p, Target::Component { id }));
    }
    if let Some((d, _, source)) = r.dropped_as::<ItemDrag>() {
        acts.drop_from(source, DropAction::Component { to: id, folder: d.folder, keys: d.keys.clone() });
    } else if let Some((d, _, source)) = r.dropped_as::<PlacementDrag>() {
        let occurrence = d.path.last().copied().unwrap_or(0);
        acts.drop_from(source, DropAction::Reparent { occurrence, path: d.path.clone(), parent: id, parent_path: None });
    }
    if !open {
        return;
    }
    let d = depth + 1;
    if id == 0 && !crate::render_workspace::active(app) && !searching {
        doc_settings(app, rows, d);
        named_view_rows(app, rows, d, acts);
        origin_rows(app, rows, d, acts);
        canvas_rows(app, rows, d, acts);
        point_clouds::rows(app, rows, d, acts);
        analysis_rows(app, rows, d, acts);
    }
    let render = crate::render_workspace::active(app);
    for (slot, folder) in root.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        if render && folder != "bodies" {
            continue;
        }
        let data = slot.get_or_insert_with(|| cache.folder(app, id, None, folder));
        folder_rows_at(app, rows, id, None, folder, d, acts, data);
    }
    if !render {
        features::rows(app, rows, id, &[], d, acts);
    }
    if id == 0 && !render {
        acts.hover_joint = crate::dialogs_assembly::browser_joints(app, rows, d);
    }
    for path in &root.children {
        occurrence_rows(app, rows, path, d, acts, cache);
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
    app.tree.close_row(&occurrence_key(path));
}

/// A placement, rather than a component definition: repeated child IDs remain distinct
/// because their complete paths are the selection and row identity.
fn occurrence_rows(app: &mut SolveApp, rows: &mut Rows<'_>, path: &[u64], depth: usize, acts: &mut Actions, cache: &mut cache::Cache) {
    // Own this cached row while drawing: children can populate the cache without copying it.
    let Some((path, mut placement)) = cache.take_occurrence(app, path) else { return };
    occurrence_rows_inner(app, rows, &path, depth, acts, cache, &mut placement);
    cache.occurrences.insert(path, placement);
}

fn occurrence_rows_inner(
    app: &mut SolveApp,
    rows: &mut Rows<'_>,
    path: &[u64],
    depth: usize,
    acts: &mut Actions,
    cache: &mut cache::Cache,
    placement: &mut cache::Node,
) {
    // A search lists the placements that match, and dimmed, those that hold a match.
    let (listed, matches, folds) = app.tree.search.hits().map_or((true, true, true), |h| (h.lists(path), h.matches(path), h.holds(path)));
    if !listed {
        return;
    }
    let key = &placement.key;
    let open = is_open(app, key, true);
    let on_screen = rows.next_visible();
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
    let row = Row {
        depth,
        fold: folds.then_some(open),
        eye: Some(own_visible),
        icon: "component",
        label: &placement.label,
        swatch: appearance.swatch,
        secondary: &appearance.name,
        count: &appearance.count,
        name: true,
        rename: true,
        selected,
        dim: faded || !visible || !matches,
        cut,
        radio: Some(active),
        badges,
        mark: acts.lit.placement(&app.session.doc, path, placement.component),
        ..Default::default()
    };
    // The top of a sibling's row puts the dragged placement before it; the rest of the row, and
    // any other row, moves the placement into this row's component. Bodies and sketches dropped on
    // it move into its component.
    let (component, occurrence) = (placement.component, placement.occurrence);
    let accept = |d: &Dragged, zone: Zone| {
        same_document(app, d)
            && match zone {
                Zone::Before => placement_drag(d).is_some_and(|d| occurrence_sibling(app, d, occurrence).is_some()),
                Zone::Into => {
                    item_drag(d).is_some_and(|d| d.comp != component && d.folder != "construction" && d.groups.is_empty())
                        || placement_drag(d).is_some_and(|d| occurrence_can_land(app, d, component))
                }
            }
    };
    let r = node_row(
        rows,
        Spec { key, item: true, drag: Some(DRAG_PLACEMENTS), drops: panel::Drops { before: true, into: true }, accept: Some(&accept) },
        row,
    );
    if let Some(rect) = r.rect.filter(|_| r.resp.is_some()) {
        crate::semantic_content::record_occurrence(rows.ui(), (path.to_vec(), placement.name.clone(), rect, open));
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
        resp.clone().backing_on_hover_ui(|ui| {
            ui.label(&placement.tooltip);
            if !inherited.is_empty() {
                ui.label(inherited.trim_start());
            }
        });
    }
    // Dragging the row moves the placement it stands for. A press on its fold arrow, eye or radio
    // is not a drag of the row, so those controls keep their own jobs.
    if r.drag_started {
        rows.drag_with(vec![node_id(key)], Some(Payload::new(PlacementDrag { path: path.to_vec(), component: placement.component })));
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
    let rename_here = r.slow_rename && app.dialog.is_none();
    if r.clicked && (takes_components || crate::dialogs_move::picks_components(app)) {
        if let Some(mut d) = app.dialog.take() {
            d.pick(&app.session, Sel::Occurrence { path: path.to_vec() });
            app.dialog = Some(d);
        }
    } else if r.clicked && !r.double && !rename_here {
        let names = app.session.occurrence_bodies(path, true).unwrap_or_default();
        if !offer_bodies(app, &names, Take::Toggle) {
            click_select(app, r.how.as_ref(), Some(Sel::Occurrence { path: path.to_vec() }));
        }
    }
    if rename_here {
        start_inline_rename(app, crate::context_menu::RenameWhat::Component(placement.component), r.name_at());
    }
    inline_rename(app, rows.ui(), r.name_at(), &placement.label);
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
    if let Some((d, _, source)) = r.dropped_as::<ItemDrag>() {
        acts.drop_from(source, DropAction::Component { to: placement.component, folder: d.folder, keys: d.keys.clone() });
    } else if let Some((d, zone, source)) = r.dropped_as::<PlacementDrag>() {
        let dragged = d.path.last().copied().unwrap_or(0);
        match (zone, occurrence_sibling(app, d, placement.occurrence)) {
            (Zone::Before, Some(parent)) => {
                let mut order: Vec<u64> =
                    app.session.doc.occurrences.iter().filter(|o| o.parent == parent && o.id != dragged).map(|o| o.id).collect();
                app.session.doc.sort_placements(parent, &mut order);
                let at = order.iter().position(|o| *o == placement.occurrence).unwrap_or(order.len());
                order.insert(at, dragged);
                acts.drop_from(source, DropAction::OccurrenceOrder { parent, order });
            }
            _ => acts.drop_from(
                source,
                DropAction::Reparent { occurrence: dragged, path: d.path.clone(), parent: placement.component, parent_path: Some(path.to_vec()) },
            ),
        }
    }
    if !open || !folds {
        return;
    }
    let render = crate::render_workspace::active(app);
    if !render {
        origin_rows_at(app, rows, depth + 1, acts, path);
    }
    for (slot, folder) in placement.folders.iter_mut().zip(["bodies", "sketches", "construction"]) {
        if render && folder != "bodies" {
            continue;
        }
        let data = slot.get_or_insert_with(|| cache.folder(app, placement.component, Some(path), folder));
        folder_rows_at(app, rows, placement.component, Some(path), folder, depth + 1, acts, data);
    }
    if !render {
        features::rows(app, rows, placement.component, path, depth + 1, acts);
        // The joints that attach this placement, as the root's Joints folder lists them (#9130).
        if let Some(joint) = crate::dialogs_assembly::component_joints(app, rows, path, key, depth + 1) {
            acts.hover_joint = Some(joint);
        }
    }
    if path.len() < 64 {
        for child in &placement.children {
            occurrence_rows(app, rows, child, depth + 1, acts, cache);
        }
    }
}

/// The Document Settings group. A second click on an entry closes the window it opened, the way
/// that window's own × does (#4150).
fn doc_settings(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize) {
    let key = "docsettings";
    let open = is_open(app, key, false);
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row { depth, fold: Some(open), icon: "settings", label: "Document Settings", ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, false);
    }
    if !open {
        return;
    }
    let units = format!("Units: {}", app.session.doc.units);
    let r =
        node_row(rows, Spec { key: "units", ..Default::default() }, Row { depth: depth + 1, icon: "settings", label: &units, ..Default::default() });
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
    if node_row(rows, Spec { key: "precision", ..Default::default() }, Row { depth: depth + 1, icon: "settings", label: &prec, ..Default::default() })
        .clicked
    {
        if app.prefs_window.shell.open {
            crate::prefs_window::close(app, true);
        } else {
            app.prefs_window.shell.open = true;
        }
    }
    let params = format!("Parameters ({})", app.session.doc.params.len());
    if node_row(rows, Spec { key: "params", ..Default::default() }, Row { depth: depth + 1, icon: "params", label: &params, ..Default::default() })
        .clicked
    {
        if app.params.is_some() {
            app.params = None;
        } else {
            app.start("parameters.change");
        }
    }
    if node_row(
        rows,
        Spec { key: "design_properties", ..Default::default() },
        Row { depth: depth + 1, icon: "settings", label: "Design Properties", ..Default::default() },
    )
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

/// The Origin folder's rows for a placement, as the Browser lists them: the O point, the
/// X/Y/Z axes and the XY/XZ/YZ planes at the placement's own origin, each with its own eye
/// and selection reuse. The design's own Origin is the `id == 0` case and keeps its bare
/// names (#7313).
fn origin_rows(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize, acts: &mut Actions) {
    origin_rows_at(app, rows, depth, acts, &[]);
}

/// The Origin rows of one component placement, under its row: [`origin_rows`] keyed by the
/// complete occurrence path, so repeated and nested placements each get their own folder
fn origin_rows_at(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize, acts: &mut Actions, path: &[u64]) {
    let component = !path.is_empty();
    let folder = solidcraft_engine::origins::folder(path);
    let key = if component { folder.as_str() } else { "origin" };
    let shown = if component { !app.session.folder_hidden(key) } else { app.origin_shown() };
    let faded = !shown || (component && !app.session.occurrence_visible(path));
    let open = is_open(app, key, false);
    let lit = |thing: &Option<Thing>| thing.as_ref().map_or((false, false), |t| acts.lit.thing(t));
    let items: Vec<_> = origin_things()
        .into_iter()
        .map(|(k, icon, thing)| {
            let name = if component { solidcraft_engine::origins::key(path, k) } else { k.into() };
            let thing = if component {
                Some(match icon {
                    "plane" => Thing::Plane(name.clone()),
                    "axis" => Thing::Axis(name.clone()),
                    _ => Thing::Point(name.clone()),
                })
            } else {
                thing
            };
            (k, name, icon, thing)
        })
        .collect();
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row {
            depth,
            fold: Some(open),
            eye: Some(shown),
            icon: "origin",
            label: "Origin",
            handle: component.then_some(key),
            dim: faded,
            mark: if items.iter().any(|(_, _, _, thing)| lit(thing) != (false, false)) { Mark::Branch } else { Mark::None },
            ..Default::default()
        },
    );
    if r.fold || r.clicked {
        toggle(app, key, false);
        if component && r.clicked {
            pick_folder(app, key);
        }
    }
    if r.eye {
        if component {
            toggle_folder(app, key);
        } else {
            app.set_origin_shown(!shown);
        }
    }
    if let Some(p) = r.secondary {
        let target = if component {
            Target::Folder {
                component: app.session.occurrence_placement(path).map_or(0, |p| p.component),
                folder: "origin".into(),
                scope: solidcraft_engine::Session::folder_scope(path),
            }
        } else {
            Target::Origin
        };
        acts.menu = Some((p, target));
    }
    if !open {
        return;
    }
    for (k, name, icon, thing) in items {
        // An origin plane by the name the preferences give it; its key stays the id.
        let label = app.preferences.origin_plane_names.label(k);
        let visible = !app.session.visibility.hidden_origin.contains(&name);
        let sel = match icon {
            "plane" => Some(Sel::Plane { name: name.clone() }),
            "axis" => Some(Sel::Axis { name: name.clone() }),
            _ if component => Some(Sel::ConstructionPoint { name: name.clone() }),
            _ => Some(Sel::Vertex { body: String::new(), point: solidcore_record::Vec3::ZERO }),
        };
        let (selected, mark) = lit_row(lit(&thing));
        let item_key = format!("{key}:{name}");
        let r = node_row(
            rows,
            Spec { key: &item_key, ..Default::default() },
            // Faded when hidden itself or with the whole Origin. A renamed plane's label is already
            // in the interface's language (`name`).
            Row {
                depth: depth + 1,
                eye: Some(visible),
                icon,
                label,
                handle: component.then_some(name.as_str()),
                name: label != k,
                selected,
                dim: !visible || faded,
                mark,
                ..Default::default()
            },
        );
        if r.eye {
            // Through the command, so the Origin row's eye is an undo step (#2219).
            let _ = app.run("browser.visibility", json!({ "items": [&name], "visible": !visible }));
        }
        if let Some(p) = r.secondary {
            let target = match icon {
                "plane" => Some(Target::OriginPlane { name: name.clone() }),
                "axis" => Some(Target::OriginAxis { name: name.clone() }),
                _ => None,
            };
            if let Some(target) = target {
                acts.menu = Some((p, target));
            }
        }
        if r.hovered && icon == "plane" {
            acts.hover_plane = Some(name);
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
/// one (animated); a slower second click renames a saved view.
fn named_view_rows(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize, acts: &mut Actions) {
    let key = "namedviews";
    let open = is_open(app, key, false);
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row { depth, fold: Some(open), icon: "folder", label: "Named Views", ..Default::default() },
    );
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
        let view_key = format!("stdview:{view}");
        let r = node_row(rows, Spec { key: &view_key, ..Default::default() }, Row { depth: depth + 1, icon: "home", label, ..Default::default() });
        if r.double {
            app.animate_view(view);
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::NamedView { name: label.to_string() }));
        }
    }
    let names: Vec<String> = app.session.doc.named_views.iter().map(|v| v.name.clone()).collect();
    for name in names {
        let view_key = format!("view:{name}");
        let r = node_row(
            rows,
            Spec { key: &view_key, ..Default::default() },
            Row { depth: depth + 1, icon: "perspective", label: &name, name: true, rename: true, ..Default::default() },
        );
        if r.slow_rename {
            start_inline_rename(app, crate::context_menu::RenameWhat::View(name.clone()), r.name_at());
        } else if r.double {
            restore_view(app, &name);
        }
        inline_rename(app, rows.ui(), r.name_at(), &name);
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
fn analysis_rows(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize, acts: &mut Actions) {
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
    let start_open = rows_start_open(app);
    let open = is_open(app, key, start_open);
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row { depth, fold: Some(open), eye: Some(folder_shown), icon: "folder", label: "Analysis", dim: !folder_shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, start_open);
    }
    if r.eye {
        app.tree.analysis_hidden = folder_shown;
        sync_section(app);
    }
    if !open {
        return;
    }
    let r = node_row(
        rows,
        Spec { key: "section_analysis", ..Default::default() },
        Row {
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
fn canvas_rows(app: &mut SolveApp, rows: &mut Rows<'_>, depth: usize, acts: &mut Actions) {
    let canvases: Vec<(u64, String, bool, bool)> = app.session.doc.canvases.iter().map(|c| (c.id, c.name.clone(), c.visible, c.locked)).collect();
    if canvases.is_empty() {
        return;
    }
    let key = "canvases";
    let start_open = rows_start_open(app);
    let open = is_open(app, key, start_open);
    // The folder's eye is its own: each canvas keeps its eye while the folder hides them all.
    let shown = !app.session.folder_hidden(CANVASES);
    let r = node_row(
        rows,
        Spec { key, ..Default::default() },
        Row { depth, fold: Some(open), eye: Some(shown), icon: "folder", label: "Canvases", dim: !shown, ..Default::default() },
    );
    if r.fold || r.clicked {
        toggle(app, key, start_open);
    }
    if r.eye {
        toggle_folder(app, CANVASES);
    }
    if !open {
        return;
    }
    for (id, name, visible, locked) in canvases {
        let selected = app.tree.picked_canvases.contains(&id);
        let badges: &[&str] = if locked { &["lock"] } else { &[] };
        let canvas_key = format!("canvas:{id}");
        let r = node_row(
            rows,
            Spec { key: &canvas_key, ..Default::default() },
            Row {
                depth: depth + 1,
                eye: Some(visible),
                icon: "canvas",
                label: &name,
                name: true,
                rename: true,
                selected,
                dim: !visible || !shown,
                badges,
                ..Default::default()
            },
        );
        if r.eye {
            let _ = app.run("canvas.edit", json!({ "canvas": id, "visible": !visible }));
        }
        if r.clicked {
            if r.slow_rename {
                app.tree.picked_canvases = vec![id];
            } else {
                if !rows.ui().input(|i| i.modifiers.command || i.modifiers.ctrl) {
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
        }
        if r.slow_rename {
            start_inline_rename(app, crate::context_menu::RenameWhat::Canvas(id), r.name_at());
        } else if r.double && locked {
            app.set_status(solidcraft_ui_text::tr("browser.canvas_locked"), true);
        } else if r.double {
            app.tree.canvas_panel = Some(CanvasPanel::new(app, id, false));
        }
        inline_rename(app, rows.ui(), r.name_at(), &name);
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
    let canvas = app.session.doc.canvases.iter().find(|c| c.id == id);
    let visible = canvas.is_some_and(|c| c.visible);
    // A locked canvas (#12310) is not moved, calibrated or deleted until it is unlocked.
    let locked = canvas.is_some_and(|c| c.locked);
    let why = locked.then(|| solidcraft_ui_text::tr("browser.canvas_locked").to_string());
    vec![
        Item::action("ui.editCanvas", "Edit Canvas", "canvas").with(json!({ "canvas": id })).off_because(why.clone()),
        Item::action("ui.calibrateCanvas", "Calibrate", "measure").with(json!({ "canvas": id })).off_because(why.clone()),
        Item::action("ui.straightenCanvas", "Straighten", "canvas").with(json!({ "canvas": id })).off_because(why.clone()),
        Item::action("canvas.to_sketch", "Canvas to Sketch", "canvas").with(json!({ "canvas": id })),
        Item::sep(),
        Item::action("ui.delete", "Delete", "delete").with(json!({ "canvases": [id] })).off_because(why),
        Item::action("ui.rename", "Rename", "rename").key("F2").with(json!({ "canvas": id })),
        Item::action("ui.canvasLocked", if locked { "Unlock" } else { "Lock" }, "ground").with(json!({ "canvas": id, "locked": !locked })),
        Item::sep(),
        Item::action("ui.canvasVisible", "Show/Hide", "eye").key("V").with(json!({ "canvas": id, "visible": !visible })),
    ]
}

/// Editing a canvas (position, size, angle, opacity, flip), calibrating it (two points on
/// the image and their true distance) or straightening it (two points along an edge of the
/// image and the direction that edge should have, #11989). Live edits of one opening are one
/// undo step.
#[derive(Clone, Debug)]
pub struct CanvasPanel {
    pub canvas: u64,
    pub calibrate: bool,
    pub straighten: bool,
    /// Undo depth when the panel opened.
    pub(crate) depth: usize,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub distance: f64,
    /// Straighten: the line A–B becomes horizontal (`0`), vertical (`1`) or `angle` (`2`).
    pub direction: usize,
    /// Straighten: the direction for `direction == 2`, in degrees.
    pub angle: f64,
    /// Next viewport click sets Point A (`0`) or Point B (`1`) (#9108).
    pub next: u8,
    /// A click on the image has set Point B: Straighten's hint then asks for the direction and
    /// the Straighten button rather than for Point A again (#11989).
    pub picked_both: bool,
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
        CanvasPanel {
            canvas,
            calibrate,
            straighten: false,
            depth: app.session.undo.len(),
            a,
            b,
            distance: d,
            direction: 0,
            angle: 0.0,
            next: 0,
            picked_both: false,
        }
    }

    /// The Straighten card: pick two points along an edge of the image, then the direction.
    pub fn straighten(app: &SolveApp, canvas: u64) -> Self {
        CanvasPanel { straighten: true, ..CanvasPanel::new(app, canvas, false) }
    }

    /// Does a click on the image set Point A or B (Calibrate and Straighten)?
    pub fn picks_points(&self) -> bool {
        self.calibrate || self.straighten
    }

    /// The direction, in degrees, Straighten turns the line A–B to.
    pub fn target_angle(&self) -> f64 {
        match self.direction {
            0 => 0.0,
            1 => 90.0,
            _ => self.angle,
        }
    }
}

/// Does the open dialog's active input take bodies (Combine's target and tools, #4452;
/// Move/Copy's bodies, #4771; Shell's Faces/Body for a closed hollow, #794)? Then a body
/// clicked in the Browser goes there, as a click on it in the view does, rather than
/// replacing the selection: overlapping bodies are often only told apart in the Browser.
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
    // The chat's picker takes a body into Bodies whichever list is active (#11318).
    if let (Sel::Body { .. }, Some(d)) = (&x, app.dialog.as_mut()) {
        d.activate_input_taking(crate::selection::BODIES);
    }
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
    if let Sel::Axis { name } = &x
        && app.dialog.is_none()
        && crate::tools::pick_axis_from_browser(app, name)
    {
        return;
    }
    let hit = match &x {
        Sel::Plane { name } => crate::viewport::Hit::Plane { name: name.clone(), point: solidcore_record::Vec3::ZERO },
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
            // A part the modelling kernel refuses says so (Repair Part is in its menu).
            label: if b.body.smlib_refused().is_some() {
                solidcraft_ui_text::format("browser.needs_repair", &[("name", &b.name)])
            } else {
                b.name.clone()
            },
            icon,
            tip: Some(tip),
            visible: app.body_visible(&world),
            selected: app.session.selection.iter().any(|s| matches!(s, Sel::Body { name } if name == &world)),
            color: b.body.smlib_refused().map(|_| Tokens::get().warning),
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
                solidcraft_ui_text::format("browser.sketch_attachment_lost", &[("name", &crate::i18n::tr_name(&f.name))])
            } else if active {
                solidcraft_ui_text::format("browser.sketch_editing", &[("name", &crate::i18n::tr_name(&f.name))])
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

/// What a drag of a component folder's rows carries in the panel: the folder, and its items or
/// groups.
#[derive(Clone, Debug)]
struct ItemDrag {
    comp: u64,
    folder: &'static str,
    keys: Vec<String>,
    /// Groups dragged by their rows (alone: a drag carries items or groups).
    groups: Vec<u64>,
}

impl ItemDrag {
    /// What is dragged, as the browser commands name it.
    fn entries(&self) -> Vec<BrowserEntry> {
        self.keys.iter().map(|k| BrowserEntry::item(k)).chain(self.groups.iter().map(|g| BrowserEntry::Group(*g))).collect()
    }
}

/// What a drag of a placement row carries in the panel: the exact path it sits at, and the
/// component it places.
#[derive(Clone, Debug)]
struct PlacementDrag {
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
    rows: &mut Rows<'_>,
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
    // A search lists the folders that hold a match (`folder_level` lists the matches).
    let searching = app.tree.search.on();
    if app.tree.search.hits().is_some_and(|h| !h.folder(path.unwrap_or(&[]), folder)) {
        return;
    }
    let fstatic = folder_static(folder);
    let key = &data.key;
    let prefix = key.rsplit_once('/').map_or(key.as_str(), |(prefix, _)| prefix);
    // A folded folder draws no row: its groups and items move up into its place, and their own
    // eyes, hover and drops do its jobs. It is always open, as nothing could open it again.
    let render = crate::render_workspace::active(app);
    let folded = !render && folder == "bodies" && bodies_folded(app, comp, prefix, data.count);
    let start_open = rows_start_open(app);
    // With the history in the browser the sketches are there too: their folder starts closed.
    let folder_start_open = start_open && !(folder == "sketches" && app.preferences.history_in_browser);
    let open = folded || is_open(app, key, folder_start_open);
    let inner = if folded { depth } else { depth + 1 };
    // The folder's eye is its own: hiding it hides its items without changing theirs, and its
    // items never change it. It and everything in it fade while it or a placement above is
    // hidden, whatever their own eyes say.
    let shown = !app.session.folder_hidden(key);
    let folder_faded = !shown || path.is_some_and(|p| !app.session.occurrence_visible(p));
    if !render {
        // Dropping on the folder row takes items out of their groups, to the end. Bodies or sketches
        // from another component's folder move into this component, as a drop on its row does
        // (#3270: the Bodies folder looked like the place to drop a body, and took nothing).
        let accept = |d: &Dragged, zone: Zone| {
            zone == Zone::Into
                && same_document(app, d)
                && item_drag(d).is_some_and(|d| d.folder == fstatic && (d.comp == comp || (d.folder != "construction" && d.groups.is_empty())))
        };
        let r = if folded {
            Hit::default()
        } else {
            node_row(
                rows,
                Spec { key, drops: panel::Drops { before: false, into: true }, accept: Some(&accept), ..Default::default() },
                Row {
                    depth,
                    fold: Some(open),
                    eye: Some(shown),
                    icon: "folder",
                    label: folder_label(folder),
                    dim: folder_faded || searching,
                    mark: if acts.lit.any() { acts.lit.branch(app, data.entries()) } else { Mark::None },
                    ..Default::default()
                },
            )
        };
        if r.fold {
            toggle(app, key, folder_start_open);
        } else if r.clicked {
            // The Bodies label is the whole set while a command is asking for bodies. The arrow
            // still folds it. With no such command, the label folds, as it always has.
            let names: Vec<String> = data.entries().flat_map(|e| e.bodies.clone()).collect();
            if !offer_bodies(app, &names, Take::Toggle) {
                pick_folder(app, key);
                toggle(app, key, folder_start_open);
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
        if let Some((d, _, source)) = r.dropped_as::<ItemDrag>() {
            acts.drop_from(
                source,
                if d.comp == comp {
                    DropAction::Order {
                        comp,
                        folder: fstatic,
                        keys: d.keys.clone(),
                        groups: d.groups.clone(),
                        order: order_before(&data.top_order, &d.entries(), None),
                    }
                } else {
                    DropAction::Component { to: comp, folder: d.folder, keys: d.keys.clone() }
                },
            );
        }
        if !open {
            return;
        }
    }
    folder_level(app, rows, comp, path, fstatic, prefix, &data.rows, None, inner, &data.top_order, folder_faded, acts);
}

/// The rows of a folder's top level, or of one group (`holder`), at `depth`; groups draw their
/// own rows the same way one level deeper. `top` is the folder's top-level order as drawn.
#[allow(clippy::too_many_arguments)]
fn folder_level(
    app: &mut SolveApp,
    rows: &mut Rows<'_>,
    comp: u64,
    path: Option<&[u64]>,
    fstatic: &'static str,
    prefix: &str,
    level: &[cache::Row],
    holder: Option<u64>,
    depth: usize,
    top: &[BrowserEntry],
    faded: bool,
    acts: &mut Actions,
) {
    let render = crate::render_workspace::active(app);
    let start_open = rows_start_open(app);
    let searching = app.tree.search.on();
    let at = path.unwrap_or(&[]);
    // The render workspace has always listed a folder's loose items above its groups.
    let items_first = render && holder.is_none();
    let ordered = level
        .iter()
        .filter(|r| items_first && matches!(r, cache::Row::Item(_)))
        .chain(level.iter().filter(|r| !items_first || matches!(r, cache::Row::Group(_))));
    for row in ordered {
        let group = match row {
            cache::Row::Item(e) => {
                if app.tree.search.hits().is_none_or(|h| h.row(at, fstatic, &e.key)) {
                    entry_row(app, rows, comp, path, fstatic, e, depth, holder, top, faded, acts);
                }
                continue;
            }
            cache::Row::Group(g) => g,
        };
        if app.tree.search.hits().is_some_and(|h| !group.any_entry(&|e| h.row(at, fstatic, &e.key))) {
            continue;
        }
        let (id, gkey) = (group.id, &group.key);
        let gopen = is_open(app, gkey, start_open);
        let gshown = !app.session.folder_hidden(gkey);
        let group_faded = faded || !gshown;
        let label = if render {
            app.session.doc.browser_groups.iter().find(|g| g.id == id).map_or(group.label.as_str(), |g| g.name.as_str())
        } else {
            &group.label
        };
        let count =
            if render && !gopen { solidcraft_ui_text::format("render.tree_more", &[("count", &group.count.to_string())]) } else { String::new() };
        // The top of the row puts the drop before the group; the rest puts it inside.
        let level_of = app.session.doc.group_level(id);
        let fits = |d: &ItemDrag, parent_level: usize| {
            let doc = &app.session.doc;
            d.groups.iter().all(|g| !doc.group_within(id, *g) && parent_level + doc.group_height(*g) <= solidcraft_engine::doc::MAX_GROUP_NESTING)
        };
        let accept = |d: &Dragged, zone: Zone| {
            same_document(app, d)
                && item_drag(d)
                    .is_some_and(|d| d.comp == comp && d.folder == fstatic && fits(d, if zone == Zone::Before { level_of - 1 } else { level_of }))
        };
        let r = node_row(
            rows,
            // Groups don't drag during a search: it shows only part of a folder, and a new order needs every item.
            Spec {
                key: gkey,
                drag: (!searching).then_some(DRAG_ITEMS),
                drops: panel::Drops { before: true, into: true },
                accept: Some(&accept),
                ..Default::default()
            },
            Row {
                depth,
                fold: Some(gopen),
                eye: (!render || gopen).then_some(gshown),
                icon: if render { "" } else { "folder" },
                label,
                name: true,
                rename: true,
                dim: group_faded || (render && !gopen) || searching,
                count: &count,
                mark: if acts.lit.any() { acts.lit.branch(app, group.entries()) } else { Mark::None },
                ..Default::default()
            },
        );
        // Dragging a group row moves the group; a press on its arrow or eye is not a drag.
        if r.drag_started {
            rows.drag_with(vec![node_id(gkey)], Some(Payload::new(ItemDrag { comp, folder: fstatic, keys: Vec::new(), groups: vec![id] })));
        }
        // With a dialog open a second click picks again: the dialog takes the group's bodies.
        if r.slow_rename && app.dialog.is_none() {
            start_inline_rename(app, crate::context_menu::RenameWhat::Group(id), r.name_at());
        } else if r.fold {
            toggle(app, gkey, start_open);
        } else if r.clicked {
            let names: Vec<String> = group.entries().into_iter().flat_map(|e| e.bodies.clone()).collect();
            if !offer_bodies(app, &names, Take::Toggle) {
                pick_folder(app, gkey);
                toggle(app, gkey, start_open);
            }
        }
        inline_rename(app, rows.ui(), r.name_at(), &group.label);
        if r.eye {
            toggle_folder(app, gkey);
        }
        if r.hovered {
            acts.hover(group.entries().into_iter().flat_map(|e| &e.bodies));
        }
        if let Some(p) = r.secondary {
            acts.menu = Some((p, Target::Group { id, scope: prefix.to_owned() }));
        }
        if let Some((d, zone, source)) = r.dropped_as::<ItemDrag>() {
            let before = BrowserEntry::Group(id);
            acts.drop_from(
                source,
                match (zone, holder) {
                    (Zone::Into, _) => {
                        DropAction::Move { comp, folder: fstatic, keys: d.keys.clone(), groups: d.groups.clone(), group: Some(id), before: None }
                    }
                    (Zone::Before, Some(h)) => DropAction::Move {
                        comp,
                        folder: fstatic,
                        keys: d.keys.clone(),
                        groups: d.groups.clone(),
                        group: Some(h),
                        before: Some(before),
                    },
                    (Zone::Before, None) => DropAction::Order {
                        comp,
                        folder: fstatic,
                        order: order_before(top, &d.entries(), Some(&before)),
                        keys: d.keys.clone(),
                        groups: d.groups.clone(),
                    },
                },
            );
        }
        if gopen {
            folder_level(app, rows, comp, path, fstatic, prefix, &group.rows, Some(id), depth + 1, top, group_faded, acts);
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

pub(crate) fn start_inline_rename(app: &mut SolveApp, what: crate::context_menu::RenameWhat, row: NameAt) {
    if let Some(row_id) = row.row_id {
        crate::context_menu::start_inline_rename(app, what, row.rect.map(|rect| rect.left_bottom()).unwrap_or_default(), row_id);
    }
}

/// Edit a browser name in its row: Enter or leaving the box keeps the name, Escape cancels.
pub(crate) fn inline_rename(app: &mut SolveApp, ui: &mut egui::Ui, row: NameAt, label: &str) {
    let (Some(edit_rect), Some(row_id)) = (row.label_rect, row.row_id) else { return };
    let editing = app.menu.rename.as_ref().is_some_and(|rename| rename.inline && rename.inline_id == Some(row_id));
    if !editing {
        return;
    }
    let Some(mut rename) = app.menu.rename.take() else { return };
    ui.painter().rect_filled(edit_rect, 3.0, Tokens::get().field);
    let id = ui.scope_id().with(("browser_rename", row_id));
    let response = crate::ui_kit::inline_name(ui, edit_rect, id, &mut rename.text);
    crate::panel_runtime::register_content_focus(ui.ctx(), &response);
    crate::scenario::publish_handle(&format!("rename:{label}"), edit_rect.center());
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
        if crate::context_menu::commit_rename(app, &rename).is_err() {
            app.menu.rename = Some(rename);
        }
    } else {
        app.menu.rename = Some(rename);
    }
}

/// One item row of a folder.
#[allow(clippy::too_many_arguments)]
fn entry_row(
    app: &mut SolveApp,
    rows: &mut Rows<'_>,
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
    // A search lists what it found and what holds it. The rows a row folds open on that aren't
    // searched (a sketch's patterns, a body's features) show neither them nor their fold until the
    // search ends.
    let unsearched = !app.tree.search.on();
    let pattern_key = e.patterns.as_deref().filter(|_| unsearched);
    let start_open = rows_start_open(app);
    let patterns_open = pattern_key.is_some_and(|key| is_open(app, key, start_open));
    let revealed = folder == "bodies" && body_reveal(app).is_some_and(|b| e.row_key.strip_prefix("b:") == Some(b));
    // Features under bodies (#11945): the body row folds open on the features that built it.
    let built_key = (folder == "bodies"
        && unsearched
        && app.preferences.browser_body_features
        && !crate::render_workspace::active(app)
        && matches!(e.sel, Some(Sel::Body { .. })))
    .then(|| format!("{}/features", e.row_key));
    let built_open = built_key.as_deref().is_some_and(|key| is_open(app, key, false));
    if !rows.next_visible() && !patterns_open && !revealed && !built_open {
        rows.skip(node_id(&e.row_key), Selectable::Yes(0));
        return;
    }
    let built: Vec<u64> = match &built_key {
        Some(_) => {
            prepare_model(app);
            app.tree.model.as_mut().map(|model| model.body_features(&app.session, &e.key)).unwrap_or_default()
        }
        None => Vec::new(),
    };
    let built_key = built_key.filter(|_| !built.is_empty());
    let (selected, mark) = acts.lit.row(app, e);
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
    let ink = sketch_state.and_then(|state| state.ink(&Tokens::get()));
    // Items of this folder dropped on the row go before it (in its group, or the folder's order).
    let fits = |d: &ItemDrag| {
        let doc = &app.session.doc;
        let level = group.map_or(0, |g| doc.group_level(g));
        d.groups
            .iter()
            .all(|g| group.is_none_or(|h| !doc.group_within(h, *g)) && level + doc.group_height(*g) <= solidcraft_engine::doc::MAX_GROUP_NESTING)
    };
    let accept = |d: &Dragged, zone: Zone| {
        zone == Zone::Before
            && same_document(app, d)
            && item_drag(d).is_some_and(|d| d.comp == comp && d.folder == folder && !d.keys.contains(&e.key) && fits(d))
    };
    let r = node_row(
        rows,
        // Items don't drag during a search, as groups don't (`folder_level`).
        Spec {
            key: &e.row_key,
            item: true,
            drag: (!app.tree.search.on()).then_some(DRAG_ITEMS),
            drops: panel::Drops { before: true, into: false },
            accept: Some(&accept),
        },
        Row {
            depth,
            fold: pattern_key.map(|_| patterns_open).or(built_key.as_ref().map(|_| built_open)),
            eye: Some(visible),
            icon: e.icon,
            kind_tip: e.tip,
            label: &e.label,
            name: true,
            rename: true,
            selected,
            dim: !visible || parent_hidden,
            cut,
            color: e.color,
            swatch: appearance.swatch,
            secondary: &appearance.name,
            count: &appearance.count,
            icon_ink: ink,
            tip: state_tip.as_deref(),
            badges,
            mark,
            ..Default::default()
        },
    );
    // Drag: the selected items of this folder when this one is selected (in closed groups too),
    // else just this one.
    if r.drag_started {
        let keys: Vec<String> = if selected { selected_keys(app, comp, folder) } else { vec![e.key.clone()] };
        rows.drag_with(vec![node_id(&e.row_key)], Some(Payload::new(ItemDrag { comp, folder, keys, groups: Vec::new() })));
    }
    // A part that needs repair, under the pointer: its refused face is ringed in the view.
    if r.hovered
        && folder == "bodies"
        && let Some(Sel::Body { name }) = &e.sel
        && let Some(face) = app.session.model.state().body(&e.key).and_then(|b| b.body.smlib_refused()).and_then(|r| r.face)
    {
        crate::refused_part::ring(app, name, face);
    }
    if r.fold
        && let Some(key) = pattern_key
    {
        toggle(app, key, start_open);
    } else if r.fold
        && let Some(key) = &built_key
    {
        toggle(app, key, false);
    }
    if folder == "bodies"
        && body_reveal(app).is_some_and(|b| e.row_key.strip_prefix("b:") == Some(b))
        && let Some(rect) = r.rect
        && crate::semantic_content::scroll_acknowledged(rows.ui(), rect)
    {
        app.tree.reveal = None;
    }
    if r.eye {
        set_visible(app, folder, &[e], !visible);
    }
    if r.double
        && folder == "sketches"
        && let Ok(id) = e.key.parse()
    {
        app.edit_sketch_in(id, path.map(|path| (comp, path)));
    }
    // A click a dialog takes as a pick (below) leaves the selection as it was: nothing to rename.
    // With no dialog open, a construction row is selected like any other and renames.
    let shift = rows.ui().input(|i| i.modifiers.shift);
    let mut dialog_pick = r.clicked && app.dialog.is_some() && (folder == "construction" || (folder == "bodies" && !shift));
    if r.clicked && !(r.double && folder == "sketches") {
        // A body row with a dialog open is a pick, as Combine does (#4452), unless Shift is
        // asking for the rows between this one and the anchor.
        if folder == "construction" || (folder == "bodies" && !shift && app.dialog.is_some()) {
            if let Some(s) = e.sel.clone() {
                pick_from_browser(app, s);
            }
        } else if folder == "sketches"
            && !rows.ui().input(|i| i.modifiers.any())
            && let Some(Sel::Feature { id }) = e.sel
            && pick_sketch_into_dialog(app, id)
        {
            // Extrude, Revolve or Emboss took all the sketch's profiles (#6106).
            dialog_pick = true;
        } else {
            click_select(app, r.how.as_ref(), e.sel.clone());
        }
        // A double-click or a slower second click renames the row (sketch double-clicks edit
        // instead), like F2.
        if (r.double || r.slow_rename) && !dialog_pick {
            crate::context_menu::rename_selection(app, r.rect.map(|x| x.left_bottom()).unwrap_or_default());
            if let Some(rename) = app.menu.rename.as_mut()
                && let Some(row_id) = r.row_id
            {
                rename.inline = true;
                rename.inline_id = Some(row_id);
            }
        }
    }
    inline_rename(app, rows.ui(), r.name_at(), &e.label);
    if r.hovered && !e.bodies.is_empty() {
        acts.hover(e.bodies.iter());
    }
    if r.hovered
        && let Thing::Plane(name) = &e.thing
    {
        acts.hover_plane = Some(name.clone());
    }
    if r.hovered
        && let Thing::Sketch(id) = &e.thing
    {
        acts.hover_feature = Some(*id);
        acts.hovered = true;
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
        crate::sketch_pattern_tree::browser_rows(app, rows, *id, depth + 1, path.map(|path| (comp, path)), !visible || parent_hidden);
    }
    if built_open && built_key.is_some() {
        features::body_rows(app, rows, acts, comp, path.unwrap_or_default(), &e.row_key, &built, depth + 1);
    }
    if let Some((d, _, source)) = r.dropped_as::<ItemDrag>() {
        let before = BrowserEntry::item(&e.key);
        acts.drop_from(
            source,
            match group {
                Some(g) => DropAction::Move { comp, folder, keys: d.keys.clone(), groups: d.groups.clone(), group: Some(g), before: Some(before) },
                None => DropAction::Order {
                    comp,
                    folder,
                    order: order_before(top, &d.entries(), Some(&before)),
                    keys: d.keys.clone(),
                    groups: d.groups.clone(),
                },
            },
        );
    }
}

/// Keys of the selected items of a component folder.
fn selected_keys(app: &SolveApp, comp: u64, folder: &str) -> Vec<String> {
    entries(app, comp, folder).into_iter().filter(|e| e.selected).map(|e| e.key).collect()
}

/// Whether a placement dragged from `d` may land in component `parent`: not where it already is,
/// and not in itself or a component it holds. The command refuses those too; this keeps the row
/// from showing a drop mark it would refuse.
fn occurrence_can_land(app: &SolveApp, d: &PlacementDrag, parent: u64) -> bool {
    let Some(id) = d.path.last().copied() else { return false };
    let Some(o) = app.session.doc.occurrences.iter().find(|o| o.id == id) else { return false };
    if parent == 0 {
        // The design row takes any placement not already at the design root: there is no cycle
        // to make at the top, and everything is inside the design.
        return o.parent != 0;
    }
    o.parent != parent && !app.session.doc.inside_component(parent, d.component)
}

/// The component a placement shares with the placement under it, when both sit in the same one
/// (and are two placements): the parent whose sub-components they are ordered among.
fn occurrence_sibling(app: &SolveApp, d: &PlacementDrag, target: u64) -> Option<u64> {
    let dragged = d.path.last().copied().filter(|o| *o != target)?;
    let occurrences = &app.session.doc.occurrences;
    let parent = occurrences.iter().find(|o| o.id == dragged)?.parent;
    occurrences.iter().any(|o| o.id == target && o.parent == parent).then_some(parent)
}

/// A drop, applied after the tree is drawn.
enum DropAction {
    OccurrenceOrder {
        parent: u64,
        order: Vec<u64>,
    },
    /// Into a group (before an item of it), or out of groups.
    Move {
        comp: u64,
        folder: &'static str,
        keys: Vec<String>,
        groups: Vec<u64>,
        group: Option<u64>,
        before: Option<BrowserEntry>,
    },
    /// Out of groups, in this order of the folder's top level.
    Order {
        comp: u64,
        folder: &'static str,
        keys: Vec<String>,
        groups: Vec<u64>,
        order: Vec<BrowserEntry>,
    },
    /// Into another component (bodies and sketches).
    Component {
        to: u64,
        folder: &'static str,
        keys: Vec<String>,
    },
    /// A placement moved into another component (or the design, `parent` 0).
    Reparent {
        occurrence: u64,
        path: Vec<u64>,
        parent: u64,
        parent_path: Option<Vec<u64>>,
    },
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
        DropAction::OccurrenceOrder { parent, order } => {
            let order: Vec<String> = order.iter().map(u64::to_string).collect();
            let _ = app.run("browser.order", json!({ "folder": "components", "component": parent, "order": order }));
        }
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
                "sketches" => {
                    // Ids, as numbers: a string is a sketch's name first, and a sketch may be named like another one's id.
                    let ids: Vec<u64> = keys.iter().filter_map(|k| k.parse().ok()).collect();
                    app.run("component.move_sketches", json!({ "sketches": ids, "component": to }))
                }
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

/// The middle of what a moved occurrence places, in the world, where its triad sits and it turns.
/// Its component's origin can be anywhere, far from the part. `None` when it places nothing.
pub(crate) fn occurrence_centre(app: &SolveApp, m: &OccMove) -> Option<solidcore_record::Vec3> {
    let bounds = if m.path.is_empty() {
        crate::dialogs_assembly::occurrence_bounds(&app.session, &[m.occurrence])
    } else {
        let names = app.session.occurrence_bodies_shared(&m.path, true).ok()?;
        let world = crate::open_meshing::session_world(&app.session);
        names.iter().filter_map(|n| world.body(n)).fold(solidcraft_engine::geom::Aabb3::EMPTY, |bb, b| bb.union(&b.mesh().bounds()))
    };
    (!bounds.is_empty()).then(|| bounds.center())
}

/// The pending move: turned by the angles in the order they were dragged (about X, then Y, then
/// Z at first) through the occurrence's centre, then moved.
pub fn apply_occurrence_move(app: &mut SolveApp, m: &OccMove) {
    use crate::gizmo;
    use solidcore_record::Vec3;
    let (axis, angle) = gizmo::axis_angle(&gizmo::turn_of(m.angles.map(f64::to_radians), m.order));
    let o = m.pivot.unwrap_or(Vec3::ZERO);
    let baseline = m.base.or(m.baseline).or_else(|| app.session.doc.occurrences.iter().find(|o| o.id == m.occurrence).map(|o| o.transform));
    let _ = app.run(
        "occurrence.move",
        if m.path.is_empty() {
            json!({ "occurrence": m.occurrence, "base_transform": baseline, "translate": m.translate, "axis": [axis.x, axis.y, axis.z], "angle": angle, "origin": [o.x, o.y, o.z], "capture": false })
        } else {
            json!({ "occurrence": m.occurrence, "path": m.path, "space": "world", "base_transform": baseline, "translate": m.translate, "axis": [axis.x, axis.y, axis.z], "angle": angle, "origin": [o.x, o.y, o.z], "capture": false })
        },
    );
}

/// The shared move triad at the occurrence's centre: arrows and squares move it, rings turn it.
pub(crate) fn occurrence_triad_model(app: &SolveApp) -> Option<crate::gizmo::Triad> {
    let movement = app.tree.occurrence_move.as_ref()?;
    let center = movement.pivot? + solidcore_record::Vec3::new(movement.translate[0], movement.translate[1], movement.translate[2]);
    Some(crate::gizmo::Triad { center, translate: true, rotate: true })
}

/// Occurrence handles belong to the viewport, not the clipped Move panel's child UI.
pub(crate) fn occurrence_triad(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &crate::viewport::Proj) {
    use crate::gizmo::{self, Change};
    let Some(mut m) = app.tree.occurrence_move.clone() else { return };
    let before = (m.translate, m.angles);
    let Some(t) = occurrence_triad_model(app) else { return };
    let step = crate::canvas::snap_step(app.cam.half_height());
    let Some(g) = gizmo::show(ui, painter, proj, egui::Id::unique("sc_occ_triad"), &t, step) else { return };
    if g.started || m.drag0.is_none() {
        m.drag0 = Some((m.translate, m.angles));
        m.ring_set = false;
    }
    match g.change {
        Change::Translate(v) => {
            let (t0, _) = m.drag0.unwrap_or((m.translate, m.angles));
            m.translate = [t0[0] + v.x, t0[1] + v.y, t0[2] + v.z];
        }
        // Not on the press: a click on a ring changes nothing. Once the ring has turned, a drag
        // back to where it started is a real zero.
        Change::Rotate { axis, angle } if angle != 0.0 || m.ring_set => {
            let i = axis.min(2);
            // The ring's first turn: let its angle add, or start over on the turns so far.
            if !m.ring_set {
                m.ring_set = true;
                match gizmo::ring_order(m.angles.map(f64::to_radians), m.order, i) {
                    Some(order) => m.order = order,
                    None => {
                        m.bake(app);
                        m.drag0 = Some((m.translate, m.angles));
                    }
                }
            }
            let (_, a0) = m.drag0.unwrap_or((m.translate, m.angles));
            m.angles[i] = gizmo::within_turn((a0[i] + angle.to_degrees()).to_radians()).to_degrees();
        }
        Change::Rotate { .. } => {}
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
    use solidcore_record::Vec2;
    use solidcraft_engine::sketch::{DimFrame, chain_centres, default_text, dim_layout_reaching, dim_reach};
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
            let Some(frame) = crate::dim_view::shown_frame(app.preferences.ellipse_axes, sk, &c.kind, c.text) else { continue };
            let anchor = match frame {
                DimFrame::Linear { p0, p1, .. } => (p0 + p1) * 0.5,
                DimFrame::Radial { center, .. } | DimFrame::ArcLength { center, .. } | DimFrame::HalfAxis { center, .. } => center,
                DimFrame::Angular { vertex, .. } => vertex,
            };
            let Some(px) = crate::dim_view::mm_per_pixel(to, anchor) else { continue };
            let expr = c.param.as_ref().and_then(|p| app.session.doc.param(p)).map(|p| p.expr.as_str());
            let col = if ss.report.failing.contains(&c.id) {
                crate::viewport::Conflict::glyph_colour(app, &tk)
            } else {
                crate::prefs::dimension_colour(&app.preferences, c.driven)
            };
            let galley = painter.layout_no_wrap(
                crate::dim_view::frame_label(&frame, &c.kind, expr, c.driven, &app.session.doc.units, app.preferences.length_decimals),
                FontId::proportional(tk.font_caption * crate::prefs::dimension_text_scale(&app.preferences)),
                col,
            );
            let size = galley.size() + vec2(6.0, 2.0);
            let half = Vec2::new(f64::from(size.x) * 0.5 * px, f64::from(size.y) * 0.5 * px);
            let lay = dim_layout_reaching(&frame, c.text.or_else(|| default_text(&frame, middle, px, half)), px, half, dim_reach(sk, &c.kind));
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

/// The Capture Position question stands for saving.
pub const SAVE: &str = "__save";

/// It stands for Save As (`__save_as:<extension>:<name>`), which needs the question as Save
/// does: a file can't hold a move not captured yet (#9736).
const SAVE_AS: &str = "__save_as:";

/// It stands for the browser build's Download design.
pub const DOWNLOAD: &str = "__download";

/// The question standing for Save As under `name` with `extension`.
pub fn save_as_question(name: &str, extension: &str) -> String {
    format!("{SAVE_AS}{extension}:{name}")
}

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
            // A tab waiting to close after the save stays open (#9736).
            crate::documents::saved_for_close(app);
            return Ok(());
        }
        other => return Err(format!("unknown answer `{other}` (capture, revert, cancel)")),
    }
    app.tree.capture_prompt = None;
    app.tree.occurrence_move = None;
    if what == SAVE {
        crate::toolbar::save(app);
    } else if let Some((extension, name)) = what.strip_prefix(SAVE_AS).and_then(|rest| rest.split_once(':')) {
        crate::toolbar::save_as_named(app, name, extension);
    } else if what == DOWNLOAD {
        crate::toolbar::download_design(app);
    } else {
        app.start(&what);
    }
    // A save that finished here (no thread, or nothing to write) answers a tab waiting to close,
    // as the "save changes?" answer does; one under way answers when it ends.
    if app.saving.is_none() && !app.file_picker_busy() {
        crate::documents::saved_for_close(app);
    }
    Ok(())
}

/// Where an occurrence's origin is now (with its pending move).
pub(crate) fn occurrence_origin(app: &SolveApp, occurrence: u64) -> Option<solidcore_record::Vec3> {
    let o = app.session.doc.occurrences.iter().find(|o| o.id == occurrence)?;
    let parent = app.session.doc.component_transform(o.parent);
    let own = app.session.pending_moves.get(&occurrence).copied().unwrap_or(o.transform);
    Some(solidcraft_engine::doc::apply_point(&solidcraft_engine::doc::mat_mul(&parent, &own), solidcore_record::Vec3::ZERO))
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
