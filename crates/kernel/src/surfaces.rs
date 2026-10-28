//! Surface bodies (open shells) and the operations that turn them into solids: Patch (a face
//! filling a closed loop of edges — planar, or B-spline patches for a loop that is not — or a
//! planar sketch region), Stitch (surfaces sharing edges sewn into one body, a solid when they
//! close), Thicken (a surface offset along its normals into a solid), copying, removing, turning
//! over and taking apart faces of a body, extending a planar surface, and sheets swept from sketch
//! curves (surface extrude, revolve, sweep and loft).
//!
//! Everything works on the body's schema 4 record. Taking faces out of a body, turning them over
//! and making a flat face are exact on the record ([`Parts`] puts any set of faces, edges and
//! vertices into a record's canonical order). Sweeps, stitching and offsets are SMLib's: each piece
//! of a sheet goes to SMLib on its own (a chain of a sweep, a column of a loft, a face of an
//! offset), the pieces' records are joined, and where pieces share edges SMLib stitches them.
//!
//! History is exact. A face taken from a body is that face (Kept), its edges the body's; a sheet's
//! wall comes from the profile segments its edges in the start plane lie along; a stitched or
//! offset face carries the tag of the face it was made from through SMLib.

use std::collections::BTreeSet;

use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{self as b, Brep, Curve2};
use solidcraft_geom::bspline::{HPoint, NurbsSurface};
use solidcraft_geom::{Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, History};
use crate::{KernelError, Result, record};

impl Body {
    /// A surface body: some shell does not close (an edge bounds a face on one side only).
    pub fn is_surface(&self) -> bool {
        !self.is_mesh() && self.record().edges.iter().any(|e| e.uses.len() == 1)
    }
}

fn invalid(what: impl Into<String>) -> KernelError {
    KernelError::Invalid(what.into())
}

fn failed(what: impl Into<String>) -> KernelError {
    KernelError::Failed(what.into())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn a3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| KernelError::TooLarge("a record index".into()))
}

// ---- Records of faces -------------------------------------------------------------------

/// Faces, edges and vertices to be put into one record: each face with whether its shell uses it
/// along its surface's natural normal, its loops naming `edges` and `vertices` by position here.
/// Edges' `uses` are ignored ([`Parts::canonical`] works them out).
#[derive(Clone, Debug, Default)]
pub(crate) struct Parts {
    pub tolerance: f64,
    pub vertices: Vec<b::Vertex>,
    pub edges: Vec<b::Edge>,
    pub faces: Vec<(b::Face, bool)>,
}

/// A record made by [`Parts::canonical`], with where each of its faces and edges came from
/// (`faces[i]`: the part face of record face i; `edges` likewise). Parts no face uses are left out.
pub(crate) struct Canonical {
    pub brep: Brep,
    pub faces: Vec<usize>,
    pub edges: Vec<usize>,
}

/// Where the parts of each record [`Parts::joined`] took begin: faces, edges, vertices.
type Offsets = Vec<[usize; 3]>;

/// The record (by index) and its own index of a part numbered by `offsets` (`kind` 0 for faces, 1
/// for edges).
fn owner(offsets: &Offsets, part: usize, kind: usize) -> Option<(usize, usize)> {
    let k = offsets.iter().rposition(|o| o[kind] <= part)?;
    Some((k, part - offsets.get(k)?[kind]))
}

impl Parts {
    /// Every face of `records` in one set of parts, each record's faces, edges and vertices after
    /// the ones before it.
    pub(crate) fn joined(records: &[&Brep]) -> Result<(Parts, Offsets)> {
        let mut parts = Parts { tolerance: records.iter().map(|r| r.tolerance).fold(0.0, f64::max), ..Parts::default() };
        let mut offsets = Vec::with_capacity(records.len());
        for r in records {
            offsets.push([parts.faces.len(), parts.edges.len(), parts.vertices.len()]);
            let faces: Vec<(usize, bool)> = (0..r.faces.len()).map(|f| (f, false)).collect();
            parts.add(r, &faces)?;
        }
        Ok((parts, offsets))
    }

    /// The faces `picked` of `r` (each with whether to turn it over), on `r`'s own edges and
    /// vertices.
    pub(crate) fn of(r: &Brep, picked: &[(usize, bool)]) -> Result<Parts> {
        let mut parts = Parts { tolerance: r.tolerance, ..Parts::default() };
        parts.add(r, picked)?;
        Ok(parts)
    }

    /// Adds the faces `picked` of `r` with all of `r`'s edges and vertices, after the ones here.
    fn add(&mut self, r: &Brep, picked: &[(usize, bool)]) -> Result<()> {
        let (e0, v0) = (self.edges.len(), self.vertices.len());
        let sides = face_sides(r);
        let shift = |i: u32, by: usize| index(i as usize + by);
        self.vertices.extend(r.vertices.iter().cloned());
        for e in &r.edges {
            let vertices = [shift(e.vertices[0], v0)?, shift(e.vertices[1], v0)?];
            self.edges.push(b::Edge { vertices, uses: Vec::new(), ..e.clone() });
        }
        for &(f, turn) in picked {
            let face = r.faces.get(f).ok_or_else(|| invalid(format!("no face {f}")))?;
            let forward = sides.get(f).copied().unwrap_or(true) != turn;
            let loops = face
                .loops
                .iter()
                .map(|l| {
                    Ok(match l {
                        b::Loop::Edges { edges } => b::Loop::Edges {
                            edges: edges.iter().map(|u| Ok(b::OrientedEdge { edge: shift(u.edge, e0)?, ..u.clone() })).collect::<Result<_>>()?,
                        },
                        b::Loop::Vertex { vertex } => b::Loop::Vertex { vertex: shift(*vertex, v0)? },
                    })
                })
                .collect::<Result<_>>()?;
            self.faces.push((b::Face { loops, ..face.clone() }, forward));
        }
        Ok(())
    }

    /// The edges a face's loops use, in order.
    fn uses_of(f: &b::Face) -> impl Iterator<Item = u32> + '_ {
        f.loops.iter().flat_map(|l| match l {
            b::Loop::Edges { edges } => edges.iter().map(|u| u.edge).collect::<Vec<_>>(),
            b::Loop::Vertex { .. } => Vec::new(),
        })
    }

    /// The faces in shells: faces joined by an edge are in one shell, shells in the order of their
    /// first face, each shell's faces in part order.
    fn shells(&self) -> Result<Vec<Vec<usize>>> {
        let n = self.faces.len();
        let mut root: Vec<usize> = (0..n).collect();
        let find = |root: &[usize], mut i: usize| {
            while let Some(&g) = root.get(i).filter(|g| **g != i) {
                i = g;
            }
            i
        };
        let mut first: Vec<Option<usize>> = vec![None; self.edges.len()];
        for (fi, (f, _)) in self.faces.iter().enumerate() {
            for e in Parts::uses_of(f) {
                let slot = first.get_mut(e as usize).ok_or_else(|| invalid("an edge reference"))?;
                match *slot {
                    None => *slot = Some(fi),
                    Some(other) => {
                        let (a, c) = (find(&root, other), find(&root, fi));
                        if let Some(g) = root.get_mut(a.max(c)) {
                            *g = a.min(c);
                        }
                    }
                }
            }
        }
        let mut shells: Vec<Vec<usize>> = Vec::new();
        let mut shell_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for fi in 0..n {
            let s = *shell_of.entry(find(&root, fi)).or_insert_with(|| {
                shells.push(Vec::new());
                shells.len() - 1
            });
            if let Some(list) = shells.get_mut(s) {
                list.push(fi);
            }
        }
        Ok(shells)
    }

    /// The parts as one record in canonical order (docs/native-brep.md, "Canonical order"): shells
    /// as [`Parts::shells`] groups them, a closed shell enclosing a region of its own; edges in
    /// first appearance walking faces, loops and uses (backwards on a face used against its
    /// natural normal), each edge's uses in that order; vertices in first appearance walking
    /// edges, then vertex loops.
    pub(crate) fn canonical(&self) -> Result<Canonical> {
        if self.faces.is_empty() {
            return Err(invalid("no faces"));
        }
        let shells = self.shells()?;
        // The record as the parts stand (shell k as `shells` groups it), then in canonical order.
        let mut regions = vec![b::Region { shells: Vec::new(), void: false }];
        let mut shell_records = Vec::with_capacity(shells.len());
        for (s, members) in shells.iter().enumerate() {
            let mut count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
            let mut list = Vec::with_capacity(members.len());
            for &fi in members {
                let (f, forward) = self.faces.get(fi).ok_or_else(|| invalid("a face reference"))?;
                for e in Parts::uses_of(f) {
                    *count.entry(e).or_default() += 1;
                }
                list.push(b::OrientedFace { face: index(fi)?, forward: *forward });
            }
            shell_records.push(b::Shell::Faces { faces: list, wires: Vec::new() });
            let s = index(s)?;
            if let Some(infinite) = regions.first_mut() {
                infinite.shells.push(s);
            }
            if count.values().all(|c| *c >= 2) {
                regions.push(b::Region { shells: vec![s], void: false });
            }
        }
        let loose = Brep {
            version: b::VERSION,
            tolerance: self.tolerance,
            regions,
            shells: shell_records,
            faces: self.faces.iter().map(|(f, _)| f.clone()).collect(),
            edges: self.edges.iter().map(|e| b::Edge { uses: Vec::new(), ..e.clone() }).collect(),
            vertices: self.vertices.clone(),
        };
        let part = loose.canonical().map_err(|e| failed(format!("faces into a record: {e}")))?;
        Ok(Canonical {
            brep: part.brep,
            faces: part.faces.iter().map(|f| *f as usize).collect(),
            edges: part.edges.iter().map(|e| *e as usize).collect(),
        })
    }
}

/// Whether each face of `r` is used along its surface's natural normal (by its first oriented
/// face).
fn face_sides(r: &Brep) -> Vec<bool> {
    let mut side = vec![None; r.faces.len()];
    for s in &r.shells {
        if let b::Shell::Faces { faces, .. } = s {
            for f in faces {
                if let Some(slot) = side.get_mut(f.face as usize) {
                    slot.get_or_insert(f.forward);
                }
            }
        }
    }
    side.into_iter().map(|s| s.unwrap_or(true)).collect()
}

/// The history of a record over `operands` operands: its faces' `faces`, each edge's origins as
/// `edge` gives them (from the edge's index, the faces it bounds, ascending, and `faces`).
fn record_history(r: &Brep, operands: usize, faces: Vec<FaceProv>, edge: impl Fn(usize, &[usize], &[FaceProv]) -> Vec<EdgeOrigin>) -> History {
    let between = r.edges.iter().map(|e| e.uses.iter().map(|u| u.face as usize).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>());
    let edges = between.enumerate().map(|(i, between)| EdgeProv { origins: edge(i, &between, &faces), faces: between }).collect();
    History { operands, faces, edges }
}

/// The parameter box of a face's loops: the box of its pcurves' control points (each pcurve lies
/// within its control polygon).
fn uv_box(loops: &[b::Loop]) -> Option<[[f64; 2]; 2]> {
    let mut bx = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for l in loops {
        let b::Loop::Edges { edges } = l else { continue };
        for u in edges {
            let pts: Vec<[f64; 2]> = match &u.pcurve {
                Curve2::Spline { points, .. } => points.clone(),
                Curve2::RationalSpline { homogeneous, .. } => homogeneous.iter().map(|h| [h[0] / h[2], h[1] / h[2]]).collect(),
            };
            for p in pts {
                for (d, x) in bx.iter_mut().zip(p) {
                    d[0] = d[0].min(x);
                    d[1] = d[1].max(x);
                }
            }
        }
    }
    bx.iter().all(|d| d[0].is_finite() && d[1] > d[0]).then_some(bx)
}

/// A body of a record this module built, and whether it numbers its faces and edges as the
/// record does (so a history in the record's order is the body's): the record is the body's own.
fn record_body(r: &Brep) -> Result<(Body, bool)> {
    Ok((Body::from_record(r.clone())?, true))
}

/// `body` with the history `h` over operands of `counts` faces and edges when its order is the
/// record's, else with none.
fn with_history(body: Body, same_order: bool, h: History, counts: &[(usize, usize)]) -> Result<Body> {
    if same_order { body.with_history_counts(h, counts) } else { Ok(body.without_history()) }
}

/// A surface body of the record `r` (faces another builder made) with no history.
pub(crate) fn surface_body(r: &Brep) -> Result<Body> {
    record_body(r).map(|(body, _)| body.without_history())
}

/// `body` (made by an operation on `operands`) with the history that every face and edge is new to
/// it.
pub(crate) fn all_new(body: Body, operands: &[&Body]) -> Result<Body> {
    let r = body.record();
    let faces = vec![record::generated(Vec::new()); r.faces.len()];
    let h = record_history(r, operands.len(), faces, |_, _, _| Vec::new());
    body.with_history_counts(h, &record::counts(operands))
}

/// The faces `picked` of `body` (its record `r`; each with whether to turn it over) as their own
/// body, each the body's face (Kept), every edge the body's.
pub(crate) fn taken(body: &Body, r: &Brep, picked: &[(usize, bool)]) -> Result<Body> {
    let c = Parts::of(r, picked)?.canonical()?;
    let faces = c.faces.iter().map(|p| picked.get(*p).map_or(0, |(f, _)| *f)).map(|f| record::from_face(0, f, Change::Kept)).collect();
    let h = record_history(&c.brep, 1, faces, |e, _, _| c.edges.get(e).map(|&edge| EdgeOrigin::Edge { operand: 0, edge }).into_iter().collect());
    let (result, same) = record_body(&c.brep)?;
    with_history(result, same, h, &record::counts(&[body]))
}

// ---- NURBS faces --------------------------------------------------------------------------

/// A NURBS surface as a record surface (a plain B-spline where every weight is 1).
pub(crate) fn spline_surface(n: &NurbsSurface) -> b::Surface {
    let [nu, nv] = n.count();
    let [ku, kv] = n.knots().clone();
    if n.homogeneous().iter().all(|h| h.w == 1.0) {
        let points = (0..nu).map(|i| (0..nv).map(|j| a3(n.point(i, j).xyz())).collect()).collect();
        return b::Surface::Spline { u_knots: ku, v_knots: kv, points };
    }
    let homogeneous = (0..nu).map(|i| (0..nv).map(|j| n.point(i, j)).map(|h| [h.x, h.y, h.z, h.w]).collect()).collect();
    b::Surface::RationalSpline { u_knots: ku, v_knots: kv, homogeneous }
}

/// The NURBS curve of homogeneous control points `ctrl` on `knots` as a record curve, and its
/// parameter interval.
pub(crate) fn spline_curve(ctrl: &[HPoint], knots: Vec<f64>) -> Result<(b::Curve, [f64; 2])> {
    let degree = knots.len().checked_sub(ctrl.len() + 1).filter(|d| *d >= 1).ok_or_else(|| invalid("a spline's knots"))?;
    let interval = [*knots.get(degree).ok_or_else(|| invalid("knots"))?, *knots.get(ctrl.len()).ok_or_else(|| invalid("knots"))?];
    let curve = if ctrl.iter().all(|h| h.w == 1.0) {
        b::Curve::Spline { knots, points: ctrl.iter().map(|h| a3(h.xyz())).collect() }
    } else {
        b::Curve::RationalSpline { knots, homogeneous: ctrl.iter().map(|h| [h.x, h.y, h.z, h.w]).collect() }
    };
    Ok((curve, interval))
}

/// The pcurve of an edge along a straight line of its face's parameters: from `a` at the
/// interval's start to `z` at its end.
pub(crate) fn uv_line(a: [f64; 2], z: [f64; 2], [t0, t1]: [f64; 2]) -> Curve2 {
    Curve2::Spline { knots: vec![t0, t0, t1, t1], points: vec![a, z] }
}

/// One face: the whole of the NURBS surface `n`, bounded by its four boundary curves (each the
/// boundary row or column of its net on its knots, exact for a clamped surface), as a surface
/// body with no history.
pub(crate) fn tensor_sheet(n: &NurbsSurface) -> Result<Body> {
    let [nu, nv] = n.count();
    if nu < 2 || nv < 2 {
        return Err(invalid("a face needs at least 2 x 2 control points"));
    }
    let [ku, kv] = n.knots().clone();
    let [[u0, u1], [v0, v1]] = n.domain();
    let corner = |i: usize, j: usize| n.point(i, j).project().ok_or_else(|| failed("a corner of the surface has no position"));
    let corners = [corner(0, 0)?, corner(nu - 1, 0)?, corner(nu - 1, nv - 1)?, corner(0, nv - 1)?];
    let mut parts = Parts { tolerance: tolerance_of(corners.into_iter()), ..Parts::default() };
    parts.vertices = corners.iter().map(|p| b::Vertex { point: a3(*p), tolerance: parts.tolerance }).collect();
    let row = |j: usize| (0..nu).map(|i| n.point(i, j)).collect::<Vec<_>>();
    let column = |i: usize| (0..nv).map(|j| n.point(i, j)).collect::<Vec<_>>();
    // Bottom (v0) and top (v1) run along u, left (u0) and right (u1) along v; the loop goes bottom,
    // right, top backwards, left backwards: counter-clockwise in (u, v).
    let sides = [
        (row(0), ku.clone(), [0, 1], [[u0, v0], [u1, v0]], true),
        (column(nu - 1), kv.clone(), [1, 2], [[u1, v0], [u1, v1]], true),
        (row(nv - 1), ku, [3, 2], [[u0, v1], [u1, v1]], false),
        (column(0), kv, [0, 3], [[u0, v0], [u0, v1]], false),
    ];
    let mut uses = Vec::with_capacity(4);
    for (i, (ctrl, knots, [a, z], [pa, pz], forward)) in sides.into_iter().enumerate() {
        let (curve, interval) = spline_curve(&ctrl, knots)?;
        parts.edges.push(b::Edge { nurbs: None, curve, interval, vertices: [index(a)?, index(z)?], tolerance: parts.tolerance, uses: Vec::new() });
        uses.push(b::OrientedEdge { edge: index(i)?, forward, pcurve: uv_line(pa, pz, interval) });
    }
    parts.faces.push((b::Face { nurbs: None, surface: spline_surface(n), domain: n.domain(), loops: vec![b::Loop::Edges { edges: uses }] }, true));
    surface_body(&parts.canonical()?.brep)
}

// ---- Patches ----------------------------------------------------------------------------

/// A planar surface filling a sketch region. Its history has no operands: the face is new.
pub fn patch_region(plane: &Plane, region: &Region2) -> Result<Body> {
    if !crate::build::frame_ok(plane) {
        return Err(invalid("patch frame is not finite and orthonormal"));
    }
    let tolerance = tolerance_of(region.outer.segs.iter().map(|s| plane.to_world(s.start())));
    let mut parts = Parts { tolerance, ..Parts::default() };
    // Seen from the plane's normal the outer loop runs counter-clockwise, holes clockwise.
    let holes = region.holes.iter().map(|h| h.ccw().reversed());
    let loops = std::iter::once(region.outer.ccw()).chain(holes).map(|lp| sketch_loop(&mut parts, plane, &lp.segs)).collect::<Result<Vec<_>>>()?;
    let domain = uv_box(&loops).ok_or_else(|| invalid("the region has no area"))?;
    parts.faces.push((b::Face { nurbs: None, surface: crate::freeform::plane_surface(plane), domain, loops }, true));
    let c = parts.canonical()?;
    let h = record_history(&c.brep, 0, vec![record::generated(Vec::new())], |_, _, _| Vec::new());
    let (body, same) = record_body(&c.brep)?;
    with_history(body, same, h, &[])
}

/// A record tolerance for geometry spanning `points`: 10⁻⁶ of its size, at least 10⁻⁵ mm (the
/// kernel's working tolerance).
fn tolerance_of(points: impl Iterator<Item = Vec3>) -> f64 {
    let mut bx = solidcraft_geom::Aabb3::EMPTY;
    for p in points {
        bx.add(p);
    }
    (bx.diagonal() * 1e-6).max(1e-5)
}

/// One closed loop of sketch segments on `plane` added to `parts` as an edge loop on the plane's
/// surface: one edge per segment, each segment's start a vertex.
fn sketch_loop(parts: &mut Parts, plane: &Plane, segs: &[Seg2]) -> Result<b::Loop> {
    if segs.is_empty() || segs.len() > 100_000 {
        return Err(invalid("a loop needs 1…100000 curves"));
    }
    let v0 = parts.vertices.len();
    for s in segs {
        parts.vertices.push(b::Vertex { point: a3(plane.to_world(s.start())), tolerance: parts.tolerance });
    }
    let mut edges = Vec::with_capacity(segs.len());
    for (i, s) in segs.iter().enumerate() {
        let (curve, pcurve) = crate::freeform::segment(plane, s)?;
        let vertices = [index(v0 + i)?, index(v0 + (i + 1) % segs.len())?];
        let edge = index(parts.edges.len())?;
        parts.edges.push(b::Edge { nurbs: None, curve, interval: crate::freeform::INTERVAL, vertices, tolerance: parts.tolerance, uses: Vec::new() });
        edges.push(b::OrientedEdge { edge, forward: true, pcurve });
    }
    Ok(b::Loop::Edges { edges })
}

/// The edge of a body nearest a point (by samples along each edge), within 5% of the body's size:
/// its index in the body's edge order.
pub(crate) fn edge_near(body: &Body, p: Vec3) -> Option<usize> {
    let r = body.record();
    let mut best: Option<(f64, usize)> = None;
    for (i, e) in r.edges.iter().enumerate() {
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
    best.filter(|(d, _)| *d <= body.size() * 0.05 + 1e-6).map(|(_, e)| e)
}

/// A chain of a body's edges: each edge (its index in the body's edge order) and whether the
/// chain runs along it.
pub(crate) type Chain = Vec<(usize, bool)>;

/// A surface filling a closed loop of a body's edges (each picked by a point on it): one face on
/// the body's own edges, the loop's plane when it is certainly planar, else a B-spline patch
/// ([`crate::fill`], with no crown; loops that do not bound one are refused). Its history is over
/// `[body]`: the face is new, its edges the body's own.
pub fn patch_edges(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("patch")?;
    if at.is_empty() || at.len() > 10_000 {
        return Err(invalid("pick the edges of a closed loop"));
    }
    crate::fill::patch_loop(body, at, 0.0)
}

/// The edges of `body` picked by points on them, chained end to end (each turned to run along the
/// chain), and whether the chain closes: a closed loop starts with the last edge picked; an open
/// chain runs from the free end met first in pick order. Edges that branch, or form more than one
/// chain, are refused.
pub(crate) fn picked_chain(body: &Body, at: &[Vec3]) -> Result<(Chain, bool)> {
    let r = body.record();
    let mut edges: Vec<usize> = Vec::new();
    for p in at {
        let e = edge_near(body, *p).ok_or_else(|| invalid(format!("no edge at {p:?}")))?;
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    let ends = |e: usize| r.edges.get(e).map(|x| x.vertices);
    let mut uses: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for &e in &edges {
        for v in ends(e).ok_or_else(|| invalid("an edge reference"))? {
            *uses.entry(v).or_default() += 1;
        }
    }
    if uses.values().any(|n| *n > 2) {
        return Err(invalid("the edges branch: pick one chain"));
    }
    let open_end = edges.iter().find_map(|&e| ends(e)?.into_iter().find(|v| uses.get(v) == Some(&1)));
    let mut pool = edges;
    let (first, closed) = match open_end {
        Some(v) => {
            let k = pool.iter().position(|&e| ends(e).is_some_and(|x| x.contains(&v))).ok_or_else(|| invalid("no edges"))?;
            let e = pool.remove(k);
            ((e, ends(e).is_some_and(|x| x[0] == v)), false)
        }
        None => ((pool.pop().ok_or_else(|| invalid("no edges"))?, true), true),
    };
    let head = |(e, forward): (usize, bool)| ends(e).map(|x| if forward { x[1] } else { x[0] });
    let start = ends(first.0).map(|x| if first.1 { x[0] } else { x[1] });
    let mut at_v = head(first);
    let mut chain = vec![first];
    while !(closed && at_v == start) {
        let Some(v) = at_v else { break };
        let Some(k) = pool.iter().position(|&e| ends(e).is_some_and(|x| x.contains(&v))) else { break };
        let e = pool.remove(k);
        let link = (e, ends(e).is_some_and(|x| x[0] == v));
        at_v = head(link);
        chain.push(link);
    }
    if !pool.is_empty() {
        let what = if closed { "the edges form more than one loop" } else { "the edges form more than one chain" };
        return Err(invalid(what));
    }
    Ok((chain, closed))
}

// ---- Faces of a body --------------------------------------------------------------------

/// Index of the face nearest a point (over its mesh triangles).
pub(crate) fn nearest_face(body: &Body, p: Vec3) -> Result<usize> {
    let m = body.mesh(&crate::MeshTolerance::chord(body.size() * 2e-3))?;
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let d = ((a + b + c) / 3.0).dist(p).min(a.dist(p)).min(b.dist(p)).min(c.dist(p));
        if best.is_none_or(|(x, _)| d < x) {
            best = Some((d, *f));
        }
    }
    best.map(|(_, f)| f as usize).ok_or_else(|| invalid("no face there"))
}

/// The faces picked by points on them (each once, in the order picked); every face when no point
/// is given.
fn picked_faces(body: &Body, at: &[Vec3]) -> Result<Vec<usize>> {
    if at.len() > 10_000 {
        return Err(invalid("too many faces picked"));
    }
    if at.is_empty() {
        return Ok((0..body.face_count()).collect());
    }
    let mut chosen = Vec::new();
    for p in at {
        let i = nearest_face(body, *p)?;
        if !chosen.contains(&i) {
            chosen.push(i);
        }
    }
    Ok(chosen)
}

/// Faces of a body copied out as a surface body (faces picked by points on them). Its history is
/// over `[body]`: the faces and their edges are the body's.
pub fn copy_faces(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("copy faces")?;
    if at.is_empty() {
        return Err(invalid("pick the faces to copy"));
    }
    let chosen = picked_faces(body, at)?;
    let r = body.record();
    taken(body, r, &chosen.into_iter().map(|f| (f, false)).collect::<Vec<_>>())
}

/// A body's faces taken apart: each picked face (by points on it; every face when none are given)
/// its own surface body, and the faces left, if any, one surface body. A solid comes apart into
/// surfaces. Histories are over `[body]`: each face is the body's.
pub fn unstitch(body: &Body, at: &[Vec3]) -> Result<(Vec<Body>, Option<Body>)> {
    body.require_brep("unstitch")?;
    let r = body.record();
    if r.faces.len() < 2 {
        return Err(invalid("the body has one face: nothing to unstitch"));
    }
    let chosen = picked_faces(body, at)?;
    let pieces = chosen.iter().map(|f| taken(body, r, &[(*f, false)])).collect::<Result<Vec<_>>>()?;
    let rest: Vec<(usize, bool)> = (0..r.faces.len()).filter(|f| !chosen.contains(f)).map(|f| (f, false)).collect();
    let rest = if rest.is_empty() { None } else { Some(taken(body, r, &rest)?) };
    Ok((pieces, rest))
}

/// A surface body with the picked faces (by points on them; every face when none are given) turned
/// over: their normals point the other way. A solid is refused (its faces face out). The history
/// is over `[body]`: each face is the body's, turned over or not.
pub fn reverse_faces(body: &Body, at: &[Vec3]) -> Result<Body> {
    body.require_brep("reverse normal")?;
    if !body.is_surface() {
        return Err(invalid("a solid's faces face out: unstitch it into surfaces first"));
    }
    let chosen = picked_faces(body, at)?;
    let r = body.record();
    taken(body, r, &(0..r.faces.len()).map(|f| (f, chosen.contains(&f))).collect::<Vec<_>>())
}

/// A body with some of its faces (indices in its face order) taken away and the gap left open: a
/// surface body of the faces left. Deleting every face is refused. The history is over `[body]`:
/// each face left is the body's.
pub fn remove_faces(body: &Body, faces: &[usize]) -> Result<Body> {
    body.require_brep("Delete Face")?;
    if faces.is_empty() {
        return Err(invalid("no faces given"));
    }
    let r = body.record();
    let left: Vec<(usize, bool)> = (0..r.faces.len()).filter(|f| !faces.contains(f)).map(|f| (f, false)).collect();
    if left.is_empty() {
        return Err(invalid("that would delete every face of the body"));
    }
    taken(body, r, &left)
}

/// `h`, a history over one record whose faces and edges came from the parts `c` numbers, carried
/// to the records `offsets` took them from: each face origin through `face`, each edge origin to
/// the record edge it was (`None` where an origin has no record behind it).
fn carried(h: &History, c: &Canonical, offsets: &Offsets, operands: usize, face: impl Fn(usize, usize) -> FaceOrigin) -> Option<History> {
    let back = |o: FaceOrigin| -> std::result::Result<FaceOrigin, ()> {
        match o {
            FaceOrigin::Face { face: f, .. } => c.faces.get(f).and_then(|p| owner(offsets, *p, 0)).map(|(k, i)| face(k, i)).ok_or(()),
            other => Ok(other),
        }
    };
    let faces = h
        .faces
        .iter()
        .map(|f| Some(FaceProv { origins: f.origins.iter().map(|o| back(*o).ok()).collect::<Option<_>>()?, change: f.change }))
        .collect::<Option<Vec<_>>>()?;
    let edges = h
        .edges
        .iter()
        .map(|e| {
            let origins = e
                .origins
                .iter()
                .map(|o| match o {
                    EdgeOrigin::Edge { edge, .. } => {
                        let (operand, edge) = c.edges.get(*edge).and_then(|p| owner(offsets, *p, 1))?;
                        Some(EdgeOrigin::Edge { operand, edge })
                    }
                    EdgeOrigin::Meet { a, b } => Some(EdgeOrigin::Meet { a: a.try_map(back).ok()?, b: b.try_map(back).ok()? }),
                })
                .collect::<Option<_>>()?;
            Some(EdgeProv { faces: e.faces.clone(), origins })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(History { operands, faces, edges })
}

/// The joined record `c` with each edge of one input split exactly at every vertex of another
/// input that lies on it within `tol` (away from the edge's own ends), so that a sheet's corner
/// on another's edge is a vertex of both and the sheets sew there (`Brep::split_edges`); `c`'s
/// maps carried through.
fn without_t_junctions(c: Canonical, offsets: &Offsets, tol: f64) -> Result<Canonical> {
    let r = &c.brep;
    let input_of_edge: Vec<Option<usize>> = c.edges.iter().map(|p| owner(offsets, *p, 1).map(|(k, _)| k)).collect();
    let mut input_of_vertex: Vec<Option<usize>> = vec![None; r.vertices.len()];
    for (e, k) in r.edges.iter().zip(&input_of_edge) {
        for v in e.vertices {
            if let Some(slot) = input_of_vertex.get_mut(v as usize) {
                *slot = slot.or(*k);
            }
        }
    }
    let point = |v: u32| r.vertices.get(v as usize).map(|x| Vec3::new(x.point[0], x.point[1], x.point[2]));
    let mut splits: Vec<(u32, f64)> = Vec::new();
    for (e, (edge, ke)) in r.edges.iter().zip(&input_of_edge).enumerate() {
        let Ok(curve) = CurveEval::new(edge.parameters()) else { continue };
        let ends: Vec<Vec3> = edge.vertices.iter().filter_map(|v| point(*v)).collect();
        // Only vertices within a chord's reach of the edge's samples can lie on it.
        let [a, z] = edge.interval;
        let samples: Vec<Vec3> = (0..=SAMPLES).filter_map(|k| curve.point(a + (z - a) * k as f64 / SAMPLES as f64)).collect();
        let chord = samples.windows(2).map(|w| w[0].dist(w[1])).fold(0.0, f64::max);
        for (v, kv) in input_of_vertex.iter().enumerate() {
            let Some(p) = point(index(v)?) else { continue };
            if kv == ke || ends.iter().any(|q| q.dist(p) <= tol) || samples.iter().all(|q| q.dist(p) > chord + tol) {
                continue;
            }
            if let Some(t) = nearest_parameter(&curve, edge.interval, p).filter(|(_, d)| *d <= tol).map(|(t, _)| t) {
                splits.push((index(e)?, t));
            }
        }
    }
    if splits.is_empty() {
        return Ok(c);
    }
    let split = r.split_edges(&splits).map_err(|e| failed(format!("stitch: splitting edges at the other sheets' vertices: {e}")))?;
    let faces =
        split.faces.iter().map(|f| c.faces.get(*f as usize).copied().ok_or_else(|| failed("stitch: a face reference"))).collect::<Result<_>>()?;
    let edges =
        split.edges.iter().map(|e| c.edges.get(*e as usize).copied().ok_or_else(|| failed("stitch: an edge reference"))).collect::<Result<_>>()?;
    Ok(Canonical { brep: split.brep, faces, edges })
}

/// Samples along an edge for finding the points of it near a vertex.
const SAMPLES: usize = 64;

/// The parameter of the curve's point nearest `p` inside `interval`, and the distance: the
/// nearest of evenly spaced samples, then a golden-section search between its neighbours.
fn nearest_parameter(c: &CurveEval, [a, b]: [f64; 2], p: Vec3) -> Option<(f64, f64)> {
    let dist = |t: f64| c.point(t).map_or(f64::INFINITY, |q| q.dist(p));
    let at = |k: usize| a + (b - a) * k as f64 / SAMPLES as f64;
    let best = (0..=SAMPLES).min_by(|&i, &j| dist(at(i)).total_cmp(&dist(at(j))))?;
    let (mut lo, mut hi) = (at(best.saturating_sub(1)), at((best + 1).min(SAMPLES)));
    let ratio = 0.5 * (5f64.sqrt() - 1.0);
    for _ in 0..80 {
        let (x, y) = (hi - ratio * (hi - lo), lo + ratio * (hi - lo));
        if dist(x) < dist(y) {
            hi = y;
        } else {
            lo = x;
        }
    }
    let t = 0.5 * (lo + hi);
    (t > a && t < b).then(|| (t, dist(t)))
}

/// Sew surface bodies along edges that coincide (within `tol`; the bodies' working tolerance when
/// not positive): one body, a solid when the faces close up. SMLib turns the faces so that
/// neighbours agree (outward on a solid). The result's history is over `bodies`: each face the one
/// it was sewn from, each edge the edges sewn into it.
pub fn stitch(bodies: &[&Body], tol: f64) -> Result<Body> {
    if bodies.is_empty() {
        return Err(invalid("nothing to stitch"));
    }
    for b in bodies {
        b.require_brep("stitch")?;
    }
    let size = bodies.iter().map(|b| b.size()).fold(0.0, f64::max);
    let tol = if tol.is_finite() && tol > 0.0 { tol } else { size * 1e-6 + 1e-6 };
    let records: Vec<&Brep> = bodies.iter().map(|b| b.record()).collect();
    let (parts, offsets) = Parts::joined(&records)?;
    let c = without_t_junctions(parts.canonical()?, &offsets, tol)?;
    let (body, h) = imp::stitched(&c.brep, tol)?;
    let h = h.and_then(|h| carried(&h, &c, &offsets, bodies.len(), |operand, face| FaceOrigin::Face { operand, face }));
    Ok(body.with_checked_history(h, &record::counts(bodies)))
}

/// A surface body thickened into a solid: offset along the faces' normals by `thickness`
/// (negative: against them), or by half each way when `symmetric`, by SMLib's thicken
/// ([`crate::offset_backend::thicken`]).
pub fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    body.require_brep("thicken")?;
    imp::thicken(body, thickness, symmetric)
}

/// The faces `picked` of `body` (indices in its face order) offset one by one by SMLib, each onto
/// its surface's offset by `distance` along its normal, and stitched where their offsets meet
/// within `tolerance` (neighbours meeting smoothly stay joined; across a sharp edge they come
/// apart): see [`crate::offset_surface`]. A distance of zero copies the faces. Each face is the
/// offset of the one it came from.
pub(crate) fn offset_faces(body: &Body, picked: &[usize], distance: f64, tolerance: f64) -> Result<Body> {
    let r = body.record();
    if distance == 0.0 {
        return taken(body, r, &picked.iter().map(|f| (*f, false)).collect::<Vec<_>>());
    }
    let moved = picked.iter().map(|f| imp::offset_face(r, *f, distance)).collect::<Result<Vec<_>>>()?;
    let (parts, offsets) = Parts::joined(&moved.iter().collect::<Vec<_>>())?;
    let c = parts.canonical()?;
    let (result, h) = imp::stitched(&c.brep, tolerance.max(c.brep.tolerance))?;
    let offset = |k: usize, _: usize| FaceOrigin::Offset { operand: 0, face: picked.get(k).copied().unwrap_or(0) };
    let faces = h
        .and_then(|h| carried(&h, &c, &offsets, 1, offset))
        .map(|h| h.faces.into_iter().map(|f| FaceProv { change: Change::Generated, ..f }).collect::<Vec<_>>());
    let rec = result.record();
    let h = faces.map(|faces| record_history(rec, 1, faces, |_, between, faces| record::meeting(between, faces)));
    Ok(result.with_checked_history(h, &record::counts(&[body])))
}

/// A planar surface extended: the chosen straight edges (picked by points on them) move outward in
/// the face's plane by `distance`, their neighbours lengthened to meet them. The result's history
/// is over `[body]`: the face is the body's face rebuilt, each edge the one it was moved from.
pub fn extend(body: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    body.require_brep("extend")?;
    if !(distance.is_finite() && distance > 0.0) {
        return Err(invalid("the extension distance must be positive"));
    }
    let r = body.record();
    let [face] = r.faces.as_slice() else { return Err(invalid("not supported yet: extending a surface of more than one face")) };
    let (origin, du, dv) = match face.surface {
        b::Surface::Plane { origin, u_axis, v_axis } => (v3(origin), v3(u_axis), v3(v_axis)),
        b::Surface::PlaneThrough { origin, u_point, v_point } => (v3(origin), v3(u_point) - v3(origin), v3(v_point) - v3(origin)),
        _ => return Err(invalid("not supported yet: extending a surface that is not planar")),
    };
    // The outer loop runs counter-clockwise about the surface's natural normal.
    let n = du.cross(dv).normalized().ok_or_else(|| invalid("a planar face with no normal"))?;
    let [b::Loop::Edges { edges: rim }] = face.loops.as_slice() else { return Err(invalid("not supported yet: extending a surface with holes")) };
    let chosen: Vec<usize> = at.iter().map(|p| edge_near(body, *p).ok_or_else(|| invalid(format!("no edge at {p:?}")))).collect::<Result<_>>()?;
    let m = rim.len();
    // Each edge as a line (point, direction along the loop), moved outward when chosen.
    let mut lines = Vec::with_capacity(m);
    for u in rim {
        let e = r.edges.get(u.edge as usize).ok_or_else(|| invalid("an edge reference"))?;
        let b::Curve::Line { start, end } = e.curve else { return Err(invalid("not supported yet: extending a surface with curved edges")) };
        let at_t = |t: f64| v3(start) + (v3(end) - v3(start)) * t;
        let (a, z) = if u.forward { (at_t(e.interval[0]), at_t(e.interval[1])) } else { (at_t(e.interval[1]), at_t(e.interval[0])) };
        let d = (z - a).normalized().ok_or_else(|| invalid("a zero-length edge"))?;
        // Outward: right of the edge seen along the normal (the face is on its left).
        let shift = if chosen.contains(&(u.edge as usize)) { distance } else { 0.0 };
        lines.push((a + d.cross(n) * shift, d));
    }
    // Corners: where consecutive lines meet.
    let meet = |(p, d): (Vec3, Vec3), (q, e): (Vec3, Vec3)| -> Option<Vec3> {
        let c = d.cross(e);
        let cc = c.dot(c);
        (cc >= 1e-24).then(|| p + d * ((q - p).cross(e).dot(c) / cc))
    };
    let mut parts = Parts { tolerance: r.tolerance, ..Parts::default() };
    for i in 0..m {
        let (Some(prev), Some(cur)) = (lines.get((i + m - 1) % m), lines.get(i)) else { continue };
        parts.vertices.push(b::Vertex { point: a3(meet(*prev, *cur).unwrap_or(cur.0)), tolerance: r.tolerance });
    }
    for i in 0..m {
        let (Some(a), Some(z)) = (parts.vertices.get(i), parts.vertices.get((i + 1) % m)) else { continue };
        let curve = b::Curve::Line { start: a.point, end: z.point };
        parts.edges.push(b::Edge {
            nurbs: None,
            curve,
            interval: [0.0, 1.0],
            vertices: [index(i)?, index((i + 1) % m)?],
            tolerance: r.tolerance,
            uses: Vec::new(),
        });
    }
    let uses = parts
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            Ok(b::pcurve::EdgeUse {
                edge: index(i)?,
                curve: e.parameters(),
                interval: e.interval,
                forward: true,
                tolerance: e.tolerance,
                given: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let surface = b::Surface::Plane { origin: a3(origin), u_axis: a3(du), v_axis: a3(dv) };
    let pcurves = b::pcurve::pcurves(&surface, &[uses], r.tolerance).map_err(|e| failed(format!("extend: {e}")))?;
    let edges = pcurves
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, pcurve)| Ok(b::OrientedEdge { edge: index(i)?, forward: true, pcurve }))
        .collect::<Result<Vec<_>>>()?;
    let loops = vec![b::Loop::Edges { edges }];
    let domain = uv_box(&loops).ok_or_else(|| invalid("the extended face has no area"))?;
    let forward = face_sides(r).first().copied().unwrap_or(true);
    parts.faces.push((b::Face { nurbs: None, surface, domain, loops }, forward));
    let c = parts.canonical()?;
    let h = record_history(&c.brep, 1, vec![record::from_face(0, 0, Change::Modified)], |e, _, _| {
        c.edges.get(e).and_then(|p| rim.get(*p)).map(|u| EdgeOrigin::Edge { operand: 0, edge: u.edge as usize }).into_iter().collect()
    });
    let (result, same) = record_body(&c.brep)?;
    with_history(result, same, h, &record::counts(&[body]))
}

// ---- Sheets swept from sketch curves ------------------------------------------------------

/// Most curve chains one surface sweep takes.
const MAX_CHAINS: usize = 10_000;

/// A sketch segment with its number across all the chains (chain after chain).
type Numbered = (Seg2, usize);

/// The chains' segments, numbered on across the chains.
fn numbered(chains: &[Vec<Seg2>]) -> Vec<Vec<Numbered>> {
    let mut next = 0;
    chains
        .iter()
        .map(|c| {
            c.iter()
                .map(|s| {
                    next += 1;
                    (*s, next - 1)
                })
                .collect()
        })
        .collect()
}

fn check_chains(chains: &[Vec<Seg2>], what: &str) -> Result<()> {
    if chains.is_empty() || chains.len() > MAX_CHAINS {
        return Err(invalid(format!("pick the curves or profiles to {what}")));
    }
    for c in chains {
        if c.is_empty() || c.len() > 4096 {
            return Err(invalid("a curve chain has no curves, or too many"));
        }
        if c.iter().any(|s| !(s.start().is_finite() && s.end().is_finite())) {
            return Err(invalid("a curve chain is not finite"));
        }
        if c.windows(2).any(|w| w[0].end().dist(w[1].start()) > 1e-6) {
            return Err(invalid("a curve chain has a gap: its curves must run end to end"));
        }
    }
    Ok(())
}

/// The faces of the sheet `r` swept from `pieces` that start on `start`: each face comes from the
/// pieces its edges lying in `start` run along (the edges' middles on the pieces' curves), none
/// where no edge does.
fn swept_from(r: &Brep, start: &Plane, pieces: &[Numbered]) -> Vec<FaceProv> {
    let mut from: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); r.faces.len()];
    for e in &r.edges {
        let Some(mid) = CurveEval::new(e.parameters()).ok().and_then(|c| c.point(0.5 * (e.interval[0] + e.interval[1]))) else { continue };
        let near = 4.0 * e.tolerance.max(r.tolerance);
        if start.height(mid).abs() > near {
            continue;
        }
        let o = mid - start.origin;
        let q = Vec2::new(o.dot(start.x), o.dot(start.y));
        for (s, number) in pieces {
            if s.point_at(s.closest_param(q)).dist(q) <= near {
                for u in &e.uses {
                    if let Some(set) = from.get_mut(u.face as usize) {
                        set.insert(*number);
                    }
                }
            }
        }
    }
    from.iter().map(|set| if set.is_empty() { record::generated(Vec::new()) } else { record::profile(set.iter()) }).collect()
}

/// A sheet of SMLib's pieces (each a record with its faces' history), joined into one record and,
/// with `stitch`, sewn where pieces share edges (at ten times the pieces' tolerance).
pub(crate) fn sheet_of(pieces: Vec<(Brep, Vec<FaceProv>)>, stitch: bool, operands: &[&Body]) -> Result<Body> {
    let counts = record::counts(operands);
    let records: Vec<&Brep> = pieces.iter().map(|(r, _)| r).collect();
    let (parts, offsets) = Parts::joined(&records)?;
    let c = parts.canonical()?;
    let prov = |part: usize| owner(&offsets, part, 0).and_then(|(k, i)| pieces.get(k)?.1.get(i).cloned());
    let faces: Option<Vec<FaceProv>> = c.faces.iter().map(|p| prov(*p)).collect();
    if !stitch {
        let (body, same) = record_body(&c.brep)?;
        return Ok(record::attach(body, &counts, faces.filter(|_| same), &[]));
    }
    let (body, h) = imp::stitched(&c.brep, c.brep.tolerance * 10.0)?;
    // A stitched face keeps the tag of the joined record's face it was.
    let sewn = h.zip(faces).and_then(|(h, faces)| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [FaceOrigin::Face { face, .. }] => faces.get(*face).cloned(),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    });
    Ok(record::attach(body, &counts, sewn, &[]))
}

/// A surface extruded from chains of sketch segments on `plane`: each chain (open, or closed such
/// as a profile's boundary) moved straight from `start` to `end` along the plane normal (offsets in
/// mm). The walls it sweeps form one surface body with no caps. Its history has no operands:
/// every face is new, from the segments it was swept from.
pub fn extrude_sheet(plane: &Plane, chains: &[Vec<Seg2>], start: f64, end: f64) -> Result<Body> {
    if !crate::build::frame_ok(plane) {
        return Err(invalid("extrude frame is not finite and orthonormal"));
    }
    if !(start.is_finite() && end.is_finite() && start.abs() < 1e7 && end.abs() < 1e7) {
        return Err(invalid("extrude distance is not finite"));
    }
    let (lo, hi) = if end >= start { (start, end) } else { (end, start) };
    if hi - lo < 1e-6 {
        return Err(invalid("extrude distance is zero"));
    }
    check_chains(chains, "extrude")?;
    let base = plane.offset(lo);
    let n = plane.normal().normalized().ok_or_else(|| invalid("extrude frame has no normal"))?;
    let pieces = numbered(chains)
        .into_iter()
        .map(|chain| {
            let r = imp::extruded(&base, &chain, n, hi - lo)?;
            let h = swept_from(&r, &base, &chain);
            Ok((r, h))
        })
        .collect::<Result<Vec<_>>>()?;
    sheet_of(pieces, false, &[])
}

/// The sheet SMLib revolves from the sketch segments `segs` on `plane` about the axis through
/// `origin` along the unit `axis` (in the plane) by `angle` radians (right-handed, up to a turn),
/// with no caps, as its record: a surface builder's sector.
pub(crate) fn revolved_record(plane: &Plane, segs: &[Seg2], origin: Vec3, axis: Vec3, angle: f64) -> Result<Brep> {
    let chain: Vec<Numbered> = segs.iter().copied().zip(0..).collect();
    imp::revolved(plane, &chain, origin, axis, angle)
}

/// A surface revolved from chains of sketch segments on `plane` about the sketch line through
/// `axis_origin` along `axis_dir`: each chain turned from `start` through `sweep` radians (a full
/// turn at most), positive angles carrying it toward the sketch normal. A chain end on the axis
/// stays put there (a cone's apex, a sphere's pole). The walls form one surface body with no caps,
/// or a solid where they close (a full turn of a closed curve, or of a curve from the axis back to
/// it); its history has no operands: every face is new.
pub fn revolve_sheet(plane: &Plane, chains: &[Vec<Seg2>], axis_origin: Vec2, axis_dir: Vec2, start: f64, sweep: f64) -> Result<Body> {
    use std::f64::consts::TAU;
    if !crate::build::frame_ok(plane) || !axis_origin.is_finite() || !axis_dir.is_finite() {
        return Err(invalid("revolve frame is not finite and orthonormal"));
    }
    if !(start.is_finite() && sweep.is_finite() && start.abs() <= TAU && sweep.abs() <= TAU + 1e-9) {
        return Err(invalid("revolve angles must be finite, a full turn at most"));
    }
    if sweep.abs() < 1e-6 {
        return Err(invalid("revolve angle is zero"));
    }
    check_chains(chains, "revolve")?;
    let d = axis_dir.normalized().ok_or_else(|| invalid("revolve axis has no direction"))?;
    let side = |p: Vec2| d.cross(p - axis_origin);
    let on_axis = |p: Vec2| side(p).abs() < 1e-7;
    let (origin, n) = (plane.to_world(axis_origin), plane.normal());
    let axis = plane.dir_to_world(d).normalized().ok_or_else(|| invalid("revolve frame has no axis"))?;
    let mut pieces = Vec::with_capacity(chains.len());
    for chain in numbered(chains) {
        // On one side of the axis; the point farthest from it fixes the turning sense.
        let pts: Vec<Vec2> = chain.iter().flat_map(|(s, _)| s.polyline(1e-3)).collect();
        if pts.iter().any(|p| side(*p) < -1e-7) && pts.iter().any(|p| side(*p) > 1e-7) {
            return Err(invalid("the curves cross the revolve axis"));
        }
        let far = pts.iter().copied().max_by(|a, c| side(*a).abs().total_cmp(&side(*c).abs())).filter(|p| !on_axis(*p));
        let Some(far) = far else { return Err(invalid("the curves lie on the revolve axis")) };
        let a = if axis.cross(plane.to_world(far) - origin).dot(n) < 0.0 { axis * -1.0 } else { axis };
        let (Some((first, _)), Some((last, _))) = (chain.first(), chain.last()) else { return Err(invalid("an empty curve chain")) };
        if first.start().dist(last.end()) <= 1e-6 && pts.iter().any(|p| on_axis(*p)) {
            return Err(invalid("not supported yet: a closed curve touching the revolve axis"));
        }
        // The chain starts `start` round: its sketch frame turned there.
        let turn = |v: Vec3| crate::build::rotate_about(v, Vec3::ZERO, a, start);
        let turned = Plane { origin: crate::build::rotate_about(plane.origin, origin, a, start), x: turn(plane.x), y: turn(plane.y) };
        let (k, angle) = if sweep < 0.0 { (a * -1.0, -sweep) } else { (a, sweep) };
        let r = imp::revolved(&turned, &chain, origin, k, angle.min(TAU))?;
        let h = swept_from(&r, &turned, &chain);
        pieces.push((r, h));
    }
    sheet_of(pieces, false, &[])
}

/// A surface swept from chains of sketch segments on `plane` along a path that starts near them,
/// turning with it (SMLib's curve sweep: along lines an extrusion, round arcs about one axis a
/// revolution, exactly); at a corner the walls meet in a mitre, as the solid sweep's do. The
/// walls form one surface body with no caps; its history has no operands: every face is new.
pub fn sweep_sheet(plane: &Plane, chains: &[Vec<Seg2>], path: &[crate::PathSeg]) -> Result<Body> {
    sweep_sheet_with(plane, chains, path, &crate::SweepControl::default())
}

/// [`sweep_sheet`] with sweep options: orientation, twist, end scale and guide rails place the
/// chains at stations along the path by [`crate::sweep_path`]'s laws (each section the chain's
/// exact image) for SMLib to skin the walls through.
pub fn sweep_sheet_with(plane: &Plane, chains: &[Vec<Seg2>], path: &[crate::PathSeg], ctl: &crate::SweepControl) -> Result<Body> {
    ctl.check()?;
    if !crate::build::frame_ok(plane) {
        return Err(invalid("sweep frame is not finite and orthonormal"));
    }
    check_chains(chains, "sweep")?;
    if path.is_empty() || path.len() > 1000 {
        return Err(invalid("the path needs 1…1000 segments"));
    }
    let runs = smooth_runs(path)?;
    let controlled = ctl.varies() || matches!(ctl.orientation, crate::SweepOrientation::Direction(_));
    if controlled && runs.len() > 1 {
        return Err(imp::refuse("sweep with a twist, an end scale, guide rails or an orientation direction along a path with corners"));
    }
    let translate = ctl.orientation == crate::SweepOrientation::Parallel;
    let mut pieces = Vec::with_capacity(chains.len());
    for chain in numbered(chains) {
        if runs.len() > 1 {
            pieces.extend(imp::cornered(plane, &chain, path, &runs, translate)?);
            continue;
        }
        let r = if controlled { imp::swept_stations(plane, &chain, path, ctl)? } else { imp::swept(plane, &chain, path, translate)? };
        let h = swept_from(&r, plane, &chain);
        pieces.push((r, h));
    }
    sheet_of(pieces, runs.len() > 1, &[])
}

/// The path's segments in runs along which its direction carries on (within a thousandth of a
/// radian across each joint): a new run starts at each corner.
fn smooth_runs(path: &[crate::PathSeg]) -> Result<Vec<std::ops::Range<usize>>> {
    let ends: Vec<(Vec3, Vec3)> =
        path.iter().map(crate::PathSeg::tangents).collect::<Option<_>>().ok_or_else(|| invalid("a path segment has no direction"))?;
    let mut runs = Vec::new();
    let mut from = 0;
    for (i, w) in ends.windows(2).enumerate() {
        if w[0].1.dot(w[1].0) < (1e-3f64).cos() {
            runs.push(from..i + 1);
            from = i + 1;
        }
    }
    runs.push(from..path.len());
    Ok(runs)
}

/// A surface swept from chains of sketch segments on `plane` along a 3D polyline path: along the
/// lines between its points, as [`sweep_sheet`] sweeps them.
pub fn sweep_sheet_points(plane: &Plane, chains: &[Vec<Seg2>], path: &[Vec3]) -> Result<Body> {
    sweep_sheet_points_with(plane, chains, path, &crate::SweepControl::default())
}

/// [`sweep_sheet_points`] with sweep options (see [`sweep_sheet_with`]).
pub fn sweep_sheet_points_with(plane: &Plane, chains: &[Vec<Seg2>], path: &[Vec3], ctl: &crate::SweepControl) -> Result<Body> {
    let pts = crate::sweep_path::clean(path)?;
    let segs: Vec<crate::PathSeg> = pts.windows(2).map(|w| crate::PathSeg::Line { a: w[0], b: w[1] }).collect();
    sweep_sheet_with(plane, chains, &segs, ctl)
}

/// A surface lofted through two or more sections, each a chain of sketch segments on its plane,
/// all open or all closed: ruled between two sections, smooth through three or more. Closed
/// sections are matched as the solid loft matches them; open ones are split to the same number of
/// segments (longest first) and each turned to run the way the one before it does. Walls only, no
/// caps; the history has no operands: every face is new.
pub fn loft_sheet(sections: &[(Plane, Vec<Seg2>)]) -> Result<Body> {
    loft_sheet_with_rails(sections, &[], crate::RailFit::Scale)
}

/// [`loft_sheet`] following one or two guide rails (polylines from the first section to the last,
/// through every section's curve; see [`crate::loft_rails`]), `fit` saying how they reshape the
/// sections between. Without rails, the plain sheet loft.
///
/// The k-th segments of every section make a column: SMLib lofts each column on its own (one wall
/// from the column's segments), and the columns are stitched along the edges they share.
pub fn loft_sheet_with_rails(sections: &[(Plane, Vec<Seg2>)], rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
    if sections.len() < 2 || sections.len() > 50 {
        return Err(invalid("a loft needs 2…50 sections"));
    }
    let closed: Vec<bool> = sections
        .iter()
        .map(|(_, c)| match (c.first(), c.last()) {
            (Some(f), Some(l)) if c.len() <= 4096 => Ok(l.end().dist(f.start()) <= 1e-6),
            _ => Err(invalid("a loft section has no curves, or too many")),
        })
        .collect::<Result<_>>()?;
    let all_closed = closed.iter().all(|c| *c);
    if !all_closed && closed.iter().any(|c| *c) {
        return Err(invalid("the loft sections must be all open or all closed"));
    }
    for (plane, chain) in sections {
        if !crate::build::frame_ok(plane) {
            return Err(invalid("a loft section's frame is not finite and orthonormal"));
        }
        check_chains(std::slice::from_ref(chain), "loft")?;
    }
    let matched = if all_closed { closed_sections(sections)? } else { open_sections(sections)? };
    let lofted = crate::loft_rails::railed(&matched, all_closed, rails, fit)?;
    let count = matched.first().map_or(0, |s| s.curves.len());
    let pieces = (0..count)
        .map(|k| {
            let column: Vec<&solidcraft_geom::bspline::CurveN<4>> = lofted.iter().filter_map(|s| s.get(k)).collect();
            let r = imp::lofted(&column)?;
            let numbers: BTreeSet<usize> = matched.iter().filter_map(|s| s.numbers.get(k)).flatten().copied().collect();
            let h = vec![record::profile(numbers.iter()); r.faces.len()];
            Ok((r, h))
        })
        .collect::<Result<Vec<_>>>()?;
    sheet_of(pieces, true, &[])
}

/// Closed sections matched as the solid loft matches them ([`crate::build::loft_numbered_loops`]),
/// in loft order.
fn closed_sections(sections: &[(Plane, Vec<Seg2>)]) -> Result<Vec<crate::loft_rails::Section>> {
    let loops: Vec<(Plane, solidcraft_geom::Loop2)> = sections.iter().map(|(p, c)| (*p, solidcraft_geom::Loop2 { segs: c.clone() })).collect();
    let reversed = crate::build::loft_reversed(&loops)?;
    let numbered = loops.iter().map(|(p, _)| *p).zip(crate::numbered::number_loops(loops.iter().map(|(_, l)| l))).collect::<Vec<_>>();
    crate::build::loft_numbered_loops(&numbered, reversed)?
        .iter()
        .map(|(plane, l)| crate::loft_rails::Section::of(plane, &l.lp.segs, (0..l.lp.segs.len()).map(|i| l.numbers(i).to_vec()).collect()))
        .collect()
}

/// Open sections split to the same number of segments (the longest in half until the counts
/// match), each turned to run the way the one before it does.
fn open_sections(sections: &[(Plane, Vec<Seg2>)]) -> Result<Vec<crate::loft_rails::Section>> {
    let n = sections.iter().map(|(_, c)| c.len()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(sections.len());
    let mut prev: Option<(Vec3, Vec3)> = None;
    let mut offset = 0;
    for (plane, chain) in sections {
        // Each piece with the segment it is (part of).
        let mut segs: Vec<(Seg2, usize)> = chain.iter().enumerate().map(|(i, s)| (*s, offset + i)).collect();
        offset += chain.len();
        while segs.len() < n {
            let Some(i) = segs.iter().enumerate().max_by(|a, c| a.1.0.length().total_cmp(&c.1.0.length())).map(|(i, _)| i) else { break };
            let Some(&(s, k)) = segs.get(i) else { break };
            let (a, c) = s.split_half();
            segs.splice(i..=i, [(a, k), (c, k)]);
        }
        let ends = |c: &[(Seg2, usize)]| c.first().zip(c.last()).map(|(f, l)| (plane.to_world(f.0.start()), plane.to_world(l.0.end())));
        if let (Some((ps, pe)), Some((s, e))) = (prev, ends(&segs))
            && s.dist(ps) + e.dist(pe) > s.dist(pe) + e.dist(ps)
        {
            segs = segs.iter().rev().map(|(s, k)| (s.reversed(), *k)).collect();
        }
        prev = ends(&segs);
        let pieces: Vec<Seg2> = segs.iter().map(|(s, _)| *s).collect();
        out.push(crate::loft_rails::Section::of(plane, &pieces, segs.iter().map(|(_, k)| vec![*k]).collect())?);
    }
    Ok(out)
}

/// SMLib's side of this module.
mod imp {
    use solidcraft_smlib::sweeps::{Affine, Curve, Ends, SweepFrame};
    use solidcraft_smlib::{Brep as SmBrep, CheckLevel};

    use super::*;

    fn sm(what: &'static str) -> impl Fn(solidcraft_smlib::Error) -> KernelError {
        move |e| failed(format!("SMLib {what}: {e}"))
    }

    pub(super) fn refuse(what: &str) -> KernelError {
        crate::smlib_sweeps::refuse(what)
    }

    /// A result SMLib's own check accepts, as its record.
    fn checked(r: &SmBrep, what: &'static str) -> Result<Brep> {
        let check = r.check(CheckLevel::All).map_err(sm(what))?;
        if !check.is_valid() {
            return Err(failed(format!("SMLib {what}: SMLib's check refuses the result: {check}")));
        }
        r.to_native().map_err(|e| failed(format!("SMLib {what}: {e}")))
    }

    /// Whether two records of one body number its faces, edges and vertices alike: the same
    /// loops of the same edges, run the same ways, and each edge between the same vertices.
    fn same_numbering(a: &Brep, b: &Brep) -> bool {
        let loops = |f: &b::Face| -> Vec<Vec<(u32, bool)>> {
            f.loops
                .iter()
                .map(|l| match l {
                    b::Loop::Edges { edges } => edges.iter().map(|u| (u.edge, u.forward)).collect(),
                    b::Loop::Vertex { vertex } => vec![(*vertex, true)],
                })
                .collect()
        };
        a.faces.len() == b.faces.len()
            && a.edges.len() == b.edges.len()
            && a.vertices.len() == b.vertices.len()
            && a.faces.iter().zip(&b.faces).all(|(x, y)| loops(x) == loops(y))
            && a.edges.iter().zip(&b.edges).all(|(x, y)| x.vertices == y.vertices)
    }

    /// The record `r` sewn by SMLib at `tol` (`smlib_stitch`), with its history over `[r]` (faces
    /// and edges by their index in `r`); none where SMLib's import of `r` does not number it as
    /// `r` does.
    pub(super) fn stitched(r: &Brep, tol: f64) -> Result<(Body, Option<History>)> {
        let brep = SmBrep::from_record(r, true).map_err(sm("stitch"))?;
        let (back, order) = brep.to_native_ordered().map_err(|e| failed(format!("SMLib stitch: {e}")))?;
        let tagged = brep.tagged(0).map_err(sm("stitch"))?;
        let (sewn, _closed) = tagged.stitch(tol).map_err(sm("stitch"))?;
        checked(&sewn, "stitch")?;
        let (record, kept) = sewn.to_native_ordered().map_err(|e| failed(format!("SMLib stitch: {e}")))?;
        let h = if same_numbering(r, &back) { sewn.history().ok().and_then(|raw| raw.history(&[order], &kept).ok()) } else { None };
        Ok((Body::from_smlib_record(record)?, h))
    }

    pub(super) fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
        crate::offset_backend::thicken(body, thickness, symmetric)
    }

    /// The face `f` of `r` alone, offset by `d` along its normal (`smlib_offset_sheet`).
    pub(super) fn offset_face(r: &Brep, f: usize, d: f64) -> Result<Brep> {
        let one = Parts::of(r, &[(f, false)])?.canonical()?;
        let sheet = SmBrep::from_record(&one.brep, true).map_err(sm("offset"))?;
        checked(&sheet.offset_sheet(d).map_err(sm("offset"))?, "offset")
    }

    fn curves(plane: &Plane, chain: &[Numbered]) -> Result<Vec<Curve>> {
        chain.iter().map(|(s, _)| crate::smlib_sweeps::curve(plane, s)).collect()
    }

    pub(super) fn extruded(base: &Plane, chain: &[Numbered], n: Vec3, distance: f64) -> Result<Brep> {
        checked(
            &SmBrep::extrude(&curves(base, chain)?, a3(n), distance, 0.0, solidcraft_smlib::sweeps::DraftCorner::default(), false)
                .map_err(sm("surface extrude"))?,
            "surface extrude",
        )
    }

    pub(super) fn revolved(plane: &Plane, chain: &[Numbered], origin: Vec3, axis: Vec3, angle: f64) -> Result<Brep> {
        checked(&SmBrep::revolve(&curves(plane, chain)?, a3(origin), a3(axis), angle, false).map_err(sm("surface revolve"))?, "surface revolve")
    }

    fn route(path: &[crate::PathSeg]) -> Result<Vec<Curve>> {
        path.iter().map(crate::smlib_sweeps::path_curve).collect()
    }

    fn frame(translate: bool) -> SweepFrame {
        if translate { SweepFrame::Translate } else { SweepFrame::Follow }
    }

    /// The chain swept along the smooth `path`, turning with it, or only moving along it when
    /// `translate`.
    pub(super) fn swept(plane: &Plane, chain: &[Numbered], path: &[crate::PathSeg], translate: bool) -> Result<Brep> {
        checked(&SmBrep::sweep(&curves(plane, chain)?, &route(path)?, frame(translate), false).map_err(sm("surface sweep"))?, "surface sweep")
    }

    /// `q` moved by the motion `a`.
    fn apply(a: &Affine, q: Vec3) -> Vec3 {
        let col = |i: usize| a.m.get(i).map_or(Vec3::ZERO, |c| v3(*c));
        col(0) * q.x + col(1) * q.y + col(2) * q.z + v3(a.t)
    }

    /// `pl` carried by the rigid motion `a`.
    fn placed(pl: &Plane, a: &Affine) -> Plane {
        let o = apply(a, Vec3::ZERO);
        Plane { origin: apply(a, pl.origin), x: apply(a, pl.x) - o, y: apply(a, pl.y) - o }
    }

    /// The bisector of a corner from direction `t1` to `t2` (unit, the way the path runs) and the
    /// turn there; a corner that turns the path back on itself is refused.
    fn mitre(t1: Vec3, t2: Vec3) -> Result<(Vec3, f64)> {
        let turn = t1.dot(t2).clamp(-1.0, 1.0).acos();
        if turn > std::f64::consts::PI - 1e-3 {
            return Err(invalid("the path turns back on itself at a corner"));
        }
        Ok(((t1 + t2).normalized().ok_or_else(|| invalid("path corner"))?, turn))
    }

    /// A tool of square planar faces, one in each plane `cuts` (a point and its normal), `size`
    /// across: what a run is imprinted with at its corners.
    fn cutting_planes(cuts: &[(Vec3, Vec3)], size: f64) -> Result<SmBrep> {
        let mut parts = Parts { tolerance: (size * 1e-9).max(1e-6), ..Parts::default() };
        let h = size * 0.5;
        let square = [Vec2::new(-h, -h), Vec2::new(h, -h), Vec2::new(h, h), Vec2::new(-h, h)];
        let segs: Vec<Seg2> = (0..4).map(|i| Seg2::Line { a: square[i], b: square[(i + 1) % 4] }).collect();
        for &(at, n) in cuts {
            let u = n.cross(if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }).normalized().ok_or_else(|| invalid("mitre plane"))?;
            let pl = Plane { origin: at, x: u, y: n.cross(u) };
            let lp = sketch_loop(&mut parts, &pl, &segs)?;
            let domain = uv_box(std::slice::from_ref(&lp)).ok_or_else(|| invalid("mitre plane"))?;
            parts.faces.push((b::Face { nurbs: None, surface: crate::freeform::plane_surface(&pl), domain, loops: vec![lp] }, true));
        }
        SmBrep::from_record(&parts.canonical()?.brep, true).map_err(sm("sweep mitre"))
    }

    /// The faces of `sheet` (a run's walls, each from what `prov` says, in its record's order)
    /// imprinted with `cuts` and kept where every cut's `keep` holds of a point inside them: the
    /// piece between a run's mitres, each face from what its uncut face came from.
    fn cut_back(sheet: &SmBrep, prov: &[FaceProv], cuts: &[(Vec3, Vec3)], size: f64) -> Result<(Brep, Vec<FaceProv>)> {
        let (_, order) = sheet.to_native_ordered().map_err(|e| failed(format!("SMLib sweep mitre: {e}")))?;
        let tagged = sheet.tagged(0).map_err(sm("sweep mitre"))?;
        let Some(cut) = tagged.imprint(&cutting_planes(cuts, size)?).map_err(sm("sweep mitre"))? else {
            return Ok((checked(sheet, "sweep mitre")?, prov.to_vec()));
        };
        let (record, kept_order) = cut.to_native_ordered().map_err(|e| failed(format!("SMLib sweep mitre: {e}")))?;
        let h = cut.history().map_err(sm("sweep mitre"))?.history(&[order], &kept_order).map_err(|e| failed(format!("SMLib sweep mitre: {e}")))?;
        // A point inside each face, from SMLib's face order to the record's.
        let points = cut.face_points().map_err(sm("sweep mitre"))?;
        let mut inside = vec![Vec3::ZERO; record.faces.len()];
        for (i, p) in points.iter().enumerate() {
            let k = if kept_order.faces.is_empty() { i } else { kept_order.faces.get(i).copied().unwrap_or(i) };
            if let Some(slot) = inside.get_mut(k) {
                *slot = v3(*p);
            }
        }
        let keep: Vec<(usize, bool)> = (0..record.faces.len())
            .filter(|f| inside.get(*f).is_some_and(|p| cuts.iter().all(|(at, n)| (*p - *at).dot(*n) < 0.0)))
            .map(|f| (f, false))
            .collect();
        let c = Parts::of(&record, &keep)?.canonical()?;
        let faces = c
            .faces
            .iter()
            .map(|p| {
                let f = keep.get(*p).map_or(0, |(f, _)| *f);
                match h.faces.get(f).map(|x| x.origins.as_slice()) {
                    Some([FaceOrigin::Face { face, .. }]) => prov.get(*face).cloned().unwrap_or_else(|| record::generated(Vec::new())),
                    _ => record::generated(Vec::new()),
                }
            })
            .collect();
        Ok((c.brep, faces))
    }

    /// A sheet swept along a path with corners, by the solid sweep's corner model with open ends:
    /// each smooth run swept on its own, turning with the path, carried on straight past each
    /// corner (by the profile's reach times the tangent of half the turn, and a little more) and
    /// cut back to the plane halving the corner, where the next run, cut back the same way, meets
    /// it; the profile turns at a corner about the line square to both directions. Each run's
    /// walls come from the segments they are swept from, found where the run starts; the runs are
    /// stitched by the caller. Only moving along the path (`translate`), the profile is the same at
    /// a corner in both runs, which meet there as they are.
    pub(super) fn cornered(
        plane: &Plane,
        chain: &[Numbered],
        path: &[crate::PathSeg],
        runs: &[std::ops::Range<usize>],
        translate: bool,
    ) -> Result<Vec<(Brep, Vec<FaceProv>)>> {
        let bad = || invalid("a path segment has no direction");
        let start = path.first().map(crate::PathSeg::start).ok_or_else(|| invalid("empty path"))?;
        let end = path.last().map(crate::PathSeg::end).ok_or_else(|| invalid("empty path"))?;
        let reach = chain
            .iter()
            .flat_map(|(s, _)| (0..=8).map(move |i| s.point_at(f64::from(i) / 8.0)))
            .map(|q| plane.to_world(q).dist(start))
            .fold(0.0f64, f64::max);
        let size = 4.0 * (path.iter().map(length).sum::<f64>() + reach) + 1.0;
        let (first_t, _) = path.first().and_then(crate::PathSeg::tangents).ok_or_else(bad)?;
        let (_, last_t) = path.last().and_then(crate::PathSeg::tangents).ok_or_else(bad)?;
        // A closed path ends where it starts; at a corner there the last run meets the first in a
        // mitre too.
        let closing = (end.dist(start) <= 1e-6 && last_t.dot(first_t) < (1e-3f64).cos()).then_some(first_t);
        let over = |turn: f64| if translate { 0.0 } else { reach * (turn * 0.5).tan() * 1.1 + reach * 1e-3 + 1e-6 };
        let mut pose = *plane;
        // The mitre the next run starts from: its point, its normal (along the run) and the turn.
        let mut entry = closing.filter(|_| !translate).map(|t| mitre(last_t, t).map(|(m, turn)| (start, m, turn))).transpose()?;
        let mut pieces = Vec::with_capacity(runs.len());
        for (ri, run) in runs.iter().enumerate() {
            let segs = path.get(run.clone()).ok_or_else(|| invalid("path run"))?;
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(invalid("path run")) };
            let ((t0, _), (_, t1)) = (first.tangents().ok_or_else(bad)?, last.tangents().ok_or_else(bad)?);
            let (a0, b1) = (first.start(), last.end());
            let next = match runs.get(ri + 1).and_then(|r| path.get(r.start)) {
                Some(s) => Some(s.tangents().ok_or_else(bad)?.0),
                None => closing,
            };
            let exit = next.filter(|_| !translate).map(|t2| mitre(t1, t2)).transpose()?;
            let pre = entry.map_or(0.0, |(_, _, turn)| over(turn));
            let post = exit.map_or(0.0, |(_, turn)| over(turn));
            let mut along = Vec::with_capacity(segs.len() + 2);
            if pre > 0.0 {
                along.push(crate::PathSeg::Line { a: a0 - t0 * pre, b: a0 });
            }
            along.extend_from_slice(segs);
            if post > 0.0 {
                along.push(crate::PathSeg::Line { a: b1, b: b1 + t1 * post });
            }
            let from = Plane { origin: pose.origin - t0 * pre, ..pose };
            let (sheet, [_, _, at_last]) =
                SmBrep::sweep_placed(&curves(&from, chain)?, &route(&along)?, frame(translate), false).map_err(sm("surface sweep"))?;
            let prov = swept_from(&sheet.to_native().map_err(|e| failed(format!("SMLib surface sweep: {e}")))?, &from, chain);
            // Kept: after the entry mitre, before the exit one.
            let cuts: Vec<(Vec3, Vec3)> = entry.map(|(at, m, _)| (at, m * -1.0)).into_iter().chain(exit.map(|(m, _)| (b1, m))).collect();
            pieces.push(cut_back(&sheet, &prov, &cuts, size)?);
            // The profile at the run's end, turned through the corner onto the next run.
            let at_end = placed(&from, &at_last);
            pose = Plane { origin: at_end.origin - t1 * post, ..at_end };
            entry = None;
            if let (Some(t2), Some((m, turn))) = (next, exit) {
                let k = t1.cross(t2).normalized().ok_or_else(|| invalid("path corner"))?;
                let turn_v = |v: Vec3| crate::build::rotate_about(v, Vec3::ZERO, k, turn);
                pose = Plane { origin: crate::build::rotate_about(pose.origin, b1, k, turn), x: turn_v(pose.x), y: turn_v(pose.y) };
                entry = Some((b1, m, turn));
            }
        }
        Ok(pieces)
    }

    /// Fewest and most stations of a sweep that twists, scales or follows rails.
    const MIN_STATIONS: usize = 16;
    const MAX_STATIONS: usize = 2048;

    /// The chain carried through stations along `path` placed by [`crate::sweep_path`]'s laws for
    /// `ctl` (a station per degree the path and the twist turn), SMLib skinning its walls through
    /// the sections with open ends (`smlib_sweep_stations`).
    pub(super) fn swept_stations(plane: &Plane, chain: &[Numbered], path: &[crate::PathSeg], ctl: &crate::SweepControl) -> Result<Brep> {
        let turn: f64 = path.iter().map(turn_of).sum::<f64>() + ctl.twist.abs();
        let rail_points = ctl.rails.iter().map(Vec::len).max().unwrap_or(0);
        let n = ((turn / 1f64.to_radians()).ceil() as usize).max(rail_points.saturating_sub(1)).clamp(MIN_STATIONS, MAX_STATIONS);
        let (pts, tangents) = samples(path, n)?;
        let st = crate::sweep_path::controlled_stations(plane, &pts, &tangents, &[], ctl)?;
        let s0 = st.first().map(|s| s.at).ok_or_else(|| invalid("sweep path"))?;
        let maps: Vec<Affine> = st
            .iter()
            .map(|s| {
                let [a, b, c] = s.m;
                Affine { m: [a3(a), a3(b), a3(c)], t: a3(s.at - s.apply(s0)) }
            })
            .collect();
        checked(&SmBrep::sweep_stations(&curves(plane, chain)?, &maps, Ends::Open).map_err(sm("surface sweep"))?, "surface sweep")
    }

    /// Samples of a segment's parameter its turn and length are read from.
    const TURN_SAMPLES: usize = 64;

    /// How far a path segment turns (radians): its tangent's turn through even steps of its
    /// parameter.
    fn turn_of(s: &crate::PathSeg) -> f64 {
        match *s {
            crate::PathSeg::Line { .. } => 0.0,
            crate::PathSeg::Arc { angle, .. } => angle.abs(),
            _ => (0..=TURN_SAMPLES)
                .filter_map(|i| at(s, i as f64 / TURN_SAMPLES as f64).map(|(_, t)| t))
                .collect::<Vec<_>>()
                .windows(2)
                .map(|w| w[0].cross(w[1]).len().atan2(w[0].dot(w[1])))
                .sum(),
        }
    }

    /// `n + 1` points spread along `path` (evenly by length from segment to segment, evenly in each
    /// segment's parameter within it), with the path's unit tangent at each.
    fn samples(path: &[crate::PathSeg], n: usize) -> Result<(Vec<Vec3>, Vec<Vec3>)> {
        let bad = || invalid("a path segment has no direction");
        let lengths: Vec<f64> = path.iter().map(length).collect();
        let total: f64 = lengths.iter().sum();
        if !(total > 1e-12) {
            return Err(invalid("the sweep path has no length"));
        }
        let (mut pts, mut tangents) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
        for i in 0..=n {
            let mut s = total * i as f64 / n as f64;
            let mut k = 0;
            while k + 1 < path.len() && s > lengths.get(k).copied().unwrap_or(0.0) {
                s -= lengths.get(k).copied().unwrap_or(0.0);
                k += 1;
            }
            let f = (s / lengths.get(k).copied().unwrap_or(1.0).max(1e-300)).clamp(0.0, 1.0);
            let (p, t) = path.get(k).and_then(|seg| at(seg, f)).ok_or_else(bad)?;
            pts.push(p);
            tangents.push(t);
        }
        Ok((pts, tangents))
    }

    /// A path segment's length (an arc's exactly; a free-form one's by its spans' own lengths).
    fn length(s: &crate::PathSeg) -> f64 {
        match s {
            crate::PathSeg::Line { a, b } => a.dist(*b),
            crate::PathSeg::Arc { a, center, angle, .. } => a.dist(*center) * angle.abs(),
            crate::PathSeg::Curve { seg, .. } => seg.length(),
            crate::PathSeg::Bezier(b) => b.length().unwrap_or(0.0),
        }
    }

    /// The point `f` (0…1) of the way along a segment's parameter and its unit tangent there.
    fn at(s: &crate::PathSeg, f: f64) -> Option<(Vec3, Vec3)> {
        match s {
            crate::PathSeg::Line { a, b } => Some((a.lerp(*b, f), (*b - *a).normalized()?)),
            crate::PathSeg::Arc { a, center, axis, angle } => {
                let k = axis.normalized()?;
                let p = crate::build::rotate_about(*a, *center, k, angle * f);
                Some((p, (k.cross(p - *center) * angle.signum()).normalized()?))
            }
            crate::PathSeg::Curve { plane, seg } => Some((plane.to_world(seg.point_at(f)), plane.dir_to_world(seg.derivative(f)).normalized()?)),
            crate::PathSeg::Bezier(b) => Some((b.point(f)?, b.derivative(f)?.normalized()?)),
        }
    }

    /// One column of a loft: its curve in each section (and station) in loft order, skinned by
    /// SMLib with no caps (`smlib_loft`), ruled between two, cubic through four or more.
    pub(super) fn lofted(column: &[&solidcraft_geom::bspline::CurveN<4>]) -> Result<Brep> {
        let sections: Vec<Vec<Curve>> = column
            .iter()
            .map(|c| {
                let points = c.ctrl.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3], h[3]]).collect();
                vec![Curve::Nurbs { degree: c.degree, points, knots: c.knots.clone() }]
            })
            .collect();
        let degree = u32::try_from(column.len().saturating_sub(1).clamp(1, 3)).unwrap_or(3);
        checked(&SmBrep::loft(&sections, degree, false).map_err(sm("surface loft"))?, "surface loft")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure, trim};
    use solidcraft_geom::Loop2;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// Walls swept from an open line, an open arc and a closed square: areas are length × height,
    /// no caps, and stitching two patches onto the square's walls closes the box.
    #[test]
    fn extrude_sheet_of_open_and_closed_chains() {
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let s = extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, 5.0).unwrap();
        assert!(s.is_surface() && s.face_count() == 1);
        assert!(rel(measure(&s).unwrap().area, 50.0) < 1e-9);
        // A quarter arc of radius 4, swept from -1 to 2: area r θ h.
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 4.0, start: 0.0, sweep: std::f64::consts::FRAC_PI_2 }];
        let a = measure(&extrude_sheet(&Plane::XY, &[arc], 2.0, -1.0).unwrap()).unwrap().area;
        let want = 4.0 * std::f64::consts::FRAC_PI_2 * 3.0;
        assert!(rel(a, want) < 1e-3, "{a} vs {want}"); // (measured on meshes)
        // A line then a line back up: one chain, two faces.
        let bent = vec![line[0], Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 6.0) }];
        assert_eq!(extrude_sheet(&Plane::XY, &[bent], 0.0, 1.0).unwrap().face_count(), 2);
        // A closed square: four walls, open at both ends; with a patch on each end, a solid box.
        let sq = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let walls = extrude_sheet(&Plane::XY, std::slice::from_ref(&sq.outer.segs), 0.0, 4.0).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4);
        assert!(rel(measure(&walls).unwrap().area, 160.0) < 1e-9);
        let (bottom, top) = (patch_region(&Plane::XY, &sq).unwrap(), patch_region(&Plane::XY.offset(4.0), &sq).unwrap());
        let closed = stitch(&[&walls, &bottom, &top], 1e-6).unwrap();
        assert!(!closed.is_surface());
        assert!(rel(measure(&closed).unwrap().volume, 400.0) < 1e-9);
    }

    /// Sheets revolved about the sketch's y axis, against their analytic areas: a cylinder (and a
    /// quarter of one, started a quarter turn round), cones with the apex at either end, a sphere
    /// from a half circle, and a square's ring (Pappus: 2π × centroid distance × length).
    #[test]
    fn revolve_sheet_areas() {
        use std::f64::consts::{FRAC_PI_2, PI};
        let sheet = |chain: Vec<Seg2>, start: f64, sweep: f64| {
            revolve_sheet(&Plane::XY, &[chain], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), start, sweep).unwrap()
        };
        let rev = |chain: Vec<Seg2>, start: f64, sweep: f64| measure(&sheet(chain, start, sweep)).unwrap().area;
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        let close = |a: f64, want: f64| assert!(rel(a, want) < 2e-3, "{a} vs {want}"); // (measured on meshes)
        close(rev(vec![line((3.0, 0.0), (3.0, 4.0))], 0.0, 2.0 * PI), 2.0 * PI * 3.0 * 4.0);
        close(rev(vec![line((3.0, 0.0), (3.0, 4.0))], FRAC_PI_2, FRAC_PI_2), FRAC_PI_2 * 3.0 * 4.0);
        close(rev(vec![line((0.0, 0.0), (3.0, 4.0))], 0.0, 2.0 * PI), PI * 3.0 * 5.0);
        close(rev(vec![line((3.0, 4.0), (0.0, 0.0))], 0.0, 2.0 * PI), PI * 3.0 * 5.0);
        assert!(sheet(vec![line((3.0, 0.0), (3.0, 4.0))], 0.0, 1.0).is_surface());
        // Closing shells are solids, outward: a sphere from a half circle, a square's ring.
        let half = Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 2.0, start: -FRAC_PI_2, sweep: PI };
        let ball = measure(&sheet(vec![half], 0.0, 2.0 * PI)).unwrap();
        close(ball.area, 4.0 * PI * 4.0);
        close(ball.volume, 4.0 / 3.0 * PI * 8.0);
        assert!(sheet(vec![half], 0.0, PI).is_surface());
        let sq = Loop2::polygon(&[Vec2::new(2.0, 0.0), Vec2::new(3.0, 0.0), Vec2::new(3.0, 1.0), Vec2::new(2.0, 1.0)]);
        let ring = measure(&sheet(sq.segs.clone(), 0.0, 2.0 * PI)).unwrap();
        close(ring.area, 2.0 * PI * (2.0 + 3.0 + 2.5 + 2.5));
        close(ring.volume, 2.0 * PI * 2.5);
    }

    #[test]
    fn revolve_sheet_refuses_bad_input() {
        let rev = |chain: Vec<Seg2>, sweep: f64| revolve_sheet(&Plane::XY, &[chain], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 0.0, sweep);
        let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
        assert!(rev(vec![line((-1.0, 0.0), (1.0, 1.0))], 1.0).is_err(), "crosses the axis");
        assert!(rev(vec![line((0.0, 0.0), (0.0, 1.0))], 1.0).is_err(), "on the axis");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], 0.0).is_err(), "zero angle");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], 7.0).is_err(), "more than a turn");
        assert!(rev(vec![line((1.0, 0.0), (1.0, 1.0))], f64::NAN).is_err());
        let touching = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(0.0, 1.0)]);
        assert!(rev(touching.segs, 1.0).is_err(), "a closed curve touching the axis");
        assert!(revolve_sheet(&Plane::XY, &[], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 0.0, 1.0).is_err());
        assert!(revolve_sheet(&Plane::XY, &[vec![line((1.0, 0.0), (1.0, 1.0))]], Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.0), 0.0, 1.0).is_err());
    }

    /// A line along x swept up z: straight (length × height), then on round a quarter turn
    /// (a quarter cylinder of radius 5 about an axis along x: 10 × 5π/2, the arc taking up the
    /// line's far edge), and along a path with a corner (mitred ruled walls, 10 × both legs); a
    /// closed square's four walls; a 3D point path.
    #[test]
    fn sweep_sheet_areas() {
        use crate::PathSeg;
        use std::f64::consts::FRAC_PI_2;
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let up = PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 5.0) };
        let area = |b: &Body| measure(b).unwrap().area;
        let straight = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up]).unwrap();
        assert!(straight.is_surface() && rel(area(&straight), 50.0) < 1e-9);
        let bend = PathSeg::Arc { a: Vec3::new(0.0, 0.0, 5.0), center: Vec3::new(0.0, 5.0, 5.0), axis: -Vec3::X, angle: FRAC_PI_2 };
        let smooth = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up, bend]).unwrap();
        assert!(smooth.is_surface());
        let want = 50.0 + 10.0 * 5.0 * FRAC_PI_2;
        assert!(rel(area(&smooth), want) < 2e-3, "{} vs {want}", area(&smooth)); // (measured on meshes)
        let over = PathSeg::Line { a: Vec3::new(0.0, 0.0, 5.0), b: Vec3::new(0.0, 5.0, 5.0) };
        let cornered = sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[up, over]).unwrap();
        assert!(cornered.is_surface() && rel(area(&cornered), 100.0) < 1e-9, "{}", area(&cornered));
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0), Vec2::new(0.0, 4.0)]);
        let walls = sweep_sheet(&Plane::XY, std::slice::from_ref(&sq.segs), &[up]).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4 && rel(area(&walls), 80.0) < 1e-9);
        let pts: Vec<Vec3> = (0..=10).map(|i| Vec3::new(0.0, 0.0, i as f64)).collect();
        assert!(rel(area(&sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &pts).unwrap()), 100.0) < 1e-6);
        // A smooth point path: a quarter circle of radius 30 in the YZ plane. One smooth wall (not
        // forty ruled strips), a piece of cylinder: 10 × 15π.
        let arc: Vec<Vec3> = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.0 * FRAC_PI_2;
                Vec3::new(0.0, 30.0 * (1.0 - t.cos()), 30.0 * t.sin())
            })
            .collect();
        let curved = sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &arc).unwrap();
        assert_eq!(curved.face_count(), 1, "smooth wall");
        let want = 10.0 * 30.0 * FRAC_PI_2;
        assert!(rel(area(&curved), want) < 2e-3, "{} vs {want}", area(&curved)); // (measured on meshes)
        // A bent chain along the same path: two smooth walls joined by one shared seam edge.
        let bent = vec![line[0], Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 5.0) }];
        let two = sweep_sheet_points(&Plane::XY, &[bent], &arc).unwrap();
        assert_eq!(two.face_count(), 2);
        assert_eq!(two.edge_count(), 7, "the walls share their seam");
    }

    #[test]
    fn sweep_sheet_refuses_bad_input() {
        use crate::PathSeg;
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let up = PathSeg::Line { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 5.0) };
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[]).is_err());
        assert!(sweep_sheet(&Plane::XY, &[], &[up]).is_err());
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[PathSeg::Line { a: Vec3::ZERO, b: Vec3::ZERO }]).is_err());
        let no_axis = PathSeg::Arc { a: Vec3::ZERO, center: Vec3::new(0.0, 5.0, 0.0), axis: Vec3::ZERO, angle: 1.0 };
        assert!(sweep_sheet(&Plane::XY, std::slice::from_ref(&line), &[no_axis]).is_err());
        assert!(sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &[Vec3::ZERO]).is_err());
        assert!(sweep_sheet_points(&Plane::XY, std::slice::from_ref(&line), &[Vec3::ZERO, Vec3::new(f64::NAN, 0.0, 0.0)]).is_err());
    }

    /// Sheets lofted between open lines and closed loops: a ruled strip (even when the second
    /// line runs the other way, or is in two pieces), a smooth strip through three, the walls
    /// between two squares, and a cylinder through three circles.
    #[test]
    fn loft_sheet_areas() {
        use std::f64::consts::PI;
        let at = |z: f64| Plane::XY.offset(z);
        let line = |a: f64, b: f64| Seg2::Line { a: Vec2::new(a, 0.0), b: Vec2::new(b, 0.0) };
        let area = |b: &Body| measure(b).unwrap().area;
        let strip = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 10.0)])]).unwrap();
        assert!(strip.is_surface() && rel(area(&strip), 50.0) < 1e-6, "{}", area(&strip));
        let turned = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(10.0, 0.0)])]).unwrap();
        assert!(rel(area(&turned), 50.0) < 1e-6, "not twisted: {}", area(&turned));
        let pieces = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 5.0), line(5.0, 10.0)])]).unwrap();
        assert!(rel(area(&pieces), 50.0) < 1e-6 && pieces.face_count() == 2, "{}", area(&pieces));
        let three = loft_sheet(&[(at(0.0), vec![line(0.0, 10.0)]), (at(5.0), vec![line(0.0, 10.0)]), (at(10.0), vec![line(0.0, 10.0)])]).unwrap();
        assert!(rel(area(&three), 100.0) < 1e-3, "{}", area(&three)); // (measured on meshes)
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]);
        let walls = loft_sheet(&[(at(0.0), sq.segs.clone()), (at(4.0), sq.segs.clone())]).unwrap();
        assert!(walls.is_surface() && walls.face_count() == 4 && rel(area(&walls), 160.0) < 1e-6, "{}", area(&walls));
        let circle = Loop2::circle(Vec2::ZERO, 2.0).segs;
        let tube = loft_sheet(&[(at(0.0), circle.clone()), (at(5.0), circle.clone()), (at(10.0), circle)]).unwrap();
        assert!(tube.is_surface());
        let want = 2.0 * PI * 2.0 * 10.0;
        assert!(rel(area(&tube), want) < 2e-3, "{} vs {want}", area(&tube)); // (measured on meshes)
    }

    #[test]
    fn loft_sheet_refuses_bad_input() {
        let at = |z: f64| Plane::XY.offset(z);
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 1.0)]).segs;
        assert!(loft_sheet(&[(at(0.0), line.clone())]).is_err(), "one section");
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), sq)]).is_err(), "open and closed");
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), vec![])]).is_err(), "an empty section");
        let gap = vec![line[0], Seg2::Line { a: Vec2::new(11.0, 0.0), b: Vec2::new(12.0, 0.0) }];
        assert!(loft_sheet(&[(at(0.0), line.clone()), (at(1.0), gap)]).is_err(), "a section with a gap");
        let nan = vec![Seg2::Line { a: Vec2::new(f64::NAN, 0.0), b: Vec2::new(1.0, 0.0) }];
        assert!(loft_sheet(&[(at(0.0), line), (at(1.0), nan)]).is_err());
    }

    #[test]
    fn extrude_sheet_refuses_bad_input() {
        let line = vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, 0.0).is_err());
        assert!(extrude_sheet(&Plane::XY, std::slice::from_ref(&line), 0.0, f64::NAN).is_err());
        assert!(extrude_sheet(&Plane::XY, &[], 0.0, 1.0).is_err());
        assert!(extrude_sheet(&Plane::XY, &[vec![]], 0.0, 1.0).is_err());
        let gap = vec![line[0], Seg2::Line { a: Vec2::new(11.0, 0.0), b: Vec2::new(12.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, &[gap], 0.0, 1.0).is_err());
        let nan = vec![Seg2::Line { a: Vec2::new(0.0, f64::NAN), b: Vec2::new(1.0, 0.0) }];
        assert!(extrude_sheet(&Plane::XY, &[nan], 0.0, 1.0).is_err());
    }

    /// A box taken apart into its six faces (each a 10 × 10 surface) and stitched back into
    /// the same solid; two picked faces come off on their own, the other four stay together;
    /// a one-face body has nothing to take apart.
    #[test]
    fn unstitch_a_box_and_stitch_it_back() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let (pieces, rest) = unstitch(&b, &[]).unwrap();
        assert!(rest.is_none() && pieces.len() == 6);
        for p in &pieces {
            assert!(p.is_surface() && p.face_count() == 1 && rel(measure(p).unwrap().area, 100.0) < 1e-9);
        }
        let back = stitch(&pieces.iter().collect::<Vec<_>>(), 1e-6).unwrap();
        assert!(!back.is_surface() && rel(measure(&back).unwrap().volume, 1000.0) < 1e-9);
        let (two, rest) = unstitch(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 5.0, 0.0)]).unwrap();
        let rest = rest.unwrap();
        assert!(two.len() == 2 && rest.is_surface() && rest.face_count() == 4);
        assert!(unstitch(&two[0], &[]).is_err());
    }

    /// A patch thickened along its normal grows up; turned over, the same thickness grows down.
    /// A solid's faces are not turned over.
    #[test]
    fn reverse_faces_turns_the_thicken_side() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let up = measure(&thicken(&s, 2.0, false).unwrap()).unwrap().bbox;
        let turned = reverse_faces(&s, &[]).unwrap();
        assert!(turned.is_surface() && turned.face_count() == 1);
        let down = measure(&thicken(&turned, 2.0, false).unwrap()).unwrap().bbox;
        assert!((up.min.z - down.min.z - 2.0).abs() < 1e-9 && (up.max.z - down.max.z - 2.0).abs() < 1e-9, "{up:?} vs {down:?}");
        assert!(reverse_faces(&box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap(), &[]).is_err());
    }

    /// A box's top taken away without healing: a five-face surface; patching the opening and
    /// stitching closes the same box. Every face, or none, is refused.
    #[test]
    fn remove_faces_leaves_the_gap_open() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let top = nearest_face(&b, Vec3::new(5.0, 5.0, 10.0)).unwrap();
        let open = remove_faces(&b, &[top]).unwrap();
        assert!(open.is_surface() && open.face_count() == 5);
        assert!(rel(measure(&open).unwrap().area, 500.0) < 1e-9);
        let lid = patch_edges(&open, &[Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)])
            .unwrap();
        let back = stitch(&[&open, &lid], 1e-6).unwrap();
        assert!(!back.is_surface() && rel(measure(&back).unwrap().volume, 1000.0) < 1e-9);
        assert!(remove_faces(&b, &(0..6).collect::<Vec<_>>()).is_err());
        assert!(remove_faces(&b, &[]).is_err());
    }

    /// Faces that are not planes or cylinders thicken onto fitted surfaces. A sixth of a sphere
    /// (one of its six patches) thickened out by 1 is a sixth of the shell between radii 10 and
    /// 11; a torus patch (a symmetric share of the torus, by its area) thickened out by 1 is that
    /// share of the shell 2π²R((r+1)² − r²); thickened in past the sphere's centre, refused.
    #[test]
    fn thicken_curved_faces_onto_fitted_surfaces() {
        use crate::{sphere, torus};
        use std::f64::consts::PI;
        let ball = sphere(Vec3::ZERO, 10.0).unwrap();
        let patch = copy_faces(&ball, &[Vec3::new(0.0, 0.0, 10.0)]).unwrap();
        assert!(rel(measure(&patch).unwrap().area, 4.0 * PI * 100.0 / 6.0) < 2e-3, "one sixth");
        let shell = thicken(&patch, 1.0, false).unwrap();
        assert!(!shell.is_surface());
        let want = 4.0 / 3.0 * PI * (1331.0 - 1000.0) / 6.0;
        let v = measure(&shell).unwrap().volume;
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
        assert!(thicken(&patch, -10.0, false).is_err());
        // A torus patch (R 20, r 5): between the tube radii r and r + 1 over its own angles, θ round
        // the tube and φ round the axis (read off its mesh): the volume is
        // Δφ·[R((r+1)² − r²)/2·Δθ + ((r+1)³ − r³)/3·(sin θ1 − sin θ0)].
        let ring = torus(Vec3::ZERO, 20.0, 5.0).unwrap();
        let face = copy_faces(&ring, &[Vec3::new(25.0, 0.0, 0.0)]).unwrap();
        let mesh = face.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        let angles: Vec<(f64, f64)> = mesh.positions.iter().map(|p| (p.z.atan2((p.x * p.x + p.y * p.y).sqrt() - 20.0), p.y.atan2(p.x))).collect();
        let span = |f: &dyn Fn(&(f64, f64)) -> f64| {
            let (lo, hi) = angles.iter().map(f).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
            (lo, hi)
        };
        let ((t0, t1), (p0, p1)) = (span(&|a| a.0), span(&|a| a.1));
        assert!(t1 - t0 < PI && p1 - p0 < PI, "a patch within half a turn each way: {t0}..{t1}, {p0}..{p1}");
        let (r, rr, big) = (5.0f64, 6.0f64, 20.0);
        let want = (p1 - p0) * (big * (rr * rr - r * r) / 2.0 * (t1 - t0) + (rr.powi(3) - r.powi(3)) / 3.0 * (t1.sin() - t0.sin()));
        let v = measure(&thicken(&face, 1.0, false).unwrap()).unwrap().volume;
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
    }

    /// Three faces of a box meeting at a corner (sharp edges), thickened out by 1: three slabs,
    /// a wedge along each sharp edge and a corner tetrahedron, 3·100 + 3·(½·1²·10) + 1/6; a closed
    /// solid that every check accepts. Thickened into the inside corner, refused.
    #[test]
    fn thicken_a_box_corner_across_its_sharp_edges() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let corner = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(10.0, 5.0, 5.0), Vec3::new(5.0, 10.0, 5.0)]).unwrap();
        assert!(corner.is_surface() && corner.face_count() == 3);
        let t = thicken(&corner, 1.0, false).unwrap();
        assert!(!t.is_surface());
        let want = 300.0 + 15.0 + 1.0 / 6.0;
        let v = measure(&t).unwrap().volume;
        assert!(rel(v, want) < 1e-9, "{v} vs {want}");
        assert!(t.validity().is_empty(), "{:?}", t.validity());
        assert!(thicken(&corner, -1.0, false).is_err());
        assert!(thicken(&corner, 1.0, true).is_err(), "half of it goes into the inside corner");
    }

    /// A saddle B-spline patch thickened by 1.5 holds the volume its offsets sweep: by Steiner,
    /// the offset's area is quadratic in the distance, so Simpson's rule over sampled offsets
    /// (Offset Surface at 0, ¾ and 1.5) is exact.
    #[test]
    fn thicken_a_bspline_patch_by_its_sampled_offsets() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = crate::cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = [Vec3::new(14.0, 0.0, z(14.0)), Vec3::new(0.0, 14.0, z(0.0)), Vec3::new(-14.0, 0.0, z(-14.0)), Vec3::new(0.0, -14.0, z(0.0))];
        let cover = patch_edges(&cup, &rim).unwrap();
        let t = 1.5;
        let area = |d: f64| measure(&crate::offset_surface(&cover, &[0], d, 0.01).unwrap()).unwrap().area;
        let want = t / 6.0 * (area(0.0) + 4.0 * area(t / 2.0) + area(t));
        let v = measure(&thicken(&cover, t, false).unwrap()).unwrap().volume;
        assert!(rel(v, want) < 2e-3, "{v} vs {want}"); // (measured on meshes)
    }

    /// A cone frustum's side (radii 10 and 5, 10 high) thickened out by 1: a cone has no Gaussian
    /// curvature, so its offset's area grows linearly and the volume is t·π·L·(r1 + r2 + t·cos β),
    /// L the slant length and β the angle between the normal and the radius (cos β = 10/L).
    #[test]
    fn thicken_a_cone_frustum_side() {
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(5.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let xz = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let frustum = crate::revolve(&xz, &[region], Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.0), 2.0 * std::f64::consts::PI).unwrap().remove(0);
        // The side comes in several pieces round the axis: picked all the way round.
        let round: Vec<Vec3> =
            (0..8).map(|k| f64::from(k) * std::f64::consts::FRAC_PI_4).map(|a| Vec3::new(7.5 * a.cos(), 7.5 * a.sin(), 5.0)).collect();
        let side = copy_faces(&frustum, &round).unwrap();
        let l = 125f64.sqrt();
        assert!(rel(measure(&side).unwrap().area, std::f64::consts::PI * 15.0 * l) < 2e-3, "the whole side");
        let v = measure(&thicken(&side, 1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * l * (15.0 + 10.0 / l);
        assert!(rel(v, want) < 3e-3, "{v} vs {want}"); // (measured on meshes)
    }

    #[test]
    fn patch_and_thicken_a_square() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        assert!(s.is_surface());
        let t = thicken(&s, 2.0, false).unwrap();
        assert!(!t.is_surface());
        assert!(rel(measure(&t).unwrap().volume, 200.0) < 1e-9);
        let t = thicken(&s, 2.0, true).unwrap();
        let m = measure(&t).unwrap();
        assert!(rel(m.volume, 200.0) < 1e-9 && (m.bbox.min.z + 1.0).abs() < 1e-9, "{:?}", m.bbox);
    }

    #[test]
    fn thicken_a_cylinder_side_and_a_box_top() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        // One half of the side, offset outward by 1: half of a ring.
        let half = copy_faces(&c, &[Vec3::new(7.0, 7.0, 10.0)]).unwrap();
        let v = measure(&thicken(&half, 1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (121.0 - 100.0) / 2.0 * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Both halves (meeting smoothly): the whole ring, inward.
        let d = 10.0 / 2f64.sqrt();
        let side = copy_faces(&c, &[Vec3::new(d, d, 10.0), Vec3::new(-d, -d, 10.0)]).unwrap();
        assert_eq!(side.face_count(), 2);
        let v = measure(&thicken(&side, -1.0, false).unwrap()).unwrap().volume;
        let want = std::f64::consts::PI * (100.0 - 81.0) * 20.0;
        assert!(rel(v, want) < 3e-4, "{v} vs {want}"); // (measured on meshes)
        // Two faces meeting at a sharp edge: two slabs and the wedge closing the gap between
        // them, its ends patched: 2·100 + ½·1²·10.
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let two = copy_faces(&b, &[Vec3::new(5.0, 5.0, 10.0), Vec3::new(5.0, 0.0, 5.0)]).unwrap();
        let v = measure(&thicken(&two, 1.0, false).unwrap()).unwrap().volume;
        assert!(rel(v, 205.0) < 1e-9, "{v}");
    }

    #[test]
    fn stitch_six_faces_into_a_box_and_patch_a_hole() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let pts = [
            Vec3::new(5.0, 10.0, 0.0),
            Vec3::new(5.0, 10.0, 30.0),
            Vec3::new(0.0, 10.0, 15.0),
            Vec3::new(10.0, 10.0, 15.0),
            Vec3::new(5.0, 0.0, 15.0),
            Vec3::new(5.0, 20.0, 15.0),
        ];
        let parts: Vec<Body> = pts.iter().map(|p| copy_faces(&b, &[*p]).unwrap()).collect();
        let refs: Vec<&Body> = parts.iter().collect();
        let s = stitch(&refs, 0.0).unwrap();
        assert!(!s.is_surface());
        assert!(rel(measure(&s).unwrap().volume, 6000.0) < 1e-9);
        // Five faces leave an open box; patching the top's edges closes it again.
        let open = stitch(&[refs[0], refs[2], refs[3], refs[4], refs[5]], 0.0).unwrap();
        assert!(open.is_surface());
        let lid =
            patch_edges(&open, &[Vec3::new(5.0, 0.0, 30.0), Vec3::new(10.0, 10.0, 30.0), Vec3::new(5.0, 20.0, 30.0), Vec3::new(0.0, 10.0, 30.0)])
                .unwrap();
        let closed = stitch(&[&open, &lid], 0.0).unwrap();
        assert!(!closed.is_surface());
        assert!(rel(measure(&closed).unwrap().volume, 6000.0) < 1e-9);
    }

    #[test]
    fn extend_a_planar_surface() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 150.0) < 1e-9);
        let e = extend(&s, &[Vec3::new(10.0, 5.0, 0.0), Vec3::new(5.0, 10.0, 0.0)], 5.0).unwrap();
        assert!(rel(measure(&e).unwrap().area, 225.0) < 1e-9);
        assert!(extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], -1.0).is_err());
    }

    #[test]
    fn trim_keeps_one_side() {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        let s = patch_region(&Plane::XY, &r).unwrap();
        let pl = Plane::new(Vec3::new(3.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(1.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 30.0) < 1e-9);
        let t = trim(&s, &crate::SplitTool::Plane(pl), Vec3::new(8.0, 5.0, 0.0)).unwrap();
        assert!(rel(measure(&t).unwrap().area, 70.0) < 1e-9);
    }
}
