//! Edge fillets and chamfers. SMLib blends exactly the picked edges in one call
//! ([`crate::blend_smlib`]): the blend faces, the corners where blends meet (a sphere, a patch, a
//! bevel or a mitre, as its fillet corners choose) and the end faces rebuilt round them. A blend
//! whose contact lines would leave the faces it rolls on is SMLib's refusal, reported as
//! [`KernelError::TooLarge`]. A tangent chain is the caller's to select ([`tangent_chain`]).
//!
//! The helpers here read a body's schema 4 record: planar faces' outward normals, the direction
//! into a face across an edge, an edge's tangent at its ends.

use solidcraft_geom::prelude::*;
use std::collections::BTreeSet;

use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve, Loop, OrientedEdge, Shell, Surface};
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};

use crate::body::Body;
use crate::{KernelError, Result};

pub(crate) fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

/// A blend too large for the faces around it.
pub(crate) fn too_large(msg: &str) -> KernelError {
    KernelError::TooLarge(msg.into())
}

/// The note on a blend's refusal as too large that names the largest `size` (radius, distance)
/// that fits: "the largest radius that fits is 3 mm". The kernel's own sized refusals and a fillet
/// feature's search for the size ([`fillet_max`]) word it alike.
pub fn largest_fit_note(size: &str, largest: f64) -> String {
    format!("the largest {size} that fits is {} mm", trimmed_mm(largest))
}

/// Whether a refusal already names the largest size that fits ([`largest_fit_note`]).
pub fn names_largest_fit(why: &str) -> bool {
    why.contains("that fits is ")
}

/// `why` with the largest size that fits it names ([`largest_fit_note`]) replaced by `largest`:
/// solidcore names the size less a margin (34.99 where 35 builds), a feature's search the
/// largest it built. `why` itself where it names none.
pub fn with_fit_note(why: &str, size: &str, largest: f64) -> String {
    let Some(start) = why.find("the largest ") else { return why.to_owned() };
    let Some(len) = why.get(start..).and_then(|rest| rest.find(" mm")) else { return why.to_owned() };
    let (head, tail) = (why.get(..start).unwrap_or_default(), why.get(start + len + 3..).unwrap_or_default());
    format!("{head}{}{tail}", largest_fit_note(size, largest))
}

/// `why` without the largest size that fits it names ([`largest_fit_note`]): the clause
/// "the largest … that fits is … mm" and the "; " or ": " before it. `why` itself where it
/// names none.
pub fn without_fit_note(why: &str) -> String {
    let Some(start) = why.find("the largest ") else { return why.to_owned() };
    let Some(len) = why.get(start..).and_then(|rest| rest.find(" mm")) else { return why.to_owned() };
    let head = why.get(..start).unwrap_or_default();
    let head = head.strip_suffix("; ").or_else(|| head.strip_suffix(": ")).unwrap_or(head);
    format!("{head}{}", why.get(start + len + 3..).unwrap_or_default())
}

/// `v` to the micrometre, without trailing zeros.
pub(crate) fn trimmed_mm(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A body's record as the blends' checks read it: the record and each face's side in its shell.
pub(crate) struct Faces {
    pub(crate) rec: Brep,
    /// Whether each face's outward normal is its surface's natural normal (`None`: in no face shell).
    forward: Vec<Option<bool>>,
}

impl Faces {
    pub(crate) fn of(body: &Body) -> Result<Faces> {
        let rec = crate::native::brep_record(body)?;
        let mut forward = vec![None; rec.faces.len()];
        for sh in &rec.shells {
            if let Shell::Faces { faces, .. } = sh {
                for f in faces {
                    if let Some(slot) = forward.get_mut(f.face as usize) {
                        *slot = Some(f.forward);
                    }
                }
            }
        }
        Ok(Faces { rec, forward })
    }

    pub(crate) fn point(&self, v: u32) -> Option<Vec3> {
        self.rec.vertices.get(v as usize).map(|v| v3(v.point)).filter(|p| p.is_finite())
    }

    /// The points of the edge's start and end vertices.
    pub(crate) fn ends(&self, e: usize) -> Option<(Vec3, Vec3)> {
        let [a, b] = self.rec.edges.get(e)?.vertices;
        Some((self.point(a)?, self.point(b)?))
    }

    /// Whether the edge's curve is a straight line.
    pub(crate) fn straight(&self, e: usize) -> bool {
        self.rec.edges.get(e).is_some_and(|x| matches!(x.curve, Curve::Line { .. }))
    }

    /// The unit natural normal of a planar face's surface.
    fn natural_plane_normal(&self, f: usize) -> Option<Vec3> {
        let (du, dv) = match &self.rec.faces.get(f)?.surface {
            Surface::Plane { u_axis, v_axis, .. } => (v3(*u_axis), v3(*v_axis)),
            Surface::PlaneThrough { origin, u_point, v_point } => (v3(*u_point) - v3(*origin), v3(*v_point) - v3(*origin)),
            _ => return None,
        };
        du.cross(dv).normalized().filter(|n| n.is_finite())
    }

    /// The side of the face in its shell: 1 where its outward normal is its surface's natural one.
    fn side(&self, f: usize) -> Option<f64> {
        self.forward.get(f).copied().flatten().map(|fw| if fw { 1.0 } else { -1.0 })
    }

    /// The outward unit normal of a planar face, if planar.
    pub(crate) fn plane_normal(&self, f: usize) -> Option<Vec3> {
        Some(self.natural_plane_normal(f)? * self.side(f)?)
    }

    /// The outward unit normal of face `f` at `p` (a point within `accept` of its surface).
    pub(crate) fn normal_at(&self, f: usize, p: Vec3, accept: f64) -> Option<Vec3> {
        let s = SurfaceEval::new(self.rec.faces.get(f)?.parameters()).ok()?;
        let uv = s.project(p, None, accept)?;
        let (_, su, sv) = s.derivs(uv[0], uv[1])?;
        Some(su.cross(sv).normalized()? * self.side(f)?)
    }

    /// Every edge use in the face's loops.
    pub(crate) fn uses(&self, f: usize) -> impl Iterator<Item = &OrientedEdge> {
        self.rec.faces.get(f).into_iter().flat_map(|face| face.loops.iter()).flat_map(|l| match l {
            Loop::Edges { edges } => edges.as_slice(),
            Loop::Vertex { .. } => &[],
        })
    }

    /// The faces that use the edge, each once.
    pub(crate) fn faces_of(&self, e: usize) -> BTreeSet<u32> {
        self.rec.edges.get(e).map(|x| x.uses.iter().map(|u| u.face).collect()).unwrap_or_default()
    }

    /// The unit direction into planar face `f` across the straight edge of `u`, square to the
    /// edge: the face lies to the left of its loop seen from its surface's natural normal.
    pub(crate) fn direction_into_face(&self, f: usize, u: &OrientedEdge) -> Option<Vec3> {
        let (a, b) = self.ends(u.edge as usize)?;
        let dir = if u.forward { b - a } else { a - b };
        self.natural_plane_normal(f)?.cross(dir.normalized()?).normalized()
    }

    /// The edge's unit tangent along its interval (start to end vertex) at its start or end.
    pub(crate) fn tangent(&self, e: usize, at_end: bool) -> Option<Vec3> {
        let edge = self.rec.edges.get(e)?;
        curve_tangent(edge.parameters(), if at_end { edge.interval[1] } else { edge.interval[0] })
    }
}

/// The unit tangent of a record curve at `t`, along increasing `t`.
pub(crate) fn curve_tangent(c: &Curve, t: f64) -> Option<Vec3> {
    curve_derivative(c, t)?.normalized().filter(|d| d.is_finite())
}

/// The first derivative of a record curve at `t` (of an intersection curve: its unit tangent).
pub(crate) fn curve_derivative(c: &Curve, t: f64) -> Option<Vec3> {
    if !t.is_finite() {
        return None;
    }
    let d = match c {
        Curve::Line { start, end } => v3(*end) - v3(*start),
        Curve::Circle { frame, radius } => {
            let (s, co) = t.sin_cos();
            (v3(frame.y_axis) * co - v3(frame.x_axis) * s) * *radius
        }
        Curve::Ellipse { frame, radii } => {
            let (s, co) = t.sin_cos();
            v3(frame.y_axis) * (radii[1] * co) - v3(frame.x_axis) * (radii[0] * s)
        }
        Curve::Polyline { points } => {
            let last = points.len().checked_sub(2)?;
            let i = (t.max(0.0).floor() as usize).min(last);
            v3(*points.get(i + 1)?) - v3(*points.get(i)?)
        }
        Curve::Spline { .. } | Curve::RationalSpline { .. } => match CurveEval::new(c).ok()? {
            CurveEval::Spline(s) => solidcraft_geom::tess::from_kernel(s.eval(t)?.1),
            _ => return None,
        },
        Curve::SurfaceIntersection { surfaces, leader, .. } => {
            // Along both surfaces: square to both normals, on the leader's side.
            let p = CurveEval::new(c).ok()?.point(t)?;
            let normal = |s: &Surface| -> Option<Vec3> {
                let e = SurfaceEval::new(s).ok()?;
                let uv = e.project(p, None, f64::INFINITY)?;
                let (_, su, sv) = e.derivs(uv[0], uv[1])?;
                Some(su.cross(sv))
            };
            let along = normal(&surfaces[0])?.cross(normal(&surfaces[1])?);
            let along = along.normalized()?;
            if along.dot(curve_tangent(leader, t)?) < 0.0 { along * -1.0 } else { along }
        }
    };
    d.is_finite().then_some(d)
}

/// Curvature-continuous (G2) fillets: each section is SMLib's G2 blend curve between the contact
/// lines the round of `radius` has (zero curvature at the contacts on planar faces), not a
/// circle. A configuration SMLib cannot blend so is refused, never given a circular round.
pub fn fillet_g2(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_g2");
    body.require_brep("G2 fillet")?;
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid("G2 fillet size must be positive and finite".into()));
    }
    if edges.is_empty() || edges.len() > 1000 || edges.iter().any(|p| ![p.x, p.y, p.z].into_iter().all(f64::is_finite)) {
        return Err(KernelError::Invalid("G2 fillet needs 1 to 1000 finite edge points".into()));
    }
    let out = crate::blend_smlib::fillet_g2(body, edges, radius)?;
    closed(body, out)
}

/// A fillet's result, refused when a face's boundary crosses itself (#871): rounds that
/// overlap where a wall is thinner than both leave such faces, which the topology checks
/// don't see but which show as holes in the model. A fillet either closes the solid or is
/// refused. Faces of `input` that crossed already and that the fillet carried over unchanged
/// (a sliver an earlier boolean left far from the rounds, #1488) are not the fillet's doing.
fn closed(input: &Body, b: Body) -> Result<Body> {
    let after = b.crossed_faces()?;
    if after.is_empty() {
        return Ok(b);
    }
    let before = input.crossed_faces()?;
    let tol = (input.size().max(b.size()) * 1e-6).max(1e-9);
    match after.iter().filter(|p| !before.iter().any(|q| q.dist(**p) <= tol)).count() {
        0 => Ok(b),
        _ => Err(KernelError::Failed(
            "fillet: the rounds would leave openings in the surface (they overlap where a wall or a face is narrower than both)".into(),
        )),
    }
}

/// How two constant rounds of one radius turn round a sharp, straight, convex side edge square to
/// both on a plane, such as the rounds along a boss's base at its upright edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilletTurn {
    /// The rounds run on to where they cross and meet in a mitre.
    #[default]
    Mitre,
    /// The rounds end at the side edge and the corner is their section revolved about it, a
    /// torus tangent to both. Refused where it is not built (unequal or variable rounds, a
    /// concave, curved or tangent side edge, a corner of four edges), with the reason.
    Swept,
}

/// Constant-radius fillet of the edges nearest to the given points. A radius too large for the
/// faces around the edges is refused with [`KernelError::TooLarge`] ([`fillet_max`] finds the
/// largest that works).
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    fillet_turning(body, edges, radius, FilletTurn::Mitre)
}

/// [`fillet`], its rounds turning at side edges as `turn` says.
fn fillet_turning(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet");
    body.require_brep("fillet")?;
    crate::blend_smlib::fillet(body, edges, radius, turn).and_then(|b| closed(body, b))
}

/// [`fillet`], its rounds turning at side edges as `turn` says, with the picks whose edges are
/// gone: where some of the rounds overflow their faces, those are built first, and a picked edge
/// one of them removes whole is not rounded (the others round what is left of their edges).
pub fn fillet_removing(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<(Body, Vec<Vec3>)> {
    let _span = solidcraft_profile::span("kernel", "fillet");
    body.require_brep("fillet")?;
    match crate::blend_smlib::fillet_removing(body, edges, radius, turn) {
        Ok((b, gone)) => Ok((closed(body, b)?, gone)),
        Err(KernelError::TooLarge(why)) => match edges {
            [pick] => match concave_overflow_round(body, *pick, radius, |carried| crate::blend_smlib::fillet(carried, &[*pick], radius, turn)) {
                Some(b) => Ok((b, Vec::new())),
                None => Err(KernelError::TooLarge(why)),
            },
            _ => Err(KernelError::TooLarge(why)),
        },
        Err(e) => Err(e),
    }
}

/// A concave blend (a round or a chamfer) on one straight edge whose contact line runs off the
/// flat face it rolls on (the face's outline turns away there, into a rounded corner or an
/// opening), which SMLib refuses (solidcore#3252): `blend` of the edge on the body with that face
/// carried on as a slab behind its plane, over the edge's length, and of the material that blend
/// adds, what stands over the face's outline (its sheet thickened along its normal) and what
/// stands past the edge's line, on the other face's side of it (a box on that side of the line,
/// over the edge's length), joined to the body. So the blend ends where the face's outline ends,
/// on the surface of whatever the face turns into there, carried on square to the face, and is
/// whole where it stands over the other face (leaning back, or curving away behind the edge's
/// line). The sheet's prism and the box reach as far either way from the plane, and the box as
/// far from the edge's line, as the added material's own exact box is across, and a margin more:
/// that material touches the edge, so none of it stands further off. `reach`: how far the blend
/// reaches from the edge (a round's radius, a chamfer's larger distance), which sizes the slab.
///
/// Only where it gives the blend SMLib would build on an unbounded face: the edge is straight,
/// one of its faces is planar and it is concave (material in front of that face on the other
/// face's side, and in its plane on one side of the edge's line only, the other face's), and
/// at each end of the edge a planar face square to it ends the blend, so the
/// slab's square ends add no material the blend could run into. `None` where it does not apply
/// or a step fails: the caller keeps SMLib's refusal. Decided on exact geometry. Its steps are
/// checked as they are built whatever the evaluation checks, also under `Validation::Off`.
fn concave_overflow_round(body: &Body, pick: Vec3, reach: f64, blend: impl Fn(&Body) -> Result<Body>) -> Option<Body> {
    // Each step checked as it is built: one whose result the check refuses gives `None`, so the
    // caller keeps the kernel's refusal rather than a body the feature's check refuses later.
    crate::validation::validating(crate::Validation::EachStep, || carried_on_round(body, pick, reach, blend)).0
}

/// [`concave_overflow_round`]'s steps.
fn carried_on_round(body: &Body, pick: Vec3, reach: f64, blend: impl Fn(&Body) -> Result<Body>) -> Option<Body> {
    use solidcore_record::{Loop2, Plane, Region2, Vec2};
    if !(reach.is_finite() && reach > 0.0) {
        return None;
    }
    let rec = Faces::of(body).ok()?;
    let (picked, _) = crate::blend_smlib::picked_edges(body, &[pick]).ok()?;
    let [e] = picked.as_slice() else { return None };
    let e = *e;
    if !rec.straight(e) {
        return None;
    }
    let (a, b) = rec.ends(e)?;
    let length = a.dist(b);
    let d = (b - a).normalized()?;
    let size = body.size();
    if !(length > 0.0 && size.is_finite() && size > 0.0) {
        return None;
    }
    let eps = (size * 1e-4).max(1e-4).min(reach * 0.1);
    let middle = (a + b) * 0.5;
    let [vs, ve] = rec.rec.edges.get(e)?.vertices;
    // A planar face square to the edge, other than its own, at vertex v.
    let squared_at = |v: u32| {
        rec.rec.edges.iter().enumerate().filter(|(_, x)| x.vertices.contains(&v)).any(|(i, _)| {
            rec.faces_of(i)
                .into_iter()
                .any(|f| !rec.faces_of(e).contains(&f) && rec.plane_normal(f as usize).is_some_and(|n| n.dot(d).abs() > 1.0 - 1e-9))
        })
    };
    if !(squared_at(vs) && squared_at(ve)) {
        return None;
    }
    for f in rec.faces_of(e) {
        let f = f as usize;
        let Some(n) = rec.plane_normal(f) else { continue };
        if n.dot(d).abs() > 1e-9 {
            continue;
        }
        let u = n.cross(d);
        // concave: material in front of the plane beside the edge, on the other face's side
        let inside = |p: Vec3| matches!(body.classify_point(p), Ok(crate::PointClass::Inside));
        if !(inside(middle + n * eps + u * eps) || inside(middle + n * eps - u * eps)) {
            continue;
        }
        // The side of the edge's line, in the plane, that the other face stands on: the
        // material is there, and the flat face lies on the other.
        let along_u = match (inside(middle + u * eps), inside(middle - u * eps)) {
            (true, false) => true,
            (false, true) => false,
            _ => continue,
        };
        let far = 4.0 * (size + reach);
        let plane = Plane { origin: a, x: d, y: u };
        let rect = Loop2::polygon(&[Vec2::new(0.0, -far), Vec2::new(length, -far), Vec2::new(length, far), Vec2::new(0.0, far)]);
        let slab = crate::build::extrude(&plane, &[Region2 { outer: rect, holes: vec![] }], -far, 0.0).ok()?.into_iter().next()?;
        let carried = crate::boolean(body, &slab, crate::BoolOp::Union).ok()??;
        let rounded = blend(&carried).ok()?;
        let fill = crate::boolean(&rounded, &carried, crate::BoolOp::Cut).ok()??;
        // the face itself, by its index, for its sheet
        let sheet = crate::surfaces::taken(body, &rec.rec, &[(f, false)]).ok()?;
        // The fill touches the edge, so it stands no further off the face's plane or the edge's
        // line than its own box is across.
        let bounds = fill.exact_bounds().ok()?;
        let room = bounds.min.dist(bounds.max) + size * 1e-3 + 1.0;
        let prism = crate::thicken(&sheet, 2.0 * room, true).ok()?;
        // Past the edge's line the fill stands over the other face, not over the flat face's
        // outline, and is kept whole.
        let (from, to) = if along_u { (0.0, room) } else { (-room, 0.0) };
        let beyond = Loop2::polygon(&[Vec2::new(0.0, from), Vec2::new(length, from), Vec2::new(length, to), Vec2::new(0.0, to)]);
        let beyond = crate::build::extrude(&plane, &[Region2 { outer: beyond, holes: vec![] }], -room, room).ok()?.into_iter().next()?;
        let keep = crate::boolean(&prism, &beyond, crate::BoolOp::Union).ok()??;
        let kept = crate::boolean(&fill, &keep, crate::BoolOp::Intersect).ok()??;
        let joined = crate::boolean(body, &kept, crate::BoolOp::Union).ok()??;
        let joined = closed(body, joined).ok()?;
        let grew = |x: &Body| crate::measure(x).ok().map(|m| m.volume);
        if grew(&joined)? > grew(body)? {
            return Some(over_body_alone(joined, body, e));
        }
    }
    None
}

/// What a refusal of a setback too short for its corner says before the least that builds (in
/// mm): SMLib certifies the corner's fill convex only from that setback on ([`fillet_setback`]).
pub const SETBACK_AT_LEAST: &str = "the smallest setback that builds is ";

/// [`fillet`] with corner setbacks: each `(at, distance)` sets back the corner at the vertex
/// nearest `at`, a vertex where every edge is rounded, so its patch starts `distance` along
/// each of its edges from the vertex (one distance for all of a corner's edges: SMLib keeps one
/// per corner). The rounds run whole up to their sections there, and the patch between them
/// meets each face in a curve from one round's contact line to the next. The distance must reach
/// past where the rounds would meet without it (past the radius at a square corner); a corner
/// where some edge is not rounded is refused as not supported, a setback as long as an edge at
/// its corner as too large.
pub fn fillet_setback(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_setback");
    body.require_brep("fillet")?;
    setbacks_reach(body, edges, radius, setbacks)?;
    crate::blend_smlib::fillet_setback(body, edges, radius, setbacks).and_then(|b| closed(body, b))
}

/// `joined` (the union of `body` and the material a round of its edge `edge` adds, its history
/// over those two) with its history over `body` alone, as a fillet reports it: a face or an
/// edge of the body stays that; a face only the added material brought is made in place of
/// the edge, and an edge where such a face meets a face of the body is where the edge's round
/// meets that face. Without a history where the union reported none or the rewritten one does
/// not describe the body: a result is never given a guessed history.
fn over_body_alone(joined: Body, body: &Body, edge: usize) -> Body {
    use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History, Side};
    let Some(union) = joined.history().filter(|h| h.operands == 2).cloned() else { return joined.without_history() };
    let round = FaceOrigin::Edge { operand: 0, edge };
    // An origin in the body stays; one in the added material is the round.
    let of_body = |o: &FaceOrigin| match *o {
        FaceOrigin::Face { operand, .. } | FaceOrigin::Edge { operand, .. } | FaceOrigin::Offset { operand, .. } => operand == 0,
        FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => false,
    };
    let faces = union
        .faces
        .iter()
        .map(|f| {
            let origins: Vec<FaceOrigin> = f.origins.iter().filter(|o| of_body(o)).copied().collect();
            if origins.is_empty() {
                return FaceProv { origins: vec![round], change: Change::Generated };
            }
            // A face of the body the added material's face was joined to is that face, grown.
            let change = if f.change == Change::Merged && origins.len() == 1 { Change::Modified } else { f.change };
            FaceProv { origins, change }
        })
        .collect();
    let side = |s: &Side| Side::new(s.origins().iter().map(|o| if of_body(o) { *o } else { round }));
    let edges = union
        .edges
        .iter()
        .map(|e| {
            let origins = e
                .origins
                .iter()
                .filter_map(|o| match o {
                    EdgeOrigin::Edge { operand: 0, .. } => Some(o.clone()),
                    EdgeOrigin::Edge { .. } => None,
                    EdgeOrigin::Meet { a, b } => match (side(a), side(b)) {
                        (Some(a), Some(b)) if a != b => Some(EdgeOrigin::meet(a, b)),
                        _ => None,
                    },
                })
                .collect();
            EdgeProv { faces: e.faces.clone(), origins }
        })
        .collect();
    let history = History { operands: 1, faces, edges };
    match history.check_counts((joined.face_count(), joined.edge_count()), &crate::record::counts(&[body])) {
        Ok(()) => joined.clone().with_history_counts(history, &crate::record::counts(&[body])).unwrap_or_else(|_| joined.without_history()),
        Err(_) => joined.without_history(),
    }
}

/// Refuses a setback that does not reach past where its corner's rounds meet without one
/// ([`setback_minimum`]), naming the least it must exceed, at a corner where every edge is one
/// of `edges` (the others are SMLib's to refuse). SMLib fails there with its own status
/// (smlib_blend_setback): its patch does not join contact lines that already cross.
fn setbacks_reach(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<()> {
    let faces = Faces::of(body)?;
    let (picked, _) = crate::blend_smlib::picked_edges(body, edges)?;
    let picked: BTreeSet<usize> = picked.into_iter().collect();
    for (at, distance) in setbacks {
        let Some(v) = corner_vertex(body, &faces, *at) else { continue };
        let all_rounded = (0..faces.rec.edges.len())
            .filter(|e| faces.rec.edges.get(*e).is_some_and(|x| !x.uses.is_empty() && x.vertices.contains(&v)))
            .all(|e| picked.contains(&e));
        if !all_rounded {
            continue;
        }
        let Some(least) = setback_minimum(&faces, v, radius) else { continue };
        if *distance <= least * (1.0 + 1e-9) {
            let p = faces.point(v).unwrap_or(*at);
            let shown = format!("{:.4}", least).trim_end_matches('0').trim_end_matches('.').to_string();
            let why =
                if (least - radius).abs() <= radius * 1e-9 { format!("more than the radius ({shown} mm)") } else { format!("more than {shown} mm") };
            return Err(KernelError::Invalid(format!(
                "the setback must be {why} at the corner ({:.3}, {:.3}, {:.3}): the rounds' contact lines meet {shown} mm from it along its edges, \
                 and the corner's patch starts past that",
                p.x, p.y, p.z
            )));
        }
    }
    Ok(())
}

/// The least setback the corner at the vertex nearest `at` takes with rounds of `radius`
/// ([`setback_minimum`]): a setback there must be more than this, or [`fillet_setback`] and
/// [`fillet_g2_corners`] refuse it. `None` where no vertex is near `at`, or at a corner whose
/// least SMLib decides (one with a curved face or edge).
pub fn fillet_least_setback(body: &Body, at: Vec3, radius: f64) -> Result<Option<f64>> {
    body.require_brep("fillet")?;
    let faces = Faces::of(body)?;
    Ok(corner_vertex(body, &faces, at).and_then(|v| setback_minimum(&faces, v, radius)))
}

/// The vertex a setback at `at` names: the nearest, within the reach the setbacks take.
fn corner_vertex(body: &Body, faces: &Faces, at: Vec3) -> Option<u32> {
    let reach = body.size() * 0.05 + 1e-3;
    (0..faces.rec.vertices.len())
        .filter_map(|v| u32::try_from(v).ok())
        .filter_map(|v| faces.point(v).map(|p| (p.dist(at), v)))
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, v)| v)
}

/// The least setback a corner of rounds of `radius` at `vertex` takes: on each face at the
/// corner, the contact lines (rails) of the two rounded edges beside it cross, and a setback
/// must reach past that crossing along both edges (smlib_blend_setback). A rail lies
/// r tan(psi / 2) from its edge, psi the angle between the outward normals of the edge's two
/// faces (r on faces square to each other); the rails of edges at angle alpha on a face, d1 and
/// d2 from them, cross (d2 + d1 cos alpha) / sin alpha along the first edge from the vertex.
/// Exact for planar faces and straight edges; `None` for a corner with any other (left to SMLib)
/// or one it cannot read.
fn setback_minimum(faces: &Faces, vertex: u32, radius: f64) -> Option<f64> {
    let at: Vec<usize> =
        (0..faces.rec.edges.len()).filter(|e| faces.rec.edges.get(*e).is_some_and(|x| !x.uses.is_empty() && x.vertices.contains(&vertex))).collect();
    if at.len() < 3 {
        return None;
    }
    // Each edge's direction away from the vertex and its rail's distance from it.
    let mut away = Vec::with_capacity(at.len());
    let mut rail = Vec::with_capacity(at.len());
    for e in &at {
        let [a, b] = faces.rec.edges.get(*e)?.vertices;
        if a == b || !faces.straight(*e) {
            return None;
        }
        let (pa, pb) = faces.ends(*e)?;
        away.push(if a == vertex { (pb - pa).normalized()? } else { (pa - pb).normalized()? });
        let [f, g]: [u32; 2] = faces.faces_of(*e).into_iter().collect::<Vec<_>>().try_into().ok()?;
        let cos = faces.plane_normal(f as usize)?.dot(faces.plane_normal(g as usize)?).clamp(-1.0, 1.0);
        rail.push(radius * (cos.acos() / 2.0).tan());
    }
    let mut least: f64 = 0.0;
    let around: BTreeSet<u32> = at.iter().flat_map(|e| faces.faces_of(*e)).collect();
    for f in around {
        let beside: Vec<usize> = (0..at.len()).filter(|i| at.get(*i).is_some_and(|e| faces.faces_of(*e).contains(&f))).collect();
        let [i, j] = beside[..] else { return None };
        let cos = away.get(i)?.dot(*away.get(j)?).clamp(-1.0, 1.0);
        let sin = (1.0 - cos * cos).sqrt();
        if sin <= 1e-9 {
            return None;
        }
        let (di, dj) = (*rail.get(i)?, *rail.get(j)?);
        least = least.max((dj + di * cos) / sin).max((di + dj * cos) / sin);
    }
    (least.is_finite() && least > 0.0).then_some(least)
}

/// [`fillet`] whose corners where three or more rounded edges meet are G2 patches
/// ([`crate::corner_g2`]): each such corner is set back, `setbacks` naming a corner by a point at
/// its vertex and the distance its patch starts along each edge there (the others take
/// [`crate::DEFAULT_SETBACK_FACTOR`] times the radius), SMLib leaves it open, and a patch that
/// meets the rounds and the faces round it to the second order fills it. The faces are sewn
/// into one solid, convex corners and concave ones alike. Without such a corner this is
/// [`fillet`]. Refused: a corner where an edge is not rounded, or whose edges are not all of one
/// convexity (SMLib sets back only corners where all are rounded, alike), a setback that does not reach past
/// where the rounds would meet, a hole that is no height field over its plane or a patch that
/// does not meet its neighbours within the tolerances.
pub fn fillet_g2_corners(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
    g2_corners(body, edges, radius, setbacks, false)
}

/// [`fillet_g2`] (curvature-continuous sections) whose corners where three or more rounded edges
/// meet are G2 patches, as [`fillet_g2_corners`] builds them: SMLib's own patch at such a corner
/// is only tangent to the rounds (its curvature jumps across their seams), which no
/// curvature-continuous fillet may leave. Without such a corner this is [`fillet_g2`]. Refused
/// as [`fillet_g2_corners`] is.
pub fn fillet_g2_with_g2_corners(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
    g2_corners(body, edges, radius, setbacks, true)
}

/// [`fillet_g2_corners`] (circular sections) or [`fillet_g2_with_g2_corners`] (`curvature`).
fn g2_corners(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)], curvature: bool) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_g2_corners");
    body.require_brep("fillet")?;
    if !(radius.is_finite() && radius > 0.0) {
        return Err(KernelError::Invalid("the blend radius must be finite and positive".into()));
    }
    let (picked, _) = crate::blend_smlib::picked_edges(body, edges)?;
    let rec = body.record();
    let picked: BTreeSet<usize> = picked.into_iter().collect();
    // Each vertex's edges (a closed edge's one vertex once).
    let mut at: std::collections::BTreeMap<u32, BTreeSet<usize>> = std::collections::BTreeMap::new();
    for (i, e) in rec.edges.iter().enumerate() {
        if e.uses.is_empty() {
            continue;
        }
        for v in e.vertices {
            at.entry(v).or_default().insert(i);
        }
    }
    let point = |v: u32| rec.vertices.get(v as usize).map(|x| v3(x.point));
    let mut corners: Vec<Vec3> = Vec::new();
    for (v, around) in &at {
        let rounded = around.iter().filter(|e| picked.contains(e)).count();
        if rounded < 3 {
            continue;
        }
        let p = point(*v).ok_or_else(|| KernelError::Failed("fillet: a vertex reference".into()))?;
        if rounded < around.len() {
            return Err(unsupported(&format!(
                "a G2 corner at ({:.3}, {:.3}, {:.3}) where an edge is not rounded (the corner is set back, and SMLib sets back only a corner where every edge is)",
                p.x, p.y, p.z
            )));
        }
        corners.push(p);
    }
    if corners.is_empty() {
        return if curvature { fillet_g2(body, edges, radius) } else { fillet(body, edges, radius) };
    }
    // Each corner's setback: the one given at its vertex, else the default.
    let reach = body.size() * 0.05 + 1e-3;
    let mut distance = vec![crate::corner_g2::DEFAULT_SETBACK_FACTOR * radius; corners.len()];
    for (p, d) in setbacks {
        let k = corners
            .iter()
            .enumerate()
            .map(|(k, c)| (c.dist(*p), k))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, k)| k)
            .ok_or_else(|| {
                KernelError::Invalid(format!(
                    "the setback at ({:.3}, {:.3}, {:.3}) names no G2 corner (a vertex where three or more rounded edges meet)",
                    p.x, p.y, p.z
                ))
            })?;
        if let Some(slot) = distance.get_mut(k) {
            *slot = *d;
        }
    }
    let set_back: Vec<(Vec3, f64)> = corners.iter().copied().zip(distance).collect();
    setbacks_reach(body, edges, radius, &set_back)?;
    let open = crate::blend_smlib::fillet_setback_open(body, edges, radius, &set_back, curvature)?;
    let mut patches = Vec::with_capacity(open.holes.len());
    for hole in &open.holes {
        let (patch, _) = crate::corner_g2::corner_patch(&open.sheet, hole, radius)?;
        patches.push(patch);
    }
    let mut parts: Vec<&Body> = vec![&open.sheet];
    parts.extend(patches.iter());
    let sewn = crate::stitch(&parts, 0.0)?;
    let open_shells = sewn.validity();
    if !open_shells.is_empty() {
        return Err(KernelError::Failed(format!("fillet: the G2 corners do not close the body: {}", open_shells.join("; "))));
    }
    let history = corner_history(&sewn, body, &open.sheet, &patches);
    closed(body, sewn.with_checked_history(history, &crate::record::counts(&[body])))
}

/// The history of the sewn G2-cornered fillet over `[body]`: the sewing's history (over the
/// sheet and the patches) carried through the sheet's (over the body); each patch face made from
/// the edges whose rounds meet at its corner, as SMLib's own corner patch is, each of its edges
/// the sheet's.
fn corner_history(sewn: &Body, body: &Body, sheet: &Body, patches: &[Body]) -> Option<crate::history::History> {
    use crate::history::{EdgeOrigin, FaceOrigin};
    let outer = sewn.history()?.clone();
    let inner = sheet.history()?;
    let mut parts = vec![inner.clone()];
    for patch in patches {
        let own = patch.history()?;
        // The patch's face comes from the rounded edges the sheet's faces beside it come from.
        let mut origins: BTreeSet<FaceOrigin> = BTreeSet::new();
        for e in &own.edges {
            for o in &e.origins {
                let EdgeOrigin::Edge { edge, .. } = o else { continue };
                for f in inner.edges.get(*edge).map(|x| x.faces.as_slice()).unwrap_or_default() {
                    let beside = inner.faces.get(*f).map(|x| x.origins.as_slice()).unwrap_or_default();
                    origins.extend(beside.iter().copied().filter(|g| matches!(g, FaceOrigin::Edge { .. })));
                }
            }
        }
        let mut h = own.clone().substituted(0, inner).ok()?;
        for f in &mut h.faces {
            *f = crate::record::generated(origins.iter().copied().collect());
        }
        parts.push(h);
    }
    let h = outer.over(&parts.iter().collect::<Vec<_>>()).ok()?;
    h.check_counts((sewn.face_count(), sewn.edge_count()), &crate::record::counts(&[body])).ok()?;
    Some(h)
}

/// The step of the radius search: three significant digits of the requested radius.
fn radius_step(radius: f64) -> f64 {
    10f64.powf(radius.log10().floor() - 3.0)
}

/// The largest radius below `radius` (to three significant digits) at which the fillet of
/// these edges works, with its result; `Ok(None)` when none does. Each candidate is a real fillet
/// of the edges, so the radius returned is one that was built; the search runs up to 16 of
/// them, plus up to 6 led by the sizes the refusals name (a refusal of a fillet feature's exact
/// fit runs it to name the largest that fits, and so does `fit: reduce`). Only a refusal as too
/// large ([`KernelError::TooLarge`]) narrows the search: any other failure of a candidate is the
/// search's error, since a smaller radius would not be the largest that fits. The rounds turn at
/// side edges as `turn` says.
pub fn fillet_max(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Option<(f64, Body)>> {
    fillet_max_why(body, edges, radius, turn, None).map(|search| search.found)
}

/// [`fillet_max`], with what the search found when nothing builds ([`FitSearch`]). `hint`: the
/// largest radius the refusal of `radius` named ([`named_fit`]), tried first.
pub fn fillet_max_why(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn, hint: Option<f64>) -> Result<FitSearch> {
    let _span = solidcraft_profile::span("kernel", "fillet_max");
    largest_that_fits(radius, hint, |r| fillet_turning(body, edges, r, turn))
}

/// [`fillet_max`] for an equal-distance chamfer of these edges: the largest distance below
/// `distance` (to three significant digits) at which it works, with its result.
pub fn chamfer_max(body: &Body, edges: &[Vec3], distance: f64) -> Result<Option<(f64, Body)>> {
    chamfer_max_why(body, edges, distance, None).map(|search| search.found)
}

/// [`chamfer_max`], with what the search found when nothing builds ([`FitSearch`]). `hint` as
/// for [`fillet_max_why`].
pub fn chamfer_max_why(body: &Body, edges: &[Vec3], distance: f64, hint: Option<f64>) -> Result<FitSearch> {
    let _span = solidcraft_profile::span("kernel", "chamfer_max");
    largest_that_fits(distance, hint, |d| chamfer(body, edges, d))
}

/// The largest size in mm a refusal names ([`largest_fit_note`]): 3 for "...; the largest radius
/// that fits is 3 mm". `None` where it names none, or names a scale ("2 times the distances").
pub fn named_fit(why: &str) -> Option<f64> {
    let (_, rest) = why.split_once("that fits is ")?;
    let (number, _) = rest.split_once(" mm")?;
    number.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

/// What [`largest_that_fits`] found: the largest size it built, with its result, or (none built)
/// the smallest size it tried and that candidate's own refusal, so a caller can say why nothing
/// smaller builds instead of repeating the first refusal's estimate.
pub struct FitSearch {
    /// The largest size built, with its result.
    pub found: Option<(f64, Body)>,
    /// When nothing built: the smallest size tried and how it failed.
    pub smallest_refused: Option<(f64, KernelError)>,
    /// How many candidates were built or refused (the search's work).
    pub tried: usize,
}

/// The largest size n·q below `size` (q: three significant digits of it) at which `blend` works,
/// over a bounded number of blends. A candidate refused as too large or failing as an operation
/// (its check refusing the result, say) narrows the search: a size that does not build is not one
/// that fits, so the size found is one that was built, and (unless the cap stops the search) the
/// next size up, n·q + q, was refused. With none built, the smallest size tried and its failure are
/// kept ([`FitSearch::smallest_refused`]). Bad input and the kernel's own faults are errors.
///
/// The search brackets the largest size between the largest built (`lo`) and the smallest refused
/// (`hi`), and each candidate narrows the bracket, by bisection where nothing better is known. A
/// refusal as too large names the largest size it estimates fits ([`named_fit`]; `hint` for the
/// request's own refusal), usually within a step or two of it: that size is tried next (unless it
/// is within 1% of the size refused, which says nothing of where the largest lies), and once
/// it builds the sizes above it are tried 1, 2, 4, ... steps up until one is refused, so a
/// close estimate settles the search in two or three blends where bisection takes eleven or
/// more. Where building is monotone in the size (everything below the largest that fits builds),
/// the size found is the one bisection finds; it never depends on how the bracket narrowed.
fn largest_that_fits(size: f64, hint: Option<f64>, blend: impl Fn(f64) -> Result<Body>) -> Result<FitSearch> {
    if !(size.is_finite() && size > 1e-6) {
        return Ok(FitSearch { found: None, smallest_refused: None, tried: 0 });
    }
    let q = radius_step(size);
    // Sizes n·q for n in [0, hi): hi is too large (it is the request, or above it), lo works.
    let mut hi = (size / q).ceil() as u64;
    let mut lo = 0u64;
    let mut best: Option<Body> = None;
    let mut smallest_refused: Option<(f64, KernelError)> = None;
    // The step on the grid at or below an estimate (a hair over, so 1.24 / 0.001 is 1240), for an
    // estimate that says where the largest size lies: one at least 1% below the size refused. An
    // estimate a margin below it (2.99 for 3: solidcore names the size less a margin where the
    // ball is as wide as a face it rolls on) says only that the size was refused, and the blend
    // just under such a limit can be the slowest to try: on the engine fuzzer's seed-2 design a
    // 2.99 mm round ran 834 s before it failed, where bisection's 1.5 mm took 2 s.
    let lead = |estimate: f64, refused: f64| (estimate < 0.99 * refused).then(|| (estimate / q * (1.0 + 1e-12)).floor() as u64);
    let mut next_hint = hint.and_then(|e| lead(e, size));
    // Steps up from `lo` while climbing from a built estimate.
    let mut climb: Option<u64> = None;
    // log2(10⁴) bisection steps reach the step size, and the estimates lead at most 6 more; the
    // caps keep a slow body from stalling.
    let (mut bisections, mut led, mut tried) = (0, 0, 0);
    while hi - lo > 1 {
        let guided = if led < 6 {
            match next_hint.take().filter(|h| lo < *h && *h < hi) {
                Some(h) => Some((h, true)),
                None => climb.map(|step| (lo.saturating_add(step).min(hi - 1), false)),
            }
        } else {
            None
        };
        let (n, from_hint) = match guided {
            Some(g) => {
                led += 1;
                g
            }
            None if bisections < 16 => {
                bisections += 1;
                (lo + (hi - lo) / 2, false)
            }
            None => break,
        };
        let was_guided = guided.is_some();
        tried += 1;
        // A candidate fits only as a body SMLib's check accepts: each is checked as it is built,
        // whatever the evaluation's validation mode (`crate::validation`).
        match crate::validation::validating(crate::Validation::EachStep, || blend(n as f64 * q)).0 {
            Ok(b) => {
                let step = n - lo;
                lo = n;
                best = Some(b);
                // Climb from a built estimate, doubling the step each time one builds.
                climb = if from_hint {
                    Some(1)
                } else if was_guided {
                    Some(step.saturating_mul(2))
                } else {
                    None
                };
            }
            Err(e @ (KernelError::TooLarge(_) | KernelError::Failed(_))) => {
                hi = n;
                climb = None;
                if let KernelError::TooLarge(why) = &e {
                    next_hint = named_fit(why).and_then(|est| lead(est, n as f64 * q));
                }
                smallest_refused = Some((n as f64 * q, e));
            }
            // Not a size the blend could not take: bad input, or the kernel's own fault.
            Err(e) => return Err(e),
        }
    }
    let found = best.map(|b| (lo as f64 * q, b));
    let smallest_refused = if found.is_some() { None } else { smallest_refused };
    Ok(FitSearch { found, smallest_refused, tried })
}

/// [`fillet_max`] for each group of the edges that may limit each other's radius (sharing a
/// face or a vertex, directly or through other edges in the group), so a round that fits isn't
/// reduced for a tight edge elsewhere. The radius per edge (in `edges`' order) and the result,
/// `Ok(None)` when no radius fits. One radius for all (as [`fillet_max`]) when the groups,
/// each reduced to fit, are too large together, or there are too many to search (each search is
/// up to 16 fillets, and this runs in the live preview). Any failure other than a refusal as too
/// large is the error.
pub fn fillet_max_each(body: &Body, edges: &[Vec3], radius: f64, turn: FilletTurn) -> Result<Option<(Vec<f64>, Body)>> {
    let _span = solidcraft_profile::span("kernel", "fillet_max_each");
    const MAX_GROUPS: usize = 8;
    const MAX_SEARCHES: usize = 2;
    let one = || -> Result<Option<(Vec<f64>, Body)>> { Ok(fillet_max(body, edges, radius, turn)?.map(|(r, b)| (vec![r; edges.len()], b))) };
    let Some(groups) = interacting_groups(body, edges).filter(|g| g.len() > 1 && g.len() <= MAX_GROUPS) else { return one() };
    let mut radii = vec![radius; edges.len()];
    let mut searches = 0;
    for g in &groups {
        let pts: Vec<Vec3> = g.iter().filter_map(|i| edges.get(*i).copied()).collect();
        let r = match fillet_turning(body, &pts, radius, turn) {
            Ok(_) => continue,
            Err(KernelError::TooLarge(_)) if searches < MAX_SEARCHES => {
                searches += 1;
                match fillet_max(body, &pts, radius, turn)? {
                    Some((r, _)) => r,
                    None => return one(),
                }
            }
            Err(KernelError::TooLarge(_)) => return one(),
            Err(e) => return Err(e),
        };
        for i in g {
            if let Some(x) = radii.get_mut(*i) {
                *x = r;
            }
        }
    }
    match fillet_each(body, edges, &radii, turn) {
        Ok(b) => Ok(Some((radii, b))),
        Err(KernelError::TooLarge(_)) => one(),
        Err(e) => Err(e),
    }
}

/// The fillet of each edge at its own radius (`radii` in `edges`' order), all in one SMLib
/// blend.
pub fn fillet_each(body: &Body, edges: &[Vec3], radii: &[f64], turn: FilletTurn) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "fillet_each");
    if radii.len() != edges.len() {
        return Err(KernelError::Invalid("one radius per edge".into()));
    }
    if let Some(r) = radii.first().filter(|r| radii.iter().all(|x| x.to_bits() == r.to_bits())) {
        return fillet_turning(body, edges, *r, turn);
    }
    body.require_brep("fillet")?;
    crate::blend_smlib::fillet_each(body, edges, radii, turn).and_then(|b| closed(body, b))
}

/// The points' edges (each the edge a blend's pick there takes) grouped where they share a face
/// or a vertex, directly or through other points' edges: indices into `pts`. `None` when a point
/// has no edge near it.
fn interacting_groups(body: &Body, pts: &[Vec3]) -> Option<Vec<Vec<usize>>> {
    let rec = Faces::of(body).ok()?;
    let (picked, _) = crate::blend_smlib::picked_edges(body, pts).ok()?;
    // What each point's edge touches: its faces and its ends.
    let mut groups: Vec<(BTreeSet<u32>, BTreeSet<u32>, Vec<usize>)> = Vec::new();
    for (i, idx) in picked.into_iter().enumerate() {
        let mut faces = rec.faces_of(idx);
        let mut ends: BTreeSet<u32> = rec.rec.edges.get(idx)?.vertices.into_iter().collect();
        let mut members = vec![i];
        // Join every group touching one of these faces or ends.
        let (meet, rest): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(fs, vs, _)| !fs.is_disjoint(&faces) || !vs.is_disjoint(&ends));
        for (fs, vs, ms) in meet {
            faces.extend(fs);
            ends.extend(vs);
            members.extend(ms);
        }
        groups = rest;
        groups.push((faces, ends, members));
    }
    Some(
        groups
            .into_iter()
            .map(|(_, _, mut ms)| {
                ms.sort_unstable();
                ms
            })
            .collect(),
    )
}

/// Equal-distance chamfer of the edges nearest to the given points, `distance` back along each
/// face, neighbouring chamfers meeting in a mitre.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "chamfer");
    body.require_brep("chamfer")?;
    if !(distance.is_finite() && distance > 1e-6 && distance < 1e6) {
        return Err(KernelError::Invalid("chamfer size must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    crate::blend_smlib::chamfer(body, edges, distance)
}

/// The second side of an unequal chamfer: a distance along the other face, or the angle the
/// chamfer makes with the first face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSide {
    Distance(f64),
    Angle(f64),
}

/// A chamfer with different setbacks on the two faces of each edge: `d1` along the first face
/// (the one facing up most, the other with `flip`), the second side by distance or angle.
pub fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "chamfer_sides");
    body.require_brep("chamfer")?;
    let ok = |x: f64| x.is_finite() && x > 1e-6 && x < 1e6;
    let valid = ok(d1)
        && match side {
            ChamferSide::Distance(d) => ok(d),
            ChamferSide::Angle(a) => a.is_finite() && a > 1e-3 && a < std::f64::consts::PI - 1e-3,
        };
    if !valid {
        return Err(KernelError::Invalid("chamfer sizes must be positive (the angle within 0°…180°)".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    crate::blend_smlib::chamfer_sides(body, edges, d1, side, flip)
}

/// [`chamfer`], and where SMLib refuses the equal chamfer of a single edge as too large or fails
/// it as an operation, the concave chamfer whose contact line runs off the flat face it rolls on,
/// built by carrying that face on ([`concave_overflow_round`]) as [`fillet_removing`] builds such
/// a round; where that does not apply, SMLib's error. The search for the largest chamfer that
/// fits ([`chamfer_max_why`]) uses [`chamfer`], without it. The two-distance and
/// distance-and-angle forms are carried on by [`chamfer_sides_carrying_face`].
pub fn chamfer_carrying_face(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    match (chamfer(body, edges, distance), edges) {
        // Just past the largest that fits, SMLib fails the chamfer as an operation (its status
        // 1001) rather than sizing it as too large; the error kinds say no more than that.
        (Err(e @ (KernelError::TooLarge(_) | KernelError::Failed(_))), [pick]) => {
            concave_overflow_round(body, *pick, distance, |carried| chamfer(carried, &[*pick], distance)).ok_or(e)
        }
        (plain, _) => plain,
    }
}

/// [`chamfer_sides`], and where SMLib refuses the chamfer of a single edge (by two distances, or
/// by a distance and an angle) as too large or fails it as an operation, the concave chamfer
/// whose contact line runs off the flat face it rolls on, built by carrying that face on
/// ([`concave_overflow_round`]) as [`chamfer_carrying_face`] builds the equal one; where that
/// does not apply, SMLib's error.
///
/// The first distance stays along the face SMLib gives it on the body itself
/// ([`first_face_normal`]), and the angle is from that face: the body with the face carried on
/// is another body, whose edge may list its two faces the other way round, so there the chamfer
/// is asked along that face's normal, which only that face leans along.
pub fn chamfer_sides_carrying_face(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    match (chamfer_sides(body, edges, d1, side, flip), edges) {
        // As the equal chamfer: past its face SMLib refuses it as too large, and just past the
        // largest that fits it fails it as an operation (its status 1001).
        (Err(e @ (KernelError::TooLarge(_) | KernelError::Failed(_))), [pick]) => {
            // Deliberate: where the first face cannot be told, SMLib's refusal stands.
            let Some(first) = first_face_normal(body, *pick, flip) else { return Err(e) };
            // The reach sizes only the slab the flat face is carried on as (four times the
            // body's size and the reach, each way from the edge). By an angle the second side's
            // contact may lie further than any multiple of the first distance: where it lies past
            // the slab, the chamfer runs off the slab's square end as it ran off the face, SMLib
            // refuses it on the carried body too, and its refusal stands.
            let reach = match side {
                ChamferSide::Distance(d2) => d1.max(d2),
                ChamferSide::Angle(_) => d1,
            };
            concave_overflow_round(body, *pick, reach, |carried| crate::blend_smlib::chamfer_sides_along(carried, &[*pick], d1, side, first)).ok_or(e)
        }
        (plain, _) => plain,
    }
}

/// How far apart two leans (each a unit normal's component along a unit direction) must be for
/// the face that leans more to be taken as told: far above the rounding of a normal, so that the
/// same face leans more on every copy of the body.
const LEANS_APART: f64 = 1e-9;

/// Whether the second of an edge's two faces takes a two-sided chamfer's first distance, from
/// how far each leans along the chooser's direction, as SMLib decides it
/// (`solidcraft_smlib::Brep::chamfer_sides`): the second only where it leans more, so the first
/// where they lean alike. `None` where the leans differ by less than [`LEANS_APART`]: rounding
/// decides there, and SMLib's own copy of the body need not round as the copy asked here.
fn second_face_first(l0: f64, l1: f64) -> Option<bool> {
    (l0 == l1 || (l1 - l0).abs() > LEANS_APART).then_some(l1 > l0)
}

/// The outward unit normal, at the middle of the edge `pick` takes, of the face [`chamfer_sides`]
/// gives its first distance on `body`: the edge's two faces and how far each leans along up (down
/// with `flip`) are the kernel's ([`solidcore_ops::edge_faces_along_ordered`]), for the choice
/// ([`second_face_first`]), and so is the chosen face's normal there
/// ([`solidcore_ops::Body::face_normal_at_edge`]).
///
/// `None` where the edge, its faces or the choice cannot be told (a mesh body, an edge not
/// between two faces, leans that rounding decides), and where the two faces do not lean clearly
/// apart along that normal ([`LEANS_APART`]), so that it would not single the face out.
fn first_face_normal(body: &Body, pick: Vec3, flip: bool) -> Option<Vec3> {
    let (picked, _) = crate::blend_smlib::picked_edges(body, &[pick]).ok()?;
    let [e] = picked.as_slice() else { return None };
    let (ops, order) = body.ops_body()?;
    let leans = |dir: [f64; 3]| solidcore_ops::edge_faces_along_ordered(ops, order, *e, dir).ok();
    let [(f0, l0), (f1, l1)] = leans([0.0, 0.0, if flip { -1.0 } else { 1.0 }])?;
    if f0 == f1 {
        return None;
    }
    let second = second_face_first(l0, l1)?;
    let first = if second { f1 } else { f0 };
    let [x, y, z] = ops.face_normal_at_edge(order, *e, first).ok()?.value;
    let n = Vec3::new(x, y, z);
    let [(_, a), (_, b)] = leans([n.x, n.y, n.z])?;
    (a != b && second_face_first(a, b)? == second).then_some(n)
}

/// The edge each pick takes, as a blend takes it (an index in the body's edge order, the order
/// of its mesh's edges): the nearest, and at a vertex where a seam or a smooth join meets a sharp
/// edge (a cylinder's seam at its rim), the sharp edge. Picks that find no edge are refused.
pub fn blend_edges(body: &Body, picks: &[Vec3]) -> Result<Vec<usize>> {
    crate::blend_smlib::picked_edges(body, picks).map(|(edges, _)| edges)
}

/// The picked edges' tangent chains, for a blend that is to run along them: `picks` (each the
/// nearest edge of `body` to it), then a point on every other edge that continues one of them
/// tangentially (within 2°, [`solidcraft_geom::TANGENT_CHAIN`], at a shared vertex), until no more do. Each pick takes its edge as a
/// blend does (a pick at a vertex where a seam meets a rim takes the rim). An edge only one face
/// uses ends a chain, and so do a seam and a smooth edge (its two faces meet tangentially, within
/// 0.5°, all along it): they have no corner to round. A chain does not run on into an edge of the
/// other convexity (a boss's concave base running on tangentially as the convex rim of its
/// overhang): a round cannot pass from one side of the faces to the other, so it ends at that
/// vertex.
/// Picks that find no edge are refused.
pub fn tangent_chain(body: &Body, picks: &[Vec3]) -> Result<Vec<Vec3>> {
    let (rec, chain, picked) = picked_chain(body, picks)?;
    let mut out = picks.to_vec();
    for e in chain.into_iter().skip(picked) {
        out.push(edge_middle(&rec, e).ok_or_else(|| KernelError::Failed("a point on a chain edge".into()))?);
    }
    Ok(out)
}

/// [`tangent_chain`] as the record's edge indices (the display's edge indices,
/// [`Body::edges`]): the edges `picks` take, each once, then the edges that continue them.
/// For a caller that shows the chain: it needs no point looked up on a tessellated edge.
pub fn tangent_chain_edges(body: &Body, picks: &[Vec3]) -> Result<Vec<usize>> {
    picked_chain(body, picks).map(|(_, chain, _)| chain)
}

/// The body's record, the picked edges' tangent chain as its edge indices (the picked edges
/// first, each once), and how many of those the picks took.
fn picked_chain(body: &Body, picks: &[Vec3]) -> Result<(Faces, Vec<usize>, usize)> {
    if picks.is_empty() || picks.len() > 1000 || picks.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("select 1…1000 edges by finite points".into()));
    }
    let rec = Faces::of(body)?;
    let (seeds, hidden) = crate::blend_smlib::picked_edges(body, picks)?;
    let mut picked: Vec<usize> = Vec::new();
    for e in seeds {
        if !picked.contains(&e) {
            picked.push(e);
        }
    }
    let chain = chain_of(&rec, &picked, &hidden)?;
    Ok((rec, chain, picked.len()))
}

/// Faces meeting at an angle (radians) below this meet tangentially, a smooth edge with nothing
/// to round: the shaded view's ([`crate::smooth_edges::ANGLE`], half a degree), so an edge the
/// view draws can be rounded and one it hides is smooth. Which edges a chain continues through
/// is wider ([`solidcraft_geom::TANGENT_CHAIN`], 2°).
const TANGENT: f64 = crate::smooth_edges::ANGLE;

/// The point halfway along edge `e`.
fn edge_middle(rec: &Faces, e: usize) -> Option<Vec3> {
    let edge = rec.rec.edges.get(e)?;
    CurveEval::new(edge.parameters()).ok().and_then(|c| c.point(0.5 * (edge.interval[0] + edge.interval[1])))
}

/// How far from a face's surface a point may lie for its normal there.
fn normal_reach(rec: &Faces) -> f64 {
    rec.rec.tolerance.max(1e-7) * 10.0
}

/// Whether edge `e`'s two uses meet tangentially (within [`TANGENT`]) all along it: a
/// smooth edge, with no corner to blend (a seam a face meets itself across is one). An edge
/// whose faces are tangent at one end only (a round running into a wall it is tangent to where
/// the wall meets its face) is a crease everywhere else, which is not smooth; nor is a cusp, its
/// faces folded back on each other (outward normals opposite), which SMLib rounds like any other
/// edge. Decided exactly from the faces' surfaces and the uses' pcurves
/// ([`crate::tangent_edge`]).
fn smooth_join(rec: &Faces, e: usize) -> Result<bool> {
    crate::tangent_edge::tangent_all_along(&rec.rec, |f| rec.side(f), e, TANGENT)
}

/// `seeds` (edges in the record's order, each once), then every other edge that continues one
/// of them tangentially, as [`tangent_chain`] follows them; `hidden` marks the edges a pick
/// passes over.
fn chain_of(rec: &Faces, seeds: &[usize], hidden: &[bool]) -> Result<Vec<usize>> {
    let mut chain = seeds.to_vec();
    // Each edge's smoothness, decided once.
    let mut smooth: Vec<Option<bool>> = vec![None; rec.rec.edges.len()];
    let mut smooth_at = |f: usize| -> Result<bool> {
        if let Some(known) = smooth.get(f).copied().flatten() {
            return Ok(known);
        }
        let s = smooth_join(rec, f)?;
        if let Some(slot) = smooth.get_mut(f) {
            *slot = Some(s);
        }
        Ok(s)
    };
    // The unit direction edge `e` leaves its end `v` in.
    let leaving = |e: usize, v: u32| -> Option<Vec3> {
        let [a, b] = rec.rec.edges.get(e)?.vertices;
        if a == b {
            return None;
        }
        if a == v { rec.tangent(e, false) } else { rec.tangent(e, true).map(|t| t * -1.0) }
    };
    let cos = solidcraft_geom::TANGENT_CHAIN.cos();
    let accept = normal_reach(rec);
    // Whether edge `e` is convex at its end `v` (its faces meet at less than 180° inside the
    // body): the direction into its second face, square to it, points against its first face's
    // outward normal. A face lies to the left of its loop seen from its surface's natural normal.
    // None for an edge without exactly two uses, or where a normal cannot be found.
    let convex = |e: usize, v: u32| -> Option<bool> {
        let edge = rec.rec.edges.get(e)?;
        let [ua, ub] = edge.uses.as_slice() else { return None };
        let forward_b = match rec.rec.faces.get(ub.face as usize)?.loops.get(ub.r#loop as usize)? {
            Loop::Edges { edges } => edges.get(ub.position as usize)?.forward,
            Loop::Vertex { .. } => return None,
        };
        let p = rec.point(v)?;
        let along = rec.tangent(e, edge.vertices[0] != v)?;
        let natural_b = rec.normal_at(ub.face as usize, p, accept)? * rec.side(ub.face as usize)?;
        let into_b = natural_b.cross(if forward_b { along } else { along * -1.0 });
        Some(into_b.dot(rec.normal_at(ua.face as usize, p, accept)?) < 0.0)
    };
    let mut k = 0;
    while k < chain.len() {
        let Some(&e) = chain.get(k) else { break };
        k += 1;
        let Some(edge) = rec.rec.edges.get(e) else { continue };
        for v in edge.vertices {
            let Some(d) = leaving(e, v) else { continue };
            for (f, other) in rec.rec.edges.iter().enumerate() {
                let passed_over = hidden.get(f).copied().unwrap_or(false);
                if passed_over || chain.contains(&f) || other.uses.len() < 2 || !other.vertices.contains(&v) || smooth_at(f)? {
                    continue;
                }
                let turns_over = matches!((convex(e, v), convex(f, v)), (Some(a), Some(b)) if a != b);
                if !turns_over && leaving(f, v).is_some_and(|g| d.dot(g) < -cos) {
                    chain.push(f);
                }
            }
        }
    }
    Ok(chain)
}

/// Before blending exactly `edges` (the record's edges `picks` took, in order): refuses an edge
/// whose faces meet tangentially (a smooth edge, nothing to round), and names the first edge a
/// picked one runs on into tangentially without being blended with it, if any (tangent
/// propagation off). SMLib blends the picked edges alone where it can end a blend there (a round
/// ending on the round it runs into); where it can't, [`with_continuation`] says why.
pub(crate) fn unfinished_chain(body: &Body, picks: &[Vec3], edges: &[usize], hidden: &[bool]) -> Result<Option<Vec3>> {
    let rec = Faces::of(body)?;
    let at = |i: usize| picks.get(i).map(|p| format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)).unwrap_or_default();
    for (i, &e) in edges.iter().enumerate() {
        if smooth_join(&rec, e)? {
            return Err(KernelError::Invalid(format!("the edge at {} is smooth: its faces meet tangentially, so there is nothing to round", at(i))));
        }
    }
    let mut seeds: Vec<usize> = Vec::new();
    for &e in edges {
        if !seeds.contains(&e) {
            seeds.push(e);
        }
    }
    Ok(chain_of(&rec, &seeds, hidden)?.get(seeds.len()).and_then(|&next| edge_middle(&rec, next)))
}

/// Before rounding exactly `edges` (the record's edges `picks` took, in order), edge i at radius
/// `radii[i]`: refuses a round too small to change the part. Across a crease whose faces' normals
/// turn by δ, a round of radius r lies r·(sec(δ/2) − 1) (≈ r·δ²/8) inside the sharp edge at its
/// middle, the farthest it moves the part; below the edge's tolerance the round is the edge
/// itself to the model's accuracy (SMLib refuses to build it). δ is the least turn along the edge
/// ([`crate::tangent_edge::least_angle_along`]); the message names the smallest radius that
/// rounds it, tolerance / (sec(δ/2) − 1), rounded up to the micrometre.
pub(crate) fn rounds_too_small(body: &Body, picks: &[Vec3], edges: &[usize], radii: &[f64]) -> Result<()> {
    let rec = Faces::of(body)?;
    for (i, (&e, &r)) in edges.iter().zip(radii).enumerate() {
        let Some(edge) = rec.rec.edges.get(e) else { continue };
        let tolerance = edge.tolerance.max(rec.rec.tolerance);
        let Some(turn) = crate::tangent_edge::least_angle_along(&rec.rec, |f| rec.side(f), e)? else { continue };
        let reach = 1.0 / (0.5 * turn).cos() - 1.0;
        if !(turn > TANGENT && reach.is_finite() && reach > 0.0) || r * reach >= tolerance {
            continue;
        }
        let at = picks.get(i).map(|p| format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)).unwrap_or_default();
        let smallest = (tolerance / reach * 1e3).ceil() / 1e3;
        return Err(KernelError::Invalid(format!(
            "a round of radius {r} mm on the edge at {at} is too small: its faces turn only {:.2}° across the edge, so the round stays within the model's tolerance ({tolerance} mm) of it and changes nothing; the smallest radius that rounds this edge is {smallest:.3} mm",
            turn.to_degrees()
        )));
    }
    Ok(())
}

/// Refuses a round whose section would cut through the material into a hollow behind its edge,
/// opening the part (a slit into the hollow, or a cut through the part): the owner's rule of
/// 2026-10-30, as [`KernelError::TooLarge`], so a fillet feature names the largest radius that
/// fits. A round running past a rim of its faces into open air is left to the blend (tangent
/// where it has room, cut where it runs out).
///
/// Decided exactly in the section square to a picked straight edge between two planar faces
/// (A and B, convex): the round's circle of radius r has its centre at E + r k, the point at
/// depth r below both planes (k = −(n_A + n_B)/(1 + n_A·n_B)), and removes the region between the
/// edge E and its arc. That region grows with r. Material behind the faces first opens into a
/// hollow where the arc, moving away from E, uncovers a point q of the body's boundary strictly
/// inside the wedge with no material just beyond it (toward the centre): q lies on the circle,
/// |q − E − r k| = r, whose larger root in r is where it opens ([`opening_radius`]). Where the
/// strips of faces A and B themselves leave their faces, the round runs out past a rim into the
/// air, which is not a point of this kind (it lies on a face's plane, not inside the wedge).
///
/// The boundary points looked at run along the edge, so their section is the same all along the
/// stretch they share with it: straight edges parallel to it (a hollow's corner), cylindrical
/// faces about axes parallel to it (a hollow's rounded corner), and a picked concave edge parallel
/// to it, rounded in the same fillet (the arc of its round, which fills the hollow's corner).
/// Whether the point uncovered is material is the body's exact point classification.
pub(crate) fn rounds_into_hollow(body: &Body, picks: &[Vec3], edges: &[usize], radii: &[f64]) -> Result<()> {
    let rec = Faces::of(body)?;
    for (i, (&e, &r)) in edges.iter().zip(radii).enumerate() {
        let Some(section) = Section::of(&rec, e) else { continue };
        // Only an opening under the round's radius (less the tolerance) refuses it.
        let Some((_, at)) = section.opening(body, &rec, edges, radii, r - section.tolerance)? else { continue };
        let pick = picks.get(i).map(|p| format!(" at ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)).unwrap_or_default();
        return Err(KernelError::TooLarge(format!(
            "the round of radius {r} mm on the edge{pick} would cut through the material behind it into the hollow at ({:.3}, {:.3}, \
             {:.3}) and open the part",
            at.x, at.y, at.z
        )));
    }
    Ok(())
}

/// The section of a round of a straight, convex edge between two planar faces, as
/// [`rounds_into_hollow`] reads it.
struct Section {
    /// The edge's start, its unit direction and its length.
    start: Vec3,
    dir: Vec3,
    length: f64,
    /// The faces' outward unit normals.
    na: Vec3,
    nb: Vec3,
    /// The centre of the round of radius r is at `start + r k` in the section through `start`.
    k: Vec3,
    tolerance: f64,
}

impl Section {
    fn of(rec: &Faces, e: usize) -> Option<Section> {
        if !rec.straight(e) {
            return None;
        }
        let [fa, fb] = <[u32; 2]>::try_from(rec.faces_of(e).into_iter().collect::<Vec<_>>()).ok()?;
        let (na, nb) = (rec.plane_normal(fa as usize)?, rec.plane_normal(fb as usize)?);
        let (start, end) = rec.ends(e)?;
        let length = start.dist(end);
        let dir = (end - start).normalized()?;
        let g = na.dot(nb);
        if !(g.is_finite() && g.abs() < 1.0 - 1e-9) {
            return None;
        }
        // Convex: going into face A from the edge goes below face B's plane.
        let use_a = rec.uses(fa as usize).find(|u| u.edge as usize == e)?;
        let into_a = rec.direction_into_face(fa as usize, use_a)?;
        if into_a.dot(nb) >= 0.0 {
            return None;
        }
        let tolerance = rec.rec.edges.get(e)?.tolerance.max(rec.rec.tolerance).max(1e-9);
        let k = (na + nb) * (-1.0 / (1.0 + g));
        Some(Section { start, dir, length, na, nb, k, tolerance })
    }

    /// `p − start` without its part along the edge: its place in the section.
    fn flat(&self, p: Vec3) -> Vec3 {
        let w = p - self.start;
        w - self.dir * w.dot(self.dir)
    }

    /// Whether `p` (in the section) runs along the edge: a direction parallel to it.
    fn parallel(&self, d: Vec3) -> bool {
        d.normalized().is_some_and(|d| d.cross(self.dir).len() <= 1e-9)
    }

    /// The stretch `[lo, hi]` along the edge (from its start) that `[a, b]` (points' places along
    /// it) shares with it, its middle; `None` where they share none.
    fn shared(&self, a: f64, b: f64) -> Option<f64> {
        let (lo, hi) = (a.min(b).max(0.0), a.max(b).min(self.length));
        (hi - lo > self.tolerance).then_some(0.5 * (lo + hi))
    }

    /// The smallest radius under `below` at which the round opens into a hollow behind it and
    /// the point where it does (at the middle of the stretch the opening runs along); `None`
    /// where it never does through the boundary looked at ([`rounds_into_hollow`]). `edges` and
    /// `radii`: the fillet's.
    ///
    /// Where each boundary point would open is plain arithmetic; only the ones that would open
    /// under `below` (the round's own radius, less the tolerance) can refuse it, so only theirs
    /// are classified, all in one classification of the body ([`Body::classify_points`]): on a
    /// large body most of a classification is setting it up, and a body with many edges
    /// parallel to the round's has many such points, nearly all far beyond a small round.
    fn opening(&self, body: &Body, rec: &Faces, edges: &[usize], radii: &[f64], below: f64) -> Result<Option<(f64, Vec3)>> {
        let mut found: Vec<Opening> = Vec::new();
        // Straight edges parallel to the round's: a hollow's corner, or a picked concave edge whose
        // round of its own fills it.
        for (j, edge) in rec.rec.edges.iter().enumerate() {
            if !matches!(edge.curve, Curve::Line { .. }) {
                continue;
            }
            let Some((a, b)) = rec.ends(j) else { continue };
            if !self.parallel(b - a) {
                continue;
            }
            let Some(middle) = self.shared((a - self.start).dot(self.dir), (b - self.start).dot(self.dir)) else { continue };
            // The round's own edge, and edges on its line, lie at the section's origin.
            let q = self.flat(a);
            if q.len() <= self.tolerance {
                continue;
            }
            let opening = match edges.iter().position(|&p| p == j) {
                Some(at) => match (concave_round_centre(rec, j, radii.get(at).copied()), radii.get(at)) {
                    (Some((o, arc)), Some(&rho)) => self.through_circle(self.flat(o), rho, middle, Some(arc), below),
                    _ => None,
                },
                None => self.through_point(q, middle, below),
            };
            found.extend(opening);
        }
        // Cylindrical faces about axes parallel to the edge: a hollow's rounded corner.
        for (f, face) in rec.rec.faces.iter().enumerate() {
            let Surface::Cylinder { frame, radius } = &face.surface else { continue };
            if !self.parallel(v3(frame.x_axis).cross(v3(frame.y_axis))) {
                continue;
            }
            let along: Vec<f64> =
                rec.uses(f).filter_map(|u| rec.ends(u.edge as usize)).flat_map(|(a, b)| [a, b]).map(|p| (p - self.start).dot(self.dir)).collect();
            let (lo, hi) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
            let Some(middle) = self.shared(lo, hi) else { continue };
            found.extend(self.through_circle(self.flat(v3(frame.origin)), *radius, middle, None, below));
        }
        if found.is_empty() {
            return Ok(None);
        }
        let points: Vec<Vec3> = found.iter().flat_map(|o| o.probes.iter().map(|&(p, _)| p)).collect();
        let mut classes = body.classify_points(&points)?.into_iter();
        let mut best: Option<(f64, Vec3)> = None;
        for o in found {
            // Every probe is read, so each opening takes its own classes from the list.
            let opens = o.probes.iter().fold(true, |all, &(_, want)| classes.next() == Some(want) && all);
            if opens && best.is_none_or(|(r, _)| o.r < r) {
                best = Some((o.r, o.at));
            }
        }
        Ok(best)
    }

    /// Where the round opens through the section point `q` (a boundary point running along the
    /// edge): its larger radius on the circle through `q`, when `q` then lies on the round's arc
    /// strictly inside the wedge and no material lies just beyond it, toward the centre.
    fn through_point(&self, q: Vec3, middle: f64, below: f64) -> Option<Opening> {
        // |q − r k|² = r²: (|k|² − 1) r² − 2 (q·k) r + |q|² = 0.
        let r = larger_root(self.k.len2() - 1.0, q.dot(self.k), q.len2())?;
        self.opens_at(r, q, middle, below, None)
    }

    /// Where the round opens through a circle of the section, centre `o`, radius `rho`, with the
    /// hollow inside it: where the circle's far point from the round's centre reaches the round's
    /// arc (|o − r k| + rho = r). `arc`: the directions from `o` the circle is the boundary
    /// along, when it is a picked concave edge's round, not yet built.
    fn through_circle(&self, o: Vec3, rho: f64, middle: f64, arc: Option<(Vec3, Vec3)>, below: f64) -> Option<Opening> {
        // |o − r k|² = (r − rho)²: (|k|² − 1) r² − 2 (o·k − rho) r + |o|² − rho² = 0.
        let r = larger_root(self.k.len2() - 1.0, o.dot(self.k) - rho, o.len2() - rho * rho)?;
        let out = (o - self.k * r).normalized()?;
        let q = o + out * rho;
        match arc {
            // On the concave round's own arc: between its two contacts, seen from its centre.
            Some((m1, m2)) => within(out, m1, m2, self.tolerance / rho.max(1.0)).then(|| self.opens_at(r, q, middle, below, None)).flatten(),
            // On the face itself.
            None => self.opens_at(r, q, middle, below, Some(self.place(q, middle))),
        }
    }

    /// The section point `q` at `middle` along the edge.
    fn place(&self, q: Vec3, middle: f64) -> Vec3 {
        self.start + self.dir * middle + q
    }

    /// The opening at radius `r` through `q`, when `r` is under `below` and `q` lies on the
    /// round's arc strictly inside the wedge: it opens when the body has no material just beyond
    /// `q` toward the centre, and `on` (a point that must be on the body's boundary) is on it.
    fn opens_at(&self, r: f64, q: Vec3, middle: f64, below: f64, on: Option<Vec3>) -> Option<Opening> {
        if !(r.is_finite() && r > self.tolerance && r < below) {
            return None;
        }
        let centre = self.k * r;
        // q − centre = α n_A + β n_B with α, β > 0: on the arc between the contacts.
        if !within(q - centre, self.na, self.nb, self.tolerance) {
            return None;
        }
        let inward = (centre - q).normalized()?;
        let step = (r * 1e-3).max(self.tolerance * 10.0);
        let at = self.place(q, middle);
        let mut probes = vec![(at + inward * step, crate::body::PointClass::Outside)];
        probes.extend(on.map(|p| (p, crate::body::PointClass::OnBoundary)));
        Some(Opening { r, at, probes })
    }
}

/// Where a round may open into a hollow ([`Section::opening`]): at radius `r`, through `at`,
/// when each probe point lies as it says.
struct Opening {
    r: f64,
    at: Vec3,
    probes: Vec<(Vec3, crate::body::PointClass)>,
}

/// Whether `w` = α a + β b with α, β > `margin` (a, b unit, not parallel).
fn within(w: Vec3, a: Vec3, b: Vec3, margin: f64) -> bool {
    let g = a.dot(b);
    let det = 1.0 - g * g;
    if !(det > 1e-12) {
        return false;
    }
    let (wa, wb) = (w.dot(a), w.dot(b));
    let alpha = (wa - g * wb) / det;
    let beta = (wb - g * wa) / det;
    alpha > margin && beta > margin
}

/// The larger root of a x² − 2 b x + c = 0 (a > 0).
fn larger_root(a: f64, b: f64, c: f64) -> Option<f64> {
    let disc = b * b - a * c;
    (a > 1e-12 && disc >= 0.0).then(|| (b + disc.sqrt()) / a).filter(|r| r.is_finite())
}

/// The centre of the round of radius `r` of concave straight edge `e` between planar faces, and
/// the directions from it to the round's two contacts: the circle tangent to both faces' planes
/// on the side away from the material.
fn concave_round_centre(rec: &Faces, e: usize, r: Option<f64>) -> Option<(Vec3, (Vec3, Vec3))> {
    let r = r?;
    let [f1, f2] = <[u32; 2]>::try_from(rec.faces_of(e).into_iter().collect::<Vec<_>>()).ok()?;
    let (n1, n2) = (rec.plane_normal(f1 as usize)?, rec.plane_normal(f2 as usize)?);
    let use1 = rec.uses(f1 as usize).find(|u| u.edge as usize == e)?;
    // Concave: going into face 1 from the edge goes above face 2's plane.
    if rec.direction_into_face(f1 as usize, use1)?.dot(n2) <= 0.0 {
        return None;
    }
    let g = n1.dot(n2);
    if !(g.is_finite() && g.abs() < 1.0 - 1e-9) {
        return None;
    }
    let (a, _) = rec.ends(e)?;
    Some((a + (n1 + n2) * (r / (1.0 + g)), (-n1, -n2)))
}

/// A blend's failure where a picked edge continues tangentially into `next`, an edge left out
/// ([`unfinished_chain`]): SMLib rounds a tangent chain as one sequence and could not end the
/// blend there, so the reason and the way out are named.
pub(crate) fn with_continuation(e: KernelError, next: Option<Vec3>) -> KernelError {
    let Some(p) = next else { return e };
    let why = format!(
        " (a picked edge continues tangentially into the edge at ({:.3}, {:.3}, {:.3}), which is not blended with it, and the blend \
         cannot end there: with tangent propagation on, tangent_chain: true, its tangent chain is blended as one)",
        p.x, p.y, p.z
    );
    match e {
        KernelError::Invalid(m) => KernelError::Invalid(m + &why),
        KernelError::Failed(m) => KernelError::Failed(m + &why),
        KernelError::TooLarge(m) => KernelError::TooLarge(m + &why),
        e => e,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use solidcore_record::{Loop2, Plane, Region2, Vec2};

    /// The plate with its r16 corner, boss and cut (SolidCraft's filletfile.sc).
    fn boss_on_rounded_plate() -> Body {
        let plate = crate::box_solid(Vec3::new(0.0, -10.5, -100.0), Vec3::new(48.5, 0.0, 0.0)).unwrap();
        let plate = fillet(&plate, &[Vec3::new(48.5, -5.25, 0.0)], 16.0).unwrap();
        let boss = crate::cylinder(Vec3::new(14.388009991673611, -14.038301415494109, -24.0), Vec3::Z, 12.90348790056394, 24.0).unwrap();
        let body = crate::boolean(&plate, &boss, crate::BoolOp::Union).unwrap().unwrap();
        let cut = crate::box_solid(Vec3::new(-1.0, -11.0, -64.0), Vec3::new(26.796894836538296, 1.0, -24.0)).unwrap();
        crate::boolean(&body, &cut, crate::BoolOp::Cut).unwrap().unwrap()
    }

    /// The boss's concave base edge on the rounded plate: rounded 5 (its contact line on the
    /// plate's front face stays short of x 32.5, where the face's top edge turns into the r16
    /// corner's arc) SMLib builds it; rounded 6 and 8 the line runs past there and SMLib
    /// refuses it (solidcore#3252), and the round is built by carrying the face on: its
    /// material is the round's section between the plane and the boss (exact, by integration
    /// below) over the boss's 24 mm, less the sliver past the arc. At 5 the carried-on build
    /// gives SMLib's volume. Closed, one shell, inside the plate's box.
    #[test]
    fn a_concave_round_runs_on_past_where_its_face_turns_into_a_round() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap();
        let pick = Vec3::new(26.796894836538296, -10.5, -12.0);
        // the round's section between the plate's plane, the boss's circle and the round's arc
        let section = |r: f64| {
            let (cx, cy, big, yp) = (14.388009991673611, -14.038301415494109, 12.90348790056394, -10.5);
            let (bx, by) = (cx + ((big + r).powi(2) - (yp - r - cy).powi(2)).sqrt(), yp - r);
            let k = big / (big + r);
            let (tx, ty) = (cx + (bx - cx) * k, cy + (by - cy) * k);
            let ex = cx + (big * big - (yp - cy).powi(2)).sqrt();
            let mut pts = vec![(ex, yp), (bx, yp)];
            let n = 4000;
            let (a0, a1) = ((yp - by).atan2(0.0), (ty - by).atan2(tx - bx));
            pts.extend((1..=n).map(|i| {
                let t = a0 + (a1 - a0) * f64::from(i) / f64::from(n);
                (bx + r * t.cos(), by + r * t.sin())
            }));
            let (c0, c1) = ((ty - cy).atan2(tx - cx), (yp - cy).atan2(ex - cx));
            pts.extend((1..n).map(|i| {
                let t = c0 + (c1 - c0) * f64::from(i) / f64::from(n);
                (cx + big * t.cos(), cy + big * t.sin())
            }));
            let twice: f64 = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(p, q)| p.0 * q.1 - q.0 * p.1).sum();
            twice.abs() / 2.0
        };
        let smlib = crate::measure(&fillet(&body, &[pick], 5.0).unwrap()).unwrap().volume;
        let (carried, _) = fillet_removing(&body, &[pick], 5.0, FilletTurn::Mitre).unwrap();
        assert!((crate::measure(&carried).unwrap().volume - smlib).abs() < 1e-6);
        assert!((smlib - before.volume - section(5.0) * 24.0).abs() < 1e-3, "{}", smlib - before.volume);
        for r in [6.0, 8.0] {
            assert!(matches!(fillet(&body, &[pick], r), Err(KernelError::TooLarge(_))), "SMLib refuses r {r}");
            let (rounded, gone) = fillet_removing(&body, &[pick], r, FilletTurn::Mitre).unwrap_or_else(|e| panic!("r {r}: {e}"));
            assert!(gone.is_empty());
            // its history is a fillet's, over the body alone: a face made in place of the edge
            let (picked, _) = crate::blend_smlib::picked_edges(&body, &[pick]).unwrap();
            let h = rounded.history().expect("a history");
            assert_eq!(h.operands, 1);
            assert!(h.faces.iter().any(|f| f.origins == [crate::history::FaceOrigin::Edge { operand: 0, edge: picked[0] }]), "r {r}");
            // and every face of the body is still there, under its own origin
            let still = |i: usize| h.faces.iter().any(|f| f.origins.contains(&crate::history::FaceOrigin::Face { operand: 0, face: i }));
            assert!((0..body.face_count()).all(still), "r {r}");
            let m = crate::measure(&rounded).unwrap();
            let added = m.volume - before.volume;
            let full = section(r) * 24.0;
            assert!(m.shells == 1 && (full - added).abs() < 0.05, "r {r}: added {added} of {full}");
            assert!(m.bbox.min.dist(before.bbox.min) < 1e-9 && m.bbox.max.dist(before.bbox.max) < 1e-9, "r {r}: {:?}", m.bbox);
        }
    }

    /// The pick on the boss's concave base edge of [`boss_on_rounded_plate`].
    const BOSS_EDGE: Vec3 = Vec3::new(26.796894836538296, -10.5, -12.0);

    /// The chamfer `d` of the boss's base edge on the rounded plate, as solidcore defines it from
    /// the wedge at the edge (angle `a`): offset `r = d (1 - cos a) / sin a` from the plate's front
    /// face (the plane y = -10.5) and from the boss's cylinder. Its contact on the plane `p` and
    /// on the boss's circle `q` (in xy: the feet of the point that far from both), and the area
    /// of its section between the plane, the chamfer's line from `p` to `q` and the circle.
    fn boss_chamfer_section(d: f64) -> ((f64, f64), (f64, f64), f64) {
        boss_chamfer_sides_section(d, d)
    }

    /// [`boss_chamfer_section`] of the two-distance chamfer `plate` along the plate's front face
    /// and `boss` along the boss's cylinder: solidcore keeps the offsets these are at the edge's
    /// wedge, `(boss - plate cos a) / sin a` from the plate's face and `(plate - boss cos a) /
    /// sin a` from the boss's cylinder, and the contacts are the feet of the point that far from
    /// each.
    fn boss_chamfer_sides_section(plate: f64, boss: f64) -> ((f64, f64), (f64, f64), f64) {
        let (cx, cy, big, yp): (f64, f64, f64, f64) = (14.388009991673611, -14.038301415494109, 12.90348790056394, -10.5);
        let ex = cx + (big * big - (yp - cy).powi(2)).sqrt();
        let (cos, sin) = ((yp - cy) / big, (ex - cx) / big);
        let (rp, rb) = ((boss - plate * cos) / sin, (plate - boss * cos) / sin);
        let (ox, oy) = (((big + rb).powi(2) - (yp - rp - cy).powi(2)).sqrt(), yp - rp - cy);
        let p = (cx + ox, yp);
        let q = (cx + ox * big / (big + rb), cy + oy * big / (big + rb));
        let triangle = 0.5 * ((p.0 - ex) * (q.1 - yp) - (q.0 - ex) * (p.1 - yp)).abs();
        let turn = (yp - cy).atan2(ex - cx) - (q.1 - cy).atan2(q.0 - cx);
        (p, q, triangle - 0.5 * big * big * (turn - turn.sin()))
    }

    /// z of the r16 corner's arc on the plate's front face at x (x >= 32.5): it lies on
    /// (x - 32.5)^2 + (z + 16)^2 = 256.
    fn on_corner_arc(x: f64) -> f64 {
        -16.0 + (256.0 - (x - 32.5).powi(2)).sqrt()
    }

    /// The boss's base edge chamfered 8: its contact line on the plate's front face runs past
    /// x 32.5, where the face's top edge turns into the r16 corner's arc, and SMLib refuses it.
    /// Carried on, the chamfer's contact line on the plate ends on the arc, its top edge meets
    /// the corner at x 32.5, and its material is its section over the boss's 24 mm less the
    /// sliver past the arc (under 0.01 mm³ here). One shell.
    #[test]
    fn a_concave_chamfer_runs_on_past_where_its_face_turns_into_a_round() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        assert!(matches!(chamfer(&body, &[BOSS_EDGE], 8.0), Err(KernelError::TooLarge(_))));
        let out = chamfer_carrying_face(&body, &[BOSS_EDGE], 8.0).unwrap();
        let m = crate::measure(&out).unwrap();
        let (p, q, area) = boss_chamfer_section(8.0);
        let short = 24.0 * area - (m.volume - before);
        assert!(m.shells == 1 && (0.0..0.01).contains(&short), "short of the full section by {short}");
        let top = p.1 + (32.5 - p.0) * (q.1 - p.1) / (q.0 - p.0);
        for v in [Vec3::new(p.0, -10.5, on_corner_arc(p.0)), Vec3::new(32.5, top, 0.0), Vec3::new(q.0, q.1, 0.0)] {
            assert!(has_vertex(&out, v, 1e-6), "no vertex at {v:?}");
        }
    }

    /// Chamfered 7 the contact line stays on the plate's face (x 32.4906 < 32.5) and SMLib builds
    /// it: carrying the face on does not run and the body is SMLib's, its material the section.
    #[test]
    fn a_concave_chamfer_that_fits_its_face_is_smlibs() {
        let body = boss_on_rounded_plate();
        let plain = chamfer(&body, &[BOSS_EDGE], 7.0).unwrap();
        let out = chamfer_carrying_face(&body, &[BOSS_EDGE], 7.0).unwrap();
        assert_eq!(crate::encode_native_body(&out).unwrap(), crate::encode_native_body(&plain).unwrap());
        let (_, _, area) = boss_chamfer_section(7.0);
        let added = crate::measure(&out).unwrap().volume - crate::measure(&body).unwrap().volume;
        assert!((24.0 * area - added).abs() < 1e-3, "{added}");
    }

    /// The carried-on sequence checks each step as it is built, whatever the evaluation around it
    /// checks: a step the check refuses gives the kernel's refusal, not a body the feature's check
    /// refuses later.
    #[test]
    fn a_carried_on_blend_checks_each_step_whatever_the_evaluation_checks() {
        let body = boss_on_rounded_plate();
        for mode in crate::Validation::ALL {
            let seen = std::cell::Cell::new(None);
            let (out, deferred) = crate::validation::validating(mode, || {
                concave_overflow_round(&body, BOSS_EDGE, 8.0, |carried| {
                    seen.set(Some(crate::validation::step()));
                    chamfer(carried, &[BOSS_EDGE], 8.0)
                })
            });
            assert!(out.is_some(), "{mode:?}");
            assert_eq!(seen.get(), Some(crate::validation::Step::Check), "{mode:?}");
            assert!(!deferred, "{mode:?}: no step's check is left for the feature");
        }
    }

    /// Just past the largest chamfer that fits (7.00), SMLib fails it as an operation instead of
    /// sizing it; carried on it builds, and the chamfer grows with its distance through there.
    #[test]
    fn a_concave_chamfer_just_past_its_face_builds() {
        let body = boss_on_rounded_plate();
        assert!(matches!(chamfer(&body, &[BOSS_EDGE], 7.02), Err(KernelError::Failed(_))));
        let before = crate::measure(&body).unwrap().volume;
        let mut last = before;
        for d in [7.0, 7.01, 7.02, 7.05, 7.1, 8.0] {
            let out = chamfer_carrying_face(&body, &[BOSS_EDGE], d).unwrap_or_else(|e| panic!("{d}: {e}"));
            let v = crate::measure(&out).unwrap().volume;
            let (_, _, area) = boss_chamfer_section(d);
            assert!(v > last && (24.0 * area - (v - before)).abs() < 0.01, "{d}: {v}");
            last = v;
        }
    }

    /// What carrying the face on leaves out of a chamfer with contacts `p` on the plate and `q`
    /// on the boss: the part of its section past the r16 corner's arc, none where `p` is short of
    /// x 32.5. At x = 32.5 + u the section is as thick as the chamfer's line stands off the plate
    /// there, `(a - u)` times the line's slope with `a = p.x - 32.5`, and the arc is
    /// `16 - sqrt(256 - u^2)` below the plate's top: their product integrated over u from 0 to a.
    fn past_corner_arc(p: (f64, f64), q: (f64, f64)) -> f64 {
        assert!(q.0 < 32.5, "the chamfer's line runs from the boss past x 32.5");
        let a = (p.0 - 32.5).max(0.0);
        let root = (256.0 - a * a).sqrt();
        let slope = (p.1 - q.1) / (p.0 - q.0);
        slope * (8.0 * a * a - a * (0.5 * a * root + 128.0 * (a / 16.0).asin()) + (4096.0 - root.powi(3)) / 3.0)
    }

    /// The round `r` of the boss's base edge on the rounded plate: its centre is `r` off the
    /// plate's front face and off the boss's cylinder. Its contact on the plane `p` and on the
    /// boss's circle `t` (in xy), and the area of its section: the triangle of the edge, `p` and
    /// `t`, less the round's segment on `p t` and the boss's segment on the edge and `t`.
    fn boss_round_section(r: f64) -> ((f64, f64), (f64, f64), f64) {
        let (cx, cy, big, yp): (f64, f64, f64, f64) = (14.388009991673611, -14.038301415494109, 12.90348790056394, -10.5);
        let ex = cx + (big * big - (yp - cy).powi(2)).sqrt();
        let (bx, by) = (cx + ((big + r).powi(2) - (yp - r - cy).powi(2)).sqrt(), yp - r);
        let (p, t) = ((bx, yp), (cx + (bx - cx) * big / (big + r), cy + (by - cy) * big / (big + r)));
        let triangle = 0.5 * ((p.0 - ex) * (t.1 - yp) - (t.0 - ex) * (p.1 - yp)).abs();
        // the round's arc turns from straight up at `p` to the direction of the boss's axis
        let round = ((cy - by) / (big + r)).acos();
        let boss = (yp - cy).atan2(ex - cx) - (t.1 - cy).atan2(t.0 - cx);
        (p, t, triangle - 0.5 * r * r * (round - round.sin()) - 0.5 * big * big * (boss - boss.sin()))
    }

    /// How many faces of `out` were made in place of the edge `pick` takes on `body`.
    fn faces_from_edge(out: &Body, body: &Body, pick: Vec3) -> usize {
        let (picked, _) = crate::blend_smlib::picked_edges(body, &[pick]).unwrap();
        let from = [crate::history::FaceOrigin::Edge { operand: 0, edge: picked[0] }];
        out.history().expect("a history").faces.iter().filter(|f| f.origins == from).count()
    }

    /// The boss curves away behind the line of its base edge: its circle is back at the edge's x
    /// (26.797) 7.08 in front of the plate. A chamfer whose contact on the boss lies past there
    /// (equal distances from 12.92 on) stands over the boss there, not over the plate's face,
    /// and is whole there all the same: chamfered 13, 14 and 16 its material is its section over
    /// the boss's 24 mm less the part past the corner's arc, its contact line on the boss runs
    /// the boss's height, and it is one face.
    #[test]
    fn a_concave_chamfer_is_whole_where_it_stands_over_the_boss() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        for d in [13.0, 14.0, 16.0] {
            assert!(matches!(chamfer(&body, &[BOSS_EDGE], d), Err(KernelError::TooLarge(_))), "SMLib refuses {d}");
            let out = chamfer_carrying_face(&body, &[BOSS_EDGE], d).unwrap_or_else(|e| panic!("{d}: {e}"));
            let (p, q, area) = boss_chamfer_section(d);
            assert!(q.0 < BOSS_EDGE.x, "{d}: the contact on the boss is behind the edge's line");
            let m = crate::measure(&out).unwrap();
            let (added, full) = (m.volume - before, 24.0 * area - past_corner_arc(p, q));
            assert!(m.shells == 1 && (full - added).abs() < 5e-4, "{d}: added {added} of {full}");
            for z in [0.0, -24.0] {
                assert!(has_vertex(&out, Vec3::new(q.0, q.1, z), 1e-6), "{d}: no vertex on the boss at z {z}");
            }
            assert_eq!(faces_from_edge(&out, &body, BOSS_EDGE), 1, "{d}");
        }
    }

    /// A round likewise, from a radius of 9.75 on: rounded 10.5, 12 and 14 its contact line on
    /// the boss, behind the edge's line, runs the boss's height, it is one face, and its material
    /// is its section less the part past the corner's arc. That part has no closed form (over x
    /// it is the round's height over the plate times the arc's drop below the plate's top, two
    /// circles' heights), so it is bracketed. With `a` the contact's reach past x 32.5, the round
    /// stands `w^2 / (r + sqrt(r^2 - w^2))` off the plate `w` short of its contact: at least
    /// `w^2 / 2r`, at most `w^2 / a^2` of its height at x 32.5. The arc drops likewise, between
    /// `v^2 / 32` and `v^2 / a^2` of its drop at the contact, and `w^2 (a - w)^2` integrates to
    /// `a^5 / 30`. The bracket is narrower than what stands over the boss (0.004 against 0.019
    /// mm³ at 10.5, 0.06 against 1.5 at 14). The section's closed form is SMLib's at radius 5.
    #[test]
    fn a_concave_round_is_whole_where_it_stands_over_the_boss() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        let fits = crate::measure(&fillet(&body, &[BOSS_EDGE], 5.0).unwrap()).unwrap().volume - before;
        assert!((fits - 24.0 * boss_round_section(5.0).2).abs() < 1e-3, "{fits}");
        for r in [10.5, 12.0, 14.0] {
            assert!(matches!(fillet(&body, &[BOSS_EDGE], r), Err(KernelError::TooLarge(_))), "SMLib refuses r {r}");
            let (out, gone) = fillet_removing(&body, &[BOSS_EDGE], r, FilletTurn::Mitre).unwrap_or_else(|e| panic!("r {r}: {e}"));
            assert!(gone.is_empty());
            let (p, t, area) = boss_round_section(r);
            assert!(t.0 < BOSS_EDGE.x, "r {r}: the contact on the boss is behind the edge's line");
            let a = p.0 - 32.5;
            let (height, drop) = (r - (r * r - a * a).sqrt(), 16.0 - (256.0 - a * a).sqrt());
            let (least, most) = (a.powi(5) / (30.0 * 2.0 * r * 32.0), height * drop * a / 30.0);
            let m = crate::measure(&out).unwrap();
            let short = 24.0 * area - (m.volume - before);
            assert!(m.shells == 1 && (least - 1e-3..=most + 1e-3).contains(&short), "r {r}: short by {short}, past the arc {least} to {most}");
            for z in [0.0, -24.0] {
                assert!(has_vertex(&out, Vec3::new(t.0, t.1, z), 1e-6), "r {r}: no vertex on the boss at z {z}");
            }
            assert_eq!(faces_from_edge(&out, &body, BOSS_EDGE), 1, "r {r}");
        }
    }

    /// A plate 30 long and 10 thick, 20 deep, with a wall on its left: the wall's side rises
    /// from the plate's top (the plane y = 10, from x 20 to its end at x 30) at (20, 10), `wedge`
    /// degrees from the top and 20 long, and meets the top there in an inside edge along z.
    fn walled_plate(wedge: f64) -> Body {
        let (sin, cos) = wedge.to_radians().sin_cos();
        let far = Vec2::new(20.0 + 20.0 * cos, 10.0 + 20.0 * sin);
        let outline = [Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 10.0), Vec2::new(20.0, 10.0), far, Vec2::new(0.0, far.y)];
        crate::build::extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&outline), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap()
    }

    /// The pick on the inside edge of [`walled_plate`].
    const WALL_EDGE: Vec3 = Vec3::new(20.0, 10.0, 10.0);

    /// Blends of [`walled_plate`]'s inside edge through where they run past the top's end at
    /// x 30. A chamfer `d` is the triangle of its two sides, `d^2 sin(wedge) / 2`, and past the
    /// end it loses its tip there, `d - 10` long and as high as its line stands off the top at
    /// x 30. Up to 10 it is SMLib's; past it SMLib refuses it and carrying the top on builds it.
    /// A round whose contacts are 12 from the edge (radius `12 tan(wedge / 2)`) is the kite of
    /// the edge, its contacts and its centre less its sector, and loses what stands past x 30:
    /// `r - sqrt(r^2 - w^2)` off the top, `w` from 0 to 2 short of its contact. Each blend is one
    /// face, in one shell.
    fn wall_blends_run_on_past_the_tops_end(wedge: f64) {
        let body = walled_plate(wedge);
        let before = crate::measure(&body).unwrap().volume;
        let (sin, cos) = wedge.to_radians().sin_cos();
        let mut last = before;
        for d in [9.0, 9.9, 10.1, 12.0] {
            let plain = chamfer(&body, &[WALL_EDGE], d);
            let out = chamfer_carrying_face(&body, &[WALL_EDGE], d).unwrap_or_else(|e| panic!("{wedge}°, {d}: {e}"));
            match plain {
                Ok(plain) if d < 10.0 => {
                    assert_eq!(crate::encode_native_body(&out).unwrap(), crate::encode_native_body(&plain).unwrap(), "{wedge}°, {d}")
                }
                Err(KernelError::TooLarge(_)) if d > 10.0 => {}
                other => panic!("{wedge}°, {d}: SMLib gave {:?}", other.map(|_| ())),
            }
            let tip = (d - 10.0).max(0.0);
            let full = 20.0 * 0.5 * (d * d * sin - tip * tip * sin / (1.0 - cos));
            let m = crate::measure(&out).unwrap();
            assert!(
                m.shells == 1 && m.volume > last && (full - (m.volume - before)).abs() < 1e-6,
                "{wedge}°, {d}: added {} of {full}",
                m.volume - before
            );
            assert_eq!(faces_from_edge(&out, &body, WALL_EDGE), 1, "{wedge}°, {d}");
            last = m.volume;
        }
        let half = wedge.to_radians() / 2.0;
        let r = 12.0 * half.tan();
        assert!(matches!(fillet(&body, &[WALL_EDGE], r), Err(KernelError::TooLarge(_))), "{wedge}°: SMLib refuses r {r}");
        let (out, gone) = fillet_removing(&body, &[WALL_EDGE], r, FilletTurn::Mitre).unwrap_or_else(|e| panic!("{wedge}°, r {r}: {e}"));
        assert!(gone.is_empty());
        let section = 12.0 * r - r * r * (std::f64::consts::FRAC_PI_2 - half);
        let past = 2.0 * r - (r * r - 4.0).sqrt() - 0.5 * r * r * (2.0 / r).asin();
        let m = crate::measure(&out).unwrap();
        let full = 20.0 * (section - past);
        assert!(m.shells == 1 && (full - (m.volume - before)).abs() < 1e-6, "{wedge}°, r {r}: added {} of {full}", m.volume - before);
        assert_eq!(faces_from_edge(&out, &body, WALL_EDGE), 1, "{wedge}°, r {r}");
    }

    /// With the wall's side leaning back 30° (a wedge of 120°) a blend stands over the wall's
    /// foot, behind the line of the edge, and is whole there: where it runs past the top's end
    /// it continues the blend SMLib builds below it.
    #[test]
    fn a_blend_in_a_wedge_wider_than_square_is_whole_over_the_other_face() {
        wall_blends_run_on_past_the_tops_end(120.0);
    }

    /// With the wall's side upright, or leaning 30° over the top (wedges of 90° and 60°), no
    /// blend stands behind the line of the edge, and the blends run on past the top's end as
    /// they did.
    #[test]
    fn a_blend_in_a_wedge_up_to_square_runs_on_past_its_face() {
        for wedge in [90.0, 60.0] {
            wall_blends_run_on_past_the_tops_end(wedge);
        }
    }

    /// The boss's base edge is vertical, so its two faces lean alike along up and along down:
    /// SMLib gives the first distance to the first of the edge's two faces, here the plate's,
    /// whatever `flip` says. Where the chamfer fits the plate's face, carrying the face on does
    /// not run and the body is SMLib's, its material the section with the first distance along
    /// the plate. The face found to be first is the one SMLib takes: the chamfer asked along its
    /// normal is that same body (with the other face's normal it would be the other chamfer).
    #[test]
    fn a_two_sided_chamfer_that_fits_its_face_is_smlibs() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        let bytes = |b: &Body| crate::encode_native_body(b).unwrap();
        for (d1, side) in [(3.0, ChamferSide::Distance(5.0)), (5.0, ChamferSide::Distance(3.0)), (5.0, ChamferSide::Angle(60f64.to_radians()))] {
            let unflipped = bytes(&chamfer_sides(&body, &[BOSS_EDGE], d1, side, false).unwrap());
            for flip in [false, true] {
                let plain = chamfer_sides(&body, &[BOSS_EDGE], d1, side, flip).unwrap();
                assert_eq!(bytes(&plain), unflipped, "{d1} {side:?}: flipped");
                let out = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], d1, side, flip).unwrap();
                assert_eq!(bytes(&out), bytes(&plain), "{d1} {side:?} flip {flip}");
                let first = first_face_normal(&body, BOSS_EDGE, flip).expect("the first face");
                assert_eq!(first, Vec3::new(0.0, -1.0, 0.0), "the plate's front face");
                let along = crate::blend_smlib::chamfer_sides_along(&body, &[BOSS_EDGE], d1, side, first).unwrap();
                assert_eq!(bytes(&along), bytes(&plain), "{d1} {side:?} flip {flip}: along the first face's normal");
                if let ChamferSide::Distance(d2) = side {
                    let (p, q, area) = boss_chamfer_sides_section(d1, d2);
                    let added = crate::measure(&plain).unwrap().volume - before;
                    assert!((24.0 * area - added).abs() < 1e-4, "{d1} and {d2}: {added}");
                    for v in [Vec3::new(p.0, -10.5, 0.0), Vec3::new(q.0, q.1, 0.0)] {
                        assert!(has_vertex(&plain, v, 1e-6), "{d1} and {d2}: no vertex at {v:?}");
                    }
                }
            }
        }
    }

    /// Past the plate's face the first distance stays along the plate. 9 and 10, 10 and 9, and 8
    /// and 4 each run the contact line on the plate past x 32.5, where the face's top edge turns
    /// into the r16 corner's arc (to x 33.32, 34.91 and 34.40), and SMLib refuses each as too
    /// large. Carried on, each is its own chamfer and not the one with the distances the other
    /// way round: its contact line on the plate ends on the arc, its top edge meets the corner
    /// at x 32.5, its contact on the boss is where the second distance puts it, and its material
    /// is its section over the boss's 24 mm less the part past the arc. One shell, within the
    /// body's box.
    #[test]
    fn a_two_distance_chamfer_runs_on_past_where_its_face_turns_into_a_round() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap();
        // The plate's top edge is z = 0 up to x 32.5 and the corner's arc from there.
        let on_plate = |p: (f64, f64)| Vec3::new(p.0, -10.5, if p.0 > 32.5 { on_corner_arc(p.0) } else { 0.0 });
        for (d1, d2) in [(9.0, 10.0), (10.0, 9.0), (8.0, 4.0)] {
            for flip in [false, true] {
                let side = ChamferSide::Distance(d2);
                assert!(matches!(chamfer_sides(&body, &[BOSS_EDGE], d1, side, flip), Err(KernelError::TooLarge(_))), "SMLib refuses {d1} and {d2}");
                let out = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], d1, side, flip).unwrap_or_else(|e| panic!("{d1} and {d2}: {e}"));
                let m = crate::measure(&out).unwrap();
                let (p, q, area) = boss_chamfer_sides_section(d1, d2);
                assert!(p.0 > 32.5, "{d1} and {d2}: the contact on the plate is past its face");
                let (added, full) = (m.volume - before.volume, 24.0 * area - past_corner_arc(p, q));
                assert!(m.shells == 1 && (full - added).abs() < 1e-4, "{d1} and {d2} flip {flip}: added {added} of {full}");
                assert!(m.bbox.min.dist(before.bbox.min) < 1e-9 && m.bbox.max.dist(before.bbox.max) < 1e-9, "{d1} and {d2}: {:?}", m.bbox);
                let top = p.1 + (32.5 - p.0) * (q.1 - p.1) / (q.0 - p.0);
                for v in [on_plate(p), Vec3::new(32.5, top, 0.0), Vec3::new(q.0, q.1, 0.0)] {
                    assert!(has_vertex(&out, v, 1e-6), "{d1} and {d2} flip {flip}: no vertex at {v:?}");
                }
                let (p, q, _) = boss_chamfer_sides_section(d2, d1);
                for v in [on_plate(p), Vec3::new(q.0, q.1, 0.0)] {
                    assert!(!has_vertex(&out, v, 1e-3), "{d1} and {d2} flip {flip}: a vertex of the other order's chamfer at {v:?}");
                }
            }
        }
    }

    /// On the body with the plate's face carried on, the normal the chamfer is asked along
    /// decides which face takes the first distance: 9 and 10 along the plate's normal is 9 along
    /// the plate, and along the boss's normal at the edge (from the boss's axis to the edge) it
    /// is 9 along the boss, the chamfer of 10 and 9.
    #[test]
    fn a_carried_on_two_distance_chamfer_has_its_first_distance_along_the_normal_asked() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        let (cy, big, yp): (f64, f64, f64) = (-14.038301415494109, 12.90348790056394, -10.5);
        let boss = Vec3::new((big * big - (yp - cy).powi(2)).sqrt() / big, (yp - cy) / big, 0.0);
        for (first, plate, along_boss) in [(Vec3::new(0.0, -1.0, 0.0), 9.0, 10.0), (boss, 10.0, 9.0)] {
            let out = concave_overflow_round(&body, BOSS_EDGE, 10.0, |carried| {
                crate::blend_smlib::chamfer_sides_along(carried, &[BOSS_EDGE], 9.0, ChamferSide::Distance(10.0), first)
            })
            .expect("carried on");
            let (p, q, area) = boss_chamfer_sides_section(plate, along_boss);
            let (added, full) = (crate::measure(&out).unwrap().volume - before, 24.0 * area - past_corner_arc(p, q));
            assert!((full - added).abs() < 1e-4, "along {first:?}: added {added} of {full}");
        }
    }

    /// A box's four upright edges, each between two upright faces that lean alike along up and
    /// down: on each, for either `flip`, the face found to be first is the one SMLib gives the
    /// first distance (the chamfer asked along its normal is SMLib's body), and round the box it
    /// is a face across x on some edges and a face across y on others.
    #[test]
    fn the_first_face_of_an_upright_edge_is_the_one_smlib_takes() {
        let body = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let bytes = |b: &Body| crate::encode_native_body(b).unwrap();
        let mut across_x = 0;
        for edge in [Vec3::new(0.0, 0.0, 15.0), Vec3::new(10.0, 0.0, 15.0), Vec3::new(10.0, 20.0, 15.0), Vec3::new(0.0, 20.0, 15.0)] {
            for flip in [false, true] {
                let plain = chamfer_sides(&body, &[edge], 2.0, ChamferSide::Distance(3.0), flip).unwrap();
                let first = first_face_normal(&body, edge, flip).expect("the first face");
                let along = crate::blend_smlib::chamfer_sides_along(&body, &[edge], 2.0, ChamferSide::Distance(3.0), first).unwrap();
                assert_eq!(bytes(&along), bytes(&plain), "{edge:?} flip {flip}");
                across_x += usize::from(first.x != 0.0);
            }
        }
        assert_eq!(across_x, 4, "two of the four edges, for either flip");
    }

    /// With 5 along the boss, 6.38 along the plate is the largest that fits the plate's face (its
    /// contact at x 32.4994) and is SMLib's body; 6.4 SMLib fails as an operation and 6.45 it
    /// refuses as too large. Carried on they build, and the chamfer grows with its first distance
    /// through there, its material the section less the part past the arc.
    #[test]
    fn a_two_distance_chamfer_just_past_its_face_builds() {
        let body = boss_on_rounded_plate();
        let side = ChamferSide::Distance(5.0);
        let plain = |d: f64| chamfer_sides(&body, &[BOSS_EDGE], d, side, false);
        assert!(matches!(plain(6.4), Err(KernelError::Failed(_))));
        assert!(matches!(plain(6.45), Err(KernelError::TooLarge(_))));
        let fits = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], 6.38, side, false).unwrap();
        assert_eq!(crate::encode_native_body(&fits).unwrap(), crate::encode_native_body(&plain(6.38).unwrap()).unwrap());
        let before = crate::measure(&body).unwrap().volume;
        let mut last = before;
        for d in [6.38, 6.39, 6.4, 6.42, 6.45, 6.6, 8.0] {
            let out = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], d, side, false).unwrap_or_else(|e| panic!("{d}: {e}"));
            let m = crate::measure(&out).unwrap();
            let (p, q, area) = boss_chamfer_sides_section(d, 5.0);
            let full = 24.0 * area - past_corner_arc(p, q);
            assert!(m.shells == 1 && m.volume > last && (full - (m.volume - before)).abs() < 1e-4, "{d}: {}", m.volume);
            last = m.volume;
        }
    }

    /// A two-distance chamfer whose contact on the boss lies behind the line of the edge stands
    /// over the boss there and is whole: 10 and 12, 11 and 13, 12 and 15 (plate and boss) run
    /// past the plate's face, and carried on each is its section over the boss's 24 mm less the
    /// part past the corner's arc, its contact line on the boss runs the boss's height, and it
    /// is one face. 9 and 12 fits the plate's face and stands over the boss too: SMLib builds
    /// it, as whole, and 10 and 12 is more.
    #[test]
    fn a_two_distance_chamfer_is_whole_where_it_stands_over_the_boss() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        let added = |d1: f64, d2: f64| {
            let out = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], d1, ChamferSide::Distance(d2), false)
                .unwrap_or_else(|e| panic!("{d1} and {d2}: {e}"));
            let (p, q, area) = boss_chamfer_sides_section(d1, d2);
            assert!(q.0 < BOSS_EDGE.x, "{d1} and {d2}: the contact on the boss is behind the edge's line");
            let m = crate::measure(&out).unwrap();
            let (added, full) = (m.volume - before, 24.0 * area - past_corner_arc(p, q));
            assert!(m.shells == 1 && (full - added).abs() < 1e-4, "{d1} and {d2}: added {added} of {full}");
            for z in [0.0, -24.0] {
                assert!(has_vertex(&out, Vec3::new(q.0, q.1, z), 1e-6), "{d1} and {d2}: no vertex on the boss at z {z}");
            }
            assert_eq!(faces_from_edge(&out, &body, BOSS_EDGE), 1, "{d1} and {d2}");
            added
        };
        assert!(chamfer_sides(&body, &[BOSS_EDGE], 9.0, ChamferSide::Distance(12.0), false).is_ok(), "SMLib builds 9 and 12");
        let fits = added(9.0, 12.0);
        for (d1, d2) in [(10.0, 12.0), (11.0, 13.0), (12.0, 15.0)] {
            assert!(
                matches!(chamfer_sides(&body, &[BOSS_EDGE], d1, ChamferSide::Distance(d2), false), Err(KernelError::TooLarge(_))),
                "{d1} and {d2}"
            );
            assert!(added(d1, d2) > fits, "{d1} and {d2}");
        }
    }

    /// By a wall the first distance is along the wall's side, for either `flip`. 5 along it and
    /// 15 along the plate's top runs 5 past the top's end at x 30, which SMLib refuses; carried
    /// on it is the triangle of its two sides less its tip past x 30, 5 long and as high as its
    /// line stands off the top there, also where the wall leans back and the chamfer stands over
    /// its foot (a wedge of 120°). One face, one shell.
    #[test]
    fn a_two_distance_chamfer_by_a_wall_runs_on_past_the_tops_end() {
        for wedge in [90.0_f64, 120.0] {
            let body = walled_plate(wedge);
            let before = crate::measure(&body).unwrap().volume;
            let (sin, cos) = wedge.to_radians().sin_cos();
            let side = ChamferSide::Distance(15.0);
            // The chamfer's line falls from the wall to the top, 15 - 5 cos(wedge) along the top.
            let tip = 5.0 * 5.0 * sin / (15.0 - 5.0 * cos);
            let full = 20.0 * 0.5 * (5.0 * 15.0 * sin - 5.0 * tip);
            for flip in [false, true] {
                let first = first_face_normal(&body, WALL_EDGE, flip).expect("the first face");
                assert!(first.dist(Vec3::new(sin, -cos, 0.0)) < 1e-12, "{wedge}° flip {flip}: {first:?}");
                assert!(matches!(chamfer_sides(&body, &[WALL_EDGE], 5.0, side, flip), Err(KernelError::TooLarge(_))), "{wedge}°");
                let out = chamfer_sides_carrying_face(&body, &[WALL_EDGE], 5.0, side, flip).unwrap_or_else(|e| panic!("{wedge}° flip {flip}: {e}"));
                let m = crate::measure(&out).unwrap();
                assert!(m.shells == 1 && (full - (m.volume - before)).abs() < 1e-6, "{wedge}° flip {flip}: added {} of {full}", m.volume - before);
                assert_eq!(faces_from_edge(&out, &body, WALL_EDGE), 1, "{wedge}° flip {flip}");
            }
        }
    }

    /// By a wall the distance is along the wall's side and the angle is from it, for either
    /// `flip`: the chamfer's contact on the plate's top is `d sin(angle) / sin(wedge + angle)`
    /// from the edge. 8 at 60° and 5 at 80° by an upright wall (13.86 and 28.36 along the top),
    /// and 8 at 45° by a wall leaning back 30° (21.86, the chamfer standing over the wall's
    /// foot), run past the top's end at x 30, which SMLib refuses. Carried on, each is the
    /// triangle of its two sides less its tip past x 30, its contact line on the wall is the
    /// distance up it, and its line meets the plane of the top's end as high as the tip is. With
    /// the angle taken from the top instead, 8 at 60° would reach 4.62 along the top and fit.
    /// One face, one shell.
    #[test]
    fn a_distance_and_angle_chamfer_by_a_wall_runs_on_past_the_tops_end() {
        for (wedge, d, degrees) in [(90.0_f64, 8.0, 60.0_f64), (90.0, 5.0, 80.0), (120.0, 8.0, 45.0)] {
            let body = walled_plate(wedge);
            let before = crate::measure(&body).unwrap().volume;
            let (sin, cos) = wedge.to_radians().sin_cos();
            let angle = degrees.to_radians();
            let along_top = d * angle.sin() / (wedge.to_radians() + angle).sin();
            // The chamfer's line stands this high over the top at its end.
            let tip = (along_top - 10.0) * d * sin / (along_top - d * cos);
            let full = 20.0 * 0.5 * (d * along_top * sin - (along_top - 10.0) * tip);
            for flip in [false, true] {
                let what = format!("{wedge}°, {d} at {degrees}°, flip {flip}");
                let side = ChamferSide::Angle(angle);
                assert!(matches!(chamfer_sides(&body, &[WALL_EDGE], d, side, flip), Err(KernelError::TooLarge(_))), "{what}");
                let out = chamfer_sides_carrying_face(&body, &[WALL_EDGE], d, side, flip).unwrap_or_else(|e| panic!("{what}: {e}"));
                let m = crate::measure(&out).unwrap();
                assert!(m.shells == 1 && (full - (m.volume - before)).abs() < 1e-6, "{what}: added {} of {full}", m.volume - before);
                for z in [0.0, 20.0] {
                    for v in [Vec3::new(20.0 + d * cos, 10.0 + d * sin, z), Vec3::new(30.0, 10.0 + tip, z)] {
                        assert!(has_vertex(&out, v, 1e-6), "{what}: no vertex at {v:?}");
                    }
                }
                assert_eq!(faces_from_edge(&out, &body, WALL_EDGE), 1, "{what}");
            }
        }
    }

    /// The top is carried on as a slab that reaches four times the body's size and the distance
    /// from the edge. By an upright wall, 5 at 88.6° reaches 204.59 along the top, short of the
    /// slab's end (207.62), and builds: the triangle of its two sides less its tip past x 30.
    /// At 88.7° it would reach 220.33, past the slab: it runs off the slab's end as it ran off
    /// the top's, and SMLib's refusal stands.
    #[test]
    fn a_distance_and_angle_chamfer_reaching_past_the_carried_on_slab_keeps_smlibs_refusal() {
        let body = walled_plate(90.0);
        let before = crate::measure(&body).unwrap().volume;
        let slab = 4.0 * (body.size() + 5.0);
        let (short, past) = (88.6_f64.to_radians(), 88.7_f64.to_radians());
        assert!(5.0 * short.tan() < slab && slab < 5.0 * past.tan(), "the slab reaches {slab}");
        let out = chamfer_sides_carrying_face(&body, &[WALL_EDGE], 5.0, ChamferSide::Angle(short), false).unwrap();
        let along_top = 5.0 * short.tan();
        let full = 20.0 * 0.5 * (5.0 * along_top - (along_top - 10.0).powi(2) * 5.0 / along_top);
        let m = crate::measure(&out).unwrap();
        assert!(m.shells == 1 && (full - (m.volume - before)).abs() < 1e-6, "added {} of {full}", m.volume - before);
        let plain = chamfer_sides(&body, &[WALL_EDGE], 5.0, ChamferSide::Angle(past), false).unwrap_err();
        assert!(matches!(plain, KernelError::TooLarge(_)), "{plain}");
        let kept = chamfer_sides_carrying_face(&body, &[WALL_EDGE], 5.0, ChamferSide::Angle(past), false).unwrap_err();
        assert_eq!(kept.to_string(), plain.to_string());
    }

    /// On the boss's base edge the distance is along the plate and the angle is from it:
    /// solidcore turns the angle into the distance along the boss at the edge's wedge,
    /// `d sin(angle) / sin(wedge + angle)`, and the chamfer is the two-distance one
    /// ([`boss_chamfer_sides_section`]); 5 at 60°, which fits, is SMLib's body and that section.
    /// At 45°, 6.43 is the largest that fits the plate's face (its contact at x 32.4942) and is
    /// SMLib's; 6.45 SMLib fails as an operation, and from 6.5 it refuses as too large, naming
    /// the part of the sizes that fits. Carried on they build, the chamfer growing with its
    /// distance through there, each its section over the boss's 24 mm less the part past the
    /// corner's arc; 8 at 45° has its contact line on the plate ending on the arc, its top edge
    /// meeting the corner at x 32.5, and its contact on the boss. 10 at 60° (12.06 along the
    /// boss) has its contact on the boss behind the edge's line and is whole there. One face,
    /// one shell.
    #[test]
    fn a_distance_and_angle_chamfer_runs_on_past_where_its_face_turns_into_a_round() {
        let body = boss_on_rounded_plate();
        let before = crate::measure(&body).unwrap().volume;
        let wedge = ((-10.5_f64 + 14.038301415494109) / 12.90348790056394).acos();
        let section = |d: f64, angle: f64| boss_chamfer_sides_section(d, d * angle.sin() / (wedge + angle).sin());
        let plain = |d: f64, angle: f64| chamfer_sides(&body, &[BOSS_EDGE], d, ChamferSide::Angle(angle), false);
        let carried = |d: f64, angle: f64| {
            let out = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], d, ChamferSide::Angle(angle), false).unwrap_or_else(|e| panic!("{d}: {e}"));
            let (p, q, area) = section(d, angle);
            let m = crate::measure(&out).unwrap();
            let (added, full) = (m.volume - before, 24.0 * area - past_corner_arc(p, q));
            assert!(m.shells == 1 && (full - added).abs() < 1e-4, "{d}: added {added} of {full}");
            assert_eq!(faces_from_edge(&out, &body, BOSS_EDGE), 1, "{d}");
            (out, added)
        };
        let steep = 60f64.to_radians();
        let fits = crate::measure(&plain(5.0, steep).unwrap()).unwrap().volume - before;
        assert!((24.0 * section(5.0, steep).2 - fits).abs() < 1e-4, "{fits}");

        let angle = 45f64.to_radians();
        assert!(matches!(plain(6.45, angle), Err(KernelError::Failed(_))));
        assert!(matches!(plain(8.0, angle), Err(KernelError::TooLarge(why)) if why.contains("times the distances given")));
        let (largest, mut last) = carried(6.43, angle);
        assert_eq!(crate::encode_native_body(&largest).unwrap(), crate::encode_native_body(&plain(6.43, angle).unwrap()).unwrap());
        for d in [6.44, 6.45, 6.5, 7.0, 8.0] {
            let (out, added) = carried(d, angle);
            assert!(added > last, "{d}: {added}");
            last = added;
            if d == 8.0 {
                let (p, q, _) = section(d, angle);
                let top = p.1 + (32.5 - p.0) * (q.1 - p.1) / (q.0 - p.0);
                for v in [Vec3::new(p.0, -10.5, on_corner_arc(p.0)), Vec3::new(32.5, top, 0.0), Vec3::new(q.0, q.1, 0.0)] {
                    assert!(has_vertex(&out, v, 1e-6), "no vertex at {v:?}");
                }
            }
        }

        assert!(matches!(plain(10.0, steep), Err(KernelError::TooLarge(_))));
        let (out, _) = carried(10.0, steep);
        let (_, q, _) = section(10.0, steep);
        assert!(q.0 < BOSS_EDGE.x, "the contact on the boss is behind the edge's line");
        for z in [0.0, -24.0] {
            assert!(has_vertex(&out, Vec3::new(q.0, q.1, z), 1e-6), "no vertex on the boss at z {z}");
        }
    }

    /// What carrying the face on does not build keeps SMLib's own error: 6 along the plate with
    /// 12 along the boss, whose contact on the plate fits its face (x 28.35). SMLib fails it for
    /// the boss's side, which carrying the plate's face on does not change.
    #[test]
    fn a_two_sided_chamfer_that_is_not_carried_on_keeps_smlibs_error() {
        let body = boss_on_rounded_plate();
        let side = ChamferSide::Distance(12.0);
        let plain = chamfer_sides(&body, &[BOSS_EDGE], 6.0, side, false).unwrap_err();
        assert!(matches!(&plain, KernelError::Failed(why) if why.contains("reach past the rails")), "{plain}");
        let kept = chamfer_sides_carrying_face(&body, &[BOSS_EDGE], 6.0, side, false).unwrap_err();
        assert_eq!(kept.to_string(), plain.to_string());
        assert_eq!(std::mem::discriminant(&kept), std::mem::discriminant(&plain));
    }

    /// The first face is the second of the pair only where it leans more, the first where they
    /// lean alike, and neither where rounding would decide.
    #[test]
    fn the_first_face_is_chosen_as_smlib_chooses_it_or_not_at_all() {
        assert_eq!(second_face_first(0.0, 0.0), Some(false));
        assert_eq!(second_face_first(-0.0, 0.0), Some(false));
        assert_eq!(second_face_first(0.0, 0.5), Some(true));
        assert_eq!(second_face_first(0.5, 0.0), Some(false));
        assert_eq!(second_face_first(0.0, 1e-12), None);
        assert_eq!(second_face_first(1e-12, 0.0), None);
        assert_eq!(second_face_first(f64::NAN, 0.0), None);
    }

    /// A search where every candidate is refused as too large keeps the smallest size it tried
    /// and that candidate's own refusal, and builds nothing: the caller can say why nothing
    /// smaller builds instead of repeating the first refusal's estimate.
    #[test]
    fn a_search_that_builds_nothing_keeps_its_smallest_refusal() {
        let search = largest_that_fits(0.3, None, |d| Err(KernelError::TooLarge(format!("refused at {d}")))).unwrap();
        assert!(search.found.is_none());
        let (at, e) = search.smallest_refused.expect("a refusal");
        assert!(at > 0.0 && at < 0.001, "the bisection goes down to the step: {at}");
        assert_eq!(e.to_string(), KernelError::TooLarge(format!("refused at {at}")).to_string());
    }

    /// A search that builds a candidate reports it and no refusal.
    #[test]
    fn a_search_that_builds_reports_what_it_built() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let search =
            largest_that_fits(0.3, None, |d| if d <= 0.1 { Ok(body.clone()) } else { Err(KernelError::TooLarge("too large".into())) }).unwrap();
        let (at, _) = search.found.expect("built");
        assert!((0.099..=0.1).contains(&at), "{at}");
        assert!(search.smallest_refused.is_none());
    }

    /// A candidate failing another way (the check) narrows the search like a refusal: the size
    /// found is the largest that built, not the request's estimate.
    #[test]
    fn a_candidate_failing_the_check_narrows_the_search() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let search =
            largest_that_fits(1.0, None, |r| if r <= 0.95 { Ok(body.clone()) } else { Err(KernelError::Failed("the check refuses it".into())) })
                .unwrap();
        let (at, _) = search.found.expect("built");
        assert!((0.949..=0.95).contains(&at), "{at}");
    }

    /// A refusal that names an estimate within a step of the largest size that fits settles the
    /// search in two blends (the estimate builds, the next step up is refused) where bisection
    /// takes eleven, and finds the same size. Work counter: `FitSearch::tried`.
    #[test]
    fn a_close_estimate_settles_the_search_in_two_blends() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        // Builds up to 1.904 mm (q = 0.001 for a 2 mm request); a refusal estimates 1.904762.
        // Sizes are n·q, as the search builds them.
        let blend = |r: f64| {
            if r <= 1.9045 { Ok(body.clone()) } else { Err(KernelError::TooLarge(format!("too large; {}", largest_fit_note("radius", 1.904762)))) }
        };
        // Bisection alone: the same refusals, naming no estimate.
        let unnamed = |r: f64| if r <= 1.9045 { Ok(body.clone()) } else { Err(KernelError::TooLarge("too large".into())) };
        let plain = largest_that_fits(2.0, None, unnamed).unwrap();
        let led = largest_that_fits(2.0, Some(1.904762), blend).unwrap();
        assert_eq!(plain.found.as_ref().map(|f| f.0), Some(1904.0 * 0.001));
        assert_eq!(led.found.as_ref().map(|f| f.0), Some(1904.0 * 0.001));
        assert!(plain.tried >= 11, "bisection alone: {}", plain.tried);
        assert_eq!(led.tried, 2);
        // With no estimate of its own, the first candidate refused leads with its estimate.
        let after_one = largest_that_fits(2.0, None, blend).unwrap();
        assert_eq!(after_one.found.as_ref().map(|f| f.0), Some(1904.0 * 0.001));
        assert!(after_one.tried < plain.tried, "{} against {}", after_one.tried, plain.tried);
    }

    /// The seed-2 design of the engine's fuzzer refused a 3 mm round naming 2.99 mm (the size less
    /// a margin: the ball was as wide as a face it rolls on), while its candidates' refusals named
    /// 1.24 mm against 1.25 that builds. The 2.99 is not followed (a 2.99 mm round there ran 834 s
    /// before it failed): the search bisects to 1.5 first, then follows 1.24 up a step, then 2,
    /// 4, ... and finds 1.25 as bisection does, in fewer blends. Over every estimate, above or
    /// below the size, the answer is bisection's.
    #[test]
    fn estimates_below_or_above_the_size_find_what_bisection_finds() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        // A refusal naming `names` (none where it is NaN), of a round that builds up to `fits`.
        let refusing = |fits: f64, names: f64| {
            let body = body.clone();
            move |r: f64| {
                if r <= fits + 1e-9 {
                    Ok(body.clone())
                } else if names.is_nan() {
                    Err(KernelError::TooLarge("too large".into()))
                } else {
                    Err(KernelError::TooLarge(format!("too large; {}", largest_fit_note("radius", names))))
                }
            }
        };
        let plain = largest_that_fits(3.0, None, refusing(1.25, f64::NAN)).unwrap();
        let tried = std::cell::RefCell::new(Vec::new());
        let seed2 = refusing(1.25, 1.24);
        let led = largest_that_fits(3.0, Some(2.99), |r| {
            tried.borrow_mut().push(r);
            seed2(r)
        })
        .unwrap();
        assert_eq!(tried.borrow().first().copied(), Some(1500.0 * 0.001), "{:?}", tried.borrow());
        assert!(tried.borrow().iter().all(|r| *r < 2.0), "{:?}", tried.borrow());
        assert_eq!(plain.found.as_ref().map(|f| f.0), Some(1250.0 * 0.001));
        assert_eq!(led.found.as_ref().map(|f| f.0), Some(1250.0 * 0.001));
        assert!(led.tried < plain.tried, "{} against {}", led.tried, plain.tried);
        // Every size the estimates may point at, over and under: the same answer as bisection.
        for fits in [0.5, 1.0, 1.25, 1.999, 2.5, 2.998] {
            let plain = largest_that_fits(3.0, None, refusing(fits, f64::NAN)).unwrap().found.map(|f| f.0);
            for names in [0.001, 0.4, fits - 0.01, fits - 0.001, fits, fits + 0.001, fits + 0.2, 2.99, 5.0] {
                for hint in [None, Some(names), Some(2.99)] {
                    let led = largest_that_fits(3.0, hint, refusing(fits, names)).unwrap();
                    assert_eq!(led.found.map(|f| f.0), plain, "fits {fits}, refusals name {names}, hint {hint:?}");
                    assert!(led.tried <= 22, "{}", led.tried);
                }
            }
        }
    }

    /// A refusal names its estimate as the largest size that fits, and only then.
    #[test]
    fn a_refusal_names_its_estimate() {
        assert_eq!(named_fit("too large, at the edge at (1, 2, 3); the largest radius that fits is 1.904762 mm (tangent chain)"), Some(1.904762));
        assert_eq!(named_fit(&format!("x; {}", largest_fit_note("distance", 0.25))), Some(0.25));
        assert_eq!(named_fit("too large; the largest that fits is 2 times the distances given"), None);
        assert_eq!(named_fit("the trim leaks"), None);
    }

    /// The largest-fit clause comes out of a refusal with its separator, and nothing else does.
    #[test]
    fn the_fit_note_comes_out_with_its_separator() {
        assert_eq!(
            without_fit_note("larger than the faces allow, at the edge at (1, 2, 3); the largest radius that fits is 3.75 mm (tangent chain)"),
            "larger than the faces allow, at the edge at (1, 2, 3) (tangent chain)"
        );
        assert_eq!(without_fit_note("too large: the largest distance that fits is 1.6 mm"), "too large");
        assert_eq!(without_fit_note("the trim leaks"), "the trim leaks");
    }

    /// A round (`round`) or an equal-distance chamfer of the edges nearest `pts`.
    fn blend(body: &Body, pts: &[Vec3], size: f64, round: bool) -> Result<Body> {
        if round { fillet(body, pts, size) } else { chamfer(body, pts, size) }
    }

    fn vol(b: &Body) -> f64 {
        b.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume
    }

    /// Whether a vertex of `b` lies within `tol` of `p`.
    fn has_vertex(b: &Body, p: Vec3, tol: f64) -> bool {
        crate::native::brep_record(b).unwrap().vertices.iter().any(|v| v3(v.point).dist(p) < tol)
    }

    /// The ladder's 08-chamfers/008: a boss on a block, rounded where it meets the block, picked
    /// at the top of its seam. The pick takes the top rim, and the chain keeps to it: it never
    /// runs down the seam into the round's own seam, which continues it tangentially.
    #[test]
    fn a_chain_picked_where_a_seam_meets_a_rim_keeps_to_the_rim() {
        let block = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
        let boss = crate::build::cylinder(Vec3::new(15.0, 15.0, 20.0), Vec3::Z, 8.0, 10.0).unwrap();
        let joined = crate::boolean(&block, &boss, crate::BoolOp::Union).unwrap().unwrap();
        let rec = joined.record();
        // The seam: the edge both of whose uses lie on the boss's side.
        let seam = rec.edges.iter().find(|e| e.uses.len() == 2 && e.uses[0].face == e.uses[1].face).unwrap();
        let ends = seam.vertices.map(|v| v3(rec.vertices[v as usize].point));
        let (foot, top) = if ends[0].z < ends[1].z { (ends[0], ends[1]) } else { (ends[1], ends[0]) };
        let rounded = fillet(&joined, &[foot], 1.0).unwrap();
        let chain = tangent_chain(&rounded, &[top]).unwrap();
        assert_eq!(chain, vec![top], "the chain left the top rim");
        assert!(chamfer(&rounded, &chain, 1.0).is_ok());
        // Even from a pick on the seam itself, the chain takes no seam: the round's continues
        // the boss's tangentially but has no corner to blend.
        let side = (top + Vec3::new(top.x, top.y, 21.0)) * 0.5;
        assert_eq!(tangent_chain(&rounded, &[side]).unwrap(), vec![side], "the chain ran into the round's seam");
    }

    /// A prism of the regular `n`-gon of circumradius 20, 5 high: its outline turns 360 / n degrees
    /// at every vertex. The point halfway along its first top edge, and the number of top edges.
    fn polygon_prism(n: usize) -> (Body, Vec3) {
        use solidcore_record::{Loop2, Plane, Region2, Vec2};
        let at = |k: usize| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            Vec2::new(20.0 * a.cos(), 20.0 * a.sin())
        };
        let outer = Loop2::polygon(&(0..n).map(at).collect::<Vec<_>>());
        let body = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 5.0).unwrap().pop().unwrap();
        let (a, b) = (at(0), at(1));
        (body, Vec3::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0, 5.0))
    }

    /// A tangent chain continues through an outline turning 2° or less at a vertex (the one angle
    /// for a chain, [`solidcraft_geom::TANGENT_CHAIN`]): a 240-gon, 1.5° at every vertex, chains
    /// whole from one pick, and so does one that blends; a 120-gon, 3° at every vertex, stops at
    /// the pick's own edge.
    #[test]
    fn a_chain_runs_through_a_one_and_a_half_degree_kink_and_stops_at_three() {
        let (near, pick) = polygon_prism(240);
        assert_eq!(tangent_chain_edges(&near, &[pick]).unwrap().len(), 240, "the whole top outline");
        assert_eq!(tangent_chain(&near, &[pick]).unwrap().len(), 240);
        let (corner, pick) = polygon_prism(120);
        assert_eq!(tangent_chain_edges(&corner, &[pick]).unwrap().len(), 1, "3° is a corner");
    }

    /// Smoothness is decided all along an edge. A bar 40 × 16 × 10 whose front face runs tangent
    /// into a cylinder R8 at its far end, its front top edge rounded r2: the round's contact with
    /// the front face and the front face's tangent line with the wall are smooth (plane against
    /// cylinder, tangent all along); the round's edge with the wall is tangent at its lower end
    /// (40, 0, 8) only, a crease everywhere else; a box edge is a crease. A knife edge (a wall of
    /// radius 21 running out into a plane, #851: its faces tangent all along but folded back on
    /// each other) is a cusp, which is not smooth.
    #[test]
    fn an_edge_is_smooth_only_where_its_faces_are_tangent_all_along_it() {
        let bar = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 16.0, 10.0)).unwrap();
        let post = crate::build::cylinder(Vec3::new(40.0, 8.0, 0.0), Vec3::Z, 8.0, 20.0).unwrap();
        let peg = crate::boolean(&bar, &post, crate::BoolOp::Union).unwrap().unwrap();
        let rounded = fillet(&peg, &[Vec3::new(20.0, 0.0, 10.0)], 2.0).unwrap();
        let rec = Faces::of(&rounded).unwrap();
        let smooth = |p: Vec3| smooth_join(&rec, blend_edges(&rounded, &[p]).unwrap()[0]).unwrap();
        assert!(smooth(Vec3::new(20.0, 0.0, 8.0)), "the round's contact with the front face");
        assert!(smooth(Vec3::new(40.0, 0.0, 4.0)), "the front face's tangent line with the wall");
        assert!(!smooth(Vec3::new(37.2641, 0.4827, 9.3025)), "the round's edge with the wall");
        assert!(!smooth(Vec3::new(20.0, 0.0, 0.0)), "the bar's front bottom edge");
        let tip = 62.0 + 1.0 / 6.0;
        let slab = crate::build::box_solid(Vec3::new(0.0, -21.0, 0.0), Vec3::new(tip, 0.0, 10.0)).unwrap();
        let disc = crate::build::cylinder(Vec3::new(tip, -21.0, 0.0), Vec3::Z, 21.0, 10.0).unwrap();
        let knife = crate::boolean(&slab, &disc, crate::BoolOp::Cut).unwrap().unwrap();
        let rec = Faces::of(&knife).unwrap();
        assert!(!smooth_join(&rec, blend_edges(&knife, &[Vec3::new(tip, 0.0, 5.0)]).unwrap()[0]).unwrap(), "the knife edge");
    }

    #[test]
    fn g2_fillet_takes_off_what_the_round_does() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(20.0, 0.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.face_count(), body.face_count() + 1);
        // The curvature section's default thumbweight is equal area: on the round's rails it takes
        // off what the round does, (1 - pi/4) setback² at a right-angle corner.
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 8000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
    }

    #[test]
    fn g2_fillet_handles_concave_edges_and_separate_edges() {
        let lp = Loop2::polygon(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(20.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 20.0),
            Vec2::new(0.0, 20.0),
        ]);
        let body = crate::build::extrude(&Plane::XY, &[Region2 { outer: lp, holes: vec![] }], 0.0, 20.0).unwrap().remove(0);
        let out = fillet_g2(&body, &[Vec3::new(10.0, 10.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 6000.0 + (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(20.0, 0.0, 10.0), Vec3::new(0.0, 20.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.face_count(), body.face_count() + 2);
    }

    #[test]
    fn g2_fillet_refuses_sections_crossing_a_cap_hole() {
        // The hole lies inside the setback without touching either side face.
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0), Vec2::new(0.0, 20.0)]),
            holes: vec![Loop2::circle(Vec2::new(0.5, 0.5), 0.15)],
        };
        for plane in [Plane::XY, Plane::XZ] {
            let body = crate::extrude(&plane, std::slice::from_ref(&region), 0.0, 20.0).unwrap().remove(0);
            let pick = plane.to_world(Vec2::ZERO) + plane.normal() * 10.0;
            assert_eq!(body.crossed_face_loops().unwrap(), 0);
            let out = fillet_g2(&body, &[pick], 0.5).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            assert_eq!(out.crossed_face_loops().unwrap(), 0);
            assert_eq!(out.face_count(), body.face_count() + 1);
            assert!(fillet_g2(&body, &[pick], 2.0).is_err());
        }
    }

    #[test]
    fn g2_fillet_meets_sloped_planar_ends() {
        // Inward and outward sloping ends, and one square end, in two orientations (#2708).
        for plane in [Plane::XY, Plane::XZ] {
            for (left, right) in [(-0.5, -0.5), (0.5, 0.5), (0.0, 0.5), (-0.25, 0.5)] {
                let region = Region2 {
                    outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0 - right * 20.0, 20.0), Vec2::new(left * 20.0, 20.0)]),
                    holes: vec![],
                };
                let body = crate::extrude(&plane, &[region], 0.0, 20.0).unwrap().pop().unwrap();
                let pick = plane.to_world(Vec2::new(20.0, 0.0)) + plane.normal() * 20.0;
                let out = fillet_g2(&body, &[pick], 5.0).unwrap();
                assert!(out.validity().is_empty(), "{:?}", out.validity());
                assert_eq!(out.crossed_face_loops().unwrap(), 0);
                assert_eq!(out.face_count(), 7);
                // Analytic volume: the equal-area section removes (1 - pi/4) r², and its first
                // moment ∫y dA is 0.0474559669443 r³ (Green's theorem on the section at the
                // square corner's equal-area thumbweight). Axial length is 40−(left+right)y.
                let removed = 40.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 25.0 - (left + right) * 0.0474559669443 * 125.0;
                let expected = 16000.0 - 4000.0 * (left + right) - removed;
                let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
                assert!((volume - expected).abs() < 0.05, "{volume} vs {expected}");
            }
        }
    }

    #[test]
    fn g2_fillet_on_concave_edge_meets_caps_sloped_in_both_directions() {
        let region = Region2 {
            outer: Loop2::polygon(&[
                Vec2::ZERO,
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 10.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(10.0, 20.0),
                Vec2::new(0.0, 20.0),
            ]),
            holes: vec![],
        };
        let body = crate::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
        // z -> z + 0.3x + 0.4y tilts both caps while leaving the vertical edge
        // and its side planes unchanged. This shear preserves volume.
        let body = crate::transform_matrix(&body, [[1.0, 0.0, 0.3, 0.0], [0.0, 1.0, 0.4, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(10.0, 10.0, 17.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.crossed_face_loops().unwrap(), 0);
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 6000.0 + (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
    }

    #[test]
    fn g2_fillet_refuses_invalid_geometry() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let edge = [Vec3::new(20.0, 0.0, 10.0)];
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e7] {
            assert!(fillet_g2(&body, &edge, radius).is_err());
        }
        assert!(fillet_g2(&body, &[], 2.0).is_err());
        assert!(fillet_g2(&body, &[Vec3::new(f64::NAN, 0.0, 0.0)], 2.0).is_err());
        assert!(fillet_g2(&body, &[Vec3::new(1000.0, 0.0, 0.0)], 2.0).is_err());
        // Wider than the faces (20), over their far edges, cliffs: cut as if they went on. The
        // section is the equal-area one from 25 out on each face; it takes the region between it
        // and the corner inside the 20 x 20 section (the area under the curve clipped to the
        // square, 2e4 strips on a 4e5-piece polygon of the curve), along 20.
        let wide = fillet_g2(&body, &edge, 25.0).unwrap();
        let v = crate::measure(&wide).unwrap().volume;
        assert!((v - 5329.0618943703).abs() < 1e-5 * 5329.0618943703, "{v}");
        assert!(wide.validity().is_empty());
    }

    /// A box with a cylinder joined across one side (#556): the bottom edge of that side runs
    /// into the cylinder, which is curved where the edge ends.
    fn box_and_boss() -> Body {
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let c = crate::build::cylinder(Vec3::new(40.0, 20.0, 0.0), Vec3::Z, 12.0, 40.0).unwrap();
        crate::ops::boolean(&b, &c, crate::BoolOp::Union).unwrap().unwrap()
    }

    #[test]
    fn a_blend_runs_into_a_cylinder_at_its_end() {
        let body = box_and_boss();
        let v0 = vol(&body);
        // The edge (40, 0..8, 0) ends in the cylinder wall at y = 8.
        let len = 20.0 - (144.0f64 - 0.0).sqrt();
        for (round, size, area) in [(true, 0.5, (1.0 - std::f64::consts::PI / 4.0) * 0.25), (false, 0.5, 0.125)] {
            let out = blend(&body, &[Vec3::new(40.0, 4.0, 0.0)], size, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            let removed = v0 - vol(&out);
            // The section runs on a little past y = 8 under the cylinder wall.
            assert!(removed > area * len * 0.99 && removed < area * (len + 0.02), "{removed} vs {}", area * len);
            assert_eq!(out.face_count(), body.face_count() + 1);
        }
    }

    #[test]
    fn a_blend_ends_on_a_hole_wall() {
        // A plate with a hole cutting through the middle of its top front edge: each piece of
        // that edge ends on the hole's wall.
        let plate = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0)).unwrap();
        let hole = crate::build::cylinder(Vec3::new(20.0, 0.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
        let body = crate::ops::boolean(&plate, &hole, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        for (round, area) in [(true, (1.0 - std::f64::consts::PI / 4.0)), (false, 0.5)] {
            let out = blend(&body, &[Vec3::new(5.0, 0.0, 10.0)], 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            let removed = v0 - vol(&out);
            assert!(removed > area * 14.0 && removed < area * 15.2, "{removed}");
        }
    }

    #[test]
    fn a_chamfer_of_a_hole_corner_is_one_bevel() {
        let plate = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 10.0)).unwrap();
        let pocket = crate::build::box_solid(Vec3::new(10.0, 10.0, -1.0), Vec3::new(30.0, 30.0, 12.0)).unwrap();
        let body = crate::ops::boolean(&plate, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        let one = chamfer(&body, &[Vec3::new(10.0, 10.0, 5.0)], 2.0).unwrap();
        assert!(one.validity().is_empty(), "{:?}", one.validity());
        assert_eq!(one.unmeshed_faces(), 0);
        let added = vol(&one) - v0;
        // The bevel fills the hole corner: a right triangle of legs 2 mm, times the 10 mm edge.
        assert!(added > 19.0 && added < 21.0, "added {added}, want 20");
        assert_eq!(one.face_count(), body.face_count() + 1, "one bevel face");
        let corners = [Vec3::new(10.0, 10.0, 5.0), Vec3::new(30.0, 10.0, 5.0), Vec3::new(30.0, 30.0, 5.0), Vec3::new(10.0, 30.0, 5.0)];
        let all = chamfer(&body, &corners, 2.0).unwrap();
        assert!(all.validity().is_empty(), "{:?}", all.validity());
        let added = vol(&all) - v0;
        assert!(added > 76.0 && added < 84.0, "added {added}, want 80");
        assert_eq!(all.face_count(), body.face_count() + 4, "one bevel face per corner");
    }

    #[test]
    fn a_chamfer_ends_across_two_faces() {
        // A plate's outline chamfered, then the mitre at a corner chamfered: its low end meets
        // both side faces, and the chamfer plane cuts back the vertical edge between them.
        let k = 5.0;
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(100.0, 50.0, 20.0)).unwrap();
        let outline = [Vec3::new(50.0, 0.0, 20.0), Vec3::new(100.0, 25.0, 20.0), Vec3::new(50.0, 50.0, 20.0), Vec3::new(0.0, 25.0, 20.0)];
        let c = chamfer(&b, &outline, k).unwrap();
        let out = chamfer(&c, &[Vec3::new(k / 2.0, k / 2.0, 20.0 - k / 2.0)], 1.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.lumps().unwrap().len(), 1);
        assert_eq!(out.face_count(), c.face_count() + 1);
        // About the triangle (sides 1 and 1 at 120°) along the mitre; its ends slant.
        let removed = vol(&c) - vol(&out);
        let along = 0.5 * 120f64.to_radians().sin() * k * 3f64.sqrt();
        assert!(removed > along * 0.8 && removed < along * 1.2, "{removed} vs {along}");
    }

    #[test]
    fn a_curved_edge_blend_ends_in_a_face_at_an_angle() {
        // A groove of radius 8 along y with its axis 4 below the top: the arcs where it meets the
        // front and back faces end in the top face at 60° to it.
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 20.0)).unwrap();
        let c = crate::build::cylinder(Vec3::new(20.0, -1.0, 16.0), Vec3::Y, 8.0, 22.0).unwrap();
        let body = crate::ops::boolean(&b, &c, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        for round in [true, false] {
            let out = blend(&body, &[Vec3::new(20.0, 0.0, 8.0)], 0.5, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // About the section along the arc (240°, at the section's distance from the axis).
            let area = if round { (1.0 - std::f64::consts::PI / 4.0) * 0.25 } else { 0.125 };
            let along = 8.2 * 240f64.to_radians();
            let removed = v0 - vol(&out);
            assert!(removed > area * along * 0.9 && removed < area * along * 1.1, "{removed} vs {}", area * along);
        }
    }

    #[test]
    fn a_blend_of_an_edge_running_into_a_rounded_corner_follows_it() {
        // A box with one vertical edge rounded: a top edge meeting that round smoothly is blended
        // with the round's top arc and the next top edge, as one smooth chain (its tangent chain,
        // as the Fillet and Chamfer features ask for it).
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
        let rounded = fillet(&b, &[Vec3::new(40.0, 30.0, 10.0)], 5.0).unwrap();
        let chain = tangent_chain(&rounded, &[Vec3::new(20.0, 30.0, 20.0)]).unwrap();
        assert_eq!(chain.len(), 3, "the top edge, the round's top arc and the next top edge");
        for round in [true, false] {
            let out = blend(&rounded, &chain, 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            assert_eq!(out.face_count(), rounded.face_count() + 3);
        }
    }

    #[test]
    fn a_blend_along_a_boss_ends_on_the_box_side_beside_it() {
        // The boss's bottom edge outside the box (a half circle) ends at the box's side, square to
        // it: the side face takes the end section.
        let body = box_and_boss();
        let v0 = vol(&body);
        for (round, area, c) in [
            (true, (1.0 - std::f64::consts::PI / 4.0) * 0.64, 0.8 * (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * (4.0 - std::f64::consts::PI))),
            (false, 0.32, 0.8 / 3.0),
        ] {
            // The half circle's tangent chain, as the Fillet and Chamfer features ask for it.
            let out = blend(&body, &tangent_chain(&body, &[Vec3::new(52.0, 20.0, 0.0)]).unwrap(), 0.8, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // Pappus: the section turned half round the axis at its centroid's radius.
            let want = area * std::f64::consts::PI * (12.0 - c);
            let removed = v0 - vol(&out);
            assert!((removed - want).abs() < want * 2e-3, "{removed} vs {want}");
            // The app's planned mesh must also close, with the same analytic removal.
            let planned = crate::issue_851_tests::planned_volume(&body) - crate::issue_851_tests::planned_volume(&out);
            assert!((planned - want).abs() < want * 2e-3, "display mesh: {planned} vs {want}");
        }
    }

    #[test]
    fn a_blend_ends_on_a_face_running_along_its_seam_twice() {
        // #691: a pocket's floor edge along its straight wall ends on a boss's cylinder, which a
        // STEP file gives as one face running along its seam edge twice.
        let b = crate::build::box_solid(Vec3::new(-55.0, -20.0, -30.0), Vec3::new(55.0, 50.0, 0.0)).unwrap();
        let boss = crate::build::cylinder(Vec3::new(55.0, 15.0, -50.0), Vec3::Z, 25.0, 50.0).unwrap();
        let b = crate::ops::boolean(&b, &boss, crate::BoolOp::Union).unwrap().unwrap();
        let pocket = crate::build::box_solid(Vec3::new(-45.0, -21.0, -15.0), Vec3::new(55.0, 10.0, 1.0)).unwrap();
        let wall = crate::build::cylinder(Vec3::new(55.0, 15.0, -20.0), Vec3::Z, 25.0, 30.0).unwrap();
        let pocket = crate::ops::boolean(&pocket, &wall, crate::BoolOp::Cut).unwrap().unwrap();
        let made = crate::ops::boolean(&b, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let body = crate::step_import(&crate::step_export(&[&made], "test").unwrap()).unwrap().bodies.remove(0).body;
        let rec = crate::native::brep_record(&body).unwrap();
        let seam_twice = rec.faces.iter().flat_map(|f| f.loops.iter()).any(|l| match l {
            Loop::Edges { edges } => edges.iter().enumerate().any(|(k, x)| edges.iter().skip(k + 1).any(|y| y.edge == x.edge)),
            Loop::Vertex { .. } => false,
        });
        assert!(seam_twice);
        let v0 = vol(&body);
        // The edge runs from x = −45 to where the wall meets the cylinder, x = 55 − √(25² − 5²).
        let len = 45.0 + 55.0 - (600f64).sqrt();
        for (round, area) in [(true, 1.0 - std::f64::consts::PI / 4.0), (false, 0.5)] {
            let out = blend(&body, &[Vec3::new(0.0, 10.0, -15.0)], 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // A concave edge: the blend adds material.
            let added = vol(&out) - v0;
            assert!((added - area * len).abs() < area * len * 0.01, "{added} vs {}", area * len);
        }
    }

    #[test]
    fn a_chamfer_meets_sloped_end_planes() {
        for slope in [-0.5, 0.5] {
            let region = Region2 {
                outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0 - slope * 20.0, 20.0), Vec2::new(slope * 20.0, 20.0)]),
                holes: vec![],
            };
            let body = crate::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
            let pick = [Vec3::new(20.0, 0.0, 20.0)];
            let result = chamfer(&body, &pick, 2.0).unwrap();
            let removed = 2.0 * (40.0 - 2.0 * slope * 2.0 / 3.0);
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&result).unwrap().volume;
            assert!((actual - removed).abs() < 1e-6, "{actual} vs {removed}");
            assert!(result.validity().is_empty());
            // The new corner vertices lie on both original end planes.
            for point in [Vec3::new(slope * 2.0, 2.0, 20.0), Vec3::new(40.0 - slope * 2.0, 2.0, 20.0)] {
                assert!(has_vertex(&result, point, 1e-8));
            }
            // Past the faces (20 deep, 20 high), over their far edges, cliffs: cut as if the
            // faces and the sloped end planes went on. The plane 25 back on each face takes, of
            // the section, z >= y - 5 (y, z in [0, 20]), each line of it 40 - 2 slope y long
            // between the end planes: 11500 - 4625 slope.
            let past = chamfer(&body, &pick, 25.0).unwrap();
            let removed = 11500.0 - 4625.0 * slope;
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&past).unwrap().volume;
            assert!((actual - removed).abs() < 1e-6 * removed, "{actual} vs {removed}");
            assert!(past.validity().is_empty());
            // A fillet ends in each sloped end plane along an ellipse: the removed section's
            // length is taken at its centroid, c from the edge.
            let rounded = fillet(&body, &pick, 2.0).unwrap();
            let (area, c) =
                ((1.0 - std::f64::consts::PI / 4.0) * 4.0, 2.0 * (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * (4.0 - std::f64::consts::PI)));
            let removed = area * (40.0 - 2.0 * slope * c);
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&rounded).unwrap().volume;
            assert!((actual - removed).abs() < 1e-3 * removed, "{actual} vs {removed}");
            assert!(rounded.validity().is_empty());
        }
    }

    /// A block 40 wide and 20 deep (z 0…20) whose top rises from y = 20 at its sides to a
    /// crease along z at x = 20, each half sloping `slope` degrees, so the faces turn by twice
    /// that across the crease; and a point on the crease.
    fn roof(slope: f64) -> (Body, Vec3) {
        let h = 20.0 * slope.to_radians().tan();
        let outline = [Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 20.0), Vec2::new(20.0, 20.0 + h), Vec2::new(0.0, 20.0)];
        let region = Region2 { outer: Loop2::polygon(&outline), holes: vec![] };
        let body = crate::build::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().remove(0);
        (body, Vec3::new(20.0, 20.0 + h, 10.0))
    }

    /// A round lies r·(sec(δ/2) − 1) inside the crease it replaces (δ the faces' turn): below the
    /// edge's tolerance it changes nothing and is refused as too small, naming the smallest
    /// radius that rounds. The block's edges are held to 2e-5 mm (SMLib's intersection tolerance
    /// for a part this size, twice its zone tolerance of 1e-5). At 1° an R0.4 round would lie
    /// 0.4 · (sec 0.5° − 1) = 0.015 µm in, under it: the smallest is 2e-5 / (sec 0.5° − 1) =
    /// 0.52523 mm, named rounded up. At 3° the R2 round is 0.685 µm in and builds: one face more,
    /// the top that much lower, removing 20 · r²(tan(δ/2) − δ/2) = 4.786e-4 mm³.
    #[test]
    fn a_round_too_small_to_change_the_part_is_refused_naming_the_smallest_that_rounds() {
        let (gentle, crease) = roof(0.5);
        let rec = Faces::of(&gentle).unwrap().rec;
        assert!(rec.tolerance == 2e-5 && rec.edges.iter().all(|e| e.tolerance == 2e-5), "{}", rec.tolerance);
        let turn = 1f64.to_radians();
        let smallest = 2e-5 / (1.0 / (0.5 * turn).cos() - 1.0);
        assert!((smallest - 0.52523).abs() < 1e-5, "{smallest}");
        let e = fillet(&gentle, &[crease], 0.4).unwrap_err().to_string();
        assert!(
            e.contains("is too small") && e.contains("turn only 1.00°") && e.contains("the smallest radius that rounds this edge is 0.526 mm"),
            "{e}"
        );

        let (steeper, crease) = roof(1.5);
        let (r, turn) = (2.0, 3f64.to_radians());
        let rounded = fillet(&steeper, &[crease], r).unwrap();
        assert!(rounded.validity().is_empty(), "{:?}", rounded.validity());
        assert_eq!(rounded.face_count(), steeper.face_count() + 1);
        let (before, after) = (crate::measure(&steeper).unwrap(), crate::measure(&rounded).unwrap());
        let sag = r * (1.0 / (0.5 * turn).cos() - 1.0);
        assert!(((before.bbox.max.y - after.bbox.max.y) - sag).abs() < 1e-9, "{} vs {sag}", before.bbox.max.y - after.bbox.max.y);
        let removed = 20.0 * r * r * ((0.5 * turn).tan() - 0.5 * turn);
        assert!(((before.volume - after.volume) - removed).abs() < 1e-6, "{} vs {removed}", before.volume - after.volume);
    }

    /// At 1° the crease rounds from 0.526 mm: R2 lies 0.076 µm in and R20 0.76 µm, and each
    /// builds, removing 20 · r²(tan(δ/2) − δ/2).
    /// A box 104 x 64 x 42 hollowed 2 mm from the inside, open at the bottom: 1668's thin wall.
    fn thin_walled_case() -> Body {
        let outer = crate::build::box_solid(Vec3::new(-52.0, -32.0, 0.0), Vec3::new(52.0, 32.0, 42.0)).unwrap();
        let hollow = crate::build::box_solid(Vec3::new(-50.0, -30.0, -1.0), Vec3::new(50.0, 30.0, 40.0)).unwrap();
        crate::ops::boolean(&outer, &hollow, crate::BoolOp::Cut).unwrap().unwrap()
    }

    #[test]
    fn a_round_cutting_through_a_thin_wall_into_the_hollow_is_refused_naming_the_largest_that_fits() {
        let case = thin_walled_case();
        let top = Vec3::new(0.0, 32.0, 42.0);
        // The round's centre is at (32 - r, 42 - r); the hollow's corner (30, 40) leaves its circle
        // where sqrt2 (r - 2) = r: r = 4 + 2 sqrt2 = 6.8284.
        let e = fillet(&case, &[top], 7.0).unwrap_err();
        assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
        let e = e.to_string();
        assert!(e.contains("into the hollow at (0.000, 30.000, 40.000) and open the part"), "{e}");
        let (largest, _) = fillet_max(&case, &[top], 7.0, FilletTurn::Mitre).unwrap().unwrap();
        assert!((largest - 6.828).abs() < 1e-9, "{largest}");

        // Just short of the corner it rounds, its whole section along the 104.
        let r = 6.8;
        let rounded = fillet(&case, &[top], r).unwrap();
        let want = 104.0 * 64.0 * 42.0 - 100.0 * 60.0 * 40.0 - (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 104.0;
        let got = crate::measure(&rounded).unwrap().volume;
        assert!((got - want).abs() < 1e-6 * want, "{got} vs {want}");

        // The inside edge rounded with it fills the hollow's corner: the outer round reaches the
        // inner R7's arc (centre (23, 33)) only at sqrt2 (r - 9) + 7 = r, r = 13.83.
        let inside = Vec3::new(0.0, 30.0, 40.0);
        assert!(fillet(&case, &[top, inside], 7.0).is_ok());
        assert!(fillet_each(&case, &[top, inside], &[13.8, 7.0], FilletTurn::Mitre).is_ok());
        let e = fillet_each(&case, &[top, inside], &[13.9, 7.0], FilletTurn::Mitre).unwrap_err().to_string();
        assert!(e.contains("into the hollow at"), "{e}");
    }

    #[test]
    fn a_one_degree_crease_rounds_from_the_smallest_radius_named() {
        let (gentle, crease) = roof(0.5);
        let turn = 1f64.to_radians();
        for r in [2.0, 20.0] {
            let rounded = fillet(&gentle, &[crease], r).unwrap();
            assert!(rounded.validity().is_empty(), "r={r}: {:?}", rounded.validity());
            let removed = 20.0 * r * r * ((0.5 * turn).tan() - 0.5 * turn);
            let (before, after) = (crate::measure(&gentle).unwrap().volume, crate::measure(&rounded).unwrap().volume);
            assert!(((before - after) - removed).abs() < 1e-6, "r={r}: {} vs {removed}", before - after);
        }
    }

    /// A pin standing in a socket, 3.5 mm from two of its walls, in a large block. The cutters
    /// for its tip reach several times the chamfer's size past each edge (more on a larger part),
    /// into the walls, and their union took bites out of them: 7.2 mm³ instead of the tip's.
    /// A cutter whose far corners are in the material is not used; the tip alone is chamfered.
    #[test]
    fn chamfering_a_pin_in_a_socket_leaves_the_socket_walls() {
        let block = crate::build::box_solid(Vec3::new(0.0, 0.0, -135.0), Vec3::new(82.0, 41.0, 19.0)).unwrap();
        let socket = crate::build::box_solid(Vec3::new(6.0, 11.5, 0.0), Vec3::new(21.0, 35.5, 25.0)).unwrap();
        let pin = crate::build::box_solid(Vec3::new(9.5, 30.5, -1.0), Vec3::new(12.5, 32.0, 10.0)).unwrap();
        let cut = crate::boolean(&block, &socket, crate::BoolOp::Cut).unwrap().unwrap();
        let body = crate::boolean(&cut, &pin, crate::BoolOp::Union).unwrap().unwrap();
        let tip = [Vec3::new(11.0, 30.5, 10.0), Vec3::new(9.5, 31.25, 10.0), Vec3::new(11.0, 32.0, 10.0), Vec3::new(12.5, 31.25, 10.0)];
        let d = 0.5;
        let out = chamfer(&body, &tip, d).unwrap();
        // Four wedges along the tip's edges, less the four corner pyramids each pair shares.
        let removed = 0.5 * d * d * 2.0 * (3.0 + 1.5) - 4.0 * d * d * d / 3.0;
        let actual = crate::measure(&body).unwrap().volume - crate::measure(&out).unwrap().volume;
        assert!((actual - removed).abs() < 1e-3, "{actual} vs {removed}");
        assert_eq!(out.face_count(), body.face_count() + 4);
        assert!(out.validity().is_empty(), "{:?}", out.validity());
    }
}

#[cfg(test)]
#[path = "curvecorner_tests.rs"]
mod corner_tests;
