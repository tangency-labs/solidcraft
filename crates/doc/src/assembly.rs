//! Components and occurrences: the design as a tree.
//!
//! The root component (id 0) is the design. Every other component lives in a parent component
//! through one or more occurrences, each with a rigid transform (instances share the component's
//! contents; a "Paste New" copy is a separate component). A component's sketches, construction
//! geometry and features are authored in its own frame (its origin); the model is evaluated in
//! those frames and placed in the world through the occurrence transforms when bodies are
//! gathered for display, measuring and export (`world_bodies`). Commands take world coordinates
//! and the engine maps a new feature's geometry into the active component's frame.

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Plane, Vec3};

use crate::{DocError, Document, FeatureKind, PlaneRef, Result};

/// Column-major 4×4 affine transform (`m[col][row]`).
pub type Mat = [[f64; 4]; 4];

pub const IDENTITY: Mat = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

pub fn mat_mul(a: &Mat, b: &Mat) -> Mat {
    let mut r = [[0.0; 4]; 4];
    for (c, col) in r.iter_mut().enumerate() {
        for (row, cell) in col.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

pub fn apply_point(m: &Mat, p: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
        m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
        m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
    )
}

pub fn apply_vector(m: &Mat, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
        m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
        m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
    )
}

pub fn is_identity(m: &Mat) -> bool {
    m.iter().flatten().zip(IDENTITY.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-12)
}

/// Inverse of an affine transform.
pub fn mat_inverse(m: &Mat) -> Option<Mat> {
    let a = |r: usize, c: usize| m[c][r];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    if det.abs() < 1e-14 || !det.is_finite() {
        return None;
    }
    let inv = |r: usize, c: usize| -> f64 {
        // Cofactor of (c, r) / det.
        let (r0, r1) = match c {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let (c0, c1) = match r {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let minor = a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
        let sign = if (r + c).is_multiple_of(2) { 1.0 } else { -1.0 };
        sign * minor / det
    };
    let mut out = IDENTITY;
    for (c, col) in out.iter_mut().enumerate().take(3) {
        for (r, cell) in col.iter_mut().enumerate().take(3) {
            *cell = inv(r, c);
        }
    }
    let t = Vec3::new(m[3][0], m[3][1], m[3][2]);
    let it = apply_vector(&out, t);
    out[3] = [-it.x, -it.y, -it.z, 1.0];
    Some(out)
}

/// A rigid transform: rotation by `angle` about `axis` through `origin`, then a translation.
pub fn rigid(translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Mat {
    let k = axis.normalized().unwrap_or(Vec3::Z);
    let (s, c) = angle.sin_cos();
    let t = 1.0 - c;
    let r = [
        [t * k.x * k.x + c, t * k.x * k.y + s * k.z, t * k.x * k.z - s * k.y],
        [t * k.x * k.y - s * k.z, t * k.y * k.y + c, t * k.y * k.z + s * k.x],
        [t * k.x * k.z + s * k.y, t * k.y * k.z - s * k.x, t * k.z * k.z + c],
    ];
    let mut m = IDENTITY;
    for (mc, rc) in m.iter_mut().zip(r) {
        mc[..3].copy_from_slice(&rc);
    }
    let ro = apply_vector(&m, origin);
    let tr = origin - ro + translate;
    m[3] = [tr.x, tr.y, tr.z, 1.0];
    m
}

/// A placement of a component inside a parent component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    pub id: u64,
    pub component: u64,
    /// The component it sits in (0 = the root).
    #[serde(default)]
    pub parent: u64,
    pub name: String,
    #[serde(default = "identity")]
    pub transform: Mat,
    /// Grounded occurrences don't move.
    #[serde(default)]
    pub grounded: bool,
    /// Moves whose translation was typed as expressions over the design's parameters, which the
    /// occurrence follows when they change (oldest first; none in older designs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressed_moves: Vec<ExpressedMove>,
}

fn identity() -> Mat {
    IDENTITY
}

/// How far an occurrence's expressed moves take it after a parameter change
/// ([`Document::expressed_move_shifts`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressedShift {
    pub occurrence: u64,
    /// In the occurrence's parent frame (mm).
    pub shift: Vec3,
    /// What each of its expressed moves works out to now, in order.
    pub at: Vec<[f64; 3]>,
}

/// How many expressed moves one occurrence keeps.
pub const MAX_EXPRESSED_MOVES: usize = 64;

/// A move of an occurrence whose translation was typed as expressions (`occurrence.move` with
/// `translate: ["0 mm", "dy", 0]`, #8192). The occurrence keeps it and, whenever the design
/// changes, moves on by the difference between what the expressions work out to now and what they
/// worked out to when last applied: a parameter change moves it as the move would have, and
/// every other move of it (a drag, another move) stays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpressedMove {
    /// Per axis, the length expression typed (`None` for a number, which never changes).
    pub translate: [Option<String>; 3],
    /// What the translation worked out to when it was last applied (mm, along `frame`'s axes).
    pub at: [f64; 3],
    /// Takes the translation into the occurrence's parent frame: the identity when it was typed
    /// in that frame (`space: local`), the parent's world rotation undone when it was typed in
    /// world axes (`space: world`).
    #[serde(default = "identity", skip_serializing_if = "is_identity")]
    pub frame: Mat,
}

impl ExpressedMove {
    /// The translation the expressions give now, or why they can't: an unknown name, a wrong
    /// unit, or a length too large.
    pub fn eval(&self, params: &crate::joints::Params) -> Result<[f64; 3]> {
        let mut out = self.at;
        for (slot, e) in out.iter_mut().zip(&self.translate) {
            let Some(e) = e else { continue };
            let v = params.eval(e, crate::expr::Kind::Length).map_err(|err| DocError::Invalid(format!("translate `{e}`: {err}")))?;
            if !v.is_finite() || v.abs() > 1e9 {
                return Err(DocError::Invalid(format!("translate `{e}` gives {v}")));
            }
            *slot = v;
        }
        Ok(out)
    }

    /// The expressions it keeps, with the axis each moves along.
    pub fn exprs(&self) -> impl Iterator<Item = (&'static str, &str)> {
        ["x", "y", "z"].into_iter().zip(&self.translate).filter_map(|(axis, e)| e.as_deref().map(|e| (axis, e)))
    }
}

/// How deep and how many placements [`Document::placements`] walks.
pub const MAX_PLACEMENT_DEPTH: usize = 64;
pub const MAX_PLACEMENTS: usize = 100_000;

fn map_plane(m: &Mat, p: &Plane) -> Plane {
    let (o, x, y) = (apply_point(m, p.origin), apply_vector(m, p.x), apply_vector(m, p.y));
    Plane::new(o, x, y).unwrap_or(*p)
}

fn map_plane_ref(m: &Mat, p: &mut PlaneRef) {
    match p {
        PlaneRef::Custom { plane } => *plane = map_plane(m, plane),
        PlaneRef::Placed { origin, x_dir, y_dir } => {
            map_point_expr(m, origin);
            *x_dir = apply_vector(m, *x_dir);
            *y_dir = apply_vector(m, *y_dir);
        }
        PlaneRef::Face { plane, at, .. } => {
            *plane = map_plane(m, plane);
            *at = apply_point(m, *at);
        }
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => map_plane_ref(m, base),
        PlaneRef::ThroughPoint { base, point } => {
            map_plane_ref(m, base);
            map_geo_ref(m, point);
        }
        PlaneRef::AtAngle { base, axis_origin, axis_dir, line, .. } => {
            map_plane_ref(m, base);
            *axis_origin = apply_point(m, *axis_origin);
            *axis_dir = apply_vector(m, *axis_dir);
            if let Some(g) = line {
                map_geo_ref(m, g);
            }
        }
        // Named origin planes and construction planes are the component's own.
        PlaneRef::Origin { .. } | PlaneRef::Construction { .. } => {}
        PlaneRef::Tangent { face, at } => {
            *at = apply_point(m, *at);
            map_geo_ref(m, face);
        }
        PlaneRef::Midplane { .. }
        | PlaneRef::TwoEdges { .. }
        | PlaneRef::ThreePoints { .. }
        | PlaneRef::AlongPath { .. }
        | PlaneRef::Perpendicular { .. } => {
            for r in p.refs_mut() {
                map_geo_ref(m, r);
            }
        }
    }
}

fn map_geo_ref(m: &Mat, g: &mut crate::construct::GeoRef) {
    g.map_points(&mut |p: &mut Vec3| *p = apply_point(m, *p), &mut |pl: &mut PlaneRef| map_plane_ref(m, pl));
}

/// Map a point given as expressions by `m`: numbers stay numbers; a coordinate that is an
/// expression stays one (`m · (x, y, z) + t`, written out), so it still follows its parameters.
pub fn map_point_expr(m: &Mat, p: &mut crate::PointExpr) {
    let num = |e: &crate::Expr| e.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    if let [Some(x), Some(y), Some(z)] = [num(&p[0]), num(&p[1]), num(&p[2])] {
        *p = crate::point_expr(apply_point(m, Vec3::new(x, y, z)));
        return;
    }
    let old = p.clone();
    for (row, out) in p.iter_mut().enumerate() {
        let mut constant = m[3][row];
        let mut terms: Vec<String> = Vec::new();
        for (col, e) in old.iter().enumerate() {
            let c = m[col][row];
            if c.abs() < 1e-12 {
                continue;
            }
            match num(e) {
                Some(v) => constant += c * v,
                None if (c - 1.0).abs() < 1e-12 => terms.push(format!("({e})")),
                None => terms.push(format!("{c} * ({e})")),
            }
        }
        let mut e = terms.join(" + ");
        if terms.is_empty() {
            e = format!("{constant} mm");
        } else if constant.abs() >= 1e-12 {
            e = format!("{e} {} {} mm", if constant < 0.0 { '-' } else { '+' }, constant.abs());
        }
        *out = e.into();
    }
}

impl FeatureKind {
    /// Map the feature's world geometry (points, directions, explicit planes) by `m`.
    pub fn transform_geometry(&mut self, m: &Mat) {
        use crate::feature_geometry::Geo;
        use crate::walk::Walk;
        self.walk_geometry_mut(&mut |g| match g {
            Geo::Pick(p) | Geo::Pos(p) => *p = apply_point(m, *p),
            Geo::Dir(v) => *v = apply_vector(m, *v),
            Geo::Plane(p) => map_plane_ref(m, p),
            Geo::Construct(r) => map_geo_ref(m, r),
            Geo::Placement(e) => map_point_expr(m, e),
            // An Emboss cylinder is picked in its component's frame (`EmbossFace`).
            Geo::Pinned(_) | Geo::Profiles(..) => {}
        });
    }
}

/// [`Document::component_transform`] with each component's first occurrence found by `first`
/// (an index, where many transforms are wanted at once).
pub(crate) fn transform_through<'a>(component: u64, first: impl Fn(u64) -> Option<&'a Occurrence>) -> Mat {
    let mut m = IDENTITY;
    let mut c = component;
    for _ in 0..1000 {
        if c == 0 {
            break;
        }
        let Some(o) = first(c) else { break };
        m = mat_mul(&o.transform, &m);
        c = o.parent;
    }
    m
}

impl Document {
    /// The first occurrence of a component (its placement for authoring).
    pub fn occurrence_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.occurrences.iter().find(|o| o.component == component)
    }

    /// World transform of a component's frame (through its first occurrence and its parents').
    pub fn component_transform(&self, component: u64) -> Mat {
        transform_through(component, |c| self.occurrence_of(c))
    }

    /// One placement: the component the occurrence `path` from the root leads to (the root for
    /// an empty path) and its world transform. `None` when the path no longer exists.
    pub fn path_placement(&self, path: &[u64]) -> Option<(u64, Mat)> {
        if path.len() > MAX_PLACEMENT_DEPTH + 1 {
            return None;
        }
        let (mut m, mut c) = (IDENTITY, 0);
        for id in path {
            let o = self.occurrences.iter().find(|o| o.id == *id && o.parent == c)?;
            m = mat_mul(&m, &o.transform);
            c = o.component;
        }
        Some((c, m))
    }

    /// World transform of one placement of `component`: the occurrence `path` from the root
    /// (empty for the root itself). `None` when the path no longer leads to that component.
    pub fn path_transform(&self, component: u64, path: &[u64]) -> Option<Mat> {
        self.path_placement(path).and_then(|(c, m)| (c == component).then_some(m))
    }

    /// Every placement of each component: (component, occurrence path ids, world transform).
    /// A branch stops past [`MAX_PLACEMENT_DEPTH`] levels, the walk past [`MAX_PLACEMENTS`] placements.
    pub fn placements(&self) -> Vec<(u64, Vec<u64>, Mat)> {
        let mut out = vec![(0, Vec::new(), IDENTITY)];
        let mut stack: Vec<(u64, Vec<u64>, Mat)> = vec![(0, Vec::new(), IDENTITY)];
        while let Some((comp, path, m)) = stack.pop() {
            if out.len() > MAX_PLACEMENTS {
                break;
            }
            // Only this branch stops past the depth limit; the others are still walked.
            if path.len() > MAX_PLACEMENT_DEPTH {
                continue;
            }
            for o in self.occurrences.iter().filter(|o| o.parent == comp) {
                let mut p = path.clone();
                p.push(o.id);
                let w = mat_mul(&m, &o.transform);
                out.push((o.component, p.clone(), w));
                stack.push((o.component, p, w));
            }
        }
        out
    }

    /// Every body as the world shows it: its world name ("Body1", or "Body1 (Occ:2)" for a
    /// further instance), its component, the occurrence path, and the transform from the frame
    /// its geometry is in to the world.
    pub fn body_frames(&self, st: &crate::ModelState) -> Vec<(String, u64, Vec<u64>, Mat)> {
        self.body_frames_with_source(st).into_iter().map(|(world, _, component, path, matrix)| (world, component, path, matrix)).collect()
    }

    /// Body frames with explicit definition names; callers must not infer them from labels.
    pub fn body_frames_with_source(&self, st: &crate::ModelState) -> Vec<(String, String, u64, Vec<u64>, Mat)> {
        let mut out = Vec::new();
        let owners = self.model_body_components(st);
        let names = WorldNames::new(self, st);
        for (comp, path, m) in self.placements() {
            for (b, _) in st.bodies.iter().zip(&owners).filter(|(_, owner)| **owner == comp) {
                let name = names.name(&b.name, comp, &path);
                let w = match self.model_body_offset(b) {
                    Some(o) => mat_mul(&m, o),
                    None => m,
                };
                out.push((name, b.definition_name().to_string(), comp, path.clone(), w));
            }
        }
        out
    }

    /// Computed placement name. Source names retain priority; ambiguous labels use full IDs.
    /// Resolves label ambiguity with bounded dynamic counts instead of expanding all placements.
    /// To name many placed bodies, make one [`WorldNames`] and ask it for each.
    pub fn world_body_name(&self, st: &crate::ModelState, source: &str, component: u64, path: &[u64]) -> String {
        // One name: scanning the design once is cheaper than indexing it first.
        placed_name(&Scan { doc: self, st }, source, component, path)
    }

    /// The occurrence path of a component's first placement.
    pub fn first_path(&self, component: u64) -> Vec<u64> {
        let mut path = Vec::new();
        let mut c = component;
        for _ in 0..1000 {
            if c == 0 {
                break;
            }
            let Some(o) = self.occurrence_of(c) else { break };
            path.insert(0, o.id);
            c = o.parent;
        }
        path
    }

    /// Is `a` inside `b` (or `b` itself)?
    pub fn component_within(&self, a: u64, b: u64) -> bool {
        let mut c = a;
        for _ in 0..1000 {
            if c == b {
                return true;
            }
            if c == 0 {
                return false;
            }
            match self.components.iter().find(|x| x.id == c) {
                Some(x) => c = x.parent,
                None => return false,
            }
        }
        false
    }

    /// Would putting a placement of `component` into `parent` put it inside itself: is `parent`
    /// the component, or inside it, created in it or placed in it at any depth? Placements can sit
    /// apart from where their component was created ([`Self::component_within`] follows only
    /// that), so both are followed.
    pub fn inside_component(&self, parent: u64, component: u64) -> bool {
        if self.component_within(parent, component) {
            return true;
        }
        let mut stack = vec![component];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(c) = stack.pop() {
            if !seen.insert(c) || seen.len() > MAX_PLACEMENTS {
                continue;
            }
            for o in self.occurrences.iter().filter(|o| o.parent == c) {
                if o.component == parent {
                    return true;
                }
                stack.push(o.component);
            }
        }
        false
    }

    /// The occurrences whose expressed moves ([`ExpressedMove`]) work out to something else than
    /// when they were last applied (a parameter changed), with how far each moves in its parent's
    /// frame. A move whose expressions don't work out (a parameter turned into an angle) leaves the
    /// occurrence where it is until they do again; so does one that would take it out of range.
    pub fn expressed_move_shifts(&self) -> Vec<ExpressedShift> {
        if self.occurrences.iter().all(|o| o.expressed_moves.is_empty()) {
            return Vec::new();
        }
        let params = crate::joints::Params::new(self);
        let mut out = Vec::new();
        for o in self.occurrences.iter().filter(|o| !o.expressed_moves.is_empty()) {
            let mut shift = Vec3::ZERO;
            let mut at = Vec::with_capacity(o.expressed_moves.len());
            for m in &o.expressed_moves {
                let now = m.eval(&params).unwrap_or(m.at);
                let d = Vec3::new(now[0] - m.at[0], now[1] - m.at[1], now[2] - m.at[2]);
                shift = shift + apply_vector(&m.frame, d);
                at.push(now);
            }
            let moved = Vec3::new(o.transform[3][0], o.transform[3][1], o.transform[3][2]) + shift;
            if shift == Vec3::ZERO || !moved.is_finite() || [moved.x, moved.y, moved.z].iter().any(|x| x.abs() > 1e9) {
                continue;
            }
            out.push(ExpressedShift { occurrence: o.id, shift, at });
        }
        out
    }

    /// Move the occurrences as [`Document::expressed_move_shifts`] worked out.
    pub fn apply_expressed_shifts(&mut self, shifts: &[ExpressedShift]) {
        for s in shifts {
            let Some(o) = self.occurrences.iter_mut().find(|o| o.id == s.occurrence) else { continue };
            for (i, v) in [s.shift.x, s.shift.y, s.shift.z].into_iter().enumerate() {
                if let Some(t) = o.transform[3].get_mut(i) {
                    *t += v;
                }
            }
            for (m, at) in o.expressed_moves.iter_mut().zip(&s.at) {
                m.at = *at;
            }
        }
    }

    /// Add an occurrence of an existing component into `parent`.
    pub fn add_occurrence(&mut self, component: u64, parent: u64, transform: Mat) -> Result<u64> {
        if component == 0 || !self.components.iter().any(|c| c.id == component) {
            return Err(DocError::Unknown(format!("component {component}")));
        }
        if self.inside_component(parent, component) {
            return Err(DocError::Invalid("a component can't contain itself".into()));
        }
        if self.occurrences.len() >= 100_000 {
            return Err(DocError::Invalid("too many occurrences".into()));
        }
        let id = self.next_id.max(1);
        self.next_id = id.checked_add(1).ok_or_else(|| DocError::Invalid("too many items in the design".into()))?;
        let cname = self.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
        let k = self.occurrences.iter().filter(|o| o.component == component).count() + 1;
        self.occurrences.push(crate::Occurrence { id, component, parent, name: format!("{cname}:{k}"), transform, grounded: false, expressed_moves: Vec::new() });
        Ok(id)
    }
}

/// The names of a design's placed bodies ([`Document::world_body_name`]), with what naming one
/// needs from the whole design worked out once: the occurrences by id, by name and by
/// component, the model's body names, and whether any name could alias a label or fallback key.
/// Naming each body of an assembly is then as cheap as its occurrence path, not a scan of the
/// whole design.
pub struct WorldNames<'a> {
    /// The first occurrence with each id.
    by_id: std::collections::HashMap<u64, &'a crate::Occurrence>,
    /// The occurrences with each name, in document order.
    by_name: std::collections::HashMap<&'a str, Vec<&'a crate::Occurrence>>,
    /// Each component's first occurrence ([`Document::occurrence_of`]).
    first_of: std::collections::HashMap<u64, &'a crate::Occurrence>,
    bodies: std::collections::HashSet<&'a str>,
    /// Some occurrence or body name has a slash or the fallback key's marker.
    reserved: bool,
}

impl<'a> WorldNames<'a> {
    pub fn new(doc: &'a Document, st: &'a crate::ModelState) -> WorldNames<'a> {
        let mut by_id = std::collections::HashMap::with_capacity(doc.occurrences.len());
        let mut by_name: std::collections::HashMap<&str, Vec<&crate::Occurrence>> = std::collections::HashMap::new();
        let mut first_of = std::collections::HashMap::new();
        for o in &doc.occurrences {
            by_id.entry(o.id).or_insert(o);
            by_name.entry(o.name.as_str()).or_default().push(o);
            first_of.entry(o.component).or_insert(o);
        }
        let reserved =
            doc.occurrences.iter().any(|o| o.name.contains('/') || o.name.contains("@occ[")) || st.bodies.iter().any(|b| b.name.contains("@occ["));
        let bodies = st.bodies.iter().map(|b| b.name.as_str()).collect();
        WorldNames { by_id, by_name, first_of, bodies, reserved }
    }

    /// The world name of the body `source` of `component` placed through `path`.
    pub fn name(&self, source: &str, component: u64, path: &[u64]) -> String {
        placed_name(self, source, component, path)
    }
}

/// What naming a placed body looks up in the design: by scanning it ([`Scan`], for one name)
/// or through its indexes ([`WorldNames`], for many).
trait NameLookup {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence>;
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence));
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence>;
    fn reserved(&self) -> bool;
    fn has_body(&self, name: &str) -> bool;
}

/// Lookups by scanning the design.
struct Scan<'a> {
    doc: &'a Document,
    st: &'a crate::ModelState,
}

impl NameLookup for Scan<'_> {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence> {
        self.doc.occurrences.iter().find(|o| o.id == id)
    }
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence)) {
        self.doc.occurrences.iter().filter(|o| o.name == name).for_each(f);
    }
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.doc.occurrence_of(component)
    }
    fn reserved(&self) -> bool {
        self.doc.occurrences.iter().any(|o| o.name.contains('/') || o.name.contains("@occ["))
            || self.st.bodies.iter().any(|b| b.name.contains("@occ["))
    }
    fn has_body(&self, name: &str) -> bool {
        self.st.body(name).is_some()
    }
}

impl NameLookup for WorldNames<'_> {
    fn occurrence(&self, id: u64) -> Option<&crate::Occurrence> {
        self.by_id.get(&id).copied()
    }
    fn each_named(&self, name: &str, f: &mut dyn FnMut(&crate::Occurrence)) {
        self.by_name.get(name).into_iter().flatten().copied().for_each(f);
    }
    fn first_of(&self, component: u64) -> Option<&crate::Occurrence> {
        self.first_of.get(&component).copied()
    }
    fn reserved(&self) -> bool {
        self.reserved
    }
    fn has_body(&self, name: &str) -> bool {
        self.bodies.contains(name)
    }
}

/// The world name of the body `source` of `component` placed through `path` (see
/// [`Document::world_body_name`]).
fn placed_name(l: &impl NameLookup, source: &str, component: u64, path: &[u64]) -> String {
    // The occurrence path of the component's first placement ([`Document::first_path`]).
    let mut first = Vec::new();
    let mut c = component;
    for _ in 0..1000 {
        if c == 0 {
            break;
        }
        let Some(o) = l.first_of(c) else { break };
        first.insert(0, o.id);
        c = o.parent;
    }
    if path == first {
        return source.to_string();
    }
    let names: Vec<&str> = path.iter().filter_map(|id| l.occurrence(*id)).map(|o| o.name.as_str()).collect();
    let label = names.join("/");
    let candidate = format!("{source} ({label})");
    let mut counts = std::collections::BTreeMap::from([(0, 1usize)]);
    for name in &names {
        let mut next = std::collections::BTreeMap::<u64, usize>::new();
        l.each_named(name, &mut |o| {
            if let Some(count) = counts.get(&o.parent) {
                let n = next.entry(o.component).or_default();
                *n = (*n + count).min(2);
            }
        });
        counts = next;
    }
    let ambiguous = counts.get(&component).copied().unwrap_or(0) > 1;
    // Slash and reserved-marker names could alias concatenated labels or fallback keys.
    if !ambiguous && !l.reserved() && !l.has_body(&candidate) {
        return candidate;
    }
    let ids = path.iter().map(u64::to_string).collect::<Vec<_>>().join("/");
    let encoded = serde_json::to_string(source).unwrap_or_else(|_| source.to_string());
    let mut key = format!("{encoded}@occ[{ids}]");
    while l.has_body(&key) {
        key.push('#');
    }
    key
}
