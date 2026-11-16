//! Construction geometry from the model: references to vertices, edges, faces, planes, axes,
//! points and sketch entities, resolved on each evaluation, and the axes, points and planes
//! built from them (the CONSTRUCT panel).
//!
//! Face and edge references carry the persistent name of what was picked (see `naming`), and a
//! vertex the names of the edges that meet there, so they follow the model when earlier features
//! change; the picked point is the fallback.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec3};
use solidcraft_kernel::{self as kernel, FaceSurface};

use crate::eval::{ModelBody, ModelState};
use crate::expr::{Kind, Value};
use crate::{DocError, Document, PlaneRef, Result};

/// Something picked to build construction geometry from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GeoRef {
    /// A fixed point.
    Point { p: Vec3 },
    /// An origin datum of an exact placement (its O point, an axis or a plane), found again on
    /// each evaluation, so it follows the component the way a named reference does.
    Datum { datum: DatumRef },
    /// The design origin.
    Origin,
    /// A body vertex: where the edges named `edges` meet (the end they share), else the one
    /// nearest `at`.
    Vertex {
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<String>>,
    },
    /// A body edge: the one named `name`, else the one nearest `at`.
    Edge {
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A body face: the one named `name`, else the one at `at`.
    Face {
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A plane (origin, construction, offset…).
    Plane { plane: PlaneRef },
    /// World X, Y or Z axis.
    World { axis: String },
    /// A construction axis feature, by name.
    Axis { name: crate::FeatureName },
    /// A construction point feature, by name.
    ConstructPoint { name: crate::FeatureName },
    /// A sketch point (by sketch feature id and point id), or a drawn wire node as `wire:index`.
    SketchPoint { sketch: u64, point: String },
    /// A sketch curve (by sketch feature id and curve id).
    SketchCurve { sketch: u64, curve: String },
    /// A snap point of an edge, face or sketch curve (#11228): its centre, its middle or a flat
    /// face's centre, found again on `of` as it is now. Where `of` has more than one (a
    /// cylinder's two end centres), `end` says which along the axis (0 the low end, 1 the
    /// high one, set when the reference is made), else the one nearest `at`, where it was picked.
    Snap {
        of: Box<GeoRef>,
        kind: SnapAt,
        at: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<usize>,
    },
}

/// Which snap point of an edge, face or sketch curve a [`GeoRef::Snap`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapAt {
    /// The centre of a circle or arc, a sphere or torus, or one end of a cylinder or cone.
    Centre,
    /// The middle of an edge or sketch curve, along its length.
    Middle,
}

/// How a construction axis is defined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AxisDef {
    /// The axis of a cylinder, cone or torus face (or a circular edge).
    ThroughFace { face: GeoRef },
    /// Perpendicular to a face, through a point (projected onto the face).
    NormalToFace { face: GeoRef, point: GeoRef },
    /// Where two planes meet.
    TwoPlanes { a: GeoRef, b: GeoRef },
    /// Through two points.
    TwoPoints { a: GeoRef, b: GeoRef },
    /// Along a straight edge (or sketch line).
    Edge { edge: GeoRef },
}

/// How a construction point is defined.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PointDef {
    /// At a vertex, sketch point or fixed point.
    At { point: GeoRef },
    /// Where two edges (lines) meet, or come closest.
    TwoEdges { a: GeoRef, b: GeoRef },
    /// Where three planes meet.
    ThreePlanes { a: GeoRef, b: GeoRef, c: GeoRef },
    /// The centre of a circular edge, a sphere or a torus.
    Center { of: GeoRef },
    /// Where an edge (line) crosses a plane.
    EdgeAndPlane { edge: GeoRef, plane: GeoRef },
    /// At a fraction `t` (0…1) of the way along an edge or sketch curve.
    AlongPath { path: GeoRef, t: crate::Expr },
}

/// Resolved construction geometry, kept in the model state by feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConstructGeom {
    Plane(Plane),
    Axis { origin: Vec3, dir: Vec3 },
    Point(Vec3),
}

impl ConstructGeom {
    /// The geometry moved by `m` (where its component is placed).
    pub fn placed(&self, m: &crate::Mat) -> ConstructGeom {
        let (point, vector) = (|p: Vec3| crate::apply_point(m, p), |v: Vec3| crate::apply_vector(m, v));
        match self {
            ConstructGeom::Plane(p) => Plane::new(point(p.origin), vector(p.x), vector(p.y)).map_or_else(|| self.clone(), ConstructGeom::Plane),
            ConstructGeom::Axis { origin, dir } => ConstructGeom::Axis { origin: point(*origin), dir: vector(*dir) },
            ConstructGeom::Point(p) => ConstructGeom::Point(point(*p)),
        }
    }
}

/// One of the seven named items of a placement's Origin folder: the O point, the X, Y and Z
/// axes, and the XY, XZ and YZ planes. Case-insensitive to parse; the `Plane::named` aliases
/// (TOP/FRONT/RIGHT) stay root-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatumItem {
    O,
    X,
    Y,
    Z,
    XY,
    XZ,
    YZ,
}

impl DatumItem {
    /// The item's name as a key spells it.
    pub fn name(self) -> &'static str {
        match self {
            DatumItem::O => "O",
            DatumItem::X => "X",
            DatumItem::Y => "Y",
            DatumItem::Z => "Z",
            DatumItem::XY => "XY",
            DatumItem::XZ => "XZ",
            DatumItem::YZ => "YZ",
        }
    }

    /// Case-insensitive; the Plane::named aliases (TOP/FRONT/RIGHT) stay root-only.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_uppercase().as_str() {
            "O" => DatumItem::O,
            "X" => DatumItem::X,
            "Y" => DatumItem::Y,
            "Z" => DatumItem::Z,
            "XY" => DatumItem::XY,
            "XZ" => DatumItem::XZ,
            "YZ" => DatumItem::YZ,
            _ => return None,
        })
    }

    /// The item in its component's own frame: O the point, X/Y/Z the axes, XY/XZ/YZ the planes.
    pub fn geom(self) -> ConstructGeom {
        match self {
            DatumItem::O => ConstructGeom::Point(Vec3::ZERO),
            DatumItem::X => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::X },
            DatumItem::Y => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::Y },
            DatumItem::Z => ConstructGeom::Axis { origin: Vec3::ZERO, dir: Vec3::Z },
            DatumItem::XY => ConstructGeom::Plane(Plane::XY),
            DatumItem::XZ => ConstructGeom::Plane(Plane::XZ),
            DatumItem::YZ => ConstructGeom::Plane(Plane::YZ),
        }
    }
}

/// An origin datum of an exact placement. The empty path is never a `DatumRef`: the root keeps
/// `PlaneRef::Origin` and `GeoRef::Origin` as they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatumRef {
    /// The occurrence ids of the placement, root first.
    pub path: Vec<u64>,
    pub item: DatumItem,
}

impl DatumRef {
    /// The datum's Browser key: `o:2/3/origin/XY`.
    pub fn key(&self) -> String {
        format!("o:{}/origin/{}", self.path.iter().map(|id| id.to_string()).collect::<Vec<_>>().join("/"), self.item.name())
    }

    /// Strict: canonical non-zero decimal ids, depth at most 64, length at most 2048. Rejects
    /// `c0/origin`, `o:01/origin/X` and `o:0/origin/X`.
    pub fn parse_key(key: &str) -> Option<Self> {
        if key.len() > 2048 {
            return None;
        }
        let rest = key.strip_prefix("o:")?;
        let (ids, item) = rest.rsplit_once("/origin/")?;
        if ids.is_empty() || ids.split('/').count() > 64 {
            return None;
        }
        let path = ids.split('/').map(|id| id.parse::<u64>().ok().filter(|n| *n != 0 && n.to_string() == id)).collect::<Option<Vec<_>>>()?;
        Some(DatumRef { path, item: DatumItem::parse(item)? })
    }
}

impl Serialize for DatumRef {
    /// Saved as its Browser key: `o:2/3/origin/XY`.
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for DatumRef {
    /// Reads a saved reference back from its Browser key: anything that is not a bounded
    /// datum key is a deserialize error, so an older release never mis-reads a newer design.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let key = String::deserialize(d)?;
        DatumRef::parse_key(&key).ok_or_else(|| serde::de::Error::custom(format!("not a datum key: `{key}`")))
    }
}

/// An origin datum of an exact placement, resolved at evaluation time: the item in its own
/// component's frame, placed by the placement's world frame. It follows the component on every
/// evaluation, the way `PlaneRef::Face` re-finds its face.
pub fn resolve_datum(doc: &Document, d: &DatumRef) -> Result<ConstructGeom> {
    Ok(d.item.geom().placed(&doc.placement_frame(&d.path)?))
}

/// [`resolve_datum`] as the component `component` sees it: placed by the placements, then
/// mapped back through that component's frame, since a feature evaluated in its own frame
/// places what it reads into the world again through that frame (the world pass). The root
/// (0) reads the world as it is.
pub fn resolve_datum_in(doc: &Document, d: &DatumRef, component: u64) -> Result<ConstructGeom> {
    if component == 0 {
        return resolve_datum(doc, d);
    }
    let inverse =
        crate::mat_inverse(&doc.component_transform(component)).ok_or_else(|| DocError::Invalid("the component's placement is singular".into()))?;
    Ok(resolve_datum(doc, d)?.placed(&inverse))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Construct {
    pub feature: u64,
    pub name: String,
    pub geom: ConstructGeom,
    /// The bodies it was built from, when every pick it reads lies on a body
    /// ([`source_bodies`]): a later Move of all of them carries it along (#10127). Empty when
    /// it also reads something fixed (a point, a plane, a sketch), which keeps it in place.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<String>,
}

/// Everything a reference needs to be resolved.
pub(crate) struct Ctx<'a> {
    pub doc: &'a Document,
    pub vals: &'a BTreeMap<String, Value>,
    pub st: &'a ModelState,
    /// How deep the plane resolution that led here already is. A plane built from model
    /// geometry resolves its sides through the document again, so the count has to continue
    /// across that hop; restarting it lets a plane that refers to itself recurse forever.
    pub depth: usize,
    /// The component the reading feature lives in: a datum it reads is found through the
    /// placements and mapped back into this frame ([`resolve_datum_in`]). The root (0) is
    /// the world.
    pub component: u64,
}

fn bad(m: impl Into<String>) -> DocError {
    DocError::Invalid(m.into())
}

fn bodies(st: &ModelState) -> impl Iterator<Item = &ModelBody> {
    st.bodies.iter().filter(|b| !b.body.is_mesh())
}

fn construct<'a>(st: &'a ModelState, name: &str) -> Result<&'a ConstructGeom> {
    st.construct.iter().find(|c| c.name == name).map(|c| &c.geom).ok_or_else(|| DocError::Unknown(format!("construction `{name}`")))
}

/// The face picked: (body, face index, point on it). A face picked with a name is the face the
/// name finds, or the reference fails, asking for it to be picked again.
fn face_of<'a>(st: &'a ModelState, at: Vec3, name: &Option<String>) -> Result<(&'a ModelBody, usize, Vec3)> {
    // The named face itself: its nearest point to the old pick can lie on an edge it shares
    // (a corner round that a parameter moved away from the pick), where picking again by the
    // point finds the flat face beside it (#1255).
    if let Some(n) = name.as_deref() {
        return crate::name_lookup::locate_face(st, n, at)
            .and_then(|hit| st.bodies.get(hit.body).filter(|b| !b.body.is_mesh() && hit.face < b.body.face_count()).map(|b| (b, hit.face, hit.point)))
            .ok_or_else(|| DocError::reselect(n));
    }
    for b in bodies(st) {
        if let Some(fi) = crate::appearance::face_index_at(b, at) {
            return Ok((b, fi, at));
        }
    }
    // A mesh body has no faces to pick: say so, rather than let the pick read as a miss (#11469).
    if let Some(m) = st.bodies.iter().find(|b| lies_on_mesh(b, at)) {
        return Err(bad(format!("no face at {at:?}: `{}` is a mesh body, which has no faces; use Convert Mesh to make it a solid first", m.name)));
    }
    Err(bad(format!("no face at {at:?}")))
}

/// Does `at` lie on the surface of a mesh body, within the pick tolerance? Only mesh bodies:
/// their triangles are the body itself.
fn lies_on_mesh(b: &ModelBody, at: Vec3) -> bool {
    if !b.body.is_mesh() {
        return false;
    }
    let Some((positions, triangles)) = b.body.triangle_mesh() else { return false };
    let tol = crate::appearance::pick_tolerance(b);
    triangles.iter().any(|t| {
        let [a, bb, c] = t.map(|i| positions[i as usize]);
        crate::appearance::closest_on_triangle(at, a, bb, c).dist(at) <= tol
    })
}

/// The edge picked, as a polyline.
fn edge_points(st: &ModelState, at: Vec3, name: &Option<String>) -> Result<Vec<Vec3>> {
    edge_pick(st, at, name).map(|(_, _, pts)| pts)
}

/// The edge picked: its body, its index there (as `Body::edges` lists it) and its polyline. An
/// edge picked with a name is the edge the name finds, or the reference fails, asking for it to
/// be picked again.
fn edge_pick<'a>(st: &'a ModelState, at: Vec3, name: &Option<String>) -> Result<(&'a ModelBody, usize, Vec<Vec3>)> {
    let mut at = at;
    if let Some(n) = name {
        at = bodies(st).find_map(|b| crate::naming::find_edge(b, n, at).ok()).ok_or_else(|| DocError::reselect(n.as_str()))?;
    }
    let mut best: Option<(f64, &ModelBody, usize, Vec<Vec3>)> = None;
    for b in bodies(st) {
        let tol = (b.body.size() * 1e-3).max(1e-3);
        let Ok(edges) = b.body.edges(tol) else { continue };
        for (i, e) in edges.into_iter().enumerate() {
            let d = e.points.windows(2).map(|w| at.dist_to_segment(w[0], w[1])).fold(f64::MAX, f64::min);
            if best.as_ref().is_none_or(|(bd, ..)| d < *bd) {
                best = Some((d, b, i, e.points));
            }
        }
    }
    match best {
        Some((d, b, i, pts)) if pts.len() >= 2 && d <= 1e-3 + 1e-2 * pts.iter().fold(0.0, |m: f64, p| m.max(p.len())).max(1.0) => Ok((b, i, pts)),
        _ => Err(bad(format!("no edge at {at:?}"))),
    }
}

/// The closest point on a polyline to `target`, projecting onto each segment rather than
/// choosing only its vertices. This keeps an edge pick on the edge between tessellation points.
fn closest_point_on_polyline(pts: &[Vec3], target: Vec3) -> Option<Vec3> {
    let mut best: Option<(f64, Vec3)> = None;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b - a;
        let len_sq = ab.dot(ab);
        let q = if len_sq < 1e-18 {
            a
        } else {
            let t = ((target - a).dot(ab) / len_sq).clamp(0.0, 1.0);
            a + ab * t
        };
        let d = target.dist(q);
        if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

/// The point on an edge nearest a pick location, projecting onto exact edge geometry
/// (such as `kernel::edge_circle`) when available, else onto its drawn polyline segments.
fn edge_point_at_pick(st: &ModelState, pick: Vec3, name: &Option<String>) -> Option<Vec3> {
    let (b, i, pts) = edge_pick(st, pick, name).ok()?;
    if let Some(c) = kernel::edge_circle(&b.body, i) {
        let v = pick - c.center;
        let v_perp = v - *c.axis * v.dot(*c.axis);
        if let Some(dir) = v_perp.normalized() {
            return Some(c.center + dir * c.radius);
        }
    }
    // Until the kernel can project onto any edge geometry (circles are exact above via
    // kernel::edge_circle), the fallback projects onto the drawn polyline. This is exact for
    // straight edges and approximates splines within mesh display tolerance.
    closest_point_on_polyline(&pts, pick)
}

/// The circle a picked edge lies on, from its exact curve (`kernel::edge_circle`) when the drawn
/// edge lies on it, else fitted to the drawn points: (centre, unit normal).
fn edge_circle(b: &ModelBody, index: usize, pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let exact = kernel::edge_circle(&b.body, index).filter(|c| {
        let tol = (b.body.size() * 1e-6).max(1e-9) * 10.0;
        pts.iter().all(|p| (p.dist(c.center) - c.radius).abs() <= tol && (*p - c.center).dot(*c.axis).abs() <= tol)
    });
    exact.map(|c| (c.center, *c.axis)).or_else(|| as_circle(pts).map(|(c, n, _)| (c, n)))
}

/// The most edge names a vertex reference keeps.
const MAX_VERTEX_EDGES: usize = 16;

/// Where the edges named `names` meet: the end every one of them has, nearest `at` (two edges
/// between the same two vertices share both). `None` when an edge is gone or they share no end.
fn named_vertex(st: &ModelState, at: Vec3, names: &[String]) -> Option<Vec3> {
    if names.len() < 2 || names.len() > MAX_VERTEX_EDGES {
        return None;
    }
    let found: Vec<(*const ModelBody, usize, [Vec3; 2])> = names
        .iter()
        .map(|n| {
            // The edge with that name itself, never the one nearest the old pick, and its ends
            // on the body that has it: picked again by a point on it, a corner found the first
            // edge of any body through that corner, such as another body's edge running past
            // it, so a vertex took that edge's end (#8650).
            bodies(st).find_map(|b| {
                let hit = crate::name_lookup::locate_edge(b, n, at).ok()?;
                let rec = b.body.record();
                let edge = rec.edges.get(hit.edge)?;
                let [v0, v1] = edge.vertices;
                let p0 = rec.vertices.get(v0 as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))?;
                let p1 = rec.vertices.get(v1 as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))?;
                Some((std::ptr::from_ref(b), hit.edge, [p0, p1]))
            })
        })
        .collect::<Option<_>>()?;
    // Two names of a vertex name two edges: a name the point decides (where the body grew so the
    // vertex lies inside another edge) can find an edge another name found.
    if found.iter().enumerate().any(|(i, (b, e, _))| found.iter().take(i).any(|(c, f, _)| std::ptr::eq(*b, *c) && e == f)) {
        return None;
    }
    let ends: Vec<[Vec3; 2]> = found.into_iter().map(|(_, _, ends)| ends).collect();
    let (first, rest) = ends.split_first()?;
    let tol = |p: Vec3| 1e-6 * (1.0 + p.len());
    first
        .iter()
        .copied()
        .filter(|p| p.is_finite() && rest.iter().all(|e| e.iter().any(|q| q.dist(*p) <= tol(*p))))
        .min_by(|p, q| p.dist(at).total_cmp(&q.dist(at)))
}

/// A polyline that is straight: its line.
fn as_line(pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let (a, b) = (*pts.first()?, *pts.last()?);
    let d = (b - a).normalized()?;
    let len = (b - a).len();
    pts.iter().all(|p| p.dist_to_segment(a, b) <= len * 1e-6 + 1e-9).then_some((a, d))
}

/// A polyline on a circle: (centre, unit normal, radius).
fn as_circle(pts: &[Vec3]) -> Option<(Vec3, Vec3, f64)> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    let (a, b, c) = (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?);
    let (ab, ac) = (b - a, c - a);
    let nrm = ab.cross(ac);
    let l2 = nrm.len2();
    if l2 < 1e-24 {
        return None;
    }
    // Circumcentre of abc.
    let centre = a + (nrm.cross(ab) * ac.len2() + ac.cross(nrm) * ab.len2()) * (1.0 / (2.0 * l2));
    let r = (a - centre).len();
    let unit = nrm.normalized()?;
    pts.iter()
        .all(|p| ((*p - centre).len() - r).abs() <= r * 1e-3 + 1e-6 && (*p - centre).dot(unit).abs() <= r * 1e-3 + 1e-6)
        .then_some((centre, unit, r))
}

fn sketch_curve_points(ctx: &Ctx, sketch: u64, curve: &str) -> Result<Vec<Vec3>> {
    let ss = ctx.st.sketch(sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    if let Some(wire) = ss.sketch.wires.iter().find(|wire| wire.id == curve) {
        if wire.pts.len() < 2 {
            return Err(bad(format!("curve `{curve}` has no length")));
        }
        // Evaluation has already mapped wires into this component's frame. Their coordinates
        // are independent of the sketch plane; applying it again would move the path twice.
        return Ok(wire.pts.clone());
    }
    let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("curve `{curve}`")))?;
    let pl = ss.sketch.polyline(ci);
    if pl.len() < 2 {
        return Err(bad(format!("curve `{curve}` has no length")));
    }
    Ok(pl.into_iter().map(|q| ss.plane.to_world(q)).collect())
}

/// A reference as a point.
pub(crate) fn point(ctx: &Ctx, g: &GeoRef) -> Result<Vec3> {
    match g {
        GeoRef::Point { p } => Ok(*p),
        GeoRef::Origin => Ok(Vec3::ZERO),
        GeoRef::Vertex { at, edges } => {
            if let Some(v) = edges.as_deref().and_then(|names| named_vertex(ctx.st, *at, names)) {
                return Ok(v);
            }
            let mut best: Option<(f64, Vec3)> = None;
            for b in bodies(ctx.st) {
                let tol = (b.body.size() * 1e-3).max(1e-3);
                let Ok(edges) = b.body.edges(tol) else { continue };
                for v in edges.iter().flat_map(|e| [e.points.first().copied(), e.points.last().copied()]).flatten() {
                    let d = v.dist(*at);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, v));
                    }
                }
            }
            best.map(|(_, v)| v).ok_or_else(|| bad("there is no vertex"))
        }
        GeoRef::ConstructPoint { name } => match construct(ctx.st, name)? {
            ConstructGeom::Point(p) => Ok(*p),
            _ => Err(bad(format!("`{name}` is not a point"))),
        },
        GeoRef::SketchPoint { sketch, point } => {
            sketch_point_position(ctx.st, *sketch, point).ok_or_else(|| DocError::Unknown(format!("sketch point `{point}`")))
        }
        // A round edge or face: its centre.
        GeoRef::Edge { .. } | GeoRef::Face { .. } | GeoRef::SketchCurve { .. } => centre(ctx, g),
        GeoRef::Snap { of, kind, at, end } => snap_point(ctx.st, of, *kind, *at, *end).or_else(|_| snap_point_ctx(ctx, of, *kind, *at)),
        GeoRef::Datum { datum } => match resolve_datum_in(ctx.doc, datum, ctx.component)? {
            ConstructGeom::Point(p) => Ok(p),
            _ => Err(bad("the datum is not a point (pick the O of a component)")),
        },
        _ => Err(bad("pick a point, vertex or sketch point")),
    }
}

/// The centres of the circular edges that end a cylinder or cone face, from its B-rep (low, high
/// along its axis), if it is one.
fn face_axis_ends(st: &ModelState, of: &GeoRef) -> Option<Vec<Vec3>> {
    let GeoRef::Face { at, name } = of else { return None };
    let (b, fi, _) = face_of(st, *at, name).ok()?;
    let (origin, axis) = match kernel::face_surface(&b.body, fi) {
        Some(FaceSurface::Cylinder { origin, axis, .. }) => (origin, axis),
        Some(FaceSurface::Cone { apex, axis, .. }) => (apex, axis),
        _ => return None,
    };
    let tol = 1e-6 * (1.0 + b.body.size());
    let mut ends: Vec<(f64, Vec3)> = kernel::face_edge_indices(&b.body, fi)
        .into_iter()
        .filter_map(|e| kernel::edge_circle(&b.body, e))
        .filter(|c| (c.center - origin).cross(axis).len() <= tol)
        .map(|c| ((c.center - origin).dot(axis), c.center))
        .collect();
    ends.sort_by(|p, q| p.0.total_cmp(&q.0));
    ends.dedup_by(|p, q| (p.0 - q.0).abs() <= tol);
    match ends.as_slice() {
        [] => None,
        [one] => Some(vec![one.1]),
        [lo, .., hi] => Some(vec![lo.1, hi.1]),
    }
}

/// The one of `found` nearest `at`.
fn nearest(found: Vec<Vec3>, at: Vec3) -> Option<(usize, Vec3)> {
    found.into_iter().enumerate().filter(|(_, p)| p.is_finite()).min_by(|(_, p), (_, q)| p.dist(at).total_cmp(&q.dist(at)))
}

/// A snap point found from the model alone: a cylinder's or cone's end centre (the `end` one,
/// else the one nearest `at`).
fn snap_point(st: &ModelState, of: &GeoRef, kind: SnapAt, at: Vec3, end: Option<usize>) -> Result<Vec3> {
    match (kind, of) {
        (SnapAt::Centre, GeoRef::Face { .. }) => {
            let ends = face_axis_ends(st, of).ok_or_else(|| bad("the face is not a cylinder or cone"))?;
            match end {
                Some(i) => ends.get(i).copied(),
                None => nearest(ends, at).map(|(_, p)| p),
            }
            .ok_or_else(|| bad("the snap point is gone"))
        }
        _ => Err(bad("not a face's snap point")),
    }
}

/// A snap point of an edge or sketch curve (its middle or centre), or a sphere's or torus's centre.
fn snap_point_ctx(ctx: &Ctx, of: &GeoRef, kind: SnapAt, at: Vec3) -> Result<Vec3> {
    let found: Vec<Vec3> = match (kind, of) {
        (SnapAt::Middle, GeoRef::Edge { .. } | GeoRef::SketchCurve { .. }) => {
            vec![along(ctx, of, 0.5)?.0]
        }
        (SnapAt::Centre, GeoRef::Edge { .. } | GeoRef::SketchCurve { .. } | GeoRef::Face { .. }) => vec![centre(ctx, of)?],
        _ => return Err(bad("a snap point is on an edge, a face or a sketch curve")),
    };
    nearest(found, at).map(|(_, p)| p).ok_or_else(|| bad("the snap point is gone"))
}

/// Resolve a planar point or a drawn 3D wire's `wire:index` node in the evaluated state's frame.
/// Wire coordinates have already been placed; applying the sketch plane again would move them twice.
pub fn sketch_point_position(st: &ModelState, sketch: u64, point: &str) -> Option<Vec3> {
    let ss = st.sketch(sketch)?;
    if let Some(p) = ss.sketch.point_index(point).and_then(|i| ss.sketch.point(i)) {
        return Some(ss.plane.to_world(p));
    }
    let (wire, index) = point.rsplit_once(':')?;
    let index = index.parse::<usize>().ok()?;
    ss.sketch.wires.get(ss.sketch.wire_index(wire)?)?.fit.get(index).copied()
}

/// An edge or face of the model as a line: along a straight edge, through the centre of a
/// circular edge along its normal, or the axis of a cylinder, cone or torus face.
fn model_line(st: &ModelState, g: &GeoRef) -> Result<(Vec3, Vec3)> {
    match g {
        GeoRef::Edge { at, name } => {
            let (b, i, pts) = edge_pick(st, *at, name)?;
            as_line(&pts).or_else(|| edge_circle(b, i, &pts)).ok_or_else(|| bad("the edge is neither straight nor circular"))
        }
        GeoRef::Face { at, name } => {
            let (b, fi, _) = face_of(st, *at, name)?;
            match kernel::face_surface(&b.body, fi) {
                Some(FaceSurface::Cylinder { origin, axis, .. }) => Ok((origin, *axis)),
                Some(FaceSurface::Cone { apex, axis, .. }) => Ok((apex, *axis)),
                Some(FaceSurface::Torus { center, axis, .. }) => Ok((center, *axis)),
                _ => Err(bad("the face is not a cylinder, cone or torus")),
            }
        }
        _ => Err(bad(MODEL_AXIS_REFS)),
    }
}

/// What [`model_axis`] says about a reference that is neither an edge nor a face.
pub const MODEL_AXIS_REFS: &str = "pick a straight or circular edge, or a cylinder, cone or torus face";

/// The axis a feature turns about when an edge or face of the model is picked for it (a revolve,
/// a circular pattern, a plane at an angle), as [`model_line`] finds it. The direction is
/// oriented the same way however the edge or surface runs (its largest component positive), so
/// the feature keeps its turn when an edit rebuilds the edge reversed.
pub fn model_axis(st: &ModelState, g: &GeoRef) -> Result<(Vec3, Vec3)> {
    let (o, d) = model_line(st, g)?;
    let d = d.normalized().filter(|_| o.is_finite()).ok_or_else(|| bad("the axis has no direction"))?;
    Ok((o, canonical_dir(d)))
}

/// [`model_axis`] for references that may name a component's origin datum: the datum is placed
/// through the document's placements ([`resolve_datum_in`], as the component `component` sees
/// it), the rest as [`model_axis`] finds it in the model state. `None` for a datum asks for an
/// evaluation.
pub fn model_axis_in(doc: Option<&Document>, st: &ModelState, g: &GeoRef, component: u64) -> Result<(Vec3, Vec3)> {
    match g {
        GeoRef::Datum { datum } => {
            let doc = doc.ok_or_else(|| bad("this axis is built from a component's origin: evaluate first"))?;
            match resolve_datum_in(doc, datum, component)? {
                ConstructGeom::Axis { origin, dir } => Ok((origin, dir)),
                _ => Err(bad("the datum is not an axis (pick the X, Y or Z of a component)")),
            }
        }
        // A construction axis by name: where it is on each evaluation, so what turns about it follows it.
        GeoRef::Axis { name } => crate::eval::construction_axis_in(doc, st, name, component),
        _ => model_axis(st, g),
    }
}

/// `d` or `-d`, whichever has its largest component (the first of equal ones) positive: how
/// [`model_axis`] orients an axis, for a preview of one to point the same way.
pub fn canonical_dir(d: Vec3) -> Vec3 {
    let c = [d.x, d.y, d.z];
    let big = c.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    match c.iter().find(|v| v.abs() >= big - 1e-9) {
        Some(v) if *v < 0.0 => -d,
        _ => d,
    }
}

/// A reference as a line (origin, unit direction).
pub(crate) fn line(ctx: &Ctx, g: &GeoRef) -> Result<(Vec3, Vec3)> {
    match g {
        GeoRef::Datum { datum } => match resolve_datum_in(ctx.doc, datum, ctx.component)? {
            ConstructGeom::Axis { origin, dir } => Ok((origin, dir)),
            _ => Err(bad("the datum is not an axis (pick the X, Y or Z of a component)")),
        },
        GeoRef::Edge { .. } | GeoRef::Face { .. } => model_line(ctx.st, g),
        GeoRef::SketchCurve { sketch, curve } => {
            let pts = sketch_curve_points(ctx, *sketch, curve)?;
            as_line(&pts).ok_or_else(|| bad(format!("sketch curve `{curve}` is not a line")))
        }
        GeoRef::World { axis } => {
            let d = match axis.to_ascii_uppercase().as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                "Z" => Vec3::Z,
                _ => return Err(DocError::Unknown(format!("axis `{axis}`"))),
            };
            Ok((Vec3::ZERO, d))
        }
        GeoRef::Axis { name } => match construct(ctx.st, name)? {
            ConstructGeom::Axis { origin, dir } => Ok((*origin, *dir)),
            _ => Err(bad(format!("`{name}` is not an axis"))),
        },
        _ => Err(bad("pick an edge, line or axis")),
    }
}

/// A reference as a plane.
pub(crate) fn plane(ctx: &Ctx, g: &GeoRef) -> Result<Plane> {
    match g {
        GeoRef::Plane { plane } => ctx.doc.resolve_plane_in(ctx.vals, plane, ctx.depth + 1, ctx.component, Some(ctx.st)),
        GeoRef::Face { at, name } => {
            let (b, fi, q) = face_of(ctx.st, *at, name)?;
            match kernel::face_surface(&b.body, fi) {
                Some(FaceSurface::Plane { normal, point }) => {
                    let o = q - normal * (q - point).dot(normal);
                    Plane::from_normal(o, normal).ok_or_else(|| bad("degenerate face"))
                }
                _ => Err(bad("the face is not planar")),
            }
        }
        GeoRef::Datum { datum } => match resolve_datum_in(ctx.doc, datum, ctx.component)? {
            ConstructGeom::Plane(p) => Ok(p),
            _ => Err(bad("the datum is not a plane (pick the XY, XZ or YZ of a component)")),
        },
        _ => Err(bad("pick a plane or planar face")),
    }
}

/// The centre of a round edge, sketch circle, sphere or torus.
fn centre(ctx: &Ctx, g: &GeoRef) -> Result<Vec3> {
    match g {
        GeoRef::Edge { at, name } => {
            let pts = edge_points(ctx.st, *at, name)?;
            as_circle(&pts).map(|(c, _, _)| c).ok_or_else(|| bad("the edge is not circular"))
        }
        GeoRef::SketchCurve { sketch, curve } => {
            let pts = sketch_curve_points(ctx, *sketch, curve)?;
            as_circle(&pts).map(|(c, _, _)| c).ok_or_else(|| bad(format!("sketch curve `{curve}` is not a circle or arc")))
        }
        GeoRef::Face { at, name } => {
            let (b, fi, _) = face_of(ctx.st, *at, name)?;
            match kernel::face_surface(&b.body, fi) {
                Some(FaceSurface::Sphere { center, .. }) | Some(FaceSurface::Torus { center, .. }) => Ok(center),
                _ => Err(bad("the face is not a sphere or torus")),
            }
        }
        _ => Err(bad("pick a circular edge, a sphere or a torus")),
    }
}

/// The point a fraction `t` (0…1) of the way along a picked edge or sketch curve by arc length,
/// and the unit direction of travel there ([`kernel::along_path`]): exact on the curve itself.
fn along(ctx: &Ctx, g: &GeoRef, t: f64) -> Result<(Vec3, Vec3)> {
    Ok(kernel::along_path(&path_segs(ctx, g)?, t)?)
}

/// A picked edge or sketch curve as the path segments that are its curve exactly, from the end
/// the drawn curve starts at: a body edge's record curve ([`kernel::edge_path`]), a planar sketch
/// curve's exact pieces, a 3D sketch curve's arc, spline, line or linked exact curves. Only a
/// curve that exists as points alone runs along them: an intersection edge (its record holds no
/// exact curve, only the surfaces and a leader near it), a 3D curve known only as its points
/// (projected onto a face, iso curves), a spline above the exact pieces' degree.
fn path_segs(ctx: &Ctx, g: &GeoRef) -> Result<Vec<kernel::PathSeg>> {
    let lines = |pts: &[Vec3]| -> Vec<kernel::PathSeg> {
        pts.windows(2).filter(|w| w[0].dist(w[1]) > 0.0).map(|w| kernel::PathSeg::Line { a: w[0], b: w[1] }).collect()
    };
    match g {
        GeoRef::Edge { at, name } => {
            let (b, i, pts) = edge_pick(ctx.st, *at, name)?;
            let procedural =
                b.body.record().edges.get(i).is_some_and(|e| matches!(e.parameters(), solidcore_record::brep::Curve::SurfaceIntersection { .. }));
            if procedural {
                return Ok(lines(&pts));
            }
            let segs = kernel::edge_path(&b.body, i)?;
            // The drawn edge sets which way t runs, as the picked edge's polyline has it.
            let (Some(first), Some(start)) = (pts.first(), segs.first().map(kernel::PathSeg::start)) else { return Ok(segs) };
            let end = segs.last().map(kernel::PathSeg::end).unwrap_or(start);
            Ok(if end.dist(*first) < start.dist(*first) { segs.iter().rev().map(kernel::PathSeg::reversed).collect() } else { segs })
        }
        GeoRef::SketchCurve { sketch, curve } => {
            let ss = ctx.st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
            // Evaluation has already mapped wires into this component's frame: their coordinates
            // are independent of the sketch plane.
            if let Some(wire) = ss.sketch.wires.iter().find(|wire| wire.id == *curve) {
                if let Some(arc) = &wire.arc {
                    let angle = solidcraft_sketch::arc_sweep(arc);
                    return Ok(vec![kernel::PathSeg::Arc { a: arc.start, center: arc.center, axis: arc.normal, angle }]);
                }
                if wire.fit.len() >= 3 {
                    let spline = kernel::PathPiece::Spline { fit: wire.fit.clone(), reversed: false };
                    if let Some(spans) = spline.spans()? {
                        return Ok(spans);
                    }
                }
                if let [a, b] = wire.fit.as_slice() {
                    return Ok(vec![kernel::PathSeg::Line { a: *a, b: *b }]);
                }
                if !wire.exact.is_empty() {
                    let mut segs = Vec::new();
                    for c in &wire.exact {
                        segs.extend(kernel::curve_path(&c.curve, c.interval)?);
                    }
                    return Ok(segs);
                }
                return Ok(lines(&wire.pts));
            }
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("curve `{curve}`")))?;
            Ok(ss.sketch.exact_pieces(ci).iter().map(|s| kernel::PathSeg::of_sketch(&ss.plane, s)).collect())
        }
        _ => Err(bad("pick an edge or sketch curve as the path")),
    }
}

/// The surface's unit normal at (the point of the surface nearest) `p`, and that point.
fn surface_normal(s: FaceSurface, p: Vec3) -> Option<(Vec3, Vec3)> {
    match s {
        FaceSurface::Plane { normal, point } => Some((p - normal * (p - point).dot(normal), *normal)),
        FaceSurface::Cylinder { origin, axis, radius } => {
            let w = p - origin;
            let r = (w - axis * w.dot(axis)).normalized()?;
            Some((origin + axis * w.dot(axis) + r * radius, r))
        }
        FaceSurface::Sphere { center, radius } => {
            let r = (p - center).normalized()?;
            Some((center + r * radius, r))
        }
        FaceSurface::Cone { apex, axis, half_angle } => {
            let w = p - apex;
            let radial = (w - axis * w.dot(axis)).normalized()?;
            // The generator through p's side, and the normal square to it in the axial plane.
            let line = axis * half_angle.cos() + radial * half_angle.sin();
            let n = (radial * half_angle.cos() - axis * half_angle.sin()).normalized()?;
            Some((apex + line * w.dot(line), n))
        }
        FaceSurface::Torus { center, axis, major, minor } => {
            let w = p - center;
            let radial = (w - axis * w.dot(axis)).normalized()?;
            let tube = center + radial * major;
            let n = (p - tube).normalized()?;
            Some((tube + n * minor, n))
        }
    }
}

/// A point over `s` where the face turns towards `towards` (a reference plane's normal): around
/// the axis of a cylinder, cone or torus, turned `angle` about it; straight at it on a sphere. Of
/// the two sides, the one `at` is on (#8822).
fn facing(s: FaceSurface, at: Vec3, towards: Vec3, angle: f64) -> Result<Vec3> {
    let side = |d: Vec3, w: Vec3| if d.dot(w) < 0.0 { d * -1.0 } else { d };
    let around = |axis: Vec3, w: Vec3| -> Result<(Vec3, f64)> {
        let d = (towards - axis * towards.dot(axis))
            .normalized()
            .ok_or_else(|| bad("the reference is square to the face's axis: every plane tangent to the face meets it at a right angle"))?;
        let d = d * angle.cos() + axis.cross(d) * angle.sin();
        let r = w - axis * w.dot(axis);
        Ok((side(d, r), r.len()))
    };
    if !angle.is_finite() {
        return Err(bad("the angle is not a number"));
    }
    Ok(match s {
        FaceSurface::Plane { .. } => at,
        FaceSurface::Cylinder { origin, axis, radius } => {
            let w = at - origin;
            let (d, _) = around(axis.into(), w)?;
            origin + axis * w.dot(axis) + d * radius
        }
        FaceSurface::Cone { apex, axis, .. } => {
            let w = at - apex;
            let (d, r) = around(axis.into(), w)?;
            apex + axis * w.dot(axis) + d * r.max(1e-6)
        }
        // On the outer equator, where the tube faces away from the axis.
        FaceSurface::Torus { center, axis, major, minor } => {
            let (d, _) = around(axis.into(), at - center)?;
            center + d * (major + minor)
        }
        FaceSurface::Sphere { center, radius } => {
            if angle.abs() > 1e-12 {
                return Err(bad("a sphere has no axis to turn the plane about: leave the angle out"));
            }
            center + side(towards, at - center) * radius
        }
    })
}

fn face_surface_at(ctx: &Ctx, g: &GeoRef) -> Result<(FaceSurface, Vec3)> {
    match g {
        GeoRef::Face { at, name } => {
            let (b, fi, q) = face_of(ctx.st, *at, name)?;
            kernel::face_surface(&b.body, fi).map(|s| (s, q)).ok_or_else(|| bad("the face's surface is not a plane, cylinder, cone, sphere or torus"))
        }
        GeoRef::Plane { .. } => {
            let pl = plane(ctx, g)?;
            Ok((FaceSurface::Plane { normal: pl.normal().unit().ok_or_else(|| bad("degenerate plane"))?, point: pl.origin }, pl.origin))
        }
        _ => Err(bad("pick a face")),
    }
}

/// Where two planes meet.
fn plane_plane(a: &Plane, b: &Plane) -> Option<(Vec3, Vec3)> {
    let (n1, n2) = (a.normal(), b.normal());
    let d = n1.cross(n2);
    let l2 = d.len2();
    if l2 < 1e-18 {
        return None;
    }
    let (h1, h2) = (n1.dot(a.origin), n2.dot(b.origin));
    let o = (n2.cross(d) * h1 + d.cross(n1) * h2) * (1.0 / l2);
    Some((o, d.normalized()?))
}

/// Closest points of two lines: their midpoint (where they meet, if they do).
fn line_line(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> Option<Vec3> {
    let ((p, u), (q, v)) = (a, b);
    let w = p - q;
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    let den = uu * vv - uv * uv;
    if den.abs() < 1e-12 {
        return None;
    }
    let s = (uv * vw - vv * uw) / den;
    let t = (uu * vw - uv * uw) / den;
    Some(((p + u * s) + (q + v * t)) * 0.5)
}

/// The bodies a construction built from `refs` lies on: each face, edge and vertex pick's body,
/// and the bodies of the constructions it is built from. Empty when any pick is not on a body
/// (a fixed point, the origin, a plane, a world axis, a sketch) or is not found: such a
/// construction is held by something a move of the bodies leaves where it is, so it stays.
pub(crate) fn source_bodies(ctx: &Ctx, refs: &[&GeoRef]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in refs {
        let found: Vec<String> = match g {
            GeoRef::Face { at, name } => face_of(ctx.st, *at, name).map(|(b, ..)| vec![b.definition_name().to_string()]).unwrap_or_default(),
            GeoRef::Edge { at, name } => edge_pick(ctx.st, *at, name).map(|(b, ..)| vec![b.definition_name().to_string()]).unwrap_or_default(),
            GeoRef::Vertex { .. } => {
                point(ctx, g).ok().and_then(|p| vertex_body(ctx.st, p)).map(|b| vec![b.definition_name().to_string()]).unwrap_or_default()
            }
            GeoRef::Snap { of, .. } => source_bodies(ctx, &[of]),
            GeoRef::Axis { name } | GeoRef::ConstructPoint { name } => {
                ctx.st.construct.iter().find(|c| c.name.as_str() == AsRef::<str>::as_ref(name)).map(|c| c.bodies.clone()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        if found.is_empty() {
            return Vec::new();
        }
        for b in found {
            if !out.contains(&b) {
                out.push(b);
            }
        }
    }
    out
}

/// The body with a vertex (an end of one of its edges) at `p`.
fn vertex_body(st: &ModelState, p: Vec3) -> Option<&ModelBody> {
    bodies(st).find(|b| {
        let tol = (b.body.size() * 1e-3).max(1e-3);
        b.body
            .edges(tol)
            .is_ok_and(|edges| edges.iter().flat_map(|e| [e.points.first(), e.points.last()]).flatten().any(|v| v.dist(p) <= 1e-6 * (1.0 + p.len())))
    })
}

pub(crate) fn resolve_axis(ctx: &Ctx, d: &AxisDef) -> Result<(Vec3, Vec3)> {
    match d {
        AxisDef::ThroughFace { face } => line(ctx, face),
        AxisDef::Edge { edge } => {
            let (o, dir) = line(ctx, edge)?;
            if let GeoRef::Edge { at, name } = edge
                && as_line(&edge_points(ctx.st, *at, name)?).is_none()
            {
                return Err(bad("the edge is not straight"));
            }
            Ok((o, dir))
        }
        AxisDef::NormalToFace { face, point: p } => {
            let (s, _) = face_surface_at(ctx, face)?;
            let q = point(ctx, p)?;
            let (on, n) = surface_normal(s, q).ok_or_else(|| bad("the point is on the face's axis"))?;
            Ok((on, n))
        }
        AxisDef::TwoPlanes { a, b } => plane_plane(&plane(ctx, a)?, &plane(ctx, b)?).ok_or_else(|| bad("the planes are parallel")),
        AxisDef::TwoPoints { a, b } => {
            let (p, q) = (point(ctx, a)?, point(ctx, b)?);
            Ok((p, (q - p).normalized().ok_or_else(|| bad("the two points coincide"))?))
        }
    }
}

pub(crate) fn resolve_point(ctx: &Ctx, d: &PointDef) -> Result<Vec3> {
    match d {
        PointDef::At { point: p } => point(ctx, p),
        PointDef::Center { of } => centre(ctx, of),
        PointDef::TwoEdges { a, b } => line_line(line(ctx, a)?, line(ctx, b)?).ok_or_else(|| bad("the edges are parallel")),
        PointDef::ThreePlanes { a, b, c } => {
            let (pa, pb, pc) = (plane(ctx, a)?, plane(ctx, b)?, plane(ctx, c)?);
            let (o, dir) = plane_plane(&pa, &pb).ok_or_else(|| bad("two of the planes are parallel"))?;
            line_plane(o, dir, &pc).ok_or_else(|| bad("the planes don't meet in a point"))
        }
        PointDef::EdgeAndPlane { edge, plane: p } => {
            let (o, dir) = line(ctx, edge)?;
            line_plane(o, dir, &plane(ctx, p)?).ok_or_else(|| bad("the edge is parallel to the plane"))
        }
        PointDef::AlongPath { path, t } => {
            let t = Document::eval_in(ctx.vals, t, Kind::Unitless)?;
            along(ctx, path, t).map(|(p, _)| p)
        }
    }
}

fn line_plane(o: Vec3, d: Vec3, p: &Plane) -> Option<Vec3> {
    let n = p.normal();
    let den = d.dot(n);
    if den.abs() < 1e-12 {
        return None;
    }
    Some(o + d * ((p.origin - o).dot(n) / den))
}

/// Planes built from model geometry (the `PlaneRef` kinds that need the model).
pub(crate) fn resolve_geo_plane(ctx: &Ctx, p: &PlaneRef) -> Result<Plane> {
    let degenerate = || bad("degenerate plane");
    match p {
        PlaneRef::Midplane { a, b } => {
            let (pa, pb) = (plane(ctx, a)?, plane(ctx, b)?);
            let (na, nb) = (pa.normal(), pb.normal());
            if na.cross(nb).len() < 1e-9 {
                // Parallel: halfway between, facing as the first.
                let half = nb * ((pb.origin - pa.origin).dot(nb) * 0.5);
                return Plane::new(pa.origin + half, pa.x, pa.y).ok_or_else(degenerate);
            }
            // Meeting at an angle: the plane through their line that halves the angle.
            let (o, dir) = plane_plane(&pa, &pb).ok_or_else(degenerate)?;
            let n = (na - nb).normalized().ok_or_else(degenerate)?;
            Plane::new(o, dir, n.cross(dir)).ok_or_else(degenerate)
        }
        PlaneRef::TwoEdges { a, b } => {
            let ((o1, d1), (o2, d2)) = (line(ctx, a)?, line(ctx, b)?);
            let n = match d1.cross(d2).normalized() {
                Some(n) if (o2 - o1).dot(n).abs() <= 1e-6 * (1.0 + (o2 - o1).len()) => n,
                Some(_) => return Err(bad("the edges are not in one plane")),
                None => d1.cross(o2 - o1).normalized().ok_or_else(|| bad("the edges are on one line"))?,
            };
            Plane::new(o1, d1, n.cross(d1)).ok_or_else(degenerate)
        }
        PlaneRef::AxisPoint { axis, point: at } => {
            let (origin, dir) = line(ctx, axis)?;
            let dir = dir.normalized().ok_or_else(|| bad("the axis has no direction"))?;
            let p = match point(ctx, at) {
                Ok(p) => {
                    let delta = p - origin;
                    let across = delta - dir * delta.dot(dir);
                    if across.len().is_finite() && across.len() >= 1e-9 {
                        p
                    } else if let GeoRef::Edge { at: pick, name } = at.as_ref() {
                        // When the centre of a circular edge lies on the axis, resolve to the picked point on the edge.
                        edge_point_at_pick(ctx.st, *pick, name).unwrap_or(p)
                    } else {
                        p
                    }
                }
                Err(e) => {
                    // Non-circular edges are not converted to centres by `point`; resolve to the picked edge point.
                    if let GeoRef::Edge { at: pick, name } = at.as_ref() {
                        edge_point_at_pick(ctx.st, *pick, name).ok_or(e)?
                    } else {
                        return Err(e);
                    }
                }
            };
            let delta = p - origin;
            let across = delta - dir * delta.dot(dir);
            if !across.len().is_finite() || across.len() < 1e-9 {
                return Err(bad("the point is on the axis: pick a point off the axis to define a unique plane"));
            }
            Plane::new(origin, dir, across).ok_or_else(degenerate)
        }
        PlaneRef::ThreePoints { a, b, c } => {
            let (pa, pb, pc) = (point(ctx, a)?, point(ctx, b)?, point(ctx, c)?);
            if (pb - pa).cross(pc - pa).len() < 1e-9 {
                return Err(bad("the three points are on one line"));
            }
            Plane::new(pa, pb - pa, pc - pa).ok_or_else(degenerate)
        }
        PlaneRef::Tangent { face, at, reference, angle } => {
            let (s, _) = face_surface_at(ctx, face)?;
            let at = match reference {
                Some(r) => {
                    let towards = plane(ctx, r)?.normal().normalized().ok_or_else(degenerate)?;
                    let angle = angle.as_ref().map(|a| Document::eval_in(ctx.vals, a, Kind::Angle)).transpose()?.unwrap_or(0.0);
                    facing(s, *at, towards, angle)?
                }
                None => *at,
            };
            let (on, n) = surface_normal(s, at).ok_or_else(|| bad("the point is on the face's axis"))?;
            Plane::from_normal(on, n).ok_or_else(degenerate)
        }
        PlaneRef::Perpendicular { plane: base, line: l } => {
            let pl = plane(ctx, base)?;
            let (o, d) = line(ctx, l)?;
            let n = d
                .cross(pl.normal())
                .normalized()
                .ok_or_else(|| bad("the line is square to the plane: every plane through it is perpendicular; pick a line in or along the plane"))?;
            Plane::new(o, d, n.cross(d)).ok_or_else(degenerate)
        }
        PlaneRef::AlongPath { path, t } => {
            let t = Document::eval_in(ctx.vals, t, Kind::Unitless)?;
            let (p, tan) = along(ctx, path, t)?;
            Plane::from_normal(p, tan).ok_or_else(degenerate)
        }
        PlaneRef::ThroughPoint { base, point: at } => {
            let b = ctx.doc.resolve_plane_in(ctx.vals, base, ctx.depth + 1, ctx.component, Some(ctx.st))?;
            let n = b.normal().normalized().ok_or_else(degenerate)?;
            Ok(Plane { origin: b.origin + n * (point(ctx, at)? - b.origin).dot(n), ..b })
        }
        _ => ctx.doc.resolve_plane_in(ctx.vals, p, ctx.depth + 1, ctx.component, Some(ctx.st)),
    }
}

impl GeoRef {
    /// Where a picked edge or face is now, found the way evaluation finds it (by its persistent
    /// name when it has one, else at the picked point): a point on it. `None` for other references and for an
    /// edge or face that is gone.
    pub fn picked_point(&self, st: &ModelState) -> Option<Vec3> {
        match self {
            GeoRef::Edge { at, name } => {
                let on = match name.as_deref() {
                    Some(n) => bodies(st).find_map(|b| crate::naming::find_edge(b, n, *at).ok())?,
                    None => *at,
                };
                edge_pick(st, on, &None).ok().map(|_| on)
            }
            GeoRef::Face { at, name } => face_of(st, *at, name).ok().map(|(_, _, p)| p),
            _ => None,
        }
    }

    /// Map the reference's world points (component frames).
    pub fn map_points(&mut self, pt: &mut dyn FnMut(&mut Vec3), plane: &mut dyn FnMut(&mut PlaneRef)) {
        match self {
            GeoRef::Point { p } => pt(p),
            GeoRef::Vertex { at, .. } | GeoRef::Edge { at, .. } | GeoRef::Face { at, .. } => pt(at),
            GeoRef::Plane { plane: p } => plane(p),
            GeoRef::Snap { of, at, .. } => {
                pt(at);
                of.map_points(pt, plane);
            }
            _ => {}
        }
    }

    /// Picked faces and edges get their persistent names, and picked vertices the names of the
    /// edges that meet there (only those still missing them).
    pub fn name_picks(&mut self, st: &ModelState) {
        match self {
            GeoRef::Face { at, name: name @ None } => {
                *name = crate::naming::face_names_at(st, &[*at]).into_iter().next().filter(|n| !n.is_empty());
            }
            GeoRef::Edge { at, name: name @ None } => {
                for b in bodies(st) {
                    let tol = (b.body.size() * 1e-3).max(1e-3);
                    if b.body.nearest_edge(*at, tol).ok().flatten().is_some_and(|(_, d)| d <= tol * 10.0) {
                        *name = crate::naming::edge_names_at(b, &[*at]).into_iter().next().filter(|n| !n.is_empty());
                        break;
                    }
                }
            }
            GeoRef::Vertex { at, edges: edges @ None } => {
                for b in bodies(st) {
                    let tol = (b.body.size() * 1e-3).max(1e-3);
                    let rec = b.body.record();
                    let ends_here = |e: &solidcore_record::brep::Edge| {
                        [e.vertices[0], e.vertices[1]]
                            .into_iter()
                            .filter_map(|v| rec.vertices.get(v as usize).map(|pt| Vec3::new(pt.point[0], pt.point[1], pt.point[2])))
                            .any(|q| q.dist(*at) <= tol)
                    };
                    let mut meet: Vec<String> =
                        rec.edges.iter().zip(crate::naming::edge_names(b)).filter(|(e, n)| !n.is_empty() && ends_here(e)).map(|(_, n)| n).collect();
                    if meet.is_empty() {
                        continue;
                    }
                    meet.sort();
                    meet.dedup();
                    if (2..=MAX_VERTEX_EDGES).contains(&meet.len()) {
                        *edges = Some(meet);
                    }
                    break;
                }
            }
            GeoRef::Plane { plane } => plane.name_picks(st),
            GeoRef::Snap { of, kind: SnapAt::Centre, at, end: end @ None } => {
                // Which end of a cylinder or cone: it stays that end when the face grows.
                *end = face_axis_ends(st, of).and_then(|ends| nearest(ends, *at)).map(|(i, _)| i);
                of.name_picks(st);
            }
            GeoRef::Snap { of, .. } => of.name_picks(st),
            _ => {}
        }
    }
}

impl AxisDef {
    pub fn refs(&self) -> Vec<&GeoRef> {
        match self {
            AxisDef::ThroughFace { face } => vec![face],
            AxisDef::Edge { edge } => vec![edge],
            AxisDef::NormalToFace { face, point } => vec![face, point],
            AxisDef::TwoPlanes { a, b } | AxisDef::TwoPoints { a, b } => vec![a, b],
        }
    }

    pub fn refs_mut(&mut self) -> Vec<&mut GeoRef> {
        match self {
            AxisDef::ThroughFace { face } => vec![face],
            AxisDef::Edge { edge } => vec![edge],
            AxisDef::NormalToFace { face, point } => vec![face, point],
            AxisDef::TwoPlanes { a, b } | AxisDef::TwoPoints { a, b } => vec![a, b],
        }
    }
}

impl PointDef {
    pub fn refs(&self) -> Vec<&GeoRef> {
        match self {
            PointDef::At { point } => vec![point],
            PointDef::Center { of } => vec![of],
            PointDef::TwoEdges { a, b } => vec![a, b],
            PointDef::ThreePlanes { a, b, c } => vec![a, b, c],
            PointDef::EdgeAndPlane { edge, plane } => vec![edge, plane],
            PointDef::AlongPath { path, .. } => vec![path],
        }
    }

    pub fn refs_mut(&mut self) -> Vec<&mut GeoRef> {
        match self {
            PointDef::At { point } => vec![point],
            PointDef::Center { of } => vec![of],
            PointDef::TwoEdges { a, b } => vec![a, b],
            PointDef::ThreePlanes { a, b, c } => vec![a, b, c],
            PointDef::EdgeAndPlane { edge, plane } => vec![edge, plane],
            PointDef::AlongPath { path, .. } => vec![path],
        }
    }
}

impl PlaneRef {
    /// The references of a plane built from model geometry.
    pub fn refs_mut(&mut self) -> Vec<&mut GeoRef> {
        match self {
            PlaneRef::Midplane { a, b } | PlaneRef::TwoEdges { a, b } | PlaneRef::AxisPoint { axis: a, point: b } => vec![a.as_mut(), b.as_mut()],
            PlaneRef::Perpendicular { plane, line } => vec![plane.as_mut(), line.as_mut()],
            PlaneRef::ThreePoints { a, b, c } => vec![a.as_mut(), b.as_mut(), c.as_mut()],
            PlaneRef::Tangent { face, reference, .. } => {
                let mut v = vec![face.as_mut()];
                v.extend(reference.as_deref_mut());
                v
            }
            PlaneRef::AlongPath { path, .. } => vec![path.as_mut()],
            PlaneRef::ThroughPoint { base, point } => {
                let mut refs = base.refs_mut();
                refs.push(point.as_mut());
                refs
            }
            PlaneRef::AtAngle { base, line, pivot, .. } => {
                let mut v = base.refs_mut();
                v.extend(line.as_deref_mut());
                v.extend(pivot.as_deref_mut());
                v
            }
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => base.refs_mut(),
            _ => Vec::new(),
        }
    }

    pub fn name_picks(&mut self, st: &ModelState) {
        for r in self.refs_mut() {
            r.name_picks(st);
        }
    }

    /// Does resolving it need the model (edges, faces, construction axes and points)?
    pub fn needs_model(&self) -> bool {
        let mut k = self.clone();
        !k.refs_mut().is_empty()
    }
}
