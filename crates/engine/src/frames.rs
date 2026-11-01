//! Component frames for picks. The viewport shows the placed model (`Session::world_state`):
//! a component's bodies and sketches moved by its occurrence (and pasted instances moved by
//! theirs). Features are made and kept in their component's own frame. So a command's picked
//! points and directions, given in the world, are mapped into the frame of the body they lie on
//! (through the inverse of that body's placement), and points that create geometry into the
//! active component's frame, before the command runs. Instance body names ("Body1 (Occ:2)")
//! become the component's body name.
//!
//! Nothing changes while every placement is the identity (the common case, and every design
//! without moved components).

use serde_json::{Map, Value};
use solidcraft_doc::{Mat, apply_point, apply_vector, is_identity, map_point_expr, map_vector_expr, mat_inverse};
use solidcraft_geom::Vec3;

use crate::Session;

/// How a parameter is mapped.
#[derive(Clone, Copy, PartialEq)]
enum K {
    /// A point on a body (a pick): into that body's frame.
    Pick,
    /// A list of picks: `[[x,y,z]…]` or `[{point|face|edge: [x,y,z], body?}…]`.
    Picks,
    /// A point that places new geometry: into the active component's frame.
    Make,
    /// A direction: turned with the command's frame.
    Dir,
    /// A displacement with length expressions: turned without the frame's translation.
    Delta,
    /// A point the command's bodies move about: into the frame of those bodies (the command's
    /// frame, as its directions are).
    Moved,
    /// An arbitrary axis object in the authoring frame: {origin, dir}.
    Axis,
    /// A plane object (`{face}`, `{origin, normal}`, `{origin, x_dir, y_dir}`) or a plane name.
    Plane,
    /// A body name, or a list of them.
    Body,
    /// A point (or a list of picks) for the sketch being made or edited: into that sketch's
    /// component frame, whichever component the geometry under it belongs to (a sketch in the
    /// root on a moved component's face finds the face where it is shown).
    Here,
    /// A plane object for a new sketch or canvas (or a mirror's copies): into the active
    /// component's frame.
    HerePlane,
}

use K::*;

/// The parameters each command takes in world coordinates.
fn table(id: &str) -> &'static [(&'static str, K)] {
    match id {
        "solid.extrude" => {
            &[("face", Pick), ("faces", Picks), ("face_body", Body), ("from", Make), ("to", Make), ("targets", Body), ("participants", Body)]
        }
        "solid.revolve" => &[("face", Pick), ("axis", Axis), ("to", Make), ("targets", Body), ("participants", Body)],
        "solid.box" | "solid.sphere" => &[("corner", Make), ("center", Make)],
        "solid.cylinder" | "solid.coil" | "solid.torus" | "solid.cone" => &[("base", Make), ("center", Make), ("axis", Dir)],
        "solid.fillet" | "solid.chamfer" => &[("edges", Picks), ("body", Body)],
        "solid.hole" => &[("position", Pick), ("direction", Dir), ("body", Body)],
        "solid.scale" => &[("origin", Make), ("bodies", Body)],
        "solid.offset_face" | "solid.shell" => &[("faces", Picks), ("body", Body)],
        "solid.thread" => &[("face", Pick)],
        "solid.emboss" => &[("face", Make)],
        "solid.draft" => &[("faces", Picks), ("neutral", Plane), ("pull", Dir), ("body", Body)],
        "solid.press_pull" => &[("face", Pick), ("edges", Picks)],
        "solid.replace_face" => &[("faces", Picks), ("target_face", Pick), ("target", Plane), ("body", Body)],
        // Align finds its picks where they are shown and takes both ends into the moved bodies'
        // frame itself: a pick mapped into the frame of the body under it would put the two ends
        // in different frames when they lie on separately placed components.
        "solid.align" => &[("bodies", Body)],
        // A mirror keeps the placement its bodies were picked at (`cmd::features::placed_bodies`);
        // its plane is in the active component, where the copies go.
        "solid.mirror" => &[("plane", HerePlane)],
        "solid.split_body" => &[("plane", Plane), ("bodies", Body), ("body", Body)],
        "plastic.boss" | "plastic.snap_fit" | "plastic.rest" => {
            &[("position", Pick), ("direction", Dir), ("hook", Dir), ("along", Dir), ("body", Body)]
        }
        "plastic.lip" => &[("face", Pick), ("body", Body)],
        "sheet.convert" => &[("face", Pick), ("bends", Picks), ("body", Body)],
        "sheet.flange" | "sheet.hem" => &[("edges", Picks), ("body", Body)],
        "sheet.fold" => &[("points", Picks), ("fixed", Pick), ("body", Body)],
        "sheet.path_flange" => &[("edge", Pick), ("body", Body)],
        "sheet.perforate" => &[("body", Body)],
        "sketch.create" | "sketch.redefine" => &[("plane", HerePlane)],
        "canvas.decal" => &[("face", Here)],
        "canvas.insert" => &[("plane", HerePlane), ("at", Here)],
        "sketch.project" | "sketch.intersect" | "sketch.include_3d" => &[("refs", Here), ("ref", Here)],
        "sketch.project_to_surface" | "sketch.isoparametric_curve" => &[("face", Here)],
        "sketch.intersection_curve" => &[("a", Here), ("b", Here)],
        "inspect.curvature_comb" | "inspect.minimum_radius" | "inspect.isocurve" => &[("edges", Picks), ("faces", Picks)],
        // A fastener's `at` picks locate host occurrences and feed joint.create in world
        // coordinates. Only free placement belongs in the active component's frame.
        "parts.insert" | "parts.fastener" => &[("point", Make), ("direction", Dir)],
        "appearance.assign" => &[("faces", Picks), ("bodies", Body), ("body", Body)],
        "solid.move" => &[("bodies", Body), ("translate", Delta), ("axis", Dir), ("origin", Moved)],
        "solid.combine" => &[("target", Body), ("tools", Body)],
        "solid.remove" | "solid.bounding_solid" | "material.assign" => &[("bodies", Body)],
        _ => &[],
    }
}

/// The world placement of bodies (name → frame), with moves not captured yet.
struct Frames {
    /// Worked out on first use: most commands pick no body, and a large assembly's frames cost
    /// a walk of every body on every command.
    bodies: std::cell::OnceCell<crate::BodyFrames>,
    active: Mat,
    /// The frame of the sketch being edited (its component), else the active component's.
    sketch: Mat,
}

fn frames(s: &Session) -> Frames {
    let sketch_comp = s.active_sketch.and_then(|id| s.doc.feature(id)).map(|f| f.component).unwrap_or(s.active_component);
    let active = s.active_component_frame();
    let sketch = if sketch_comp == s.active_component { active } else { s.placed_document().component_transform(sketch_comp) };
    Frames { bodies: std::cell::OnceCell::new(), active, sketch }
}

impl Frames {
    /// World name, the component's own body name, and its frame, for every body.
    fn bodies(&self, s: &Session) -> &[(String, String, Mat)] {
        self.bodies.get_or_init(|| s.body_frames())
    }
}

/// Does world geometry or its instance naming differ from source components?
pub fn placed(s: &Session) -> bool {
    let mut components = std::collections::BTreeSet::new();
    !s.pending_moves.is_empty()
        || !s.doc.body_offsets.is_empty()
        || s.doc.occurrences.iter().any(|o| !is_identity(&o.transform) || !components.insert(o.component))
}

fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    Some(Vec3::new(c(0)?, c(1)?, c(2)?))
}

fn pt(p: Vec3) -> Value {
    serde_json::json!([p.x, p.y, p.z])
}

/// The frame of the body a world point lies on (the nearest), or of the named body, with the
/// bodies whose faces meet there: the first is the body under the point, and more than one
/// means the pick is ambiguous.
fn frame_at(s: &Session, f: &Frames, p: Vec3, body: Option<&str>) -> Option<(Mat, Vec<String>)> {
    if let Some(b) = body
        && let Some((_, own, m)) = f.bodies(s).iter().find(|(w, _, _)| w == b)
    {
        return Some((*m, vec![own.clone()]));
    }
    let world = s.world_state();
    let mut best: Option<(f64, Mat, Vec<String>)> = None;
    for wb in &world.bodies {
        let Some((_, own, m)) = f.bodies(s).iter().find(|(w, _, _)| *w == wb.name) else { continue };
        let mesh = wb.mesh();
        let bb = mesh.bounds();
        let tol = (bb.diagonal() * 1e-3).max(1e-3);
        if p.x < bb.min.x - tol
            || p.y < bb.min.y - tol
            || p.z < bb.min.z - tol
            || p.x > bb.max.x + tol
            || p.y > bb.max.y + tol
            || p.z > bb.max.z + tol
        {
            continue;
        }
        let d = mesh.triangles.iter().filter_map(|t| mesh.tri(t)).map(|[a, b, c]| tri_dist(p, a, b, c)).fold(f64::INFINITY, f64::min);
        if d > tol {
            continue;
        }
        // Bodies whose faces are as near the point as the nearest one meet there: two of them
        // make the pick ambiguous. The same body placed twice is one body, not a tie.
        let as_near = best.as_ref().is_some_and(|(bd, _, _)| (d - *bd).abs() <= 1e-8);
        if as_near {
            if let Some((_, _, names)) = &mut best
                && !names.contains(own)
            {
                names.push(own.clone());
            }
        } else if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
            best = Some((d, *m, vec![own.clone()]));
        }
    }
    best.map(|(_, m, names)| (m, names))
}

fn tri_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return p.dist_to_segment(a, b).min(p.dist_to_segment(b, c)) };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

struct Mapper<'a> {
    s: &'a Session,
    f: Frames,
    /// The frame of the command's first pick (its directions turn with it).
    first: Option<Mat>,
    changed: bool,
    /// Picks go into the sketch's frame (`Here`), not the frame of the body under them.
    here: bool,
}

impl Mapper<'_> {
    fn pick(&mut self, v: &Value, body: Option<&str>) -> Option<Value> {
        let p = vec3(v)?;
        let under = if self.here { None } else { frame_at(self.s, &self.f, p, body) };
        let m = if self.here { self.f.sketch } else { under.as_ref().map(|(m, _)| *m).unwrap_or(self.f.active) };
        self.first.get_or_insert(m);
        let out = self.local_point(&m, p);
        // The command sees the pick in the frame of the body under it. Remember the bodies
        // whose faces meet there, so a face of that body is found and a body that only
        // shares its place in the model does not make the pick ambiguous (#419).
        if let Some((_, names)) = &under {
            remember(out.as_ref().and_then(vec3).unwrap_or(p), names.clone());
        }
        out
    }

    fn local_point(&mut self, m: &Mat, p: Vec3) -> Option<Value> {
        if is_identity(m) {
            return None;
        }
        let inv = mat_inverse(m)?;
        self.changed = true;
        Some(pt(apply_point(&inv, p)))
    }

    fn make(&mut self, v: &Value) -> Option<Value> {
        let m = self.f.active;
        let Some(p) = vec3(v) else { return self.map_expr(&m, v, map_point_expr) };
        self.local_point(&m, p)
    }

    /// Coordinates such as `["w / 2", 0, 0]`, mapped without evaluating their expressions.
    fn map_expr(&mut self, m: &Mat, v: &Value, map: fn(&Mat, &mut solidcraft_doc::PointExpr)) -> Option<Value> {
        let a = v.as_array().filter(|a| a.len() == 3)?;
        let c = |i: usize| match a.get(i)? {
            Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(|x| solidcraft_doc::Expr::new(format!("{x}"))),
            Value::String(e) => Some(solidcraft_doc::Expr::new(e.as_str())),
            _ => None,
        };
        let mut p = [c(0)?, c(1)?, c(2)?];
        if is_identity(m) {
            return None;
        }
        let inv = mat_inverse(m)?;
        map(&inv, &mut p);
        self.changed = true;
        Some(serde_json::json!(p))
    }

    fn dir(&mut self, v: &Value) -> Option<Value> {
        let d = vec3(v)?;
        let m = self.first.unwrap_or(self.f.active);
        if is_identity(&m) {
            return None;
        }
        let inv = mat_inverse(&m)?;
        self.changed = true;
        Some(pt(apply_vector(&inv, d)))
    }

    /// A list of picks (points, or objects holding one).
    fn picks(&mut self, v: &Value, body: Option<&str>) -> Option<Value> {
        match v {
            Value::Array(a) if vec3(v).is_some() && a.iter().all(Value::is_number) => self.pick(v, body),
            Value::Array(a) => {
                let out: Vec<Value> = a.iter().map(|x| self.picks(x, body).unwrap_or_else(|| x.clone())).collect();
                Some(Value::Array(out))
            }
            Value::Object(o) => {
                let b = o.get("body").and_then(Value::as_str).map(str::to_string);
                let mut m = o.clone();
                for k in ["point", "face", "edge", "vertex", "at"] {
                    if let Some(x) = o.get(k)
                        && let Some(y) = self.pick(x, b.as_deref().or(body))
                    {
                        m.insert(k.into(), y);
                    }
                }
                if let Some(bn) = &b
                    && let Some(own) = self.own_name(bn)
                {
                    m.insert("body".into(), Value::String(own));
                }
                Some(Value::Object(m))
            }
            _ => None,
        }
    }

    fn plane(&mut self, v: &Value) -> Option<Value> {
        let o = v.as_object()?;
        let mut m = o.clone();
        if let Some(x) = o.get("face").and_then(|x| self.pick(x, None)) {
            m.insert("face".into(), x);
        }
        if let Some(x) = o.get("origin").and_then(|x| self.pick(x, None)) {
            m.insert("origin".into(), x);
        }
        for k in ["normal", "x_dir", "y_dir"] {
            if let Some(x) = o.get(k).and_then(|x| self.dir(x)) {
                m.insert(k.into(), x);
            }
        }
        Some(Value::Object(m))
    }

    /// The component's body name for a world (instance) body name.
    fn own_name(&mut self, world: &str) -> Option<String> {
        let own = self.f.bodies(self.s).iter().find(|(w, _, _)| w == world).map(|(_, own, _)| own.clone())?;
        (own != world).then(|| {
            self.changed = true;
            own
        })
    }

    fn body(&mut self, v: &Value) -> Option<Value> {
        match v {
            Value::String(n) => self.own_name(n).map(Value::String),
            Value::Array(a) => {
                let out: Vec<Value> = a.iter().map(|x| self.body(x).unwrap_or_else(|| x.clone())).collect();
                Some(Value::Array(out))
            }
            _ => None,
        }
    }
}

thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// The outermost command's coordinates were mapped here (its table lists them).
    static MAPPED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The picks of the running command: the point it sees and the bodies whose faces meet
    /// there (the first is the body under it; more than one means the pick is ambiguous).
    static PICKS: std::cell::RefCell<Vec<(Vec3, Vec<String>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// How many picks of one command are remembered (a command takes few).
const MAX_PICKS: usize = 4096;
/// How many bodies may meet at one pick (an ambiguous pick names them).
const MAX_PICK_BODIES: usize = 64;

/// Remember the bodies whose faces meet at a pick, against the point the command sees.
fn remember(point: Vec3, names: Vec<String>) {
    let names: Vec<String> = names.into_iter().take(MAX_PICK_BODIES).collect();
    PICKS.with(|picks| {
        let mut picks = picks.borrow_mut();
        if picks.len() < MAX_PICKS {
            picks.push((point, names));
        }
    });
}

/// The bodies whose faces meet at `point`, when the running command picked it: the first is
/// the body under the pick, and more than one means the pick is ambiguous.
pub(crate) fn picked(point: Vec3) -> Option<Vec<String>> {
    PICKS.with(|picks| picks.borrow().iter().find(|(p, _)| *p == point).map(|(_, names)| names.clone()))
}

/// Forget the picks of the last command: a new one starts with none.
pub(crate) fn forget_picks() {
    PICKS.with(|picks| picks.borrow_mut().clear());
}

/// Did this module map the running command's coordinates? (Otherwise a new feature's geometry
/// goes into the active component's frame as a whole.)
pub fn mapped() -> bool {
    MAPPED.with(std::cell::Cell::get)
}

/// Author generated geometry directly in the active component's local frame.
/// Restores the enclosing command's mapping state, including on an error.
pub(crate) fn local_geometry() -> LocalGeometry {
    LocalGeometry(MAPPED.with(|m| m.replace(true)))
}

/// Steps out of the enclosing command's scope so every command run inside it behaves as the
/// outermost one: its parameters are mapped from the world the way a command run from the UI or
/// from a script has them. `macro.play` uses it to replay recorded commands exactly as they ran.
pub struct TopLevel {
    depth: u32,
    mapped: bool,
}

impl TopLevel {
    pub fn enter() -> Self {
        let depth = DEPTH.with(|d| {
            let n = d.get();
            d.set(0);
            n
        });
        let mapped = MAPPED.with(|m| m.replace(false));
        TopLevel { depth, mapped }
    }
}

impl Drop for TopLevel {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(self.depth));
        MAPPED.with(|m| m.set(self.mapped));
    }
}

pub(crate) struct LocalGeometry(bool);

impl Drop for LocalGeometry {
    fn drop(&mut self) {
        MAPPED.with(|m| m.set(self.0));
    }
}

/// A command running inside another (its parameters are already in component frames).
pub struct Nested;

impl Nested {
    /// Enter command `id`; true when it is the outermost one.
    pub fn enter(id: &str) -> (Nested, bool) {
        let outer = DEPTH.with(|d| {
            let n = d.get();
            d.set(n + 1);
            n == 0
        });
        if outer {
            let inner = if id == "timeline.redefine" { None } else { Some(id) };
            MAPPED.with(|m| m.set(inner.is_none_or(|i| !table(i).is_empty() || i == "construct.plane.axis_point")));
        }
        (Nested, outer)
    }
}

impl Drop for Nested {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// `params` with world picks mapped into component frames (`None`: nothing to change).
pub fn to_local(s: &Session, id: &str, params: &Value) -> Option<Value> {
    if id == "library.apply" {
        // Library targets are the same world selections as appearance.assign,
        // nested one level down. Convert at the outer boundary exactly once;
        // run_nested subsequently receives the component-local picks and names.
        let targets = to_local(s, "appearance.assign", params.get("targets")?)?;
        let mut out = params.clone();
        out["targets"] = targets;
        return Some(out);
    }
    // Several placements of one part moved together are moved as placements, in the world
    // (`placements_moved`): their names and vectors stay as given.
    if id == "solid.move" && placements_moved(s, params).is_some() {
        return None;
    }
    if id == "timeline.redefine" {
        let cmd = params.get("command")?.as_str()?;
        let key = params.get("feature").and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|id| id.to_string())))?;
        let feature = s.doc.find_feature(&key)?;
        // Edit Feature authors in the existing feature's frame, even when another
        // component is active. Body-relative picks still use the picked body's frame.
        let mut context = s.scratch();
        context.active_component = feature.component;
        if let solidcraft_doc::FeatureKind::Emboss { face: Some(face), .. } = &feature.kind {
            context.active_occurrence_path = (!face.author_path.is_empty()).then(|| face.author_path.clone());
        }
        let mut inner = to_local(&context, cmd, params.get("params")?)?;
        // An untouched explicit axis is displayed in world coordinates by Edit
        // Feature. Keep its exact saved values instead of an affine round trip.
        if cmd == "solid.revolve"
            && let solidcraft_doc::FeatureKind::Revolve { axis: solidcraft_doc::AxisRef::Line { origin, dir }, .. } = &feature.kind
        {
            let frame = context.active_component_frame();
            let displayed = serde_json::json!({"origin":pt(apply_point(&frame,origin.0)),"dir":pt(apply_vector(&frame,dir.0))});
            if params.get("params").and_then(|p| p.get("axis")) == Some(&displayed) {
                inner["axis"] = serde_json::json!({"origin":pt(origin.0),"dir":pt(dir.0)});
            }
        }
        let points = match (&feature.kind, cmd) {
            (solidcraft_doc::FeatureKind::Revolve { to, .. }, "solid.revolve") => [("to", *to), ("from", None)],
            (solidcraft_doc::FeatureKind::Extrude { extent, .. }, "solid.extrude") => [("to", extent.to), ("from", extent.from)],
            _ => [("to", None), ("from", None)],
        };
        for (key, point) in points {
            if let Some(point) = point {
                let displayed = pt(apply_point(&context.active_component_frame(), point.0));
                if params.get("params").and_then(|p| p.get(key)) == Some(&displayed) {
                    inner[key] = pt(point.0);
                }
            }
        }
        // Edit Feature passes a primitive's placement through as saved (it can hold
        // expressions), as `timeline.inspect` reports it: mapping it again would move a
        // primitive in a moved component (#2926).
        for (key, saved) in primitive_placement(&feature.kind, cmd) {
            if params.get("params").and_then(|p| p.get(key)).is_some_and(|given| same_coordinates(given, &saved)) {
                inner[key] = saved;
            }
        }
        let mut out = params.clone();
        out["params"] = inner;
        return Some(out);
    }
    // Exact face capture retains the original world pick and occurrence identity; an explicit
    // plane with `frame: "component"` is already in the component's own coordinates (#6000).
    if matches!(id, "sketch.create" | "sketch.redefine")
        && (params.get("plane").and_then(|plane| plane.get("face")).is_some() || params.get("frame").and_then(Value::as_str) == Some("component"))
    {
        return None;
    }
    if id == "construct.plane.axis_point" && placed(s) {
        // Construction references include tagged saved forms as well as pick wrappers.
        // Map the parsed definition once at the boundary, including a nested Edit Feature.
        // The constructor can then name local model picks and retain their exact references.
        let mut kind = crate::cmd::plane_axis_point_kind(s, params).ok()?;
        kind.transform_geometry(&mat_inverse(&s.active_component_frame())?);
        let solidcraft_doc::FeatureKind::ConstructionPlane { plane: solidcraft_doc::PlaneRef::AxisPoint { axis, point } } = kind else {
            return None;
        };
        let mut out = params.clone();
        out["axis"] = serde_json::to_value(axis).ok()?;
        out["point"] = serde_json::to_value(point).ok()?;
        return Some(out);
    }
    let keys = table(id);
    if keys.is_empty() || !placed(s) {
        return None;
    }
    let obj = params.as_object()?;
    let mut command_frames = frames(s);
    if id == "sketch.redefine" {
        let key = params.get("sketch").and_then(|value| value.as_str().map(str::to_string).or_else(|| value.as_u64().map(|id| id.to_string())))?;
        let feature = s.doc.find_feature(&key)?;
        let path = crate::cmd::author_path_for_rebind(s, feature, params, id).ok()?;
        let frame = solidcraft_doc::EmbossFace::component_frame(&s.placed_document(), feature.component, &path).ok()?;
        command_frames.active = frame;
        command_frames.sketch = frame;
    }
    // A move has no picks: its vector, axis and pivot are in the world and turn with the frame
    // of the bodies it moves (an imported part stood upright moved up goes up, not along its
    // own frame's z). A turn with no pivot is about the world origin.
    let first = (id == "solid.move")
        .then(|| obj.get("bodies").and_then(Value::as_array)?.first()?.as_str())
        .flatten()
        .and_then(|b| command_frames.bodies(s).iter().find(|(world, own, _)| world == b || own == b).map(|(.., m)| *m));
    let mut obj = obj.clone();
    if first.is_some() && obj.contains_key("axis") && !obj.contains_key("origin") {
        obj.insert("origin".into(), serde_json::json!([0.0, 0.0, 0.0]));
    }
    let obj = &obj;
    let mut m = Mapper { s, f: command_frames, first, changed: false, here: false };
    let body = obj.get("body").or_else(|| obj.get("face_body")).and_then(Value::as_str).map(str::to_string);
    let mut out: Map<String, Value> = obj.clone();
    // Picks first: their frame turns the directions.
    for pass in [true, false] {
        for (k, kind) in keys {
            let Some(v) = obj.get(*k) else { continue };
            let is_pick = matches!(kind, Pick | Picks | Plane | Here | HerePlane);
            if is_pick != pass {
                continue;
            }
            let new = match kind {
                Pick => m.pick(v, body.as_deref()),
                Picks => m.picks(v, body.as_deref()),
                Make => m.make(v),
                Dir => m.dir(v),
                Delta => m.dir(v).or_else(|| {
                    let frame = m.first.unwrap_or(m.f.active);
                    m.map_expr(&frame, v, map_vector_expr)
                }),
                Moved => {
                    let frame = m.first.unwrap_or(m.f.active);
                    vec3(v).and_then(|p| m.local_point(&frame, p))
                }
                Axis => {
                    if let Some(axis) = v.as_object() {
                        let mut axis = axis.clone();
                        if let Some(origin) = axis.get("origin").and_then(|o| m.make(o)) {
                            axis.insert("origin".into(), origin);
                        }
                        if let Some(dir) = axis.get("dir").and_then(vec3)
                            && !is_identity(&m.f.active)
                            && let Some(inv) = mat_inverse(&m.f.active)
                        {
                            axis.insert("dir".into(), pt(apply_vector(&inv, dir)));
                            m.changed = true;
                        }
                        Some(Value::Object(axis))
                    } else {
                        None
                    }
                }
                Plane => m.plane(v),
                Body => m.body(v),
                Here | HerePlane => {
                    // A new sketch or canvas goes into the active component.
                    let keep = m.f.sketch;
                    if *kind == HerePlane {
                        m.f.sketch = m.f.active;
                    }
                    m.here = true;
                    // Body names stay: the sketch's view of the model keeps them.
                    let r = if *kind == HerePlane { m.plane(v) } else { m.picks(v, None) };
                    m.here = false;
                    m.f.sketch = keep;
                    r
                }
            };
            if let Some(n) = new {
                out.insert((*k).to_string(), n);
            }
        }
    }
    m.changed.then_some(Value::Object(out))
}

/// A primitive's saved placement inputs, in its component's frame, for redefining it with `cmd`.
fn primitive_placement(kind: &solidcraft_doc::FeatureKind, cmd: &str) -> Vec<(&'static str, Value)> {
    use solidcraft_doc::FeatureKind as Feature;
    let point = |p: &solidcraft_doc::PointExpr| {
        Value::Array(
            p.iter()
                .map(|e| e.as_str().trim().parse::<f64>().ok().filter(|x| x.is_finite()).map_or_else(|| Value::from(e.as_str()), Value::from))
                .collect(),
        )
    };
    match (kind, cmd) {
        (Feature::Box { corner, .. }, "solid.box") => vec![("corner", point(corner))],
        (Feature::Sphere { center, .. }, "solid.sphere") | (Feature::Torus { center, .. }, "solid.torus") => vec![("center", point(center))],
        (Feature::Cylinder { base, axis, .. }, "solid.cylinder") | (Feature::Coil { base, axis, .. }, "solid.coil") => {
            vec![("base", point(base)), ("axis", pt(axis.0))]
        }
        _ => Vec::new(),
    }
}

/// Do two coordinate lists hold the same values (`0` and `0.0` and `"0"` alike, expressions
/// by their text)?
fn same_coordinates(a: &Value, b: &Value) -> bool {
    let number = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()));
    match (a.as_array(), b.as_array()) {
        (Some(a), Some(b)) => {
            a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| match (number(x), number(y)) {
                    (Some(x), Some(y)) => x == y,
                    _ => x.as_str().map(str::trim).is_some_and(|x| Some(x) == y.as_str().map(str::trim)),
                })
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;

/// What a `solid.move` of the placed model moves as placements (#4768). A part placed more than
/// once (an imported assembly's screws or fan corners) is one body: moving it once per placement
/// listed moved it that many times, each placement turning the move its own way, so the copies
/// flew apart. When `bodies` name one part through two or more placements, each listed body's
/// placement is moved instead, in the world, as `occurrence.move` moves it. A placement held by
/// a rigid group (an imported assembly) moves with the outermost placement that holds it, and one
/// inside another placement being moved is not moved again. Bodies of the root component, which
/// have no placement, are moved as bodies (`bodies`). `None`: an ordinary move of bodies (no part
/// named through two placements, or a copy).
pub(crate) struct PlacementMove {
    /// Occurrence paths (root first) to move.
    pub paths: Vec<Vec<u64>>,
    /// Root-component bodies listed, moved as bodies.
    pub bodies: Vec<String>,
}

pub(crate) fn placements_moved(s: &Session, params: &Value) -> Option<PlacementMove> {
    if params.get("copy").and_then(Value::as_bool) == Some(true) || !placed(s) {
        return None;
    }
    let names: Vec<&str> = params.get("bodies")?.as_array()?.iter().filter_map(Value::as_str).collect();
    let st = s.model.state();
    let doc = s.placed_document();
    let frames = doc.body_frames_with_source(&st);
    let listed: Vec<&(String, String, u64, Vec<u64>, Mat)> = names.iter().filter_map(|n| frames.iter().find(|f| f.0 == *n)).collect();
    // One part (a component's body) named through two different placements.
    let repeated = listed.iter().enumerate().any(|(i, a)| listed[..i].iter().any(|b| b.1 == a.1 && b.2 == a.2 && b.3 != a.3));
    if !repeated {
        return None;
    }
    let mut paths: Vec<Vec<u64>> = Vec::new();
    let mut bodies = Vec::new();
    for (world, _, _, path, _) in listed {
        if path.is_empty() {
            bodies.push(world.clone());
            continue;
        }
        // Up through the rigid groups that hold it (as `occurrence.move` asks for).
        let mut k = path.len();
        while k > 1 && path.get(k - 1).is_some_and(|o| s.held_by_rigid_group(*o).is_some()) {
            k -= 1;
        }
        let target = path[..k].to_vec();
        if !paths.contains(&target) {
            paths.push(target);
        }
    }
    // A placement inside another one being moved goes with it.
    let all = paths.clone();
    paths.retain(|p| !all.iter().any(|q| q.len() < p.len() && p.starts_with(q)));
    Some(PlacementMove { paths, bodies })
}
