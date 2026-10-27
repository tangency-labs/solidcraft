//! A FreeCAD design's history rebuilt as SolidCraft features: its sketches, the operations that
//! use them (Pad, Pocket, Revolution, Groove, Fillet, Chamfer) and the bodies they build.
//!
//! cadmpeg decodes the file at full depth into its neutral model; this module reads that model's
//! design arenas through their serialised (wire) form, which is stable for a pinned cadmpeg, and
//! writes a small [`Document`] with one feature per FreeCAD feature, in history order. Sketch
//! geometry is FreeCAD's stored solved geometry; a constraint is carried over only when the
//! sketch still solves to that same geometry with it.
//!
//! What cannot be rebuilt (another operation, an up-to-face extent, a sketch curve kind we do not
//! draw) is an error naming it: the caller then imports the file's solids as before. The caller
//! also compares the rebuilt bodies with the file's own solids ([`check_against`]).

use std::collections::HashMap;

use serde_json::Value;
use solidcraft_doc::sketch::{ConstraintKind, CurveKind, Sketch};
use solidcraft_doc::{
    AxisRef, Dir, Direction, Document, Expr, Extent, ExtrudeConvention, FeatureKind, Operation, PlaneRef, Position, ProfileSel, RevolveConvention,
    SketchId,
};
use solidcraft_geom::{Plane, Vec2, Vec3};

/// Features read at most (a design with more is imported as solids).
const MAX_FEATURES: usize = 2000;
/// Sketch entities and constraints read at most per sketch.
const MAX_SKETCH_ITEMS: usize = 2000;
/// Two sketch points closer than this (mm) are one point.
const SAME_POINT: f64 = 1e-7;
/// A solve may move a point this far (mm) and still keep FreeCAD's geometry.
const KEPT: f64 = 1e-6;

/// A FreeCAD design rebuilt as features.
pub struct NativeDesign {
    /// The features, in history order, in the document's root component; the bodies are named
    /// after FreeCAD's bodies.
    pub doc: Document,
    /// What came over only in part (constraints that were left out, for one).
    pub warnings: Vec<String>,
}

/// The design arenas of a decoded model, in wire form.
pub(super) struct Arenas {
    features: Vec<Value>,
    sketches: HashMap<String, Value>,
    entities: Vec<Value>,
    constraints: Vec<Value>,
    parameters: HashMap<String, Value>,
    /// Each FreeCAD object's placement (by object name), a row-major 4 × 4 matrix. A body's
    /// features are placed in it; a feature's stored shape is placed by its own.
    pub(super) placements: HashMap<String, Mat4>,
    /// The origin axis (origin, direction) a LinearPattern's `Direction` or a PolarPattern's
    /// `Axis` names, by pattern object name: cadmpeg 0.6.6 takes such an axis's direction from
    /// the third column of its placement, where a FreeCAD origin axis runs along the first.
    pub(super) pattern_axes: HashMap<String, (Vec3, Vec3)>,
}

/// A row-major 4 × 4 placement matrix.
pub(super) type Mat4 = [[f64; 4]; 4];

fn apply(m: &Mat4, p: Vec3) -> Vec3 {
    let row = |r: &[f64; 4]| r[0] * p.x + r[1] * p.y + r[2] * p.z + r[3];
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

fn rotate(m: &Mat4, d: Vec3) -> Vec3 {
    let row = |r: &[f64; 4]| r[0] * d.x + r[1] * d.y + r[2] * d.z;
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

impl Arenas {
    pub(super) fn of(model: &cadmpeg_ir::document::Model) -> Result<Arenas, String> {
        let wire = |e: serde_json::Error| format!("could not read the design history: {e}");
        let list = |v: Value| match v {
            Value::Array(a) => a,
            _ => Vec::new(),
        };
        let by_id = |v: Vec<Value>| v.into_iter().filter_map(|x| Some((x.get("id")?.as_str()?.to_string(), x))).collect::<HashMap<_, _>>();
        Ok(Arenas {
            features: list(serde_json::to_value(&model.features).map_err(wire)?),
            sketches: by_id(list(serde_json::to_value(&model.sketches).map_err(wire)?)),
            entities: list(serde_json::to_value(&model.sketch_entities).map_err(wire)?),
            constraints: list(serde_json::to_value(&model.sketch_constraints).map_err(wire)?),
            parameters: by_id(list(serde_json::to_value(&model.parameters).map_err(wire)?)),
            placements: HashMap::new(),
            pattern_axes: HashMap::new(),
        })
    }
}

/// Where a fillet or chamfer edge is: a point on it, in model coordinates.
pub(super) type EdgePoint<'a> = dyn Fn(&str) -> Option<Vec3> + 'a;

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str()
}

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k)?.as_f64().filter(|x| x.is_finite())
}

fn vec3(v: Option<&Value>) -> Option<Vec3> {
    let v = v?;
    Some(Vec3::new(f(v, "x")?, f(v, "y")?, f(v, "z")?))
}

fn uv(v: Option<&Value>) -> Option<Vec2> {
    let v = v?;
    Some(Vec2::new(f(v, "u")?, f(v, "v")?))
}

fn mm(v: f64) -> Expr {
    Expr::from(format!("{} mm", round(v)).as_str())
}

fn deg(radians: f64) -> Expr {
    Expr::from(format!("{} deg", round(radians.to_degrees())).as_str())
}

/// `v` without the last bits of binary noise (`9.999999999999998` → `10`).
fn round(v: f64) -> f64 {
    let r = (v * 1e9).round() / 1e9;
    if r == 0.0 { 0.0 } else { r }
}

/// The FreeCAD object name in an id such as `fcstd:design:feature#Pad`.
fn short(id: &str) -> &str {
    id.rsplit_once('#').map_or(id, |(_, n)| n)
}

/// The operation inside a feature's definition (FreeCAD wraps refined ones in `post_process`).
fn operation(feature: &Value) -> Option<&Value> {
    let d = feature.get("definition")?;
    if s(d, "definition") == Some("post_process") { d.get("operation") } else { Some(d) }
}

/// The design's history rebuilt as features. `edge_point` finds a point on a model edge.
pub(super) fn rebuild(arenas: &Arenas, edge_point: &EdgePoint<'_>) -> Result<NativeDesign, String> {
    if arenas.features.len() > MAX_FEATURES {
        return Err(format!("{} features (at most {MAX_FEATURES} are rebuilt)", arenas.features.len()));
    }
    let mut features: Vec<&Value> = arenas.features.iter().collect();
    features.sort_by_key(|f| f.get("ordinal").and_then(Value::as_u64).unwrap_or(u64::MAX));

    // Each feature's PartDesign body. A feature after a body's tip changes what is built, and
    // the comparison with the file's solids then refuses the rebuild.
    let mut body_of: HashMap<&str, &str> = HashMap::new();
    for feature in &features {
        let Some(d) = operation(feature) else { continue };
        if s(d, "definition") == Some("tree_node") && s(d, "role") == Some("solid_bodies") {
            let body = s(feature, "id").unwrap_or_default();
            for child in d.pointer("/children/children").and_then(Value::as_array).into_iter().flatten() {
                if let Some(c) = child.as_str() {
                    body_of.insert(c, body);
                }
            }
        }
    }
    let name_of = |id: &str| -> String {
        features.iter().find(|f| s(f, "id") == Some(id)).and_then(|f| s(f, "name")).map_or_else(|| short(id).to_string(), str::to_string)
    };

    let mut b = Builder {
        doc: Document::new("FreeCAD"),
        warnings: Vec::new(),
        sketches: HashMap::new(),
        bodies: HashMap::new(),
        frame: IDENTITY,
        made: HashMap::new(),
    };
    // A point on a model edge, placed: the edge's shape by its object's placement, and that
    // object by its body's.
    let placed_edge = |edge: &str| -> Option<Vec3> {
        let object = edge.split_once('#')?.1.split(':').next()?;
        let mut p = edge_point(edge)?;
        if let Some(m) = arenas.placements.get(object) {
            p = apply(m, p);
        }
        let body = body_of.get(format!("fcstd:design:feature#{object}").as_str()).map(|b| short(b));
        Some(body.and_then(|b| arenas.placements.get(b)).map_or(p, |m| apply(m, p)))
    };
    for feature in &features {
        let id = s(feature, "id").unwrap_or_default();
        let name = s(feature, "name").map_or_else(|| short(id).to_string(), str::to_string);
        let suppressed = feature.get("suppressed").and_then(Value::as_bool).unwrap_or(false);
        let tag = s(feature, "source_tag").unwrap_or("feature");
        let d = operation(feature).ok_or_else(|| format!("{name}: no definition"))?;
        let body = body_of.get(id).map(|b| name_of(b));
        b.frame = body_of.get(id).and_then(|b| arenas.placements.get(short(b))).copied().unwrap_or(IDENTITY);
        let made = match s(d, "definition").unwrap_or_default() {
            // Structure (bodies, groups, spreadsheets), construction geometry and binders (copies
            // of other geometry for sketches to refer to) make no solid.
            "tree_node" | "datum_plane" | "datum_axis" | "datum_point" | "datum_coordinate_system" | "binder" => None,
            "sketch" => Some(b.sketch(arenas, d, &name)?),
            _ if suppressed => {
                b.warnings.push(format!("{name}: suppressed in FreeCAD, left out"));
                None
            }
            "extrude" => Some(b.extrude(d, &name, body.as_deref(), tag == "PartDesign::Pocket")?),
            "revolve" => Some(b.revolve(d, &name, body.as_deref())?),
            "primitive" => {
                let placement = arenas.placements.get(short(id)).copied().unwrap_or(IDENTITY);
                Some(b.primitive(d, &name, body.as_deref(), &placement)?)
            }
            "fillet" | "chamfer" => Some(b.edge_treatment(d, &name, body.as_deref(), &placed_edge)?),
            "pattern" => Some(b.pattern(d, &name, body.as_deref(), arenas.pattern_axes.get(short(id)))?),
            other => return Err(format!("{name}: FreeCAD {tag} ({other}) is not rebuilt yet")),
        };
        if let Some(fid) = made {
            b.made.insert(id.to_string(), fid);
        }
        if let (Some(fid), true) = (made, suppressed)
            && let Some(f) = b.doc.feature_mut(fid)
        {
            f.suppressed = true;
        }
    }
    if b.bodies.is_empty() {
        return Err("no solid is built by its history".into());
    }
    Ok(NativeDesign { doc: b.doc, warnings: b.warnings })
}

struct Builder {
    doc: Document,
    warnings: Vec<String>,
    /// cadmpeg sketch id → (our sketch feature, its plane).
    sketches: HashMap<String, (u64, Plane)>,
    /// FreeCAD body name → whether a feature has made it yet.
    bodies: HashMap<String, ()>,
    /// The placement of the body the current feature is in.
    frame: Mat4,
    /// cadmpeg feature id → our feature made for it (the last, when it made several).
    made: HashMap<String, u64>,
}

const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// The parts of a sketch entity a constraint can name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Role {
    Start,
    End,
    Center,
    Point,
}

impl Builder {
    fn add(&mut self, kind: FeatureKind, name: &str) -> Result<u64, String> {
        self.doc.add_feature(kind, Some(name)).map_err(|e| format!("{name}: {e}"))
    }

    /// The operation and target of a solid feature in FreeCAD body `body`.
    fn operation(&mut self, op: &str, name: &str, body: Option<&str>) -> Result<(Operation, Vec<solidcraft_doc::BodyName>, Option<String>), String> {
        let Some(body) = body else {
            // A Part workbench solid: a body of its own.
            return match op {
                "new_body" => Ok((Operation::NewBody, Vec::new(), Some(name.to_string()))),
                _ => Err(format!("{name}: a {op} outside a body is not rebuilt yet")),
            };
        };
        if self.bodies.contains_key(body) {
            let operation = match op {
                "join" | "new_body" => Operation::Join,
                "cut" => Operation::Cut,
                "intersect" => Operation::Intersect,
                other => return Err(format!("{name}: boolean {other} is not rebuilt yet")),
            };
            Ok((operation, vec![body.into()], None))
        } else {
            match op {
                // A body's first solid feature makes the body.
                "join" | "new_body" => {
                    self.bodies.insert(body.to_string(), ());
                    Ok((Operation::NewBody, Vec::new(), Some(body.to_string())))
                }
                _ => Err(format!("{name}: a {op} with nothing to act on")),
            }
        }
    }

    fn named(&mut self, id: u64, body: Option<String>) {
        if let (Some(body), Some(f)) = (body, self.doc.feature_mut(id)) {
            self.bodies.insert(body.clone(), ());
            f.body_names = vec![body];
        }
    }

    fn profile(&self, d: &Value, name: &str) -> Result<(u64, Plane), String> {
        let profile = d.get("profile").ok_or_else(|| format!("{name}: no profile"))?;
        match (s(profile, "kind"), s(profile, "value")) {
            (Some("sketch"), Some(id)) => self.sketches.get(id).copied().ok_or_else(|| format!("{name}: its sketch was not rebuilt")),
            _ => Err(format!("{name}: a profile that is not a sketch is not rebuilt yet")),
        }
    }

    /// `pocket`: a FreeCAD Pocket, which travels against the sketch normal cadmpeg states as its
    /// direction (the normal, reversed with `Reversed`, for Pad and Pocket alike).
    fn extrude(&mut self, d: &Value, name: &str, body: Option<&str>, pocket: bool) -> Result<u64, String> {
        let (sketch, plane) = self.profile(d, name)?;
        if d.pointer("/start/kind").and_then(Value::as_str).is_some_and(|k| k != "profile_plane") {
            return Err(format!("{name}: an extrusion that starts off its sketch plane is not rebuilt yet"));
        }
        if d.get("solid").and_then(Value::as_bool) == Some(false) {
            return Err(format!("{name}: a surface extrusion is not rebuilt yet"));
        }
        let along = match s(d.get("direction").unwrap_or(&Value::Null), "kind") {
            Some("explicit") => rotate(&self.frame, vec3(d.pointer("/direction/vector")).ok_or_else(|| format!("{name}: no direction"))?),
            Some("reversed_profile_normal") => -plane.normal(),
            _ => plane.normal(),
        };
        // Only a direction cadmpeg took from the sketch normal is turned for a Pocket; one taken
        // from an edge or given outright is the way it travels.
        let from_normal = d.pointer("/direction/source/kind").and_then(Value::as_str).is_none_or(|k| k == "profile_normal");
        let along = if pocket && from_normal { -along } else { along };
        let n = plane.normal();
        let cos = along.dot(n) / along.len().max(f64::MIN_POSITIVE);
        if cos.abs() < 1.0 - 1e-9 {
            return Err(format!("{name}: an extrusion along a slanted direction is not rebuilt yet"));
        }
        let side = |side: Option<&Value>| -> Result<(Option<f64>, bool), String> {
            let side = side.ok_or_else(|| format!("{name}: no extent"))?;
            if side.get("draft").is_some_and(|d| !d.is_null()) {
                return Err(format!("{name}: a tapered extrusion is not rebuilt yet"));
            }
            let t = side.get("termination").ok_or_else(|| format!("{name}: no extent"))?;
            match s(t, "kind") {
                Some("blind") => Ok((Some(f(t, "length").ok_or_else(|| format!("{name}: no length"))?), false)),
                Some("through_all") => Ok((None, true)),
                Some(other) => Err(format!("{name}: an extrusion {} is not rebuilt yet", other.replace('_', " "))),
                None => Err(format!("{name}: no extent")),
            }
        };
        let e = d.get("extent").ok_or_else(|| format!("{name}: no extent"))?;
        let positive = if cos > 0.0 { Direction::Positive } else { Direction::Negative };
        let (direction, (first, through), second, convention) = match s(e, "kind") {
            Some("one_sided") => (positive, side(e.get("side"))?, None, ExtrudeConvention::LegacyPerSide),
            Some("symmetric") => (Direction::Symmetric, side(e.get("side"))?, None, ExtrudeConvention::TotalDistance),
            Some("two_sided") => {
                let (second, through2) = side(e.get("second"))?;
                let first = side(e.get("first"))?;
                if through2 != first.1 {
                    return Err(format!("{name}: a two-sided extrusion through all on one side only is not rebuilt yet"));
                }
                (positive, first, if through2 { None } else { second }, ExtrudeConvention::LegacyPerSide)
            }
            _ => return Err(format!("{name}: no extent")),
        };
        // Through all both ways is a symmetric through all here.
        let direction = if through && s(e, "kind") == Some("two_sided") { Direction::Symmetric } else { direction };
        let (operation, targets, made) = self.operation(s(d, "op").unwrap_or("join"), name, body)?;
        let profiles = self.even_odd(sketch);
        let kind = FeatureKind::Extrude {
            sketch: SketchId(sketch),
            profiles,
            extent: Extent {
                distance: mm(first.unwrap_or(10.0)),
                convention,
                direction,
                distance2: second.map(mm),
                start_offset: None,
                through_all: through,
                taper: None,
                to: None,
                to_offset: None,
                from: None,
            },
            operation,
            targets,
            thin: None,
        };
        let id = self.add(kind, name)?;
        self.named(id, made);
        Ok(id)
    }

    /// A box, cylinder, cone, sphere or torus at its object's `placement` (in its body).
    fn primitive(&mut self, d: &Value, name: &str, body: Option<&str>, placement: &Mat4) -> Result<u64, String> {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        let solid = d.get("solid").ok_or_else(|| format!("{name}: no solid"))?;
        let at = apply(&self.frame, apply(placement, Vec3::ZERO));
        let x = rotate(&self.frame, rotate(placement, Vec3::X));
        let y = rotate(&self.frame, rotate(placement, Vec3::Y));
        let z = rotate(&self.frame, rotate(placement, Vec3::Z));
        let upright = near(x, Vec3::X) && near(y, Vec3::Y);
        let len = |k: &str| f(solid, k).ok_or_else(|| format!("{name}: no {k}"));
        let whole = |k: &str, full: f64| f(solid, k).is_none_or(|a| (a - full).abs() < 1e-9);
        let partial = || Err(format!("{name}: a partial or turned {} is not rebuilt yet", s(solid, "kind").unwrap_or("primitive")));
        let (op, targets, made) = self.operation(s(d, "op").unwrap_or("join"), name, body)?;
        if !targets.is_empty() && self.bodies.len() > 1 {
            return Err(format!("{name}: a primitive combined with one of several bodies is not rebuilt yet"));
        }
        let corner = solidcraft_doc::point_expr(at);
        let kind = match s(solid, "kind") {
            Some("box") if upright => {
                FeatureKind::Box { corner, length: mm(len("length")?), width: mm(len("width")?), height: mm(len("height")?), operation: op }
            }
            Some("cylinder") if whole("angle", TAU) => {
                FeatureKind::Cylinder { base: corner, axis: Dir(z), radius: mm(len("radius")?), height: mm(len("height")?), operation: op }
            }
            Some("cone") if whole("angle", TAU) => FeatureKind::Cone {
                base: corner,
                axis: Dir(z),
                radius: mm(len("radius1")?),
                top_radius: mm(len("radius2")?),
                height: mm(len("height")?),
                operation: op,
            },
            Some("sphere") if whole("latitude1", -FRAC_PI_2) && whole("latitude2", FRAC_PI_2) && whole("longitude", TAU) => {
                FeatureKind::Sphere { center: corner, radius: mm(len("radius")?), operation: op }
            }
            Some("torus") if near(z, Vec3::Z) && whole("latitude1", -PI) && whole("latitude2", PI) && whole("longitude", TAU) => {
                FeatureKind::Torus { center: corner, major: mm(len("major_radius")?), minor: mm(len("minor_radius")?), operation: op }
            }
            Some("box" | "cylinder" | "cone" | "sphere" | "torus") => return partial(),
            Some(other) => return Err(format!("{name}: a {other} primitive is not rebuilt yet")),
            None => return Err(format!("{name}: no solid")),
        };
        let id = self.add(kind, name)?;
        self.named(id, made);
        Ok(id)
    }

    /// The regions FreeCAD makes faces of: a loop inside another is a hole, one inside that
    /// a region again (even-odd). All the sketch's profiles when none is nested in another.
    fn even_odd(&self, sketch: u64) -> ProfileSel {
        let Ok(sk) = self.doc.sketch(sketch) else { return ProfileSel::All };
        let profiles = solidcraft_doc::sketch::find_profiles(sk);
        let outer_area = |p: &solidcraft_doc::sketch::Profile| p.region.outer.signed_area().abs();
        let keep: Vec<&solidcraft_doc::sketch::Profile> = profiles
            .iter()
            .filter(|p| {
                let Some(on) = p.region.outer.polyline(1e-3).first().copied() else { return true };
                let depth = profiles.iter().filter(|q| outer_area(q) > outer_area(p) && q.region.outer.contains(on)).count();
                depth % 2 == 0
            })
            .collect();
        if keep.len() == profiles.len() {
            return ProfileSel::All;
        }
        let whole = keep
            .iter()
            .map(|p| p.whole_curve_boundaries().map(|(outer, holes)| solidcraft_doc::WholeProfileBoundary { outer, holes }))
            .collect::<Option<Vec<_>>>();
        match whole {
            Some(profiles) if !profiles.is_empty() => ProfileSel::WholeBoundaries { profiles },
            _ => ProfileSel::All,
        }
    }

    /// A Mirrored, LinearPattern or PolarPattern of features: their copies, replayed.
    /// `axis`: the origin axis the pattern names, when it names one (see [`Arenas::pattern_axes`]).
    fn pattern(&mut self, d: &Value, name: &str, body: Option<&str>, axis: Option<&(Vec3, Vec3)>) -> Result<u64, String> {
        if body.is_none_or(|b| !self.bodies.contains_key(b)) {
            return Err(format!("{name}: a pattern outside a body is not rebuilt yet"));
        }
        let mut features = Vec::new();
        for seed in d.get("seeds").and_then(Value::as_array).into_iter().flatten() {
            let made = (s(seed, "kind") == Some("feature")).then(|| s(seed, "value")).flatten().and_then(|f| self.made.get(f));
            features.push(solidcraft_doc::FeatureId(*made.ok_or_else(|| format!("{name}: it copies something that was not rebuilt"))?));
        }
        if features.is_empty() {
            return Err(format!("{name}: nothing to copy"));
        }
        let p = d.get("pattern").ok_or_else(|| format!("{name}: no pattern"))?;
        let count = |k: &str| p.get(k).and_then(Value::as_u64).filter(|n| (1..=10_000).contains(n)).ok_or_else(|| format!("{name}: no count"));
        let kind = match s(p, "kind") {
            Some("mirror") => {
                let origin = apply(&self.frame, vec3(p.get("plane_origin")).ok_or_else(|| format!("{name}: no mirror plane"))?);
                let normal = rotate(&self.frame, vec3(p.get("plane_normal")).ok_or_else(|| format!("{name}: no mirror plane"))?);
                let plane = Plane::from_normal(origin, normal).ok_or_else(|| format!("{name}: a degenerate mirror plane"))?;
                FeatureKind::Mirror { features, plane: PlaneRef::Custom { plane }, bodies: Vec::new(), combine: false, placements: Vec::new() }
            }
            Some("linear") if p.get("second").is_none_or(Value::is_null) => {
                let dir = axis.map(|a| a.1).or_else(|| vec3(p.get("direction")));
                let dir = dir.map(|d| rotate(&self.frame, d)).and_then(Vec3::normalized).ok_or_else(|| format!("{name}: no direction"))?;
                FeatureKind::Pattern {
                    features,
                    pattern: solidcraft_doc::PatternKind::Rectangular {
                        dir1: Dir(dir),
                        count1: Expr::from(count("count")?.to_string().as_str()),
                        spacing1: mm(f(p, "spacing").ok_or_else(|| format!("{name}: no spacing"))?),
                        dir2: None,
                        count2: None,
                        spacing2: None,
                        alignment: Default::default(),
                        alignment2: Default::default(),
                        reversed: false,
                        reversed2: false,
                    },
                    bodies: Vec::new(),
                }
            }
            Some("circular") => {
                let (at, along) = match axis {
                    Some((at, along)) => (Some(*at), Some(*along)),
                    None => (vec3(p.get("axis_origin")), vec3(p.get("axis_dir"))),
                };
                let origin = apply(&self.frame, at.ok_or_else(|| format!("{name}: no axis"))?);
                let axis = along.map(|d| rotate(&self.frame, d)).and_then(Vec3::normalized).ok_or_else(|| format!("{name}: no axis"))?;
                FeatureKind::Pattern {
                    features,
                    pattern: solidcraft_doc::PatternKind::Circular {
                        origin: Position(origin),
                        axis: Dir(axis),
                        count: Expr::from(count("count")?.to_string().as_str()),
                        // cadmpeg's angle runs from the first copy to the last, as ours does for
                        // less than a whole turn.
                        angle: deg(f(p, "angle").ok_or_else(|| format!("{name}: no angle"))?),
                        alignment: Default::default(),
                        reversed: false,
                        axis_ref: None,
                        axis_feature: None,
                    },
                    bodies: Vec::new(),
                }
            }
            other => return Err(format!("{name}: a {} pattern is not rebuilt yet", other.unwrap_or("unknown").replace('_', " "))),
        };
        self.add(kind, name)
    }

    fn revolve(&mut self, d: &Value, name: &str, body: Option<&str>) -> Result<u64, String> {
        let c = d.get("construction").ok_or_else(|| format!("{name}: no construction"))?;
        if s(c, "state") != Some("resolved") {
            return Err(format!("{name}: its profile or axis could not be read"));
        }
        let (sketch, plane) = self.profile(c, name)?;
        let origin = apply(&self.frame, vec3(c.pointer("/axis/origin")).ok_or_else(|| format!("{name}: no axis"))?);
        let dir = vec3(c.pointer("/axis/direction"))
            .map(|d| rotate(&self.frame, d))
            .and_then(Vec3::normalized)
            .ok_or_else(|| format!("{name}: no axis"))?;
        let angle = |t: Option<&Value>| -> Result<f64, String> {
            let t = t.ok_or_else(|| format!("{name}: no angle"))?;
            match s(t, "kind") {
                Some("angle") => f(t, "angle").ok_or_else(|| format!("{name}: no angle")),
                Some(other) => Err(format!("{name}: a revolution {} is not rebuilt yet", other.replace('_', " "))),
                None => Err(format!("{name}: no angle")),
            }
        };
        let e = c.get("extent").ok_or_else(|| format!("{name}: no extent"))?;
        let (first, second, symmetric) = match s(e, "kind") {
            Some("one_sided") => (angle(e.get("termination"))?, None, false),
            Some("symmetric") => (angle(e.get("termination"))?, None, true),
            Some("two_sided") => (angle(e.get("first"))?, Some(angle(e.get("second"))?), false),
            _ => return Err(format!("{name}: no extent")),
        };
        // FreeCAD turns the profile right-handed about the axis; ours turns it along the sketch
        // normal for a positive angle, so the angles change sign when the two disagree.
        let centre = self.doc.sketch(sketch).map(|sk| plane.to_world(drawn_centre(sk))).map_err(|e| format!("{name}: {e}"))?;
        let radial = centre - origin - dir * (centre - origin).dot(dir);
        let sign = if dir.cross(radial).dot(plane.normal()) < 0.0 { -1.0 } else { 1.0 };
        let (first, second) = (first * sign, second.map(|a| a * sign));
        let (operation, targets, made) = self.operation(s(d, "op").unwrap_or("join"), name, body)?;
        let profiles = self.even_odd(sketch);
        let kind = FeatureKind::Revolve {
            sketch: SketchId(sketch),
            profiles,
            axis: AxisRef::Line { origin: Position(origin), dir: Dir(dir) },
            angle: deg(first),
            operation,
            targets,
            angle2: second.map(deg),
            symmetric,
            convention: RevolveConvention::SketchNormal,
            to: None,
        };
        let id = self.add(kind, name)?;
        self.named(id, made);
        Ok(id)
    }

    /// A fillet or chamfer: one feature per group of edges.
    fn edge_treatment(&mut self, d: &Value, name: &str, body: Option<&str>, edge_point: &EdgePoint<'_>) -> Result<u64, String> {
        let fillet = s(d, "definition") == Some("fillet");
        let body = body.filter(|b| self.bodies.contains_key(*b)).ok_or_else(|| format!("{name}: no body to act on"))?;
        let groups = d.get("groups").and_then(Value::as_array).filter(|g| !g.is_empty()).ok_or_else(|| format!("{name}: no edges"))?;
        let mut last = None;
        for (i, g) in groups.iter().enumerate() {
            let sel = g.get("edges").ok_or_else(|| format!("{name}: no edges"))?;
            if s(sel, "kind") != Some("resolved") {
                return Err(format!("{name}: its edges could not be found in the shape it rounds"));
            }
            let mut edges = Vec::new();
            for e in sel.pointer("/value/edges").and_then(Value::as_array).into_iter().flatten() {
                let at = e.as_str().and_then(edge_point).ok_or_else(|| format!("{name}: an edge it rounds could not be placed"))?;
                edges.push(Position(at));
            }
            if edges.is_empty() {
                return Err(format!("{name}: no edges"));
            }
            let kind = if fillet {
                let r = g.get("radius").ok_or_else(|| format!("{name}: no radius"))?;
                if s(r, "kind") != Some("constant") {
                    return Err(format!("{name}: a variable radius fillet is not rebuilt yet"));
                }
                FeatureKind::Fillet {
                    edges,
                    radius: mm(f(r, "radius").ok_or_else(|| format!("{name}: no radius"))?),
                    body: Some(body.into()),
                    style: Default::default(),
                    fit: Default::default(),
                    corner: Default::default(),
                }
            } else {
                let spec = g.get("spec").ok_or_else(|| format!("{name}: no size"))?;
                let size = |k: &str| f(spec, k).map(mm).ok_or_else(|| format!("{name}: no size"));
                let (distance, distance2, angle) = match s(spec, "kind") {
                    Some("distance") => (size("distance")?, None, None),
                    Some("two_distances") => (size("first")?, Some(size("second")?), None),
                    Some("distance_angle") => (size("distance")?, None, Some(deg(f(spec, "angle").ok_or_else(|| format!("{name}: no angle"))?))),
                    _ => return Err(format!("{name}: this chamfer is not rebuilt yet")),
                };
                FeatureKind::Chamfer {
                    edges,
                    distance,
                    distance2,
                    angle,
                    flip: d.get("flip_direction").and_then(Value::as_bool).unwrap_or(false),
                    body: Some(body.into()),
                }
            };
            let label = if groups.len() > 1 { format!("{name} {}", i + 1) } else { name.to_string() };
            last = Some(self.add(kind, &label)?);
        }
        last.ok_or_else(|| format!("{name}: no edges"))
    }

    fn sketch(&mut self, arenas: &Arenas, d: &Value, name: &str) -> Result<u64, String> {
        let sid = d.pointer("/sketch/sketch").and_then(Value::as_str).ok_or_else(|| format!("{name}: not a planar sketch"))?;
        let rec = arenas.sketches.get(sid).ok_or_else(|| format!("{name}: no sketch"))?;
        let frame = rec.pointer("/placement/frame").ok_or_else(|| format!("{name}: its placement could not be read"))?;
        let origin = apply(&self.frame, vec3(frame.get("origin")).ok_or_else(|| format!("{name}: no placement"))?);
        let normal = rotate(&self.frame, vec3(frame.get("normal")).ok_or_else(|| format!("{name}: no placement"))?);
        let u = rotate(&self.frame, vec3(frame.get("u_axis")).ok_or_else(|| format!("{name}: no placement"))?);
        let plane = Plane::new(origin, u, normal.cross(u)).ok_or_else(|| format!("{name}: a degenerate placement"))?;
        let plane_ref = if near_plane(&plane, &Plane::XY) { PlaneRef::Origin { name: "XY".into() } } else { PlaneRef::Custom { plane } };

        let (sketch, kept, total) = build_sketch(arenas, sid, &mut self.doc).map_err(|e| format!("{name}: {e}"))?;
        if kept < total {
            self.warnings.push(format!("{name}: {kept} of its {total} constraints came over"));
        }
        let id = self.add(FeatureKind::Sketch { plane: plane_ref, sketch }, name)?;
        self.sketches.insert(sid.to_string(), (id, plane));
        Ok(id)
    }
}

/// The middle of a sketch's drawn (not construction) curves' points.
fn drawn_centre(sk: &Sketch) -> Vec2 {
    let mut sum = Vec2::new(0.0, 0.0);
    let mut n = 0.0;
    for c in sk.curves.iter().filter(|c| !c.construction) {
        for p in c.kind.point_ids() {
            if let Some(p) = sk.points.get(p) {
                sum += p.pos;
                n += 1.0;
            }
        }
    }
    if n > 0.0 { sum * (1.0 / n) } else { sum }
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).len() < 1e-9
}

fn near_plane(a: &Plane, b: &Plane) -> bool {
    near(a.origin, b.origin) && near(a.x, b.x) && near(a.y, b.y)
}

/// One locus key: an entity and the part of it.
type Locus = (String, Role);

fn locus(v: &Value) -> Option<Locus> {
    let role = match s(v, "kind")? {
        "start" => Role::Start,
        "end" => Role::End,
        "center" => Role::Center,
        "entity" => Role::Point,
        _ => return None,
    };
    Some((s(v, "entity")?.to_string(), role))
}

/// Union-find over locus keys.
struct Merge {
    parent: Vec<usize>,
}

impl Merge {
    fn find(&mut self, mut i: usize) -> usize {
        while let Some(&p) = self.parent.get(i) {
            if p == i {
                break;
            }
            let gp = self.parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.parent.get_mut(i) {
                *slot = gp;
            }
            i = p;
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b
            && let Some(slot) = self.parent.get_mut(a.max(b))
        {
            *slot = a.min(b);
        }
    }
}

/// A FreeCAD sketch as ours: its geometry, with the constraints that keep it. Returns the sketch
/// and how many of its constraints were kept out of how many.
fn build_sketch(arenas: &Arenas, sid: &str, doc: &mut Document) -> Result<(Sketch, usize, usize), String> {
    const ROOT: &str = ":reference-root-point";
    let entities: Vec<&Value> = arenas.entities.iter().filter(|e| s(e, "sketch") == Some(sid)).collect();
    let constraints: Vec<&Value> =
        arenas.constraints.iter().filter(|c| s(c, "sketch") == Some(sid) && c.get("active").and_then(Value::as_bool) != Some(false)).collect();
    if entities.len() > MAX_SKETCH_ITEMS || constraints.len() > MAX_SKETCH_ITEMS {
        return Err("a sketch too large to rebuild".into());
    }

    // Every locus with its position; the root point is the sketch origin.
    let mut keys: Vec<(Locus, Vec2)> = Vec::new();
    let mut curves: Vec<(&str, &Value, bool)> = Vec::new();
    for e in &entities {
        let id = s(e, "id").unwrap_or_default();
        let g = e.get("geometry").ok_or("an entity without geometry")?;
        let construction = e.get("construction").and_then(Value::as_bool).unwrap_or(false);
        match s(g, "kind").unwrap_or_default() {
            "point" if id.ends_with(ROOT) => keys.push(((id.to_string(), Role::Point), Vec2::new(0.0, 0.0))),
            "point" => keys.push(((id.to_string(), Role::Point), uv(g.get("position")).ok_or("a point without a position")?)),
            "line" => {
                keys.push(((id.to_string(), Role::Start), uv(g.get("start")).ok_or("a line without ends")?));
                keys.push(((id.to_string(), Role::End), uv(g.get("end")).ok_or("a line without ends")?));
                curves.push((id, g, construction));
            }
            "circle" => {
                keys.push(((id.to_string(), Role::Center), uv(g.get("center")).ok_or("a circle without a centre")?));
                curves.push((id, g, construction));
            }
            "arc" => {
                let c = uv(g.get("center")).ok_or("an arc without a centre")?;
                let r = f(g, "radius").ok_or("an arc without a radius")?;
                let (a0, a1) = (f(g, "start_angle").ok_or("an arc without angles")?, f(g, "end_angle").ok_or("an arc without angles")?);
                keys.push(((id.to_string(), Role::Center), c));
                keys.push(((id.to_string(), Role::Start), c + Vec2::new(a0.cos(), a0.sin()) * r));
                keys.push(((id.to_string(), Role::End), c + Vec2::new(a1.cos(), a1.sin()) * r));
                curves.push((id, g, construction));
            }
            // The sketch's own axes: constraints may name them.
            "reference_line" => {}
            // Geometry projected from outside the sketch: kept out of the profile here.
            _ if id.contains(":external") => {}
            other => return Err(format!("a sketch {} is not rebuilt yet", other.replace('_', " "))),
        }
    }
    let index: HashMap<Locus, usize> = keys.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect();
    let mut merge = Merge { parent: (0..keys.len()).collect() };
    // Curve ends at one place are one point (FreeCAD joins tangent ends without a coincidence).
    let ends: Vec<usize> = keys.iter().enumerate().filter(|(_, ((_, r), _))| matches!(r, Role::Start | Role::End)).map(|(i, _)| i).collect();
    for (n, &i) in ends.iter().enumerate() {
        for &j in ends.iter().skip(n + 1) {
            if let (Some((_, a)), Some((_, b))) = (keys.get(i), keys.get(j))
                && a.dist(*b) < SAME_POINT
            {
                merge.union(i, j);
            }
        }
    }
    // Coincidences FreeCAD states.
    for c in &constraints {
        let def = c.get("definition").unwrap_or(&Value::Null);
        let loci: Vec<Locus> = match s(def, "kind") {
            Some("coincident_loci") => def.get("loci").and_then(Value::as_array).into_iter().flatten().filter_map(locus).collect(),
            Some("coincident") => def
                .get("entities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|e| Some((e.as_str()?.to_string(), Role::Point)))
                .collect(),
            _ => continue,
        };
        let found: Vec<usize> = loci.iter().filter_map(|l| index.get(l).copied()).collect();
        for w in found.windows(2) {
            if let [a, b] = *w
                && let (Some((_, pa)), Some((_, pb))) = (keys.get(a), keys.get(b))
                && pa.dist(*pb) < KEPT
            {
                merge.union(a, b);
            }
        }
    }

    // Points: one per merged group; the group holding the root point is the sketch origin.
    let mut sk = Sketch::new();
    let mut point_of: HashMap<usize, usize> = HashMap::new();
    for (i, ((id, _), _)) in keys.iter().enumerate() {
        if id.ends_with(ROOT) {
            point_of.insert(merge.find(i), 0);
        }
    }
    let mut point = |i: usize, sk: &mut Sketch| -> Result<usize, String> {
        let root = merge.find(i);
        if let Some(p) = point_of.get(&root) {
            return Ok(*p);
        }
        let at = keys.get(root).map(|(_, p)| *p).ok_or("a point out of range")?;
        let p = sk.add_point(at, None).map_err(|e| e.to_string())?;
        point_of.insert(root, p);
        Ok(p)
    };
    let mut pt: HashMap<Locus, usize> = HashMap::new();
    for (i, (k, _)) in keys.iter().enumerate() {
        pt.insert(k.clone(), point(i, &mut sk)?);
    }
    let get = |k: &Locus| pt.get(k).copied().ok_or("a point out of range");
    let mut curve_of: HashMap<String, usize> = HashMap::new();
    for (id, g, construction) in curves {
        let key = |r: Role| (id.to_string(), r);
        let kind = match s(g, "kind") {
            Some("line") => CurveKind::Line { a: get(&key(Role::Start))?, b: get(&key(Role::End))? },
            Some("circle") => CurveKind::Circle { c: get(&key(Role::Center))?, r: f(g, "radius").ok_or("a circle without a radius")? },
            _ => CurveKind::Arc { c: get(&key(Role::Center))?, a: get(&key(Role::Start))?, b: get(&key(Role::End))? },
        };
        let c = sk.add_curve(kind, None).map_err(|e| e.to_string())?;
        if let Some(curve) = sk.curves.get_mut(c) {
            curve.construction = construction;
        }
        curve_of.insert(id.to_string(), c);
    }

    // Constraints, each kept only when the sketch still solves to FreeCAD's geometry with it.
    let solved = sk.clone();
    let mut dof = solidcraft_doc::sketch::solve(&mut sk.clone()).dof;
    let total = constraints
        .iter()
        .filter(|c| !matches!(c.pointer("/definition/kind").and_then(Value::as_str), Some("coincident_loci" | "coincident")))
        .count();
    let mut kept = 0;
    let ctx = Ctx { pt: &pt, curve_of: &curve_of, sk: &solved };
    for c in &constraints {
        let def = c.get("definition").unwrap_or(&Value::Null);
        if matches!(s(def, "kind"), Some("coincident_loci" | "coincident")) {
            continue;
        }
        if c.get("driving").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let value = s(def, "parameter").and_then(|p| arenas.parameters.get(p)).and_then(|p| p.pointer("/value/value")).and_then(Value::as_f64);
        for candidate in ctx.candidates(def, value) {
            if let Some(d) = try_add(&mut sk, doc, candidate, dof, &solved) {
                dof = d;
                kept += 1;
                break;
            }
        }
    }
    Ok((sk, kept, total))
}

/// Add `k` when the sketch still solves, loses a degree of freedom, and keeps every point where
/// FreeCAD put it.
fn try_add(sk: &mut Sketch, doc: &mut Document, k: ConstraintKind, dof: usize, geometry: &Sketch) -> Option<usize> {
    let param = match k.value() {
        Some(v) => {
            let (e, unit) = if k.is_angle() { (format!("{} deg", round(v.to_degrees())), "deg") } else { (format!("{} mm", round(v)), "mm") };
            Some(doc.new_model_param(&e, unit))
        }
        None => None,
    };
    let undo = |doc: &mut Document, param: &Option<String>| {
        if let Some(p) = param {
            let _ = doc.remove_param(p);
        }
    };
    let mut trial = sk.clone();
    if trial.add_constraint(k, param.clone()).is_err() {
        undo(doc, &param);
        return None;
    }
    let rep = solidcraft_doc::sketch::solve(&mut trial);
    let moved = trial.points.iter().zip(&geometry.points).any(|(a, b)| a.pos.dist(b.pos) > KEPT);
    if rep.ok() && rep.dof < dof && !moved {
        *sk = trial;
        return Some(rep.dof);
    }
    undo(doc, &param);
    None
}

struct Ctx<'a> {
    pt: &'a HashMap<Locus, usize>,
    curve_of: &'a HashMap<String, usize>,
    sk: &'a Sketch,
}

impl Ctx<'_> {
    fn point(&self, v: Option<&Value>) -> Option<usize> {
        let l = locus(v?)?;
        self.pt.get(&l).copied().or_else(|| self.pt.get(&(l.0, Role::Point)).copied())
    }
    fn curve(&self, v: Option<&Value>) -> Option<usize> {
        self.curve_of.get(v?.as_str()?).copied()
    }
    /// A locus naming a whole line (`kind: entity` on a line).
    fn line_locus(&self, v: Option<&Value>) -> Option<usize> {
        let v = v?;
        if s(v, "kind") != Some("entity") {
            return None;
        }
        self.curve(v.get("entity"))
    }
    fn axis(v: Option<&Value>) -> Option<&'static str> {
        let id = v?.as_str()?;
        if id.ends_with(":reference-horizontal-axis") {
            Some("h")
        } else if id.ends_with(":reference-vertical-axis") {
            Some("v")
        } else {
            None
        }
    }
    fn is_line(&self, c: usize) -> bool {
        self.sk.curves.get(c).is_some_and(|c| matches!(c.kind, CurveKind::Line { .. }))
    }
    fn line_points(&self, c: usize) -> Option<(usize, usize)> {
        match self.sk.curves.get(c)?.kind {
            CurveKind::Line { a, b } => Some((a, b)),
            _ => None,
        }
    }

    /// Our constraints that may say what `def` says, most likely first.
    fn candidates(&self, def: &Value, value: Option<f64>) -> Vec<ConstraintKind> {
        use ConstraintKind::*;
        let first = |k: &str| def.get(k);
        let two = |a: Option<usize>, b: Option<usize>| a.zip(b);
        let mut out = Vec::new();
        match s(def, "kind").unwrap_or_default() {
            "horizontal" => out.extend(self.curve(first("entity")).map(|l| Horizontal { l })),
            "vertical" => out.extend(self.curve(first("entity")).map(|l| Vertical { l })),
            "parallel" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Parallel { a, b })),
            "perpendicular" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Perpendicular { a, b })),
            "tangent" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Tangent { a, b })),
            "tangent_loci" => {
                let entity = |v: Option<&Value>| self.curve(v.and_then(|v| v.get("entity")));
                out.extend(two(entity(first("first")), entity(first("second"))).map(|(a, b)| Tangent { a, b }));
            }
            "equal" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Equal { a, b })),
            "collinear" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Collinear { a, b })),
            "concentric" => out.extend(two(self.curve(first("first")), self.curve(first("second"))).map(|(a, b)| Concentric { a, b })),
            "midpoint" => out.extend(two(self.point(first("point")), self.curve(first("entity"))).map(|(p, l)| Midpoint { p, l })),
            "symmetric" => {
                if let (Some(p), Some(q)) = (self.point(first("first")), self.point(first("second"))) {
                    match (self.curve(first("axis")), Self::axis(first("axis"))) {
                        (Some(l), _) if self.is_line(l) => out.push(Symmetric { p, q, l }),
                        (_, Some("h")) => out.extend([VerticalPoints { p, q }]),
                        (_, Some("v")) => out.extend([HorizontalPoints { p, q }]),
                        _ => {}
                    }
                }
            }
            "point_on_object" => {
                if let Some(p) = self.point(first("point")) {
                    match (Self::axis(first("entity")), self.curve(first("entity"))) {
                        // On the sketch's horizontal axis: level with the origin.
                        (Some("h"), _) => out.push(HorizontalPoints { p, q: 0 }),
                        (Some("v"), _) => out.push(VerticalPoints { p, q: 0 }),
                        (_, Some(c)) => out.push(PointOnCurve { p, c }),
                        _ => {}
                    }
                }
            }
            "fixed" => {
                if let Some(l) = self.curve(first("entity")) {
                    if let Some((a, b)) = self.line_points(l) {
                        out.push(Fix { p: a });
                        out.push(Fix { p: b });
                    }
                } else if let Some(p) = first("entity").and_then(Value::as_str).and_then(|e| self.pt.get(&(e.to_string(), Role::Point)).copied()) {
                    out.push(Fix { p });
                }
            }
            "radius" => out.extend(self.curve(first("entity")).zip(value).map(|(c, value)| Radius { c, value: value.abs() })),
            "diameter" => out.extend(self.curve(first("entity")).zip(value).map(|(c, value)| Diameter { c, value: value.abs() })),
            "horizontal_distance" => {
                out.extend(two(self.point(first("first")), self.point(first("second"))).zip(value).map(|((p, q), v)| DistanceX {
                    p,
                    q,
                    value: v.abs(),
                }))
            }
            "vertical_distance" => out.extend(two(self.point(first("first")), self.point(first("second"))).zip(value).map(|((p, q), v)| DistanceY {
                p,
                q,
                value: v.abs(),
            })),
            "distance_loci" => {
                let v = value.map(f64::abs);
                match (self.line_locus(first("first")), self.line_locus(first("second"))) {
                    (Some(l), None) if self.is_line(l) => {
                        out.extend(self.point(first("second")).zip(v).map(|(p, value)| PointLineDistance { p, l, value }))
                    }
                    (None, Some(l)) if self.is_line(l) => {
                        out.extend(self.point(first("first")).zip(v).map(|(p, value)| PointLineDistance { p, l, value }))
                    }
                    _ => out.extend(two(self.point(first("first")), self.point(first("second"))).zip(v).map(|((p, q), value)| Distance {
                        p,
                        q,
                        value,
                    })),
                }
            }
            "distance" => {
                let ids: Vec<&Value> = first("entities").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
                let v = value.map(f64::abs);
                match ids.as_slice() {
                    [one] => {
                        if let (Some(l), Some(value)) = (self.curve(Some(*one)), v)
                            && self.is_line(l)
                        {
                            out.push(Length { l, value });
                        }
                    }
                    [a, b] => {
                        let point = |e: &Value| e.as_str().and_then(|e| self.pt.get(&(e.to_string(), Role::Point)).copied());
                        if let (Some(p), Some(l), Some(value)) = (point(a), self.curve(Some(*b)), v) {
                            out.push(PointLineDistance { p, l, value });
                        }
                    }
                    _ => {}
                }
            }
            "angle_to_axis" => {
                if let (Some(l), Some(v)) = (self.curve(first("entity")), value) {
                    let base = if s(def, "axis") == Some("vertical") { v + std::f64::consts::FRAC_PI_2 } else { v };
                    for a in [base, -base, std::f64::consts::PI - base, base - std::f64::consts::PI] {
                        out.push(LineAngle { l, value: a });
                    }
                }
            }
            "angle" => {
                if let (Some(a), Some(b), Some(v)) = (self.curve(first("first")), self.curve(first("second")), value)
                    && self.is_line(a)
                    && self.is_line(b)
                {
                    for (value, flip) in [(v, false), (v, true), (-v, false), (-v, true)] {
                        out.push(Angle { a, b, value: value.abs(), flip });
                        if value < 0.0 {
                            out.push(Angle { a: b, b: a, value: value.abs(), flip });
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }
}

/// Bodies match when each rebuilt body has the volume, extent and centroid of one of the file's
/// solids.
/// Returns why they do not.
pub(super) fn check_against(rebuilt: &[solidcraft_doc::kernel::Body], file: &[solidcraft_doc::kernel::Body]) -> Result<(), String> {
    if rebuilt.len() != file.len() {
        return Err(format!("it rebuilds {} solids where the file has {}", rebuilt.len(), file.len()));
    }
    let key = |b: &solidcraft_doc::kernel::Body| -> Result<(f64, Vec3, Vec3, Vec3), String> {
        let m = solidcraft_doc::kernel::measure(b).map_err(|e| e.to_string())?;
        Ok((m.volume, m.bbox.min, m.bbox.max, m.centroid))
    };
    let mut theirs: Vec<Option<(f64, Vec3, Vec3, Vec3)>> = file.iter().map(|b| key(b).ok()).collect();
    for b in rebuilt {
        let (v, lo, hi, c) = key(b)?;
        let size = (hi - lo).len().max(1.0);
        let close = |a: Vec3, b: Vec3| (a - b).len() < 1e-4 * size;
        let found = theirs.iter_mut().find(|t| {
            t.as_ref()
                .is_some_and(|(tv, tlo, thi, tc)| (v - tv).abs() <= 1e-4 * tv.abs().max(1.0) && close(lo, *tlo) && close(hi, *thi) && close(c, *tc))
        });
        match found {
            Some(slot) => *slot = None,
            None => return Err(format!("a rebuilt solid (volume {:.3} mm³) matches none of the file's", v)),
        }
    }
    Ok(())
}

/// Bodies the document builds, by name, in its order.
pub(super) fn evaluate(doc: &Document) -> Result<Vec<solidcraft_doc::kernel::Body>, String> {
    let mut model = solidcraft_doc::Model::new();
    model.evaluate(doc);
    if let Some(r) = model.results.iter().find(|r| r.error.is_some()) {
        return Err(format!("{}: {}", r.name, r.error.clone().unwrap_or_default()));
    }
    Ok(model.state().bodies.iter().map(|b| b.body.clone()).collect())
}
