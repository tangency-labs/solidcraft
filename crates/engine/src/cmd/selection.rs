//! Reading the selection (`selection.get`): what is selected, resolved to what an agent or a
//! script needs to act on it, without changing the design, the selection or the undo history.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use super::CommandSpec;
use super::measure_sel::{resolve, sketch_of};
use crate::params::bad;
use crate::schema::selection::{MAX_DETAIL, SelectionGetParams, SelectionGetResult, SelectionItem};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Sel, Session};

const CONTRACT: TypedContract<SelectionGetParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("selection.get", "Get Selection", crate::checked!("selection.get", selection_get))
    .icon("select")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<SelectionGetResult>)
    .params(
    "detail?: bool (default true: resolve each item's geometry) → {count, items: [{kind (= type: body|face|edge|vertex|feature|sketch_curve|\
     sketch_point|sketch_midpoint|sketch_constraint|profile|plane|axis|construction_point|occurrence…), the selection's own fields as select.set takes them (body, index, point, \
     id, sketch, path…), and with detail: face: name (persistent face name), name_exact, surface ({type: plane|cylinder|cone|sphere|torus|\
     other, normal/point, origin/axis/radius…}), area_mm2, planar; edge: name (persistent edge name), curve (line|circle|arc|other), \
     length_mm, start, end, center/axis/radius for round edges; body: volume_mm3, feature, visible; any body item: component {id, name} \
     and occurrence_path in an assembly; sketch items: sketch (its id, as select.set takes it), sketch_name, entity (line|circle|arc|point|midpoint|…), position for points and midpoints}]}; read-only, [] when nothing is \
     selected",
)];

fn selection_get(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "selection.get";
    let detail = match p.get("detail") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`detail` must be true or false")),
    };
    let s: &Session = s;
    let mut ctx = Context::new(s)?;
    let items: Vec<SelectionItem> = s
        .selection
        .iter()
        .enumerate()
        .map(|(i, sel)| {
            let mut v = identity(sel);
            if detail && i < MAX_DETAIL {
                ctx.add_detail(sel, &mut v);
            }
            SelectionItem(v)
        })
        .collect();
    let detail_limit = (detail && s.selection.len() > MAX_DETAIL).then_some(MAX_DETAIL);
    let reply = SelectionGetResult { count: items.len(), items, detail_limit, param_warnings: Vec::new() };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

/// The selection item as `select.set` takes it, with `kind` naming what it is.
fn identity(sel: &Sel) -> Map<String, Value> {
    let mut v = match serde_json::to_value(sel) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Some(t) = v.get("type").cloned() {
        v.insert("kind".into(), t);
    }
    v
}

/// What resolving items shares: each body's placement and edge names, worked out once.
struct Context<'a> {
    s: &'a Session,
    st: std::sync::Arc<solidcraft_doc::ModelState>,
    /// World body name → (definition name, component, occurrence path); only in an assembly.
    placements: Option<BTreeMap<String, (String, u64, Vec<u64>)>>,
    edge_names: BTreeMap<String, Vec<String>>,
}

impl<'a> Context<'a> {
    fn new(s: &'a Session) -> Result<Self> {
        Ok(Context { s, st: s.world_state()?, placements: None, edge_names: BTreeMap::new() })
    }

    fn add_detail(&mut self, sel: &Sel, v: &mut Map<String, Value>) {
        // The measurement `inspect.measure` gives the item, without its own `type`.
        let mut dir = None;
        if let Some((Value::Object(m), d)) = resolve(self.s, &self.st, sel) {
            dir = d;
            for (k, x) in m {
                if k != "type" && !v.contains_key(&k) {
                    v.insert(k, x);
                }
            }
        }
        match sel {
            Sel::Body { name } => {
                self.body_info(name, v);
                if let Some(b) = self.st.body(name) {
                    let feature = self.s.doc.feature(b.feature);
                    v.insert("feature".into(), json!({"id": b.feature, "name": feature.map(|f| f.name.clone())}));
                }
                v.insert("visible".into(), json!(self.s.body_visible(name)));
            }
            Sel::Face { body, index, .. } => {
                self.body_info(body, v);
                if let Some(b) = self.st.body(body) {
                    let names = solidcraft_doc::naming::face_names(b);
                    if let Some(n) = names.get(*index).filter(|n| !n.is_empty()) {
                        v.insert("name".into(), json!(n));
                        v.insert("name_exact".into(), json!(solidcraft_doc::naming::face_name_exact(b, *index)));
                    }
                    v.insert("surface".into(), surface(solidcraft_kernel::face_surface(&b.body, *index), dir));
                }
            }
            Sel::Edge { body, index, .. } => {
                self.body_info(body, v);
                if let Some(b) = self.st.body(body) {
                    // Its ends from its exact curve; an intersection curve, which the record holds
                    // only as its surfaces, has none to give.
                    if let Ok(segs) = solidcraft_kernel::edge_path(&b.body, *index)
                        && let (Some(a), Some(z)) = (segs.first(), segs.last())
                    {
                        v.insert("start".into(), json!(a.start()));
                        v.insert("end".into(), json!(z.end()));
                    }
                    let names = self.edge_names.entry(body.clone()).or_insert_with(|| solidcraft_doc::naming::edge_names(b));
                    if let Some(n) = names.get(*index).filter(|n| !n.is_empty()) {
                        v.insert("name".into(), json!(n));
                    }
                    if let Some(d) = dir {
                        v.insert("direction".into(), json!(d));
                    }
                }
            }
            Sel::Vertex { body, .. } => self.body_info(body, v),
            Sel::Feature { id } => {
                if let Some(f) = self.s.doc.feature(*id) {
                    v.insert("name".into(), json!(f.name));
                    v.insert("feature_type".into(), json!(f.kind.type_name()));
                    v.insert("suppressed".into(), json!(f.suppressed));
                }
            }
            Sel::SketchCurve { id, sketch } => self.sketch_entity(*sketch, id, Entity::Curve, v),
            Sel::ExternalSketchCurve { sketch, id } => self.sketch_entity(Some(*sketch), id, Entity::Curve, v),
            Sel::SketchPoint { id } => self.sketch_entity(None, id, Entity::Point, v),
            // The midpoint of a line (`id` names the line): its sketch and where it is.
            Sel::SketchMidpoint { id } => self.sketch_entity(None, id, Entity::Midpoint, v),
            Sel::ExternalSketchPoint { sketch, id } => self.sketch_entity(Some(*sketch), id, Entity::Point, v),
            Sel::SketchConstraint { id } => self.sketch_entity(None, id, Entity::Constraint, v),
            Sel::Profile { sketch, .. } => {
                if let Some(ss) = self.st.sketch(*sketch) {
                    v.insert("sketch".into(), json!(ss.feature));
                    v.insert("sketch_name".into(), json!(ss.name));
                }
            }
            Sel::Occurrence { path } => {
                if let Ok(o) = self.s.occurrence_placement(path) {
                    v.insert("name".into(), json!(o.name));
                    v.insert("component".into(), self.component(o.component));
                    v.insert("grounded".into(), json!(o.grounded));
                    v.insert("bodies".into(), json!(o.bodies.iter().map(|b| &b.world).collect::<Vec<_>>()));
                }
            }
            // What these are (origin, direction, position) comes from the measurement above.
            Sel::Plane { .. } | Sel::Axis { .. } | Sel::ConstructionPoint { .. } => {}
        }
    }

    /// A body item's component and occurrence path (in an assembly), and its definition's name
    /// when it is a further placement of it.
    fn body_info(&mut self, body: &str, v: &mut Map<String, Value>) {
        if self.s.doc.occurrences.is_empty() {
            return;
        }
        let s = self.s;
        let placements = self.placements.get_or_insert_with(|| {
            s.doc
                .body_frames_with_source(&s.model.state())
                .into_iter()
                .map(|(world, source, component, path, _)| (world, (source, component, path)))
                .collect()
        });
        let Some((source, component, path)) = placements.get(body).cloned() else { return };
        if source != body {
            v.insert("source_body".into(), json!(source));
        }
        if !path.is_empty() {
            v.insert("occurrence_path".into(), json!(path));
        }
        v.insert("component".into(), self.component(component));
    }

    fn component(&self, id: u64) -> Value {
        let name = if id == 0 { Some(self.s.doc.name.clone()) } else { self.s.doc.components.iter().find(|c| c.id == id).map(|c| c.name.clone()) };
        json!({"id": id, "name": name})
    }

    /// A sketch curve, point, line midpoint or constraint: its sketch and what kind of entity it is.
    fn sketch_entity(&self, sketch: Option<u64>, id: &str, entity: Entity, v: &mut Map<String, Value>) {
        // "<sketch id>:<point id>" names a point of any sketch.
        let (sketch, id) = match (entity, id.split_once(':').and_then(|(k, p)| Some((k.parse::<u64>().ok()?, p)))) {
            (Entity::Point, Some((k, p))) if sketch.is_none() => (Some(k), p),
            _ => (sketch, id),
        };
        let found = match entity {
            Entity::Curve | Entity::Midpoint => sketch_of(self.s, &self.st, sketch, |sk| sk.curve_index(id).is_some()),
            Entity::Point => sketch_of(self.s, &self.st, sketch, |sk| sk.point_index(id).is_some()),
            Entity::Constraint => sketch_of(self.s, &self.st, sketch, |sk| sk.constraints.iter().any(|c| c.id == id)),
        };
        let Some(ss) = found else { return };
        v.insert("sketch".into(), json!(ss.feature));
        v.insert("sketch_name".into(), json!(ss.name));
        let sk = &ss.sketch;
        match entity {
            Entity::Curve => {
                if let Some(c) = sk.curve_index(id).and_then(|i| sk.curves.get(i)) {
                    v.insert("entity".into(), kind_name(serde_json::to_value(&c.kind).ok()));
                    v.insert("construction".into(), json!(c.construction));
                }
            }
            Entity::Point => {
                if let Some(pt) = sk.point_index(id).and_then(|i| sk.points.get(i)) {
                    v.insert("entity".into(), json!("point"));
                    v.entry("position").or_insert_with(|| json!(ss.plane.to_world(pt.pos)));
                }
            }
            Entity::Midpoint => {
                v.insert("entity".into(), json!("midpoint"));
                let line = sk.curve_index(id).and_then(|i| sk.curves.get(i)).and_then(|c| match c.kind {
                    solidcraft_sketch::CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
                    _ => None,
                });
                if let Some((a, b)) = line {
                    v.entry("position").or_insert_with(|| json!(ss.plane.to_world(a.lerp(b, 0.5))));
                }
            }
            Entity::Constraint => {
                if let Some(c) = sk.constraints.iter().find(|c| c.id == id) {
                    v.insert("entity".into(), kind_name(serde_json::to_value(c).ok()));
                    v.insert("driven".into(), json!(c.driven));
                    if let Some(param) = &c.param {
                        v.insert("parameter".into(), json!(param));
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Entity {
    Curve,
    Point,
    Midpoint,
    Constraint,
}

/// The point a note anchors to when the caller names none: where the selection points, the
/// newest item first, or the origin when nothing is selected (#16015).
pub(super) fn anchor_from_selection(s: &Session) -> Vec3 {
    s.selection.iter().rev().find_map(|sel| anchor_of(s, sel)).unwrap_or(Vec3::ZERO)
}

/// The point a selected item points at, when it names one. An item that names no point (a
/// feature, a constraint, a profile) leaves the anchor to the next selected item.
fn anchor_of(s: &Session, sel: &Sel) -> Option<Vec3> {
    match sel {
        // A picked face, edge or vertex: the point it was picked at.
        Sel::Face { point, .. } | Sel::Edge { point, .. } | Sel::Vertex { point, .. } => point.is_finite().then_some(*point),
        // A body: the middle of its box.
        Sel::Body { name } => {
            let st = s.world_state().ok()?;
            st.body(name).map(|b| b.outline_bounds().center())
        }
        // A construction point: where it sits.
        Sel::ConstructionPoint { name } => crate::view::construction_points(s).into_iter().find(|(_, n, _)| n == name).map(|(_, _, p)| p),
        // A datum plane or axis: the point it passes through.
        Sel::Plane { name } => super::measure_sel::plane_named(s, name).map(|(p, _)| p),
        Sel::Axis { name } => super::measure_sel::axis_named(s, name).map(|(p, _)| p),
        // A sketch point: where it is in the world.
        Sel::SketchPoint { id } => sketch_point(s, None, id),
        Sel::ExternalSketchPoint { sketch, id } => sketch_point(s, Some(*sketch), id),
        // A sketch line's midpoint.
        Sel::SketchMidpoint { id } => sketch_midpoint(s, id),
        // A sketch curve: its middle.
        Sel::SketchCurve { id, sketch } => sketch_curve_point(s, *sketch, id),
        Sel::ExternalSketchCurve { sketch, id } => sketch_curve_point(s, Some(*sketch), id),
        // An occurrence: where it is placed in the world.
        Sel::Occurrence { path } => s.occurrence_placement(path).ok().map(|p| solidcraft_doc::apply_point(&p.world, Vec3::ZERO)),
        Sel::Feature { .. } | Sel::SketchConstraint { .. } | Sel::Profile { .. } => None,
    }
}

/// A sketch point's world position. The id of a point of any sketch may name its sketch
/// ("<sketch id>:<point id>").
fn sketch_point(s: &Session, sketch: Option<u64>, id: &str) -> Option<Vec3> {
    let st = s.world_state().ok()?;
    let (sketch, id) = match id.split_once(':').and_then(|(k, p)| k.parse::<u64>().ok().map(|k| (k, p))) {
        Some((k, p)) if sketch.is_none() => (Some(k), p),
        _ => (sketch, id),
    };
    let ss = sketch_of(s, &st, sketch, |sk| sk.point_index(id).is_some())?;
    let pt = ss.sketch.point_index(id).and_then(|i| ss.sketch.points.get(i))?;
    Some(ss.plane.to_world(pt.pos))
}

/// A sketch line's midpoint, in world space.
fn sketch_midpoint(s: &Session, id: &str) -> Option<Vec3> {
    let st = s.world_state().ok()?;
    let ss = sketch_of(s, &st, None, |sk| sk.curve_index(id).is_some())?;
    let curve = ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i))?;
    let solidcraft_sketch::CurveKind::Line { a, b } = curve.kind else { return None };
    let (a, b) = (ss.sketch.point(a)?, ss.sketch.point(b)?);
    Some(ss.plane.to_world(a.lerp(b, 0.5)))
}

/// The middle of a sketch curve, in world space: the middle of a line, the centre of a circle
/// or an ellipse, and the point halfway along an arc.
fn sketch_curve_point(s: &Session, sketch: Option<u64>, id: &str) -> Option<Vec3> {
    let st = s.world_state().ok()?;
    let ss = sketch_of(s, &st, sketch, |sk| sk.curve_index(id).is_some())?;
    let curve = ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i))?;
    let at = |i: usize| ss.sketch.point(i);
    match curve.kind {
        solidcraft_sketch::CurveKind::Line { a, b } => {
            let (a, b) = (at(a)?, at(b)?);
            Some(ss.plane.to_world(a.lerp(b, 0.5)))
        }
        solidcraft_sketch::CurveKind::Circle { c, .. } | solidcraft_sketch::CurveKind::Ellipse { c, .. } => Some(ss.plane.to_world(at(c)?)),
        solidcraft_sketch::CurveKind::Arc { c, a, b } => {
            let (centre, a, b) = (at(c)?, at(a)?, at(b)?);
            let radius = (a - centre).len();
            let start = (a - centre).angle();
            // The arc runs counter-clockwise from `a` to `b` about its centre.
            let sweep = ((b - centre).angle() - start).rem_euclid(std::f64::consts::TAU);
            let mid = centre + Vec2::from_angle(start + sweep / 2.0) * radius;
            Some(ss.plane.to_world(mid))
        }
        _ => None,
    }
}

/// The `type` tag of a serialized sketch curve or constraint.
fn kind_name(v: Option<Value>) -> Value {
    v.and_then(|v| v.get("type").cloned()).unwrap_or_else(|| json!("other"))
}

/// A face's surface: an analytic one with its defining values, else a plane when the face is
/// flat (`normal` from its triangles), else `other`.
fn surface(s: Option<solidcraft_kernel::FaceSurface>, planar: Option<Vec3>) -> Value {
    use solidcraft_kernel::FaceSurface as F;
    match s {
        Some(F::Plane { normal, point }) => json!({"type": "plane", "normal": normal, "point": point}),
        Some(F::Cylinder { origin, axis, radius }) => json!({"type": "cylinder", "origin": origin, "axis": axis, "radius": radius}),
        Some(F::Cone { apex, axis, half_angle }) => json!({"type": "cone", "apex": apex, "axis": axis, "half_angle_deg": half_angle.to_degrees()}),
        Some(F::Sphere { center, radius }) => json!({"type": "sphere", "center": center, "radius": radius}),
        Some(F::Torus { center, axis, major, minor }) => {
            json!({"type": "torus", "center": center, "axis": axis, "major_radius": major, "minor_radius": minor})
        }
        None => match planar {
            Some(n) => json!({"type": "plane", "normal": n}),
            None => json!({"type": "other"}),
        },
    }
}
