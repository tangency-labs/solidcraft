//! Reading the selection (`selection.get`): what is selected, resolved to what an agent or a
//! script needs to act on it, without changing the design, the selection or the undo history.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use solidcraft_geom::Vec3;

use super::CommandSpec;
use super::measure_sel::{edge_index, resolve, sketch_of};
use crate::params::bad;
use crate::{Result, Sel, Session};

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("selection.get", "Get Selection", selection_get).icon("select").noundo().params(
    "detail?: bool (default true: resolve each item's geometry) → {count, items: [{kind (= type: body|face|edge|vertex|feature|sketch_curve|\
     sketch_point|sketch_constraint|profile|plane|axis|occurrence…), the selection's own fields as select.set takes them (body, index, point, \
     id, sketch, path…), and with detail: face: name (persistent face name), name_exact, surface ({type: plane|cylinder|cone|sphere|torus|\
     other, normal/point, origin/axis/radius…}), area_mm2, planar; edge: name (persistent edge name), curve (line|circle|arc|other), \
     length_mm, start, end, center/axis/radius for round edges; body: volume_mm3, feature, visible; any body item: component {id, name} \
     and occurrence_path in an assembly; sketch items: sketch {id, name}, entity (line|circle|arc|…)}]}; read-only, [] when nothing is \
     selected",
)];

/// Items resolved in full; the rest of a huge selection is listed by identity only.
const MAX_DETAIL: usize = 500;

fn selection_get(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "selection.get";
    let detail = match p.get("detail") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(cmd, "`detail` must be true or false")),
    };
    let s: &Session = s;
    let mut ctx = Context::new(s);
    let items: Vec<Value> = s
        .selection
        .iter()
        .enumerate()
        .map(|(i, sel)| {
            let mut v = identity(sel);
            if detail && i < MAX_DETAIL {
                ctx.add_detail(sel, &mut v);
            }
            Value::Object(v)
        })
        .collect();
    let mut out = json!({"count": items.len(), "items": items});
    if detail && s.selection.len() > MAX_DETAIL {
        out["detail_limit"] = json!(MAX_DETAIL);
    }
    Ok(out)
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
    fn new(s: &'a Session) -> Self {
        Context { s, st: s.world_state(), placements: None, edge_names: BTreeMap::new() }
    }

    fn add_detail(&mut self, sel: &Sel, v: &mut Map<String, Value>) {
        // The measurement `inspect.measure` gives the item, without its own `type`.
        let mut dir = None;
        if let Some((Value::Object(m), d)) = resolve(self.s, sel) {
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
            Sel::Edge { body, index, point } => {
                self.body_info(body, v);
                if let Some(b) = self.st.body(body) {
                    let m = b.mesh();
                    if let Some(i) = edge_index(&m, *index, *point) {
                        if let Some(pts) = m.edges.get(i) {
                            v.insert("start".into(), json!(pts.first()));
                            v.insert("end".into(), json!(pts.last()));
                        }
                        let names = self.edge_names.entry(body.clone()).or_insert_with(|| solidcraft_doc::naming::edge_names(b));
                        if let Some(n) = names.get(i).filter(|n| !n.is_empty()) {
                            v.insert("name".into(), json!(n));
                        }
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
            Sel::SketchCurve { id } => self.sketch_entity(None, id, Entity::Curve, v),
            Sel::ExternalSketchCurve { sketch, id } => self.sketch_entity(Some(*sketch), id, Entity::Curve, v),
            Sel::SketchPoint { id } => self.sketch_entity(None, id, Entity::Point, v),
            Sel::ExternalSketchPoint { sketch, id } => self.sketch_entity(Some(*sketch), id, Entity::Point, v),
            Sel::SketchConstraint { id } => self.sketch_entity(None, id, Entity::Constraint, v),
            Sel::Profile { sketch, .. } => {
                if let Some(ss) = self.st.sketch(*sketch) {
                    v.insert("sketch".into(), json!({"id": ss.feature, "name": ss.name}));
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
            Sel::Plane { .. } | Sel::Axis { .. } => {}
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

    /// A sketch curve, point or constraint: its sketch and what kind of entity it is.
    fn sketch_entity(&self, sketch: Option<u64>, id: &str, entity: Entity, v: &mut Map<String, Value>) {
        // "<sketch id>:<point id>" names a point of any sketch.
        let (sketch, id) = match (entity, id.split_once(':').and_then(|(k, p)| Some((k.parse::<u64>().ok()?, p)))) {
            (Entity::Point, Some((k, p))) if sketch.is_none() => (Some(k), p),
            _ => (sketch, id),
        };
        let found = match entity {
            Entity::Curve => sketch_of(self.s, &self.st, sketch, |sk| sk.curve_index(id).is_some()),
            Entity::Point => sketch_of(self.s, &self.st, sketch, |sk| sk.point_index(id).is_some()),
            Entity::Constraint => sketch_of(self.s, &self.st, sketch, |sk| sk.constraints.iter().any(|c| c.id == id)),
        };
        let Some(ss) = found else { return };
        v.insert("sketch".into(), json!({"id": ss.feature, "name": ss.name}));
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
    Constraint,
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
