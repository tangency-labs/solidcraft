//! Filling a closed loop of a body's edges with one face, bounded by the body's own edges,
//! whole: the loop's plane when its edges certainly lie in one, otherwise a B-spline height field
//! over the loop's best-fit plane (`solidcraft_geom::fill::fill_loop`). The face is assembled on
//! the body's own edge records (`sew::assemble`) and built by SMLib. The height field's
//! parameters are plane coordinates, so each edge's pcurve is its exact projection onto the
//! plane.
//!
//! The general fill, which also meets the faces beside the loop with G1 … G4, passes through
//! given points, and fills loops that bound no height field, is a patch whose boundary rows are
//! the loop's own edges (`solidcraft_geom::fill_iso`): each edge's pcurve is a straight line in
//! (u, v), affine in the edge's own parameter. It takes loops of three to sixteen line, spline or
//! rational edges (a circle's arc on the NURBS form SMLib holds it as); the frames across each
//! edge are read off the neighbouring face's record surface, with its exact derivatives to the
//! order asked: the neighbour as a height over its tangent plane to that order
//! (`solidcraft_geom::linkage`).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::brep::{self as b, Curve2, eval::CurveEval};
use solidcraft_geom::bspline::{HPoint, SplineError};
use solidcraft_geom::fill_general::{Across, MAX_FILL_POINTS, Station};
use solidcraft_geom::fill_iso::{IsoEdge, IsoFill, IsoSpec};
use solidcraft_geom::fill_matched::{MatchedFill, MatchedSpec};

use crate::body::Body;
use crate::bridge::Continuity;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceProv, History};
use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
use crate::{KernelError, Result};

/// How far (degrees) a G1 … G4 patch's normal may turn from its neighbour's along the loop.
pub const PATCH_ANGLE_TOL_DEG: f64 = 0.05;

/// A spline error as the patch reports it.
fn spline_error(e: SplineError) -> KernelError {
    match e {
        SplineError::Unsupported(m) => KernelError::Invalid(format!("not supported yet: {m}")),
        other => KernelError::Invalid(format!("cannot fill the loop: {other}")),
    }
}

/// What a patch on a loop of edges must do besides filling it.
#[derive(Clone, Debug, PartialEq)]
pub struct PatchSpec {
    /// How every edge's neighbour is met (G0 … G4).
    pub continuity: Continuity,
    /// Edges met otherwise: a point on the edge and its continuity.
    pub edges: Vec<(Vec3, Continuity)>,
    /// Points the patch passes through.
    pub points: Vec<Vec3>,
    /// How full the patch's middle is, `crown` times the loop's size (−0.9…3): the bump of
    /// [`patch_edges_crowned`], laid along the general fill's own normal there (to the side its
    /// G1/G2 neighbours' normals face) on control points clear of the loop, so the loop and the
    /// conditions across it do not move. A general fill needs a G1 or G2 edge to crown.
    pub crown: f64,
}

impl Default for PatchSpec {
    fn default() -> PatchSpec {
        PatchSpec { continuity: Continuity::G0, edges: Vec::new(), points: Vec::new(), crown: 0.0 }
    }
}

impl PatchSpec {
    /// Whether this is the plain patch of [`patch_edges_crowned`]: G0 everywhere, no points.
    pub fn is_plain(&self) -> bool {
        self.continuity == Continuity::G0 && self.edges.iter().all(|(_, g)| *g == Continuity::G0) && self.points.is_empty()
    }
}

/// What a general fill achieved (all zero for a planar or height-field patch, which meet their
/// edges within [`face_tolerance`] and ask nothing else).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PatchReport {
    /// Whether the general fill made the face.
    pub general: bool,
    /// The farthest loop station from the surface.
    pub gap: f64,
    /// The largest angle (degrees) between the patch's normal and a G1/G2 neighbour's, away from
    /// corners where the neighbours themselves disagree, and anywhere.
    pub normal_deviation_deg: f64,
    pub normal_deviation_all_deg: f64,
    /// The largest angle (degrees) by which the patch's normal misses a neighbour's beyond the
    /// loop edge's own lean out of that neighbour's tangent plane (what the tolerance holds).
    pub normal_excess_deg: f64,
    /// `jumps[k]`, k = 2 … 4: the largest jump across the edges asked for order k or more, clear
    /// of the loop's corners, of the k-th derivative of the linkage curves' height above the
    /// neighbour (1/mmᵏ⁻¹): the normal curvature (k = 2), its rate of change
    /// (3) and that rate's (4) (`solidcraft_geom::fill_iso::Achieved::jumps`).
    pub jumps: [f64; solidcraft_geom::linkage::MAX_CONTINUITY + 1],
    /// The farthest point from the surface.
    pub point_gap: f64,
}

/// [`crate::patch_edges`] meeting the faces beside the loop with the continuity `spec` asks for
/// and passing through its points; a plain spec is exactly [`patch_edges_crowned`]. Its history
/// is over `[body]`: the face is new, its edges the body's own.
pub fn patch_edges_with(body: &Body, at: &[Vec3], spec: &PatchSpec) -> Result<Body> {
    patch_edges_report(body, at, spec).map(|(b, _)| b)
}

/// [`patch_edges_with`] and what the fill achieved.
pub fn patch_edges_report(body: &Body, at: &[Vec3], spec: &PatchSpec) -> Result<(Body, PatchReport)> {
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    // A range test also throws out NaN.
    if !(solidcraft_geom::fill::MIN_CROWN..=solidcraft_geom::fill::MAX_CROWN).contains(&spec.crown) {
        return Err(KernelError::Invalid("a patch's crown must be between -0.9 and 3".into()));
    }
    if spec.edges.len() > 10_000 || spec.edges.iter().any(|(p, _)| !p.is_finite()) {
        return Err(KernelError::Invalid("pick at most 10000 edges, each by a finite point, for their continuity".into()));
    }
    if spec.points.len() > MAX_FILL_POINTS || spec.points.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid(format!("a patch passes through at most {MAX_FILL_POINTS} finite points")));
    }
    if spec.is_plain() {
        return patch_edges_crowned(body, at, spec.crown).map(|b| (b, PatchReport::default()));
    }
    body.require_brep("patch")?;
    let record = crate::native::brep_record(body)?;
    let chain = picked_loop(&record, at, body.size() * 0.05 + 1e-6)?;
    let mut orders = vec![spec.continuity; chain.len()];
    for (p, g) in &spec.edges {
        let e = nearest_edge(&record, *p, body.size() * 0.05 + 1e-6).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        let k = chain.iter().position(|c| c.0 == e).ok_or_else(|| KernelError::Invalid("a continuity pick is not on an edge of the loop".into()))?;
        if let Some(slot) = orders.get_mut(k) {
            *slot = *g;
        }
    }
    if spec.crown != 0.0 && orders.iter().all(|g| *g == Continuity::G0) {
        return Err(KernelError::Invalid("not supported yet: a crown on a patch through points with no g1 or higher edge".into()));
    }
    general_fill(body, &record, &chain, &orders, &spec.points, spec.crown)
}

/// The general fill of the loop `chain` of the record's edges, edge `k` meeting the face beside it
/// with `orders[k]` (module docs).
fn general_fill(
    body: &Body,
    record: &b::Brep,
    chain: &[(usize, bool)],
    orders: &[Continuity],
    points: &[Vec3],
    crown: f64,
) -> Result<(Body, PatchReport)> {
    if !(1..=solidcraft_geom::fill_iso::MAX_ISO_EDGES).contains(&chain.len()) {
        return Err(KernelError::Invalid(format!(
            "not supported yet: a general patch on a loop of {} edges (one to {})",
            chain.len(),
            solidcraft_geom::fill_iso::MAX_ISO_EDGES
        )));
    }
    let pieces = loop_pieces(record, chain)?;
    let edges: Vec<IsoEdge> = pieces.iter().map(|p| iso_edge(record, p)).collect::<Result<_>>()?;
    let size = body.size().max(solidcraft_geom::LINEAR_TOL);
    let mut beside: Vec<Option<Beside>> = Vec::with_capacity(pieces.len());
    for p in &pieces {
        let order = chain.iter().position(|&(e, _)| e == p.edge).and_then(|k| orders.get(k)).copied().unwrap_or(Continuity::G0);
        beside.push(if order == Continuity::G0 { None } else { Some(Beside::new(record, p.edge, p.forward, order, size)?) });
    }
    let mut across = |k: usize, ts: &[f64]| -> std::result::Result<Vec<Option<Across>>, String> {
        match beside.get(k) {
            None => Err("no such edge".into()),
            Some(None) => Ok(vec![None; ts.len()]),
            Some(Some(side)) => ts.iter().map(|t| side.across(record, *t).map_err(|e| e.to_string())).collect(),
        }
    };
    let spec =
        IsoSpec { tol: face_tolerance(body), angle_tol: PATCH_ANGLE_TOL_DEG.to_radians(), crown, corner_turn: CORNER_TURN_DEG.to_radians(), points };
    let fill = solidcraft_geom::fill_iso::iso_fill(&edges, &mut across, &spec).map_err(spline_error)?;
    let made = quilt_body(body, record, &pieces, &fill, face_tolerance(body))?;
    let a = fill.achieved;
    let report = PatchReport {
        general: true,
        gap: 0.0,
        normal_deviation_deg: a.normal_deviation.to_degrees(),
        normal_deviation_all_deg: a.normal_deviation_all.to_degrees(),
        normal_excess_deg: a.normal_excess.to_degrees(),
        jumps: a.jumps,
        point_gap: a.point_deviation,
    };
    Ok((made, report))
}

/// How far (degrees) the loop may turn at a vertex for it not to be a corner of the loop.
const CORNER_TURN_DEG: f64 = 1.0;

/// The one-face sheet on the loop `chain` of `body`'s own edges (each a free edge of the face
/// beside it), a height field over the loop's plane that meets every face beside the loop with
/// G2 (`solidcraft_geom::fill_matched`), within [`face_tolerance`] of the loop, its normal within
/// [`PATCH_ANGLE_TOL_DEG`] and its normal curvature across the loop within `curvature_tol` of
/// each face's, clear of the loop's corners (a vertex where the loop turns, where two faces
/// meet). Each edge's pcurve is its exact projection onto the plane. Its history is over
/// `[body]`: the face is new, each edge the body's own.
pub(crate) fn matched_patch(body: &Body, chain: &[(usize, bool)], curvature_tol: f64) -> Result<(Body, MatchedFill)> {
    body.require_brep("patch")?;
    if chain.len() < 2 {
        return Err(KernelError::Invalid("a patch needs a loop of two or more edges".into()));
    }
    let record = crate::native::brep_record(body)?;
    let size = body.size().max(solidcraft_geom::LINEAR_TOL);
    let mut sides = Vec::with_capacity(chain.len());
    let mut beside = Vec::with_capacity(chain.len());
    for &(e, forward) in chain {
        sides.push(Side::new(&record, e, forward)?);
        beside.push(Beside::new(&record, e, forward, Continuity::G2, size)?);
    }
    // A vertex of the loop is a corner where the loop turns there: the edge arriving and the one
    // leaving go different ways.
    let n = chain.len();
    let mut corner = vec![false; n];
    for (k, slot) in corner.iter_mut().enumerate() {
        let prev = (k + n - 1) % n;
        let (Some(a), Some(b)) = (sides.get(prev), sides.get(k)) else { continue };
        let (Some(arrive), Some(leave)) = (a.direction(1.0), b.direction(0.0)) else { continue };
        *slot = arrive.dot(leave) < CORNER_TURN_DEG.to_radians().cos();
    }
    let mut stations = |k: usize, ts: &[f64]| -> std::result::Result<Vec<Station>, String> {
        let (Some(side), Some(face)) = (sides.get_mut(k), beside.get(k)) else { return Err("no such edge".into()) };
        let edge = record.edges.get(face.edge).ok_or("edge reference")?;
        let [a, z] = edge.interval;
        ts.iter()
            .map(|&t| {
                let p = side.point(t).ok_or("an edge has no point there")?;
                let across = face.across(&record, a + (z - a) * t).map_err(|e| e.to_string())?.ok_or("an edge has no tangent there")?;
                let at_corner = t == 0.0 && corner.get(k).copied().unwrap_or(false);
                Ok(Station { p, up: across.normal, across: Some(across), corner: at_corner, clash: false })
            })
            .collect()
    };
    let spec = MatchedSpec { tol: face_tolerance(body), angle_tol: PATCH_ANGLE_TOL_DEG.to_radians(), curvature_tol };
    let fill = solidcraft_geom::fill_matched::fill_matched(n, &mut stations, &spec).map_err(spline_error)?;
    let projection = Projection { origin: fill.fill.origin, axes: fill.fill.axes };
    let surface = crate::surfaces::spline_surface(&fill.fill.surface);
    let made = patch_body(body, &record, chain, surface, fill.fill.counter_clockwise, &Pcurves::Projected(&projection), face_tolerance(body))?;
    Ok((made, fill))
}

/// A piece of a loop edge as the general fill takes it: record edge `edge` over `interval` (in
/// its own parameter, min < max), run the loop's way along it (`forward`) or back.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    edge: usize,
    forward: bool,
    interval: [f64; 2],
}

/// The loop `chain` as the general fill's sides: each edge whole, but for a loop of one or two
/// edges (a tube's end: one closed circle) each edge in equal pieces of its parameter, four sides
/// in all, so the fill is one four-sided patch on pieces of the edges' own curves.
fn loop_pieces(record: &b::Brep, chain: &[(usize, bool)]) -> Result<Vec<Piece>> {
    let split = match chain.len() {
        1 => 4,
        2 => 2,
        _ => 1,
    };
    let mut out = Vec::with_capacity(chain.len() * split);
    for &(e, forward) in chain {
        let edge = record.edges.get(e).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
        let [a, z] = edge.interval;
        // A break that falls on one of the curve's own knots (a conic's quarter points) is that
        // knot exactly: a piece must not start a rounding error off one.
        let own: Vec<f64> = match edge.parameters() {
            b::Curve::Spline { knots, .. } | b::Curve::RationalSpline { knots, .. } => knots.clone(),
            _ => Vec::new(),
        };
        let at = |i: usize| {
            if i == 0 {
                a
            } else if i == split {
                z
            } else {
                let t = a + (z - a) * i as f64 / split as f64;
                own.iter().copied().find(|k| (k - t).abs() <= 1e-9 * (z - a).abs()).unwrap_or(t)
            }
        };
        let mut pieces: Vec<Piece> = (0..split).map(|i| Piece { edge: e, forward, interval: [at(i), at(i + 1)] }).collect();
        if !forward {
            pieces.reverse();
        }
        out.extend(pieces);
    }
    Ok(out)
}

/// The general fill's parameter `t` along a piece `[plo, phi]` of a record edge on `[a, z]`, run
/// the loop's way (`forward`) or back, in the record edge's own parameter: the edge's, reflected
/// where the loop runs against it, a piece's end or middle exactly.
fn on_record(a: f64, z: f64, [plo, phi]: [f64; 2], forward: bool, t: f64) -> f64 {
    let near = 1e-9 * (1.0 + (z - a).abs() + a.abs().max(z.abs()));
    let t = if forward { t } else { reflected(a, z, t) };
    [plo, phi, 0.5 * (plo + phi)].into_iter().find(|s| (s - t).abs() <= near).unwrap_or(t)
}

/// `t` on an edge's interval `[a, z]` reflected about its middle: the loop's parameter along an
/// edge it runs against.
fn reflected(a: f64, z: f64, t: f64) -> f64 {
    if t == a {
        z
    } else if t == z {
        a
    } else {
        a + z - t
    }
}

/// A loop edge as the patch takes it: a B-spline on the edge's interval, rational when the edge
/// is (a circle or ellipse on the NURBS form SMLib holds it as, whose parameter the edge's interval
/// and pcurves are in), running the loop's way, its ends on the edge's vertices. A traced curve
/// has no such form in its own parameter and is refused by name.
fn iso_edge(record: &b::Brep, piece: &Piece) -> Result<IsoEdge> {
    let Piece { edge: e, forward, interval: [lo, hi] } = *piece;
    let edge = record.edges.get(e).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
    let v3 = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let [a, z] = edge.interval;
    let (degree, knots, h): (usize, Vec<f64>, Vec<HPoint>) = match edge.parameters() {
        b::Curve::Line { start, end } => {
            let (s, t) = (v3(start), v3(end));
            (1, vec![a, a, z, z], vec![HPoint::weighted(s + (t - s) * a, 1.0), HPoint::weighted(s + (t - s) * z, 1.0)])
        }
        b::Curve::Polyline { points } => {
            let mut knots = vec![0.0];
            knots.extend((0..points.len()).map(|i| i as f64));
            knots.push(points.len().saturating_sub(1) as f64);
            (1, knots, points.iter().map(|p| HPoint::weighted(v3(p), 1.0)).collect())
        }
        b::Curve::Spline { knots, points } => {
            (knots.len().saturating_sub(points.len() + 1), knots.clone(), points.iter().map(|p| HPoint::weighted(v3(p), 1.0)).collect())
        }
        b::Curve::RationalSpline { knots, homogeneous } => (
            knots.len().saturating_sub(homogeneous.len() + 1),
            knots.clone(),
            homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect(),
        ),
        _ => {
            return Err(KernelError::Invalid(format!(
                "not supported yet: a general patch on edge {e}, a circle or ellipse with no NURBS form, or a traced curve (its pcurve on the patch has no exact form in its own parameter)"
            )));
        }
    };
    // The piece of the edge, then run the loop's way (the edge's parameter reflected about its
    // interval's middle).
    let (mut knots, mut h) = solidcraft_geom::bspline::segment(degree, &knots, &h, lo, hi).map_err(spline_error)?;
    if !forward {
        knots = knots.iter().rev().map(|k| a + z - k).collect();
        h.reverse();
        // The ends exactly (the reflection's rounding), so a piece's ends name them.
        let (rlo, rhi) = (reflected(a, z, hi), reflected(a, z, lo));
        let n = knots.len();
        for (i, k) in knots.iter_mut().enumerate() {
            if i <= degree {
                *k = rlo;
            } else if i + degree + 1 >= n {
                *k = rhi;
            }
        }
    }
    let rational = h.iter().any(|q| q.w != 1.0);
    let mut points = h
        .iter()
        .map(|q| q.project().filter(|_| q.w > 0.0))
        .collect::<Option<Vec<Vec3>>>()
        .ok_or_else(|| KernelError::Invalid(format!("not supported yet: a general patch on edge {e}, whose weights are not all positive")))?;
    // Its ends exactly on the loop's vertices, where they are the edge's ends.
    let vertex = |at: f64| match (at == a, at == z) {
        (true, _) => record.vertices.get(edge.vertices[0] as usize).map(|v| v3(&v.point)),
        (_, true) => record.vertices.get(edge.vertices[1] as usize).map(|v| v3(&v.point)),
        _ => None,
    };
    let (first, last) = if forward { (vertex(lo), vertex(hi)) } else { (vertex(hi), vertex(lo)) };
    if let (Some(p), Some(f)) = (first, points.first_mut()) {
        *f = p;
    }
    if let (Some(q), Some(l)) = (last, points.last_mut()) {
        *l = q;
    }
    let weights = rational.then(|| h.iter().map(|q| q.w).collect());
    Ok(IsoEdge { degree, knots, points, weights })
}

/// The face beside a G1 … G4 edge, read at points of the edge: its record surface, which side of
/// it is outward, whether its boundary runs the edge the loop's way, and how much of it is read
/// (`order`: its tangent plane for G1, and its jet to that order above).
struct Beside {
    surface: SurfaceEval,
    side: f64,
    along: bool,
    edge: usize,
    forward: bool,
    order: Continuity,
    reach: f64,
}

impl Beside {
    fn new(record: &b::Brep, e: usize, forward: bool, order: Continuity, size: f64) -> Result<Beside> {
        let edge = record.edges.get(e).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
        let [u] = edge.uses.as_slice() else {
            return Err(KernelError::Invalid("an edge of the loop lies between two faces: g1 and up need the one face beside it".into()));
        };
        let face = record.faces.get(u.face as usize).ok_or_else(|| KernelError::Failed("patch: face reference".into()))?;
        let surface = SurfaceEval::new(face.parameters())
            .map_err(|err| KernelError::Invalid(format!("the face beside edge {e} cannot be evaluated: {err}")))?;
        let outward = record
            .shells
            .iter()
            .find_map(|s| match s {
                b::Shell::Faces { faces, .. } => faces.iter().find(|f| f.face == u.face).map(|f| f.forward),
                _ => None,
            })
            .unwrap_or(true);
        let used_forward = match face.loops.get(u.r#loop as usize) {
            Some(b::Loop::Edges { edges }) => edges.get(u.position as usize).map(|x| x.forward),
            _ => None,
        }
        .ok_or_else(|| KernelError::Failed("patch: edge use".into()))?;
        // A face's boundary runs counter-clockwise about its outward normal: the record's loops
        // run so about the natural normal, reversed where the outward side is the other.
        let runs_forward = used_forward == outward;
        Ok(Beside {
            surface,
            side: if outward { 1.0 } else { -1.0 },
            along: runs_forward == forward,
            edge: e,
            forward,
            order,
            reach: (size * 1e-4).max(1e-6),
        })
    }

    /// The neighbour's conditions at parameter `t` of the loop's edge (its `IsoEdge`, run the
    /// loop's way, whose parameter is the edge's reversed where the loop runs against it):
    /// its normal, the direction it leaves the edge in, away from it, and for G2 and up its
    /// curvature vector along that direction and its jet to the order asked
    /// (`solidcraft_geom::linkage::surface_jet`, on the frame of the boundary and the leave
    /// direction). `None` where the edge has no tangent there.
    fn across(&self, record: &b::Brep, t: f64) -> Result<Option<Across>> {
        let edge = record.edges.get(self.edge).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
        let [a, z] = edge.interval;
        let tau = if self.forward { t } else { a + z - t };
        let curve = CurveEval::new(edge.parameters()).map_err(|e| KernelError::Invalid(e.to_string()))?;
        let Some((p, d)) = curve.derivs(tau) else { return Ok(None) };
        let Some(tangent) = (d * if self.forward { 1.0 } else { -1.0 }).normalized() else { return Ok(None) };
        let uv =
            self.surface.project(p, None, self.reach).ok_or_else(|| KernelError::Invalid("a picked edge is not on the face beside it".into()))?;
        let (_, su, sv) =
            self.surface.derivs(uv[0], uv[1]).ok_or_else(|| KernelError::Failed("patch: the face beside an edge has no derivatives".into()))?;
        let normal =
            (su.cross(sv) * self.side).normalized().ok_or_else(|| KernelError::Failed("patch: the face beside an edge has no normal".into()))?;
        // Away from the face: its boundary's direction × its outward normal.
        let boundary = if self.along { tangent } else { tangent * -1.0 };
        let leave = boundary.cross(normal).normalized().ok_or_else(|| KernelError::Invalid("the boundary runs along the face's normal".into()))?;
        let order = continuity_order(self.order);
        if order < 2 {
            return Ok(Some(Across { leave, normal, bend: None, jet: None }));
        }
        let bend = Some(normal * normal_curvature(&self.surface, uv, normal, leave)?);
        let jet = solidcraft_geom::linkage::surface_jet(&self.surface, uv, normal, leave, order).ok_or_else(|| {
            KernelError::Invalid(format!("not supported yet: g{order} to a face whose surface has no derivatives of order {order} there"))
        })?;
        Ok(Some(Across { leave, normal, bend, jet: Some(jet) }))
    }
}

/// The continuity order a [`Continuity`] asks: 0 (G0) … n, the one place it is read as a number.
fn continuity_order(g: Continuity) -> usize {
    g.conditions().saturating_sub(1)
}

/// The normal curvature of a surface at `uv` along the unit tangent `w`: `n·r''` of the unit-speed
/// curve on it leaving that way (the second fundamental form, exact second derivatives).
fn normal_curvature(s: &SurfaceEval, uv: [f64; 2], n: Vec3, w: Vec3) -> Result<f64> {
    let fail = || KernelError::Failed("patch: the face beside an edge has no curvature there".into());
    let (_, ru, rv) = s.derivs(uv[0], uv[1]).ok_or_else(fail)?;
    let (ruu, ruv, rvv) = s.second(uv[0], uv[1]).ok_or_else(fail)?;
    // w = ru·α + rv·β, solved in the tangent plane.
    let (e, f, g) = (ru.dot(ru), ru.dot(rv), rv.dot(rv));
    let det = e * g - f * f;
    if !(det > 0.0) {
        return Err(fail());
    }
    let (wu, wv) = (w.dot(ru), w.dot(rv));
    let (alpha, beta) = ((g * wu - f * wv) / det, (e * wv - f * wu) / det);
    let k = n.dot(ruu * (alpha * alpha) + ruv * (2.0 * alpha * beta) + rvv * (beta * beta));
    k.is_finite().then_some(k).ok_or_else(fail)
}

/// [`crate::patch_edges`] with a crown on the fitted face: positive makes the patch's middle
/// fuller, negative flatter, by `crown` times the loop's size (its projected box's diagonal), the
/// bump vanishing along the loop, so the face still meets the body's own edges. `crown` 0 is
/// exactly [`crate::patch_edges`], and a loop that is certainly planar makes the same exact planar
/// face whatever the crown. Its history is over `[body]`: the face is new, its edges the body's
/// own.
pub fn patch_edges_crowned(body: &Body, at: &[Vec3], crown: f64) -> Result<Body> {
    body.require_brep("patch")?;
    if at.is_empty() || at.len() > 10_000 {
        return Err(KernelError::Invalid("pick the edges of a closed loop".into()));
    }
    // A range test also throws out NaN.
    if !(solidcraft_geom::fill::MIN_CROWN..=solidcraft_geom::fill::MAX_CROWN).contains(&crown) {
        return Err(KernelError::Invalid("a patch's crown must be between -0.9 and 3".into()));
    }
    patch_loop(body, at, crown)
}

/// The patch on the closed loop of `body`'s edges picked by `at`: the loop's plane when it is
/// certainly planar (within [`planarity_tolerance`]), else the height field crowned by `crown`.
/// What `crate::patch_edges` makes with `crown` 0.
pub(crate) fn patch_loop(body: &Body, at: &[Vec3], crown: f64) -> Result<Body> {
    let record = crate::native::brep_record(body)?;
    let reach = body.size() * 0.05 + 1e-6;
    let chain = picked_loop(&record, at, reach)?;
    let mut sides = Vec::with_capacity(chain.len());
    for &(e, forward) in &chain {
        sides.push(Side::new(&record, e, forward)?);
    }
    let (surface, ccw, pcurve): (b::Surface, bool, Option<Projection>) = match planar(&record, &chain, &mut sides, planarity_tolerance(body))? {
        Some(plane) => (plane, true, None),
        None => {
            let tol = face_tolerance(body);
            let mut point = |k: usize, t: f64| sides.get_mut(k).and_then(|s| s.point(t));
            let fill = match solidcraft_geom::fill::fill_loop(chain.len(), &mut point, tol, crown) {
                Ok(fill) => fill,
                // Not a height field over its plane: the general fill, when no crown asks for the
                // height field's bump.
                Err(SplineError::Unsupported(_)) if crown == 0.0 => {
                    let orders = vec![Continuity::G0; chain.len()];
                    return general_fill(body, &record, &chain, &orders, &[], 0.0).map(|(b, _)| b);
                }
                Err(e) => return Err(spline_error(e)),
            };
            let projection = Projection { origin: fill.origin, axes: fill.axes };
            (crate::surfaces::spline_surface(&fill.surface), fill.counter_clockwise, Some(projection))
        }
    };
    let pcurves = match &pcurve {
        Some(p) => Pcurves::Projected(p),
        None => Pcurves::Computed,
    };
    patch_body(body, &record, &chain, surface, ccw, &pcurves, face_tolerance(body))
}

/// The closed loop of `record`'s edges picked by points on them (the edge nearest each, within
/// `reach`), each with whether the loop runs along it: it starts with the last edge picked, run
/// its own way. Edges that branch, or do not close one loop, are refused.
fn picked_loop(record: &b::Brep, at: &[Vec3], reach: f64) -> Result<Vec<(usize, bool)>> {
    let mut picked: Vec<usize> = Vec::new();
    for p in at {
        if !p.is_finite() {
            return Err(KernelError::Invalid(format!("no edge at {p:?}")));
        }
        let e = nearest_edge(record, *p, reach).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        if !picked.contains(&e) {
            picked.push(e);
        }
    }
    let ends = |e: usize| record.edges.get(e).map(|x| x.vertices);
    let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for &e in &picked {
        for v in ends(e).into_iter().flatten() {
            *uses.entry(v).or_default() += 1;
        }
    }
    if uses.values().any(|n| *n > 2) {
        return Err(KernelError::Invalid("the edges branch: pick one chain".into()));
    }
    let open = || KernelError::Invalid("the edges do not form a closed loop".into());
    if uses.values().any(|n| *n != 2) {
        return Err(open());
    }
    let first = picked.pop().ok_or_else(open)?;
    let [start, mut at_v] = ends(first).ok_or_else(open)?;
    let mut chain = vec![(first, true)];
    while at_v != start {
        let k = picked.iter().position(|e| ends(*e).is_some_and(|[a, z]| a == at_v || z == at_v)).ok_or_else(open)?;
        let e = picked.remove(k);
        let [a, z] = ends(e).ok_or_else(open)?;
        let forward = a == at_v;
        at_v = if forward { z } else { a };
        chain.push((e, forward));
    }
    if !picked.is_empty() {
        return Err(open());
    }
    Ok(chain)
}

/// The record edge whose curve comes nearest `p` (by samples along each), within `reach`.
fn nearest_edge(record: &b::Brep, p: Vec3, reach: f64) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (i, e) in record.edges.iter().enumerate() {
        let Ok(c) = CurveEval::new(e.parameters()) else { continue };
        let [t0, t1] = e.interval;
        for k in 0..=32 {
            let Some(q) = c.point(t0 + (t1 - t0) * k as f64 / 32.0) else { continue };
            let d = q.dist(p);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
    }
    best.filter(|(d, _)| *d <= reach).map(|(_, e)| e)
}

/// One edge of the loop as the fill samples it: `t` from 0 at the loop's start of the edge to 1 at
/// its end, the start being the edge's own vertex (an imported edge's curve may end a little off
/// it).
struct Side {
    curve: CurveEval,
    interval: [f64; 2],
    forward: bool,
    start: Vec3,
}

impl Side {
    fn new(record: &b::Brep, e: usize, forward: bool) -> Result<Side> {
        let edge = record.edges.get(e).ok_or_else(|| KernelError::Failed("patch: edge reference".into()))?;
        let curve = CurveEval::new(edge.parameters()).map_err(|err| KernelError::Invalid(format!("edge {e} cannot be sampled: {err}")))?;
        let v = edge.vertices[if forward { 0 } else { 1 }];
        let start = record.vertices.get(v as usize).map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]));
        let start = start.ok_or_else(|| KernelError::Failed("patch: vertex reference".into()))?;
        Ok(Side { curve, interval: edge.interval, forward, start })
    }

    fn point(&mut self, t: f64) -> Option<Vec3> {
        if t <= 0.0 {
            return Some(self.start);
        }
        let [a, z] = self.interval;
        let s = t.min(1.0);
        self.curve.point(if self.forward { a + (z - a) * s } else { z - (z - a) * s })
    }

    /// The unit direction the loop runs the edge in at `t`.
    fn direction(&self, t: f64) -> Option<Vec3> {
        let [a, z] = self.interval;
        let s = t.clamp(0.0, 1.0);
        let (_, d) = self.curve.derivs(if self.forward { a + (z - a) * s } else { z - (z - a) * s })?;
        (if self.forward { d } else { d * -1.0 }).normalized()
    }
}

/// The plane of the loop when its edges certainly lie in one within `tol`, `None` otherwise. The
/// plane is fitted to points along the edges (through their mean, its normal the loop's area
/// vector, so the loop runs counter-clockwise about it); each edge is then certified
/// conservatively: a line by its ends, a polyline or spline by its control points (the curve lies
/// in their convex hull, weights positive), a circle or ellipse by its centre and how far its
/// axes tilt out of the plane, a traced intersection only when one of its two surfaces is a plane
/// that the loop's points lie on. Samples alone are never trusted: a spline can bulge out of the
/// plane between them.
fn planar(record: &b::Brep, chain: &[(usize, bool)], sides: &mut [Side], tol: f64) -> Result<Option<b::Surface>> {
    const SAMPLES: usize = 16;
    let mut pts = Vec::with_capacity(sides.len() * SAMPLES);
    for side in sides.iter_mut() {
        for k in 0..SAMPLES {
            let Some(p) = side.point(k as f64 / SAMPLES as f64) else { return Ok(None) };
            pts.push(p);
        }
    }
    let mean = pts.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / pts.len().max(1) as f64);
    let area = pts.iter().zip(pts.iter().cycle().skip(1)).fold(Vec3::ZERO, |n, (p, q)| n + (*p - mean).cross(*q - mean));
    let Some(n) = area.normalized() else { return Ok(None) };
    let off = |p: Vec3| (p - mean).dot(n).abs();
    let on = |p: [f64; 3]| off(Vec3::new(p[0], p[1], p[2])) <= tol;
    if !pts.iter().all(|p| off(*p) <= tol) {
        return Ok(None);
    }
    let holds = |s: &b::Surface| -> bool {
        let (o, normal) = match s {
            b::Surface::Plane { origin, u_axis, v_axis } => {
                let (u, v) = (Vec3::new(u_axis[0], u_axis[1], u_axis[2]), Vec3::new(v_axis[0], v_axis[1], v_axis[2]));
                (Vec3::new(origin[0], origin[1], origin[2]), u.cross(v).normalized())
            }
            _ => return false,
        };
        let Some(q) = normal else { return false };
        pts.iter().all(|p| (*p - o).dot(q).abs() <= tol && (*p - n * (*p - mean).dot(n) - o).dot(q).abs() <= tol)
    };
    for &(e, _) in chain {
        let Some(edge) = record.edges.get(e) else { return Ok(None) };
        let certified = match &edge.curve {
            b::Curve::Line { start, end } => on(*start) && on(*end),
            b::Curve::Polyline { points } | b::Curve::Spline { points, .. } => points.iter().all(|p| on(*p)),
            b::Curve::RationalSpline { homogeneous, .. } => homogeneous.iter().all(|h| h[3] > 0.0 && on([h[0] / h[3], h[1] / h[3], h[2] / h[3]])),
            b::Curve::Circle { frame, radius } => conic_on(frame, [*radius; 2], &on, n, tol),
            b::Curve::Ellipse { frame, radii } => conic_on(frame, *radii, &on, n, tol),
            b::Curve::SurfaceIntersection { surfaces, .. } => surfaces.iter().any(|s| holds(s)),
        };
        if !certified {
            return Ok(None);
        }
    }
    let t = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let Some(u) = (t - n * t.dot(n)).normalized() else { return Ok(None) };
    let arr = |p: Vec3| [p.x, p.y, p.z];
    Ok(Some(b::Surface::Plane { origin: arr(mean), u_axis: arr(u), v_axis: arr(n.cross(u)) }))
}

/// A conic lies within `tol` of the plane (normal `n`) when its centre does and its axes, scaled by
/// its radii, tilt out of the plane by no more than that together.
fn conic_on(frame: &b::Frame, radii: [f64; 2], on: &dyn Fn([f64; 3]) -> bool, n: Vec3, tol: f64) -> bool {
    let axis = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]).dot(n).abs();
    on(frame.origin) && radii[0] * axis(frame.x_axis) + radii[1] * axis(frame.y_axis) <= tol
}

/// Where a patch's pcurves come from: the record's routine (a plane), or the exact projection
/// onto a height field's plane.
enum Pcurves<'a> {
    Computed,
    Projected(&'a Projection),
}

/// The plane a height field stands on: its parameters are coordinates along `axes` from `origin`.
struct Projection {
    origin: Vec3,
    axes: [Vec3; 2],
}

impl Projection {
    /// The edge's exact pcurve on the height field: its curve projected onto the plane, which is
    /// affine, so a line or spline stays one with its knots and weights. `None` for a conic or a
    /// traced intersection, whose angle or trace parameter no B-spline in (u, v) reproduces.
    fn pcurve(&self, curve: &b::Curve, [a, z]: [f64; 2]) -> Option<Curve2> {
        let uv = |p: Vec3| [(p - self.origin).dot(self.axes[0]), (p - self.origin).dot(self.axes[1])];
        let v = |p: &[f64; 3]| Vec3::new(p[0], p[1], p[2]);
        let (degree, knots, ctrl): (usize, Vec<f64>, Vec<HPoint>) = match curve {
            b::Curve::Line { start, end } => {
                let (s, e) = (v(start), v(end));
                let (p, q) = (s + (e - s) * a, s + (e - s) * z);
                return Some(Curve2::Spline { knots: vec![a, a, z, z], points: vec![uv(p), uv(q)] });
            }
            b::Curve::Polyline { points } => {
                let mut knots = vec![0.0];
                knots.extend((0..points.len()).map(|i| i as f64));
                knots.push(points.len().saturating_sub(1) as f64);
                (1, knots, points.iter().map(|p| HPoint::weighted(v(p), 1.0)).collect())
            }
            b::Curve::Spline { knots, points } => {
                (knots.len().checked_sub(points.len() + 1)?, knots.clone(), points.iter().map(|p| HPoint::weighted(v(p), 1.0)).collect())
            }
            b::Curve::RationalSpline { knots, homogeneous } => (
                knots.len().checked_sub(homogeneous.len() + 1)?,
                knots.clone(),
                homogeneous.iter().map(|h| HPoint { x: h[0], y: h[1], z: h[2], w: h[3] }).collect(),
            ),
            b::Curve::Circle { .. } | b::Curve::Ellipse { .. } | b::Curve::SurfaceIntersection { .. } => return None,
        };
        // The piece on the edge's own interval (the same points at the same parameters).
        let domain = [*knots.get(degree)?, *knots.get(knots.len() - degree - 1)?];
        let (knots, ctrl) = if domain == [a, z] { (knots, ctrl) } else { solidcraft_geom::bspline::segment(degree, &knots, &ctrl, a, z).ok()? };
        let homogeneous = ctrl
            .iter()
            .map(|h| {
                let p = uv(Vec3::new(h.x / h.w, h.y / h.w, h.z / h.w));
                [p[0] * h.w, p[1] * h.w, h.w]
            })
            .collect();
        Some(Curve2::RationalSpline { knots, homogeneous })
    }
}

/// The one-face sheet on `surface` bounded by the loop `chain` of `record`'s own edges, run
/// counter-clockwise about the surface's natural normal (`ccw`: the chain already does), each
/// edge's pcurve the exact projection where the surface is a height field. Its history is over
/// `[body]`: the face is new, each edge the body's own.
fn patch_body(body: &Body, record: &b::Brep, chain: &[(usize, bool)], surface: b::Surface, ccw: bool, pcurves: &Pcurves, tol: f64) -> Result<Body> {
    let fail = |m: &str| KernelError::Failed(format!("patch: {m}"));
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut vertex_of: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut edges = Vec::with_capacity(chain.len());
    let mut uses = Vec::with_capacity(chain.len());
    for (k, &(e, forward)) in chain.iter().enumerate() {
        let edge = record.edges.get(e).ok_or_else(|| fail("edge reference"))?;
        let mut ends = [0u32; 2];
        for (end, v) in ends.iter_mut().zip(edge.vertices) {
            let next = u32::try_from(vertices.len()).map_err(|_| fail("vertex budget"))?;
            *end = *vertex_of.entry(v).or_insert(next);
            if *end == next {
                vertices.push(record.vertices.get(v as usize).ok_or_else(|| fail("vertex reference"))?.point);
            }
        }
        let given = match pcurves {
            Pcurves::Projected(p) => Some(p.pcurve(edge.parameters(), edge.interval).ok_or_else(|| {
                KernelError::Invalid(
                    "not supported yet: a fitted patch on a circular, elliptic or traced edge (it has no exact pcurve on the fit)".into(),
                )
            })?),
            Pcurves::Computed => None,
        };
        edges.push(PartEdge { curve: edge.curve.clone(), nurbs: edge.nurbs.clone(), interval: edge.interval, vertices: ends });
        uses.push(PartUse { edge: k as u32, forward, given });
    }
    if !ccw {
        uses = uses.into_iter().rev().map(|u| PartUse { forward: !u.forward, ..u }).collect();
    }
    let mut made = assemble(Parts { vertices, edges, faces: vec![PartFace { surface, forward: true, loops: vec![uses] }], tolerance: tol }, false)?;
    hold_gap(&mut made.brep, tol);
    let smlib = solidcraft_smlib::Brep::from_record(&made.brep, true).map_err(|e| KernelError::Failed(format!("patch: SMLib: {e}")))?;
    let (patch, order) = crate::native::body_from_smlib_ordered(&smlib)?;
    // The patch's edges in its own order, each from the loop edge it is.
    let Some(order) = order else { return Ok(patch) };
    let mut origins: Vec<Option<usize>> = vec![None; patch.edge_count()];
    for (k, c) in made.edges.iter().enumerate() {
        let (Some(c), Some(&(e, _))) = (c, chain.get(k)) else { continue };
        let j = if order.edges.is_empty() { *c } else { *order.edges.get(*c).ok_or_else(|| fail("edge order"))? };
        *origins.get_mut(j).ok_or_else(|| fail("edge order"))? = Some(e);
    }
    let Some(origins) = origins.into_iter().collect::<Option<Vec<usize>>>() else { return Ok(patch) };
    let history = History {
        operands: 1,
        faces: vec![FaceProv { origins: Vec::new(), change: Change::Generated }],
        edges: origins.into_iter().map(|edge| EdgeProv { faces: vec![0], origins: vec![EdgeOrigin::Edge { operand: 0, edge }] }).collect(),
    };
    patch.with_history(history, &[body])
}

/// The sheet of the general fill's patches on the loop `chain` of the record's edges: each edge
/// whole, or (a loop of five or more) its two halves at the middle of its interval, both on its
/// exact curve record; the spokes from the halves' common vertex to the centre; every pcurve the
/// straight line in (u, v) the patch gives its side. Its history is over `[body]`: every face is
/// new, each loop edge (or half) a piece of the body's own, each spoke new.
fn quilt_body(body: &Body, record: &b::Brep, chain: &[Piece], fill: &IsoFill, tol: f64) -> Result<Body> {
    use solidcraft_geom::fill_iso::IsoSide;
    let fail = |m: &str| KernelError::Failed(format!("patch: {m}"));
    let index = |n: usize| u32::try_from(n).map_err(|_| fail("budget"));
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut vertex_of: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut middle_of: std::collections::HashMap<(usize, u64), u32> = std::collections::HashMap::new();
    let mut edges: Vec<PartEdge> = Vec::new();
    // Each part edge's origin: the record edge it is a piece of (`None` for a spoke).
    let mut origin: Vec<Option<usize>> = Vec::new();
    let mut piece_of: std::collections::HashMap<(usize, u64, u64), u32> = std::collections::HashMap::new();
    let mut spoke_of: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let centre = match fill.spokes.first().and_then(|s| s.points.last()) {
        Some(c) => {
            vertices.push([c.x, c.y, c.z]);
            Some(0u32)
        }
        None => None,
    };
    let mut faces = Vec::with_capacity(fill.patches.len());
    for patch in &fill.patches {
        let mut uses = Vec::with_capacity(patch.sides.len());
        for side in &patch.sides {
            let line = |interval: [f64; 2], at: [[f64; 2]; 2]| Curve2::Spline {
                knots: vec![interval[0], interval[0], interval[1], interval[1]],
                points: at.to_vec(),
            };
            match *side {
                IsoSide::Edge { edge: k, from, to, uv } => {
                    let &Piece { edge: e, forward, interval: [plo, phi] } = chain.get(k).ok_or_else(|| fail("side"))?;
                    let edge = record.edges.get(e).ok_or_else(|| fail("edge reference"))?;
                    let [a, z] = edge.interval;
                    // The patch's parameter runs the loop's way: the edge's own, reflected where the
                    // loop runs against it; a side's ends are its piece's ends or middle, exactly.
                    let tau = |t: f64| on_record(a, z, [plo, phi], forward, t);
                    let (tf, tt) = (tau(from), tau(to));
                    let (lo, hi, along) = if tf < tt { (tf, tt, true) } else { (tt, tf, false) };
                    let key = (e, lo.to_bits(), hi.to_bits());
                    let id = match piece_of.get(&key) {
                        Some(id) => *id,
                        None => {
                            let ends = [
                                vertex_on(record, &mut vertices, &mut vertex_of, &mut middle_of, e, lo)?,
                                vertex_on(record, &mut vertices, &mut vertex_of, &mut middle_of, e, hi)?,
                            ];
                            let id = index(edges.len())?;
                            // The edge itself: the interval and the pcurve's parameter are on its
                            // NURBS form (a conic's), which it keeps.
                            edges.push(PartEdge { curve: edge.curve.clone(), nurbs: edge.nurbs.clone(), interval: [lo, hi], vertices: ends });
                            origin.push(Some(e));
                            piece_of.insert(key, id);
                            id
                        }
                    };
                    let at = if along { uv } else { [uv[1], uv[0]] };
                    uses.push(PartUse { edge: id, forward: along, given: Some(line([lo, hi], at)) });
                }
                IsoSide::Spoke { spoke, forward, uv } => {
                    let id = match spoke_of.get(&spoke) {
                        Some(id) => *id,
                        None => {
                            let curve = fill.spokes.get(spoke).ok_or_else(|| fail("spoke"))?;
                            // Its foot on the loop, in the record edge's parameter as the sides'
                            // pieces are (the same bits where a side's piece ends there).
                            let &(k, t) = fill.feet.get(spoke).ok_or_else(|| fail("spoke"))?;
                            let &Piece { edge: e, forward, interval: [plo, phi] } = chain.get(k).ok_or_else(|| fail("spoke"))?;
                            let edge = record.edges.get(e).ok_or_else(|| fail("edge reference"))?;
                            let [a, z] = edge.interval;
                            let start = vertex_on(record, &mut vertices, &mut vertex_of, &mut middle_of, e, on_record(a, z, [plo, phi], forward, t))?;
                            let end = centre.ok_or_else(|| fail("centre"))?;
                            let id = index(edges.len())?;
                            let spline = match &curve.weights {
                                Some(w) => b::Curve::RationalSpline {
                                    knots: curve.knots.clone(),
                                    homogeneous: curve.points.iter().zip(w).map(|(p, w)| [p.x * w, p.y * w, p.z * w, *w]).collect(),
                                },
                                None => {
                                    b::Curve::Spline { knots: curve.knots.clone(), points: curve.points.iter().map(|p| [p.x, p.y, p.z]).collect() }
                                }
                            };
                            edges.push(PartEdge { curve: spline, nurbs: None, interval: [0.0, 1.0], vertices: [start, end] });
                            origin.push(None);
                            spoke_of.insert(spoke, id);
                            id
                        }
                    };
                    let at = if forward { uv } else { [uv[1], uv[0]] };
                    uses.push(PartUse { edge: id, forward, given: Some(line([0.0, 1.0], at)) });
                }
            }
        }
        faces.push(PartFace { surface: crate::surfaces::spline_surface(&patch.surface), forward: true, loops: vec![uses] });
    }
    let mut made = assemble(Parts { vertices, edges, faces, tolerance: tol }, false)?;
    hold_gap(&mut made.brep, tol);
    let smlib = solidcraft_smlib::Brep::from_record(&made.brep, true).map_err(|e| KernelError::Failed(format!("patch: SMLib: {e}")))?;
    let (sheet, order) = crate::native::body_from_smlib_ordered(&smlib)?;
    let Some(order) = order else { return Ok(sheet) };
    let place = |list: &[usize], c: usize| if list.is_empty() { Some(c) } else { list.get(c).copied() };
    // Each body edge's origin and the body faces beside it.
    let mut prov: Vec<Option<EdgeProv>> = vec![None; sheet.edge_count()];
    let record_out = &made.brep;
    for (pe, c) in made.edges.iter().enumerate() {
        let Some(c) = *c else { continue };
        let j = place(&order.edges, c).ok_or_else(|| fail("edge order"))?;
        let mut beside: Vec<usize> =
            record_out.edges.get(c).map(|x| x.uses.iter().filter_map(|u| place(&order.faces, u.face as usize)).collect()).unwrap_or_default();
        beside.sort_unstable();
        beside.dedup();
        let origins = origin.get(pe).copied().flatten().map(|edge| vec![EdgeOrigin::Edge { operand: 0, edge }]).unwrap_or_default();
        *prov.get_mut(j).ok_or_else(|| fail("edge order"))? = Some(EdgeProv { faces: beside, origins });
    }
    let Some(edges) = prov.into_iter().collect::<Option<Vec<EdgeProv>>>() else { return Ok(sheet) };
    let history = History { operands: 1, faces: vec![FaceProv { origins: Vec::new(), change: Change::Generated }; sheet.face_count()], edges };
    sheet.with_history(history, &[body])
}

/// A patch record's edges and vertices given the tolerances that hold a surface `gap` from them.
/// A record's tolerance is SMLib's intersection tolerance, twice the zone tolerance SMLib holds an
/// edge's gap to its faces to, and a vertex holds gaps up to half its zone tolerance (SMLib's
/// `SM_XSECT_TO_ZONETOL3D`, `SmBrep::ValidateAndUpdateTolerances`): an edge takes `2·gap`, a
/// vertex `4·gap`.
fn hold_gap(brep: &mut b::Brep, gap: f64) {
    for e in &mut brep.edges {
        e.tolerance = e.tolerance.max(2.0 * gap);
    }
    for v in &mut brep.vertices {
        v.tolerance = v.tolerance.max(4.0 * gap);
    }
}

/// The part vertex at parameter `t` of record edge `e`: the record's own vertex at an end of the
/// edge's interval (made once), else [`middle`].
fn vertex_on(
    record: &b::Brep,
    vertices: &mut Vec<[f64; 3]>,
    vertex_of: &mut std::collections::HashMap<u32, u32>,
    middle_of: &mut std::collections::HashMap<(usize, u64), u32>,
    e: usize,
    t: f64,
) -> Result<u32> {
    let fail = |m: &str| KernelError::Failed(format!("patch: {m}"));
    let edge = record.edges.get(e).ok_or_else(|| fail("edge reference"))?;
    let [a, z] = edge.interval;
    if t != a && t != z {
        return middle(vertices, middle_of, e, edge, t);
    }
    let [start, finish] = edge.vertices;
    let v = if t == a { start } else { finish };
    let next = u32::try_from(vertices.len()).map_err(|_| fail("vertex budget"))?;
    let id = *vertex_of.entry(v).or_insert(next);
    if id == next {
        vertices.push(record.vertices.get(v as usize).ok_or_else(|| fail("vertex reference"))?.point);
    }
    Ok(id)
}

/// The vertex at parameter `t` inside record edge `e` (`edge`), made once.
fn middle(vertices: &mut Vec<[f64; 3]>, made: &mut std::collections::HashMap<(usize, u64), u32>, e: usize, edge: &b::Edge, t: f64) -> Result<u32> {
    let k = (e, t.to_bits());
    if let Some(v) = made.get(&k) {
        return Ok(*v);
    }
    let p =
        CurveEval::new(edge.parameters()).ok().and_then(|c| c.point(t)).ok_or_else(|| KernelError::Failed("patch: an edge has no middle".into()))?;
    let v = u32::try_from(vertices.len()).map_err(|_| KernelError::Failed("patch: vertex budget".into()))?;
    vertices.push([p.x, p.y, p.z]);
    made.insert(k, v);
    Ok(v)
}

/// How far a loop's edges may be from a plane for its patch to be that plane: `10⁻⁷` of the
/// body's size, at least the linear tolerance (`10⁻⁶` mm) — the body's own "same point"
/// tolerance, since a planar face is exact and other operations treat it as containing its
/// edges to that tolerance. A fitted face gets [`face_tolerance`], ten times looser.
pub(crate) fn planarity_tolerance(body: &Body) -> f64 {
    (placed_size(body) * 1e-7).max(solidcraft_geom::LINEAR_TOL)
}

/// The body's size as the fill tolerances take it, the same wherever the body is and however it
/// is turned (the bounding box's diagonal is not: a cup turned 0.7 rad about (1, 2, 3) grows from
/// 54.7 to 61.6, and its fill was sampled to another tolerance): the diameter of the smallest ball
/// holding its vertices. (Twice the farthest vertex's distance from their centroid is not the
/// body's size where the vertices crowd one end: up to twice its diameter, and a rounded box's G2
/// corner patch, its vertices crowding that corner, then fitted past the box.) A body with no
/// vertices takes its box's diagonal.
fn placed_size(body: &Body) -> f64 {
    let vertices: Vec<Vec3> = body.record().vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect();
    solidcraft_geom::Ball::enclosing(&vertices).map_or_else(|| body.size(), |ball| ball.diameter())
}

/// How far the fill's surface may be from the loop's edges: `10⁻⁶` of the body's size
/// ([`placed_size`]), at least
/// `10⁻⁵` mm — ten times [`planarity_tolerance`]. Ten times the linear tolerance because a
/// fitted surface approaches curved edges like a cubic spline, so each further factor of ten
/// costs about twice the spans each way, and imported edges may already lie `10⁻⁵` mm or more off
/// their vertices. Later operations must take the face to contain its edges within this
/// tolerance, not the linear one: the face's edges are the body's exact edges, the surface
/// between them is the fit (the patch record's edges and vertices hold this gap: [`hold_gap`]).
pub(crate) fn face_tolerance(body: &Body) -> f64 {
    (placed_size(body) * 1e-6).max(1e-5)
}

#[cfg(test)]
mod tests {
    use solidcraft_geom::brep::{self as b, eval::CurveEval, eval::SurfaceEval};
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

    use crate::body::Body;
    use crate::boolean_record::tests::{Reach, checked};
    use crate::history::{Change, EdgeOrigin};
    use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
    use crate::{BoolOp, boolean, copy_faces, cylinder, extrude, measure, patch_edges, patch_edges_crowned, stitch};

    fn v3(p: [f64; 3]) -> Vec3 {
        Vec3::new(p[0], p[1], p[2])
    }

    /// The cutting cylinder: axis along y at x = 0, z = 50, radius 40, so the top of a prism
    /// within 20 of the axis and 20 high is cut away everywhere (to between 10 and 15.4 mm).
    fn cut_height(x: f64) -> f64 {
        50.0 - (1600.0 - x * x).sqrt()
    }

    /// A prism on the polygon `corners`, 20 high, its top cut by the cylinder; the cut solid, the
    /// open cup without its top, and a point on each rim edge (the edges' midpoints in plan).
    /// `inside` is a point inside the polygon.
    fn cup(corners: &[Vec2], inside: Vec2) -> (Body, Body, Vec<Vec3>) {
        let prism = extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(corners), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let solid = boolean(&prism, &tool, BoolOp::Cut).unwrap().unwrap();
        let n = corners.len();
        let mut keep = vec![inside.extend(0.0)];
        let mut rim = Vec::new();
        for k in 0..n {
            let m = corners[k].lerp(corners[(k + 1) % n], 0.5);
            keep.push(m.extend(5.0));
            rim.push(m.extend(cut_height(m.x)));
        }
        let open = copy_faces(&solid, &keep).unwrap();
        assert!(open.is_surface());
        (solid, open, rim)
    }

    fn regular(n: usize, phase: f64) -> Vec<Vec2> {
        (0..n).map(|k| Vec2::from_angle(phase + k as f64 * std::f64::consts::TAU / n as f64) * 20.0).collect()
    }

    /// The body's record.
    fn record(body: &Body) -> b::Brep {
        crate::native::brep_record(body).unwrap()
    }

    /// The surface of the body's first face.
    fn first_surface(body: &Body) -> b::Surface {
        record(body).faces[0].surface.clone()
    }

    /// The distance from `p` to the surface (its nearest point).
    fn to_surface(s: &b::Surface, p: Vec3) -> f64 {
        let e = SurfaceEval::new(s).unwrap();
        let uv = e.project(p, None, f64::INFINITY).expect("a nearest point");
        e.point(uv).unwrap().dist(p)
    }

    /// The farthest point along the first face's edges from its surface.
    fn edges_off_surface(body: &Body) -> f64 {
        let r = record(body);
        let face = &r.faces[0];
        let mut worst = 0.0f64;
        for l in &face.loops {
            let b::Loop::Edges { edges } = l else { continue };
            for u in edges {
                let e = &r.edges[u.edge as usize];
                let c = CurveEval::new(e.parameters()).unwrap();
                let [t0, t1] = e.interval;
                for k in 0..=24 {
                    worst = worst.max(to_surface(&face.surface, c.point(t0 + (t1 - t0) * k as f64 / 24.0).unwrap()));
                }
            }
        }
        worst
    }

    /// Whether every edge of `made` is one of `of`'s own: the same kind of curve, on the same
    /// interval, on the curve it is parameterised on (its NURBS form where it has one) to
    /// rounding: the same knots and control points within 10⁻¹² of their size. (SMLib makes a
    /// conic's frame and NURBS form again on a round trip, the last bit of a coordinate apart.)
    fn own_edges(made: &Body, of: &Body) -> bool {
        let own = record(of).edges;
        let kind = |e: &b::Edge| std::mem::discriminant(&e.curve);
        let close = |p: &[f64], q: &[f64]| p.len() == q.len() && p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs().max(y.abs())));
        let same = |c: &b::Curve, d: &b::Curve| match (c, d) {
            (b::Curve::Spline { knots: k, points: p }, b::Curve::Spline { knots: l, points: q }) => {
                k == l && p.len() == q.len() && p.iter().zip(q).all(|(x, y)| close(x, y))
            }
            (b::Curve::RationalSpline { knots: k, homogeneous: p }, b::Curve::RationalSpline { knots: l, homogeneous: q }) => {
                k == l && p.len() == q.len() && p.iter().zip(q).all(|(x, y)| close(x, y))
            }
            _ => c == d,
        };
        record(made).edges.iter().all(|e| own.iter().any(|o| kind(o) == kind(e) && same(o.parameters(), e.parameters()) && o.interval == e.interval))
    }

    /// The fill's mesh: every normal finite, and no triangle folded over (all face the same way
    /// as the mesh's total area vector, by a clear margin).
    fn mesh_is_clean(fill: &Body) -> f64 {
        let m = fill.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        assert!(!m.triangles.is_empty());
        assert!(m.normals.iter().all(|n| n.is_finite() && n.len() > 0.5), "a mesh normal is not finite");
        let tri = |t: &[u32; 3]| m.tri(t).unwrap();
        let total = m.triangles.iter().map(|t| {
            let [a, b, c] = tri(t);
            (b - a).cross(c - a)
        });
        let total = total.fold(Vec3::ZERO, |s, v| s + v);
        let n = total.normalized().unwrap();
        for t in &m.triangles {
            let [a, b, c] = tri(t);
            let v = (b - a).cross(c - a);
            if v.len() > 1e-12 {
                assert!(v.dot(n) > 0.0, "a folded triangle");
            }
        }
        measure(fill).unwrap().area
    }

    /// Fill the rim, check the face, stitch it back: (the fill, the closed solid's volume).
    fn fill_rim(open: &Body, rim: &[Vec3]) -> (Body, f64) {
        let fill = patch_edges(open, rim).unwrap();
        assert!(fill.is_surface());
        assert_eq!(fill.face_count(), 1, "one face");
        assert!(!matches!(first_surface(&fill), b::Surface::Plane { .. }));
        // Its edges are the open body's own, and nothing else.
        assert!(own_edges(&fill, open), "an edge that is not the body's");
        let h = checked(&fill, &[open], 0.05, Reach { made: 0.0, moved: 0.0 }, &|_| true);
        assert!(h.faces.iter().all(|f| f.change == Change::Generated && f.origins.is_empty()));
        assert!(h.edges.iter().all(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Edge { operand: 0, .. }])));
        let off = edges_off_surface(&fill);
        let tol = super::face_tolerance(open);
        assert!(off <= tol, "an edge is {off} off the face (tolerance {tol})");
        mesh_is_clean(&fill);
        let closed = stitch(&[open, &fill], 0.0).unwrap();
        assert!(!closed.is_surface(), "the fill closes the cup");
        (fill, measure(&closed).unwrap().volume)
    }

    /// The fill's height over the point `(x, y)`: Newton's method on the surface's `x`, `y` from
    /// the middle of its face's domain (or the last answer).
    fn height_over(body: &Body, x: f64, y: f64, start: &mut Option<(f64, f64)>) -> Option<f64> {
        let r = record(body);
        let face = &r.faces[0];
        let s = SurfaceEval::new(face.parameters()).ok()?;
        let [[u0, u1], [v0, v1]] = face.domain;
        let (mut u, mut v) = start.unwrap_or(((u0 + u1) / 2.0, (v0 + v1) / 2.0));
        for _ in 0..30 {
            let (q, a, b) = s.derivs(u, v)?;
            let (rx, ry) = (q.x - x, q.y - y);
            let det = a.x * b.y - a.y * b.x;
            if det.abs() < 1e-300 {
                return None;
            }
            u = (u - (b.y * rx - b.x * ry) / det).clamp(u0, u1);
            v = (v - (a.x * ry - a.y * rx) / det).clamp(v0, v1);
        }
        let q = s.point([u, v])?;
        *start = Some((u, v));
        ((q.x - x).hypot(q.y - y) < 1e-9).then_some(q.z)
    }

    /// The volume the cut prism gains with the fill for its top: the fill's height above the
    /// cylinder integrated over the polygon (midpoint rule on a 300 × 300 grid; the integrand
    /// vanishes on the rim, so cells across it add little error).
    fn gained(fill: &Body, corners: &[Vec2]) -> f64 {
        let polygon = solidcraft_geom::Ring::from(corners.to_vec());
        let solidcraft_geom::Aabb2 { min: lo, max: hi } = polygon.bounds();
        let n = 300;
        let (dx, dy) = ((hi.x - lo.x) / n as f64, (hi.y - lo.y) / n as f64);
        let inside = |x: f64, y: f64| polygon.contains(Vec2::new(x, y));
        let mut sum = 0.0;
        let mut start = None;
        for i in 0..n {
            let x = lo.x + (i as f64 + 0.5) * dx;
            for j in 0..n {
                let y = lo.y + (j as f64 + 0.5) * dy;
                if inside(x, y) {
                    let h = height_over(fill, x, y, &mut start).expect("the fill covers the polygon");
                    sum += (h - cut_height(x)) * dx * dy;
                }
            }
        }
        sum
    }

    /// The closed solid's volume against the cut prism's plus what the fill adds over the
    /// cylinder (integrated independently, [`gained`]); they agree to the integration's error
    /// (0.1% of what the fill adds) plus the error of measuring both solids on meshes
    /// (`5·10⁻⁵` of the volume).
    fn check_volume(name: &str, solid: &Body, fill: &Body, corners: &[Vec2], got: f64) {
        let prism = measure(solid).unwrap().volume;
        let extra = gained(fill, corners);
        let want = prism + extra;
        let bound = 1e-3 * extra.abs() + 5e-5 * want;
        eprintln!("{name}: closed volume {got:.4}, cut prism {prism:.4} + fill above the cylinder {extra:.4} = {want:.4} (bound ±{bound:.4})");
        assert!((got - want).abs() <= bound, "{name}: {got} vs {want}");
    }

    /// An open sheet of walls hanging from the closed loop `top` (each a curve record from one
    /// corner to the next, `corners[k]` to `corners[k + 1]`), each wall swept straight along
    /// `drop`: a plane under a line, a ruled B-spline under a spline (its pcurves its exact iso
    /// lines and rulings).
    fn walls(corners: &[Vec3], top: &[b::Curve], drop: Vec3) -> Body {
        let n = corners.len();
        let arr = |p: Vec3| [p.x, p.y, p.z];
        // Vertices: the top corners, then the bottom ones.
        let vertices: Vec<[f64; 3]> = corners.iter().map(|p| arr(*p)).chain(corners.iter().map(|p| arr(*p + drop))).collect();
        let moved = |c: &b::Curve| match c {
            b::Curve::Line { start, end } => b::Curve::Line { start: arr(v3(*start) + drop), end: arr(v3(*end) + drop) },
            b::Curve::Spline { knots, points } => {
                b::Curve::Spline { knots: knots.clone(), points: points.iter().map(|p| arr(v3(*p) + drop)).collect() }
            }
            _ => panic!("lines and splines"),
        };
        let interval = |c: &b::Curve| match c {
            b::Curve::Spline { knots, points } => [knots[knots.len() - points.len() - 1], knots[points.len()]],
            _ => [0.0, 1.0],
        };
        let mut edges = Vec::new();
        for k in 0..n {
            let c = &top[k];
            edges.push(PartEdge { nurbs: None, curve: c.clone(), interval: interval(c), vertices: [k as u32, ((k + 1) % n) as u32] });
            edges.push(PartEdge { nurbs: None, curve: moved(c), interval: interval(c), vertices: [(n + k) as u32, (n + (k + 1) % n) as u32] });
            edges.push(PartEdge {
                nurbs: None,
                curve: b::Curve::Line { start: arr(corners[k]), end: arr(corners[k] + drop) },
                interval: [0.0, 1.0],
                vertices: [k as u32, (n + k) as u32],
            });
        }
        let (top_edge, bottom_edge, down) = (|k: usize| 3 * k as u32, |k: usize| 3 * k as u32 + 1, |k: usize| 3 * (k % n) as u32 + 2);
        let mut faces = Vec::new();
        for k in 0..n {
            let (surface, given) = match &top[k] {
                b::Curve::Spline { knots, points } => {
                    let rows: Vec<Vec<[f64; 3]>> = points.iter().map(|p| vec![*p, arr(v3(*p) + drop)]).collect();
                    let [a, z] = interval(&top[k]);
                    let degree = knots.len() - points.len() - 1;
                    let greville = |v: f64| -> b::Curve2 {
                        let pts = (0..points.len()).map(|i| [(1..=degree).map(|r| knots[i + r]).sum::<f64>() / degree as f64, v]).collect();
                        b::Curve2::Spline { knots: knots.clone(), points: pts }
                    };
                    let ruling = |u: f64| b::Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[u, 0.0], [u, 1.0]] };
                    let surface = b::Surface::Spline { u_knots: knots.clone(), v_knots: vec![0.0, 0.0, 1.0, 1.0], points: rows };
                    (surface, Some([greville(0.0), ruling(z), greville(1.0), ruling(a)]))
                }
                _ => {
                    let (p, q) = (corners[k], corners[(k + 1) % n]);
                    let u = (q - p).normalized().unwrap();
                    let w = (drop - u * drop.dot(u)).normalized().unwrap();
                    (b::Surface::Plane { origin: arr(p), u_axis: arr(u), v_axis: arr(w) }, None)
                }
            };
            // Round the wall: along the top, down its far side, back along the bottom, up.
            let mut uses = vec![
                PartUse { edge: top_edge(k), forward: true, given: None },
                PartUse { edge: down(k + 1), forward: true, given: None },
                PartUse { edge: bottom_edge(k), forward: false, given: None },
                PartUse { edge: down(k), forward: false, given: None },
            ];
            if let Some(given) = given {
                for (u, g) in uses.iter_mut().zip(given) {
                    u.given = Some(g);
                }
            }
            faces.push(PartFace { surface, forward: true, loops: vec![uses] });
        }
        let made = assemble(Parts { vertices, edges, faces, tolerance: 1e-6 }, false).unwrap();
        crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&made.brep, true).unwrap()).unwrap()
    }

    #[test]
    fn regular_rims_of_three_to_eight_sides_fill_with_one_face_and_close_the_cup() {
        for n in 3..=8 {
            let corners = regular(n, 0.3);
            let (solid, open, rim) = cup(&corners, Vec2::ZERO);
            let (fill, got) = fill_rim(&open, &rim);
            assert_eq!(fill.edge_count(), n, "the rim's own edges");
            check_volume(&format!("{n} sides"), &solid, &fill, &corners, got);
        }
    }

    #[test]
    fn concave_rims_fill_with_one_face() {
        let v = |x: f64, y: f64| Vec2::new(x, y);
        let shapes = [
            ("dart", vec![v(-10.0, 0.0), v(15.0, -12.0), v(3.0, 0.0), v(15.0, 12.0)], v(0.0, 0.0)),
            ("arrow", vec![v(-12.0, 0.0), v(0.0, -10.0), v(0.0, -4.0), v(14.0, 0.0), v(0.0, 4.0)], v(0.0, 0.0)),
            (
                "plus",
                vec![
                    v(-4.0, -14.0),
                    v(4.0, -14.0),
                    v(4.0, -4.0),
                    v(14.0, -4.0),
                    v(14.0, 4.0),
                    v(4.0, 4.0),
                    v(4.0, 14.0),
                    v(-4.0, 14.0),
                    v(-4.0, 4.0),
                    v(-14.0, 4.0),
                    v(-14.0, -4.0),
                    v(-4.0, -4.0),
                ],
                v(0.0, 0.0),
            ),
        ];
        for (name, corners, inside) in shapes {
            let (solid, open, rim) = cup(&corners, inside);
            let (fill, got) = fill_rim(&open, &rim);
            assert_eq!(fill.edge_count(), corners.len(), "{name}: the rim's own edges");
            check_volume(name, &solid, &fill, &corners, got);
        }
    }

    #[test]
    fn a_flat_corner_between_two_edges_is_kept() {
        // A rim of five straight edges, two of them in line (a 180° corner between them), the
        // four true corners at different heights; walls hang below it.
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        let v = [p(-14.0, -14.0, 12.0), p(0.0, -14.0, 12.0), p(14.0, -14.0, 12.0), p(14.0, 14.0, 15.0), p(-14.0, 14.0, 10.0)];
        let arr = |q: Vec3| [q.x, q.y, q.z];
        let top: Vec<b::Curve> = (0..5).map(|k| b::Curve::Line { start: arr(v[k]), end: arr(v[(k + 1) % 5]) }).collect();
        let body = walls(&v, &top, Vec3::new(0.0, 0.0, -10.0));
        let rim: Vec<Vec3> = (0..5).map(|k| v[k].lerp(v[(k + 1) % 5], 0.5)).collect();
        let fill = patch_edges(&body, &rim).unwrap();
        assert_eq!((fill.face_count(), fill.edge_count()), (1, 5), "one face on all five edges");
        assert!(edges_off_surface(&fill) <= super::face_tolerance(&body));
        let area = mesh_is_clean(&fill);
        eprintln!("flat corner: one face of area {area}");
        assert!(area > 28.0 * 28.0 * 0.99 && area < 28.0 * 28.0 * 1.05, "{area}");
    }

    #[test]
    fn the_fill_does_not_depend_on_pick_order_or_where_the_body_is() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        let (a, _) = fill_rim(&open, &rim);
        let mut shuffled = rim.clone();
        shuffled.rotate_left(3);
        shuffled.swap(0, 1);
        let (b, _) = fill_rim(&open, &shuffled);
        let (ra, rb) = (record(&a), record(&b));
        let (sa, sb) = (SurfaceEval::new(ra.faces[0].parameters()).unwrap(), SurfaceEval::new(rb.faces[0].parameters()).unwrap());
        let [[u0, u1], [v0, v1]] = ra.faces[0].domain;
        let mut gap = 0.0f64;
        for (x, y) in [(0.2, 0.3), (0.5, 0.5), (0.7, 0.9)] {
            let uv = [u0 + (u1 - u0) * x, v0 + (v1 - v0) * y];
            gap = gap.max(sa.point(uv).unwrap().dist(sb.point(uv).unwrap()));
        }
        assert!(gap < 1e-7, "pick order moved the surface by {gap}");
        let (area_a, area_b) = (mesh_is_clean(&a), mesh_is_clean(&b));
        assert!((area_a - area_b).abs() < area_a * 1e-9);
        // Moved and turned: the same area.
        let moved = crate::transform(&open, Vec3::new(130.0, -70.0, 40.0), Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), 0.7).unwrap();
        let ax = Vec3::new(1.0, 2.0, 3.0).normalized().unwrap();
        let turn = |p: Vec3| {
            // Rodrigues' rotation by 0.7 about `ax`, then the shift.
            let (c, s) = (0.7f64.cos(), 0.7f64.sin());
            p * c + ax.cross(p) * s + ax * (ax.dot(p) * (1.0 - c)) + Vec3::new(130.0, -70.0, 40.0)
        };
        let moved_rim: Vec<Vec3> = rim.iter().map(|p| turn(*p)).collect();
        let (c, _) = fill_rim(&moved, &moved_rim);
        let area_c = mesh_is_clean(&c);
        eprintln!("pick order: surfaces within {gap:e}; areas {area_a} / {area_b} / moved {area_c}");
        assert!((area_a - area_c).abs() < area_a * 1e-6, "{area_a} vs moved {area_c}");
    }

    #[test]
    fn a_saddle_rim_of_two_intersection_curves_fills() {
        // A round post with its top cut by the sideways cylinder: the rim is two intersection
        // curves; the fill is one face on them.
        let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let solid = boolean(&post, &tool, BoolOp::Cut).unwrap().unwrap();
        let sides: Vec<Vec3> = (0..8).map(|k| Vec2::from_angle(k as f64 * 0.8).extend(0.0) * 10.0 + Vec3::new(0.0, 0.0, 5.0)).collect();
        let open = copy_faces(&solid, &[&[Vec3::ZERO][..], &sides].concat()).unwrap();
        let rim: Vec<Vec3> = [-0.98, 2.18]
            .map(|a: f64| {
                let (x, y) = (10.0 * a.cos(), 10.0 * a.sin());
                Vec3::new(x, y, cut_height(x))
            })
            .to_vec();
        let (fill, got) = fill_rim(&open, &rim);
        let circle: Vec<Vec2> = (0..720).map(|k| Vec2::from_angle(k as f64 * std::f64::consts::TAU / 720.0) * 10.0).collect();
        check_volume("saddle rim", &solid, &fill, &circle, got);
    }

    #[test]
    fn imported_edges_whose_vertices_are_off_their_curves_fill() {
        // The cup's vertices moved 2·10⁻⁵ mm off their curves, as a file might give them.
        let corners = regular(5, 0.3);
        let (solid, open, rim) = cup(&corners, Vec2::ZERO);
        let mut moved = record(&open);
        for v in &mut moved.vertices {
            v.point[0] += 2e-5;
            v.tolerance = v.tolerance.max(1e-4);
        }
        let imported = crate::native::body_from_smlib(&solidcraft_smlib::Brep::from_record(&moved, true).unwrap()).unwrap();
        let (fill, got) = fill_rim(&imported, &rim);
        check_volume("imported", &solid, &fill, &corners, got);
    }

    #[test]
    fn a_spline_edge_bulging_out_of_plane_is_never_given_a_flat_face() {
        // A flat rectangle of edges but for one cubic edge with a control point raised 1.5 mm: no
        // plane can be certified. A one-span bump is followed by the fitted face; a narrow one
        // (64 spans, about 1 mm wide, its third derivative jumping at every knot) may be beyond
        // a 32 × 32 grid, and then it is refused.
        for (spans, raised, must_fill) in [(1, 1, true), (64, 30, false)] {
            let count = spans + 3;
            let points: Vec<[f64; 3]> =
                (0..count).map(|i| [20.0 * i as f64 / (count - 1) as f64, 0.0, if i == raised { 1.5 } else { 0.0 }]).collect();
            // Uniform knots over [0, 1], clamped.
            let mut knots = vec![0.0; 4];
            knots.extend((1..spans).map(|i| i as f64 / spans as f64));
            knots.extend([1.0; 4]);
            let spline = b::Curve::Spline { knots, points };
            let c = CurveEval::new(&spline).unwrap();
            let corners: Vec<Vec3> = [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)].iter().map(|&(x, y)| Vec3::new(x, y, 0.0)).collect();
            let line = |a: Vec3, z: Vec3| b::Curve::Line { start: [a.x, a.y, a.z], end: [z.x, z.y, z.z] };
            let top = vec![spline.clone(), line(corners[1], corners[2]), line(corners[2], corners[3]), line(corners[3], corners[0])];
            let body = walls(&corners, &top, Vec3::new(0.0, 0.0, -5.0));
            let peak = c.point((raised as f64 + 1.0) / (count + 1) as f64).unwrap();
            let rim = [c.point(0.5).unwrap(), Vec3::new(20.0, 5.0, 0.0), Vec3::new(10.0, 10.0, 0.0), Vec3::new(0.0, 5.0, 0.0)];
            match patch_edges(&body, &rim) {
                Ok(fill) => {
                    let surface = first_surface(&fill);
                    assert!(!matches!(surface, b::Surface::Plane { .. }), "a plane would miss the bulge");
                    let (off, tol) = (edges_off_surface(&fill), super::face_tolerance(&body));
                    eprintln!("{spans}-span bump: filled, edges within {off:e} of the face");
                    assert!(off <= tol && to_surface(&surface, peak) <= tol);
                    mesh_is_clean(&fill);
                }
                Err(e) => {
                    eprintln!("{spans}-span bump: {e}");
                    assert!(!must_fill && e.to_string().contains("not supported yet"), "{e}");
                }
            }
        }
    }

    /// A point on each edge that bounds one face of `body` (its free edges).
    fn free_edge_points(body: &Body) -> Vec<Vec3> {
        let r = record(body);
        r.edges
            .iter()
            .filter(|e| e.uses.len() == 1)
            .map(|e| {
                let [t0, t1] = e.interval;
                CurveEval::new(e.parameters()).unwrap().point(0.5 * (t0 + t1)).unwrap()
            })
            .collect()
    }

    #[test]
    fn a_loop_that_does_not_bound_a_height_field_fills_with_one_face() {
        // Half a round post's side: two straight edges up and two half circles, whose plane
        // stands on the post's axis; the half circles run along its normal at their ends, so no
        // height field over it bounds them (the height-field fill refuses it). The general fill
        // (#759) fills it with one face on its four edges.
        let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let halves = crate::split_by_plane(&post, &Plane::from_normal(Vec3::ZERO, Vec3::X).unwrap()).unwrap();
        let half = halves.into_iter().find(|b| measure(b).unwrap().bbox.min.x > -1e-6).unwrap();
        let at: Vec<Vec3> =
            (-4..=4).map(|k| Vec2::from_angle(k as f64 * 0.35).extend(10.0) * 1.0).map(|p| Vec3::new(p.x * 10.0, p.y * 10.0, 10.0)).collect();
        let side = copy_faces(&half, &at).unwrap();
        let rim = free_edge_points(&side);
        let fill = patch_edges(&side, &rim).unwrap();
        assert_eq!(fill.face_count(), 1, "one face");
        let (off, tol) = (edges_off_surface(&fill), super::face_tolerance(&side));
        assert!(off <= tol, "an edge is {off} off the face (tolerance {tol})");
        // Its area is at least its shadow on the plane x = 0 (the 20 × 20 square the loop bounds
        // there) and, a least-bending fill, no more than the half side it spans.
        let area = measure(&fill).unwrap().area;
        eprintln!("half a post's side: one face of area {area}, its edges within {off:e}");
        assert!(area > 400.0 && area < std::f64::consts::PI * 10.0 * 20.0, "{area}");
    }

    #[test]
    fn planar_loops_of_intersection_curves_are_one_planar_face() {
        // A round post cut by planes tilted every way, gently to steeply: each rim is an ellipse
        // of intersection curves (split where the post's side has seams) lying on the cutting
        // plane, so the patch is one planar face, and the post closes again.
        for tilt in [Vec3::new(0.0, -0.4, 1.0), Vec3::new(0.7, -0.9, 1.0), Vec3::new(1.3, 0.2, 1.0)] {
            let post = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 30.0).unwrap();
            let n = tilt.normalized().unwrap();
            let plane = Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), n).unwrap();
            let pieces = crate::split_by_plane(&post, &plane).unwrap();
            let solid = pieces.into_iter().find(|b| measure(b).unwrap().bbox.min.z < 0.5).unwrap();
            let height = |x: f64, y: f64| 15.0 - (tilt.x * x + tilt.y * y) / tilt.z;
            let sides: Vec<Vec3> = (0..8).map(|k| Vec2::from_angle(k as f64 * 0.8).extend(0.0) * 10.0 + Vec3::new(0.0, 0.0, 1.0)).collect();
            let open = copy_faces(&solid, &[&[Vec3::ZERO][..], &sides].concat()).unwrap();
            let rim: Vec<Vec3> = (0..12)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / 12.0 + 0.2;
                    let (x, y) = (10.0 * a.cos(), 10.0 * a.sin());
                    Vec3::new(x, y, height(x, y))
                })
                .collect();
            let lid = patch_edges(&open, &rim).unwrap();
            assert_eq!(lid.face_count(), 1);
            let edges = lid.edge_count();
            assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }), "tilt {tilt:?}");
            let closed = stitch(&[&open, &lid], 0.0).unwrap();
            assert!(!closed.is_surface());
            let (want, got) = (measure(&solid).unwrap().volume, measure(&closed).unwrap().volume);
            eprintln!("rim cut at {tilt:?}: {edges} intersection curves, planar face, volume {got} against {want}");
            assert!((got - want).abs() < want * 1e-6, "{got} vs {want}");
        }
    }

    #[test]
    fn planar_loops_are_still_one_planar_face() {
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let open = copy_faces(
            &b,
            &[Vec3::new(5.0, 5.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(0.0, 5.0, 5.0)],
        )
        .unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
        let lid = patch_edges(&open, &rim).unwrap();
        assert_eq!(lid.face_count(), 1);
        assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }));
        let closed = stitch(&[&open, &lid], 0.0).unwrap();
        assert!((measure(&closed).unwrap().volume - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn open_partial_or_hostile_picks_are_refused() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        assert!(patch_edges(&open, &rim[..4]).is_err(), "a loop with a missing edge");
        assert!(patch_edges(&open, &[]).is_err());
        assert!(patch_edges(&open, &[Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
        assert!(patch_edges(&open, &[Vec3::new(1e300, 0.0, 0.0)]).is_err());
        assert!(patch_edges(&open, &vec![rim[0]; 20_000]).is_err());
    }

    /// A crowned patch: the middle is fuller by `crown` times the loop's size, the loop's own
    /// edges stay exactly where they were (the face still meets them within [`face_tolerance`]),
    /// and a planar loop is the same exact plane whatever the crown.
    #[test]
    fn a_crowned_patch_fills_its_middle_and_keeps_its_edges() {
        let (_, open, rim) = cup(&regular(5, 0.3), Vec2::ZERO);
        let plain = patch_edges(&open, &rim).unwrap();
        let same = patch_edges_crowned(&open, &rim, 0.0).unwrap();
        // Crown 0 is the plain patch: the same surface, to the last bit.
        assert_eq!(measure(&plain).unwrap().area, measure(&same).unwrap().area, "crown 0 is patch_edges");
        for crown in [0.25, -0.25] {
            let full = patch_edges_crowned(&open, &rim, crown).unwrap();
            assert_eq!(full.face_count(), 1, "one face");
            assert!(!matches!(first_surface(&full), b::Surface::Plane { .. }));
            assert!(own_edges(&full, &open), "an edge that is not the body's");
            // The loop's edges are within the fill's tolerance of the face, as for the plain fill.
            let off = edges_off_surface(&full);
            assert!(off <= super::face_tolerance(&open), "an edge is {off} off the crowned face");
            mesh_is_clean(&full);
            // The middle of the loop's box moved by crown × its diagonal (the bump is 1 there).
            let (ra, rb) = (record(&plain), record(&full));
            let (a, b) = (SurfaceEval::new(ra.faces[0].parameters()).unwrap(), SurfaceEval::new(rb.faces[0].parameters()).unwrap());
            let [[bu0, bu1], [bv0, bv1]] = ra.faces[0].domain;
            let (uc, vc) = ((bu0 + bu1) / 2.0, (bv0 + bv1) / 2.0);
            let size = (bu1 - bu0).hypot(bv1 - bv0);
            let bump = a.point([uc, vc]).unwrap().dist(b.point([uc, vc]).unwrap());
            let want = (crown * size).abs();
            eprintln!("crown {crown}: the middle moved {bump:.6}, wanted {want:.6}");
            assert!((bump - want).abs() < 1e-6 * size, "middle {bump} against {want}");
        }
        // A planar loop: the exact plane, crowned or not (the crown has no middle to fill).
        let boxed = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let open = copy_faces(
            &boxed,
            &[Vec3::new(5.0, 5.0, 0.0), Vec3::new(5.0, 0.0, 5.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0), Vec3::new(0.0, 5.0, 5.0)],
        )
        .unwrap();
        let rim = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
        let lid = patch_edges_crowned(&open, &rim, 1.0).unwrap();
        assert!(matches!(first_surface(&lid), b::Surface::Plane { .. }), "a planar loop is a plane");
        // Hostile crowns are refused before anything is built.
        for crown in [f64::NAN, f64::INFINITY, 3.01, -0.91, 1e300] {
            let e = patch_edges_crowned(&open, &rim, crown).unwrap_err();
            assert!(e.to_string().contains("crown must be between -0.9 and 3"), "crown {crown}: {e}");
        }
    }
}

#[cfg(test)]
#[path = "fill_general_tests.rs"]
mod general_tests;
