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
use solidcraft_engine::{Sel, doc::FeatureKind};

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
    /// Construction geometry (its feature id).
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
    /// An origin axis (X, Y or Z).
    OriginAxis {
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
    /// Insert › Insert Design: the designs open in other tabs, and Browse… (#8570).
    InsertDesign,
    /// Model › Edit Sketch: choose an existing sketch when none is uniquely selected.
    EditSketch,
    /// View › Visual style: the four styles, the current one marked (#8868).
    VisualStyle,
    /// View › Navigation tool: Orbit, Pan, Zoom; the latched one marked (#8868).
    ViewTool,
    /// The mouse mappings (Preferences › Navigation), from the status bar (#7023).
    Navigation,
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
    /// The History controls (first, previous, next, last, and Play when shown).
    HistoryControls,
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
    /// A point cloud.
    PointCloud {
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
    pub(crate) fn heading(label: &str) -> Item {
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
    /// When the pointer came to rest on another row while a flyout was open (it closes the
    /// flyout after [`crate::ui_kit::SUBMENU_CLOSE_DELAY`]).
    submenu_leave: Option<f64>,
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
    /// A selected part has no free joint value for the explicit assembly Move command.
    pub(crate) joint_move_warning: Option<crate::joint_handles::Blocked>,
    /// Identical copies Extract to File found, waiting to be linked too.
    pub link_copies: Option<crate::linked_source::Copies>,
    /// The Properties window: title and measurements.
    pub props: Option<(String, Value)>,
    /// Its measurement while it runs beside the window (#5203).
    pub props_measuring: Option<crate::design_properties::Measuring>,
    /// The viewport menu opened over something: it offers Select Other.
    pub over_geometry: bool,
    /// The plane under the cursor when the viewport menu opened: it offers a sketch on it.
    pub plane: Option<String>,
    /// Where the viewport menu opened and the keys held at that click (Select Other adds with
    /// them).
    opened: Option<(Pos2, crate::tool_modifiers::Held)>,
    /// The viewport menu's right-click selected what was under it: where the menu opened, the
    /// selection before and the one it made. Select Other puts the one before back.
    picked: Option<(Pos2, Vec<Sel>, Vec<Sel>)>,
    /// The button that opened the menu, when it toggles it: a press there is not a press outside,
    /// so its click closes the menu instead of closing and reopening it (#1522).
    opener: Option<Rect>,
    /// The menu was on screen in an earlier frame. Until then a press is the opening click's own.
    shown: bool,
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
    /// Edited in the browser row itself (a double-click on a body), not in the floating box.
    pub(crate) inline: bool,
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
    let key = crate::keymap::display_for(app, "edit.repeat").unwrap_or_default();
    let repeat = match &app.last_command {
        Some((_, label)) => act("ui.repeat", &format!("Repeat {label}"), "redo").key(&key),
        None => act("ui.repeat", "Repeat", "redo").key(&key).on(false),
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
        Target::Body { name } if crate::render_workspace::active(app) => vec![
            cmd(app, "select.set", "Select").with(json!({"items":[Sel::Body {name:name.clone()}]})),
            cmd(app, "select.same_look", "Select Same Look").with(json!({"body":name})),
            Item::sep(),
            act("ui.isolate", "Isolate", "body").with(json!({"bodies":[name]})),
            act(if app.session.visibility.hidden_bodies.contains(name) { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye")
                .with(json!({"bodies":[name]})),
        ],
        Target::Body { name } => body_items(app, std::slice::from_ref(name), true),
        Target::Sketch { id } => sketch_items(app, *id),
        Target::Feature { id } => feature_items(app, *id),
        Target::Plane { id } => plane_items(app, *id),
        Target::OriginPlane { name } => origin_plane_items(app, name),
        Target::OriginAxis { name } => origin_axis_items(app, name),
        Target::Component { id } => component_items(app, *id),
        Target::Occurrence { path } => occurrence_items(app, path),
        Target::Group { id, scope } => crate::browser::group_items(app, *id, scope),
        Target::Folder { component, folder, scope } => crate::browser::folder_items(app, *component, folder, scope),
        Target::Canvas { id } => crate::browser::canvas_items(app, *id),
        Target::PointCloud { id } => crate::browser::point_cloud_items(app, *id),
        Target::Origin => crate::browser::origin_items(app),
        Target::NamedViews => vec![act("ui.newView", "New Named View", "perspective")],
        Target::Section => crate::browser::section_items(app),
        Target::Joint { id } => joint_items(app, *id),
        Target::ToolbarCommand { id } => crate::toolbar_custom::command_items(app, id),
        Target::Toolbar => crate::toolbar_custom::toolbar_items(app),
        Target::HistoryControls => crate::timeline::transport_items(app),
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
        Target::Navigation => crate::prefs::NAV_PRESETS
            .iter()
            .map(|preset| {
                let item = act("ui.navPreset", preset.translated_label(), "").with(json!({ "preset": preset.id }));
                if app.preferences.nav == preset.id { item.key("current") } else { item }
            })
            .collect(),
        Target::NamedView { name } => crate::browser::view_items(app, name),
        Target::InsertDesign => insert_design_items(app),
        Target::EditSketch => {
            let items: Vec<Item> = app
                .session
                .doc
                .features
                .iter()
                .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
                .map(|f| act("ui.editSketch", &f.name, "sketch").with(json!({"sketch": f.id})))
                .collect();
            if items.is_empty() { vec![act("ui.editSketch", app.tr("ui.edit_sketch_empty"), "sketch").on(false)] } else { items }
        }
        Target::VisualStyle => crate::navigation_bar::VISUAL_STYLES
            .iter()
            .enumerate()
            .map(|(index, (style, label))| {
                let current = usize::from(app.ui.visual_style) == index;
                let mut item = cmd(app, "view.visual_style", label).with(json!({"style": style})).key(if current { "current" } else { "" });
                item.icon = crate::navigation_bar::visual_style_icon(u8::try_from(index).unwrap_or_default()).into();
                item
            })
            .collect(),
        Target::ViewTool => {
            let latched = match app.viewport.nav {
                Some(crate::viewport::NavMode::Orbit) => "orbit",
                Some(crate::viewport::NavMode::Pan) => "pan",
                Some(crate::viewport::NavMode::Zoom) => "zoom",
                None => "",
            };
            [("orbit", "Orbit"), ("pan", "Pan"), ("zoom", "Zoom")]
                .into_iter()
                .map(|(tool, label)| {
                    let current = latched == tool;
                    // Picking the latched tool again releases it, as its navigation-bar button does.
                    let mut item = cmd(app, "view.tool", label).with(json!({"tool": if current { "none" } else { tool }})).key(if current {
                        "current"
                    } else {
                        ""
                    });
                    item.icon = tool.into();
                    item
                })
                .collect()
        }
    };
    if !crate::render_workspace::active(app) {
        v.extend(crate::browser::folded_folder_items(app, target));
    }
    if crate::render_workspace::active(app) {
        let (bodies, selection) = match target {
            Target::Component { id } => {
                let bodies = component_bodies(app, *id);
                let selection: Vec<Sel> = bodies.iter().map(|name| Sel::Body { name: name.clone() }).collect();
                (bodies, selection)
            }
            Target::Occurrence { path } => {
                (app.session.occurrence_bodies(path, true).unwrap_or_default(), vec![Sel::Occurrence { path: path.clone() }])
            }
            _ => (Vec::new(), Vec::new()),
        };
        if let Some(body) = bodies.first() {
            v.insert(0, cmd(app, "select.set", "Select").with(json!({"items":selection})));
            v.insert(
                1,
                cmd(app, "select.same_look", "Select Same Look")
                    .with(json!({"body":body}))
                    .on(crate::render_workspace::scene::same_look(app, &bodies)),
            );
        }
    }
    if crate::render_workspace::active(app)
        && let Target::Body { name } = target
    {
        v.push(crate::render_workspace::assign::menu(app, name));
    }
    // Several items of the folder selected: they can be grouped.
    if let Some(p) = crate::browser::selection_group(app, target) {
        v.push(Item::sep());
        v.push(act("ui.groupSelected", "Group Selected", "folder").with(p));
    }
    for item in &mut v {
        if item.id != "ui.appearance" {
            continue;
        }
        // A selected browser body keeps the selection for the editor and its quick choices.
        // A row outside the selection still targets only the body that was right-clicked.
        if let Target::Body { name } = target {
            let bodies = sel_bodies(app);
            if bodies.contains(name) {
                item.params = Some(json!({"bodies": bodies}));
            }
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
    face_feature(app, body, index)
}

/// The feature that made face `index` of `body` (by its world name).
pub(crate) fn face_feature(app: &SolveApp, body: &str, index: usize) -> Option<u64> {
    let state = app.session.world_state();
    let body = state.body(body)?;
    let names = solidcraft_engine::doc::naming::face_names(body);
    let name = names.get(index)?;
    let id = solidcraft_engine::doc::persistent_name::made_by(name)?;
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

/// A browser feature row's menu: the timeline's feature actions, so the browser alone is enough
/// to edit, suppress and roll back.
fn feature_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let feature = app.session.doc.feature(id);
    let exists = feature.is_some();
    let sketch = feature.and_then(|f| crate::timeline::TimelineKind::of(&f.kind).profile_sketch());
    let suppressed = feature.is_some_and(|f| f.suppressed);
    vec![
        act("ui.editFeature", "Edit Feature", "edit").with(json!({"feature":id})).on(exists),
        act("ui.editSketch", "Edit Profile Sketch", "sketch").with(json!({"sketch": sketch.map(|s| s.0)})).on(sketch.is_some()),
        act("ui.rename", "Rename", "rename").key("F2").with(json!({"feature":id})).on(exists),
        act("timeline.suppress", if suppressed { "Unsuppress" } else { "Suppress" }, "suppress")
            .with(json!({"feature":id, "suppressed": !suppressed}))
            .on(exists),
        act("timeline.roll_to", "Roll Back Here", "rollback").with(json!({"feature":id})).on(exists),
        act("ui.cut", "Cut", "cut").key("Ctrl+X").on(exists && crate::edit_clipboard::can_cut(app)),
        act("ui.copy", "Copy", "copy").key("Ctrl+C").on(exists && crate::edit_clipboard::something_to_copy(app)),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({"items": [{"type": "feature", "id": id}]})).on(exists),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({"feature":id})).on(exists),
    ]
}

/// The canvas menus put Repeat on
/// top, then what fits the selection, our extras after a separator.
fn viewport_items(app: &SolveApp) -> Vec<Item> {
    let sel = &app.session.selection;
    let has = |f: fn(&Sel) -> bool| sel.iter().any(f);
    let mut v = Vec::new();
    if let Some((_, label)) = &app.last_command {
        let key = crate::keymap::display_for(app, "edit.repeat").unwrap_or_default();
        v.push(act("ui.repeat", &format!("Repeat {label}"), "redo").key(&key));
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
        if crate::joint_handles::selected_occurrence(app).is_some() {
            v.push(cmd(app, "occurrence.move", "Move"));
        }
        v.push(act("ui.editFeature", "Edit Feature", "edit").with(json!({ "feature": feature })).on(feature.is_some()));
        v.push(act("ui.editSketch", "Edit Profile Sketch", "sketch").with(json!({ "sketch": sketch })).on(sketch.is_some()));
        let planar =
            sel.iter().any(|s| matches!(s, Sel::Face { body, index, .. } if crate::dialogs::planar_face(&app.session, body, *index).is_some()));
        v.push(act("ui.lookAt", "Look At", "lookat").on(planar));
        let marked = matches!(sel.as_slice(), [Sel::Face { body, index, .. }] if crate::print_face_ui::is_print_face(app, body, *index));
        let one_flat = planar && matches!(sel.as_slice(), [Sel::Face { .. }]);
        v.push(cmd(app, "body.print_face", if marked { "Clear Print Face" } else { "Print Face" }).on(one_flat));
        v.push(Item::sep());
        v.push(act("ui.appearance", "Appearance", "appearance").key("A"));
        v.push(act("ui.properties", "Properties", "measure").with(json!({ "bodies": [face] })));
        v.push(act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.delete", "Delete", "delete").key("Del"));
        v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.findBrowser", "Find in Browser", "list").with(json!({ "body": face })));
        v.push(act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": feature, "body": face })).on(feature.is_some()));
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
        // Empty space, or a plane: both lead with Create Sketch (#3032). On a plane it sketches
        // there, with Look At beside it; on empty space it is the toolbar's, which asks for the
        // plane or face.
        let sketch = cmd(app, "sketch.create", "Create Sketch");
        match &app.menu.plane {
            Some(plane) => {
                v.push(sketch.with(json!({ "plane": plane })));
                v.push(act("ui.lookAt", "Look At", "lookat").with(json!({ "plane": plane })));
            }
            None => v.push(sketch),
        }
        v.push(Item::sep());
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
        v.push(act("ui.origin", if app.origin_shown() { "Hide Origin" } else { "Show Origin" }, "origin"));
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
    let locked = bodies.iter().all(|b| app.session.visibility.locked_bodies.contains(b));
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
        act("ui.sendToSlicer", "Send to Slicer", "export").with(json!({ "bodies": names })).on(crate::dialogs_save_mesh::available(app)),
        act("ui.export", "Export…", "export").with(json!({ "bodies": names })),
        Item::sep(),
        // The clipboard beside Delete, Remove and Rename, as in every menu.
        act("ui.cut", "Cut", "cut").key("Ctrl+X").on(crate::edit_clipboard::can_cut(app)),
        act("ui.copy", "Copy", "copy").key("Ctrl+C").on(crate::edit_clipboard::something_to_copy(app)),
        act("ui.paste", "Paste", "paste").key("Ctrl+V").on(crate::edit_clipboard::can_paste(app)),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": items })).on(!locked),
        cmd(app, "solid.remove", "Remove").with(json!({ "bodies": names })).on(!locked),
        act("ui.rename", "Rename", "rename").key("F2").with(json!({ "body": bodies.first() })).on(one),
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
    let profiles = !app.session.visibility.hidden_profiles.contains(&id);
    let dims = app.session.visibility.shown_dimensions.contains(&id);
    let editing = app.session.active_sketch == Some(id);
    let has_profiles = app.session.world_state().sketch(id).is_some_and(|s| !s.profiles.is_empty());
    // This sketch, or every selected sketch when it is one of them, to the part it is on (#10218).
    let selected: Vec<String> = app
        .session
        .selection
        .iter()
        .filter_map(|sel| match sel {
            solidcraft_engine::Sel::Feature { id }
                if app.session.doc.feature(*id).is_some_and(|f| matches!(f.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. })) =>
            {
                Some(id.to_string())
            }
            _ => None,
        })
        .collect();
    let sketches = if selected.contains(&id.to_string()) { selected } else { vec![id.to_string()] };
    let parts = !app.session.doc.components.is_empty();
    let mut items = vec![
        act("ui.extrudeSketch", "Extrude", "extrude").key("E").with(json!({ "sketch": id })).on(has_profiles && !editing),
        Item::sep(),
        act("ui.editSketch", "Edit Sketch", "sketch").with(json!({ "sketch": id })).on(!editing),
        act("ui.redefineSketch", "Redefine Sketch Plane", "plane").with(json!({ "sketch": id })),
        act("ui.exportDxf", "Export DXF…", "export").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.cut", "Cut", "cut").key("Ctrl+X").on(crate::edit_clipboard::can_cut(app)),
        act("ui.copy", "Copy", "copy").key("Ctrl+C").on(crate::edit_clipboard::something_to_copy(app)),
        act("ui.paste", "Paste", "paste").key("Ctrl+V").on(crate::edit_clipboard::can_paste(app)),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": [{"type": "feature", "id": id}] })),
        act("ui.rename", "Rename", "rename").key("F2").with(json!({ "feature": id })),
        crate::browser::sketches_to_parts_item(sketches).on(parts && !editing),
        Item::sep(),
        act("ui.lookAt", "Look At", "lookat").with(json!({ "sketch": id })),
        act("ui.sketchProfile", if profiles { "Hide Profile" } else { "Show Profile" }, "sketch").with(json!({ "sketch": id })),
        act("ui.sketchDims", if dims { "Hide Dimension" } else { "Show Dimension" }, "dimension").with(json!({ "sketch": id })),
        act(if visible { "ui.hideSketch" } else { "ui.showSketch" }, "Show/Hide", "eye").key("V").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": id })),
    ];
    items.retain(|item| parts || item.id != "component.move_sketches.to_parts");
    items
}

/// Construction geometry's row: edit, delete, rename, show or hide; sketch on planes.
fn plane_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let Some(f) = app.session.doc.feature(id) else { return Vec::new() };
    let visible = !app.session.visibility.hidden_origin.contains(&f.name);
    let mut items = Vec::new();
    if matches!(f.kind, solidcraft_engine::doc::FeatureKind::ConstructionPlane { .. }) {
        items.push(cmd(app, "sketch.create", "Create Sketch").with(json!({ "plane": f.name })));
        items.push(Item::sep());
    }
    items.push(act("ui.editFeature", "Edit Feature", crate::timeline::icon_of(&f.kind)).with(json!({ "feature": id })));
    items.push(Item::sep());
    items.extend([
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": [{"type": "feature", "id": id}] })),
        act("ui.rename", "Rename", "rename").key("F2").with(json!({ "feature": id })),
        Item::sep(),
        act("ui.originPart", "Show/Hide", "eye").key("V").with(json!({ "keys": [f.name], "show": !visible })),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "rollback").with(json!({ "feature": id })),
    ]);
    items
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

/// An origin axis: Show/Hide, as on a plane (an axis takes no sketch) (#8382).
fn origin_axis_items(app: &SolveApp, name: &str) -> Vec<Item> {
    let visible = !app.session.visibility.hidden_origin.iter().any(|h| h == name);
    vec![act("ui.originPart", "Show/Hide", "eye").key("V").with(json!({ "keys": [name], "show": !visible }))]
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
    // The component's menu asks its first placement; this row may be another one.
    let movable = !placement.grounded && app.session.held_by_rigid_group(placement.occurrence).is_none();
    for item in &mut entries {
        match item.id.as_str() {
            "ui.activate" => {
                item.params = Some(json!({"component": placement.component, "occurrence": placement.occurrence, "path": path}));
                item.enabled = app.session.active_occurrence_path.as_deref() != Some(path);
            }
            "ui.editInPlace" => item.params = Some(json!({"component": placement.component, "path": path})),
            "occurrence.ground" => {
                item.label = if placement.grounded { "Unground" } else { "Ground" }.into();
                item.params = Some(json!({"occurrence": placement.occurrence, "grounded": !placement.grounded}));
            }
            "ui.moveOccurrence" => {
                item.params = Some(json!({"occurrence": placement.occurrence, "path": path}));
                item.enabled = movable;
            }
            "ui.properties" => item.params = Some(json!({"occurrence": placement.occurrence, "path": path})),
            "ui.turnUpAxis" => {
                item.enabled = movable;
                for child in &mut item.children {
                    child.enabled = movable;
                    if let Some(params) = child.params.as_mut() {
                        params["occurrence"] = json!(placement.occurrence);
                    }
                }
            }
            "occurrence.copy" => item.params = Some(json!({"occurrence": placement.occurrence, "translate": [10,10,0]})),
            "ui.paste" | "ui.pasteNew" => item.params = Some(json!({"component": placement.component, "path": path})),
            "component.create" => item.params = Some(json!({"parent": placement.component, "parent_path": path})),
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
            "ui.export" | "ui.saveMesh" | "ui.sendToSlicer" => item.params = Some(json!({"bodies": bodies})),
            "browser.expand_all" | "browser.collapse_all" => item.params = Some(json!({"path": path})),
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
    // The clipboard: Copy takes the selection (the row right-clicked); Paste puts what was copied
    // inside this component (never a component inside itself or what it holds); Paste New pastes
    // copied components as independent copies.
    let into = json!({ "component": id, "path": app.session.doc.first_path(id) });
    let pastes_here = crate::edit_clipboard::can_paste_into(app, id);
    let mut v = Vec::new();
    if doc.parts.contains_key(&id) {
        v.push(cmd(app, "parts.resize", "Edit").with(json!({"component":id})));
    }
    if id != 0 {
        let grounded = occ.is_some_and(|o| o.grounded);
        let oid = occ.map(|o| o.id).unwrap_or(0);
        v.push(act("ui.activate", "Activate", "component").with(json!({ "component": id })).on(!active));
        if doc.links.contains_key(&id) {
            v.push(act("ui.editInPlace", "Edit in Place", "edit").with(json!({ "component": id })));
            v.push(act("ui.openLinkedDesign", "Open Linked Design", "open").with(json!({ "component": id })));
        }
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
    v.push(
        act("ui.sendToSlicer", "Send to Slicer", "export")
            .with(json!({ "bodies": names }))
            .on(!bodies.is_empty() && crate::dialogs_save_mesh::available(app)),
    );
    v.push(Item::sep());
    // The clipboard, then Delete and Rename, in the same place for the design and its components
    // (the design is copied as a saved design to link, and is neither cut nor deleted).
    if id == 0 {
        v.push(act("ui.copyDesign", "Copy", "copy").key("Ctrl+C"));
    } else {
        v.push(act("ui.cut", "Cut", "cut").key("Ctrl+X").on(crate::edit_clipboard::can_cut(app)));
        v.push(act("ui.copy", "Copy", "copy").key("Ctrl+C").on(crate::edit_clipboard::something_to_copy(app)));
    }
    v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").with(into.clone()).on(pastes_here));
    v.push(
        act("ui.pasteNew", "Paste New", "paste")
            .key("Ctrl+Shift+V")
            .with(into.clone())
            .on(pastes_here && crate::edit_clipboard::pasting_components(app)),
    );
    if id != 0 {
        v.push(act("ui.delete", "Delete", "delete").key("Del").with(json!({ "occurrences": [occ.map(|o| o.id)] })).on(occ.is_some()));
    }
    v.push(act("ui.rename", "Rename", "rename").key("F2").with(json!({ "component": id })));
    v.push(Item::sep());
    v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.showAll", "Show All Bodies", "eye"));
    v.push(act("ui.isolate", "Isolate", "body").with(json!({ "bodies": names })).on(!bodies.is_empty() && id != 0));
    v.push(act("ui.newGroup", "New Group", "folder").with(json!({ "component": id })));
    if id != 0 {
        // Keep the component in the design but leave its features out of the History bar (#10393).
        let hidden = doc.component_history_hidden(id);
        v.push(
            cmd(
                app,
                if hidden { "component.show_history" } else { "component.hide_history" },
                if hidden { "Show its History" } else { "Hide its History" },
            )
            .with(json!({ "component": id })),
        );
    }
    v.push(Item::sep());
    v.push(cmd(app, "browser.expand_all", "Expand All").with(json!({ "component": id })));
    v.push(cmd(app, "browser.collapse_all", "Collapse All").with(json!({ "component": id })));
    v
}

/// Explode Text (#1842): the selected text's outlines become normal sketch lines, arcs and
/// curves, no longer tied to the text, so they can be picked, trimmed and edited one by one.
fn explode_text(app: &SolveApp) -> Option<Item> {
    let link = crate::sketch_text::selected_link(app)?;
    Some(cmd(app, "sketch.break_link", "Explode Text").with(json!({ "link": link })))
}

/// Break Link (#10417): projected geometry among the selection (a model edge or vertex, another
/// sketch's curve) becomes normal sketch geometry, which Trim, drags and dimensions change like
/// any other. Messages already send people here ("use Break Link on it"); text has Explode Text.
fn break_link(app: &SolveApp, sk: &solidcraft_engine::sketch::Sketch, ents: &[String]) -> Option<Item> {
    let projected = |link: Option<&String>| {
        link.and_then(|l| sk.link(l)).is_some_and(|l| !matches!(l.source, solidcraft_engine::sketch::LinkSource::Text { .. }))
    };
    let linked: Vec<&str> = ents
        .iter()
        .filter_map(|id| match sk.curves.iter().find(|c| &c.id == id) {
            Some(c) => projected(c.link.as_ref()).then_some(c.id.as_str()),
            None => sk.resolve_point(id).and_then(|i| sk.points.get(i)).filter(|p| projected(p.link.as_ref())).map(|p| p.id.as_str()),
        })
        .collect();
    (!linked.is_empty()).then(|| cmd(app, "sketch.break_link", "Break Link").with(json!({ "entities": linked })))
}

fn alignment_items(selection: &[Sel]) -> Vec<Item> {
    let Some(points) = crate::selection::alignment_points(selection) else { return Vec::new() };
    vec![
        act("ui.horizontalConstraint", "Horizontal Constraint", "c_hv")
            .with(json!({"points": points, "mode": "horizontal", "if_redundant": "refuse"})),
        act("ui.verticalConstraint", "Vertical Constraint", "c_hv").with(json!({"points": points, "mode": "vertical", "if_redundant": "refuse"})),
    ]
}

/// Sketch entity items: construction, fix, delete, dimension and the constraints the selection
/// can take.
fn sketch_entity_items(app: &SolveApp) -> Vec<Item> {
    use solidcraft_engine::sketch::{ConstraintKind, CurveKind};
    let Some(sid) = app.session.active_sketch else { return Vec::new() };
    let st = app.session.world_state();
    let Some(ss) = st.sketch(sid) else { return Vec::new() };
    if app.session.selection.iter().any(|sel| matches!(sel, Sel::SketchMidpoint { .. })) {
        // A midpoint with a point or curve can also be made coincident with it (#10311).
        let coincident = crate::selection::coincident_operands(&app.session.selection)
            .map(|operands| cmd(app, "sketch.constraint.coincident", "Coincident").with(operands));
        return coincident.into_iter().chain(alignment_items(&app.session.selection)).collect();
    }
    let mut lines = Vec::new();
    let mut rounds = Vec::new();
    let mut curves = Vec::new();
    let mut points = Vec::new();
    // Selection order: a two-entity constraint keeps the first and moves the second.
    let mut picked: Vec<String> = Vec::new();
    for s in &app.session.selection {
        match s {
            Sel::SketchCurve { id, .. } => {
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
    // Dimensions (#320): a selected dimension switches between driving and driven through the
    // engine's `sketch.toggle_driven`, which takes one constraint, so the item appears for a
    // single selected dimension. Non-dimension constraints (coincident, tangent, ...) have no
    // driven state and get no item.
    let dims: Vec<String> = app
        .session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::SketchConstraint { id } => Some(id.clone()),
            _ => None,
        })
        .filter(|id| ss.sketch.constraints.iter().any(|c| &c.id == id && c.kind.value().is_some()))
        .collect();
    let mut v = Vec::new();
    if curves.is_empty() && points.is_empty() {
        if dims.len() == 1 {
            v.push(cmd(app, "sketch.toggle_driven", "Toggle Driven Dimension").with(json!({ "constraint": dims[0] })));
            // A circle's or an arc's radius or diameter switches to the other one (#7499).
            let round = |c: usize| matches!(ss.sketch.curves.get(c).map(|c| &c.kind), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }));
            if ss
                .sketch
                .constraints
                .iter()
                .any(|k| k.id == dims[0] && matches!(k.kind, ConstraintKind::Radius { c, .. } | ConstraintKind::Diameter { c, .. } if round(c)))
            {
                v.push(cmd(app, "sketch.toggle_diameter", "Toggle Radius / Diameter").with(json!({ "constraint": dims[0] })));
            }
        }
        if app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { .. })) {
            v.push(act("ui.delete", "Delete", "delete").key("Del"));
            v.push(Item::sep());
        }
        if crate::sketch_text::selected_entity(app).is_some() {
            v.push(cmd(app, "sketch.edit_text", "Edit Text"));
            v.extend(explode_text(app));
        }
        v.push(cmd(app, "sketch.line", "Line"));
        v.push(cmd(app, "sketch.rectangle.two_point", "Rectangle"));
        v.push(cmd(app, "sketch.circle.center", "Circle"));
        v.push(cmd(app, "sketch.dimension", "Sketch Dimension"));
        v.push(Item::sep());
        v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").on(crate::edit_clipboard::can_paste_sketch(app)));
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
    v.extend(break_link(app, &ss.sketch, &ents));
    if let Some(curve) = crate::sketch_modify_ui::selected_equation(app) {
        v.push(act("ui.editEquationCurve", "Edit Equation Curve", "equation_curve").with(json!({ "curve": curve })));
    }
    v.push(cmd(app, "sketch.move", "Move/Copy").key("M"));
    v.push(cmd(app, "sketch.transform", "Transform").key("G"));
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
    // #6533: a fit point on a fit point spline, where the menu was opened over it.
    if let [curve] = curves.as_slice()
        && ss
            .sketch
            .curve_index(curve)
            .and_then(|i| ss.sketch.curves.get(i))
            .is_some_and(|c| matches!(c.kind, CurveKind::Spline { control: false, .. }))
        && let Some(at) = menu_sketch_point(app, &ss.plane)
    {
        v.push(cmd(app, "sketch.spline.insert_point", "Add Spline Point").with(json!({"curve": curve, "at": [at.x, at.y]})));
    }
    // #8224: any spline of the sketch's own runs the other way.
    if let [curve] = curves.as_slice()
        && ss
            .sketch
            .curve_index(curve)
            .and_then(|i| ss.sketch.curves.get(i))
            .is_some_and(|c| matches!(c.kind, CurveKind::Spline { .. }) && c.link.is_none())
    {
        v.push(cmd(app, "sketch.spline.reverse", "Reverse Spline Direction").with(json!({ "curve": curve })));
    }
    v.push(act("ui.fix", "Fix/Unfix", "c_fix").with(json!({ "entities": ents })));
    // The clipboard beside Delete, as in every menu.
    v.push(act("ui.cut", "Cut", "cut").key("Ctrl+X"));
    v.push(act("ui.copy", "Copy", "copy").key("Ctrl+C"));
    v.push(act("ui.paste", "Paste", "paste").key("Ctrl+V").on(crate::edit_clipboard::can_paste_sketch(app)));
    v.push(act("ui.delete", "Delete", "delete").key("Del"));
    // Three points alone: the angle at the middle one (the vertex) between the lines to the
    // outer two (#10137).
    v.push(
        cmd(app, "sketch.dimension", "Sketch Dimension")
            .with(json!({ "entities": ents }))
            .on(ents.len() <= 2 || (curves.is_empty() && points.len() == 3)),
    );
    // One arc alone: Sketch Dimension gives its radius, this its length along the curve (#6039).
    if let [arc] = ents.as_slice()
        && ss.sketch.curve_index(arc).and_then(|i| ss.sketch.curves.get(i)).is_some_and(|c| matches!(c.kind, CurveKind::Arc { .. }))
    {
        v.push(cmd(app, "sketch.dimension", "Arc Length Dimension").with(json!({ "entities": [arc], "type": "arc_length" })));
    }
    let mut c = Vec::new();
    let n = ents.len();
    let (l, r, p) = (lines.len(), rounds.len(), points.len());
    if l == 1 && n == 1 {
        c.push(cmd(app, "sketch.constraint.horizontal_vertical", "Horizontal/Vertical").with(json!({ "line": lines[0] })));
    }
    if p == 2 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": points[0], "b": points[1] })));
        c.extend(alignment_items(&app.session.selection));
    }
    if p == 1 && curves.len() == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": picked[0], "b": picked[1] })));
        if l == 1 {
            c.push(cmd(app, "sketch.constraint.midpoint", "MidPoint").with(json!({ "point": points[0], "line": lines[0] })));
            // #617: the point can also be aligned with the line (a circle's centre with a square's edge).
            if let Some(align) = crate::tools::point_line_points(&ss.sketch, &json!(points[0]), &lines[0]) {
                c.push(
                    act("ui.horizontalConstraint", "Horizontal Constraint", "c_hv")
                        .with(json!({"points": align, "mode": "horizontal", "if_redundant": "refuse"})),
                );
                c.push(
                    act("ui.verticalConstraint", "Vertical Constraint", "c_hv")
                        .with(json!({"points": align, "mode": "vertical", "if_redundant": "refuse"})),
                );
            }
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
    if r == 1 && p == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.concentric", "Concentric").with(json!({ "a": picked[0], "b": picked[1] })));
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

/// Where on the active sketch's plane the viewport menu was opened (sketch coordinates).
fn menu_sketch_point(app: &SolveApp, plane: &solidcraft_engine::geom::Plane) -> Option<solidcraft_engine::geom::Vec2> {
    let (at, _) = app.menu.opened?;
    let proj = crate::viewport::projection(app, app.viewport.rect?);
    let (o, d) = proj.ray(at);
    Some(plane.to_local(plane.intersect_ray(o, d)?))
}

/// Insert Design's list (#8570): each design open in another tab, inserted with one click (its
/// file name beside it; one not saved yet greyed out, as there is no file to link), then
/// Browse… for a file, as the button did before.
fn insert_design_items(app: &SolveApp) -> Vec<Item> {
    let mut v = vec![Item::heading("Open designs")];
    for d in crate::documents::other_designs(app) {
        let file = d.path.as_deref().map(|p| std::path::Path::new(p).file_name().map_or(p.to_string(), |f| f.to_string_lossy().into_owned()));
        let note = match (&file, d.unsaved) {
            (None, _) => "Not saved yet".to_string(),
            (Some(_), true) => "Unsaved changes".to_string(),
            (Some(file), false) => file.clone(),
        };
        v.push(
            act("ui.insertOpenDesign", &d.name, "component")
                .key(&note)
                .on(d.path.is_some())
                .with(json!({"path": d.path, "name": d.name, "unsaved": d.unsaved})),
        );
    }
    v.push(Item::sep());
    v.push(act("ui.insertBrowse", "Browse…", "open"));
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
        "appearance.library" if crate::render_workspace::active(app) => {
            app.start("appearance.library");
        }
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
            // The list picks instead of the right-click.
            if let Some((opened, before, made)) = app.menu.picked.take()
                && opened == at
                && app.session.selection == made
            {
                let _ = app.run("select.set", json!({ "items": before }));
            }
            let held = app.menu.opened.filter(|(opened, _)| *opened == at).map(|(_, held)| held).unwrap_or_default();
            crate::select_other::open(app, at, held);
        }
        "ui.delete" if item.params.is_some() => crate::delete::request(app, p),
        "ui.delete" => delete(app),
        "ui.horizontalConstraint" | "ui.verticalConstraint" => {
            let selection = app.session.selection.clone();
            let keep_midpoints = selection.iter().any(|s| matches!(s, Sel::SketchMidpoint { .. }));
            if app.run("sketch.constraint.horizontal_vertical", p).is_ok() && keep_midpoints {
                let _ = app.run("select.set", json!({"items": selection}));
            }
        }
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
        "ui.paste" | "ui.pasteNew" => {
            // A component's menu pastes into it (that placement); the others into the active one.
            let into = id_of(&p, "component").map(|c| {
                let path = p.get("path").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect());
                (c, path.unwrap_or_else(|| app.session.doc.first_path(c)))
            });
            match id_of(&p, "group") {
                // A Bodies group's menu: into the group too.
                Some(group) => {
                    let folder = p.get("folder").and_then(Value::as_str).unwrap_or("bodies");
                    let _ = crate::edit_clipboard::paste_into_group(app, Some(at), into, Some(group), folder);
                }
                None => drop(crate::edit_clipboard::paste_into(app, Some(at), item.id == "ui.pasteNew", into)),
            }
        }
        "ui.editEquationCurve" => {
            if let Some(id) = p.get("curve").and_then(Value::as_str) {
                crate::sketch_modify_ui::edit_equation(app, id);
            }
        }
        "ui.editSketchPattern" => {
            if let Some(id) = p.get("pattern").and_then(Value::as_str) {
                crate::sketch_modify_ui::edit_pattern(app, id);
            }
        }
        "ui.hide" => {
            // Through the command, so hiding is one undo step (#2219).
            let _ = app.run("browser.visibility", json!({ "items": bodies, "visible": false }));
        }
        "ui.show" => {
            let _ = app.run("browser.visibility", json!({ "items": bodies, "visible": true }));
        }
        "ui.showAll" => {
            // One command and so one undo step: everything shown, folders and placements included.
            let _ = app.run("browser.visibility", json!({ "folder": "all", "visible": true }));
        }
        "ui.isolate" => {
            // The picked bodies stay (shown first if they were hidden), everything else goes.
            let others: Vec<String> = app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|b| !bodies.contains(b)).collect();
            if !bodies.is_empty() {
                let _ = app.run("browser.visibility", json!({ "items": bodies, "visible": true }));
            }
            if !others.is_empty() {
                let _ = app.run("browser.visibility", json!({ "items": others, "visible": false }));
            }
        }
        "ui.lock" => {
            let locked = bodies.iter().all(|b| app.session.visibility.locked_bodies.contains(b));
            if locked {
                app.session.visibility.locked_bodies.retain(|b| !bodies.contains(b));
            } else {
                app.session
                    .visibility
                    .locked_bodies
                    .extend(bodies.into_iter().filter(|b| !app.session.visibility.locked_bodies.contains(b)).collect::<Vec<_>>());
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
            let fallback = if bodies.len() == 1 { bodies.join("") } else { format!("{} bodies", bodies.len()) };
            crate::design_properties::open_object(app, command, &params, fallback);
        }
        "ui.designProperties" => app.start("document.metadata"),
        "ui.insertOpenDesign" => {
            let (Some(path), name) = (p.get("path").and_then(Value::as_str), p.get("name").and_then(Value::as_str).unwrap_or_default()) else {
                return;
            };
            // The design is open here, so its file's place is already the user's choice.
            let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(path).into_iter().collect();
            let inserted = solidcraft_engine::io::vfs::allowing_hosts(&hosts, || app.run("component.insert_design", json!({ "path": path })));
            if inserted.is_ok() {
                app.fit_view();
                if p.get("unsaved").and_then(Value::as_bool) == Some(true) {
                    app.set_status(crate::i18n::format("ui.insert_design.saved_file", &[("name", name)]), false);
                }
            }
        }
        "ui.insertBrowse" => app.pick_design_to_insert(),
        "ui.export" => {
            let name = bodies.first().cloned().unwrap_or_else(|| app.session.doc.name.clone());
            app.pick_save_command(&format!("{name}.step"), &["step", "stp", "igs", "stl", "3mf", "obj"], "file.export", json!({"bodies": bodies}));
        }
        "ui.findBrowser" => {
            if let Some(b) = p.get("body").and_then(Value::as_str) {
                crate::browser::reveal_body(app, b);
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": b}]}));
            }
        }
        "ui.findTimeline" => {
            let body = p.get("body").and_then(Value::as_str);
            let f = id_of(&p, "feature").or_else(|| body.and_then(|b| body_feature(app, b)));
            if let Some(f) = f {
                if app.preferences.history_in_browser {
                    crate::browser::reveal_in_history(app, f);
                } else {
                    app.ui.show_timeline = true;
                    crate::timeline_left::reveal_feature(app, f);
                    // From a face or body, the Browser shows the feature in that body's copy. From a
                    // sketch, plane or feature row there is no body: that row is the Browser's view.
                    if let Some(body) = body {
                        crate::browser::reveal_feature(app, f, body);
                    }
                }
                let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": f}]}));
                app.tree.timeline_reveal = Some(f);
            }
        }
        "ui.tangentChain" => tangent_chain(app),
        "ui.origin" => app.set_origin_shown(!app.origin_shown()),
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
                if app.session.visibility.shown_dimensions.contains(&id) {
                    app.session.visibility.shown_dimensions.retain(|x| *x != id);
                } else {
                    app.session.visibility.shown_dimensions.push(id);
                }
            }
        }
        "ui.sketchProfile" => {
            if let Some(id) = id_of(&p, "sketch") {
                if app.session.visibility.hidden_profiles.contains(&id) {
                    app.session.visibility.hidden_profiles.retain(|x| *x != id);
                } else {
                    app.session.visibility.hidden_profiles.push(id);
                }
            }
        }
        "ui.activate" => {
            if id_of(&p, "component").is_some() {
                let _ = app.run("component.activate", p);
            }
        }
        "ui.editInPlace" => {
            if let Some(component) = id_of(&p, "component") {
                let path = p["path"].as_array().map(|path| path.iter().filter_map(Value::as_u64).collect());
                crate::edit_in_place::open(app, component, path);
            }
        }
        "ui.openLinkedDesign" => {
            let Some(component) = id_of(&p, "component") else { return };
            let links = match app.session.execute("component.links", &json!({})) {
                Ok(links) => links,
                Err(e) => {
                    app.set_status(e.to_string(), true);
                    return;
                }
            };
            let Some(link) = links["links"].as_array().and_then(|links| links.iter().find(|link| link["component"] == component)) else {
                app.set_status("This component is no longer linked to a design", true);
                return;
            };
            // A link on a network server not allowed yet asks first.
            crate::network_prompt::open_linked(app, link);
        }
        "ui.moveOccurrence" => {
            if let Some(o) = id_of(&p, "occurrence") {
                if crate::joint_handles::begin(app, o) {
                    return;
                }
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
        "ui.toolbarPin" | "ui.toolbarRemove" | "ui.toolbarReset" | "ui.toolbarCustomize" | "ui.toolbarTab" | "ui.shortcutPin" => {
            crate::toolbar_custom::run(app, &item.id, &p)
        }
        "ui.navPreset" => {
            // Picked here, a preset sets what Preferences › Navigation sets for it.
            if let Some(preset) = crate::prefs::NAV_PRESETS.iter().find(|preset| p.get("preset").and_then(Value::as_str) == Some(preset.id)) {
                crate::prefs::choose_nav_preset(&mut app.preferences, preset);
            }
        }
        "ui.historyPlayButton" => {
            if let Err(e) = crate::prefs::set(app, &json!({ "history_play_button": p["show"] })) {
                app.set_status(e, true);
            }
        }
        "ui.historyShowHidden" => app.tree.history_show_hidden = p["show"].as_bool().unwrap_or(false),
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
            app.set_origin_shown(true);
            let _ = app.run("browser.visibility", json!({ "items": ["O", "X", "Y", "Z", "XY", "XZ", "YZ"], "visible": true }));
        }
        "ui.originPart" => {
            let show = p.get("show").and_then(Value::as_bool).unwrap_or(true);
            let keys = strs(&p, "keys");
            if !keys.is_empty() {
                let _ = app.run("browser.visibility", json!({ "items": keys, "visible": show }));
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
        "ui.saveMesh" | "ui.sendToSlicer" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start(if item.id == "ui.saveMesh" { "file.save_mesh" } else { "file.send_to_slicer" });
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
            } else if let Some(plane) = p.get("plane").and_then(Value::as_str).and_then(solidcraft_engine::geom::Plane::named) {
                let mut to = app.cam.looking_from(crate::dialogs::facing(app, plane.normal(), true));
                to.target = plane.origin;
                app.animate_to(to);
            } else if p.get("sketch").is_none() {
                crate::viewport::look_at_selection(app);
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
        "ui.editPointCloud" => {
            if let Some(c) = id_of(&p, "cloud") {
                app.tree.cloud_panel = Some(crate::browser::CloudPanel::new(app, c));
            }
        }
        "ui.editCanvas" | "ui.calibrateCanvas" => {
            if let Some(c) = id_of(&p, "canvas") {
                let calibrate = item.id == "ui.calibrateCanvas";
                app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, c, calibrate));
                if calibrate {
                    app.set_status(crate::i18n::tr("browser.calibrate_click_a"), false);
                }
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
        // They recompute the history from the changed feature on: off the frame, as the timeline does.
        "timeline.suppress" | "timeline.roll_to" if item.params.is_some() => drop(app.run_heavy(&item.id, p)),
        "component.move_sketches.to_parts" => sketches_to_parts(app, p),
        id if item.params.is_some() => {
            let _ = app.run(id, p);
        }
        id => app.start(id),
    }
}

/// Move Sketches to the Part They're On, and say in the status bar how many moved and why the
/// others stayed (#10218).
fn sketches_to_parts(app: &mut SolveApp, p: Value) {
    let out = match app.run("component.move_sketches.to_parts", p) {
        Ok(out) => out,
        Err(error) => return app.set_status(crate::friendly_error("component.move_sketches.to_parts", &error), true),
    };
    let count = |key: &str| out.get(key).and_then(Value::as_u64).unwrap_or(0).to_string();
    let mut why: Vec<String> = Vec::new();
    for kept in out.get("kept").and_then(Value::as_array).into_iter().flatten() {
        let code = kept.get("code").and_then(Value::as_str).unwrap_or_default();
        let reason = crate::i18n::tr(&format!("browser.sketches_to_parts_{code}")).to_string();
        if !why.contains(&reason) {
            why.push(reason);
        }
    }
    let message = if why.is_empty() {
        crate::i18n::format("browser.sketches_to_parts_moved", &[("moved", &count("moved"))])
    } else {
        crate::i18n::format("browser.sketches_to_parts_result", &[("moved", &count("moved")), ("stayed", &count("stayed")), ("why", &why.join("; "))])
    };
    app.set_status(message, false);
}

/// Delete what is selected: sketch entities, features, and bodies (a Remove feature).
pub fn delete(app: &mut SolveApp) {
    crate::delete::delete_selection(app);
}

/// A shortcut key pressed while a menu is open: the row of that menu showing `key` runs on the
/// menu's target, as a click on it would, and the menu closes. A folder, the Origin folder or an
/// origin plane is not selected by the right-click that opens its menu, so the key's own action
/// (V on the selection) would miss it. Returns whether the open menu has a row for the key; a
/// disabled row takes the key and does nothing, as a click on it does.
pub fn press_shortcut(app: &mut SolveApp, key: &str) -> bool {
    let Some(at) = app.viewport.context_menu else { return false };
    let target = app.menu.target.clone();
    let Some(item) = items(app, &target).into_iter().find(|i| i.shortcut == key && i.children.is_empty()) else { return false };
    if item.enabled {
        close(app);
        run_item(app, &item, at);
    }
    true
}

/// Show/Hide (the V key): what is selected in the viewport or picked in the browser flips its
/// visibility (bodies, faces' and edges' bodies, sketches, construction and origin planes and
/// axes, components' bodies, canvases, joints' glyphs). When some are shown and some hidden, all are hidden.
/// A clicked folder takes precedence, and toggles its own visibility without changing its items.
pub fn toggle_visibility(app: &mut SolveApp) {
    use solidcraft_engine::doc::FeatureKind;
    if crate::browser::toggle_picked_folder(app) {
        return;
    }
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
            Sel::Plane { name } | Sel::Axis { name } | Sel::ConstructionPoint { name } => Some(name.clone()),
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
    // One command for the picked bodies, construction items and sketches: the same step the
    // menus run, so hiding from the viewport (V) is undone like any other hide (#2219).
    let mut items: Vec<Value> = bodies.iter().map(|b| json!(b)).collect();
    items.extend(origin.iter().map(|o| json!(o)));
    items.extend(sketches.iter().map(|s| json!(s)));
    if !items.is_empty() {
        let _ = app.run("browser.visibility", json!({ "items": items, "visible": !hide }));
    }
    if !joints.is_empty() {
        let _ = app.run("browser.visibility", json!({ "joints": joints, "visible": !hide }));
    }
    for c in canvases {
        let _ = app.run("canvas.edit", json!({ "canvas": c, "visible": !hide }));
    }
}

/// Rename what is selected (F2): one body, sketch or feature, construction plane, component
/// picked in the browser, canvas, or a group whose Browser row was clicked.
pub fn rename_selection(app: &mut SolveApp, at: Pos2) -> bool {
    // A clicked group row takes precedence, as it does for V: groups have no selection of their own.
    if let Some(g) = crate::browser::picked_group(app) {
        start_rename(app, RenameWhat::Group(g), at);
        return true;
    }
    let what = match (app.session.selection.as_slice(), app.tree.picked_components.as_slice(), app.tree.picked_canvases.as_slice()) {
        ([Sel::Body { name }], [], []) => RenameWhat::Body(name.clone()),
        ([Sel::Feature { id }], [], []) => RenameWhat::Feature(*id),
        ([Sel::Occurrence { path }], [], []) => match app.session.occurrence_placement(path) {
            Ok(p) => RenameWhat::Component(p.component),
            Err(_) => return false,
        },
        ([Sel::Plane { name } | Sel::Axis { name } | Sel::ConstructionPoint { name }], [], []) => match app.session.doc.find_feature(name) {
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
    app.menu.rename = Some(Rename { what, text, at, focused: false, inline: false });
}

pub(crate) fn commit_rename(app: &mut SolveApp, r: &Rename) {
    let name = r.text.trim().to_string();
    if name.is_empty() {
        return;
    }
    match &r.what {
        RenameWhat::Body(old) => {
            // The command renames the body where it is hidden or locked too.
            drop(app.run_heavy("body.rename", json!({ "body": old, "name": name })));
        }
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

/// Right-clicking the MID of an already selected edge keeps that geometry selection.
fn context_target_selected(selection: &[Sel], target: &Sel) -> bool {
    selection.iter().any(|selected| {
        crate::selection::same_item(selected, target)
            || matches!((selected, target), (Sel::SketchCurve { id: a, .. }, Sel::SketchMidpoint { id: b }) if a == b)
    })
}

/// The body a viewport pick belongs to: a face, edge or vertex carries its body's name. The
/// origin point is a vertex of no body, so an empty name is not a body.
fn pick_body(sel: &Sel) -> Option<&str> {
    match sel {
        Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } if !body.is_empty() => Some(body),
        _ => None,
    }
}

/// Is the pick already part of the selection? Beside the item itself (and an edge's midpoint,
/// above), a face, edge or vertex of a selected body counts, whether the body is selected on its
/// own or through an occurrence: the menu is then about the bodies, and the right-click leaves
/// the selection alone (#5849).
fn in_selection(app: &SolveApp, sel: &Sel) -> bool {
    if context_target_selected(&app.session.selection, sel) {
        return true;
    }
    let Some(body) = pick_body(sel) else { return false };
    app.session.selection.iter().any(|x| match x {
        Sel::Body { name } => name == body,
        Sel::Occurrence { path } => app.session.occurrence_bodies(path, true).is_ok_and(|b| b.iter().any(|n| n == body)),
        _ => false,
    })
}

/// Open the viewport menu at a screen position. A right-click on something not selected
/// selects it first, so the menu is about what is under the cursor; a face or edge already in a
/// multi-selection keeps the whole selection (#3733), as does one of a selected body (#5849). `held`: the keys held at that click.
pub fn open(app: &mut SolveApp, at: Pos2, held: crate::tool_modifiers::Held) {
    app.menu.opened = Some((at, held));
    app.menu.picked = None;
    let before = app.session.selection.clone();
    app.menu.over_geometry = app.viewport.hover.is_some();
    // A right-click leaves a plane unselected, but the menu offers a sketch on it.
    app.menu.plane = app.viewport.hover.clone().and_then(|h| crate::viewport::candidate(app, &[h])).and_then(|(_, sel)| match sel {
        Sel::Plane { name } => Some(name),
        _ => None,
    });
    crate::select_other::close(app);
    if let Some(sel) = crate::viewport::sketch_constraint_at(app, at, app.viewport.hover.as_ref()) {
        if !in_selection(app, &sel) {
            let _ = app.run("select.set", json!({ "items": [sel] }));
            app.menu.picked = Some((at, before, app.session.selection.clone()));
        }
    } else if app.session.active_sketch.is_none()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && !in_selection(app, &sel)
        && !matches!(sel, Sel::Plane { .. } | Sel::Axis { .. })
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
        app.menu.picked = Some((at, before, app.session.selection.clone()));
    } else if app.session.active_sketch.is_some()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && (matches!(sel, Sel::SketchCurve { .. } | Sel::SketchPoint { .. } | Sel::SketchMidpoint { .. })
            || crate::sketch_text::entity_for_selection(app, &sel).is_some())
        && !in_selection(app, &sel)
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
        app.menu.picked = Some((at, before, app.session.selection.clone()));
    }
    open_for(app, at, Target::Viewport);
}

/// Open a menu for a target (the radial ring only for the viewport).
pub fn open_for(app: &mut SolveApp, at: Pos2, target: Target) {
    app.menu.key_row = None;
    app.menu.submenu = None;
    app.menu.submenu_key = None;
    app.menu.submenu_leave = None;
    app.menu.radial = target == Target::Viewport && app.preferences.radial_context_menu;
    app.menu.target = target;
    app.menu.flick = false;
    app.menu.opener = None;
    app.menu.shown = false;
    app.viewport.context_menu = Some(at);
}

/// A button's menu: a click opens it, and a click on the same button while it is open closes it.
pub fn toggle_for(app: &mut SolveApp, opener: Rect, at: Pos2, target: Target) {
    if open_from(app, opener, &target) {
        close(app);
    } else {
        open_for(app, at, target);
        app.menu.opener = Some(opener);
    }
}

/// The button's menu is open, so the button can show it pressed.
pub fn open_from(app: &SolveApp, opener: Rect, target: &Target) -> bool {
    app.viewport.context_menu.is_some() && app.menu.target == *target && app.menu.opener == Some(opener)
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
            open(app, p0, crate::tool_modifiers::Held::from_modifiers(ctx.input(|i| i.modifiers)));
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
        if target == Target::Viewport { open(app, at, crate::tool_modifiers::Held::default()) } else { open_for(app, at, target) }
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
    let leaving = app.menu.submenu_leave.is_some();
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
    let mut other_row = false;
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
                        if item.is_sep() || item.is_heading() {
                            let _ = list_row_lit(ui, item, false);
                        } else if item.children.is_empty() {
                            let row = kit_item(item, key_row == Some(k)).show(ui);
                            if row.clicked() && item.enabled {
                                picked = Some(item.clone());
                            }
                            other_row |= row.hovered();
                        } else {
                            let row = kit_item(item, key_row == Some(k) || (app.menu.submenu == Some(k) && !leaving)).show(ui);
                            // Appearance keeps its direct editor action as well as hover shortcuts.
                            if item.enabled && row.clicked() && item.id == "ui.appearance" {
                                picked = Some(item.clone());
                            } else if item.enabled && (crate::ui_kit::Menu::flyout_hover(&row) || row.clicked()) {
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

    // A flyout closes once the pointer moves on to another row of its menu and rests there
    // (#6657). A pointer that is still (the flyout opened with the keys) leaves it open.
    let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO);
    if app.menu.submenu.is_some() && other_row && (moved || app.menu.submenu_leave.is_some()) {
        if crate::ui_kit::Menu::flyout_leaving(ctx, &mut app.menu.submenu_leave) {
            app.menu.submenu = None;
            app.menu.submenu_key = None;
            submenu_row = None;
        }
    } else {
        app.menu.submenu_leave = None;
    }

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

    let shown = std::mem::replace(&mut app.menu.shown, true);
    let (primary, secondary, esc) = ctx.input(|i| {
        (
            shown && i.pointer.button_pressed(egui::PointerButton::Primary),
            shown && i.pointer.button_pressed(egui::PointerButton::Secondary),
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
    if app.menu.rename.as_ref().is_some_and(|r| r.inline) {
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

    /// #320: a selected sketch dimension gets the Toggle Driven Dimension item, it round-trips
    /// driving -> driven -> driving, and non-dimension selections get no such item.
    #[test]
    fn dimension_menu_toggles_driving_driven_and_other_selections_get_no_item() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let sid = h.app.session.active_sketch.unwrap();
        let l = h.app.run("sketch.line", json!({"points":[[0,0],[20,0]],"connect":false,"infer":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let dparam = h.app.run("sketch.dimension", json!({"entities":[l],"value":10})).unwrap()["param"].as_str().unwrap().to_owned();
        let dim = h.app.run("sketch.glyphs", json!({})).unwrap()["glyphs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["param"].as_str() == Some(dparam.as_str()))
            .and_then(|x| x["id"].as_str())
            .expect("the dimension glyph carries its constraint id")
            .to_owned();
        let horiz = h.app.run("sketch.constrainer", json!({"entities":[l]})).unwrap()["constraint"].as_str().unwrap().to_owned();
        let is_driven = |h: &crate::scenario::Harness| {
            h.app.session.world_state().sketch(sid).unwrap().sketch.constraints.iter().any(|c| c.id == dim && c.driven)
        };

        // A selected dimension offers the toggle, aimed at that constraint.
        h.app.session.selection = vec![Sel::SketchConstraint { id: dim.clone() }];
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|i| i.id == "sketch.toggle_driven").expect("a selected dimension must offer Toggle Driven Dimension");
        assert!(item.enabled);
        assert_eq!(item.params, Some(json!({"constraint": dim})));

        // Running it flips the state both ways.
        run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(is_driven(&h), "the dimension is driven now");
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|i| i.id == "sketch.toggle_driven").expect("a driven dimension still offers the toggle");
        run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(!is_driven(&h), "and driving again");

        // A non-dimension constraint, and a plain curve, get no toggle item.
        h.app.session.selection = vec![Sel::SketchConstraint { id: horiz }];
        assert!(!items(&h.app, &Target::Viewport).iter().any(|i| i.id == "sketch.toggle_driven"), "a horizontal constraint is not a dimension");
        h.app.session.selection = vec![Sel::SketchCurve { id: l, sketch: None }];
        assert!(!items(&h.app, &Target::Viewport).iter().any(|i| i.id == "sketch.toggle_driven"));
    }

    /// #7499: a selected circle's dimension offers Toggle Radius / Diameter, which switches it
    /// both ways; a line's length does not.
    #[test]
    fn a_circle_dimension_menu_toggles_radius_and_diameter() {
        use solidcraft_engine::sketch::ConstraintKind;
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let sid = h.app.session.active_sketch.unwrap();
        let c = h.app.run("sketch.circle.center", json!({"center":[0,0],"radius":10})).unwrap()["curves"][0].as_str().unwrap().to_owned();
        let l = h.app.run("sketch.line", json!({"points":[[30,0],[50,0]],"connect":false,"infer":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let pc = h.app.run("sketch.dimension", json!({"entities":[c]})).unwrap()["param"].as_str().unwrap().to_owned();
        let pl = h.app.run("sketch.dimension", json!({"entities":[l],"value":20})).unwrap()["param"].as_str().unwrap().to_owned();
        let dim = |h: &crate::scenario::Harness, p: &str| {
            h.app.session.world_state().sketch(sid).unwrap().sketch.constraints.iter().find(|c| c.param.as_deref() == Some(p)).unwrap().clone()
        };
        let did = dim(&h, &pc).id;

        h.app.session.selection = vec![Sel::SketchConstraint { id: did.clone() }];
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|i| i.id == "sketch.toggle_diameter").expect("a circle's dimension must offer Toggle Radius / Diameter");
        assert!(item.enabled);
        assert_eq!(item.params, Some(json!({"constraint": did})));
        run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(matches!(dim(&h, &pc).kind, ConstraintKind::Radius { .. }), "the diameter is a radius now");
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|i| i.id == "sketch.toggle_diameter").expect("a radius still offers the toggle");
        run_item(&mut h.app, item, egui::pos2(0.0, 0.0));
        assert!(matches!(dim(&h, &pc).kind, ConstraintKind::Diameter { .. }), "and a diameter again");

        h.app.session.selection = vec![Sel::SketchConstraint { id: dim(&h, &pl).id }];
        let menu = items(&h.app, &Target::Viewport);
        assert!(menu.iter().any(|i| i.id == "sketch.toggle_driven"));
        assert!(!menu.iter().any(|i| i.id == "sketch.toggle_diameter"), "a length is no radius");
    }

    #[test]
    fn units_toolbar_start_offers_choices_and_one_undo_step() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.call("ui.start", json!({"command":"document.units"}));
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(h.app.menu.target, Target::Units);
        assert!(h.app.viewport.context_menu.is_some());
        let menu = describe(&h.app);
        let choices = menu["items"].as_array().unwrap();
        assert_eq!(choices.iter().map(|item| item["label"].as_str().unwrap()).collect::<Vec<_>>(), crate::prefs::UNITS);
        assert!(choices.iter().all(|item| item["enabled"] == true));
        assert!(h.app.session.undo.is_empty());
        h.call("ui.menuPick", json!({"item":"in"}));
        assert_eq!(h.app.session.doc.units, "in");
        assert!(h.app.viewport.context_menu.is_none());
        assert_eq!(h.app.session.undo.len(), 1);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.units, "mm");
        h.app.run("edit.redo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.units, "in");
    }

    #[test]
    fn units_toolbar_menu_is_anchored_and_programmatic_calls_stay_headless() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let missing = h.call("engine.execute", json!({"command":"document.units","params":{}}));
        assert_eq!(missing["ok"], false);
        assert!(h.app.viewport.context_menu.is_none());
        let changed = h.call("engine.execute", json!({"command":"document.units","params":{"units":"cm"}}));
        assert_eq!(changed["ok"], true);
        assert_eq!(h.app.session.doc.units, "cm");
        assert!(h.app.viewport.context_menu.is_none());
        h.app.status = None;
        h.step(&json!({"click":{"handle":"tab:MANAGE"}})).unwrap();
        let button = crate::scenario::rect_of("button:document.units").unwrap();
        h.step(&json!({"click":{"handle":"button:document.units"}})).unwrap();
        assert_eq!(h.app.viewport.context_menu, Some(button.left_bottom()));
        assert_eq!(h.app.menu.target, Target::Units);
        assert!(!h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
        assert_eq!(items(&h.app, &Target::Units).iter().find(|item| item.label == "cm").unwrap().shortcut, "current");
        h.step(&json!({"click":{"handle":"button:document.units"}})).unwrap();
        assert!(h.app.viewport.context_menu.is_none());
    }

    #[test]
    fn visual_style_toolbar_start_offers_the_four_styles_not_an_error() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        let error = |h: &crate::scenario::Harness| h.app.status.as_ref().is_some_and(|(_, _, error)| *error);
        h.app.status = None;
        h.call("ui.start", json!({"command":"view.visual_style"}));
        assert!(!error(&h), "{:?}", h.app.status);
        assert_eq!(h.app.menu.target, Target::VisualStyle);
        assert!(h.app.viewport.context_menu.is_some());
        let menu = describe(&h.app);
        let labels: Vec<_> = menu["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_owned()).collect();
        assert_eq!(labels, ["Shaded with edges", "Shaded", "Wireframe", "Shaded with hidden edges"]);
        let rows = items(&h.app, &Target::VisualStyle);
        assert_eq!(rows.iter().filter(|i| i.shortcut == "current").map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Shaded with edges"]);
        h.call("ui.menuPick", json!({"item":"Wireframe"}));
        assert_eq!(h.app.ui.visual_style, 2);
        assert!(h.app.viewport.context_menu.is_none());
        assert!(!error(&h));
        assert!(h.app.session.undo.is_empty(), "a display style is not a design edit");
        // The real ribbon path: the VIEW tab's panel drop-down, then the Visual style row.
        h.step(&json!({"click":{"handle":"tab:VIEW"}})).unwrap();
        h.step(&json!({"click":{"handle":"panel:VIEW:VIEWS"}})).unwrap();
        h.step(&json!({"click":{"handle":"toolbar:view.visual_style"}})).unwrap();
        assert!(!error(&h), "{:?}", h.app.status);
        assert_eq!(h.app.menu.target, Target::VisualStyle);
        assert!(h.app.viewport.context_menu.is_some());
        assert_eq!(items(&h.app, &Target::VisualStyle).iter().find(|i| i.label == "Wireframe").unwrap().shortcut, "current");
        // The drop-down closes on a pick, so the toggle is checked with the opener's rect directly.
        let button = egui::Rect::from_min_size(egui::pos2(40.0, 60.0), egui::vec2(28.0, 28.0));
        h.app.start_at("view.visual_style", Some(button));
        assert_eq!(h.app.viewport.context_menu, Some(button.left_bottom()));
        h.app.start_at("view.visual_style", Some(button));
        assert!(h.app.viewport.context_menu.is_none(), "a second click closes the menu");
        let missing = h.call("engine.execute", json!({"command":"view.visual_style","params":{}}));
        assert_eq!(missing["ok"], false);
        assert!(h.app.viewport.context_menu.is_none());
    }

    #[test]
    fn view_tool_toolbar_start_offers_orbit_pan_zoom_and_releases_the_latched_one() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.status = None;
        let error = |h: &crate::scenario::Harness| h.app.status.as_ref().is_some_and(|(_, _, error)| *error);
        h.call("ui.start", json!({"command":"view.tool"}));
        assert!(!error(&h), "{:?}", h.app.status);
        assert_eq!(h.app.menu.target, Target::ViewTool);
        let menu = describe(&h.app);
        let labels: Vec<_> = menu["items"].as_array().unwrap().iter().map(|i| i["label"].as_str().unwrap().to_owned()).collect();
        assert_eq!(labels, ["Orbit", "Pan", "Zoom"]);
        assert!(items(&h.app, &Target::ViewTool).iter().all(|i| i.shortcut != "current"));
        h.call("ui.menuPick", json!({"item":"Pan"}));
        assert_eq!(h.app.viewport.nav, Some(crate::viewport::NavMode::Pan));
        h.call("ui.start", json!({"command":"view.tool"}));
        assert_eq!(items(&h.app, &Target::ViewTool).iter().find(|i| i.label == "Pan").unwrap().shortcut, "current");
        h.call("ui.menuPick", json!({"item":"Pan"}));
        assert_eq!(h.app.viewport.nav, None, "picking the latched tool releases it, as its navigation-bar button does");
        assert!(!error(&h));
    }

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
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: output.clone(), sketch: None }]);
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

    /// #10417: a projected edge's context menu offers Break Link, and the item makes it normal
    /// sketch geometry that Trim then cuts. A line drawn in the sketch is offered no Break Link.
    #[test]
    fn projected_geometry_context_menu_breaks_its_link_and_allows_trim() {
        use solidcraft_engine::geom::Vec2;

        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length":20,"width":20,"height":10})).unwrap();
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.project", json!({"refs":[{"body":"Body1"}],"projection_link":true})).unwrap();
        let drawn = h.app.run("sketch.line", json!({"points":[[10,-5],[10,5]],"connect":false,"infer":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let sid = h.app.session.active_sketch.unwrap();
        let cut = Vec2::new(15.0, 0.0);
        let edge = {
            let state = h.app.session.world_state();
            let sk = &state.sketch(sid).unwrap().sketch;
            (0..sk.curves.len())
                .find(|i| sk.curves[*i].link.is_some() && sk.shape(*i).is_some_and(|shape| shape.dist(cut) < 1e-6))
                .map(|i| sk.curves[i].id.clone())
        }
        .expect("the box's edge along y = 0 is projected");
        assert!(h.app.run("sketch.trim", json!({"curve":edge,"at":[cut.x,cut.y]})).is_err(), "a linked edge is not trimmed");

        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.0;
        h.settle();
        h.step(&json!({"click":{"world":[10,3,0]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: drawn.clone(), sketch: None }]);
        assert!(!items(&h.app, &Target::Viewport).iter().any(|item| item.id == "sketch.break_link"), "a drawn line has no link to break");
        h.step(&json!({"key":"Escape"})).unwrap();
        h.step(&json!({"click":{"world":[cut.x,cut.y,0]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: edge.clone(), sketch: None }]);
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|item| item.id == "sketch.break_link").expect("a projected edge must offer Break Link");
        assert_eq!(item.label, "Break Link");
        assert!(item.enabled);
        assert_eq!(item.params, Some(json!({"entities":[edge]})));
        h.step(&json!({"click":{"handle":"menu:sketch.break_link"}})).unwrap();
        let unlinked = h.app.session.world_state().sketch(sid).unwrap().sketch.curves.iter().find(|c| c.id == edge).is_some_and(|c| c.link.is_none());
        assert!(unlinked, "Break Link makes the edge normal sketch geometry");
        h.app.run("sketch.trim", json!({"curve":edge,"at":[cut.x,cut.y]})).unwrap();
    }

    /// #6533: a right-click on a fit point spline offers Add Spline Point at the clicked spot, and
    /// the item puts a fit point there. A control point spline is not offered it. Every spline is
    /// offered Reverse Spline Direction (#8224), which runs it the other way.
    #[test]
    fn spline_context_menu_adds_a_fit_point_where_it_was_clicked() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        let fit = h.app.run("sketch.spline.fit_point", json!({"points":[[0,0],[10,8],[20,0],[30,6]],"connect":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let control = h.app.run("sketch.spline.control_point", json!({"points":[[0,-12],[10,-4],[20,-12]],"connect":false})).unwrap()["curves"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let sid = h.app.session.active_sketch.unwrap();
        let spot = |h: &crate::scenario::Harness, id: &str, piece: usize| {
            let state = h.app.session.world_state();
            let sk = &state.sketch(sid).unwrap().sketch;
            sk.exact_segs(sk.curve_index(id).unwrap()).unwrap()[piece].point_at(0.5)
        };
        let count = |h: &crate::scenario::Harness, id: &str| {
            let state = h.app.session.world_state();
            let sk = &state.sketch(sid).unwrap().sketch;
            sk.curves[sk.curve_index(id).unwrap()].kind.point_ids().len()
        };
        h.call("ui.view", json!({"view":"top"}));
        h.app.cam.distance = 80.0;
        h.settle();

        let c = spot(&h, &control, 0);
        h.step(&json!({"click":{"world":[c.x,c.y,0]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: control.clone(), sketch: None }]);
        assert!(items(&h.app, &Target::Viewport).iter().all(|item| item.id != "sketch.spline.insert_point"));
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|item| item.id == "sketch.spline.reverse").expect("a control point spline offers Reverse Spline Direction");
        assert_eq!((item.label.as_str(), item.params.clone()), ("Reverse Spline Direction", Some(json!({"curve": control}))));
        let points = |h: &crate::scenario::Harness, id: &str| {
            let state = h.app.session.world_state();
            let sk = &state.sketch(sid).unwrap().sketch;
            sk.curves[sk.curve_index(id).unwrap()].kind.point_ids()
        };
        let mut reversed = points(&h, &control);
        reversed.reverse();
        h.step(&json!({"click":{"handle":"menu:sketch.spline.reverse"}})).unwrap();
        assert_eq!(points(&h, &control), reversed);
        close(&mut h.app);

        let at = spot(&h, &fit, 1);
        h.step(&json!({"click":{"world":[at.x,at.y,0]},"button":"right"})).unwrap();
        assert_eq!(h.app.session.selection, vec![Sel::SketchCurve { id: fit.clone(), sketch: None }]);
        let menu = items(&h.app, &Target::Viewport);
        let item = menu.iter().find(|item| item.id == "sketch.spline.insert_point").expect("a fit spline offers Add Spline Point");
        assert_eq!(item.label, "Add Spline Point");
        assert!(item.enabled);
        let p = item.params.clone().unwrap();
        assert_eq!(p["curve"], json!(fit));
        let clicked = solidcraft_engine::geom::Vec2::new(p["at"][0].as_f64().unwrap(), p["at"][1].as_f64().unwrap());
        assert!(clicked.dist(at) < 0.5, "the item carries the clicked spot: {clicked:?} for {at:?}");
        h.step(&json!({"click":{"handle":"menu:sketch.spline.insert_point"}})).unwrap();
        assert_eq!(count(&h, &fit), 5);
        let state = h.app.session.world_state();
        let sk = &state.sketch(sid).unwrap().sketch;
        let added = sk.point(sk.curves[sk.curve_index(&fit).unwrap()].kind.point_ids()[2]).unwrap();
        assert!(added.dist(at) < 0.5, "the new fit point is where the menu was opened: {added:?}");
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
        assert!(!h.app.origin_shown() && h.app.viewport.context_menu.is_none(), "the ordinary menu must execute Hide Origin and close");
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
