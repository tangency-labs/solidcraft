//! Context menus, one look everywhere.
//!
//! The viewport's right-click opens a list of commands for the selection or the thing under the
//! cursor: faces, edges, bodies, sketch entities or empty space. The optional radial marking
//! menu adds eight commands around the cursor, with Repeat at the top. With that preference on,
//! holding the right button opens it too; releasing over a direction picks a command (a flick).
//! The browser's right-click
//! opens the same list styling for a body, sketch or component.
//!
//! Items are commands (by id, with parameters filled from the target) or `ui.*` actions on view
//! state; [`items`] lists them and [`run_item`] runs one, so the control channel (`ui.menu`,
//! `ui.menuPick`) and tests reach everything a click does.

use crate::ui_kit::{Container, Surface};
use egui::{Color32, Pos2, Rect, Sense, Vec2, pos2, vec2};
use serde::Serialize;
use serde_json::{Value, json};
use solidcraft_engine::Sel;

use crate::SolveApp;

/// What a menu is about.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    /// The viewport: the selection, or empty space.
    #[default]
    Viewport,
    Body {
        name: String,
    },
    Sketch {
        id: u64,
    },
    /// A construction plane (its feature id).
    Plane {
        id: u64,
    },
    /// A native feature row in a component's Features folder.
    Feature {
        id: u64,
    },
    /// An origin plane (XY, XZ or YZ).
    OriginPlane {
        name: String,
    },
    Component {
        id: u64,
    },
    Occurrence {
        path: Vec<u64>,
    },
    /// A browser group of items, under the design (`scope` `c0`) or a placement (`o:<path>`).
    Group {
        id: u64,
        scope: String,
    },
    /// The Origin folder.
    Origin,
    /// Document Settings › Units.
    Units,
    /// An assembly joint.
    Joint {
        id: u64,
    },
    /// A command (toolbar button, panel drop-down row, S box row).
    ToolbarCommand {
        id: String,
    },
    /// The toolbar's empty space.
    Toolbar,
    /// The workspace switcher.
    Workspace,
    /// The Named Views folder, and one view (saved or standard) in it.
    NamedViews,
    NamedView {
        name: String,
    },
    /// A canvas (reference image).
    Canvas {
        id: u64,
    },
    /// The Section Analysis in the browser's Analysis folder.
    Section,
    /// A component's Bodies, Sketches or Construction folder, under the design (`scope` `c0`)
    /// or a placement (`o:<path>`).
    Folder {
        component: u64,
        folder: String,
        scope: String,
    },
}

/// One menu entry: a command id (run with `params` when given, else started like a toolbar
/// click) or a `ui.*` action. `-` is a separator, `#…` a section heading.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swatch: Option<[u8; 3]>,
}

impl Item {
    /// A view action (`ui.*`).
    pub fn action(id: &str, label: &str, icon: &str) -> Item {
        act(id, label, icon)
    }
    pub fn sep() -> Item {
        Item {
            id: "-".into(),
            label: String::new(),
            shortcut: String::new(),
            enabled: false,
            icon: String::new(),
            params: None,
            children: Vec::new(),
            swatch: None,
        }
    }
    fn heading(label: &str) -> Item {
        Item {
            id: format!("#{label}"),
            label: label.into(),
            shortcut: String::new(),
            enabled: false,
            icon: String::new(),
            params: None,
            children: Vec::new(),
            swatch: None,
        }
    }
    fn is_sep(&self) -> bool {
        self.id == "-"
    }
    fn is_heading(&self) -> bool {
        self.id.starts_with('#')
    }
    pub fn with(mut self, params: Value) -> Item {
        self.params = Some(params);
        self
    }
    pub fn on_if(self, enabled: bool) -> Item {
        self.on(enabled)
    }
    pub(crate) fn on(mut self, enabled: bool) -> Item {
        self.enabled &= enabled;
        self
    }
    pub fn key(mut self, k: &str) -> Item {
        self.shortcut = k.into();
        self
    }
}

/// A command entry: label, icon, shortcut and enablement come from the registry.
pub(crate) fn cmd(app: &SolveApp, id: &str, label: &str) -> Item {
    let info = solidcraft_engine::find_command(id).map(|c| c.info(&app.session));
    let shortcut = match id {
        "edit.undo" => "Ctrl+Z".to_string(),
        "edit.redo" => "Ctrl+Y".to_string(),
        _ => crate::keymap::display_for(app, id).unwrap_or_default(),
    };
    Item {
        id: id.into(),
        label: label.into(),
        shortcut,
        enabled: info.as_ref().is_some_and(|i| i.enabled),
        icon: info.map(|i| i.icon.to_string()).unwrap_or_default(),
        params: None,
        children: Vec::new(),
        swatch: None,
    }
}

/// A view action (`ui.*`).
pub(crate) fn act(id: &str, label: &str, icon: &str) -> Item {
    Item {
        id: id.into(),
        label: label.into(),
        shortcut: String::new(),
        enabled: true,
        icon: icon.into(),
        params: None,
        children: Vec::new(),
        swatch: None,
    }
}

/// Menu state kept between frames. The anchor is `app.viewport.context_menu`.
#[derive(Default)]
pub struct MenuState {
    pub target: Target,
    /// The list row chosen with the arrow keys (Enter runs it).
    pub key_row: Option<usize>,
    submenu: Option<usize>,
    submenu_key: Option<usize>,
    /// The radial ring is shown (viewport menus).
    pub radial: bool,
    /// The right button is held through the menu: releasing over a direction picks it.
    pub flick: bool,
    /// A right press in the viewport that may become a press-and-hold: where and when.
    press: Option<(Pos2, f64)>,
    /// `ui.menu` asked to open at a point once the pointer got there.
    pending: Option<(Pos2, Target)>,
    /// An inline rename in progress.
    pub rename: Option<Rename>,
    /// A delete waiting for its confirmation (it takes or breaks other features).
    pub confirm: Option<crate::delete::Pending>,
    /// Identical copies Extract to File found, waiting to be linked too.
    pub link_copies: Option<crate::linked_source::Copies>,
    /// The Properties window: title and measurements.
    pub props: Option<(String, Value)>,
    /// The viewport menu opened over something: it offers Select Other.
    pub over_geometry: bool,
    /// The plane under the cursor when the viewport menu opened: it offers a sketch on it.
    pub plane: Option<String>,
    /// The button that opened the menu, when it toggles it: a press there is not a press outside,
    /// so its click closes the menu instead of closing and reopening it (#1522).
    opener: Option<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenameWhat {
    Body(String),
    Feature(u64),
    Component(u64),
    Group(u64),
    TimelineGroup(u64),
    Canvas(u64),
    View(String),
    Joint(u64),
}

#[derive(Clone, Debug)]
pub struct Rename {
    pub what: RenameWhat,
    pub text: String,
    pub at: Pos2,
    pub(crate) focused: bool,
}

/// Radial slots, clockwise from the top.
const DIRS: usize = 8;
/// Half axes of the ring of radial items.
const RX: f32 = crate::ui_kit::RADIAL_RADIUS;
const RY: f32 = crate::ui_kit::RADIAL_RADIUS;

// ---------------------------------------------------------------------------------------------
// What the menus offer
// ---------------------------------------------------------------------------------------------

fn sel_bodies(app: &SolveApp) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in &app.session.selection {
        if let Sel::Body { name } = s
            && !v.contains(name)
        {
            v.push(name.clone());
        }
    }
    v
}

/// The sketch a selected sketch curve or point belongs to. Geometry of a sketch that is not
/// being edited selects as `ExternalSketchCurve` or `ExternalSketchPoint`
/// (`viewport::hit_candidate`). Those two carry the owning sketch's feature id. The entities of
/// the sketch being edited are `SketchCurve` and `SketchPoint` instead. The sketch-editing menu
/// already covers that case.
fn sel_sketch(app: &SolveApp) -> Option<u64> {
    app.session.selection.iter().find_map(|s| match s {
        Sel::ExternalSketchCurve { sketch, .. } | Sel::ExternalSketchPoint { sketch, .. } => Some(*sketch),
        _ => None,
    })
}

/// Bodies the selection touches (bodies, and the bodies of selected faces and edges).
fn touched_bodies(app: &SolveApp) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in &app.session.selection {
        let b = match s {
            Sel::Body { name } => name,
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => body,
            _ => continue,
        };
        if !v.contains(b) {
            v.push(b.clone());
        }
    }
    v
}

fn body_feature(app: &SolveApp, body: &str) -> Option<u64> {
    app.session.world_state().body(body).map(|b| b.feature)
}

/// Bodies in a component and its subcomponents.
pub fn component_bodies(app: &SolveApp, id: u64) -> Vec<String> {
    let doc = &app.session.doc;
    app.session.model.state().bodies.iter().filter(|b| doc.component_within(doc.model_body_component(b), id)).map(|b| b.name.clone()).collect()
}

/// The eight radial commands, clockwise from the top.
pub fn radial_items(app: &SolveApp) -> Vec<Item> {
    let repeat = match &app.last_command {
        Some((_, label)) => act("ui.repeat", &format!("Repeat {label}"), "redo").key(crate::keymap::REPEAT_KEY),
        None => act("ui.repeat", "Repeat", "redo").key(crate::keymap::REPEAT_KEY).on(false),
    };
    let delete = act("ui.delete", "Delete", "delete").key("Del").on(!app.session.selection.is_empty());
    if app.session.active_sketch.is_some() {
        return vec![
            repeat,
            delete,
            cmd(app, "sketch.line", "Line"),
            cmd(app, "sketch.rectangle.two_point", "Rectangle"),
            act("ui.finishSketch", "Finish Sketch", "finish"),
            cmd(app, "sketch.circle.center", "Circle"),
            cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()),
            cmd(app, "sketch.dimension", "Sketch Dimension"),
        ];
    }
    vec![
        repeat,
        delete,
        cmd(app, "solid.press_pull", "Press Pull"),
        cmd(app, "solid.hole", "Hole"),
        cmd(app, "sketch.create", "Create Sketch"),
        cmd(app, "solid.extrude", "Extrude"),
        cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()),
        cmd(app, "solid.move", "Move/Copy"),
    ]
}

/// The list of a menu (under the ring for the viewport).
pub fn items(app: &SolveApp, target: &Target) -> Vec<Item> {
    let mut v = match target {
        Target::Viewport => viewport_items(app),
        Target::Body { name } => body_items(app, std::slice::from_ref(name), true),
        Target::Sketch { id } => sketch_items(app, *id),
        Target::Feature { id } => vec![
            act("ui.editFeature", "Edit Feature", "edit").with(json!({"feature":id})).on(app.session.doc.feature(*id).is_some()),
            act("ui.rename", "Rename", "rename").with(json!({"feature":id})).on(app.session.doc.feature(*id).is_some()),
            act("ui.delete", "Delete", "delete").key("Del").with(json!({"items": [{"type": "feature", "id": id}]})).on(app
                .session
                .doc
                .feature(*id)
                .is_some()),
            Item::sep(),
            act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({"feature":id})).on(app.session.doc.feature(*id).is_some()),
        ],
        Target::Plane { id } => plane_items(app, *id),
        Target::OriginPlane { name } => origin_plane_items(app, name),
        Target::Component { id } => component_items(app, *id),
        Target::Occurrence { path } => occurrence_items(app, path),
        Target::Group { id, scope } => crate::browser::group_items(app, *id, scope),
        Target::Folder { component, folder, scope } => crate::browser::folder_items(app, *component, folder, scope),
        Target::Canvas { id } => crate::browser::canvas_items(app, *id),
        Target::Origin => crate::browser::origin_items(app),
        Target::NamedViews => vec![act("ui.newView", "New Named View", "perspective")],
        Target::Section => crate::browser::section_items(app),
        Target::Joint { id } => joint_items(app, *id),
        Target::ToolbarCommand { id } => crate::toolbar_custom::command_items(app, id),
        Target::Toolbar => crate::toolbar_custom::toolbar_items(app),
        Target::Workspace => {
            let rendering = crate::render_workspace::active(app);
            let mut v = vec![
                act("ui.workspace.design", "Design", "ws_design").with(json!({"workspace":"design"})).key(if rendering { "" } else { "current" }),
                act("ui.workspace.render", "Render", "ws_render")
                    .with(json!({"workspace":"render"}))
                    .key(if rendering { "current" } else { "" })
                    .on(app.session.active_sketch.is_none() && app.tool.is_none() && app.dialog.is_none() && !app.file_load.busy()),
            ];
            for (w, icon) in [("Animation", "ws_animate"), ("Simulation", ""), ("Manufacture", ""), ("Drawing", "ws_drawing")] {
                v.push(act("ui.workspace", w, icon).key("coming later").on(false));
            }
            v
        }
        Target::Units => crate::prefs::UNITS
            .iter()
            .map(|u| {
                let item = cmd(app, "document.units", u).with(json!({ "units": u }));
                if app.session.doc.units == *u { item.key("current") } else { item }
            })
            .collect(),
        Target::NamedView { name } => crate::browser::view_items(app, name),
    };
    // Several items of the folder selected: they can be grouped.
    if let Some(p) = crate::browser::selection_group(app, target) {
        v.push(Item::sep());
        v.push(act("ui.groupSelected", "Group Selected", "folder").with(p));
    }
    for item in &mut v {
        if item.id != "ui.appearance" {
            continue;
        }
        let original = item.clone();
        let params = item.params.clone().unwrap_or_else(|| {
            let faces: Vec<_> = app
                .session
                .selection
                .iter()
                .filter_map(|s| match s {
                    Sel::Face { body, point, .. } => Some(json!({"body":body,"point":[point.x, point.y, point.z]})),
                    _ => None,
                })
                .collect();
            if faces.is_empty() { json!({"bodies": touched_bodies(app)}) } else { json!({"faces":faces}) }
        });
        let targets = if let Some(component) = params.get("component").and_then(Value::as_u64).filter(|id| *id != 0) {
            json!({"components": [component.to_string()]})
        } else {
            params
        };
        item.children.push(original);
        if ["bodies", "components", "faces"].iter().any(|key| targets.get(key).and_then(Value::as_array).is_some_and(|v| !v.is_empty())) {
            for look in crate::dialogs_appearance::builtin() {
                let mut params = targets.clone();
                params["appearance"] = json!(look.name);
                params["color"] = json!(crate::dialogs_appearance::hex(look.color));
                params["opacity"] = json!(look.opacity);
                let mut child = cmd(app, "appearance.assign", &look.name).with(params);
                child.swatch = Some(look.color);
                item.children.push(child);
            }
            item.children.push(Item::sep());
            item.children.push(cmd(app, "appearance.assign", "Remove override").with(targets));
        }
    }
    v
}

/// Resolve the picked face's creator, rather than the feature that first made its body.
/// Persistent face names retain their creator through later trims and placements.
fn picked_face_feature(app: &SolveApp) -> Option<u64> {
    let (body, index) = app.session.selection.iter().find_map(|s| match s {
        Sel::Face { body, index, .. } => Some((body, *index)),
        _ => None,
    })?;
    let state = app.session.world_state();
    let body = state.body(body)?;
    let names = solidcraft_engine::doc::naming::face_names(body);
    let name = names.get(index)?;
    let (creator, _) = name.strip_prefix('F')?.split_once(':')?;
    let id = creator.parse::<u64>().ok()?;
    let owner = app.session.doc.feature(body.feature)?;
    if matches!(owner.kind, solidcraft_engine::doc::FeatureKind::Import { .. } | solidcraft_engine::doc::FeatureKind::MeshImport { .. })
        && id != owner.id
    {
        return None;
    }
    app.session.doc.feature(id).filter(|f| f.component == app.session.doc.model_body_component(body)).map(|f| f.id)
}

/// A feature and its profile sketch, if it has one.
fn feature_and_sketch(app: &SolveApp, f: Option<u64>) -> (Option<u64>, Option<u64>) {
    let sketch = f
        .and_then(|f| app.session.doc.feature(f))
        .and_then(|f| serde_json::to_value(&f.kind).ok())
        .and_then(|v| v.get("sketch").and_then(Value::as_u64).or_else(|| v.as_object()?.values().find_map(|x| x.get("sketch")?.as_u64())));
    (f, sketch)
}

/// The canvas menus put Repeat on
/// top, then what fits the selection, our extras after a separator.
fn viewport_items(app: &SolveApp) -> Vec<Item> {
    let sel = &app.session.selection;
    let has = |f: fn(&Sel) -> bool| sel.iter().any(f);
    let mut v = Vec::new();
    if let Some((_, label)) = &app.last_command {
        v.push(act("ui.repeat", &format!("Repeat {label}"), "redo").key(crate::keymap::REPEAT_KEY));
        v.push(Item::sep());
    }
    // Everything under the cursor, for what lies beneath something else.
    if app.menu.over_geometry {
        v.push(act("ui.selectOther", "Select Other…", "select").key("Hold click"));
        v.push(Item::sep());
    }
    if app.session.active_sketch.is_some() {
        v.extend(sketch_entity_items(app));
        v.push(Item::sep());
        v.push(cmd(app, "sketch.trim", "Trim"));
        v.push(cmd(app, "sketch.offset", "Offset"));
        v.push(Item::sep());
        v.push(act("ui.finishSketch", "Finish Sketch", "finish"));
        v.push(act("ui.cancelSketch", "Cancel Sketch", "sketch_cancel"));
        v.push(Item::sep());
        v.push(cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()));
        v.push(cmd(app, "edit.redo", "Redo").on(!app.session.redo.is_empty()));
        return v;
    }
    // A sketch curve or point in the viewport gets the sketch's own menu, the one its browser row
    // opens, with Edit Sketch among it. Solid geometry in the selection keeps the body menu: a
    // right-click on a face still offers the face's commands. The picker already prefers a sketch
    // curve to the face it lies on (`pick_hits` offers curves before faces). A right-click that
    // resolved to a sketch curve therefore replaces any face in the selection with that curve.
    // The menu then follows the pick.
    let solid = has(|s| matches!(s, Sel::Face { .. } | Sel::Edge { .. } | Sel::Vertex { .. } | Sel::Body { .. }));
    if let Some(sketch) = sel_sketch(app).filter(|_| !solid) {
        v.extend(sketch_items(app, sketch));
    } else if has(|s| matches!(s, Sel::Face { .. })) {
        let face = sel.iter().find_map(|s| if let Sel::Face { body, .. } = s { Some(body.clone()) } else { None }).unwrap_or_default();
        let (feature, sketch) = feature_and_sketch(app, picked_face_feature(app));
        let hidden = app.session.visibility.hidden_bodies.contains(&face);
        if app.preferences.nav == "solidworks" {
            v.push(cmd(app, "view.normal_to", "Normal to Selection").with(json!({"camera": app.cam})));
            v.push(Item::sep());
        }
        v.push(cmd(app, "sketch.create", "Create Sketch"));
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "construct.plane.offset", "Offset Plane"));
        v.push(cmd(app, "solid.shell", "Shell"));
        v.push(act("ui.editFeature", "Edit Feature", "edit").with(json!({ "feature": feature })).on(feature.is_some()));
        v.push(act("ui.editSketch", "Edit Profile Sketch", "sketch").with(json!({ "sketch": sketch })).on(sketch.is_some()));
        v.push(Item::sep());
        v.push(act("ui.appearance", "Appearance", "appearance").key("A"));
        v.push(act("ui.properties", "Properties", "measure").with(json!({ "bodies": [face] })));
        v.push(act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.delete", "Delete", "delete").key("Del"));
        v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.findBrowser", "Find in Browser", "list").with(json!({ "body": face, "feature": feature })).on(feature.is_some()));
        v.push(act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": feature })).on(feature.is_some()));
        v.push(Item::sep());
        v.push(cmd(app, "solid.press_pull", "Press Pull"));
        v.push(cmd(app, "inspect.measure", "Measure"));
    } else if has(|s| matches!(s, Sel::Edge { .. })) {
        let body = sel.iter().find_map(|s| if let Sel::Edge { body, .. } = s { Some(body.clone()) } else { None }).unwrap_or_default();
        let hidden = app.session.visibility.hidden_bodies.contains(&body);
        v.push(cmd(app, "solid.fillet", "Fillet"));
        v.push(cmd(app, "solid.chamfer", "Chamfer"));
        v.push(Item::sep());
        v.push(act("ui.delete", "Delete", "delete").key("Del"));
        v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": [body] })));
        v.push(Item::sep());
        v.push(act("ui.findBrowser", "Find in Browser", "list").with(json!({ "body": body })));
        v.push(act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": [body] })));
        v.push(Item::sep());
        v.push(act("ui.tangentChain", "Select Tangent Chain", "chain"));
        v.push(cmd(app, "inspect.measure", "Measure"));
    } else if has(|s| matches!(s, Sel::Body { .. })) {
        v.extend(body_items(app, &sel_bodies(app), false));
    } else if has(|s| matches!(s, Sel::Profile { .. })) {
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "solid.revolve", "Revolve"));
    } else {
        // Empty space, or a plane, which leads with a sketch on it.
        if let Some(plane) = &app.menu.plane {
            v.push(cmd(app, "sketch.create", "Create Sketch").with(json!({ "plane": plane })));
            v.push(Item::sep());
        }
        v.push(act("ui.nav", "Pan", "pan").with(json!({ "mode": "pan" })));
        v.push(act("ui.nav", "Zoom", "zoom").with(json!({ "mode": "zoom" })));
        v.push(act("ui.nav", "Orbit", "orbit").with(json!({ "mode": "orbit" })));
        v.push(act("ui.fit", "Fit", "fit"));
        v.push(act("ui.home", "Home View", "home"));
        v.push(Item::sep());
        let anything_hidden = !app.session.visibility.hidden_bodies.is_empty()
            || !app.session.visibility.hidden_sketches.is_empty()
            || !app.session.visibility.hidden_folders.is_empty();
        v.push(act("ui.showAll", "Show All", "eye").on(anything_hidden));
        v.push(act("ui.showAll", "Unisolate", "eye").on(!app.session.visibility.hidden_bodies.is_empty()));
        v.push(Item::sep());
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "solid.fillet", "Fillet"));
        v.push(Item::sep());
        v.push(cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()));
        v.push(cmd(app, "edit.redo", "Redo").on(!app.session.redo.is_empty()));
        v.push(Item::sep());
        if app.session.section.is_some() {
            v.push(act("ui.unsection", "Remove Section", "section"));
        }
        v.push(act("ui.origin", if app.ui.show_origin { "Hide Origin" } else { "Show Origin" }, "origin"));
        return v;
    }
    if !sel.is_empty() {
        if !v.iter().any(|item| item.id == "parts.resize")
            && let Some(component) = crate::dialogs_part_size::selected_component(app)
        {
            v.push(cmd(app, "parts.resize", "Change Part Size").with(json!({"component":component})));
        }
        v.push(Item::sep());
        v.push(act("ui.clear", "Clear Selection", "close").key("Esc"));
    }
    v
}

fn body_items(app: &SolveApp, bodies: &[String], browser: bool) -> Vec<Item> {
    let names = json!(bodies);
    let all_hidden = bodies.iter().all(|b| app.session.visibility.hidden_bodies.contains(b));
    let one = bodies.len() == 1;
    let locked = bodies.iter().all(|b| app.ui.locked_bodies.contains(b));
    let items = json!(bodies.iter().map(|b| json!({"type": "body", "name": b})).collect::<Vec<_>>());
    let mut v = vec![
        act("ui.moveBodies", "Move/Copy", "move").key("M").with(json!({ "bodies": names })).on(!locked),
        cmd(app, "component.from_bodies", "Create Components from Bodies").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.material", "Physical Material", "material").with(json!({ "bodies": names })),
        act("ui.appearance", "Appearance", "appearance").key("A").with(json!({ "bodies": names })),
        act("ui.properties", "Properties", "measure").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": names })),
        act("ui.export", "Export…", "export").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": items })).on(!locked),
        cmd(app, "solid.remove", "Remove").with(json!({ "bodies": names })).on(!locked),
        act("ui.rename", "Rename", "rename").with(json!({ "body": bodies.first() })).on(one),
        Item::sep(),
        act(if all_hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": names })),
        act("ui.lock", if locked { "Unlock" } else { "Lock" }, "ground").with(json!({ "bodies": names })),
        act("ui.isolate", "Isolate", "body").with(json!({ "bodies": names })),
    ];
    if one {
        let b = bodies.first().cloned().unwrap_or_default();
        if let Some(component) = crate::dialogs_part_size::component_for_bodies(app, bodies) {
            v.push(cmd(app, "parts.resize", "Change Part Size").with(json!({"component":component})));
        }
        v.push(Item::sep());
        if !browser {
            v.push(act("ui.findBrowser", "Find in Browser", "list").with(json!({ "body": b })));
        }
        v.push(act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "body": b })));
    }
    v
}

fn sketch_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = crate::browser::sketch_visible(app, id);
    let profiles = !app.ui.hidden_profiles.contains(&id);
    let dims = app.ui.shown_dims.contains(&id);
    let editing = app.session.active_sketch == Some(id);
    let has_profiles = app.session.world_state().sketch(id).is_some_and(|s| !s.profiles.is_empty());
    vec![
        act("ui.extrudeSketch", "Extrude", "extrude").key("E").with(json!({ "sketch": id })).on(has_profiles && !editing),
        Item::sep(),
        act("ui.editSketch", "Edit Sketch", "sketch").with(json!({ "sketch": id })).on(!editing),
        act("ui.redefineSketch", "Redefine Sketch Plane", "plane").with(json!({ "sketch": id })),
        act("ui.exportDxf", "Export DXF…", "export").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": [{"type": "feature", "id": id}] })),
        act("ui.rename", "Rename", "rename").with(json!({ "feature": id })),
        Item::sep(),
        act("ui.lookAt", "Look At", "lookat").with(json!({ "sketch": id })),
        act("ui.sketchProfile", if profiles { "Hide Profile" } else { "Show Profile" }, "sketch").with(json!({ "sketch": id })),
        act("ui.sketchDims", if dims { "Hide Dimension" } else { "Show Dimension" }, "dimension").with(json!({ "sketch": id })),
        act(if visible { "ui.hideSketch" } else { "ui.showSketch" }, "Show/Hide", "eye").key("V").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": id })),
    ]
}

/// A construction plane's row in the browser: sketch on it, edit, delete, rename, show or hide.
fn plane_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let Some(f) = app.session.doc.feature(id) else { return Vec::new() };
    let visible = !app.session.visibility.hidden_origin.contains(&f.name);
    vec![
        cmd(app, "sketch.create", "Create Sketch").with(json!({ "plane": f.name })),
        Item::sep(),
        act("ui.editFeature", "Edit Feature", "plane").with(json!({ "feature": id })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": [{"type": "feature", "id": id}] })),
        act("ui.rename", "Rename", "rename").with(json!({ "feature": id })),
        Item::sep(),
        act("ui.originPart", "Show/Hide", "eye").key("V").with(json!({ "keys": [f.name], "show": !visible })),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": id })),
    ]
}

/// An origin plane's row in the browser: sketch on it, show or hide.
fn origin_plane_items(app: &SolveApp, name: &str) -> Vec<Item> {
    let visible = !app.session.visibility.hidden_origin.iter().any(|h| h == name);
    vec![
        cmd(app, "sketch.create", "Create Sketch").with(json!({ "plane": name })),
        Item::sep(),
        act("ui.originPart", "Show/Hide", "eye").key("V").with(json!({ "keys": [name], "show": !visible })),
    ]
}

/// An assembly joint: edit, drive, suppress, delete.
fn joint_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { return Vec::new() };
    let rigid = j.kind == solidcraft_engine::doc::joints::JointKind::Rigid;
    // Show/Hide acts on every joint picked in the browser (this one among them).
    let mut joints = app.tree.picked_joints.clone();
    if !joints.contains(&id) {
        joints = vec![id];
    }
    let hidden = !joints.iter().any(|j| app.session.joint_visible(*j));
    vec![
        act("ui.editJoint", "Edit Joint", "joint").with(json!({ "joint": id })),
        act("ui.editJoint", "Edit Joint Limits", "joint").with(json!({ "joint": id })).on(!rigid),
        act("ui.driveJoint", "Drive Joint", "drive").with(json!({ "joint": id })).on(!rigid),
        act("ui.animateJoint", "Animate Joint", "play").with(json!({ "joint": id })).on(!rigid && !j.suppressed),
        act("ui.findJoint", "Find in Window", "fit").with(json!({ "joint": id })),
        cmd(app, "joint.edit", if j.suppressed { "Unsuppress" } else { "Suppress" }).with(json!({ "joint": id, "suppressed": !j.suppressed })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "joints": [id] })),
        act("ui.rename", "Rename", "rename").with(json!({ "joint": id })),
        Item::sep(),
        // Hiding a joint hides only its glyph: it still constrains and moves the parts.
        act("browser.visibility", "Show/Hide", "eye").key("V").with(json!({ "joints": joints, "visible": hidden })),
    ]
}

fn occurrence_items(app: &SolveApp, path: &[u64]) -> Vec<Item> {
    let Ok(placement) = app.session.occurrence_placement(path) else { return Vec::new() };
    let mut entries = component_items(app, placement.component);
    if app.session.occurrence_scope_count(path).is_ok_and(|n| n > 1) {
        entries.insert(0, Item::heading("Shared child: edits affect all copies of its parent"));
    }
    let bodies = app.session.occurrence_bodies(path, true).unwrap_or_default();
    for item in &mut entries {
        match item.id.as_str() {
            "ui.activate" => {
                item.params = Some(json!({"component": placement.component, "occurrence": placement.occurrence, "path": path}));
                item.enabled = app.session.active_occurrence_path.as_deref() != Some(path);
            }
            "occurrence.ground" => {
                item.label = if placement.grounded { "Unground" } else { "Ground" }.into();
                item.params = Some(json!({"occurrence": placement.occurrence, "grounded": !placement.grounded}));
            }
            "ui.moveOccurrence" => {
                item.params = Some(json!({"occurrence": placement.occurrence, "path": path}));
                item.enabled = !placement.grounded && app.session.held_by_rigid_group(placement.occurrence).is_none();
            }
            "ui.properties" => item.params = Some(json!({"occurrence": placement.occurrence, "path": path})),
            "ui.turnUpAxis" => {
                for child in &mut item.children {
                    if let Some(params) = child.params.as_mut() {
                        params["occurrence"] = json!(placement.occurrence);
                    }
                }
            }
            "occurrence.copy" => item.params = Some(json!({"occurrence": placement.occurrence, "translate": [10,10,0]})),
            "component.create" => item.params = Some(json!({"parent": placement.component, "parent_path": path})),
            "component.paste_new" => {
                item.label = "Paste New (Own Features)".into();
                item.params = Some(json!({"component":placement.component,"occurrence":placement.occurrence,"path":path,"translate":[10,10,0]}));
            }
            "ui.rename" => item.label = "Rename Component".into(),
            "ui.appearance" => item.label = "Component Appearance".into(),
            "ui.material" => item.label = "Component Physical Material".into(),
            "ui.delete" => item.params = Some(json!({"occurrences": [placement.occurrence]})),
            "ui.hide" | "ui.show" => {
                item.id = "browser.visibility".into();
                item.params = Some(json!({"visible": app.session.occurrence_hidden(path), "paths": [path]}));
                item.enabled = true;
            }
            "ui.isolate" => {
                item.id = "ui.isolateOccurrence".into();
                item.params = Some(json!({"path": path}));
            }
            "ui.export" | "ui.saveMesh" => item.params = Some(json!({"bodies": bodies})),
            _ => {}
        }
    }
    entries
}

/// "Turn Up Axis": a quarter turn about X of a component's occurrence for a file saved with the
/// other axis up, one `occurrence.move` (one undo step).
fn turn_up_axis(occurrence: u64, enabled: bool) -> Item {
    let mut item = act("ui.turnUpAxis", "Turn Up Axis", "move").on(enabled);
    item.children = [("Y up \u{2192} Z up", "90 deg"), ("Z up \u{2192} Y up", "-90 deg")]
        .into_iter()
        .map(|(label, angle)| {
            act("occurrence.move", label, "move").with(json!({"occurrence": occurrence, "axis": [1, 0, 0], "angle": angle})).on(enabled)
        })
        .collect();
    item
}

fn component_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let doc = &app.session.doc;
    let occ = doc.occurrence_of(id);
    let active = app.session.active_component == id;
    let bodies = component_bodies(app, id);
    let hidden = !bodies.is_empty() && bodies.iter().all(|b| app.session.visibility.hidden_bodies.contains(b));
    let names = json!(bodies);
    let mut v = Vec::new();
    if doc.parts.contains_key(&id) {
        v.push(cmd(app, "parts.resize", "Edit").with(json!({"component":id})));
    }
    if id != 0 {
        let grounded = occ.is_some_and(|o| o.grounded);
        let oid = occ.map(|o| o.id).unwrap_or(0);
        v.push(act("ui.activate", "Activate", "component").with(json!({ "component": id })).on(!active));
        v.push(cmd(app, "occurrence.ground", if grounded { "Unground" } else { "Ground" }).with(json!({ "occurrence": oid, "grounded": !grounded })));
        v.push(
            act("ui.moveOccurrence", "Move/Copy", "move")
                .key("M")
                .with(json!({ "occurrence": oid }))
                .on(!grounded && occ.is_some() && app.session.held_by_rigid_group(oid).is_none()),
        );
        v.push(turn_up_axis(oid, !grounded && occ.is_some() && app.session.held_by_rigid_group(oid).is_none()));
        v.push(cmd(app, "component.capture_position", "Capture Position").on(!app.session.pending_moves.is_empty()));
        v.push(cmd(app, "component.revert_position", "Revert Position").on(!app.session.pending_moves.is_empty()));
        // An imported assembly moves as one until its rigid group is released.
        if !app.session.contents_joints(id).is_empty() {
            v.push(cmd(app, "component.unlock", "Unlock Contents").with(json!({ "component": id })));
        }
        v.push(Item::sep());
    } else {
        v.push(act("ui.activate", "Activate", "component").with(json!({ "component": id })).on(!active));
    }
    v.push(cmd(app, "component.create", "New Component").with(json!({ "parent": id })));
    if id == 0 {
        v.push(act("ui.copyDesign", "Copy", "copy").key("Ctrl+C"));
        v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").on(crate::edit_clipboard::can_paste(app)));
        v.push(cmd(app, "component.from_bodies", "Create Components from Bodies").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    }
    v.push(Item::sep());
    v.push(act("ui.material", "Physical Material", "material").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(
        act("ui.appearance", "Appearance", "appearance")
            .key("A")
            .with(if id == 0 { json!({ "bodies": names }) } else { json!({"component": id}) })
            .on(id != 0 || !bodies.is_empty()),
    );
    if id != 0 {
        v.push(cmd(app, "component.cycle_color", "Cycle Component Color").with(json!({"component": id})));
    }
    v.push(act("ui.properties", "Properties", "measure").with(json!({ "component": id })));
    if doc.links.contains_key(&id) {
        v.push(act("ui.relinkSource", "Relink Source (All Instances)…", "update").with(json!({"component": id})));
    } else if id != 0 && app.session.can_extract(id) {
        v.push(act("ui.extractComponent", "Extract to File…", "export").with(json!({"component": id})));
    }
    if id == 0 {
        v.push(act("ui.designProperties", "Design Properties", "settings"));
    }
    v.push(Item::sep());
    v.push(act("ui.export", "Export…", "export").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(Item::sep());
    if id != 0 {
        v.push(cmd(app, "occurrence.copy", "Paste (Instance)").with(json!({ "component": id, "translate": [10, 10, 0] })));
        v.push(cmd(app, "component.paste_new", "Paste New").with(json!({ "component": id, "translate": [10, 10, 0] })));
        v.push(act("ui.delete", "Delete", "delete").key("Del").with(json!({ "occurrences": [occ.map(|o| o.id)] })).on(occ.is_some()));
    }
    v.push(act("ui.rename", "Rename", "rename").with(json!({ "component": id })));
    v.push(Item::sep());
    v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.showAll", "Show All Bodies", "eye"));
    v.push(act("ui.isolate", "Isolate", "body").with(json!({ "bodies": names })).on(!bodies.is_empty() && id != 0));
    v.push(act("ui.newGroup", "New Group", "folder").with(json!({ "component": id })));
    v
}

/// Explode Text (#1842): the selected text's outlines become normal sketch lines, arcs and
/// curves, no longer tied to the text, so they can be picked, trimmed and edited one by one.
fn explode_text(app: &SolveApp) -> Option<Item> {
    let link = crate::sketch_text::selected_link(app)?;
    Some(cmd(app, "sketch.break_link", "Explode Text").with(json!({ "link": link })))
}

/// Sketch entity items: construction, fix, delete, dimension and the constraints the selection
/// can take.
fn sketch_entity_items(app: &SolveApp) -> Vec<Item> {
    use solidcraft_engine::sketch::CurveKind;
    let Some(sid) = app.session.active_sketch else { return Vec::new() };
    let st = app.session.world_state();
    let Some(ss) = st.sketch(sid) else { return Vec::new() };
    let mut lines = Vec::new();
    let mut rounds = Vec::new();
    let mut curves = Vec::new();
    let mut points = Vec::new();
    // Selection order: a two-entity constraint keeps the first and moves the second.
    let mut picked: Vec<String> = Vec::new();
    for s in &app.session.selection {
        match s {
            Sel::SketchCurve { id } => {
                picked.push(id.clone());
                curves.push(id.clone());
                match ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i)).map(|c| &c.kind) {
                    Some(CurveKind::Line { .. }) => lines.push(id.clone()),
                    Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }) => rounds.push(id.clone()),
                    _ => {}
                }
            }
            Sel::SketchPoint { id } => {
                picked.push(id.clone());
                points.push(id.clone());
            }
            _ => {}
        }
    }
    let mut v = Vec::new();
    if curves.is_empty() && points.is_empty() {
        if crate::sketch_text::selected_entity(app).is_some() {
            v.push(cmd(app, "sketch.edit_text", "Edit Text"));
            v.extend(explode_text(app));
        }
        v.push(cmd(app, "sketch.line", "Line"));
        v.push(cmd(app, "sketch.rectangle.two_point", "Rectangle"));
        v.push(cmd(app, "sketch.circle.center", "Circle"));
        v.push(cmd(app, "sketch.dimension", "Sketch Dimension"));
        v.push(Item::sep());
        v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").on(app.session.sketch_clipboard.is_some()));
        return v;
    }
    let ents: Vec<String> = curves.iter().chain(points.iter()).cloned().collect();
    for pattern in &ss.sketch.patterns {
        if ents.iter().any(|id| pattern.contains(id))
            || app.session.selection.iter().filter_map(|sel| crate::sketch_modify_ui::point_id(&app.session, sel)).any(|id| pattern.contains(id))
        {
            v.push(act("ui.editSketchPattern", "Edit Sketch Pattern", "sketch_circ_pattern").with(json!({"pattern":pattern.id})));
        }
    }
    if crate::sketch_text::selected_entity(app).is_some() {
        v.push(cmd(app, "sketch.edit_text", "Edit Text"));
        v.extend(explode_text(app));
    }
    v.push(cmd(app, "sketch.move", "Move/Copy").key("M"));
    v.push(act("ui.cut", "Cut", "cut").key("Ctrl+X"));
    v.push(act("ui.copy", "Copy", "copy").key("Ctrl+C"));
    v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").on(app.session.sketch_clipboard.is_some()));
    if !curves.is_empty() {
        v.push(cmd(app, "sketch.construction", "Normal / Construction").with(json!({ "curves": curves })));
    }
    // Infinite lines (#1359): the selected lines switch together.
    let lines: Vec<String> = curves
        .iter()
        .filter(|id| {
            ss.sketch
                .curve_index(id)
                .and_then(|i| ss.sketch.curves.get(i))
                .is_some_and(|c| matches!(c.kind, solidcraft_engine::sketch::CurveKind::Line { .. }))
        })
        .cloned()
        .collect();
    if !lines.is_empty() {
        v.push(cmd(app, "sketch.infinite", "Infinite Line").with(json!({ "curves": lines })));
    }
    if curves.len() == 1 && ss.sketch.offset_distance(&curves[0]).is_some() {
        v.push(cmd(app, "sketch.offset.break", "Break Offset Association").with(json!({"curve":curves[0]})));
    }
    v.push(act("ui.fix", "Fix/Unfix", "c_fix").with(json!({ "entities": ents })));
    v.push(act("ui.delete", "Delete", "delete").key("Del"));
    v.push(cmd(app, "sketch.dimension", "Sketch Dimension").with(json!({ "entities": ents })).on(ents.len() <= 2));
    let mut c = Vec::new();
    let n = ents.len();
    let (l, r, p) = (lines.len(), rounds.len(), points.len());
    if l == 1 && n == 1 {
        c.push(cmd(app, "sketch.constraint.horizontal_vertical", "Horizontal/Vertical").with(json!({ "line": lines[0] })));
    }
    if p == 2 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": points[0], "b": points[1] })));
        c.push(cmd(app, "sketch.constraint.horizontal_vertical", "Horizontal/Vertical").with(json!({ "points": points })));
    }
    if p == 1 && curves.len() == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": picked[0], "b": picked[1] })));
        if l == 1 {
            c.push(cmd(app, "sketch.constraint.midpoint", "MidPoint").with(json!({ "point": points[0], "line": lines[0] })));
        }
    }
    if l == 2 && n == 2 {
        let ab = json!({ "a": lines[0], "b": lines[1] });
        c.push(cmd(app, "sketch.constraint.parallel", "Parallel").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.perpendicular", "Perpendicular").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.collinear", "Collinear").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.equal", "Equal").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.midpoint", "MidPoint").with(ab));
    }
    if r == 2 && n == 2 {
        let ab = json!({ "a": rounds[0], "b": rounds[1] });
        c.push(cmd(app, "sketch.constraint.concentric", "Concentric").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.equal", "Equal").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.tangent", "Tangent").with(ab));
    }
    if l == 1 && r == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.tangent", "Tangent").with(json!({ "a": picked[0], "b": picked[1] })));
    }
    if p == 2 && l == 1 && n == 3 {
        c.push(cmd(app, "sketch.constraint.symmetry", "Symmetry").with(json!({ "a": points[0], "b": points[1], "line": lines[0] })));
    }
    if !c.is_empty() {
        v.push(Item::heading("Constraints"));
        v.extend(c);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Running an item
// ---------------------------------------------------------------------------------------------

fn strs(p: &Value, k: &str) -> Vec<String> {
    p.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn id_of(p: &Value, k: &str) -> Option<u64> {
    p.get(k).and_then(Value::as_u64)
}

/// Run a menu item (what a click on it does). `at` places a rename box.
pub fn run_item(app: &mut SolveApp, item: &Item, at: Pos2) {
    if !item.enabled || item.is_sep() || item.is_heading() {
        return;
    }
    let p = item.params.clone().unwrap_or(Value::Null);
    let bodies = strs(&p, "bodies");
    match item.id.as_str() {
        "parts.resize" => {
            if let Some(component) = id_of(&p, "component") {
                crate::dialogs_part_size::open(app, component);
            } else {
                app.start("parts.resize");
            }
        }
        "ui.repeat" => {
            app.repeat_last();
        }
        "ui.selectOther" => {
            crate::select_other::open(app, at);
        }
        "ui.delete" if item.params.is_some() => crate::delete::request(app, p),
        "ui.delete" => delete(app),
        "ui.finishSketch" => app.finish_sketch(),
        "ui.cancelSketch" => app.cancel_sketch(),
        "ui.relinkSource" => {
            if let Some(component) = id_of(&p, "component") {
                crate::linked_source::start(app, component);
            }
        }
        "ui.extractComponent" => {
            if let Some(component) = id_of(&p, "component") {
                crate::linked_source::extract(app, component);
            }
        }
        "ui.copy" => drop(crate::edit_clipboard::copy(app)),
        "ui.copyDesign" => drop(crate::edit_clipboard::copy_design(app)),
        "ui.cut" => drop(crate::edit_clipboard::cut(app)),
        "ui.paste" => drop(crate::edit_clipboard::paste(app, Some(at))),
        "ui.editSketchPattern" => {
            if let Some(id) = p.get("pattern").and_then(Value::as_str) {
                crate::sketch_modify_ui::edit_pattern(app, id);
            }
        }
        "ui.hide" => {
            for b in bodies {
                if !app.session.visibility.hidden_bodies.contains(&b) {
                    app.session.visibility.hidden_bodies.push(b);
                }
            }
        }
        "ui.show" => app.session.visibility.hidden_bodies.retain(|b| !bodies.contains(b)),
        "ui.showAll" => {
            let _ = app.run("browser.visibility", json!({"folder":"bodies", "visible":true}));
            app.session.visibility.hidden_sketches.clear();
            // Hidden folders and groups too, so everything really shows.
            app.session.visibility.hidden_folders.clear();
            app.ui.show_sketches = true;
        }
        "ui.isolate" => {
            app.session.visibility.hidden_bodies =
                app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|b| !bodies.contains(b)).collect();
        }
        "ui.lock" => {
            let locked = bodies.iter().all(|b| app.ui.locked_bodies.contains(b));
            if locked {
                app.ui.locked_bodies.retain(|b| !bodies.contains(b));
            } else {
                app.ui.locked_bodies.extend(bodies.into_iter().filter(|b| !app.ui.locked_bodies.contains(b)).collect::<Vec<_>>());
            }
        }
        "ui.moveBodies" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start("solid.move");
        }
        "ui.rename" => {
            let what = if let Some(b) = p.get("body").and_then(Value::as_str) {
                RenameWhat::Body(b.to_string())
            } else if let Some(f) = id_of(&p, "feature") {
                RenameWhat::Feature(f)
            } else if let Some(c) = id_of(&p, "component") {
                RenameWhat::Component(c)
            } else if let Some(g) = id_of(&p, "group") {
                RenameWhat::Group(g)
            } else if let Some(c) = id_of(&p, "canvas") {
                RenameWhat::Canvas(c)
            } else if let Some(v) = p.get("view").and_then(Value::as_str) {
                RenameWhat::View(v.to_string())
            } else if let Some(j) = id_of(&p, "joint") {
                RenameWhat::Joint(j)
            } else {
                return;
            };
            start_rename(app, what, at);
        }
        "ui.properties" => {
            let command = if p.get("occurrence").is_some() {
                "occurrence.properties"
            } else if p.get("component").is_some() {
                "component.properties"
            } else {
                "inspect.measure"
            };
            let params = if command == "inspect.measure" { json!({ "bodies": bodies }) } else { p.clone() };
            match app.session.execute(command, &params) {
                Ok(v) => {
                    let title = v["name"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| if bodies.len() == 1 { bodies.join("") } else { format!("{} bodies", bodies.len()) });
                    app.menu.props = Some((title, v));
                }
                Err(e) => app.set_status(e.to_string(), true),
            }
        }
        "ui.designProperties" => app.start("document.metadata"),
        "ui.export" => {
            let name = bodies.first().cloned().unwrap_or_else(|| app.session.doc.name.clone());
            app.pick_save_command(&format!("{name}.step"), &["step", "stp", "igs", "stl", "3mf", "obj"], "file.export", json!({"bodies": bodies}));
        }
        "ui.findBrowser" => {
            if let Some(feature) = id_of(&p, "feature") {
                if let Some(body) = p.get("body").and_then(Value::as_str) {
                    crate::browser::reveal_feature(app, feature, body);
                }
                return;
            }
            if let Some(b) = p.get("body").and_then(Value::as_str) {
                app.ui.show_browser = true;
                app.preferences.timeline_left_browser_collapsed = false;
                app.tree.reveal_feature = None;
                app.tree.reveal = Some(b.to_string());
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": b}]}));
            }
        }
        "ui.findTimeline" => {
            let f = id_of(&p, "feature").or_else(|| p.get("body").and_then(Value::as_str).and_then(|b| body_feature(app, b)));
            if let Some(f) = f {
                app.ui.show_timeline = true;
                let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": f}]}));
            }
        }
        "ui.tangentChain" => tangent_chain(app),
        "ui.origin" => app.ui.show_origin = !app.ui.show_origin,
        "ui.clear" => drop(app.run("select.clear", json!({}))),
        "ui.unsection" => {
            app.tree.hidden_section = None;
            drop(app.run("inspect.section", json!({"clear": true})));
        }
        "ui.editSection" => crate::dialogs::edit_section(app),
        "ui.sectionVisible" => crate::browser::set_section_shown(app, p.get("visible").and_then(Value::as_bool).unwrap_or(true)),
        "ui.fit" => app.animate_view("fit"),
        "ui.home" => app.animate_view("home"),
        // One command for the whole selection: all fixed unless all already are.
        "ui.fix" => drop(app.run("sketch.constraint.fix", json!({ "entities": strs(&p, "entities") }))),
        "ui.editSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                app.edit_sketch(id);
            }
        }
        "ui.redefineSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                crate::browser::redefine_sketch(app, id);
            }
        }
        "ui.hideSketch" | "ui.showSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                crate::browser::set_sketch_visible(app, id, item.id == "ui.showSketch");
            }
        }
        "ui.sketchDims" => {
            if let Some(id) = id_of(&p, "sketch") {
                if app.ui.shown_dims.contains(&id) {
                    app.ui.shown_dims.retain(|x| *x != id);
                } else {
                    app.ui.shown_dims.push(id);
                }
            }
        }
        "ui.sketchProfile" => {
            if let Some(id) = id_of(&p, "sketch") {
                if app.ui.hidden_profiles.contains(&id) {
                    app.ui.hidden_profiles.retain(|x| *x != id);
                } else {
                    app.ui.hidden_profiles.push(id);
                }
            }
        }
        "ui.activate" => {
            if id_of(&p, "component").is_some() {
                let _ = app.run("component.activate", p);
            }
        }
        "ui.moveOccurrence" => {
            if let Some(o) = id_of(&p, "occurrence") {
                let mut movement = crate::browser::OccMove::new(o);
                movement.had_pending = app.session.pending_moves.contains_key(&o);
                movement.path = p.get("path").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
                movement.baseline = app
                    .session
                    .pending_moves
                    .get(&o)
                    .copied()
                    .or_else(|| app.session.doc.occurrences.iter().find(|occ| occ.id == o).map(|occ| occ.transform));
                app.tree.occurrence_move = Some(movement);
            }
        }
        "ui.isolateOccurrence" => {
            let _ = app.run("browser.visibility", json!({"visible": true, "paths": [p["path"]], "isolate": true}));
        }
        "ui.canvasVisible" => drop(app.run("canvas.edit", p)),
        "ui.toolbarPin" | "ui.toolbarRemove" | "ui.toolbarReset" | "ui.toolbarCustomize" | "ui.shortcutPin" => {
            crate::toolbar_custom::run(app, &item.id, &p)
        }
        "ui.workspace" | "ui.workspace.design" | "ui.workspace.render" => {
            if let Some(workspace) = p.get("workspace").and_then(Value::as_str)
                && let Err(e) = crate::render_workspace::enter(app, workspace == "render")
            {
                app.set_status(e, true);
            }
        }
        "ui.findJoint" => {
            if let Some(id) = id_of(&p, "joint") {
                crate::dialogs_assembly::find_joint(app, id);
            }
        }
        "ui.editJoint" | "ui.driveJoint" | "ui.animateJoint" => {
            if let Some(id) = id_of(&p, "joint") {
                let d = match item.id.as_str() {
                    "ui.editJoint" => crate::dialogs_assembly::edit_joint(app, id),
                    "ui.animateJoint" => crate::dialogs_assembly::animate_joint(app, id),
                    _ => crate::dialogs_assembly::drive_joint(app, id),
                };
                if let Some(d) = d {
                    app.tool = None;
                    app.dialog = Some(d);
                }
            }
        }
        "ui.newView" => drop(crate::browser::save_view(app, None)),
        "ui.restoreView" => {
            if let Some(v) = p.get("view").and_then(Value::as_str) {
                crate::browser::restore_view(app, v);
            }
        }
        "ui.updateView" => {
            if let Some(v) = p.get("view").and_then(Value::as_str) {
                crate::browser::save_view(app, Some(v));
            }
        }
        "ui.folderVisible" => {
            if let Some(key) = p.get("key").and_then(Value::as_str) {
                crate::browser::toggle_folder(app, key);
            }
        }
        "ui.originAll" => {
            app.ui.show_origin = true;
            app.session.visibility.hidden_origin.retain(|h| !["O", "X", "Y", "Z", "XY", "XZ", "YZ"].contains(&h.as_str()));
        }
        "ui.originPart" => {
            let show = p.get("show").and_then(Value::as_bool).unwrap_or(true);
            for k in strs(&p, "keys") {
                app.session.visibility.hidden_origin.retain(|h| *h != k);
                if !show {
                    app.session.visibility.hidden_origin.push(k);
                }
            }
        }
        "ui.editFeature" => {
            if let Some(f) = id_of(&p, "feature") {
                app.edit_feature(f);
            }
        }
        "ui.appearance" => {
            if item.params.is_some() {
                let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
                let _ = app.run("select.set", json!({ "items": items }));
            }
            app.start("appearance.assign");
            if let Some(name) = id_of(&p, "component").and_then(|id| app.session.doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()))
                && let Some(d) = app.dialog.as_mut()
                && let crate::dialogs::Kind::Appearance(k) = &mut d.kind
            {
                if let Some(id) = id_of(&p, "component")
                    && let Some(look) = app.session.doc.appearances.components.get(&id)
                {
                    k.color = look.color;
                    k.opacity = look.opacity;
                    k.pbr = look.pbr;
                    k.look = Some(look.name.clone());
                }
                k.component = Some(name);
            }
        }
        "ui.material" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start("material.assign");
        }
        "ui.saveMesh" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start("file.save_mesh");
        }
        "ui.exportDxf" => {
            if let Some(id) = id_of(&p, "sketch") {
                let name = app.session.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
                app.pick_save_command(&format!("{name}.dxf"), &["dxf"], "sketch.export_dxf", json!({"sketch": id}));
            }
        }
        "ui.extrudeSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                let n = app.session.world_state().sketch(id).map_or(0, |s| s.profiles.len());
                let items: Vec<Value> = (0..n).map(|i| json!({"type": "profile", "sketch": id, "index": i})).collect();
                let _ = app.run("select.set", json!({ "items": items }));
                app.start("solid.extrude");
            }
        }
        "ui.lookAt" => {
            let st = app.session.world_state();
            if let Some(ss) = id_of(&p, "sketch").and_then(|id| st.sketch(id)) {
                let to = crate::dialogs::onto_sketch(app.cam.looking_from(ss.plane.normal()), ss);
                app.animate_to(to);
            }
        }
        "ui.nav" => {
            app.viewport.nav = match p.get("mode").and_then(Value::as_str) {
                Some("pan") => Some(crate::viewport::NavMode::Pan),
                Some("zoom") => Some(crate::viewport::NavMode::Zoom),
                Some("orbit") => Some(crate::viewport::NavMode::Orbit),
                _ => None,
            };
        }
        "ui.editCanvas" | "ui.calibrateCanvas" => {
            if let Some(c) = id_of(&p, "canvas") {
                app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, c, item.id == "ui.calibrateCanvas"));
            }
        }
        "ui.newGroup" | "ui.ungroup" | "ui.groupSelected" => crate::browser::group_action(app, &item.id, &p),
        // A plane's Create Sketch is the toolbar's with that plane picked first: it replaces a
        // dialog in progress, and the dialog starts the sketch at once, facing the plane.
        "sketch.create" if item.params.is_some() => {
            if app.run("select.set", json!({ "items": [{"type": "plane", "name": p.get("plane")}] })).is_ok() {
                app.start("sketch.create");
            }
        }
        id if item.params.is_some() => {
            let _ = app.run(id, p);
        }
        id => app.start(id),
    }
}

/// Delete what is selected: sketch entities, features, and bodies (a Remove feature).
pub fn delete(app: &mut SolveApp) {
    crate::delete::delete_selection(app);
}

/// Show/Hide (the V key): what is selected in the viewport or picked in the browser flips its
/// visibility (bodies, faces' and edges' bodies, sketches, construction and origin planes and
/// axes, components' bodies, canvases, joints' glyphs). When some are shown and some hidden, all are hidden.
pub fn toggle_visibility(app: &mut SolveApp) {
    use solidcraft_engine::doc::FeatureKind;
    let paths: Vec<Vec<u64>> =
        app.session.selection.iter().filter_map(|s| if let Sel::Occurrence { path } = s { Some(path.clone()) } else { None }).collect();
    if !paths.is_empty() {
        let visible = !paths.iter().any(|p| !app.session.occurrence_hidden(p));
        let _ = app.run("browser.visibility", json!({"visible": visible, "paths": paths}));
    }
    let mut bodies: Vec<String> = touched_bodies(app);
    for c in app.tree.picked_components.clone() {
        bodies.extend(component_bodies(app, c));
    }
    let sketches: Vec<u64> = app
        .session
        .selection
        .iter()
        .filter_map(|s| if let Sel::Feature { id } = s { Some(*id) } else { None })
        .filter(|id| app.session.doc.feature(*id).is_some_and(|f| matches!(f.kind, FeatureKind::Sketch { .. })))
        .collect();
    let origin: Vec<String> = app
        .session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::Plane { name } | Sel::Axis { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let canvases = app.tree.picked_canvases.clone();
    let joints = app.tree.picked_joints.clone();
    let any_visible = bodies.iter().any(|b| !app.session.visibility.hidden_bodies.contains(b))
        || joints.iter().any(|j| app.session.joint_visible(*j))
        || sketches.iter().any(|s| crate::browser::sketch_visible(app, *s))
        || origin.iter().any(|o| !app.session.visibility.hidden_origin.contains(o))
        || app.session.doc.canvases.iter().any(|c| canvases.contains(&c.id) && c.visible);
    let hide = any_visible;
    app.session.visibility.hidden_bodies.retain(|b| !bodies.contains(b));
    app.session.visibility.hidden_origin.retain(|o| !origin.contains(o));
    if hide {
        app.session.visibility.hidden_bodies.extend(bodies);
        app.session.visibility.hidden_origin.extend(origin);
    }
    for s in sketches {
        crate::browser::set_sketch_visible(app, s, !hide);
    }
    if !joints.is_empty() {
        let _ = app.run("browser.visibility", json!({ "joints": joints, "visible": !hide }));
    }
    for c in canvases {
        let _ = app.run("canvas.edit", json!({ "canvas": c, "visible": !hide }));
    }
}

/// Rename what is selected (F2): one body, sketch or feature, construction plane, component
/// picked in the browser, or canvas.
pub fn rename_selection(app: &mut SolveApp, at: Pos2) -> bool {
    let what = match (app.session.selection.as_slice(), app.tree.picked_components.as_slice(), app.tree.picked_canvases.as_slice()) {
        ([Sel::Body { name }], [], []) => RenameWhat::Body(name.clone()),
        ([Sel::Feature { id }], [], []) => RenameWhat::Feature(*id),
        ([Sel::Occurrence { path }], [], []) => match app.session.occurrence_placement(path) {
            Ok(p) => RenameWhat::Component(p.component),
            Err(_) => return false,
        },
        ([Sel::Plane { name }], [], []) => match app.session.doc.find_feature(name) {
            Some(f) => RenameWhat::Feature(f.id),
            None => return false,
        },
        ([], [c], []) => RenameWhat::Component(*c),
        ([], [], [c]) => RenameWhat::Canvas(*c),
        _ => return false,
    };
    start_rename(app, what, at);
    true
}

/// Add the edges that continue the selected edges smoothly to the selection.
fn tangent_chain(app: &mut SolveApp) {
    let st = app.session.world_state();
    let mut out: Vec<Sel> = Vec::new();
    for s in &app.session.selection {
        let Sel::Edge { body, index, .. } = s else { continue };
        let Some(b) = st.body(body) else { continue };
        let m = b.mesh();
        for i in m.tangent_chain(*index, 2f64.to_radians()) {
            let Some(e) = m.edges.get(i) else { continue };
            let Some(mid) = e.get(e.len() / 2).copied() else { continue };
            let x = Sel::Edge { body: body.clone(), index: i, point: mid };
            if !out.iter().any(|y| matches!(y, Sel::Edge { body: bb, index: ii, .. } if bb == body && *ii == i)) {
                out.push(x);
            }
        }
    }
    if !out.is_empty() {
        let _ = app.run("select.set", json!({ "items": out }));
    }
}

pub fn start_rename(app: &mut SolveApp, what: RenameWhat, at: Pos2) {
    let doc = &app.session.doc;
    let text = match &what {
        RenameWhat::Body(b) => b.clone(),
        RenameWhat::Feature(f) => doc.feature(*f).map(|f| f.name.clone()).unwrap_or_default(),
        RenameWhat::Component(0) => doc.name.clone(),
        RenameWhat::Component(c) => doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone()).unwrap_or_default(),
        RenameWhat::Group(g) => crate::browser::group_name(app, *g).unwrap_or_default(),
        RenameWhat::TimelineGroup(g) => doc.timeline_groups.iter().find(|group| group.id == *g).map(|group| group.name.clone()).unwrap_or_default(),
        RenameWhat::Canvas(c) => doc.canvases.iter().find(|x| x.id == *c).map(|x| x.name.clone()).unwrap_or_default(),
        RenameWhat::View(v) => v.clone(),
        RenameWhat::Joint(j) => doc.assembly.joints.iter().find(|x| x.id == *j).map(|x| x.name.clone()).unwrap_or_default(),
    };
    app.menu.rename = Some(Rename { what, text, at, focused: false });
}

pub(crate) fn commit_rename(app: &mut SolveApp, r: &Rename) {
    let name = r.text.trim().to_string();
    if name.is_empty() {
        return;
    }
    match &r.what {
        RenameWhat::Body(old) => drop(app.run_heavy("body.rename", json!({ "body": old, "name": name }))),
        RenameWhat::Feature(f) => drop(app.run("timeline.rename", json!({ "feature": f, "name": name }))),
        RenameWhat::Component(c) => drop(app.run("component.rename", json!({ "component": c, "name": name }))),
        RenameWhat::Group(g) => crate::browser::group_action(app, "ui.renameGroup", &json!({ "group": g, "name": name })),
        RenameWhat::TimelineGroup(g) => drop(app.run("timeline.rename_group", json!({ "group": g, "name": name }))),
        RenameWhat::Canvas(c) => drop(app.run("canvas.edit", json!({ "canvas": c, "name": name }))),
        RenameWhat::View(v) => drop(app.run("view.rename", json!({ "view": v, "name": name }))),
        RenameWhat::Joint(j) => drop(app.run("joint.edit", json!({ "joint": j, "name": name }))),
    }
}

// ---------------------------------------------------------------------------------------------
// Opening and showing
// ---------------------------------------------------------------------------------------------

/// Open the viewport menu at a screen position. A right-click on something not selected
/// selects it first, so the menu is about what is under the cursor; a face or edge already in a
/// multi-selection keeps the whole selection (#3733).
pub fn open(app: &mut SolveApp, at: Pos2) {
    app.menu.over_geometry = app.viewport.hover.is_some();
    // A right-click leaves a plane unselected, but the menu offers a sketch on it.
    app.menu.plane = app.viewport.hover.clone().and_then(|h| crate::viewport::candidate(app, &[h])).and_then(|(_, sel)| match sel {
        Sel::Plane { name } => Some(name),
        _ => None,
    });
    crate::select_other::close(app);
    let selected = |app: &SolveApp, sel: &Sel| app.session.selection.iter().any(|x| crate::selection::same_item(x, sel));
    if app.session.active_sketch.is_none()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && !selected(app, &sel)
        && !matches!(sel, Sel::Plane { .. } | Sel::Axis { .. })
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
    } else if app.session.active_sketch.is_some()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && (matches!(sel, Sel::SketchCurve { .. } | Sel::SketchPoint { .. }) || crate::sketch_text::entity_for_selection(app, &sel).is_some())
        && !selected(app, &sel)
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
    }
    open_for(app, at, Target::Viewport);
}

/// Open a menu for a target (the radial ring only for the viewport).
pub fn open_for(app: &mut SolveApp, at: Pos2, target: Target) {
    app.menu.key_row = None;
    app.menu.submenu = None;
    app.menu.submenu_key = None;
    app.menu.radial = target == Target::Viewport && app.preferences.radial_context_menu;
    app.menu.target = target;
    app.menu.flick = false;
    app.menu.opener = None;
    app.viewport.context_menu = Some(at);
}

/// A button's menu: a click opens it, and a click on the same button while it is open closes it.
pub fn toggle_for(app: &mut SolveApp, opener: Rect, at: Pos2, target: Target) {
    if app.viewport.context_menu.is_some() && app.menu.target == target && app.menu.opener == Some(opener) {
        close(app);
    } else {
        open_for(app, at, target);
        app.menu.opener = Some(opener);
    }
}

pub fn close(app: &mut SolveApp) {
    app.viewport.context_menu = None;
    app.menu.flick = false;
    app.menu.opener = None;
}

/// Open the menu at a point for automation: the pointer moves there first so what is under it
/// counts, then the menu opens on a later frame.
pub fn request_open(app: &mut SolveApp, at: Pos2, target: Target) {
    app.synthetic.push(egui::Event::PointerMoved(at));
    app.menu.pending = Some((at, target));
}

/// The open menu as JSON (for `ui.menu`).
pub fn describe(app: &SolveApp) -> Value {
    let Some(at) = app.viewport.context_menu else {
        return json!({ "open": false, "pending": app.menu.pending.is_some() });
    };
    let radial = if app.menu.radial { radial_items(app) } else { Vec::new() };
    let describe_items = |items: Vec<Item>| {
        items
            .into_iter()
            .map(|item| {
                let placeholder = !item.icon.is_empty() && crate::icon_svg::resolve(&item.icon).placeholder;
                let mut value = json!(item);
                value["placeholder"] = json!(placeholder);
                value
            })
            .collect::<Vec<_>>()
    };
    json!({
        "open": true,
        "at": [at.x, at.y],
        "target": app.menu.target,
        "radial": describe_items(radial),
        "items": describe_items(items(app, &app.menu.target)),
        "rename": app.menu.rename.as_ref().map(|r| r.text.clone()),
    })
}

/// Pick an item of the open menu by id or label (for `ui.menuPick`).
pub fn pick(app: &mut SolveApp, key: &str) -> Result<Value, String> {
    let Some(at) = app.viewport.context_menu else { return Err("no menu is open".into()) };
    let target = app.menu.target.clone();
    let mut all = if app.menu.radial { radial_items(app) } else { Vec::new() };
    let roots = items(app, &target);
    all.extend(roots.iter().cloned());
    all.extend(roots.into_iter().flat_map(|item| item.children));
    let item =
        all.into_iter().find(|i| !i.is_sep() && !i.is_heading() && (i.id == key || i.label == key)).ok_or_else(|| format!("no menu item `{key}`"))?;
    if !item.enabled {
        return Err(format!("`{}` is disabled", item.label));
    }
    close(app);
    run_item(app, &item, at);
    Ok(json!({ "ran": item.id, "label": item.label }))
}

/// Where the radial menu goes so its ring stays on screen.
fn radial_center(at: Pos2, screen: Rect) -> Pos2 {
    let mx = RX + 4.0;
    let my = RY + 4.0;
    let x = if screen.width() > 2.0 * mx { at.x.clamp(screen.left() + mx, screen.right() - mx) } else { screen.center().x };
    let y = if screen.height() > 2.0 * my { at.y.clamp(screen.top() + my, screen.bottom() - my) } else { screen.center().y };
    pos2(x, y)
}

/// Height of a list of items as [`list_row`] draws them in [`menu_frame`] (margins and border).
fn list_height(items: &[Item]) -> f32 {
    14.0 + items
        .iter()
        .map(|i| {
            if i.is_sep() {
                11.0
            } else if i.is_heading() {
                24.0
            } else {
                30.0
            }
        })
        .sum::<f32>()
}

/// Where the list goes next to a ring centred at `c`: below it when it fits, otherwise beside
/// it (right, else left), kept on screen.
fn list_place(c: Pos2, h: f32, screen: Rect) -> Pos2 {
    let below = c.y + RY + 12.0;
    if below + h <= screen.bottom() - 4.0 {
        return pos2((c.x - LIST_W / 2.0).clamp(screen.left() + 4.0, (screen.right() - LIST_W - 4.0).max(screen.left())), below);
    }
    let y = (c.y - h / 2.0).clamp(screen.top() + 4.0, (screen.bottom() - h - 4.0).max(screen.top()));
    let right = c.x + RX + 12.0;
    if right + LIST_W <= screen.right() - 4.0 {
        return pos2(right, y);
    }
    pos2((c.x - RX - 12.0 - LIST_W).max(screen.left() + 4.0), y)
}

/// Width of list menus.
pub const LIST_W: f32 = 250.0;

/// Where a menu of `size` opened at `at` goes: right of and below the point (`gap` away) when it
/// fits in `screen`, flipped to the left or above it when it would run past that edge, and
/// against the far edge when it fits on neither side. A flipped menu ends at the cursor, so it
/// stays in view even when the window reaches past the screen (#1354).
pub fn place(at: Pos2, gap: Vec2, size: Vec2, screen: Rect) -> Pos2 {
    let axis = |at: f32, gap: f32, len: f32, lo: f32, hi: f32| {
        if at + gap + len <= hi {
            at + gap
        } else if at - gap - len >= lo {
            at - gap - len
        } else {
            (hi - len).max(lo)
        }
    };
    pos2(axis(at.x, gap.x, size.x, screen.left(), screen.right()), axis(at.y, gap.y, size.y, screen.top(), screen.bottom()))
}

/// The radial slot a pointer offset points at (none near the centre).
fn sector(d: Vec2) -> Option<usize> {
    if d.length() < 22.0 {
        return None;
    }
    // Clockwise from the top (screen y points down).
    let a = d.x.atan2(-d.y).to_degrees();
    let k = ((a + 360.0 + 22.5) / 45.0).floor() as i64;
    Some((k.rem_euclid(DIRS as i64)) as usize)
}

/// Press-and-hold of the right button in the viewport opens the menu; releasing over a
/// direction picks it.
fn track_hold(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.preferences.radial_context_menu || crate::mouse_gesture::active(app) {
        app.menu.press = None;
        app.menu.flick = false;
        app.menu.radial = false;
        return;
    }
    let (pressed, down, released, pos, time) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.pointer.secondary_down(),
            i.pointer.button_released(egui::PointerButton::Secondary),
            i.pointer.latest_pos(),
            i.time,
        )
    });
    let free = app.tool.is_none() && app.dialog.is_none() && app.preferences.nav != crate::prefs::OPENSCAD_NAV;
    if pressed && free && app.viewport.context_menu.is_none() && app.viewport.mouse.is_some() {
        app.menu.press = pos.map(|p| (p, time));
    }
    if let Some((p0, t0)) = app.menu.press {
        let moved = pos.is_some_and(|p| p.distance(p0) > 5.0);
        if !down || moved {
            app.menu.press = None;
        } else if time - t0 > 0.35 {
            app.menu.press = None;
            open(app, p0);
            app.menu.flick = true;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    if released && app.menu.flick {
        app.menu.flick = false;
        if let (Some(at), Some(p)) = (app.viewport.context_menu, pos) {
            let c = radial_center(at, ctx.content_rect());
            if app.menu.radial
                && let Some(k) = sector(p - c)
                && p.distance(c) > 36.0
                && let Some(item) = radial_items(app).into_iter().nth(k).filter(|i| i.enabled)
            {
                close(app);
                run_item(app, &item, at);
            }
        }
    }
}

/// Show the open menu, the rename box and the Properties window.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some((at, target)) = app.menu.pending.clone()
        && app.synthetic.is_empty()
    {
        app.menu.pending = None;
        if target == Target::Viewport { open(app, at) } else { open_for(app, at, target) }
    }
    crate::browser::dims_overlay(app, ctx);
    track_hold(app, ctx);
    show_rename(app, ctx);
    crate::design_properties::show_object(app, ctx);
    let Some(at) = app.viewport.context_menu else { return };
    let screen = ctx.content_rect();
    let target = app.menu.target.clone();
    let list = items(app, &target);
    let mut picked: Option<Item> = None;
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    let mut inside = app.menu.opener.is_some_and(|r| pointer.is_some_and(|p| r.contains(p)));

    let list_at = if target == Target::Workspace {
        pos2(at.x.clamp(screen.left(), (screen.right() - 470.0).max(screen.left())), at.y)
    } else if app.menu.radial {
        let c = radial_center(at, screen);
        let radial = radial_items(app);
        let area = Rect::from_center_size(c, vec2(2.0 * (RX + 4.0), 2.0 * (RY + 4.0)));
        inside |= pointer.is_some_and(|p| area.contains(p));
        let hot = pointer.and_then(|p| sector(p - c));
        let resp = Container::new(ctx, Surface::Popup)
            .area(egui::Id::new("sc_marking_menu"))
            .fixed_pos(area.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let _ = ui.allocate_exact_size(area.size(), Sense::hover());
                picked = crate::ui_kit::radial_menu(ui, c, &radial, hot).or(picked.take());
            });
        inside |= resp.response.contains_pointer();
        list_place(c, list_height(&list), screen)
    } else {
        place(at, Vec2::ZERO, vec2(LIST_W, list_height(&list)), screen)
    };

    let had_submenu = app.menu.submenu.is_some();
    let enabled: Vec<_> = list.iter().map(|i| i.enabled && !i.is_sep() && !i.is_heading()).collect();
    if app.menu.submenu.is_none() {
        let enter = crate::ui_kit::Menu::navigate(ctx, &enabled, &mut app.menu.key_row);
        let right = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        if let Some(k) = app.menu.key_row
            && let Some(item) = list.get(k)
        {
            if (right || enter) && !item.children.is_empty() {
                app.menu.submenu = Some(k);
                app.menu.submenu_key = item.children.iter().position(|c| c.enabled);
            } else if enter {
                picked = Some(item.clone());
            }
        }
    } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
        app.menu.submenu = None;
        app.menu.submenu_key = None;
    }
    let mut submenu_row = None;
    let key_row = app.menu.key_row;
    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::new("sc_context_menu"))
        .fixed_pos(list_at)
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if target == Target::Workspace {
                picked = crate::ui_kit::workspace_menu(ui, &list, key_row, (screen.bottom() - list_at.y).max(160.0)).or(picked.take());
            } else {
                menu_frame(ui, |ui| {
                    for (k, item) in list.iter().enumerate() {
                        if item.children.is_empty() {
                            if let Some(p) = list_row_lit(ui, item, key_row == Some(k)) {
                                picked = Some(p);
                            }
                        } else {
                            let row = kit_item(item, key_row == Some(k) || app.menu.submenu == Some(k)).show(ui);
                            if item.enabled && ((row.hovered() && ctx.input(|i| i.pointer.delta() != Vec2::ZERO)) || row.clicked()) {
                                if app.menu.submenu != Some(k) {
                                    app.menu.submenu_key = None;
                                }
                                app.menu.submenu = Some(k);
                            }
                            if app.menu.submenu == Some(k) {
                                submenu_row = Some((k, row.rect));
                            }
                        }
                    }
                });
            }
        });
    inside |= resp.response.contains_pointer() || resp.response.rect.contains(pointer.unwrap_or(Pos2::ZERO));

    if let Some((k, row)) = submenu_row
        && let Some(parent) = list.get(k)
    {
        let enabled: Vec<_> = parent.children.iter().map(|i| i.enabled && !i.is_sep() && !i.is_heading()).collect();
        if had_submenu
            && crate::ui_kit::Menu::navigate(ctx, &enabled, &mut app.menu.submenu_key)
            && let Some(child) = app.menu.submenu_key.and_then(|r| parent.children.get(r))
        {
            picked = Some(child.clone());
        }
        let sub = crate::ui_kit::Menu::submenu(ctx, egui::Id::new("sc_context_submenu"), row, list_height(&parent.children), |ui| {
            egui::ScrollArea::vertical().max_height((screen.height() - 28.0).max(30.0)).show(ui, |ui| {
                for (n, child) in parent.children.iter().enumerate() {
                    if child.is_sep() {
                        crate::ui_kit::Menu::separator(ui);
                        continue;
                    }
                    if child.is_heading() {
                        crate::ui_kit::Menu::heading(ui, &child.label);
                        continue;
                    }
                    // Child commands may share an id (swatches); preserve the parent handle
                    // and publish a distinct identity for every adjacent submenu action.
                    let handle = format!("{}:{}", child.id, child.label);
                    let mut row = kit_item(child, app.menu.submenu_key == Some(n));
                    row.id = &handle;
                    if row.show(ui).clicked() && child.enabled {
                        picked = Some(child.clone());
                    }
                }
            });
        });
        inside |= sub.response.contains_pointer() || pointer.is_some_and(|p| sub.response.rect.contains(p));
    }

    let (primary, secondary, esc) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if secondary && inside && app.menu.radial {
        // A right press on the ring: drag toward a command and release to pick it.
        app.menu.flick = true;
    } else if (primary || secondary) && !inside || esc {
        close(app);
    }
    if let Some(item) = picked {
        close(app);
        run_item(app, &item, at);
    }
}

/// Shared kit surface; legacy callers retain their menu positioning and dispatch.
pub fn menu_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    crate::ui_kit::Menu::show(ui, LIST_W, add)
}

pub fn button(ui: &mut egui::Ui, label: &str, icon: &str) -> egui::Response {
    button_enabled(ui, label, icon, true)
}

pub fn button_enabled(ui: &mut egui::Ui, label: &str, icon: &str, enabled: bool) -> egui::Response {
    let mut item = crate::ui_kit::MenuItem::new(label, label, icon);
    item.enabled = enabled;
    item.show(ui)
}

pub fn list_row(ui: &mut egui::Ui, item: &Item) -> Option<Item> {
    list_row_lit(ui, item, false)
}

pub fn list_row_lit(ui: &mut egui::Ui, item: &Item, lit: bool) -> Option<Item> {
    if item.is_sep() {
        crate::ui_kit::Menu::separator(ui);
        return None;
    }
    if item.is_heading() {
        crate::ui_kit::Menu::heading(ui, &item.label);
        return None;
    }
    let row = kit_item(item, lit);
    (row.show(ui).clicked() && item.enabled).then(|| item.clone())
}

fn kit_item(item: &Item, lit: bool) -> crate::ui_kit::MenuItem<'_> {
    crate::ui_kit::MenuItem {
        id: &item.id,
        label: &item.label,
        icon: &item.icon,
        shortcut: if item.shortcut == "current" { "" } else { &item.shortcut },
        enabled: item.enabled,
        highlighted: lit,
        danger: item.icon == "delete",
        checked: (item.shortcut == "current").then_some(true),
        swatch: item.swatch.map(|c| Color32::from_rgb(c[0], c[1], c[2])),
        submenu: !item.children.is_empty(),
    }
}

fn show_rename(app: &mut SolveApp, ctx: &egui::Context) {
    // Feature-history folders own their editor inside the row. Keep the shared rename state so
    // automation and keyboard handling still use the same path, but do not draw the floating box.
    if app.preferences.timeline_left && matches!(app.menu.rename.as_ref().map(|rename| &rename.what), Some(RenameWhat::TimelineGroup(_))) {
        return;
    }
    let Some(mut r) = app.menu.rename.take() else { return };
    let mut done: Option<bool> = None;
    let resp =
        Container::new(ctx, Surface::Popup).area(egui::Id::new("sc_rename")).fixed_pos(r.at).constrain(true).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                done = crate::ui_kit::rename_menu(ui, &mut r.text, &mut r.focused);
            },
        );
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && r.focused {
        done = Some(true);
    }
    match done {
        Some(true) => commit_rename(app, &r),
        Some(false) => {}
        None => app.menu.rename = Some(r),
    }
}

/// Finish a rename from automation: commit (true) or cancel.
pub fn finish_rename(app: &mut SolveApp, text: Option<&str>, commit: bool) -> bool {
    let Some(mut r) = app.menu.rename.take() else { return false };
    if let Some(s) = text {
        r.text = s.to_string();
    }
    if commit {
        commit_rename(app, &r);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_offset_context_menu_breaks_association_and_allows_trim() {
        use solidcraft_engine::{geom::Vec2, sketch::Shape};

        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let source = h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"connect":false,"infer":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let output =
            h.app.run("sketch.offset", json!({"curves":[source],"distance":2,"chain":false})).unwrap()["curves"][0].as_str().unwrap().to_owned();
        h.app.run("sketch.line", json!({"points":[[10,-5],[10,5]],"connect":false,"infer":false})).unwrap();
        let sid = h.app.session.active_sketch.unwrap();
        let state = h.app.session.world_state();
        let sk = &state.sketch(sid).unwrap().sketch;
        let Shape::Line { a, b } = sk.shape(sk.curve_index(&output).unwrap()).unwrap() else { panic!("offset must be a line") };
        let cut = Vec2::new(15.0, a.y);
        assert!((a.dist(b) - 20.0).abs() < 1e-6);
        assert!(h.app.run("sketch.trim", json!({"curve":output,"at":[cut.x,cut.y]})).is_err());

        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 60.0;
        h.settle();
        h.step(&json!({"click":{"world":[cut.x,cut.y,0]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: output.clone() }]);
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|item| item.id == "sketch.offset.break").expect("offset line must offer Break Offset Association");
        assert_eq!(item.label, "Break Offset Association");
        assert!(item.enabled);
        assert_eq!(item.params, Some(json!({"curve":output})));
        h.step(&json!({"click":{"handle":"menu:sketch.offset.break"}})).unwrap();
        assert!(h.app.session.world_state().sketch(sid).unwrap().sketch.offset_distance(&output).is_none());
        h.app.run("sketch.trim", json!({"curve":output,"at":[cut.x,cut.y]})).unwrap();
        let state = h.app.session.world_state();
        let sk = &state.sketch(sid).unwrap().sketch;
        let remaining = sk.shape(sk.curve_index(&output).unwrap()).unwrap();
        assert!(matches!(remaining, Shape::Line { a, b } if (a.dist(b) - 10.0).abs() < 1e-6), "{remaining:?}");
        assert!(remaining.dist(cut) > 1.0);
    }

    #[test]
    fn solidworks_face_menu_runs_normal_to_selection_and_rejects_curved_faces() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.cylinder", json!({"diameter":30,"height":20})).unwrap();
        let state = h.app.session.world_state();
        let body = state.bodies.first().unwrap();
        let faces = body.body.faces(0.1).unwrap();
        let flat = faces.iter().find(|f| f.plane_normal.is_some()).unwrap();
        let curved = faces.iter().find(|f| f.plane_normal.is_none()).unwrap();
        h.app.session.selection = vec![Sel::Face { body: body.name.clone(), index: flat.index, point: flat.centroid }];
        assert!(items(&h.app, &Target::Viewport).iter().all(|i| i.id != "view.normal_to"));
        h.app.preferences.nav = "solidworks".into();
        h.app.preferences.language = "ja".into();
        h.settle();
        let before = h.app.cam;
        open_for(&mut h.app, pos2(800.0, 400.0), Target::Viewport);
        h.settle();
        assert!(items(&h.app, &Target::Viewport).iter().any(|i| i.id == "view.normal_to" && i.enabled));
        h.step(&json!({"click":{"handle":"menu:view.normal_to"}})).unwrap();
        h.frames(90);
        assert!(h.app.cam.back().dist(flat.plane_normal.unwrap()) < 2e-6);
        assert_eq!((h.app.cam.distance, h.app.cam.fov), (before.distance, before.fov));
        assert!(h.app.viewport.context_menu.is_none());
        assert_eq!(crate::i18n::Locale::for_language("ja").label("Normal to Selection"), "選択アイテムに垂直");
        // Ctrl+8 and the context menu share Look At, including its repeat-to-flip behavior.
        h.step(&json!({"key":"8","cmd":true})).unwrap();
        h.frames(90);
        assert!(h.app.cam.back().dist(-flat.plane_normal.unwrap()) < 2e-6);
        open_for(&mut h.app, pos2(800.0, 400.0), Target::Viewport);
        h.settle();
        h.step(&json!({"click":{"handle":"menu:view.normal_to"}})).unwrap();
        h.frames(90);
        assert!(h.app.cam.back().dist(flat.plane_normal.unwrap()) < 2e-6);
        h.app.session.selection = vec![Sel::Face { body: body.name.clone(), index: curved.index, point: curved.centroid }];
        assert!(items(&h.app, &Target::Viewport).iter().any(|i| i.id == "view.normal_to" && !i.enabled));
    }

    #[test]
    fn viewport_right_click_keeps_list_menu_and_radial_is_opt_in() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.settle();
        let point = h.at(&json!({"world":[0,0,0]})).unwrap();
        h.step(&json!({"click":point,"button":"right"})).unwrap();
        assert!(h.app.viewport.context_menu.is_some(), "ordinary right click must still open a menu");
        let list = describe(&h.app);
        assert!(list["radial"].as_array().unwrap().is_empty());
        assert!(list["items"].as_array().unwrap().iter().any(|item| item["id"] == "ui.origin" && item["enabled"] == true));
        h.step(&json!({"click":{"handle":"menu:ui.origin"}})).unwrap();
        assert!(!h.app.ui.show_origin && h.app.viewport.context_menu.is_none(), "the ordinary menu must execute Hide Origin and close");
        h.call("ui.prefs", json!({"radial_context_menu":true}));
        h.step(&json!({"click":point,"button":"right"})).unwrap();
        assert_eq!(describe(&h.app)["radial"].as_array().unwrap().len(), DIRS);
        open_for(&mut h.app, pos2(point[0] as f32, point[1] as f32), Target::Origin);
        assert!(!h.app.menu.radial, "browser menus remain lists when radial menus are enabled");
    }

    #[test]
    fn disabled_radial_menu_does_not_capture_right_button_hold() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let ctx = crate::fonts::test_context();
        let point = pos2(600.0, 400.0);
        app.viewport.mouse = Some(point);
        for (time, events) in [
            (
                0.0,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            ),
            (1.0, vec![]),
        ] {
            let mut output = ctx.run_ui(egui::RawInput { time: Some(time), events, ..Default::default() }, |_| track_hold(&mut app, &ctx));
            output.textures_delta.clear();
        }
        assert!(app.viewport.context_menu.is_none() && app.menu.press.is_none() && !app.menu.flick);
    }

    #[test]
    fn sectors_go_clockwise_from_the_top() {
        assert_eq!(sector(vec2(0.0, -50.0)), Some(0));
        assert_eq!(sector(vec2(50.0, -50.0)), Some(1));
        assert_eq!(sector(vec2(50.0, 0.0)), Some(2));
        assert_eq!(sector(vec2(0.0, 50.0)), Some(4));
        assert_eq!(sector(vec2(-50.0, 0.0)), Some(6));
        assert_eq!(sector(vec2(-50.0, -50.0)), Some(7));
        assert_eq!(sector(vec2(3.0, 3.0)), None);
    }

    #[test]
    fn list_goes_beside_the_ring_near_the_bottom() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let c = radial_center(pos2(800.0, 300.0), screen);
        assert!(list_place(c, 300.0, screen).y > c.y + RY, "room below");
        for at in [pos2(800.0, 900.0), pos2(1590.0, 900.0), pos2(10.0, 990.0)] {
            let c = radial_center(at, screen);
            let p = list_place(c, 400.0, screen);
            let r = Rect::from_min_size(p, vec2(LIST_W, 400.0));
            assert!(screen.contains_rect(r), "{at:?}: {r:?}");
            let ring = Rect::from_center_size(c, vec2(2.0 * RX, 2.0 * RY));
            assert!(!ring.intersects(r), "{at:?}: list {r:?} over ring {ring:?}");
        }
    }

    #[test]
    fn list_menus_flip_to_stay_in_the_window() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let size = vec2(LIST_W, 300.0);
        assert_eq!(place(pos2(100.0, 100.0), Vec2::ZERO, size, screen), pos2(100.0, 100.0), "room right and below");
        assert_eq!(place(pos2(1500.0, 900.0), Vec2::ZERO, size, screen), pos2(1500.0 - LIST_W, 600.0), "ends at the cursor");
        assert_eq!(place(pos2(1500.0, 900.0), vec2(12.0, 12.0), size, screen), pos2(1488.0 - LIST_W, 588.0), "the gap both ways");
        let tall = vec2(LIST_W, 700.0);
        assert_eq!(place(pos2(100.0, 500.0), Vec2::ZERO, tall, screen).y, 300.0, "room on neither side: against the bottom");
        assert_eq!(place(pos2(100.0, 500.0), Vec2::ZERO, vec2(LIST_W, 1200.0), screen).y, 0.0, "taller than the window: from the top");
    }

    #[test]
    fn radial_menu_stays_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        for at in [pos2(2.0, 2.0), pos2(1199.0, 799.0), pos2(600.0, 400.0), pos2(-50.0, 900.0)] {
            let c = radial_center(at, screen);
            assert!(screen.contains_rect(Rect::from_center_size(c, vec2(2.0 * RX, 2.0 * RY))));
        }
    }
}

#[cfg(test)]
mod workspace_menu_tests {
    use super::*;
    use crate::scenario::{Harness, rect_of};

    #[test]
    fn workspace_titles_use_one_semibold_text_run() {
        let ctx = crate::fonts::test_context();
        let app = SolveApp::new(Default::default(), Default::default());
        let list = items(&app, &Target::Workspace);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui_kit::workspace_menu(ui, &list, None, 1000.0);
        });
        output.textures_delta.clear();
        let titles: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(text) if text.galley.text() == "Design" => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(titles.len(), 1, "workspace titles must never be drawn twice");
        assert_eq!(titles[0].galley.job.sections[0].format.font_id, crate::fonts::semibold(14.0));
    }

    #[test]
    fn workspace_rows_keep_switching_handles_and_disabled_render_behavior() {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1200,"height":1000}));
        h.step(&json!({"click":{"handle":"ribbon:workspace"}})).unwrap();
        let opener = rect_of("ribbon:workspace").unwrap();
        let rows: Vec<_> = ["design", "render", "animation", "simulation", "manufacture", "drawing"]
            .iter()
            .map(|slug| rect_of(&format!("workspace:menu:{slug}")).unwrap())
            .collect();
        assert!(rows.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
        assert!(rows[0].top() > opener.bottom());
        assert_eq!(rows[0].width(), 450.0);
        assert!(items(&h.app, &Target::Workspace).iter().skip(2).all(|item| !item.enabled));
        h.step(&json!({"click":{"handle":"workspace:menu:animation"}})).unwrap();
        assert!(!crate::render_workspace::active(&h.app));
        assert!(h.app.viewport.context_menu.is_some());
        h.step(&json!({"click":{"handle":"workspace:menu:render"}})).unwrap();
        assert!(crate::render_workspace::active(&h.app));
        h.step(&json!({"click":{"handle":"ribbon:workspace"}})).unwrap();
        h.step(&json!({"click":{"handle":"workspace:menu:design"}})).unwrap();
        assert!(!crate::render_workspace::active(&h.app));
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.frames(2);
        h.step(&json!({"click":{"handle":"ribbon:workspace"}})).unwrap();
        assert!(!items(&h.app, &Target::Workspace).iter().find(|i| i.id == "ui.workspace.render").unwrap().enabled);
        h.step(&json!({"click":{"handle":"workspace:menu:render"}})).unwrap();
        assert!(!crate::render_workspace::active(&h.app));
        assert!(h.app.viewport.context_menu.is_some());
    }
}
