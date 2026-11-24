//! Select Other: everything under the cursor, for picking what lies under something else (two
//! lines drawn on top of each other, coincident points, an edge in front of a face, overlapping
//! bodies).
//!
//! - Holding the left button still (or a finger, on touch screens) for [`HOLD_SECS`], or the
//!   viewport menu's "Select Other…", opens a list at the cursor of every pickable thing there,
//!   including hidden geometry in a six-point radius without changing its visibility by default.
//!   The `select_other_hidden` preference restores the visible-only list. It is
//!   in the selection filter's and the open dialog's terms, front to back like layers: sketch
//!   points and curves first (drawn over the model), then what belongs to a body, one body at a
//!   time and the nearest body first (its vertices, edges, face, then the body itself), then
//!   profiles, planes and axes. On a dense mesh (a modelled thread, an imported mesh) thousands
//!   of facets can share one cursor, so at most [`MAX_PER_KIND`] rows are kept per kind, nearest
//!   first, and the list reports how many things there were in all.
//! - Hovering a row lights its entity up in the viewport; clicking it selects it (Shift/Ctrl add,
//!   as clicks do, held then or at the click or press that opened the list) or, with a dialog
//!   open, gives it to the dialog's active selection input.
//!   Esc or a click elsewhere closes the list.
//! - Clicking again on the same spot with nothing changed selects the next thing in that list
//!   (click cycling). The second click of a double-click keeps its own actions.
//!
//! The control channel's `ui.selectOther` opens, describes and picks from the list.

use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{Container, Surface};
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::CurveKind;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use crate::SolveApp;
use crate::tool_modifiers::Held;
use crate::viewport::{Hit, Proj};
use solidcraft_theme::Tokens;

/// How long the left button stays down, without moving, before the list opens.
pub const HOLD_SECS: f64 = 0.5;
/// How far (screen points) a repeat click may be from the last one and still cycle.
const CYCLE_SLOP: f32 = 4.0;
const ROW_HEIGHT: f32 = 24.0;
/// Indentation per nesting level in the tree view.
const INDENT_STEP: f32 = 14.0;
/// Popup width when category filter chips or tree nesting are shown.
const WIDE_LIST_W: f32 = 280.0;
/// Maximum component parent-chain depth walked when resolving hierarchy (guards against cycles).
const MAX_COMPONENT_DEPTH: usize = 64;
/// The most rows kept per kind of thing (vertices, edges, faces, …). The list exists to tell a
/// handful of overlapping things apart; nearest first, farther ones of the same kind are dropped
/// so it stays readable and bounded.
pub(crate) const MAX_PER_KIND: usize = 20;

/// High-level filter category for Select Other rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Sketch,
    Faces,
    Edges,
    Vertices,
    Bodies,
    Construction,
}

impl Category {
    /// Display order of category filter chips.
    pub const ALL: [Category; 6] = [Category::Sketch, Category::Faces, Category::Edges, Category::Vertices, Category::Bodies, Category::Construction];

    pub fn of(sel: &Sel) -> Category {
        match sel {
            Sel::SketchPoint { .. }
            | Sel::SketchMidpoint { .. }
            | Sel::ExternalSketchPoint { .. }
            | Sel::SketchCurve { .. }
            | Sel::ExternalSketchCurve { .. }
            | Sel::SketchConstraint { .. }
            | Sel::Profile { .. } => Category::Sketch,
            Sel::Face { .. } | Sel::Feature { .. } => Category::Faces,
            Sel::Edge { .. } => Category::Edges,
            Sel::Vertex { .. } | Sel::PointCloudPoint { .. } => Category::Vertices,
            Sel::Body { .. } | Sel::Occurrence { .. } => Category::Bodies,
            Sel::Plane { .. } | Sel::Axis { .. } | Sel::ConstructionPoint { .. } | Sel::Drawing { .. } => Category::Construction,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Category::Sketch => "sketch",
            Category::Faces => "faces",
            Category::Edges => "edges",
            Category::Vertices => "vertices",
            Category::Bodies => "bodies",
            Category::Construction => "construction",
        }
    }

    pub fn from_key(s: &str) -> Option<Category> {
        match s.to_ascii_lowercase().as_str() {
            "sketch" => Some(Category::Sketch),
            "faces" | "face" => Some(Category::Faces),
            "edges" | "edge" => Some(Category::Edges),
            "vertices" | "vertex" => Some(Category::Vertices),
            "bodies" | "body" => Some(Category::Bodies),
            "construction" => Some(Category::Construction),
            _ => None,
        }
    }

    pub fn tr_key(self) -> &'static str {
        match self {
            Category::Sketch => "ui.select_other.cat_sketch",
            Category::Faces => "ui.select_other.cat_faces",
            Category::Edges => "ui.select_other.cat_edges",
            Category::Vertices => "ui.select_other.cat_vertices",
            Category::Bodies => "ui.select_other.cat_bodies",
            Category::Construction => "ui.select_other.cat_construction",
        }
    }
}

/// What kind of container a tree segment represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKind {
    Occurrence,
    Component,
    Body,
    Sketch,
}

/// One ancestor node in the Browser hierarchy above a Select Other row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupSeg {
    /// Stable key for collapse/expand state (e.g. `"occ:1/2"`, `"comp:3"`, `"body:Body1"`, `"sketch:4"`).
    pub key: String,
    /// Human-readable container name.
    pub name: String,
    /// Kit icon name (`"component"`, `"body"`, `"sketch"`).
    pub icon: &'static str,
    /// Container kind.
    pub kind: GroupKind,
}

/// One row of the list.
#[derive(Clone, Debug)]
pub struct Row {
    pub hit: Hit,
    pub sel: Sel,
    pub label: String,
    /// Concise leaf label used when nested under its parent body or sketch.
    pub short_label: String,
    pub icon: &'static str,
    pub category: Category,
    /// Ancestor chain from outermost (occurrence/component) to innermost (body/sketch).
    pub hierarchy: Vec<GroupSeg>,
}

/// The open list.
#[derive(Clone, Debug)]
pub struct Popup {
    pub at: Pos2,
    pub rows: Vec<Row>,
    /// How many things were under the cursor in all (the rows are capped per kind).
    pub total: usize,
    /// The row under the pointer (lit in the viewport).
    pub hovered: Option<usize>,
    /// Active category filter (`None` = All).
    pub filter: Option<Category>,
    /// Whether to nest rows by component/occurrence/body/sketch when multiple groups exist.
    pub grouped: bool,
    /// Collapsed group keys (`GroupSeg::key`).
    pub collapsed: HashSet<String>,
    /// The model and dialog it was made for: it closes when they change.
    revision: u64,
    dialog: bool,
    include_hidden: bool,
    /// The keys held at the press or click that opened it.
    held: Held,
}

/// The last plain click, for cycling.
#[derive(Clone, Debug)]
struct Cycle {
    at: Pos2,
    chosen: Sel,
    selection: Vec<Sel>,
    key: u64,
}

#[derive(Default)]
pub struct State {
    pub popup: Option<Popup>,
    /// A left press that may become a hold: where, when and with which keys.
    hold: Option<(Pos2, f64, Held)>,
    /// The current press belongs to Select Other (it opened or closed the list): the viewport
    /// doesn't also click, drag or box-select with it.
    held: bool,
    cycle: Option<Cycle>,
}

// ---------------------------------------------------------------------------------------------
// What is under the cursor
// ---------------------------------------------------------------------------------------------

/// Every pickable thing under `pos`, front to back (the sketch being edited first): see
/// [`order`].
pub fn rows(app: &SolveApp, proj: &Proj, pos: Pos2) -> Vec<Row> {
    rows_capped(app, proj, pos, app.preferences.select_other_hidden).0
}

/// [`rows`], with how many things were under the cursor in all.
fn rows_capped(app: &SolveApp, proj: &Proj, pos: Pos2, through: bool) -> (Vec<Row>, usize) {
    let hits = if through { crate::viewport::pick_through(app, proj, pos) } else { crate::viewport::pick_all(app, proj, pos) };
    let mut found = crate::viewport::candidates(app, &hits);
    let dialog = app.dialog.as_ref().is_some_and(|d| d.wants_picks());
    // A body behind each face (a whole-object selection mode already turns faces into objects).
    if !dialog && crate::selection_mode::picks(app) == solidcraft_engine::PickMode::Faces {
        for h in &hits {
            if let Hit::Face { body, .. } = h {
                found.push((h.clone(), Sel::Body { name: body.clone() }));
            }
        }
    }
    // A face place a sketch point snapped (a Single hole's) is its own row: beside the face as
    // picked, and beside other points on that face.
    let snapped = |h: &Hit| matches!(h, Hit::SketchPoint { .. });
    let mut out: Vec<(Hit, Sel)> = Vec::new();
    for (h, s) in found {
        if !out.iter().any(|(hx, x)| if snapped(hx) || snapped(&h) { *x == s } else { crate::selection::same_item(x, &s) }) {
            out.push((h, s));
        }
    }
    let sketching = app.session.active_sketch.is_some();
    let out = order(app, proj, pos, out, sketching);
    let total = out.len();
    // Nearest first within a kind, so capping drops the farther ones.
    let mut per_kind: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    let rows: Vec<Row> = out
        .into_iter()
        .filter(|(h, s)| {
            let n = per_kind.entry(rank(&shown_as(h, s), sketching).1).or_default();
            if *n >= MAX_PER_KIND {
                return false;
            }
            *n += 1;
            true
        })
        .map(|(hit, sel)| {
            let shown = shown_as(&hit, &sel);
            let label = label(app, &shown);
            let short_label = short_label(app, &shown);
            let category = Category::of(&shown);
            let hierarchy = hierarchy_for(app, &shown);
            Row { icon: icon(app, &shown), hit, sel, label, short_label, category, hierarchy }
        })
        .collect();
    (rows, total)
}

/// The list's order, front to back like layers (#5345). Sketch points and curves come first
/// (drawn over the model, the sketch being edited before the rest), then each body's things
/// together, bodies by their nearest hit along the pick ray; within a body by kind (vertices,
/// edges, its face, the body), each kind keeping the pick's nearest-first order. Profiles,
/// planes and axes follow. The sort is stable.
fn order(app: &SolveApp, proj: &Proj, pos: Pos2, found: Vec<(Hit, Sel)>, sketching: bool) -> Vec<(Hit, Sel)> {
    let (o, d) = proj.ray(pos);
    let st = crate::open_meshing::shown_world(app);
    let index: std::collections::HashMap<&str, usize> = st.bodies.iter().enumerate().map(|(i, b)| (b.name.as_str(), i)).collect();
    let mut nearest: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for (h, _) in &found {
        if let (Some(body), Some(t)) = (body_of(h), hit_depth(&st, &index, o, d, h)) {
            let e = nearest.entry(body.to_string()).or_insert(t);
            *e = e.min(t);
        }
    }
    let mut keyed: Vec<((u8, u8, f64, u8), (Hit, Sel))> = found
        .into_iter()
        .map(|(h, s)| {
            let (own, kind) = rank(&shown_as(&h, &s), sketching);
            let key = match body_of(&h) {
                Some(body) => (own, 1, nearest.get(body).copied().unwrap_or(f64::INFINITY), kind),
                None => (own, if kind <= 3 { 0 } else { 2 }, 0.0, kind),
            };
            (key, (h, s))
        })
        .collect();
    keyed.sort_by(|(a, _), (b, _)| (a.0, a.1).cmp(&(b.0, b.1)).then(a.2.total_cmp(&b.2)).then(a.3.cmp(&b.3)));
    keyed.into_iter().map(|(_, x)| x).collect()
}

/// The body a hit lies on (the origin point, a vertex of no body, has none).
fn body_of(h: &Hit) -> Option<&str> {
    match h {
        Hit::Vertex { body, .. } | Hit::Edge { body, .. } | Hit::Face { body, .. } if !body.is_empty() => Some(body),
        _ => None,
    }
}

/// How far along the pick ray (`o`, `d`) a body's hit lies: a face's or vertex's point, or an
/// edge's point nearest the ray (its middle can lie much deeper or nearer).
fn hit_depth(
    st: &solidcraft_engine::doc::ModelState,
    index: &std::collections::HashMap<&str, usize>,
    o: solidcore_record::Vec3,
    d: solidcore_record::Vec3,
    h: &Hit,
) -> Option<f64> {
    let along = |p: solidcore_record::Vec3| Some((p - o).dot(d)).filter(|t| t.is_finite());
    match h {
        Hit::Vertex { point, .. } | Hit::Face { point, .. } => along(*point),
        Hit::Edge { body, index: ei, mid, .. } => {
            let under = index.get(body.as_str()).and_then(|i| st.bodies.get(*i)).and_then(|b| {
                let (m, place) = b.mesh_in_place();
                let at = |p: solidcore_record::Vec3| solidcraft_engine::doc::apply_point(&place, p);
                let dd = d.dot(d);
                m.edges
                    .get(*ei)?
                    .windows(2)
                    .map(|w| crate::viewport::ray_segment_closest(o, d, at(w[0]), at(w[1])))
                    .map(|q| {
                        let t = (q - o).dot(d);
                        // Squared distance from the ray, to take the segment under the cursor.
                        ((q - o).dot(q - o) - t * t / dd, t)
                    })
                    .filter(|(off, t)| off.is_finite() && t.is_finite())
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, t)| t)
            });
            under.or_else(|| along(*mid))
        }
        _ => None,
    }
}

/// What a row shows itself as: a face place a sketch point snapped is that point (ranked, capped
/// and named with the points).
fn shown_as(hit: &Hit, sel: &Sel) -> Sel {
    match (hit, sel) {
        (Hit::SketchPoint { sketch, id, .. }, Sel::Face { .. }) => Sel::ExternalSketchPoint { sketch: *sketch, id: id.clone() },
        _ => sel.clone(),
    }
}

/// Sort key: in a sketch its own points and curves come first; then by kind.
fn rank(s: &Sel, sketching: bool) -> (u8, u8) {
    let kind = match s {
        Sel::SketchPoint { .. } | Sel::SketchMidpoint { .. } | Sel::ExternalSketchPoint { .. } | Sel::PlacedSketchPoint { .. } => 0,
        Sel::SketchCurve { .. } | Sel::ExternalSketchCurve { .. } => 1,
        Sel::SketchConstraint { .. } => 2,
        Sel::Vertex { .. } | Sel::ConstructionPoint { .. } | Sel::PointCloudPoint { .. } => 3,
        Sel::Edge { .. } => 4,
        Sel::Face { .. } | Sel::Feature { .. } => 5,
        Sel::Body { .. } | Sel::Occurrence { .. } => 6,
        Sel::Profile { .. } => 7,
        Sel::Plane { .. } => 8,
        Sel::Axis { .. } => 9,
        Sel::Drawing { .. } => 10,
    };
    let own = matches!(s, Sel::SketchPoint { .. } | Sel::SketchMidpoint { .. } | Sel::SketchCurve { .. });
    (if sketching && !own { 1 } else { 0 }, kind)
}

/// The number at the end of an id (`l3` → 3).
fn id_number(id: &str) -> Option<u64> {
    let digits: String = id.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse().ok()
}

fn curve_kind_name(k: &CurveKind) -> &'static str {
    match k {
        CurveKind::Line { .. } => "Line",
        CurveKind::Circle { .. } => "Circle",
        CurveKind::Arc { .. } => "Arc",
        CurveKind::Ellipse { .. } => "Ellipse",
        CurveKind::Spline { .. } => "Spline",
        CurveKind::Conic { .. } => "Conic",
    }
}

fn sketch_name(app: &SolveApp, sketch: u64) -> String {
    app.session.doc.feature(sketch).map(|f| f.name.clone()).unwrap_or_else(|| format!("Sketch {sketch}"))
}

/// "Line 3" (its kind and number), with what kind of geometry it is when that matters.
pub(crate) fn curve_label(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> String {
    if let Some((i, c)) = sk.curves.iter().enumerate().find(|(_, c)| c.id == id) {
        let kind = curve_kind_name(&c.kind);
        let n = id_number(id).unwrap_or_else(|| sk.curves.iter().take(i + 1).filter(|x| curve_kind_name(&x.kind) == kind).count() as u64);
        let mut s = format!("{kind} {n}");
        if let Some(link) = &c.link {
            let kind = sk.links.iter().find(|l| &l.id == link).map(|l| l.kind);
            s.push_str(if kind == Some(solidcraft_engine::sketch::LinkKind::Intersect) { " (intersected)" } else { " (projected)" });
        } else if c.construction {
            s.push_str(" (construction)");
        }
        return s;
    }
    if sk.wires.iter().any(|w| w.id == id) {
        return format!("3D Curve {}", id_number(id).unwrap_or(0));
    }
    id.to_string()
}

/// "Point (arc 2 centre)": the role it plays in the first curve that uses it.
pub(crate) fn point_label(sk: &solidcraft_engine::sketch::Sketch, id: &str) -> String {
    let Some(pi) = sk.points.iter().position(|p| p.id == id) else { return id.to_string() };
    let role = sk.curves.iter().find_map(|c| {
        let name = format!("{} {}", curve_kind_name(&c.kind).to_lowercase(), id_number(&c.id).unwrap_or(0));
        let (start, end) = if c.reversed { ("end", "start") } else { ("start", "end") };
        let role = match &c.kind {
            CurveKind::Line { a, b } | CurveKind::Arc { a, b, .. } | CurveKind::Conic { a, b, .. } if *a == pi => Some(start.to_string()),
            CurveKind::Line { b, .. } | CurveKind::Arc { b, .. } | CurveKind::Conic { b, .. } if *b == pi => Some(end.to_string()),
            CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } | CurveKind::Ellipse { c, .. } if *c == pi => Some("centre".to_string()),
            CurveKind::Ellipse { m, .. } if *m == pi => Some("major axis end".to_string()),
            CurveKind::Conic { apex, .. } if *apex == pi => Some("apex".to_string()),
            CurveKind::Spline { pts, .. } => pts.iter().position(|p| *p == pi).map(|k| format!("point {}", k + 1)),
            _ => None,
        }?;
        Some(format!("{name} {role}"))
    });
    let fixed = if sk.point_locked(pi) { ", fixed" } else { "" };
    match role {
        Some(r) => format!("Point ({r}{fixed})"),
        None if fixed.is_empty() => format!("Point {}", id_number(id).unwrap_or(pi as u64 + 1)),
        None => format!("Point {} (fixed)", id_number(id).unwrap_or(pi as u64 + 1)),
    }
}

/// A row's name: "Line 3", "Point (arc 2 centre)", "Edge 4 · Body1", "XY Plane" (an origin plane
/// by the name the preferences give it)…
pub fn label(app: &SolveApp, sel: &Sel) -> String {
    let st = crate::open_meshing::session_world(&app.session);
    let in_sketch =
        |sketch: u64, f: &dyn Fn(&solidcraft_engine::sketch::Sketch) -> String| st.sketch(sketch).map(|ss| f(&ss.sketch)).unwrap_or_default();
    match sel {
        Sel::SketchMidpoint { id } => {
            format!("MID · {}", app.session.active_sketch.map(|s| in_sketch(s, &|sk| curve_label(sk, id))).unwrap_or_else(|| id.clone()))
        }
        Sel::SketchPoint { id } => app.session.active_sketch.map(|s| in_sketch(s, &|sk| point_label(sk, id))).unwrap_or_else(|| id.clone()),
        Sel::SketchCurve { id, .. } => app.session.active_sketch.map(|s| in_sketch(s, &|sk| curve_label(sk, id))).unwrap_or_else(|| id.clone()),
        Sel::ExternalSketchPoint { sketch, id } => format!("{} · {}", in_sketch(*sketch, &|sk| point_label(sk, id)), sketch_name(app, *sketch)),
        // On another copy of its part: that copy's name too.
        Sel::PlacedSketchPoint { sketch, id, path } => {
            let copy = path.last().and_then(|o| app.session.doc.occurrences.iter().find(|x| x.id == *o)).map(|o| o.name.clone()).unwrap_or_default();
            format!("{} · {} · {copy}", in_sketch(*sketch, &|sk| point_label(sk, id)), sketch_name(app, *sketch))
        }
        Sel::ExternalSketchCurve { sketch, id } => format!("{} · {}", in_sketch(*sketch, &|sk| curve_label(sk, id)), sketch_name(app, *sketch)),
        Sel::SketchConstraint { id } => format!("Constraint {id}"),
        Sel::Vertex { body, .. } => format!("Vertex · {body}"),
        Sel::PointCloudPoint { cloud, index, .. } => {
            let name = app.session.doc.point_clouds.iter().find(|c| c.id == *cloud).map(|c| c.name.as_str()).unwrap_or("Point Cloud");
            format!("Point {index} · {name}")
        }
        Sel::Edge { body, index, .. } => format!("Edge {} · {body}", index + 1),
        Sel::Face { body, index, .. } => format!("Face {} · {body}", index + 1),
        Sel::Body { name } => format!("Body · {name}"),
        Sel::Occurrence { path } => format!("Occurrence {path:?}"),
        Sel::Drawing { object, .. } => format!("Drawing object {object}"),
        Sel::Feature { id } => app.session.doc.feature(*id).map(|f| f.name.clone()).unwrap_or_else(|| format!("Feature {id}")),
        Sel::Profile { sketch, index } => format!("Profile {} · {}", index + 1, sketch_name(app, *sketch)),
        Sel::Plane { name } if matches!(name.as_str(), "XY" | "XZ" | "YZ") => format!("{} Plane", app.preferences.origin_plane_names.label(name)),
        Sel::Axis { name } if matches!(name.as_str(), "X" | "Y" | "Z") => format!("{name} Axis"),
        Sel::Plane { name } | Sel::Axis { name } | Sel::ConstructionPoint { name } => {
            crate::dialogs::component_origin_label(&app.session, app.preferences.origin_plane_names, name).unwrap_or_else(|| name.clone())
        }
    }
}

fn icon(app: &SolveApp, sel: &Sel) -> &'static str {
    let st = crate::open_meshing::session_world(&app.session);
    let curve = |sketch: Option<u64>, id: &str| {
        let kind = sketch.and_then(|s| st.sketch(s)).and_then(|ss| ss.sketch.curves.iter().find(|c| c.id == id).map(|c| c.kind.clone()));
        match kind {
            Some(CurveKind::Circle { .. }) => "circle",
            Some(CurveKind::Arc { .. }) => "arc_tangent",
            Some(CurveKind::Ellipse { .. }) => "ellipse",
            Some(CurveKind::Spline { .. }) => "spline",
            Some(CurveKind::Conic { .. }) => "conic",
            _ => "line",
        }
    };
    match sel {
        Sel::SketchPoint { .. }
        | Sel::SketchMidpoint { .. }
        | Sel::ExternalSketchPoint { .. }
        | Sel::PlacedSketchPoint { .. }
        | Sel::Vertex { .. }
        | Sel::ConstructionPoint { .. }
        | Sel::PointCloudPoint { .. } => "point",
        Sel::SketchCurve { id, .. } => curve(app.session.active_sketch, id),
        Sel::ExternalSketchCurve { sketch, id } => curve(Some(*sketch), id),
        Sel::SketchConstraint { .. } => "dimension",
        Sel::Edge { .. } => "fillet",
        Sel::Face { .. } | Sel::Feature { .. } => "presspull",
        Sel::Body { .. } => "body",
        Sel::Occurrence { .. } | Sel::Drawing { .. } => "component",
        Sel::Profile { .. } => "sketch",
        Sel::Plane { .. } => "plane",
        Sel::Axis { .. } => "axis",
    }
}

/// Concise leaf label used when the row is nested inside its parent body or sketch group.
fn short_label(app: &SolveApp, sel: &Sel) -> String {
    let st = crate::open_meshing::session_world(&app.session);
    let in_sketch =
        |sketch: u64, f: &dyn Fn(&solidcraft_engine::sketch::Sketch) -> String| st.sketch(sketch).map(|ss| f(&ss.sketch)).unwrap_or_default();
    match sel {
        Sel::Vertex { .. } => "Vertex".to_string(),
        Sel::Edge { index, .. } => format!("Edge {}", index + 1),
        Sel::Face { index, .. } => format!("Face {}", index + 1),
        Sel::Body { name } => body_display_name(app, name),
        Sel::ExternalSketchPoint { sketch, id } => in_sketch(*sketch, &|sk| point_label(sk, id)),
        Sel::ExternalSketchCurve { sketch, id } => in_sketch(*sketch, &|sk| curve_label(sk, id)),
        Sel::Profile { index, .. } => format!("Profile {}", index + 1),
        Sel::Occurrence { path } => path
            .last()
            .and_then(|id| app.session.doc.occurrences.iter().find(|o| o.id == *id))
            .map(|o| o.name.clone())
            .unwrap_or_else(|| format!("Occurrence {path:?}")),
        _ => label(app, sel),
    }
}

/// Display name for a body inside its component/occurrence: uses `source_name` when placed so it
/// does not repeat the occurrence prefix.
fn body_display_name(app: &SolveApp, body_name: &str) -> String {
    let st = crate::open_meshing::session_world(&app.session);
    st.body(body_name).and_then(|b| b.source_name.as_deref()).filter(|s| !s.is_empty()).unwrap_or(body_name).to_string()
}

fn join_path(path: &[u64]) -> String {
    let mut out = String::new();
    for (i, id) in path.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(&id.to_string());
    }
    out
}

/// Ancestor segments for an occurrence path (outermost to innermost).
fn occurrence_chain(app: &SolveApp, path: &[u64]) -> Vec<GroupSeg> {
    let mut segs = Vec::with_capacity(path.len());
    for end in 1..=path.len() {
        let prefix = &path[..end];
        let Some(&occ_id) = prefix.last() else { continue };
        let name =
            app.session.doc.occurrences.iter().find(|o| o.id == occ_id).map(|o| o.name.clone()).unwrap_or_else(|| format!("Component {occ_id}"));
        segs.push(GroupSeg { key: format!("occ:{}", join_path(prefix)), name, icon: "component", kind: GroupKind::Occurrence });
    }
    segs
}

/// Ancestor segments for a non-root component definition (`comp_id != 0`), outermost to innermost.
fn component_chain(app: &SolveApp, comp_id: u64) -> Vec<GroupSeg> {
    if comp_id == 0 {
        return Vec::new();
    }
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = comp_id;
    while cur != 0 && chain.len() < MAX_COMPONENT_DEPTH && seen.insert(cur) {
        let Some(comp) = app.session.doc.components.iter().find(|c| c.id == cur) else {
            break;
        };
        chain.push(GroupSeg { key: format!("comp:{}", comp.id), name: comp.name.clone(), icon: "component", kind: GroupKind::Component });
        cur = comp.parent;
    }
    chain.reverse();
    chain
}

/// Ancestor segments for a body by its world name, ending with the body itself.
fn body_hierarchy(app: &SolveApp, body_name: &str) -> Vec<GroupSeg> {
    if body_name.is_empty() {
        return Vec::new();
    }
    let mut segs = if let Some(path) = crate::browser::placed_body_path(app, body_name)
        && !path.is_empty()
    {
        occurrence_chain(app, &path)
    } else {
        let st = crate::open_meshing::session_world(&app.session);
        let comp_id = st.body(body_name).map(|b| app.session.doc.model_body_component(b)).unwrap_or(0);
        component_chain(app, comp_id)
    };
    segs.push(GroupSeg { key: format!("body:{body_name}"), name: body_display_name(app, body_name), icon: "body", kind: GroupKind::Body });
    segs
}

/// Browser-tree ancestor chain for `sel`, from outermost component/occurrence to innermost
/// body/sketch.
fn hierarchy_for(app: &SolveApp, sel: &Sel) -> Vec<GroupSeg> {
    match sel {
        Sel::Vertex { body, .. } | Sel::Edge { body, .. } | Sel::Face { body, .. } | Sel::Body { name: body } => body_hierarchy(app, body),
        Sel::Occurrence { path } => occurrence_chain(app, path),
        Sel::SketchPoint { .. } | Sel::SketchMidpoint { .. } | Sel::SketchCurve { .. } | Sel::SketchConstraint { .. } => {
            let Some(sid) = app.session.active_sketch else { return Vec::new() };
            let comp = app.session.doc.feature(sid).map(|f| f.component).unwrap_or(0);
            let mut segs = component_chain(app, comp);
            segs.push(GroupSeg { key: format!("sketch:{sid}"), name: sketch_name(app, sid), icon: "sketch", kind: GroupKind::Sketch });
            segs
        }
        Sel::ExternalSketchPoint { sketch, .. } | Sel::ExternalSketchCurve { sketch, .. } | Sel::Profile { sketch, .. } => {
            let comp = app.session.doc.feature(*sketch).map(|f| f.component).unwrap_or(0);
            let mut segs = component_chain(app, comp);
            segs.push(GroupSeg { key: format!("sketch:{sketch}"), name: sketch_name(app, *sketch), icon: "sketch", kind: GroupKind::Sketch });
            segs
        }
        Sel::Feature { id } => {
            let comp = app.session.doc.feature(*id).map(|f| f.component).unwrap_or(0);
            component_chain(app, comp)
        }
        Sel::Plane { .. } | Sel::Axis { .. } | Sel::ConstructionPoint { .. } | Sel::PointCloudPoint { .. } | Sel::Drawing { .. } => Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Opening, picking, closing
// ---------------------------------------------------------------------------------------------

fn projection_now(app: &SolveApp) -> Option<Proj> {
    app.viewport.rect.map(|r| crate::viewport::projection(app, r))
}

/// Open the list at a screen point; false (with a status message) when nothing is there. `held`:
/// the keys held at the click that opened it.
pub fn open(app: &mut SolveApp, at: Pos2, held: Held) -> bool {
    let Some(proj) = projection_now(app) else { return false };
    open_with(app, &proj, at, held)
}

fn open_with(app: &mut SolveApp, proj: &Proj, at: Pos2, held: Held) -> bool {
    // Ensure placed-body occurrence paths are cached for small/medium assemblies without blocking
    // large ones.
    crate::browser::prepare_body_paths(app);
    let (rows, total) = rows_capped(app, proj, at, app.preferences.select_other_hidden);
    close(app);
    if rows.is_empty() {
        app.set_status("Nothing to select here", false);
        return false;
    }
    app.viewport.select_other.popup = Some(Popup {
        at,
        rows,
        total,
        hovered: None,
        filter: None,
        grouped: true,
        collapsed: HashSet::new(),
        revision: app.session.revision,
        dialog: app.dialog.is_some(),
        include_hidden: app.preferences.select_other_hidden,
        held,
    });
    true
}

pub fn close(app: &mut SolveApp) {
    app.viewport.select_other.popup = None;
}

pub fn is_open(app: &SolveApp) -> bool {
    app.viewport.select_other.popup.is_some()
}

/// Pick row `i` of the open list and close it.
///
/// `add` / `toggle` match viewport selection (Shift pure-add, Primary toggle when
/// `selection_shift_add_only` is on); the keys held when the list opened add too.
pub fn choose(app: &mut SolveApp, i: usize, add: bool, toggle: bool) -> Result<Row, String> {
    let Some(p) = app.viewport.select_other.popup.as_ref() else { return Err("Select Other is not open".into()) };
    let at = p.at;
    let (opened_add, opened_toggle) = crate::tool_modifiers::selection_click(p.held, app.preferences.selection_shift_add_only);
    let (add, toggle) = (add || opened_add, toggle || opened_toggle);
    let row = p.rows.get(i).cloned().ok_or_else(|| format!("no row {i} (the list has {})", p.rows.len()))?;
    close(app);
    apply(app, &row, add, toggle);
    remember(app, at, if add { None } else { Some(row.sel.clone()) });
    Ok(row)
}

/// Select a row's entity: into the dialog's active input, or as a click does.
fn apply(app: &mut SolveApp, row: &Row, add: bool, toggle: bool) {
    if app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        crate::viewport::dialog_pick(app, &row.hit, row.sel.clone());
    } else {
        crate::viewport::select(app, Some(row.sel.clone()), add, toggle);
    }
}

/// The open list as JSON (for `ui.selectOther`).
pub fn describe(app: &SolveApp) -> Value {
    match &app.viewport.select_other.popup {
        None => json!({ "open": false }),
        Some(p) => {
            let counts = category_counts(&p.rows);
            let (display, can_group) = build_display_rows(&p.rows, p.filter, p.grouped, &p.collapsed);
            let visible_rows: Vec<usize> = display
                .iter()
                .filter_map(|d| match d {
                    DisplayRow::Item { row_idx, .. } => Some(*row_idx),
                    DisplayRow::Group { .. } => None,
                })
                .collect();
            let groups: Vec<Value> = display
                .iter()
                .filter_map(|d| match d {
                    DisplayRow::Group { key, name, depth, count, collapsed, .. } => {
                        Some(json!({ "key": key, "name": name, "depth": depth, "count": count, "collapsed": collapsed }))
                    }
                    DisplayRow::Item { label, depth, toggle: Some((key, collapsed, count)), .. } => {
                        Some(json!({ "key": key, "name": label, "depth": depth, "count": count, "collapsed": collapsed, "selectable": true }))
                    }
                    DisplayRow::Item { .. } => None,
                })
                .collect();
            json!({
                "open": true,
                "at": [p.at.x, p.at.y],
                "hovered": p.hovered,
                "total": p.total,
                "filter": p.filter.map(Category::key),
                "grouped": p.grouped && can_group,
                "can_group": can_group,
                "categories": counts.iter().map(|(c, n)| json!({ "key": c.key(), "count": n })).collect::<Vec<_>>(),
                "visible_rows": visible_rows,
                "groups": groups,
                "rows": p.rows.iter().map(|r| json!({
                    "label": r.label,
                    "short_label": r.short_label,
                    "icon": r.icon,
                    "category": r.category.key(),
                    "sel": r.sel,
                })).collect::<Vec<_>>(),
            })
        }
    }
}

/// The control channel's `ui.selectOther`: `{x, y}` opens the list there, `{pick: i, add?}`
/// picks a row, `{hover: i}` lights one up, `{filter: "edges" | "all"}` filters by category,
/// `{grouped: true | false}` switches tree/flat view, `{toggle_group: key}` collapses/expands a
/// group, `{close: true}` closes it; it answers with the list.
pub fn control(app: &mut SolveApp, p: &Value) -> Result<Value, String> {
    if p.get("close").and_then(Value::as_bool) == Some(true) {
        close(app);
        return Ok(describe(app));
    }
    if let (Some(x), Some(y)) = (p.get("x").and_then(Value::as_f64), p.get("y").and_then(Value::as_f64)) {
        if !(x.is_finite() && y.is_finite()) || x.abs() > 1e6 || y.abs() > 1e6 {
            return Err("x and y must be finite screen points within ±1000000".into());
        }
        if app.viewport.rect.is_none() {
            return Err("no viewport".into());
        }
        open(app, pos2(x as f32, y as f32), Held::default());
        return Ok(describe(app));
    }
    if let Some(f) = p.get("filter") {
        let Some(pop) = app.viewport.select_other.popup.as_mut() else { return Err("Select Other is not open".into()) };
        if f.is_null() {
            pop.filter = None;
        } else if let Some(s) = f.as_str() {
            if s.eq_ignore_ascii_case("all") || s.is_empty() {
                pop.filter = None;
            } else if let Some(cat) = Category::from_key(s) {
                pop.filter = Some(cat);
            } else {
                return Err(format!("unknown Select Other filter: {s}"));
            }
        } else {
            return Err("filter must be a category string or null".into());
        }
        pop.hovered = None;
        hover_override(app);
        return Ok(describe(app));
    }
    if let Some(grouped) = p.get("grouped").and_then(Value::as_bool) {
        let Some(pop) = app.viewport.select_other.popup.as_mut() else { return Err("Select Other is not open".into()) };
        pop.grouped = grouped;
        return Ok(describe(app));
    }
    if let Some(key) = p.get("toggle_group").and_then(Value::as_str) {
        let Some(pop) = app.viewport.select_other.popup.as_mut() else { return Err("Select Other is not open".into()) };
        if !pop.collapsed.remove(key) {
            pop.collapsed.insert(key.to_string());
        }
        return Ok(describe(app));
    }
    if let Some(i) = p.get("hover").and_then(Value::as_u64) {
        let Some(pop) = app.viewport.select_other.popup.as_mut() else { return Err("Select Other is not open".into()) };
        pop.hovered = Some(i as usize).filter(|i| *i < pop.rows.len());
        hover_override(app);
        return Ok(describe(app));
    }
    if let Some(i) = p.get("pick").and_then(Value::as_u64) {
        // Control channel: `add` enables both add and toggle (legacy API).
        let add = p.get("add").and_then(Value::as_bool).unwrap_or(false);
        let row = choose(app, i as usize, add, add)?;
        return Ok(json!({ "picked": row.label, "sel": row.sel, "selection": app.session.selection }));
    }
    Ok(describe(app))
}

// ---------------------------------------------------------------------------------------------
// Press and hold, and click cycling (called by the viewport)
// ---------------------------------------------------------------------------------------------

/// Follow a left press held still in the viewport; open the list when it has been held long
/// enough. True while the press belongs to Select Other (the viewport must not also click,
/// drag or box-select with it).
#[allow(clippy::too_many_arguments)]
pub fn track_hold(
    app: &mut SolveApp,
    ctx: &egui::Context,
    proj: &Proj,
    inside: bool,
    pressed: bool,
    down: bool,
    hover: Option<Pos2>,
    navigating: bool,
) -> bool {
    let consumed = app.viewport.select_other.held;
    let now = ctx.input(|i| i.time);
    let free = app.tool.is_none()
        && app.viewport.context_menu.is_none()
        && !is_open(app)
        && app.dialog.as_ref().is_none_or(|d| d.wants_picks())
        && app.viewport.point_drag.is_none();
    // A press on a canvas handle (a manipulator) drags it, and a still hold there is someone
    // thinking about where to drag, not asking for the overlapping-pick list.
    if pressed && inside && free && !navigating && !hover.is_some_and(|p| crate::canvas::owns_pointer(app, proj, p)) {
        let held = Held::from_modifiers(ctx.input(|i| i.modifiers));
        app.viewport.select_other.hold = hover.map(|p| (p, now, held));
    }
    let mut opened = false;
    if let Some((p0, t0, held)) = app.viewport.select_other.hold {
        let slop = ctx.options(|o| o.input_options.max_click_dist);
        if !down || navigating || !free || hover.is_some_and(|p| p.distance(p0) > slop) {
            app.viewport.select_other.hold = None;
        } else if now - t0 >= HOLD_SECS {
            app.viewport.select_other.hold = None;
            opened = open_with(app, proj, p0, held);
            app.viewport.select_other.held = true;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    if !down {
        app.viewport.select_other.held = false;
    }
    consumed || opened
}

/// The popup belongs to the focused modeling view. Other views have their own canvas feedback.
fn active_popup(app: &SolveApp) -> Option<&Popup> {
    if app.viewport.card || crate::render_workspace::active(app) {
        return None;
    }
    app.viewport.select_other.popup.as_ref().filter(|p| {
        p.revision == app.session.revision
            && p.dialog == app.dialog.is_some()
            && p.include_hidden == app.preferences.select_other_hidden
            && app.tool.is_none()
    })
}

pub(crate) fn hovered_body(app: &SolveApp) -> Option<&str> {
    let popup = active_popup(app)?;
    match &popup.rows.get(popup.hovered?)?.sel {
        Sel::Body { name } => Some(name),
        _ => None,
    }
}

/// While the list is open, the hovered row is what the viewport lights up.
pub fn hover_override(app: &mut SolveApp) {
    let Some(popup) = active_popup(app) else { return };
    app.viewport.hover = popup.hovered.and_then(|i| popup.rows.get(i)).filter(|row| !matches!(row.sel, Sel::Body { .. })).map(|row| row.hit.clone());
}

/// What a repeat click must find unchanged: the model, the view, the sketch and the filter.
fn cycle_key(app: &SolveApp) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.session.revision.hash(&mut h);
    for v in [app.cam.yaw, app.cam.pitch, app.cam.roll, app.cam.distance, app.cam.target.x, app.cam.target.y, app.cam.target.z] {
        v.to_bits().hash(&mut h);
    }
    app.session.active_sketch.hash(&mut h);
    app.ui.pick_off.hash(&mut h);
    crate::selection_mode::picks(app).hash(&mut h);
    app.dialog.is_some().hash(&mut h);
    h.finish()
}

fn remember(app: &mut SolveApp, at: Pos2, chosen: Option<Sel>) {
    app.viewport.select_other.cycle = chosen.map(|chosen| Cycle { at, chosen, selection: app.session.selection.clone(), key: cycle_key(app) });
}

/// A plain click selected `sel` at `at`: a repeat click there may cycle from it.
pub fn note_click(app: &mut SolveApp, at: Pos2, sel: Option<Sel>) {
    remember(app, at, sel);
}

/// A plain click at `at`: when it repeats the last one (same spot, nothing changed since), select
/// the next thing under the cursor and return true.
pub fn cycle_click(app: &mut SolveApp, proj: &Proj, at: Pos2) -> bool {
    let Some(c) = app.viewport.select_other.cycle.clone() else { return false };
    if c.at.distance(at) > CYCLE_SLOP || c.key != cycle_key(app) || c.selection != app.session.selection {
        return false;
    }
    let rows = rows_capped(app, proj, at, false).0;
    if rows.len() < 2 {
        return false;
    }
    let next = rows.iter().position(|r| crate::selection::same_item(&r.sel, &c.chosen)).map_or(0, |k| (k + 1) % rows.len());
    let Some(row) = rows.get(next).cloned() else { return false };
    apply(app, &row, false, false);
    app.set_status(format!("{} ({} of {})", row.label, next + 1, rows.len()), false);
    remember(app, c.at, Some(row.sel));
    true
}

// ---------------------------------------------------------------------------------------------
// Filtering, tree grouping, and drawing
// ---------------------------------------------------------------------------------------------

/// Non-empty categories in `rows`, in [`Category::ALL`] order, with counts.
fn category_counts(rows: &[Row]) -> Vec<(Category, usize)> {
    let mut counts = [0usize; 6];
    for r in rows {
        let idx = match r.category {
            Category::Sketch => 0,
            Category::Faces => 1,
            Category::Edges => 2,
            Category::Vertices => 3,
            Category::Bodies => 4,
            Category::Construction => 5,
        };
        counts[idx] += 1;
    }
    Category::ALL.into_iter().enumerate().filter_map(|(i, cat)| (counts[i] > 0).then_some((cat, counts[i]))).collect()
}

/// One visual row in the Select Other popup (either a collapsible group header or a selectable
/// entity row, which may itself have child rows collapsed/expanded beneath it).
enum DisplayRow {
    Group {
        key: String,
        name: String,
        icon: &'static str,
        depth: usize,
        count: usize,
        collapsed: bool,
    },
    Item {
        row_idx: usize,
        label: String,
        tooltip: String,
        icon: &'static str,
        depth: usize,
        /// `(group_key, is_collapsed, child_count)` when this selectable row (e.g. a Body or
        /// Occurrence) is also the parent of nested sub-entities.
        toggle: Option<(String, bool, usize)>,
    },
}

struct TreeNode {
    seg: GroupSeg,
    selectable_row: Option<usize>,
    leaves: Vec<usize>,
    children: Vec<TreeNode>,
}

impl TreeNode {
    fn new(seg: GroupSeg) -> Self {
        Self { seg, selectable_row: None, leaves: Vec::new(), children: Vec::new() }
    }

    fn total_items(&self) -> usize {
        usize::from(self.selectable_row.is_some()) + self.leaves.len() + self.children.iter().map(TreeNode::total_items).sum::<usize>()
    }
}

enum TopEntry {
    Node(TreeNode),
    Leaf(usize),
}

fn insert_into_nodes(nodes: &mut Vec<TreeNode>, path: &[GroupSeg], row_idx: usize, is_container_self: bool) {
    let Some((first, rest)) = path.split_first() else { return };
    let pos = if let Some(p) = nodes.iter().position(|n| n.seg.key == first.key) {
        p
    } else {
        nodes.push(TreeNode::new(first.clone()));
        nodes.len() - 1
    };
    let Some(node) = nodes.get_mut(pos) else { return };
    if rest.is_empty() {
        if is_container_self && node.selectable_row.is_none() {
            node.selectable_row = Some(row_idx);
        } else {
            node.leaves.push(row_idx);
        }
    } else {
        insert_into_nodes(&mut node.children, rest, row_idx, is_container_self);
    }
}

fn insert_top(entries: &mut Vec<TopEntry>, path: &[GroupSeg], row_idx: usize, is_container_self: bool) {
    let Some((first, rest)) = path.split_first() else {
        entries.push(TopEntry::Leaf(row_idx));
        return;
    };
    let pos = if let Some(p) = entries.iter().position(|e| matches!(e, TopEntry::Node(n) if n.seg.key == first.key)) {
        p
    } else {
        entries.push(TopEntry::Node(TreeNode::new(first.clone())));
        entries.len() - 1
    };
    let Some(TopEntry::Node(node)) = entries.get_mut(pos) else { return };
    if rest.is_empty() {
        if is_container_self && node.selectable_row.is_none() {
            node.selectable_row = Some(row_idx);
        } else {
            node.leaves.push(row_idx);
        }
    } else {
        insert_into_nodes(&mut node.children, rest, row_idx, is_container_self);
    }
}

fn emit_node(node: &TreeNode, rows: &[Row], collapsed: &HashSet<String>, depth: usize, out: &mut Vec<DisplayRow>) {
    let total = node.total_items();
    let is_collapsed = collapsed.contains(&node.seg.key);
    let child_count = total.saturating_sub(usize::from(node.selectable_row.is_some()));
    if let Some(row_idx) = node.selectable_row {
        if let Some(row) = rows.get(row_idx) {
            let toggle = (child_count > 0).then(|| (node.seg.key.clone(), is_collapsed, child_count));
            out.push(DisplayRow::Item { row_idx, label: node.seg.name.clone(), tooltip: row.label.clone(), icon: row.icon, depth, toggle });
        }
    } else {
        out.push(DisplayRow::Group {
            key: node.seg.key.clone(),
            name: node.seg.name.clone(),
            icon: node.seg.icon,
            depth,
            count: total,
            collapsed: is_collapsed,
        });
    }
    if !is_collapsed {
        for &leaf_idx in &node.leaves {
            if let Some(row) = rows.get(leaf_idx) {
                out.push(DisplayRow::Item {
                    row_idx: leaf_idx,
                    label: row.short_label.clone(),
                    tooltip: row.label.clone(),
                    icon: row.icon,
                    depth: depth + 1,
                    toggle: None,
                });
            }
        }
        for child in &node.children {
            emit_node(child, rows, collapsed, depth + 1, out);
        }
    }
}

/// Whether `filtered` rows benefit from tree grouping: any component/occurrence hierarchy is
/// present, or there are at least two distinct top-level groups (bodies/sketches) with at least
/// one group holding multiple items.
fn can_group_rows(rows: &[Row], filtered: &[usize]) -> bool {
    let mut top_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for &idx in filtered {
        let Some(row) = rows.get(idx) else { continue };
        if row.hierarchy.iter().any(|s| matches!(s.kind, GroupKind::Occurrence | GroupKind::Component)) {
            return true;
        }
        if let Some(first) = row.hierarchy.first() {
            *top_counts.entry(first.key.as_str()).or_default() += 1;
        }
    }
    top_counts.len() >= 2 && top_counts.values().any(|&n| n >= 2)
}

/// Build the visible display rows for `rows`, applying `filter`, `grouped` and `collapsed`.
/// Returns `(display_rows, can_group)`.
fn build_display_rows(rows: &[Row], filter: Option<Category>, grouped: bool, collapsed: &HashSet<String>) -> (Vec<DisplayRow>, bool) {
    let filtered: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| filter.is_none_or(|cat| r.category == cat)).map(|(i, _)| i).collect();
    let can_group = can_group_rows(rows, &filtered);
    if !grouped || !can_group {
        let flat = filtered
            .into_iter()
            .filter_map(|i| {
                let r = rows.get(i)?;
                Some(DisplayRow::Item { row_idx: i, label: r.label.clone(), tooltip: r.label.clone(), icon: r.icon, depth: 0, toggle: None })
            })
            .collect();
        return (flat, can_group);
    }
    let mut entries: Vec<TopEntry> = Vec::new();
    for idx in filtered {
        let Some(row) = rows.get(idx) else { continue };
        let is_container_self = matches!(
            (&row.sel, row.hierarchy.last().map(|s| s.kind)),
            (Sel::Body { .. }, Some(GroupKind::Body)) | (Sel::Occurrence { .. }, Some(GroupKind::Occurrence))
        );
        insert_top(&mut entries, &row.hierarchy, idx, is_container_self);
    }
    let mut out = Vec::new();
    for entry in &entries {
        match entry {
            TopEntry::Node(node) => emit_node(node, rows, collapsed, 0, &mut out),
            TopEntry::Leaf(idx) => {
                if let Some(r) = rows.get(*idx) {
                    out.push(DisplayRow::Item {
                        row_idx: *idx,
                        label: r.label.clone(),
                        tooltip: r.label.clone(),
                        icon: r.icon,
                        depth: 0,
                        toggle: None,
                    });
                }
            }
        }
    }
    (out, can_group)
}

struct ChipSpec {
    filter: Option<Category>,
    handle: String,
    text: String,
    active: bool,
}

struct ChipRect {
    spec_idx: usize,
    offset: egui::Vec2,
    size: egui::Vec2,
}

const CHIP_H: f32 = 20.0;
const CHIP_GAP: f32 = 4.0;
const CHIP_PAD_X: f32 = 7.0;
const BAR_PAD_X: f32 = 6.0;
const BAR_PAD_Y: f32 = 4.0;

fn layout_chips(ctx: &egui::Context, w: f32, font: &FontId, color: egui::Color32, chips: &[ChipSpec]) -> (Vec<ChipRect>, f32) {
    if chips.is_empty() {
        return (Vec::new(), 0.0);
    }
    let max_x = (w - BAR_PAD_X).max(BAR_PAD_X + 20.0);
    let mut placed = Vec::with_capacity(chips.len());
    let mut x = BAR_PAD_X;
    let mut y = BAR_PAD_Y;
    ctx.fonts_mut(|fonts| {
        for (i, chip) in chips.iter().enumerate() {
            let text_w = fonts.layout_no_wrap(chip.text.clone(), font.clone(), color).size().x;
            let cw = (text_w + CHIP_PAD_X * 2.0).min(max_x - BAR_PAD_X);
            if x > BAR_PAD_X && x + cw > max_x {
                x = BAR_PAD_X;
                y += CHIP_H + CHIP_GAP;
            }
            placed.push(ChipRect { spec_idx: i, offset: vec2(x, y), size: vec2(cw, CHIP_H) });
            x += cw + CHIP_GAP;
        }
    });
    let total_h = y + CHIP_H + BAR_PAD_Y + 2.0;
    (placed, total_h)
}

fn paint_elided_label(ui: &egui::Ui, text_rect: Rect, cy: f32, text: &str, font: FontId, color: egui::Color32) -> bool {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, text_rect.width().max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    let at = pos2(text_rect.left(), cy - galley.size().y / 2.0);
    ui.painter().with_clip_rect(ui.clip_rect().intersect(text_rect)).galley(at, galley, color);
    elided
}

/// Draw the open list; handle its hover, clicks, and a press elsewhere (which closes it).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.viewport.select_other.popup.is_none() {
        return;
    }
    let Some(p) = active_popup(app).cloned() else {
        close(app);
        return;
    };
    let t = Tokens::get();
    let counts = category_counts(&p.rows);
    let show_filters = counts.len() >= 2;
    let (display_rows, can_group) = build_display_rows(&p.rows, p.filter, p.grouped, &p.collapsed);
    let show_tree = p.grouped && can_group;

    let chip_font = FontId::proportional(t.font_small);
    let chips: Vec<ChipSpec> = if show_filters {
        let mut v = Vec::with_capacity(counts.len() + 1);
        v.push(ChipSpec {
            filter: None,
            handle: "select_other:filter:all".into(),
            text: format!("{} {}", app.tr("ui.select_other.filter_all"), p.rows.len()),
            active: p.filter.is_none(),
        });
        for (cat, count) in &counts {
            v.push(ChipSpec {
                filter: Some(*cat),
                handle: format!("select_other:filter:{}", cat.key()),
                text: format!("{} {count}", app.tr(cat.tr_key())),
                active: p.filter == Some(*cat),
            });
        }
        v
    } else {
        Vec::new()
    };

    let screen = ctx.content_rect();
    let margins = crate::ui_kit::Menu::frame().total_margin().sum();
    let base_w = if show_filters || can_group { WIDE_LIST_W } else { crate::ui_kit::LIST_W };
    let width = base_w.min(screen.width());
    let w = (width - margins.x).max(0.0);
    let (chip_rects, filter_bar_h) = layout_chips(ctx, w, &chip_font, t.text, &chips);
    let header = 22.0;
    let gap = vec2(12.0, 12.0);
    let at = pos2(p.at.x.clamp(screen.left(), screen.right()), p.at.y.clamp(screen.top(), screen.bottom()));
    let rows_height = display_rows.len() as f32 * ROW_HEIGHT;
    let overhead = header + filter_bar_h + margins.y;
    let scroll_height = (screen.height() - overhead).max(0.0);
    let size = vec2(width, overhead + rows_height.min(scroll_height));

    let mut hovered = None;
    let mut picked = None;
    let mut next_filter: Option<Option<Category>> = None;
    let mut toggle_grouped = false;
    let mut toggled_group: Option<String> = None;

    let tree_label = app.tr(if show_tree { "ui.select_other.view_flat" } else { "ui.select_other.view_tree" }).to_string();
    let tree_tip = app.tr(if show_tree { "ui.select_other.view_flat_tip" } else { "ui.select_other.view_tree_tip" }).to_string();

    let resp = Container::new(ctx, Surface::Popup)
        .area(egui::Id::unique("sc_select_other"))
        .fixed_pos(crate::context_menu::place(at, gap, size, screen))
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // Area remembers the preceding list's size, which must not limit a longer list.
            ui.set_max_height(size.y);
            crate::ui_kit::Menu::show(ui, width, |ui| {
                // Header: title (+ total when capped) and optional Tree/Flat view toggle.
                let (r, _) = ui.allocate_exact_size(vec2(w, header), Sense::hover());
                let title = solidcraft_ui_text::tr_label("Select Other").to_uppercase();
                let title = if p.total > p.rows.len() { format!("{title} · {} / {}", p.rows.len(), p.total) } else { title };
                ui.painter().text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(t.font_panel), t.text_dim);
                if can_group {
                    let toggle_font = FontId::proportional(t.font_small);
                    let tw = ui.fonts_mut(|f| f.layout_no_wrap(tree_label.clone(), toggle_font.clone(), t.text_dim).size().x);
                    let btn_w = tw + 12.0;
                    let btn_rect = Rect::from_min_size(pos2(r.right() - btn_w - 6.0, r.center().y - 9.0), vec2(btn_w, 18.0));
                    let btn_resp =
                        ui.interact(btn_rect, ui.scope_id().with("select_other_tree_toggle"), Sense::click()).backing_on_hover_text(&tree_tip);
                    crate::scenario::publish_handle("select_other:toggle_tree", btn_rect.center());
                    let bg = if btn_resp.hovered() { t.hover } else { t.panel_header };
                    ui.painter().rect(btn_rect, t.radius_sm, bg, Stroke::new(1.0, t.border), StrokeKind::Inside);
                    ui.painter().text(
                        btn_rect.center(),
                        Align2::CENTER_CENTER,
                        &tree_label,
                        toggle_font,
                        if btn_resp.hovered() { t.text } else { t.text_dim },
                    );
                    if btn_resp.clicked() {
                        toggle_grouped = true;
                    }
                }

                // Category filter chips bar (shown when 2+ categories are present).
                if show_filters && filter_bar_h > 0.0 {
                    let (bar_rect, _) = ui.allocate_exact_size(vec2(w, filter_bar_h), Sense::hover());
                    for cr in &chip_rects {
                        let Some(chip) = chips.get(cr.spec_idx) else { continue };
                        let rect = Rect::from_min_size(bar_rect.min + cr.offset, cr.size);
                        let chip_resp = ui.interact(rect, ui.scope_id().with(&chip.handle), Sense::click());
                        crate::scenario::publish_handle(&chip.handle, rect.center());
                        let (fill, stroke, ink) = if chip.active {
                            (t.accent_soft, Stroke::new(1.0, t.accent), t.text)
                        } else if chip_resp.hovered() {
                            (t.hover, Stroke::new(1.0, t.border), t.text)
                        } else {
                            (t.panel_header, Stroke::new(1.0, t.border), t.text_dim)
                        };
                        ui.painter().rect(rect, t.radius_sm, fill, stroke, StrokeKind::Inside);
                        ui.painter().text(rect.center(), Align2::CENTER_CENTER, &chip.text, chip_font.clone(), ink);
                        if chip_resp.clicked() {
                            next_filter = Some(if chip.active && chip.filter.is_some() { None } else { chip.filter });
                        }
                    }
                    let y = bar_rect.bottom() - 1.0;
                    ui.painter().line_segment([pos2(bar_rect.left() + 4.0, y), pos2(bar_rect.right() - 4.0, y)], Stroke::new(1.0, t.border));
                }

                // Scrollable item / tree list.
                egui::ScrollArea::vertical().min_scrolled_height(0.0).max_height(scroll_height).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for drow in &display_rows {
                        match drow {
                            DisplayRow::Group { key, name, icon, depth, count, collapsed } => {
                                let (r, resp) = ui.allocate_exact_size(vec2(w, ROW_HEIGHT), Sense::click());
                                crate::scenario::publish_handle(&format!("select_other:group:{key}"), r.center());
                                if resp.hovered() {
                                    ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), t.radius_sm, t.hover);
                                }
                                let base_x = r.left() + 6.0 + (*depth as f32) * INDENT_STEP;
                                crate::ui_kit::disclosure(
                                    ui.painter(),
                                    pos2(base_x + 7.0, r.center().y),
                                    !*collapsed,
                                    if resp.hovered() { t.text } else { t.text_dim },
                                );
                                crate::icons::paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(pos2(base_x + 23.0, r.center().y), vec2(15.0, 15.0)),
                                    icon,
                                    (f32::from(t.icon.a()) / 255.0) * 0.85,
                                );
                                let count_text = count.to_string();
                                let count_font = FontId::proportional(t.font_small);
                                let count_w = ui.fonts_mut(|f| f.layout_no_wrap(count_text.clone(), count_font.clone(), t.text_dim).size().x);
                                ui.painter().text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, &count_text, count_font, t.text_dim);
                                let text_rect = Rect::from_min_max(
                                    pos2(base_x + 36.0, r.top()),
                                    pos2((r.right() - count_w - 14.0).max(base_x + 36.0), r.bottom()),
                                );
                                let elided = paint_elided_label(ui, text_rect, r.center().y, name, FontId::proportional(t.font_button), t.text_dim);
                                let resp = if elided { resp.backing_on_hover_text(name) } else { resp };
                                if resp.clicked() {
                                    toggled_group = Some(key.clone());
                                }
                            }
                            DisplayRow::Item { row_idx, label, tooltip, icon, depth, toggle } => {
                                let (r, row_resp) = ui.allocate_exact_size(vec2(w, ROW_HEIGHT), Sense::hover());
                                let (icon_cx, text_left, toggle_clicked) = if show_tree {
                                    let base_x = r.left() + 6.0 + (*depth as f32) * INDENT_STEP;
                                    let mut tclick = false;
                                    if let Some((key, collapsed, _)) = toggle {
                                        let chev_rect = Rect::from_min_size(pos2(base_x, r.top()), vec2(16.0, ROW_HEIGHT));
                                        let chev_resp = ui.interact(chev_rect, ui.scope_id().with(("select_other_chev", key)), Sense::click());
                                        crate::scenario::publish_handle(&format!("select_other:toggle:{key}"), chev_rect.center());
                                        crate::ui_kit::disclosure(
                                            ui.painter(),
                                            chev_rect.center(),
                                            !*collapsed,
                                            if chev_resp.hovered() { t.text } else { t.text_dim },
                                        );
                                        if chev_resp.clicked() {
                                            toggled_group = Some(key.clone());
                                            tclick = true;
                                        }
                                    }
                                    (base_x + 23.0, base_x + 36.0, tclick)
                                } else {
                                    (r.left() + 16.0, r.left() + 32.0, false)
                                };

                                let click_left = if show_tree && toggle.is_some() { icon_cx - 8.0 } else { r.left() };
                                let click_rect = Rect::from_min_max(pos2(click_left, r.top()), r.max);
                                let resp = ui.interact(click_rect, ui.scope_id().with(("select_other_row", *row_idx)), Sense::click());
                                crate::scenario::publish_handle(&format!("select_other:{row_idx}"), click_rect.center());
                                if resp.hovered() {
                                    hovered = Some(*row_idx);
                                    ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), t.radius_sm, t.hover);
                                } else if row_resp.hovered() && !toggle_clicked {
                                    // Subtle hover feedback when pointer is over the chevron of a selectable parent.
                                    ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), t.radius_sm, t.hover);
                                }
                                crate::icons::paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(pos2(icon_cx, r.center().y), vec2(16.0, 16.0)),
                                    icon,
                                    f32::from(t.icon.a()) / 255.0,
                                );
                                let text_right = if let Some((_, _, child_count)) = toggle {
                                    let count_text = child_count.to_string();
                                    let count_font = FontId::proportional(t.font_small);
                                    let count_w = ui.fonts_mut(|f| f.layout_no_wrap(count_text.clone(), count_font.clone(), t.text_dim).size().x);
                                    ui.painter().text(pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, &count_text, count_font, t.text_dim);
                                    (r.right() - count_w - 14.0).max(text_left)
                                } else {
                                    (r.right() - 8.0).max(text_left)
                                };
                                // Names can be wider than the list. Keep one line inside the row and
                                // expose the full name on hover, as the browser tree does.
                                let text_rect = Rect::from_min_max(pos2(text_left, r.top()), pos2(text_right, r.bottom()));
                                let elided = paint_elided_label(ui, text_rect, r.center().y, label, FontId::proportional(t.font_button), t.text);
                                let resp = if elided { resp.backing_on_hover_text(tooltip) } else { resp };
                                if resp.clicked() && !toggle_clicked {
                                    picked = Some(*row_idx);
                                }
                            }
                        }
                    }
                });
            });
        });
    crate::scenario::publish_rect("select_other", resp.response.rect);
    if let Some(pop) = app.viewport.select_other.popup.as_mut() {
        let mut changed = false;
        if pop.hovered != hovered {
            pop.hovered = hovered;
            changed = true;
        }
        if let Some(f) = next_filter {
            pop.filter = f;
            pop.hovered = None;
            changed = true;
        }
        if toggle_grouped {
            pop.grouped = !pop.grouped;
            changed = true;
        }
        if let Some(key) = toggled_group {
            if !pop.collapsed.remove(&key) {
                pop.collapsed.insert(key);
            }
            changed = true;
        }
        if changed {
            ctx.request_repaint();
        }
    }
    let (pressed, pointer, mods) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos(), i.modifiers));
    if let Some(i) = picked {
        let held = Held::from_modifiers(mods);
        let (add, toggle) = crate::tool_modifiers::selection_click(held, app.preferences.selection_shift_add_only);
        let _ = choose(app, i, add, toggle);
    } else if pressed && !pointer.is_some_and(|q| resp.response.rect.contains(q)) {
        // A press elsewhere only closes the list.
        close(app);
        app.viewport.select_other.held = true;
    }
}

#[cfg(test)]
#[path = "select_other_tests.rs"]
mod tests;
