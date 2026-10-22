//! SolidCraft's mesher: each face of a schema 4 record ([`brep::Brep`]) triangulated in its own
//! parameters, with every edge sampled once so neighbouring faces share their boundary points
//! exactly, handed over as a [`TessPlan`] for the CPU or GPU evaluator. A boundary node's
//! parameters on each face come from that face's exact pcurve of the edge, at the node's curve
//! parameter: nothing is projected, and a seam's two sides are its two pcurves.
//!
//! Per face:
//! 1. a cheaper layout where the face suits one: planar faces whose boundary runs along their
//!    parameter box, and curved ones, take a structured grid zipped to the edge nodes; a
//!    four-cornered face whose sides are not parameter lines (mitred fillet strips, cross hole
//!    walls) a Coons blend of its four sides; a trimmed face the patches of its cuts or the
//!    iso-line grid cut along its motorcycle traces;
//! 2. otherwise, the general method for every face: constrained Delaunay in the face's (u, v),
//!    with interior seeds and Ruppert refinement, every node then evaluated on the exact
//!    surface (and refined where a triangle sags off it);
//! 3. a face even that refuses (its loops cross or do not close in (u, v), or wind round a
//!    seam or pole), or whose pcurves do not lie on its edges, is an error naming the face (and
//!    the edge) and why. Its cause is upstream, in the record's pcurves or seams, and is fixed
//!    there; there is no other mesher to fall back to.

pub(crate) mod cdt;
pub(crate) mod collar;
pub(crate) mod coons;
pub(crate) mod cutgrid;
pub(crate) mod edges;
pub(crate) mod grid;
pub(crate) mod motorcycle;
pub(crate) mod pieces;
pub(crate) mod sag;
pub(crate) mod seam;
pub(crate) mod size;
pub(crate) mod surface;
pub(crate) mod uv;

use std::collections::HashMap;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::{self, Curve, Loop, Shell};
use solidcraft_geom::tess::{EvalSurface, FacePlan, TessPlan};

use crate::{KernelError, Result};

/// How the faces of a plan were meshed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlanStats {
    pub planar: usize,
    pub structured: usize,
    pub delaunay: usize,
}

/// Curved faces from which their sizes are worked out on the worker pool.
const PARALLEL_SIZES: usize = 8;
/// Curved faces from which faces are planned on the worker pool (measured on the oracle
/// parts: two curved faces plan faster serially, four break even, six run twice as fast).
const PARALLEL_FACES: usize = 4;
/// Intersection curves (a Newton solve per point), or edges of any kind, from which edges are
/// sampled on the worker pool.
const PARALLEL_CURVES: usize = 2;
const PARALLEL_EDGES: usize = 256;

/// Most lines a curved face's grid gets in one direction: a turn's worth for each of the many
/// turns a coil's or a thread's wall winds (the grid as a whole stays within
/// [`grid::MAX_FACE_GRID`]).
const MAX_LINES: usize = 8192;
/// Most points Ruppert refinement may add to one face, beyond its seeds.
const MAX_REFINE: usize = 20_000;

fn unsupported(what: impl std::fmt::Display) -> KernelError {
    KernelError::Failed(format!("not supported yet: tessellation: {what}"))
}

/// The surface kind of a face, for an error that names it.
fn kind(s: &brep::Surface) -> &'static str {
    match s {
        brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. } => "plane",
        brep::Surface::Cylinder { .. } => "cylinder",
        brep::Surface::Cone { .. } => "cone",
        brep::Surface::Sphere { .. } => "sphere",
        brep::Surface::Torus { .. } => "torus",
        brep::Surface::Spline { .. } => "spline",
        brep::Surface::RationalSpline { .. } => "rational spline",
        brep::Surface::PlacedRevolution { .. } => "revolution",
    }
}

/// Whether each face's normal is its surface's natural normal, as the shell that uses it says
/// (a face no shell lists keeps its surface's).
fn forward_faces(b: &brep::Brep) -> Vec<bool> {
    let mut forward = vec![true; b.faces.len()];
    for shell in &b.shells {
        if let Shell::Faces { faces, .. } = shell {
            for f in faces {
                if let Some(slot) = forward.get_mut(f.face as usize) {
                    *slot = f.forward;
                }
            }
        }
    }
    forward
}

/// The edges of each face's loops, in loop order.
fn face_edges(f: &brep::Face) -> impl Iterator<Item = &brep::OrientedEdge> {
    f.loops.iter().flat_map(|l| match l {
        Loop::Edges { edges } => edges.as_slice(),
        Loop::Vertex { .. } => &[],
    })
}

/// Plan the tessellation of `b` with chord tolerance `tol` and, when given, a facet angle no
/// facet may turn more than (where it sags above the angle's floor). Faces and edges are the
/// record's, in its order.
pub(crate) fn plan(b: &brep::Brep, tol: f64, turn: Option<size::Turn>) -> Result<(TessPlan, PlanStats)> {
    let faces = &b.faces;
    let forward = forward_faces(b);
    // The faces each edge bounds, each once, in the order of the edge's uses.
    let edge_faces: Vec<Vec<usize>> = b
        .edges
        .iter()
        .map(|e| {
            let mut fs: Vec<usize> = Vec::with_capacity(e.uses.len());
            for u in &e.uses {
                if !fs.contains(&(u.face as usize)) {
                    fs.push(u.face as usize);
                }
            }
            fs
        })
        .collect();
    let rules = size::Rules::new(tol, size::extent(b.vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2]))), turn);
    // Each face's surface and target side length, then every edge once at the smaller size of
    // its faces; faces then read their nodes. A planar face costs microseconds and a curved
    // one about a hundred, so the pool is used only where curved faces (or intersection
    // curves) make up the work.
    let evals: Vec<(EvalSurface, surface::Params)> = faces
        .iter()
        .enumerate()
        .map(|(fi, f)| surface::eval_surface(&f.surface, f.domain).map_err(|e| unsupported(format!("face {fi} ({}): {e}", kind(&f.surface)))))
        .collect::<Result<_>>()?;
    let (evals, params): (Vec<EvalSurface>, Vec<surface::Params>) = evals.into_iter().unzip();
    let curved = evals.iter().filter(|e| !matches!(e, EvalSurface::Plane { .. })).count();
    let sizes: Vec<(f64, [f64; 2])> =
        crate::par::map("face sizes", &evals, curved >= PARALLEL_SIZES, |s| Ok(size::face_size(s, &uv::domain(s), &rules)))
            .into_iter()
            .collect::<Result<_>>()?;
    let face_h = |f: usize| sizes.get(f).map_or(rules.max_len, |(h, _)| *h);
    let edge_h: Vec<f64> = edge_faces.iter().map(|fs| fs.iter().map(|&f| face_h(f)).fold(rules.max_len, f64::min)).collect();
    let jobs: Vec<(usize, f64)> = edge_h.into_iter().enumerate().collect();
    let solved = b.edges.iter().filter(|e| matches!(e.curve, Curve::SurfaceIntersection { .. })).count();
    let mut nodes: Vec<edges::Nodes> = crate::par::map("edge nodes", &jobs, solved >= PARALLEL_CURVES || jobs.len() >= PARALLEL_EDGES, |(e, h)| {
        edges::sample_edge(b, *e, &rules, *h)
    })
    .into_iter()
    .collect::<Result<_>>()?;
    relax_crowded_faces(b, &jobs, &rules, &mut nodes)?;
    let indexed: Vec<usize> = (0..faces.len()).collect();
    let planned = crate::par::map("face tessellation", &indexed, curved >= PARALLEL_FACES, |&fi| {
        let (Some(face), Some(eval), Some(map), Some(size)) = (faces.get(fi), evals.get(fi), params.get(fi), sizes.get(fi)) else {
            return Err(unsupported(format!("face {fi}: missing")));
        };
        let loops = boundary(b, fi, face, eval, map, &nodes, &rules)?;
        let mut stats = PlanStats::default();
        let fp = plan_face(fi, kind(&face.surface), eval, &loops, &rules, *size, &mut stats)?;
        let fwd = forward.get(fi).copied().unwrap_or(true);
        let fp =
            FacePlan { flip: !fwd, triangles: if fwd { fp.triangles } else { fp.triangles.into_iter().map(|[a, b, c]| [a, c, b]).collect() }, ..fp };
        Ok((fp, stats))
    });
    let mut plan = TessPlan::default();
    let mut stats = PlanStats::default();
    for (r, eval) in planned.into_iter().zip(evals) {
        let (fp, s) = r?;
        let surface = u32::try_from(plan.surfaces.len()).map_err(|_| unsupported("too many faces"))?;
        plan.surfaces.push(eval);
        plan.faces.push(FacePlan { surface: Some(surface), ..fp });
        stats.planar += s.planar;
        stats.structured += s.structured;
        stats.delaunay += s.delaunay;
    }
    plan.edges = nodes.into_iter().map(|n| n.points).collect();
    plan.edge_faces = edge_faces.into_iter().map(|fs| fs.into_iter().map(|f| u32::try_from(f).unwrap_or(u32::MAX)).collect()).collect();
    Ok((plan, stats))
}

/// Face `fi`'s boundary, loop by loop: the shared edge nodes (each edge's last node is the next
/// edge's first) and their parameters on the face's surface, from each edge use's exact pcurve
/// at the node's curve parameter. A seam edge's two uses on one face have their own pcurves, so
/// its two sides are told apart. A pcurve runs on continuously; where one edge meets the next, a
/// pcurve that starts a whole period along from where the previous one ended is moved back by
/// it, except in a direction the meeting point is a pole of: there the parameter is free, and
/// the two pcurves' own values are where the face's boundary runs along the pole from and to
/// (a meridian revolved half a turn meets the other at the pole half a period away; unwrapped,
/// its loop would wind round the surface). Each loop then starts at a node off every pole whose
/// predecessor is off them too, so the step from its last node to its first, which
/// [`seam::drift`] takes as the nearest copy, never crosses a pole. A vertex loop (a pole)
/// bounds no area and has no nodes of its own. A pcurve that does not cover its edge's interval, or puts a
/// node farther off the edge than the tolerances allow, is an error naming the face and edge:
/// the record is wrong there.
fn boundary(
    b: &brep::Brep,
    fi: usize,
    face: &brep::Face,
    s: &EvalSurface,
    map: &surface::Params,
    nodes: &[edges::Nodes],
    rules: &size::Rules,
) -> Result<Vec<seam::Loop>> {
    let dom = uv::domain(s);
    let fail = |e: u32, why: String| unsupported(format!("face {fi} ({}): edge {e}: {why}", kind(&face.surface)));
    let mut loops = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let Loop::Edges { edges } = l else { continue };
        let (mut pts, mut uvs): (Vec<Vec3>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
        // Where the previous edge ended, and the periods its pcurve was moved by.
        let (mut end, mut shift): (Option<[f64; 2]>, [f64; 2]) = (None, [0.0; 2]);
        for oe in edges {
            let n = nodes.get(oe.edge as usize).ok_or_else(|| fail(oe.edge, "no nodes".into()))?;
            let edge = b.edges.get(oe.edge as usize).ok_or_else(|| fail(oe.edge, "no such edge".into()))?;
            let [t0, t1] = edge.interval;
            let [p0, p1] = oe.pcurve.interval().ok_or_else(|| fail(oe.edge, "a malformed pcurve".into()))?;
            let slack = 1e-9 * (t1 - t0).abs().max(t0.abs()).max(t1.abs()).max(1.0);
            if (p0 - t0).abs() > slack || (p1 - t1).abs() > slack {
                return Err(fail(oe.edge, format!("its pcurve spans [{p0}, {p1}], the edge [{t0}, {t1}]")));
            }
            // The edge's own tolerance: how far its curve may stand off the face.
            let accept = (rules.tol * 10.0).max(1e-6) + edge.tolerance.max(0.0);
            let mut ep: Vec<Vec3> = n.points.clone();
            let mut eq: Vec<[f64; 2]> = Vec::with_capacity(ep.len());
            for (k, (t, p)) in n.params.iter().zip(&n.points).enumerate() {
                let q = oe.pcurve.eval(t.clamp(p0, p1)).ok_or_else(|| fail(oe.edge, format!("its pcurve does not evaluate at t = {t}")))?;
                let q = map.to_eval(q);
                let on =
                    s.derivs(q[0], q[1]).map(|d| d.0).ok_or_else(|| fail(oe.edge, format!("node {k}: the surface does not evaluate at {q:?}")))?;
                if !(on.dist(*p) <= accept) {
                    return Err(fail(oe.edge, format!("node {k}: its pcurve is {} mm off the edge at t = {t}", on.dist(*p))));
                }
                eq.push(q);
            }
            if !oe.forward {
                ep.reverse();
                eq.reverse();
            }
            if let (Some(e), Some(&start)) = (end, eq.first()) {
                for d in 0..2 {
                    let Some(per) = dom.period[d] else { continue };
                    if !(uv::degenerate(s, e, d) || uv::degenerate(s, start, d)) {
                        shift[d] = per * ((e[d] - start[d]) / per).round();
                    }
                }
            }
            for q in &mut eq {
                q[0] += shift[0];
                q[1] += shift[1];
            }
            end = eq.last().copied();
            ep.pop();
            eq.pop();
            pts.extend(ep);
            uvs.extend(eq);
        }
        let n = uvs.len();
        let pole = |q: &[f64; 2]| (0..2).any(|d| uv::degenerate(s, *q, d));
        if let Some(k) = (0..n).find(|&k| uvs.get(k).is_some_and(|q| !pole(q)) && uvs.get((k + n - 1) % n).is_some_and(|q| !pole(q))) {
            uvs.rotate_left(k);
            pts.rotate_left(k);
        }
        if pts.len() >= 2 {
            loops.push(seam::Loop { uv: uvs, pts });
        }
    }
    Ok(loops)
}

/// Bounded work for the facet angle: a face whose boundary would get more than
/// [`size::MAX_ANGLE_BOUNDARY`] segments has its curved edges sampled again at twice the angle,
/// as often as it takes, down to the tolerance alone (a plate with a thousand small holes is
/// not worth a hundred times the work to draw each hole round). An edge keeps one set of nodes
/// for both its faces, at the coarser of their levels, so every rim of a crowded face gets the
/// same share whatever the order of the faces.
fn relax_crowded_faces(b: &brep::Brep, jobs: &[(usize, f64)], rules: &size::Rules, nodes: &mut [edges::Nodes]) -> Result<()> {
    let Some(turn) = rules.turn else { return Ok(()) };
    let boundaries: Vec<Vec<usize>> = b.faces.iter().map(|f| face_edges(f).map(|oe| oe.edge as usize).collect()).collect();
    let curved = |e: usize| b.edges.get(e).is_some_and(|e| !matches!(e.curve, Curve::Line { .. }));
    let mut level: Vec<u8> = vec![0; nodes.len()];
    for l in 1..=size::MAX_RELAX {
        let segments = |e: &usize| nodes.get(*e).map_or(0, |n| n.points.len().saturating_sub(1));
        let mut crowded: Vec<usize> = boundaries
            .iter()
            .filter(|es| es.iter().map(segments).sum::<usize>() > size::MAX_ANGLE_BOUNDARY)
            .flatten()
            .filter(|e| level.get(**e).copied().unwrap_or(0) < l && curved(**e))
            .copied()
            .collect();
        crowded.sort_unstable();
        crowded.dedup();
        if crowded.is_empty() {
            break;
        }
        let relaxed = size::Rules { turn: turn.relaxed(l), ..*rules };
        for e in crowded {
            let Some(&(_, h)) = jobs.get(e) else { continue };
            if let Some(slot) = nodes.get_mut(e) {
                *slot = edges::sample_edge(b, e, &relaxed, h)?;
            }
            if let Some(slot) = level.get_mut(e) {
                *slot = l;
            }
        }
    }
    Ok(())
}

/// Twice the signed area of a parameter polygon.
fn area2(uv: &[[f64; 2]]) -> f64 {
    let n = uv.len();
    (0..n).map(|k| uv.get(k).zip(uv.get((k + 1) % n)).map_or(0.0, |(a, b)| a[0] * b[1] - b[0] * a[1])).sum()
}

/// A loop through both poles that turns straight back at one of them (it runs along a seam
/// and back: a whole sphere bounded by its meridian, or a sphere face whose split meridian it
/// uses both ways) retraces that seam once projected, since the periodic parameter carries on
/// unchanged across the pole. Across a pole it may jump by any amount, so the run from the
/// second pole back to the first is moved one period along, whichever way keeps the loop
/// within one period with a positive area: the seam's two uses then lie a period apart.
fn open_pole_to_pole(s: &EvalSurface, dom: &uv::Domain, l: &mut seam::Loop) {
    let n = l.uv.len();
    let poles: Vec<usize> = (0..n).filter(|&k| l.uv.get(k).is_some_and(|q| (0..2).any(|d| uv::degenerate(s, *q, d)))).collect();
    let [p0, p1] = poles.as_slice() else { return };
    for d in 0..2 {
        let Some(per) = dom.period[d] else { continue };
        let near =
            |a: Option<&[f64; 2]>, b: Option<&[f64; 2]>| a.zip(b).is_some_and(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1e-9 * per);
        let turns_back = [*p0, *p1].iter().any(|&p| near(l.uv.get((p + n - 1) % n), l.uv.get((p + 1) % n)));
        if !turns_back {
            continue;
        }
        // Along the seam the periodic parameter is constant between the poles.
        let second: Vec<usize> = (0..n).filter(|k| (k > p1 || k < p0) && *k != *p0 && *k != *p1).collect();
        if second.is_empty() {
            continue;
        }
        for sign in [1.0, -1.0] {
            let mut moved = l.uv.clone();
            for &k in &second {
                if let Some(q) = moved.get_mut(k) {
                    q[d] += sign * per;
                }
            }
            // The poles as the planner splits them: one copy at each neighbour's parameter.
            let mut ring = Vec::with_capacity(n + 2);
            for k in 0..n {
                let (Some(q), Some(a), Some(b)) = (moved.get(k), moved.get((k + n - 1) % n), moved.get((k + 1) % n)) else { continue };
                if k == *p0 || k == *p1 {
                    let (mut qa, mut qb) = (*q, *q);
                    qa[d] = a[d];
                    qb[d] = b[d];
                    ring.extend([qa, qb]);
                } else {
                    ring.push(*q);
                }
            }
            let (lo, hi) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[d]), b.max(q[d])));
            if hi - lo <= per * (1.0 + 1e-9) && area2(&ring) > 0.0 {
                l.uv = moved;
                return;
            }
        }
    }
}

/// The poles of a loop where it turns straight back along a seam: the nodes either side have
/// the same parameters.
fn turning_poles(s: &EvalSurface, per: f64, uv: &[[f64; 2]]) -> Vec<usize> {
    let n = uv.len();
    let near = |a: Option<&[f64; 2]>, b: Option<&[f64; 2]>| a.zip(b).is_some_and(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1e-9 * per);
    (0..n)
        .filter(|&k| uv.get(k).is_some_and(|q| (0..2).any(|d| uv::degenerate(s, *q, d))))
        .filter(|&k| near(uv.get((k + n - 1) % n), uv.get((k + 1) % n)))
        .collect()
}

/// A loop with one pole, where it turns straight back along a seam, that winds once around the
/// periodic direction as projected: a half sphere's rim, then its seam down to the pole and up
/// again. Unwrapped along the loop, both runs of the seam lie on one parameter line, and the
/// period the loop is short of is closed by the rim's jump back. The face lies between the
/// seam's two uses a period apart, so the loop is unwrapped from the node after the pole round
/// to the pole, and starts at the node before it: the seam's uses then bound the face's parameter box on either side, and share
/// their nodes as every other edge does. Kept as it was unless the opened loop spans at most a
/// period with a positive area.
fn open_at_pole(s: &EvalSurface, dom: &uv::Domain, l: &mut seam::Loop) {
    let n = l.uv.len();
    if n < 4 || l.pts.len() != n {
        return;
    }
    for d in 0..2 {
        let Some(per) = dom.period[d] else { continue };
        if (seam::drift(l, d, per).abs() - per).abs() > per * 1e-6 {
            continue;
        }
        let [pole] = turning_poles(s, per, &l.uv)[..] else { continue };
        // From the node after the pole round to the pole, unwrapped along the way.
        let order: Vec<usize> = (1..=n).map(|i| (pole + i) % n).collect();
        let mut uv: Vec<[f64; 2]> = Vec::with_capacity(n);
        for &k in &order {
            let Some(mut q) = l.uv.get(k).copied() else { return };
            if let Some(pr) = uv.last() {
                for e in 0..2 {
                    q[e] = uv::unwrap(q[e], pr[e], dom.period[e]);
                }
            }
            uv.push(q);
        }
        // The pole's free parameter: its predecessor's (the poles are split from it later).
        if let (Some(prev), Some(q)) = (uv.len().checked_sub(2).and_then(|i| uv.get(i)).copied(), uv.last_mut()) {
            q[d] = prev[d];
        }
        let (lo, hi) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[d]), b.max(q[d])));
        // Started at the node before the pole, so the loop closes without a jump.
        let mut opened = seam::Loop { uv, pts: order.iter().filter_map(|&k| l.pts.get(k).copied()).collect() };
        opened.uv.rotate_right(2);
        opened.pts.rotate_right(2);
        if opened.pts.len() == n && hi - lo <= per * (1.0 + 1e-9) && area2(&opened.uv) > 0.0 && !seam::winds(std::slice::from_ref(&opened), d, per) {
            *l = opened;
            return;
        }
    }
}

/// Each loop is projected on its own, so along a closed direction a hole can come out a whole
/// period away from the outer loop: a dome's outer loop opened at its pole spans v in [-1, 0]
/// while the hole projects into [0, 1], outside it (tangency-labs/solidcore#433). Every other
/// loop is moved by whole periods to the copy whose middle is nearest the outer loop's (the
/// loop of largest area); a face that does not wind lies within one period, so that is the
/// copy inside it.
fn holes_beside_outer(dom: &uv::Domain, loops: &mut [seam::Loop]) {
    let Some(outer) = loops.iter().map(|l| area2(&l.uv).abs()).enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i) else { return };
    let middle = |l: &seam::Loop, d: usize| {
        let (lo, hi) = l.uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q[d]), b.max(q[d])));
        0.5 * (lo + hi)
    };
    for d in 0..2 {
        let Some(per) = dom.period[d] else { continue };
        let Some(centre) = loops.get(outer).map(|l| middle(l, d)) else { continue };
        for (i, l) in loops.iter_mut().enumerate() {
            let k = ((middle(l, d) - centre) / per).round();
            if i == outer || !k.is_finite() || k == 0.0 {
                continue;
            }
            for q in &mut l.uv {
                q[d] -= k * per;
            }
        }
    }
}

/// One face: boundary loops in 3D (shared edge nodes) to parameters and triangles.
/// `size`: the face's target side length (mm) and the surface's mean speed along u and v.
fn plan_face(
    fi: usize,
    kind: &str,
    s: &EvalSurface,
    loops: &[seam::Loop],
    rules: &size::Rules,
    size: (f64, [f64; 2]),
    stats: &mut PlanStats,
) -> Result<FacePlan> {
    let tol = rules.tol;
    let (h, speed) = size;
    let dom = uv::domain(s);
    let fail = |why: &str| unsupported(format!("face {fi} ({kind}): {why}"));
    let mut projected: Vec<seam::Loop> = loops.to_vec();
    // A loop that winds around a periodic direction does not close in (u, v): the face is cut
    // along a parameter line into one period (a seamless cylinder face, a helical trim).
    // A loop that runs up a seam from a pole and straight back down it (a half sphere bounded
    // by its rim and its seam used both ways) winds once as projected: it is opened at the pole.
    for l in projected.iter_mut() {
        open_at_pole(s, &dom, l);
    }
    let wound: Vec<usize> = (0..2).filter(|&d| dom.period[d].is_some_and(|per| seam::winds(&projected, d, per))).collect();
    // A face cut along a seam is a long winding strip whose sides are the cut lines and the
    // boundary in turn: Delaunay with sag refinement meshes it (the patch layouts below assume
    // boundary all round, and leave fans across the strip's corners off the surface).
    let seamed = !wound.is_empty();
    match wound.as_slice() {
        [] => {}
        [d] => {
            let per = dom.period[*d].unwrap_or(std::f64::consts::TAU);
            projected = seam::cut(s, &projected, *d, per, rules, h).ok_or_else(|| fail("boundary winds around the surface"))?;
        }
        _ => return Err(fail("boundary winds around the surface both ways")),
    }
    // A loop from pole to pole and back along one seam (a whole sphere as one face, as another
    // kernel writes it) projects onto one parameter line: its second run is a period along.
    if !seamed {
        for l in projected.iter_mut() {
            open_pole_to_pole(s, &dom, l);
        }
        holes_beside_outer(&dom, &mut projected);
    }
    // Split poles.
    let mut fixed: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<[f64; 2]> = Vec::new();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for seam::Loop { uv: luv, pts } in &projected {
        let start = uvs.len();
        let n = luv.len();
        for (k, (q, p)) in luv.iter().zip(pts).enumerate() {
            // At a pole the parameter along the degenerate direction is free: one copy with
            // the previous node's value, one with the next node's.
            let (prev, next) = (luv.get((k + n - 1) % n).copied(), luv.get((k + 1) % n).copied());
            let pole = (0..2).find(|&d| uv::degenerate(s, *q, d));
            match (pole, prev, next) {
                (Some(d), Some(a), Some(b)) => {
                    let mut qa = *q;
                    let mut qb = *q;
                    qa[d] = a[d];
                    qb[d] = b[d];
                    uvs.push(qa);
                    fixed.push(*p);
                    if (qa[d] - qb[d]).abs() > 1e-12 {
                        uvs.push(qb);
                        fixed.push(*p);
                    }
                }
                _ => {
                    uvs.push(*q);
                    fixed.push(*p);
                }
            }
        }
        ranges.push(start..uvs.len());
    }
    if uvs.len() < 3 {
        return Err(fail("degenerate boundary"));
    }
    let mut bbox = [(f64::MAX, f64::MIN); 2];
    for p in &uvs {
        for d in 0..2 {
            bbox[d] = (bbox[d].0.min(p[d]), bbox[d].1.max(p[d]));
        }
    }
    // Constrained Delaunay in (u, v) scaled to millimetres: equilateral seeds of side `h`,
    // refined where triangles leave the surface, are too long or bend too much, then shaped by
    // Ruppert refinement.
    let polys: Vec<&[[f64; 2]]> = ranges.iter().filter_map(|r| uvs.get(r.clone())).collect();
    let step = [h / speed[0], h / speed[1]];
    let delaunay = || {
        let seeds = cdt::lattice(bbox, step, &polys, speed);
        let eval = |u: f64, v: f64| s.derivs(u, v).map(|(p, su, sv)| (p, su.cross(sv).normalized()));
        let sag = cdt::Sag { eval: &eval, tol, h, turn: rules.turn, scale: speed };
        let fill = cdt::Fill { seeds: &seeds, refine: Some((sag, MAX_REFINE)), shape: !matches!(s, EvalSurface::Plane { .. }) };
        cdt::triangulate(&uvs, &ranges, fill).ok_or_else(|| fail("boundary crosses itself"))
    };
    // Planar and curved faces alike: a structured grid where the boundary runs along the
    // parameter box (a rectangle, a cylinder half), a Coons grid on four corners, else Delaunay.
    let planar = matches!(s, EvalSurface::Plane { .. });
    let (extra, triangles) = {
        let mut us = uv::divide(s, 0, bbox[0], bbox[1], rules, h, MAX_LINES);
        let mut vs = uv::divide(s, 1, bbox[1], bbox[0], rules, h, MAX_LINES);
        grid::thin(&mut us, &mut vs, grid::MAX_FACE_GRID);
        let structured = match (ranges.len(), if seamed { None } else { grid::quad(&uvs) }) {
            (1, Some(q)) => {
                // Interior lines: the boundary's own where opposite sides agree (refined where the
                // surface turns more between them inside the face), else the division.
                let lines = |d: usize, div: &Vec<f64>| {
                    let inner = |l: Vec<f64>| l.into_iter().filter(|x| *x > bbox[d].0 && *x < bbox[d].1).collect::<Vec<f64>>();
                    match grid::aligned_lines(&uvs, &q, d) {
                        Some(aligned) => {
                            let ends: Vec<f64> = std::iter::once(bbox[d].0).chain(aligned).chain(std::iter::once(bbox[d].1)).collect();
                            inner(uv::refine(s, d, &ends, bbox[1 - d], rules, h, MAX_LINES))
                        }
                        None => inner(div.clone()),
                    }
                };
                let (lu, lv) = (lines(0, &us), lines(1, &vs));
                let strip = match (lu.is_empty(), lv.is_empty()) {
                    (false, true) => grid::strip(&uvs, &q, 0),
                    (true, false) => grid::strip(&uvs, &q, 1),
                    _ => None,
                };
                strip.map(|t| (Vec::new(), t)).or_else(|| grid::structured(&uvs, &q, &grid::interior(lu, bbox[0]), &grid::interior(lv, bbox[1])))
            }
            _ => None,
        };
        let structured = match structured {
            None if ranges.len() == 1 && !seamed => {
                let fine = |a: [f64; 2], b: [f64; 2]| uv::fine_between(s, rules, h, a, b);
                coons::coons(&uvs, &fixed, speed, grid::MAX_FACE_GRID, h, Some(&fine))
            }
            other => other,
        };
        match structured {
            Some(r) => {
                if planar {
                    stats.planar += 1;
                } else {
                    stats.structured += 1;
                }
                r
            }
            None => {
                // A curved face neither box nor four-cornered: cut into four-sided patches, else
                // the iso-line grid cut along its motorcycle traces and trimmed by its boundary,
                // else Delaunay. A plane: the butterfly of a round face, else Delaunay (a plane
                // is shaded the same by any triangles, and its Delaunay needs no refinement).
                // A curved strip, straight and within a side across one parameter (its division
                // there has no line inside) and with no room for a point of its own (a thread's
                // root band, which runs out obliquely so it has no four corners), is its boundary
                // nodes alone, refined where the surface leaves them: patches and grid lines
                // across it would only crowd it (the butterfly's cuts run lengthwise through a
                // long strip, seeded the whole way). A strip curved across (a fillet narrower
                // than a side) keeps the layouts that follow its curvature.
                let face = Region { s, uvs: &uvs, fixed: &fixed, ranges: &ranges, rules, h, speed, us: &us, vs: &vs };
                let strip = !planar && (us.len() <= 2 || vs.len() <= 2) && !cdt::has_room(bbox, step, &polys, speed);
                let trimmed = if seamed || strip {
                    None
                } else if planar {
                    if ranges.len() == 1 { plan_pieces(&face) } else { None }
                } else {
                    plan_pieces(&face).or_else(|| {
                        let traces = motorcycle::traces(&uvs, &ranges, speed, 0.25 * h);
                        cutgrid::cut_grid(&uvs, &ranges, &us, &vs, &traces, speed)
                    })
                };
                match trimmed {
                    Some(r) => {
                        stats.structured += 1;
                        r
                    }
                    None => {
                        if planar {
                            stats.planar += 1;
                        } else {
                            stats.delaunay += 1;
                        }
                        delaunay()?
                    }
                }
            }
        }
    };
    // At a pole the boundary has two copies of one node: a triangle on both copies has two
    // corners at one point and no area (its other two sides are one segment both ways).
    let pole_pair = |i: u32, j: u32| i != j && fixed.get(i as usize).zip(fixed.get(j as usize)).is_some_and(|(p, q)| p == q);
    let mut triangles = triangles;
    triangles.retain(|t| !(pole_pair(t[0], t[1]) || pole_pair(t[1], t[2]) || pole_pair(t[2], t[0])));
    if !closed_boundary(&ranges, &fixed, &triangles) {
        return Err(fail("triangulation does not follow the boundary"));
    }
    let mut all = uvs;
    all.extend(extra);
    // Whatever the layout, no triangle leaves the surface by more than the tolerance.
    if !planar {
        sag::refine(s, tol, &fixed, &mut all, &mut triangles, MAX_REFINE);
    }
    // Both evaluators take a closed spline's parameters inside its active interval.
    for q in &mut all {
        *q = s.wrap(*q);
    }
    Ok(FacePlan { face: u32::try_from(fi).unwrap_or(u32::MAX), surface: None, uv: all, fixed, normals: Vec::new(), flip: false, triangles })
}

/// A face's (u, v) region and what meshing it needs.
struct Region<'a> {
    s: &'a EvalSurface,
    uvs: &'a [[f64; 2]],
    fixed: &'a [Vec3],
    ranges: &'a [std::ops::Range<usize>],
    rules: &'a size::Rules,
    h: f64,
    speed: [f64; 2],
    us: &'a [f64],
    vs: &'a [f64],
}

/// A trimmed face cut into patches (squares round its holes or a butterfly in a round face,
/// joined to the outer loop by motorcycle traces), each meshed on its own (a Coons blend on its
/// best four corners; else the trimmed grid; else Delaunay), all sharing the cuts' nodes. The
/// cut nodes and patch interiors follow the boundary nodes. `None` when the cuts do not make
/// patches.
fn plan_pieces(f: &Region) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    // The face's global seed: one spacing in every direction, the finest its size rules ask for
    // along either parameter (the median gap between the grid lines they give, in mm). On a
    // cylinder that is the spacing round it, which the cuts along its axis then get too.
    let median_gap = |lines: &[f64], speed: f64| -> f64 {
        let mut gaps: Vec<f64> = lines.windows(2).filter_map(|w| Some((w.get(1)? - w.first()?) * speed)).filter(|g| *g > 0.0).collect();
        gaps.sort_by(f64::total_cmp);
        gaps.get(gaps.len() / 2).copied().unwrap_or(f64::INFINITY)
    };
    let seed = median_gap(f.us, f.speed[0]).min(median_gap(f.vs, f.speed[1])).min(f.h);
    let segments = collar::segments(f.uvs, f.ranges, f.speed, seed, matches!(f.s, EvalSurface::Plane { .. }))?;
    let s = f.s;
    let fine = |a: [f64; 2], b: [f64; 2]| uv::fine_between(s, f.rules, f.h, a, b);
    // A cut's seeds: spaced from the boundary's own spacing at each end, growing geometrically
    // between them, never wider than the global seed, and split further wherever the surface
    // between two seeds fails the size rules.
    let seeds = |a: [f64; 2], b: [f64; 2], sa: f64, sb: f64| -> Vec<f64> { graded(a, b, sa, sb, seed, f.speed, &fine) };
    let (cut_uv, pieces) = pieces::cut(f.uvs, f.ranges, &segments, f.speed, &seeds)?;
    let n = f.uvs.len();
    let mut uv_all: Vec<[f64; 2]> = f.uvs.to_vec();
    uv_all.extend_from_slice(&cut_uv);
    let mut pos: Vec<Vec3> = f.fixed.to_vec();
    for q in &cut_uv {
        pos.push(s.derivs(q[0], q[1])?.0);
    }
    let mut extra: Vec<[f64; 2]> = cut_uv;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for pc in &pieces {
        // The patch's own loops, numbered from zero.
        let mut ids: Vec<u32> = pc.ring.to_vec();
        let mut ranges = vec![std::ops::Range { start: 0, end: pc.ring.len() }];
        for hole in &pc.holes {
            ranges.push(ids.len()..ids.len() + hole.len());
            ids.extend_from_slice(hole);
        }
        let luv: Vec<[f64; 2]> = ids.iter().map(|&k| uv_all.get(k as usize).copied()).collect::<Option<_>>()?;
        let lpos: Vec<Vec3> = ids.iter().map(|&k| pos.get(k as usize).copied()).collect::<Option<_>>()?;
        let mut bbox = [(f64::MAX, f64::MIN); 2];
        for p in &luv {
            for d in 0..2 {
                bbox[d] = (bbox[d].0.min(p[d]), bbox[d].1.max(p[d]));
            }
        }
        let clip = |lines: &[f64], (a, b): (f64, f64)| -> Vec<f64> {
            std::iter::once(a).chain(lines.iter().copied().filter(|x| *x > a && *x < b)).chain(std::iter::once(b)).collect()
        };
        // A patch whose outline changes between boundary and cut at exactly four nodes (a
        // quarter of a ring, a butterfly wing round a hole) takes those as its corners, so each
        // side is all boundary or all cut; any other patch takes the best four of its corners.
        let is_cut = |a: u32, b: u32| -> bool {
            let (a, b) = (a as usize, b as usize);
            a >= n || b >= n || !f.ranges.iter().any(|r| r.contains(&a) && r.contains(&b) && (a.abs_diff(b) == 1 || a.abs_diff(b) == r.len() - 1))
        };
        let ring = &pc.ring;
        let switches: Vec<usize> = ring.corners().enumerate().filter(|&(_, (&p, &c, &x))| is_cut(p, c) != is_cut(c, x)).map(|(k, _)| k).collect();
        // The patch is a rectangle in (u, v) with corners cut by free boundary (Lyon et al. §4;
        // completed virtually in §4.2): its bounding box. A grid needs four real sides, so each
        // corner is the node furthest out towards the box's corner (in the box's own
        // proportions): an uncut corner is the corner itself, and on an arc cutting a corner it
        // is the arc's 45° point, so the arc becomes a rounded corner shared by two sides.
        let completed = || -> Option<[usize; 4]> {
            let m = ring.len();
            let at = |k: usize| luv.get(k).map(|q| [q[0] * f.speed[0], q[1] * f.speed[1]]);
            let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
            for k in 0..m {
                let q = at(k)?;
                for d in 0..2 {
                    lo[d] = lo[d].min(q[d]);
                    hi[d] = hi[d].max(q[d]);
                }
            }
            let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            let mut pick = [0usize; 4];
            for (slot, dir) in pick.iter_mut().zip([[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]) {
                *slot = (0..m).max_by(|&x, &y| {
                    let reach = |k: usize| at(k).map_or(f64::MIN, |q| dir[0] * (q[0] - lo[0]) / w + dir[1] * (q[1] - lo[1]) / h);
                    reach(x).total_cmp(&reach(y))
                })?;
            }
            // Four distinct corners in the loop's (counter-clockwise) order.
            let start = pick[0];
            let rel = pick.map(|k| (k + m - start) % m);
            (rel[0] < rel[1] && rel[1] < rel[2] && rel[2] < rel[3]).then_some(pick)
        };
        let quad = if pc.holes.is_empty() {
            // Every patch is four-sided by construction: its corners are where its outline turns.
            coons::coons(&luv, &lpos, f.speed, grid::MAX_FACE_GRID, f.h, None).or_else(|| {
                match switches[..] {
                    [a, b, c, d] => coons::coons_at(&luv, &lpos, [a, b, c, d], grid::MAX_FACE_GRID, f.h, None),
                    _ => None,
                }
                .or_else(|| completed().and_then(|c| coons::coons_at(&luv, &lpos, c, grid::MAX_FACE_GRID, f.h, None)))
            })
        } else {
            None
        };
        let local = quad
            .or_else(|| if pc.holes.is_empty() { coons::coons_best(&luv, &lpos, f.speed, grid::MAX_FACE_GRID, f.h) } else { None })
            .or_else(|| cutgrid::cut_grid(&luv, &ranges, &clip(f.us, bbox[0]), &clip(f.vs, bbox[1]), &[], f.speed))
            .or_else(|| {
                let polys: Vec<&[[f64; 2]]> = ranges.iter().filter_map(|r| luv.get(r.clone())).collect();
                let seeds = cdt::lattice(bbox, [f.h / f.speed[0], f.h / f.speed[1]], &polys, f.speed);
                let eval = |u: f64, v: f64| s.derivs(u, v).map(|(p, su, sv)| (p, su.cross(sv).normalized()));
                let sag = cdt::Sag { eval: &eval, tol: f.rules.tol, h: f.h, turn: f.rules.turn, scale: f.speed };
                cdt::triangulate(&luv, &ranges, cdt::Fill { seeds: &seeds, refine: Some((sag, MAX_REFINE)), shape: true })
            })?;
        let (lextra, ltris) = local;
        let base = u32::try_from(n + extra.len()).ok()?;
        let m = ids.len();
        for t in ltris {
            let map = |i: u32| -> Option<u32> {
                let i = i as usize;
                if i < m { ids.get(i).copied() } else { base.checked_add(u32::try_from(i - m).ok()?) }
            };
            tris.push([map(t[0])?, map(t[1])?, map(t[2])?]);
        }
        extra.extend(lextra);
    }
    Some((extra, tris))
}

/// Seeds on the segment `a`–`b` in (u, v) (fractions strictly between 0 and 1): node spacing
/// `sa` at `a` and `sb` at `b` (in the metric `speed`, each at most `h`), growing geometrically
/// between them (sizes `sa · q^k` summing to the length), then each interval split in half while
/// `fine` fails between its ends.
fn graded(a: [f64; 2], b: [f64; 2], sa: f64, sb: f64, h: f64, speed: [f64; 2], fine: &dyn Fn([f64; 2], [f64; 2]) -> bool) -> Vec<f64> {
    let len = ((b[0] - a[0]) * speed[0]).hypot((b[1] - a[1]) * speed[1]);
    let cap = |x: f64| {
        if x.is_finite() && x > 0.0 {
            x.min(if h > 0.0 { h } else { f64::INFINITY })
        } else if h > 0.0 {
            h
        } else {
            len
        }
    };
    let (sa, sb) = (cap(sa), cap(sb));
    let mut cuts: Vec<f64> = Vec::new();
    if len.is_finite() && len > sa.max(sb) && sa > 0.0 && sb > 0.0 {
        let q = (len - sa) / (len - sb);
        let sizes: Vec<f64> = if (q - 1.0).abs() < 1e-3 || !(q > 0.0) || !q.is_finite() {
            // Even spacing at the mean of the two.
            let k = (len / (0.5 * (sa + sb))).round().clamp(1.0, 4096.0) as usize;
            vec![len / k as f64; k]
        } else {
            let k = (1.0 + (sb / sa).ln() / q.ln()).round().clamp(1.0, 4096.0) as usize;
            let raw: Vec<f64> = (0..k).map(|j| sa * q.powi(j as i32)).collect();
            let total: f64 = raw.iter().sum();
            raw.into_iter().map(|x| x * len / total).collect()
        };
        let mut at = 0.0;
        for s in sizes.iter().take(sizes.len().saturating_sub(1)) {
            at += s;
            cuts.push(at / len);
        }
    }
    // The size rules between seeds (sag, facet angle).
    let point = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let mut all: Vec<f64> = std::iter::once(0.0).chain(cuts).chain(std::iter::once(1.0)).collect();
    for _ in 0..12 {
        let mut next = Vec::with_capacity(all.len() * 2);
        let mut split = false;
        for w in all.windows(2) {
            let (Some(&x), Some(&y)) = (w.first(), w.get(1)) else { continue };
            next.push(x);
            if !fine(point(x), point(y)) {
                next.push(0.5 * (x + y));
                split = true;
            }
        }
        next.push(1.0);
        all = next;
        if !split || all.len() > 4096 {
            break;
        }
    }
    all.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect()
}

/// Every boundary segment (consecutive nodes of a loop) is the side of exactly one triangle:
/// the face covers its boundary with nothing missing or doubled, so it meets its neighbours
/// exactly. Pole copies of one node (consecutive in a loop, at one point) are one vertex: the
/// segment between them has no length, and a side ending on either copy is the same side.
fn closed_boundary(ranges: &[std::ops::Range<usize>], fixed: &[Vec3], triangles: &[[u32; 3]]) -> bool {
    let mut one: Vec<u32> = (0..fixed.len() as u32).collect();
    let root = |one: &[u32], mut i: u32| {
        while let Some(&j) = one.get(i as usize).filter(|j| **j != i) {
            i = j;
        }
        i
    };
    let segment = |r: &std::ops::Range<usize>, k: usize| ((r.start + k) as u32, (r.start + (k + 1) % r.len()) as u32);
    for r in ranges {
        for k in 0..r.len() {
            let (a, b) = segment(r, k);
            if fixed.get(a as usize).zip(fixed.get(b as usize)).is_some_and(|(p, q)| p == q) {
                let (ra, rb) = (root(&one, a), root(&one, b));
                if let Some(x) = one.get_mut(rb.max(ra) as usize) {
                    *x = ra.min(rb);
                }
            }
        }
    }
    let side = |a: u32, b: u32| {
        let (a, b) = (root(&one, a), root(&one, b));
        (a != b).then_some((a.min(b), a.max(b)))
    };
    let mut sides: HashMap<(u32, u32), u32> = HashMap::with_capacity(triangles.len() * 3);
    for t in triangles {
        for e in 0..3 {
            if let Some(k) = side(t[e], t[(e + 1) % 3]) {
                *sides.entry(k).or_default() += 1;
            }
        }
    }
    ranges.iter().all(|r| {
        (0..r.len()).all(|k| {
            let (a, b) = segment(r, k);
            side(a, b).is_none_or(|k| sides.get(&k).copied() == Some(1))
        })
    })
}

#[cfg(test)]
mod tests;
