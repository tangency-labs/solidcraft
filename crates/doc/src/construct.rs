//! Construction geometry from the model: references to vertices, edges, faces, planes, axes,
//! points and sketch entities, resolved on each evaluation, and the axes, points and planes
//! built from them (the CONSTRUCT panel).
//!
//! Face and edge references carry the persistent name of what was picked (see `naming`), and a
//! vertex the names of the edges that meet there, so they follow the model when earlier features
//! change; the picked point is the fallback.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use solidcraft_geom::{Plane, Vec2, Vec3};
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
    /// A sketch point (by sketch feature id and point id).
    SketchPoint { sketch: u64, point: String },
    /// A sketch curve (by sketch feature id and curve id).
    SketchCurve { sketch: u64, curve: String },
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

/// The face picked: (body, face index, point on it).
fn face_of<'a>(st: &'a ModelState, at: Vec3, name: &Option<String>) -> Result<(&'a ModelBody, usize, Vec3)> {
    // The named face itself: its nearest point to the old pick can lie on an edge it shares
    // (a corner round that a parameter moved away from the pick), where picking again by the
    // point finds the flat face beside it (#1255).
    if let Some(hit) = name.as_deref().and_then(|n| crate::name_lookup::locate_face(st, n, at))
        && let Some(b) = st.bodies.get(hit.body).filter(|b| !b.body.is_mesh())
        && hit.face < b.body.face_count()
    {
        return Ok((b, hit.face, hit.point));
    }
    for b in bodies(st) {
        if let Some(fi) = crate::appearance::face_index_at(b, at) {
            return Ok((b, fi, at));
        }
    }
    Err(bad(format!("no face at {at:?}")))
}

/// The edge picked, as a polyline.
fn edge_points(st: &ModelState, at: Vec3, name: &Option<String>) -> Result<Vec<Vec3>> {
    edge_pick(st, at, name).map(|(_, _, pts)| pts)
}

/// The edge picked: its body, its index there (as `Body::edges` lists it) and its polyline.
fn edge_pick<'a>(st: &'a ModelState, at: Vec3, name: &Option<String>) -> Result<(&'a ModelBody, usize, Vec<Vec3>)> {
    let mut at = at;
    if let Some(n) = name {
        for b in bodies(st) {
            if let Ok((q, _)) = crate::naming::find_edge(b, n, at) {
                at = q;
                break;
            }
        }
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
    let ends: Vec<[Vec3; 2]> = names
        .iter()
        .map(|n| {
            // The edge with that name itself, never the one nearest the old pick, and its ends
            // on the body that has it: picked again by a point on it, a corner found the first
            // edge of any body through that corner, such as another body's edge running past
            // it, so a vertex took that edge's end (#8650).
            bodies(st).find_map(|b| {
                let hit = crate::name_lookup::locate_edge(b, n, at).ok()?;
                let m = b.mesh();
                let pts = m.edges.get(hit.edge)?;
                Some([*pts.first()?, *pts.last()?])
            })
        })
        .collect::<Option<_>>()?;
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

fn sketch_world(ctx: &Ctx, sketch: u64, q: Vec2) -> Result<Vec3> {
    let ss = ctx.st.sketch(sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    Ok(ss.plane.to_world(q))
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
            let ss = ctx.st.sketch(*sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
            let q =
                ss.sketch.point_index(point).and_then(|i| ss.sketch.point(i)).ok_or_else(|| DocError::Unknown(format!("sketch point `{point}`")))?;
            sketch_world(ctx, *sketch, q)
        }
        // A round edge or face: its centre.
        GeoRef::Edge { .. } | GeoRef::Face { .. } | GeoRef::SketchCurve { .. } => centre(ctx, g),
        _ => Err(bad("pick a point, vertex or sketch point")),
    }
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
        GeoRef::Plane { plane } => ctx.doc.resolve_plane_in(ctx.vals, plane, ctx.depth + 1, Some(ctx.st)),
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

/// Point and unit tangent at fraction `t` along a polyline.
fn along(pts: &[Vec3], t: f64) -> Option<(Vec3, Vec3)> {
    let total: f64 = pts.windows(2).map(|w| (w[1] - w[0]).len()).sum();
    if !(total > 0.0) {
        return None;
    }
    let mut left = t.clamp(0.0, 1.0) * total;
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).len();
        if l <= 0.0 {
            continue;
        }
        if left <= l {
            return Some((w[0] + (w[1] - w[0]) * (left / l), (w[1] - w[0]) * (1.0 / l)));
        }
        left -= l;
    }
    let (a, b) = (pts.get(pts.len() - 2)?, pts.last()?);
    Some((*b, (*b - *a).normalized()?))
}

fn path_points(ctx: &Ctx, g: &GeoRef) -> Result<Vec<Vec3>> {
    match g {
        GeoRef::Edge { at, name } => edge_points(ctx.st, *at, name),
        GeoRef::SketchCurve { sketch, curve } => sketch_curve_points(ctx, *sketch, curve),
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
            along(&path_points(ctx, path)?, t).map(|(p, _)| p).ok_or_else(|| bad("the path has no length"))
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
        PlaneRef::ThreePoints { a, b, c } => {
            let (pa, pb, pc) = (point(ctx, a)?, point(ctx, b)?, point(ctx, c)?);
            if (pb - pa).cross(pc - pa).len() < 1e-9 {
                return Err(bad("the three points are on one line"));
            }
            Plane::new(pa, pb - pa, pc - pa).ok_or_else(degenerate)
        }
        PlaneRef::Tangent { face, at } => {
            let (s, _) = face_surface_at(ctx, face)?;
            let (on, n) = surface_normal(s, *at).ok_or_else(|| bad("the point is on the face's axis"))?;
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
            let (p, tan) = along(&path_points(ctx, path)?, t).ok_or_else(|| bad("the path has no length"))?;
            Plane::from_normal(p, tan).ok_or_else(degenerate)
        }
        PlaneRef::ThroughPoint { base, point: at } => {
            let b = ctx.doc.resolve_plane_in(ctx.vals, base, ctx.depth + 1, Some(ctx.st))?;
            let n = b.normal().normalized().ok_or_else(degenerate)?;
            Ok(Plane { origin: b.origin + n * (point(ctx, at)? - b.origin).dot(n), ..b })
        }
        _ => ctx.doc.resolve_plane_in(ctx.vals, p, ctx.depth + 1, Some(ctx.st)),
    }
}

impl GeoRef {
    /// Where a picked edge or face is now, found the way evaluation finds it (by its persistent
    /// name, else at the picked point): a point on it. `None` for other references and for an
    /// edge or face that is gone.
    pub fn picked_point(&self, st: &ModelState) -> Option<Vec3> {
        match self {
            GeoRef::Edge { at, name } => {
                let on = name.as_deref().and_then(|n| bodies(st).find_map(|b| crate::naming::find_edge(b, n, *at).ok())).map_or(*at, |(q, _)| q);
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
                    let m = b.mesh();
                    let ends_here = |e: &Vec<Vec3>| [e.first(), e.last()].into_iter().flatten().any(|q| q.dist(*at) <= tol);
                    let mut meet: Vec<String> =
                        m.edges.iter().zip(crate::naming::edge_names(b)).filter(|(e, n)| !n.is_empty() && ends_here(e)).map(|(_, n)| n).collect();
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
            PlaneRef::Midplane { a, b } | PlaneRef::TwoEdges { a, b } => vec![a.as_mut(), b.as_mut()],
            PlaneRef::Perpendicular { plane, line } => vec![plane.as_mut(), line.as_mut()],
            PlaneRef::ThreePoints { a, b, c } => vec![a.as_mut(), b.as_mut(), c.as_mut()],
            PlaneRef::Tangent { face, .. } => vec![face.as_mut()],
            PlaneRef::AlongPath { path, .. } => vec![path.as_mut()],
            PlaneRef::ThroughPoint { base, point } => {
                let mut refs = base.refs_mut();
                refs.push(point.as_mut());
                refs
            }
            PlaneRef::AtAngle { base, line, .. } => {
                let mut v = base.refs_mut();
                v.extend(line.as_deref_mut());
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
