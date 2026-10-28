//! Repair Part (#8371): a part SMLib refuses, or an open part that should be a solid, made into
//! one SMLib takes, saying what changed face by face. In order:
//!
//! 1. A face SMLib refuses on its own is rebuilt from its own surface, its geometry unchanged: a
//!    cylinder, cone, sphere or torus face on a NURBS form SMLib rejects goes as the analytic
//!    surface it is, which SMLib parameterises on its own form (the form's control net is the
//!    import's construction; the surface it stands for is the label's, exactly).
//! 2. Free edges that pair up within `gap` (a sliver between two faces that should meet) are sewn
//!    by SMLib's stitch, which gives each edge it glues its own tolerance from that pair's gap
//!    (`SmBrep::GlueEdgesGeneral`: at least twice the gap); the body's tolerance, and every other
//!    edge's, stays as it was.
//! 3. A loop of free edges no pair explains is a missing face: Patch on that loop is offered (and
//!    made, and sewn in, when asked), planar only where the faces around it are that plane; where
//!    they are curved the patch meets them tangent (G1). Extending the neighbours is not done.
//!
//! Nothing is moved: a face rebuilt keeps its surface, a sewn edge keeps its curve (SMLib keeps
//! one of the pair), a patch is a new face. What was done comes back in [`RepairReport`].

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{self as b, Brep};

use crate::body::{Body, SurfaceKind};
use crate::{Continuity, KernelError, Result};

/// Most free edges paired against each other (each against every other near it).
const MAX_FREE_EDGES: usize = 20_000;
/// Most faces sent into SMLib one by one to find the ones it refuses.
const MAX_FACE_PROBES: usize = 20_000;
/// Points along an edge where its distance to its partner is measured.
const GAP_SAMPLES: usize = 16;
/// SMLib's check tests that bound an item's tolerance by the body's (`SmBrep::AssertValid` #5,
/// #6: face; #59: edge, under 1000 times the body's; #82: vertex). An edge sewn across a gap
/// carries that gap; the body's tolerance is not raised to cover it, so these may fail on sewn
/// items, as they already fail on an import's edges that lie off their faces by more.
const TOLERANCE_BOUND_TESTS: [u32; 4] = [5, 6, 59, 82];

/// What a repair may do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RepairOptions {
    /// The widest gap between two free edges that is sewn, mm (> 0).
    pub gap: f64,
    /// Patch the loops of free edges no pair explains (missing faces), and sew the patches in.
    pub patch_holes: bool,
}

/// A face rebuilt so SMLib takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct RebuiltFace {
    /// Its index in the body before the repair.
    pub face: usize,
    pub surface: SurfaceKind,
    /// Why SMLib refused it before.
    pub why: String,
}

/// A face SMLib refuses that the repair could not rebuild.
#[derive(Clone, Debug, PartialEq)]
pub struct RefusedFace {
    pub face: usize,
    pub surface: SurfaceKind,
    pub why: String,
}

/// Two free edges sewn into one.
#[derive(Clone, Debug, PartialEq)]
pub struct SewnGap {
    /// The faces on either side, by index before the repair.
    pub faces: [usize; 2],
    /// The widest distance between the two edges, mm.
    pub gap: f64,
    /// A point halfway along the first edge.
    pub at: Vec3,
}

/// A loop of free edges no pair explains: a face is missing there.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    /// The faces around it, by index before the repair.
    pub faces: Vec<usize>,
    /// A point halfway along each of its edges: Patch's `edges`.
    pub edges: Vec<Vec3>,
    /// How a patch meets the faces around it: G0 (the plane they lie in) or G1 (curved neighbours).
    pub continuity: Continuity,
    /// The patch was made and sewn in.
    pub patched: bool,
    /// Why it was not, when it was asked for.
    pub why: Option<String>,
}

/// What a repair did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepairReport {
    pub rebuilt: Vec<RebuiltFace>,
    pub refused: Vec<RefusedFace>,
    pub sewn: Vec<SewnGap>,
    pub holes: Vec<Hole>,
    /// Faces before and after.
    pub faces: [usize; 2],
    /// The largest edge tolerance before and after, mm.
    pub largest_edge_tolerance: [f64; 2],
    /// The result is a closed solid.
    pub closed: bool,
    /// Whether SMLib takes the result as it is.
    pub modelling: bool,
}

impl RepairReport {
    /// Whether the repair changed anything.
    pub fn changed(&self) -> bool {
        !self.rebuilt.is_empty() || !self.sewn.is_empty() || self.holes.iter().any(|h| h.patched)
    }
}

fn invalid(what: impl Into<String>) -> KernelError {
    KernelError::Invalid(what.into())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// `body` repaired as the module says, with what was done. Refused (with why) when nothing
/// applies: SMLib takes it and it has no free edges, or its only free edges bound holes and
/// `patch_holes` is off (the report's holes are then in the message).
pub fn repair(body: &Body, opts: &RepairOptions) -> Result<(Body, RepairReport)> {
    let _span = solidcraft_profile::span("kernel", "repair");
    body.require_brep("repair")?;
    if !(opts.gap.is_finite() && opts.gap > 0.0 && opts.gap <= 1.0) {
        return Err(invalid("the gap to sew must be more than 0 and at most 1 mm"));
    }
    let before = body.record();
    let mut report =
        RepairReport { faces: [before.faces.len(), 0], largest_edge_tolerance: [largest_tolerance(before), 0.0], ..RepairReport::default() };

    // 1. Faces SMLib refuses on their own, rebuilt from their surfaces.
    let mut record = before.clone();
    if body.smlib().is_err() {
        rebuild_refused(&mut record, &mut report)?;
    }
    let mut current = if report.rebuilt.is_empty() { body.clone() } else { Body::from_record(record)?.with_color(body.color()) };
    if !report.refused.is_empty() && current.smlib().is_err() {
        // SMLib still refuses the part: nothing can be sewn or patched (both are SMLib's). The
        // faces rebuilt are kept, and the report names the ones left.
        let list: Vec<String> = report.refused.iter().map(|r| format!("face {} ({:?}): {}", r.face + 1, r.surface, r.why)).collect();
        if report.rebuilt.is_empty() {
            return Err(KernelError::Failed(format!("SMLib refuses faces the repair cannot rebuild: {}", list.join("; "))));
        }
        let r = current.record();
        report.holes = loops_of(r, &free_edges(r)?).iter().map(|l| hole(r, l)).collect();
        report.faces[1] = r.faces.len();
        report.largest_edge_tolerance[1] = largest_tolerance(r);
        report.closed = false;
        report.modelling = false;
        return Ok((current, report));
    }

    // 2. and 3. The free edges: pairs within the gap, and loops of the rest.
    let free = free_edges(current.record())?;
    let pairs = pair_up(current.record(), &free, opts.gap);
    let paired: std::collections::HashSet<u32> = pairs.iter().flat_map(|p| [p.edges[0], p.edges[1]]).collect();
    let rest: Vec<u32> = free.iter().copied().filter(|e| !paired.contains(e)).collect();
    let loops = loops_of(current.record(), &rest);
    for l in &loops {
        report.holes.push(hole(current.record(), l));
    }
    for p in &pairs {
        report.sewn.push(SewnGap { faces: p.faces, gap: p.gap, at: p.at });
    }
    if !pairs.is_empty() {
        let widest = pairs.iter().map(|p| p.gap).fold(0.0, f64::max);
        // SMLib glues the lamina pairs within the stitch tolerance: just past the widest pair, so
        // each of them glues and nothing farther apart does.
        let tol = (widest * 1.05).max(current.record().tolerance * 2.0);
        current = crate::surfaces::stitch_no_worse(&[&current], tol, &TOLERANCE_BOUND_TESTS)?;
    }
    if opts.patch_holes && !report.holes.is_empty() {
        current = patch_holes(current, &mut report.holes)?;
    }
    if !report.changed() {
        let holes: Vec<String> = report
            .holes
            .iter()
            .map(|h| {
                format!(
                    "a loop of {} edges around faces {}",
                    h.edges.len(),
                    h.faces.iter().map(|f| (f + 1).to_string()).collect::<Vec<_>>().join(", ")
                )
            })
            .collect();
        return Err(invalid(if holes.is_empty() {
            "nothing to repair: SMLib takes the part and it has no gaps or holes".to_string()
        } else {
            format!("nothing sewn: the free edges bound holes ({}); patch them to close the part", holes.join("; "))
        }));
    }
    let after = current.record();
    report.faces[1] = after.faces.len();
    report.largest_edge_tolerance[1] = largest_tolerance(after);
    report.closed = !after.faces.is_empty() && after.edges.iter().all(|e| e.uses.len() >= 2);
    report.modelling = current.smlib().is_ok();
    Ok((current, report))
}

fn largest_tolerance(r: &Brep) -> f64 {
    r.edges.iter().map(|e| e.tolerance).fold(r.tolerance, f64::max)
}

// ---- 1. Faces SMLib refuses -------------------------------------------------------------

/// Whether SMLib takes face `f` of `r` on its own; `Err` with SMLib's reason when it does not.
fn alone(r: &Brep, f: usize) -> Result<std::result::Result<(), String>> {
    let one = crate::surfaces::Parts::of(r, &[(f, false)])?.canonical()?;
    Ok(solidcraft_smlib::Brep::from_record(&one.brep, true).map(|_| ()).map_err(|e| e.to_string()))
}

/// Every face of `record` SMLib refuses on its own, rebuilt where its surface allows (into
/// `report.rebuilt`), else listed (`report.refused`).
fn rebuild_refused(record: &mut Brep, report: &mut RepairReport) -> Result<()> {
    if record.faces.len() > MAX_FACE_PROBES {
        return Err(KernelError::TooLarge(format!("a part of {} faces (the repair looks at most {MAX_FACE_PROBES})", record.faces.len())));
    }
    for f in 0..record.faces.len() {
        let Err(why) = alone(record, f)? else { continue };
        let Some(face) = record.faces.get(f) else { continue };
        let surface = SurfaceKind::of(&face.surface);
        match rebuilt_face(record, f) {
            Ok(Some(face)) => {
                let mut trial = record.clone();
                if let Some(slot) = trial.faces.get_mut(f) {
                    *slot = face;
                }
                match alone(&trial, f)? {
                    Ok(()) => {
                        *record = trial;
                        report.rebuilt.push(RebuiltFace { face: f, surface, why });
                    }
                    Err(still) => {
                        report.refused.push(RefusedFace { face: f, surface, why: format!("{why}; rebuilt on its own surface, still: {still}") })
                    }
                }
            }
            Ok(None) => report.refused.push(RefusedFace { face: f, surface, why }),
            Err(e) => report.refused.push(RefusedFace { face: f, surface, why: format!("{why}; not rebuilt: {e}") }),
        }
    }
    Ok(())
}

/// Face `f` of `r` rebuilt on its own surface: a cylinder, cone, sphere or torus face on a
/// NURBS form of the surface laid on arcs from where the face starts
/// ([`b::form::surface_on_own_arcs`]) instead of its frame's quarter grid, the same surface
/// exactly, with SMLib's own trims of the face's edges on it (`SmlibTrims`, as the import makes the
/// trims it has no closed form for). The face's edges and loops stay as they were. `None` for a
/// face on another surface.
fn rebuilt_face(r: &Brep, f: usize) -> Result<Option<b::Face>> {
    use b::pcurve::{TrimEdge, TrimFace, TrimUse, Trims};
    let face = r.faces.get(f).ok_or_else(|| invalid(format!("no face {f}")))?;
    if !matches!(face.surface, b::Surface::Cylinder { .. } | b::Surface::Cone { .. } | b::Surface::Sphere { .. } | b::Surface::Torus { .. }) {
        return Ok(None);
    }
    // The face's edges and vertices, numbered here as the trim face takes them.
    let mut local: Vec<u32> = Vec::new();
    let mut vertices: Vec<u32> = Vec::new();
    let mut loops = Vec::with_capacity(face.loops.len());
    let mut boundary = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let b::Loop::Edges { edges } = l else { return Ok(None) };
        let mut uses = Vec::with_capacity(edges.len());
        let mut lb = Vec::with_capacity(edges.len());
        for u in edges {
            let e = r.edges.get(u.edge as usize).ok_or_else(|| invalid(format!("no edge {}", u.edge)))?;
            let k = match local.iter().position(|&x| x == u.edge) {
                Some(k) => k,
                None => {
                    local.push(u.edge);
                    local.len() - 1
                }
            };
            uses.push(TrimUse { edge: u32::try_from(k).map_err(|_| invalid("too many edges"))?, forward: u.forward, pcurve: None });
            lb.push((&e.curve, e.interval, u.forward));
        }
        loops.push(uses);
        boundary.push(lb);
    }
    let mut edges = Vec::with_capacity(local.len());
    for &e in &local {
        let edge = r.edges.get(e as usize).ok_or_else(|| invalid(format!("no edge {e}")))?;
        let mut ends = [0u32; 2];
        for (end, v) in ends.iter_mut().zip(edge.vertices) {
            let k = match vertices.iter().position(|&x| x == v) {
                Some(k) => k,
                None => {
                    vertices.push(v);
                    vertices.len() - 1
                }
            };
            *end = u32::try_from(k).map_err(|_| invalid("too many vertices"))?;
        }
        edges.push(TrimEdge { curve: &edge.curve, nurbs: edge.nurbs.as_ref(), interval: edge.interval, vertices: ends, tolerance: edge.tolerance });
    }
    let points = vertices
        .iter()
        .map(|&v| r.vertices.get(v as usize).map(|v| v.point).ok_or_else(|| invalid(format!("no vertex {v}"))))
        .collect::<Result<Vec<_>>>()?;
    let extent = b::form::extent(&face.surface, &boundary).ok_or_else(|| invalid("its boundary does not lie on its surface"))?;
    let (form, domain) = b::form::surface_on_own_arcs(
        &face.surface,
        extent,
        local.iter().filter_map(|&e| r.edges.get(e as usize)).map(|e| e.tolerance).fold(r.tolerance, f64::max),
    )
    .ok_or_else(|| invalid("no NURBS form of its surface over its boundary"))?;
    let trim = TrimFace { surface: &face.surface, nurbs: Some(&form), domain, edges, vertices: points, loops, tolerance: r.tolerance };
    let pcurves = solidcraft_smlib::pcurves::SmlibTrims.trims(&trim).map_err(KernelError::Failed)?;
    let mut out = face.clone();
    out.nurbs = Some(form);
    out.domain = domain;
    let mut it = pcurves.into_iter();
    for l in &mut out.loops {
        let b::Loop::Edges { edges } = l else { continue };
        let pcs = it.next().ok_or_else(|| invalid("pcurves of a loop"))?;
        for (u, pc) in edges.iter_mut().zip(pcs) {
            u.pcurve = pc;
        }
    }
    Ok(Some(out))
}

// ---- 2. Gaps ----------------------------------------------------------------------------

/// The edges of `r` only one face uses.
fn free_edges(r: &Brep) -> Result<Vec<u32>> {
    let free: Vec<u32> =
        (0..r.edges.len()).filter(|&e| r.edges.get(e).is_some_and(|e| e.uses.len() == 1)).filter_map(|e| u32::try_from(e).ok()).collect();
    if free.len() > MAX_FREE_EDGES {
        return Err(KernelError::TooLarge(format!("a part with {} free edges (the repair pairs at most {MAX_FREE_EDGES})", free.len())));
    }
    Ok(free)
}

/// Two free edges within the gap of each other.
struct Pair {
    edges: [u32; 2],
    faces: [usize; 2],
    gap: f64,
    at: Vec3,
}

/// An edge's points, evenly in its parameter (ends included).
fn points(r: &Brep, e: u32, n: usize) -> Vec<Vec3> {
    let Some(edge) = r.edges.get(e as usize) else { return Vec::new() };
    let Ok(c) = CurveEval::new(edge.parameters()) else { return Vec::new() };
    let [t0, t1] = edge.interval;
    (0..=n).filter_map(|k| c.point(t0 + (t1 - t0) * k as f64 / n as f64)).collect()
}

/// The widest distance from a point of edge `a` to edge `b` (each of `a`'s points to its nearest
/// point on `b`), `None` where `b` cannot be evaluated.
fn reach(r: &Brep, a: &[Vec3], b: u32) -> Option<f64> {
    let edge = r.edges.get(b as usize)?;
    let c = CurveEval::new(edge.parameters()).ok()?;
    let ends = [c.point(edge.interval[0])?, c.point(edge.interval[1])?];
    a.iter()
        .map(|p| {
            let inside = crate::surfaces::nearest_parameter(&c, edge.interval, *p).map_or(f64::INFINITY, |(_, d)| d);
            inside.min(ends[0].dist(*p)).min(ends[1].dist(*p))
        })
        .try_fold(0.0f64, |m, d| d.is_finite().then_some(m.max(d)))
}

/// The free edges that pair up within `gap`, both ways (each edge's every point near the other),
/// each edge in at most one pair: the closest pairs first. An edge of a face with itself is not
/// a gap.
fn pair_up(r: &Brep, free: &[u32], gap: f64) -> Vec<Pair> {
    let samples: Vec<Vec<Vec3>> = free.iter().map(|&e| points(r, e, GAP_SAMPLES)).collect();
    let boxes: Vec<Option<(Vec3, Vec3)>> = samples
        .iter()
        .map(|s| {
            let first = *s.first()?;
            Some(s.iter().fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
        })
        .collect();
    let face = |e: u32| r.edges.get(e as usize).and_then(|e| e.uses.first()).map(|u| u.face as usize);
    let mut candidates = Vec::new();
    for i in 0..free.len() {
        for j in i + 1..free.len() {
            let (Some(&a), Some(&b)) = (free.get(i), free.get(j)) else { continue };
            let (Some(Some((alo, ahi))), Some(Some((blo, bhi)))) = (boxes.get(i), boxes.get(j)) else { continue };
            let apart = (blo.x - ahi.x).max(alo.x - bhi.x).max(blo.y - ahi.y).max(alo.y - bhi.y).max(blo.z - ahi.z).max(alo.z - bhi.z);
            if apart > gap || face(a) == face(b) {
                continue;
            }
            let (Some(sa), Some(sb)) = (samples.get(i), samples.get(j)) else { continue };
            let Some(d) = reach(r, sa, b).zip(reach(r, sb, a)).map(|(x, y)| x.max(y)) else { continue };
            if d <= gap {
                candidates.push((d, a, b));
            }
        }
    }
    candidates.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut taken = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (d, a, b) in candidates {
        if taken.contains(&a) || taken.contains(&b) {
            continue;
        }
        let (Some(fa), Some(fb)) = (face(a), face(b)) else { continue };
        taken.insert(a);
        taken.insert(b);
        let at = points(r, a, 2).get(1).copied().unwrap_or(Vec3::ZERO);
        out.push(Pair { edges: [a, b], faces: [fa, fb], gap: d, at });
    }
    out
}

// ---- 3. Holes ---------------------------------------------------------------------------

/// The edges `rest` of `r` in chains joined at shared vertices: each a loop around a hole (or an
/// open run where the boundary does not close).
fn loops_of(r: &Brep, rest: &[u32]) -> Vec<Vec<u32>> {
    let mut by_vertex: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for &e in rest {
        if let Some(edge) = r.edges.get(e as usize) {
            for v in edge.vertices {
                by_vertex.entry(v).or_default().push(e);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for &start in rest {
        if !seen.insert(start) {
            continue;
        }
        let mut chain = vec![start];
        let mut stack = vec![start];
        while let Some(e) = stack.pop() {
            let Some(edge) = r.edges.get(e as usize) else { continue };
            for v in edge.vertices {
                for &n in by_vertex.get(&v).into_iter().flatten() {
                    if seen.insert(n) {
                        chain.push(n);
                        stack.push(n);
                    }
                }
            }
        }
        out.push(chain);
    }
    out
}

/// The hole a loop of free edges bounds, and how a patch would meet the faces around it: flat
/// (the loop's plane) when the loop is planar and every face around it is a plane, else tangent
/// to them.
fn hole(r: &Brep, edges: &[u32]) -> Hole {
    let mut faces: Vec<usize> = edges.iter().filter_map(|&e| r.edges.get(e as usize)?.uses.first().map(|u| u.face as usize)).collect();
    faces.sort_unstable();
    faces.dedup();
    let at: Vec<Vec3> = edges.iter().filter_map(|&e| points(r, e, 2).get(1).copied()).collect();
    let along: Vec<Vec3> = edges.iter().flat_map(|&e| points(r, e, 8)).collect();
    let tol = edges.iter().filter_map(|&e| r.edges.get(e as usize)).map(|e| e.tolerance).fold(r.tolerance, f64::max);
    let planes = faces.iter().all(|&f| r.faces.get(f).is_some_and(|face| plane_of(&face.surface).is_some()));
    let flat = planes && plane_through(&along).is_some_and(|(o, n)| along.iter().all(|p| (*p - o).dot(n).abs() <= tol));
    Hole { faces, edges: at, continuity: if flat { Continuity::G0 } else { Continuity::G1 }, patched: false, why: None }
}

/// The plane of a closed polygon's points (Newell's normal, through their centroid); `None`
/// where they enclose no area.
fn plane_through(pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let c = pts.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / n as f64);
    let normal = (0..n).filter_map(|i| Some((pts.get(i)?, pts.get((i + 1) % n)?))).fold(Vec3::ZERO, |s, (p, q)| s + (*p - c).cross(*q - c));
    Some((c, normal.normalized()?))
}

/// A plane surface's point and unit normal.
fn plane_of(s: &b::Surface) -> Option<(Vec3, Vec3)> {
    let (o, u, v) = match s {
        b::Surface::Plane { origin, u_axis, v_axis } => (v3(*origin), v3(*u_axis), v3(*v_axis)),
        b::Surface::PlaneThrough { origin, u_point, v_point } => (v3(*origin), v3(*u_point) - v3(*origin), v3(*v_point) - v3(*origin)),
        _ => return None,
    };
    Some((o, u.cross(v).normalized()?))
}

/// `body` with each hole patched (Patch on its loop, at its continuity) and the patches sewn in
/// along the hole's own edges; a hole whose patch fails keeps why.
fn patch_holes(body: Body, holes: &mut [Hole]) -> Result<Body> {
    let mut patches = Vec::new();
    let mut tol = body.record().tolerance;
    for h in holes.iter_mut() {
        let spec = crate::PatchSpec { continuity: h.continuity, ..crate::PatchSpec::default() };
        match crate::patch_edges_with(&body, &h.edges, &spec) {
            Ok(p) => {
                tol = tol.max(largest_tolerance(p.record()));
                patches.push(p);
                h.patched = true;
            }
            Err(e) => h.why = Some(e.to_string()),
        }
    }
    if patches.is_empty() {
        return Ok(body);
    }
    let mut all: Vec<&Body> = vec![&body];
    all.extend(patches.iter());
    // A patch's edges are the hole's own: they meet within those edges' tolerances.
    crate::surfaces::stitch_no_worse(&all, tol * 2.0, &TOLERANCE_BOUND_TESTS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, copy_faces, measure, stitch, transform};

    /// The six faces of a 10 × 20 × 30 box, each its own surface body: bottom, top, then the sides.
    fn box_faces() -> Vec<Body> {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        [[5.0, 10.0, 0.0], [5.0, 10.0, 30.0], [0.0, 10.0, 15.0], [10.0, 10.0, 15.0], [5.0, 0.0, 15.0], [5.0, 20.0, 15.0]]
            .iter()
            .map(|p| copy_faces(&b, &[v3(*p)]).unwrap())
            .collect()
    }

    /// The box without its top: an open part with a hole the four sides and their plane bound.
    fn open_box(faces: &[Body]) -> Body {
        stitch(&[&faces[0], &faces[2], &faces[3], &faces[4], &faces[5]], 0.0).unwrap()
    }

    #[test]
    fn a_gap_between_faces_is_sewn_each_sewn_edge_carrying_its_gap() {
        let faces = box_faces();
        let open = open_box(&faces);
        // The lid 0.05 mm above the walls: one part of six faces whose top edges pair up 0.05 apart.
        let lid = transform(&faces[1], Vec3::new(0.0, 0.0, 0.05), Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 0.0).unwrap();
        let part = stitch(&[&open, &lid], 1e-6).unwrap();
        assert_eq!(part.record().edges.iter().filter(|e| e.uses.len() == 1).count(), 8, "the gap's edges are free");
        let opts = RepairOptions { gap: 0.1, patch_holes: false };
        let (fixed, report) = repair(&part, &opts).unwrap();
        assert_eq!(report.sewn.len(), 4, "{report:?}");
        assert!(report.sewn.iter().all(|g| (g.gap - 0.05).abs() < 1e-6), "{report:?}");
        assert!(report.holes.is_empty() && report.rebuilt.is_empty());
        assert!(report.closed && report.modelling, "{report:?}");
        assert_eq!(report.faces, [6, 6]);
        let volume = measure(&fixed).unwrap().volume;
        assert!((6000.0..=6010.0).contains(&volume), "{volume}");
        // Only the four sewn edges carry the gap; the body's tolerance and every other edge's
        // stay SMLib's own.
        let r = fixed.record();
        assert_eq!(r.tolerance, part.record().tolerance);
        let loose: Vec<f64> = r.edges.iter().map(|e| e.tolerance).filter(|t| *t > 0.01).collect();
        assert_eq!(loose.len(), 4, "{loose:?}");
        assert!(loose.iter().all(|t| *t >= 0.05 && *t <= 0.25), "{loose:?}");
        // A gap wider than asked for is not sewn: the free edges then bound holes.
        let err = repair(&part, &RepairOptions { gap: 0.01, patch_holes: false }).unwrap_err().to_string();
        assert!(err.contains("nothing sewn"), "{err}");
    }

    #[test]
    fn a_hole_is_offered_then_patched_flat_in_the_plane_around_it() {
        let open = open_box(&box_faces());
        let opts = RepairOptions { gap: 0.2, patch_holes: false };
        let err = repair(&open, &opts).unwrap_err().to_string();
        assert!(err.contains("a loop of 4 edges"), "{err}");
        // Planar walls around a planar loop: the patch is the loop's plane.
        let (fixed, report) = repair(&open, &RepairOptions { patch_holes: true, ..opts }).unwrap();
        let [hole] = report.holes.as_slice() else { panic!("{report:?}") };
        assert_eq!((hole.faces.len(), hole.edges.len(), hole.continuity, hole.patched), (4, 4, Continuity::G0, true), "{hole:?}");
        assert!(report.closed && report.modelling, "{report:?}");
        assert_eq!(report.faces, [5, 6]);
        assert!((measure(&fixed).unwrap().volume - 6000.0).abs() < 1e-6);
    }

    #[test]
    fn a_part_with_nothing_to_repair_is_refused_with_why() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let err = repair(&b, &RepairOptions { gap: 0.2, patch_holes: true }).unwrap_err().to_string();
        assert!(err.contains("nothing to repair"), "{err}");
        for gap in [0.0, -1.0, 2.0, f64::NAN, f64::INFINITY] {
            assert!(repair(&b, &RepairOptions { gap, patch_holes: false }).is_err(), "{gap}");
        }
    }
}
