//! The mesher: each face of a schema 4 record ([`brep::Brep`]) triangulated in its own
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
//! 3. a face even that refuses because its loops cross in (u, v) (a cusp, a sliver whose sides
//!    sag across each other) is still a defect upstream, in the record's pcurves or seams, and is
//!    named in the plan's stats ([`PlanStats::lenient`], with why it was refused) so it is
//!    reported and fixed there; it is filled leniently in (u, v) (the sides crossing one already
//!    kept are left out, see [`Lenient`]) so the display has no hole. A face that refuses even
//!    that (its loops do not close in (u, v), or wind round a seam or pole) is left out of the
//!    plan with no triangles and named in [`PlanStats::unmeshed`], and the rest of the body is
//!    meshed; there is no other mesher to fall back to. So is a face whose surface the kernel's
//!    evaluator does not take, or whose pcurves do not lie on its edges (named with the edge),
//!    and every face of an edge that cannot be sampled (each naming the edge). A body none of
//!    whose faces can be planned is the first face's error.

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
pub(crate) mod tangle;
pub(crate) mod uv;

use solidcore_convert as kernel;
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::vec::VecExt as _;
use crate::{FacePlan, TessPlan};
use solidcore::eval::EvalSurface;
use solidcore_record::Vec3;
use solidcore_record::brep::{self, Curve, Loop, Shell};

use crate::{Result, TessError};

/// A mesh of a face in its parameters: the nodes' (u, v) and the triangles over them.
pub(crate) type UvMesh = (Vec<[f64; 2]>, Vec<[u32; 3]>);

pub use size::MAX_ANGLE_BOUNDARY;

/// Whether [`plan_face`] may leave out of its constrained Delaunay the boundary sides that cross
/// a side already kept, and not hold the triangles to every side: the last resort for a face
/// that crosses itself in (u, v) (a cusp, a sliver whose sides sag across each other), so the
/// display has no hole. Its points stay in (u, v), so a curved face is still drawn on its
/// surface. Such a face is named in [`PlanStats::lenient`]: its pcurves or seams are wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lenient {
    No,
    Yes,
}

/// How the faces of a plan were meshed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanStats {
    /// Planar faces meshed whole as a box or four-cornered layout, or by Delaunay.
    pub planar: usize,
    /// Faces meshed on a structured layout: a box or four-cornered grid, patches or a cut grid.
    pub structured: usize,
    /// Curved faces meshed by constrained Delaunay triangulation.
    pub delaunay: usize,
    /// Faces every method refused, filled leniently in (u, v) (see [`Lenient`]) so the display
    /// has no hole: each still a defect upstream, in its pcurves or seams.
    pub lenient: Vec<RefusedFace>,
    /// Faces even the lenient fill refused, faces whose surface does not evaluate at a point
    /// the planner needs (a node, a middle it checks, a node of a cut line), and faces the
    /// planner could not read (a surface the evaluator does not take, a pcurve off its edge, a
    /// loop too short to bound anything, an edge that cannot be sampled): left out of the plan
    /// whole (no triangles), so the rest of the body is still meshed, each with why it could not
    /// be planned. Nothing is substituted for a point the surface refuses.
    pub unmeshed: Vec<RefusedFace>,
    /// Edges that bound no face and could not be sampled: left out of the plan (an empty
    /// polyline), so the rest of the body is still meshed, each with why.
    pub unsampled_edges: Vec<RefusedEdge>,
    /// Faces a memory bound of the planner limited: their grid thinned below the lines their
    /// tolerance asked for, or their sag refinement stopped at the points it may add. Each is
    /// still meshed, named here with the sag its triangles reach and the bounds that limited it,
    /// so a mesh that leaves the tolerance is never so unreported.
    pub over_tolerance: Vec<OverTolerance>,
    /// Edges whose sampling at a coarser facet angle, for a face crowded with segments, failed:
    /// each keeps its nodes at the rules' own level (valid, only finer), no face is refused, and
    /// the edge is named here with the coarser sampling's error.
    pub kept_strict: Vec<RefusedEdge>,
    /// Edges whose sampling stopped at a bound ([`EdgeCap`]) with a segment still wanting a
    /// split: their chords may lie off them by more than the tolerance (no refinement follows an
    /// edge's sampling), each named with the bound and how far its chords lie off.
    pub edges_capped: Vec<CappedEdge>,
    /// Poles of higher order (no normal at the point) whose limit normal the kernel refused
    /// for one of their triangles (a pole of order four or more, or another refusal): each is
    /// meshed on the planner's layout, its normal the area-weighted mean of the face's
    /// triangles' normals there, a display normal only, named here with the kernel's reason.
    /// The other poles of higher order get the exact limit normal per triangle
    /// ([`crate::PoleRule`]).
    pub averaged_poles: Vec<crate::AveragedPole>,
}

/// An edge a bound of its sampling stopped (see [`PlanStats::edges_capped`]).
#[derive(Clone, Debug, PartialEq)]
pub struct CappedEdge {
    /// The edge's index in the record.
    pub edge: usize,
    /// The bound: [`EdgeCap::Nodes`] where both were met.
    pub cap: EdgeCap,
    /// The farthest any of the edge's chords lies from its curve (mm), at their quarter points
    /// and middles (a sampled lower bound: exact where a chord's farthest point is its middle,
    /// as on a circle; below it at a corner between them). The segments exceeding their rules
    /// most are halved first, so at the node bound it is spread along the edge. A line's
    /// chords lie on it: 0.
    pub sag: f64,
    /// The most any of its chords exceeds its rules: the largest of its sag over the tolerance,
    /// its length over the side asked, its turn over a quarter turn and over the facet angle
    /// (past 1 for a rule broken; a line's, its sides' length over the side asked).
    pub excess: f64,
    /// The chord tolerance the edge was sampled to (mm).
    pub tol: f64,
    /// Whether `sag` is within `tol`: the chords follow the curve, and only a side's length or
    /// turn is past its rule (as at a corner the facet angle never settles at, or a line).
    pub within: bool,
}

/// A bound of an edge's sampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeCap {
    /// The edge has the most nodes an edge may have (`MAX_EDGE_NODES`, 20 000): for a line,
    /// its sides are longer than the side asked.
    Nodes,
    /// A segment was halved as often as one may be (24 times).
    Depth,
}

/// A face a memory bound limited (see [`PlanStats::over_tolerance`]).
#[derive(Clone, Debug, PartialEq)]
pub struct OverTolerance {
    /// The face's index in the record.
    pub face: usize,
    /// The farthest its triangles lie from its surface (mm), measured as the sag refinement
    /// measures them: at each triangle's middle and its sides' middles, along the surface's
    /// normal there, wherever the surface evaluates.
    pub sag: f64,
    /// The measurements `sag` could not make (one per middle per triangle, so a side shared by
    /// two triangles counts twice): the surface has no point there.
    pub unmeasured: usize,
    /// The chord tolerance the face was planned to (mm).
    pub tol: f64,
    /// Whether every middle was measured and `sag` is within `tol` after all: the refinement
    /// made up what a thinned grid left (for [`Cap::Refine`], which stops with a side still
    /// over, normally not, unless a flip after the refinement laid that side down).
    pub within: bool,
    /// The bounds that limited it, one or both.
    pub caps: Vec<Cap>,
}

/// A memory bound of the planner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    /// The face's grid was thinned to fit [`MAX_FACE_GRID`](grid::MAX_FACE_GRID) points: the
    /// lines along u and v its division gave, and those kept. `asked` is the division's own
    /// count, itself bounded (at most `MAX_LINES` lines, 20 halvings an interval); `kept` is before
    /// any further thinning a cut grid makes for its traces. The sag refinement's measure of the
    /// final triangles ([`OverTolerance::sag`]) covers both.
    GridThinned {
        /// The lines along u and v the division gave.
        asked: [usize; 2],
        /// The lines kept.
        kept: [usize; 2],
    },
    /// The sag refinement added all the points it may ([`MAX_REFINE`]) with triangles still off
    /// the surface by more than the tolerance.
    Refine,
}

/// An edge the planner could not sample: its index in the record and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedEdge {
    /// The edge's index in the record.
    pub edge: usize,
    /// Why it was refused.
    pub reason: String,
}

/// A parameter point where a face's surface does not evaluate: the kernel's evaluator refuses
/// the face there. Nothing is substituted for the point; the face is left out of the plan whole
/// and named in [`PlanStats::unmeshed`] with the point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NoValue(pub [f64; 2]);

/// A parameter point whose poles the kernel's classifier cannot tell
/// ([`solidcore::eval::uv::degenerate_within`]: not finite, off the surface's net, a weight not
/// positive), with the classifier's reason. Nothing is taken for "not a pole" there: the face is
/// refused, named with the point and the reason.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Unclassified {
    pub at: [f64; 2],
    pub why: String,
}

impl std::fmt::Display for Unclassified {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the surface's poles cannot be told at {:?}: {}",
            self.at, self.why
        )
    }
}

/// Whether `s` is collapsed along `d` at `q`, to within `reach` (a pole, or a patch side
/// collapsed to within a tolerance), as the kernel's classifier says; its refusal the error.
pub(crate) fn pole_at(
    s: &EvalSurface,
    q: [f64; 2],
    d: usize,
    reach: f64,
) -> std::result::Result<bool, Unclassified> {
    solidcore::eval::uv::degenerate_within(s, q, d, reach).map_err(|e| Unclassified {
        at: q,
        why: crate::eval_message(e),
    })
}

/// The first direction (u, then v) `s` is collapsed along at `q` ([`pole_at`]), if any.
pub(crate) fn pole_dir(
    s: &EvalSurface,
    q: [f64; 2],
    reach: f64,
) -> std::result::Result<Option<usize>, Unclassified> {
    for d in 0..2 {
        if pole_at(s, q, d, reach)? {
            return Ok(Some(d));
        }
    }
    Ok(None)
}

/// Run a layout whose own refusals are `None` (it does not fit the face) and which also stops
/// where a check it makes fails (a point the surface does not evaluate at): `body` records that
/// error in its slot with [`stop`] and returns `None`, and the error comes out as this `Err`.
pub(crate) fn layout<T, E>(
    body: impl FnOnce(&mut Option<E>) -> Option<T>,
) -> std::result::Result<Option<T>, E> {
    let mut stopped = None;
    let out = body(&mut stopped);
    match stopped {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

/// `r`'s value, or `None` with its error recorded in `slot` for [`layout`].
pub(crate) fn stop<T, E>(slot: &mut Option<E>, r: std::result::Result<T, E>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            *slot = Some(e);
            None
        }
    }
}

/// The value of a result that cannot fail.
pub(crate) fn infallible<T>(r: std::result::Result<T, std::convert::Infallible>) -> T {
    match r {
        Ok(v) => v,
        Err(never) => match never {},
    }
}

/// Why [`plan_face`] refused a face.
#[derive(Debug)]
enum FaceError {
    /// The layout could not mesh the face: the lenient fill is tried next.
    Layout(TessError),
    /// The surface does not evaluate where the face needs it: no layout can mesh it, so the face
    /// is left out without the lenient fill.
    Refused(TessError),
}

impl From<TessError> for FaceError {
    fn from(e: TessError) -> Self {
        FaceError::Layout(e)
    }
}

/// A face a method refused: its index in the record and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedFace {
    /// The face's index in the record.
    pub face: usize,
    /// Why it was refused.
    pub reason: String,
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
/// Most nodes of one face, its boundary's and those its layout and refinement add: every index
/// of them, and of its triangles (fewer than twice as many), fits a u32.
pub(crate) const MAX_FACE_NODES: usize = 1 << 28;
/// Most points Ruppert refinement may add to one face, beyond its seeds.
const MAX_REFINE: usize = 20_000;
/// Most rounds of splitting the edge segments where a face's sampled boundary crosses itself:
/// each round halves them, so their chords sag a quarter as far.
const MAX_UNTANGLE: u32 = 16;

fn unsupported(what: impl std::fmt::Display) -> TessError {
    TessError::Failed(format!("not supported yet: tessellation: {what}"))
}

/// The surface kind of a face, for an error that names it.
fn kind(s: &brep::Surface) -> &'static str {
    match s {
        brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. } => "plane",
        brep::Surface::Cylinder { .. } => "cylinder",
        brep::Surface::Cone { .. } => "cone",
        brep::Surface::Sphere { .. } => "sphere",
        brep::Surface::BallPatch { .. } => "ball patch",
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

/// Plan the display tessellation of `b`, a body of size `size` (its box diagonal), at the
/// tolerance `t` ([`MeshTolerance::rules`]): faces and edges are the record's, in its order.
pub fn plan(b: &brep::Brep, t: &crate::MeshTolerance, size: f64) -> Result<(TessPlan, PlanStats)> {
    let (tol, turn) = t.rules(size);
    plan_at(b, tol, turn)
}

/// Plan the tessellation of `b` with chord tolerance `tol` and, when given, a facet angle no
/// facet may turn more than (where it sags above the angle's floor). Faces and edges are the
/// record's, in its order.
fn plan_at(b: &brep::Brep, tol: f64, turn: Option<size::Turn>) -> Result<(TessPlan, PlanStats)> {
    let faces = &b.faces;
    // The plan indexes faces and edges as u32.
    if u32::try_from(faces.len()).is_err() || u32::try_from(b.edges.len()).is_err() {
        return Err(unsupported(format!(
            "{} faces and {} edges are more than a plan indexes",
            faces.len(),
            b.edges.len()
        )));
    }
    let forward = forward_faces(b);
    // The faces each edge bounds, each once: the faces whose loops run it, in the order of the
    // edge's uses, then those its uses do not list (an unvalidated record). A face the uses list
    // but whose loops do not run the edge is not its (a stale use): an edge that cannot be
    // sampled refuses every face that runs it, and only those.
    let mut runs: Vec<Vec<usize>> = vec![Vec::new(); b.edges.len()];
    for (fi, f) in faces.iter().enumerate() {
        for oe in face_edges(f) {
            if let Some(fs) = runs.get_mut(oe.edge as usize)
                && !fs.contains(&fi)
            {
                fs.push(fi);
            }
        }
    }
    let edge_faces: Vec<Vec<usize>> = b
        .edges
        .iter()
        .zip(&runs)
        .map(|(e, run)| {
            // Proven: capacity of the allocated `run`'s length, the same element type.
            let mut fs: Vec<usize> = Vec::with_capacity(run.len());
            for u in &e.uses {
                let f = u.face as usize;
                if run.contains(&f) && !fs.contains(&f) {
                    fs.push(f);
                }
            }
            for &f in run {
                if !fs.contains(&f) {
                    fs.push(f);
                }
            }
            fs
        })
        .collect();
    let rules = size::Rules::new(
        tol,
        size::extent(
            b.vertices
                .iter()
                .map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])),
        ),
        turn,
    );
    let mut refused = Refused::new(b);
    // Each face's surface and target side length, then every edge once at the smaller size of
    // its faces; faces then read their nodes. A planar face costs microseconds and a curved
    // one about a hundred, so the pool is used only where curved faces (or intersection
    // curves) make up the work. A face refused at one step (its surface, an edge of it, its
    // boundary) is passed over by the steps after, and left out of the plan, named.
    let mut evals: Vec<Option<EvalSurface>> = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        let s = match surface::eval_surface(f.parameters()) {
            Ok(s) => Some(s),
            Err(e) => {
                refused.face(
                    fi,
                    unsupported(format!("face {fi} ({}): {e}", kind(&f.surface))),
                );
                None
            }
        };
        evals.push(s);
    }
    let curved = evals
        .iter()
        .flatten()
        .filter(|e| !matches!(e, EvalSurface::Plane { .. }))
        .count();
    let sizes: Vec<Option<size::FaceSize>> =
        crate::par::map("face sizes", &evals, curved >= PARALLEL_SIZES, |s| {
            Ok(s.as_ref()
                .map(|s| size::face_size(s, &solidcore::eval::uv::domain(s), &rules)))
        })
        .into_iter()
        .enumerate()
        .map(|(fi, r)| refused.take(fi, r).flatten())
        .collect();
    let face_h = |f: usize| sizes.get(f).and_then(Option::as_ref).map(|s| s.h);
    let edge_h: Vec<f64> = edge_faces
        .iter()
        .map(|fs| {
            fs.iter()
                .filter_map(|&f| face_h(f))
                .fold(rules.max_len, f64::min)
        })
        .collect();
    let jobs: Vec<(usize, f64)> = edge_h.into_iter().enumerate().collect();
    let solved = b
        .edges
        .iter()
        .filter(|e| matches!(e.curve, Curve::SurfaceIntersection { .. }))
        .count();
    // An edge that cannot be sampled refuses every face it bounds, each naming it, and has no
    // nodes in the plan; one that bounds no face is named in the stats on its own.
    let mut nodes: Vec<edges::Nodes> = Vec::new();
    let mut unsampled_edges: Vec<RefusedEdge> = Vec::new();
    for (ei, r) in crate::par::map(
        "edge nodes",
        &jobs,
        solved >= PARALLEL_CURVES || jobs.len() >= PARALLEL_EDGES,
        |(e, h)| edges::sample_edge(b, *e, &rules, *h),
    )
    .into_iter()
    .enumerate()
    {
        nodes.push(match r {
            Ok(n) => n,
            Err(e) if edge_faces.get(ei).is_none_or(Vec::is_empty) => {
                unsampled_edges.push(RefusedEdge {
                    edge: ei,
                    reason: e.to_string(),
                });
                edges::Nodes::default()
            }
            Err(e) => {
                refused.edge(&edge_faces, ei, &e);
                edges::Nodes::default()
            }
        });
    }
    let kept_strict = relax_crowded_faces(b, &jobs, &rules, &mut nodes, &refused.flags());
    for (ei, e) in split_thin_loops(b, &mut nodes, &refused.flags()) {
        refused.edge(&edge_faces, ei, &e);
    }
    let indexed: Vec<usize> = (0..faces.len()).collect();
    let face_boundary = |fi: usize, nodes: &[edges::Nodes]| {
        let (Some(face), Some(eval)) = (faces.get(fi), evals.get(fi).and_then(Option::as_ref))
        else {
            return Err(unsupported(format!("face {fi}: missing")));
        };
        boundary(b, fi, face, eval, nodes, &rules)
    };
    let skip = refused.flags();
    let mut boundaries: Vec<Boundary> = crate::par::map(
        "face boundaries",
        &indexed,
        curved >= PARALLEL_FACES,
        |&fi| {
            if skip.get(fi).copied().unwrap_or(true) {
                return Ok(Boundary::default());
            }
            face_boundary(fi, &nodes)
        },
    )
    .into_iter()
    .enumerate()
    .map(|(fi, r)| refused.take(fi, r).unwrap_or_default())
    .collect();
    let tangled = untangle(
        b,
        &evals,
        &edge_faces,
        rules.tol,
        &mut nodes,
        &mut boundaries,
        &face_boundary,
        &mut refused,
    );
    let why = refused.why;
    let planned = crate::par::map(
        "face tessellation",
        &indexed,
        curved >= PARALLEL_FACES,
        |&fi| {
            if let Some(Some(e)) = why.get(fi) {
                return Err(e.clone());
            }
            let (Some(face), Some(eval), Some(size), Some(fb)) = (
                faces.get(fi),
                evals.get(fi).and_then(Option::as_ref),
                sizes.get(fi).and_then(Option::as_ref),
                boundaries.get(fi),
            ) else {
                return Err(unsupported(format!("face {fi}: missing")));
            };
            let refused = tangled
                .iter()
                .find(|(f, _)| *f == fi)
                .map(|(_, why)| why.as_str());
            // The plan indexes faces as u32, checked for the body above.
            let index = u32::try_from(fi).map_err(|_| unsupported("too many faces"))?;
            let (fp, stats) = plan_or_fill(
                index,
                kind(face.parameters()),
                eval,
                &fb.loops,
                &rules,
                size,
                fb.reach,
                refused,
            )?;
            let fwd = forward.get(fi).copied().unwrap_or(true);
            let fp = FacePlan {
                flip: !fwd,
                triangles: if fwd {
                    fp.triangles
                } else {
                    fp.triangles
                        .into_iter()
                        .map(|[a, b, c]| [a, c, b])
                        .collect()
                },
                ..fp
            };
            // Its poles of higher order and their normals, on the oriented triangles.
            let (poles, averaged) = crate::pole::plan_poles(fi, eval, &fp, fb.reach);
            let mut stats = stats;
            stats.averaged_poles = averaged;
            Ok((FacePlan { poles, ..fp }, stats))
        },
    );
    let mut plan = TessPlan::default();
    let edges_capped: Vec<CappedEdge> = nodes
        .iter()
        .enumerate()
        .filter_map(|(edge, n)| {
            n.capped.map(|(cap, sag, excess)| CappedEdge {
                edge,
                cap,
                sag,
                excess,
                tol: rules.tol,
                within: sag <= rules.tol,
            })
        })
        .collect();
    let mut stats = PlanStats {
        unsampled_edges,
        kept_strict,
        edges_capped,
        ..PlanStats::default()
    };
    // A face no method plans is left out, named in the stats, and the rest of the body is
    // meshed; a body none of whose faces plan is the first face's error.
    let mut first_error: Option<TessError> = None;
    let too_many = || unsupported("too many faces");
    for (fi, (r, eval)) in planned.into_iter().zip(evals).enumerate() {
        let surface = match eval {
            Some(eval) => {
                let i = u32::try_from(plan.surfaces.len()).map_err(|_| too_many())?;
                plan.surfaces.push(eval);
                Some(i)
            }
            None => None,
        };
        let face = u32::try_from(fi).map_err(|_| too_many())?;
        let (fp, s) = match r {
            Ok(done) => done,
            Err(e) => {
                // A refusal from before is named already; the planner's own may not be.
                let e = if why.get(fi).is_some_and(Option::is_some) {
                    e
                } else {
                    named(b, fi, e)
                };
                stats.unmeshed.push(RefusedFace {
                    face: fi,
                    reason: e.to_string(),
                });
                first_error.get_or_insert(e);
                (
                    FacePlan {
                        face,
                        ..FacePlan::default()
                    },
                    PlanStats::default(),
                )
            }
        };
        plan.faces.push(FacePlan { surface, ..fp });
        stats.planar += s.planar;
        stats.structured += s.structured;
        stats.delaunay += s.delaunay;
        stats.lenient.extend(s.lenient);
        stats.over_tolerance.extend(s.over_tolerance);
        stats.averaged_poles.extend(s.averaged_poles);
    }
    if let Some(e) = first_error
        && stats.unmeshed.len() == faces.len()
    {
        return Err(e);
    }
    plan.edges = nodes.into_iter().map(|n| n.points).collect();
    plan.edge_faces = edge_faces
        .into_iter()
        .map(|fs| {
            fs.into_iter()
                .map(|f| u32::try_from(f).map_err(|_| too_many()))
                .collect()
        })
        .collect::<Result<_>>()?;
    Ok((plan, stats))
}

/// The first reason each face of a body is refused, as the planner meets them (its surface, an
/// edge of it, its boundary): a face refused at one step is passed over by the steps after, and
/// left out of the plan, named in [`PlanStats::unmeshed`].
struct Refused<'a> {
    b: &'a brep::Brep,
    why: Vec<Option<TessError>>,
}

impl<'a> Refused<'a> {
    fn new(b: &'a brep::Brep) -> Self {
        Refused {
            b,
            // Proven: one slot per face of the allocated `b.faces`, each no larger than a face.
            why: vec![None; b.faces.len()],
        }
    }

    /// Which faces are refused, by index.
    fn flags(&self) -> Vec<bool> {
        self.why.iter().map(Option::is_some).collect()
    }

    /// Whether face `fi` is refused.
    fn is(&self, fi: usize) -> bool {
        self.why.get(fi).is_some_and(Option::is_some)
    }

    /// Refuse face `fi` with `e`, unless it is refused already.
    fn face(&mut self, fi: usize, e: TessError) {
        if let Some(slot @ None) = self.why.get_mut(fi) {
            *slot = Some(named(self.b, fi, e));
        }
    }

    /// Refuse every face edge `ei` bounds with the edge's error `e`, each naming itself.
    fn edge(&mut self, edge_faces: &[Vec<usize>], ei: usize, e: &TessError) {
        for &fi in edge_faces.get(ei).into_iter().flatten() {
            let kind = self.b.faces.get(fi).map_or("?", |f| kind(&f.surface));
            self.face(fi, on_face(fi, kind, e));
        }
    }

    /// `r`'s value, or `None` with face `fi` refused with its error.
    fn take<T>(&mut self, fi: usize, r: Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.face(fi, e);
                None
            }
        }
    }
}

/// `e` as face `fi`'s (of kind `kind`), its message naming the face.
fn on_face(fi: usize, kind: &str, e: &TessError) -> TessError {
    let name = |m: &str| format!("face {fi} ({kind}): {m}");
    match e {
        TessError::Invalid(m) => TessError::Invalid(name(m)),
        TessError::Failed(m) => TessError::Failed(name(m)),
        TessError::Internal(m) => TessError::Internal(name(m)),
    }
}

/// Face `fi`'s error `e`, naming the face where it does not already (a panic caught on the
/// worker pool names only the step).
fn named(b: &brep::Brep, fi: usize, e: TessError) -> TessError {
    match e {
        TessError::Internal(_) => {
            let kind = b.faces.get(fi).map_or("?", |f| kind(&f.surface));
            on_face(fi, kind, &e)
        }
        other => other,
    }
}

/// Face `fi`'s boundary, loop by loop: the shared edge nodes (each edge's last node is the next
/// edge's first) and their parameters on the face's surface, from each edge use's exact pcurve
/// at the node's curve parameter. A seam edge's two uses on one face have their own pcurves, so
/// its two sides are told apart. A pcurve runs on continuously; where one edge meets the next, a
/// pcurve that starts a whole period along from where the previous one ended is moved back by
/// it, except in a direction the meeting point is a pole of (or lies on a patch side collapsed
/// to within its vertex's tolerance, see [`Boundary::reach`]): there the parameter is free, and
/// the two pcurves' own values are where the face's boundary runs along the pole from and to
/// (a meridian revolved half a turn meets the other at the pole half a period away; unwrapped,
/// its loop would wind round the surface). Each loop then starts at a node off every pole whose
/// predecessor is off them too, so the step from its last node to its first, which
/// [`seam::drift`] takes as the nearest copy, never crosses a pole. A vertex loop (a pole)
/// bounds no area and has no nodes of its own. Where the face's loops pass a vertex more than once
/// (two holes touching at a point, a loop through another's corner), the pcurves of the edges from
/// it give its parameters apart by their rounding, and two nodes 1e-14 apart put crossing
/// constraints into the triangulation: a pass whose parameters agree with the first pass's to
/// that rounding (1e-9 relative) takes the first's. Farther apart they are two places of the
/// surface (a seam's two sides, a closed spline's ends). A pcurve that does not cover its edge's
/// interval, or puts a node farther off the edge than the tolerances allow, is an error naming
/// the face and edge: the record is wrong there.
fn boundary(
    b: &brep::Brep,
    fi: usize,
    face: &brep::Face,
    s: &EvalSurface,
    nodes: &[edges::Nodes],
    rules: &size::Rules,
) -> Result<Boundary> {
    let dom = solidcore::eval::uv::domain(s);
    let fail = |e: u32, why: String| {
        unsupported(format!(
            "face {fi} ({}): edge {e}: {why}",
            kind(&face.surface)
        ))
    };
    let unclassified =
        |u: Unclassified| unsupported(format!("face {fi} ({}): {u}", kind(&face.surface)));
    // How near a pole a node is one: the tolerance of a vertex of the face that lies on a patch
    // side collapsed to within it, not to a point, as the record check takes a loop's pole; at
    // most the chord tolerance, finer than which the mesh resolves nothing (a vertex tolerance
    // as large as the surface's features would make a pole of every node near one).
    let mut reach: f64 = 0.0;
    for oe in face_edges(face) {
        let (Some(edge), Some([p0, p1])) = (b.edges.get(oe.edge as usize), oe.pcurve.interval())
        else {
            continue;
        };
        let tol = edge
            .vertices
            .get(usize::from(!oe.forward))
            .and_then(|v| b.vertices.get(*v as usize))
            .map_or(0.0, |v| v.tolerance.min(rules.tol));
        let Some(q) = kernel::curve2(&oe.pcurve).eval(if oe.forward { p0 } else { p1 }) else {
            continue;
        };
        if tol.is_finite() && tol > reach && pole_dir(s, q, tol).map_err(unclassified)?.is_some() {
            reach = tol;
        }
    }
    let pole_along = |q: [f64; 2], d: usize| pole_at(s, q, d, reach).map_err(unclassified);
    let too_many = || {
        unsupported(format!(
            "face {fi} ({}): its {} loops cannot be held",
            kind(&face.surface),
            face.loops.len()
        ))
    };
    let mut out = Boundary {
        loops: crate::with_room(face.loops.len()).ok_or_else(too_many)?,
        segments: crate::with_room(face.loops.len()).ok_or_else(too_many)?,
        reach,
    };
    // Each vertex's parameters, from the first edge use of the face to start there.
    let mut at_vertex: HashMap<u32, [f64; 2]> = HashMap::new();
    // The vertex pairs an edge of the face joins.
    let joined: std::collections::HashSet<(u32, u32)> = face_edges(face)
        .filter_map(|oe| b.edges.get(oe.edge as usize))
        .map(|e| {
            (
                e.vertices[0].min(e.vertices[1]),
                e.vertices[0].max(e.vertices[1]),
            )
        })
        .collect();
    let vertex_point = |v: u32| {
        b.vertices.get(v as usize).map(|x| {
            (
                Vec3::new(x.point[0], x.point[1], x.point[2]),
                x.tolerance.max(0.0),
            )
        })
    };
    // Each loop's nodes that are another edge's node (see [`edges::snap`]).
    // Proven: capacity of allocated `face.loops`' length; each loop holds a Vec, as large.
    let mut snapped: Vec<Vec<bool>> = Vec::with_capacity(face.loops.len());
    for (li, l) in face.loops.iter().enumerate() {
        let Loop::Edges { edges } = l else { continue };
        let (mut pts, mut uvs): (Vec<Vec3>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
        let mut segs: Vec<(u32, u32)> = Vec::new();
        let mut shared: Vec<bool> = Vec::new();
        // Where the previous edge ended, and the periods its pcurve was moved by.
        let (mut end, mut shift): (Option<[f64; 2]>, [f64; 2]) = (None, [0.0; 2]);
        for oe in edges {
            let n = nodes
                .get(oe.edge as usize)
                .ok_or_else(|| fail(oe.edge, "no nodes".into()))?;
            // An edge without nodes (it could not be sampled) leaves a gap in the loop that a
            // chord would close: the face is refused, never meshed across it. Defensive: such an
            // edge has already refused every face that runs it, and sampling gives at least two.
            if n.points.len() < 2 || n.params.len() != n.points.len() {
                return Err(fail(
                    oe.edge,
                    format!("{} nodes: it was not sampled", n.points.len()),
                ));
            }
            let edge = b
                .edges
                .get(oe.edge as usize)
                .ok_or_else(|| fail(oe.edge, "no such edge".into()))?;
            let [t0, t1] = edge.interval;
            let [p0, p1] = oe
                .pcurve
                .interval()
                .ok_or_else(|| fail(oe.edge, "a malformed pcurve".into()))?;
            let slack = 1e-9 * (t1 - t0).abs().max(t0.abs()).max(t1.abs()).max(1.0);
            if (p0 - t0).abs() > slack || (p1 - t1).abs() > slack {
                return Err(fail(
                    oe.edge,
                    format!("its pcurve spans [{p0}, {p1}], the edge [{t0}, {t1}]"),
                ));
            }
            if !matches!(p0.partial_cmp(&p1), Some(Ordering::Less | Ordering::Equal)) {
                return Err(fail(
                    oe.edge,
                    format!("its pcurve's interval [{p0}, {p1}] is reversed or not a number"),
                ));
            }
            // The edge's own tolerance: how far its curve may stand off the face.
            let accept = (rules.tol * 10.0).max(1e-6) + edge.tolerance.max(0.0);
            let mut ep: Vec<Vec3> = n.points.clone();
            let mut es: Vec<bool> = n.params.iter().map(|t| n.snapped.contains(t)).collect();
            // Proven: capacity of allocated `ep`'s length, its elements (24 bytes) larger.
            let mut eq: Vec<[f64; 2]> = Vec::with_capacity(ep.len());
            let pcurve = kernel::curve2(&oe.pcurve);
            for (k, (t, p)) in n.params.iter().zip(&n.points).enumerate() {
                // Proven: p0 <= p1, neither NaN, checked above.
                let q = pcurve.eval(t.clamp(p0, p1)).ok_or_else(|| {
                    fail(oe.edge, format!("its pcurve does not evaluate at t = {t}"))
                })?;
                let on = s
                    .derivs(q[0], q[1])
                    .map(kernel::record_triple)
                    .map(|d| d.0)
                    .ok_or_else(|| {
                        fail(
                            oe.edge,
                            format!("node {k}: the surface does not evaluate at {q:?}"),
                        )
                    })?;
                if !matches!(
                    on.dist(*p).partial_cmp(&accept),
                    Some(Ordering::Less | Ordering::Equal)
                ) {
                    return Err(fail(
                        oe.edge,
                        format!(
                            "node {k}: its pcurve is {} mm off the edge at t = {t}",
                            on.dist(*p)
                        ),
                    ));
                }
                eq.push(q);
            }
            if !oe.forward {
                ep.reverse();
                eq.reverse();
                es.reverse();
            }
            if let (Some(e), Some(&start)) = (end, eq.first()) {
                // Proven: `d` < 2 indexes the `[_; 2]` shift, periods and parameters.
                #[allow(clippy::indexing_slicing)]
                for d in 0..2 {
                    let Some(per) = dom.period[d] else { continue };
                    if !(pole_along(e, d)? || pole_along(start, d)?) {
                        shift[d] = per * ((e[d] - start[d]) / per).round();
                    }
                }
            }
            for q in &mut eq {
                q[0] += shift[0];
                q[1] += shift[1];
            }
            if let (Some(&v), Some(q)) =
                (edge.vertices.get(usize::from(!oe.forward)), eq.first_mut())
            {
                if let Some(first) = at_vertex.get(&v) {
                    if first
                        .iter()
                        .zip(q.iter())
                        .all(|(f, x)| (f - x).abs() <= 1e-9 * f.abs().max(x.abs()).max(1.0))
                    {
                        *q = *first;
                    }
                } else {
                    // Another vertex of the face within the tolerance of both (and of the
                    // chord tolerance), and not joined to this one by an edge (which makes
                    // them two points however near: a step that small), is the same point:
                    // the record's corners a hair apart (a planar boolean's, where a cut-out's
                    // corner touches another loop's corner) are one point of the face, so its
                    // loops touch there instead of crossing.
                    let same = vertex_point(v).and_then(|(p, tv)| {
                        at_vertex
                            .iter()
                            .filter(|(w, _)| {
                                **w != v && !joined.contains(&(v.min(**w), v.max(**w)))
                            })
                            .find_map(|(w, wq)| {
                                let (pw, tw) = vertex_point(*w)?;
                                (p.dist(pw) <= tv.max(tw).min(rules.tol)).then_some(*wq)
                            })
                    });
                    // The vertex's parameters taken only where neither it nor the other lies on a
                    // pole.
                    let off_poles = |w: [f64; 2]| -> Result<bool> {
                        for d in 0..2 {
                            if pole_along(w, d)? || pole_along(*q, d)? {
                                return Ok(false);
                            }
                        }
                        Ok(true)
                    };
                    let same = match same {
                        Some(w) if off_poles(w)? => Some(w),
                        _ => None,
                    };
                    if let Some(mut w) = same {
                        for ((x, y), per) in w.iter_mut().zip(q.iter()).zip(dom.period) {
                            *x = solidcore::eval::uv::unwrap(*x, *y, per);
                        }
                        *q = w;
                    }
                    at_vertex.insert(v, *q);
                }
            }
            end = eq.last().copied();
            ep.pop();
            eq.pop();
            es.pop();
            // The segment from each node on: the edge's own, counted from its start.
            // Proven: an edge has at most MAX_EDGE_NODES (20 000) nodes (sample_edge, split_segments
            // and snap keep within it): the count fits a u32.
            let m = ep.len() as u32;
            segs.extend((0..m).map(|j| (oe.edge, if oe.forward { j } else { m - 1 - j })));
            pts.extend(ep);
            uvs.extend(eq);
            shared.extend(es);
        }
        let n = uvs.len();
        // Whether each node is on a pole.
        let mut on_pole: Vec<bool> = Vec::new();
        for q in &uvs {
            on_pole.push(pole_dir(s, *q, reach).map_err(unclassified)?.is_some());
        }
        let off = |k: usize| on_pole.get(k).is_some_and(|p| !p);
        // A node off the poles whose predecessor is off them too; failing that (a whole sphere's
        // seam in two segments: pole, equator, pole), one whose predecessor is a pole on the same
        // edge, so the step into it is the edge's own and the step into that pole, from the
        // edge before, is in the loop where it is unwrapped like any other.
        let same_edge = |k: usize| {
            (k + n - 1)
                .checked_rem(n)
                .and_then(|i| segs.get(i))
                .zip(segs.get(k))
                .is_some_and(|(a, b)| a.0 == b.0)
        };
        let start = (0..n)
            // Proven: n > 0 inside `0..n`.
            .find(|&k| off(k) && off((k + n - 1) % n))
            .or_else(|| (0..n).find(|&k| off(k) && same_edge(k)));
        if let Some(k) = start {
            // Proven: k < n = uvs.len(), and each edge adds at least as many points, segments
            // and snap flags as parameters (`eq` follows `ep`'s nodes, one each, and all pop one).
            uvs.rotate_left(k);
            pts.rotate_left(k);
            segs.rotate_left(k);
            shared.rotate_left(k);
        }
        // A loop of edges that leaves fewer than two nodes bounds nothing: the record is wrong
        // there, and the face is refused rather than meshed without it.
        if pts.len() < 2 {
            return Err(unsupported(format!(
                "face {fi} ({}): loop {li} has {} node{} ({} edge uses)",
                kind(&face.surface),
                pts.len(),
                if pts.len() == 1 { "" } else { "s" },
                edges.len()
            )));
        }
        out.loops.push(seam::Loop { uv: uvs, pts });
        out.segments.push(segs);
        snapped.push(shared);
    }
    // A node snapped from another edge is that edge's node, the same point: where the face runs
    // that edge too, the node takes the other's parameters, so the face's boundary passes one
    // point of its (u, v) twice (a pinch) and does not cross itself there. Its own pcurve puts it
    // within the chord tolerance of that point, not on it.
    for (l, flags) in snapped.iter().enumerate() {
        for (k, _) in flags.iter().enumerate().filter(|(_, f)| **f) {
            let Some((&p, &q)) = out
                .loops
                .get(l)
                .and_then(|lp| lp.pts.get(k).zip(lp.uv.get(k)))
            else {
                continue;
            };
            let partner = out
                .loops
                .iter()
                .zip(&snapped)
                .flat_map(|(lp, fs)| lp.pts.iter().zip(&lp.uv).zip(fs))
                .filter(|((r, _), f)| !**f && **r == p)
                .map(|((_, w), _)| *w)
                .min_by(|a, b| {
                    (a[0] - q[0])
                        .hypot(a[1] - q[1])
                        .total_cmp(&(b[0] - q[0]).hypot(b[1] - q[1]))
                });
            if let (Some(mut w), Some(slot)) = (
                partner,
                out.loops.get_mut(l).and_then(|lp| lp.uv.get_mut(k)),
            ) {
                for ((x, y), per) in w.iter_mut().zip(q).zip(dom.period) {
                    if per.is_some() {
                        *x = solidcore::eval::uv::unwrap(*x, y, per);
                    }
                }
                *slot = w;
            }
        }
    }
    Ok(out)
}

/// A face's boundary loops, and for each loop node the edge segment that starts there: the
/// edge, and the segment's place along it from its start vertex. `reach`: how near a pole a
/// point of the face is taken as one (see [`solidcore::eval::uv::degenerate_within`]): the tolerance of a vertex
/// of it on a patch side collapsed to within that tolerance, else 0.
#[derive(Default)]
struct Boundary {
    loops: Vec<seam::Loop>,
    segments: Vec<Vec<(u32, u32)>>,
    reach: f64,
}

/// Edges are sampled to the chord tolerance each on its own, so where two curves of one face
/// run closer than their chords sag (the two arcs of a thin lune, a sliver cap between two
/// cylinders), the chords may cross although the curves do not, and no triangulation follows
/// such a boundary. The edge segments whose chords cross another of the face (in its loop or
/// another, see [`tangle::crossing`]) are halved at their curve parameter's middle, on the edge,
/// so every face of the edge gets the new nodes; the faces of every edge split are checked again,
/// until no chords cross. Boundaries that still cross after [`MAX_UNTANGLE`] rounds, or would
/// put more than [`edges::MAX_EDGE_NODES`] on an edge, refuse every face of that edge (in
/// `refused`), as does a boundary that cannot be built again; a refused face is passed over, its
/// boundary emptied.
#[allow(clippy::too_many_arguments)]
fn untangle(
    b: &brep::Brep,
    evals: &[Option<EvalSurface>],
    edge_faces: &[Vec<usize>],
    tol: f64,
    nodes: &mut [edges::Nodes],
    boundaries: &mut [Boundary],
    face_boundary: &(dyn Fn(usize, &[edges::Nodes]) -> Result<Boundary> + Sync),
    refused: &mut Refused,
) -> Vec<(usize, String)> {
    let crossing = |fi: usize, fb: &Boundary| -> Result<Vec<(u32, u32)>> {
        let Some(s) = evals.get(fi).and_then(Option::as_ref) else {
            return Ok(Vec::new());
        };
        Ok(tangle::crossing(s, fb.reach, &fb.loops)
            .map_err(|u| unclassified_face(b, fi, &u))?
            .into_iter()
            .filter_map(|(l, k)| fb.segments.get(l)?.get(k).copied())
            .collect())
    };
    let indexed: Vec<usize> = (0..boundaries.len()).collect();
    let curved = evals
        .iter()
        .flatten()
        .filter(|e| !matches!(e, EvalSurface::Plane { .. }))
        .count();
    let found: Vec<Option<Vec<(u32, u32)>>> = crate::par::map(
        "boundary crossings",
        &indexed,
        curved >= PARALLEL_FACES,
        |&fi| {
            boundaries
                .get(fi)
                .map_or_else(|| Ok(Vec::new()), |fb| crossing(fi, fb))
        },
    )
    .into_iter()
    .enumerate()
    .map(|(fi, r)| refused.take(fi, r))
    .collect();
    let mut tangled: Vec<Tangled> = found
        .into_iter()
        .enumerate()
        .filter_map(|(fi, c)| Some((fi, c?)))
        .filter(|(_, c)| !c.is_empty())
        .collect();
    for round in 0..=MAX_UNTANGLE {
        tangled.retain(|(fi, _)| !refused.is(*fi));
        if tangled.is_empty() {
            return Vec::new();
        }
        if round == MAX_UNTANGLE {
            // Halving cannot part two curves that touch or cross: where they come within the chord
            // tolerance of each other, they share nodes.
            let faces: Vec<usize> = tangled.iter().map(|(fi, _)| *fi).collect();
            tangled = snap_touches(
                evals,
                edge_faces,
                tol,
                nodes,
                boundaries,
                face_boundary,
                &faces,
                refused,
            );
            // Each face still crossing, with why: planned leniently, and named (`plan_or_fill`).
            return tangled
                .iter()
                .map(|(fi, c)| {
                    let mut es: Vec<u32> = c.iter().map(|(e, _)| *e).collect();
                    es.sort_unstable();
                    es.dedup();
                    let kind = b.faces.get(*fi).map_or("?", |f| kind(&f.surface));
                    (*fi, format!("face {fi} ({kind}): boundary crosses itself: edges {es:?} run closer together than their chords can follow after {MAX_UNTANGLE} halvings"))
                })
                .collect();
        }
        let mut split: Vec<(u32, u32)> = tangled
            .iter()
            .flat_map(|(_, c)| c.iter().copied())
            .collect();
        split.sort_unstable();
        split.dedup();
        let mut redo: Vec<usize> = Vec::new();
        for run in split.chunk_by(|a, c| a.0 == c.0) {
            let Some(&(e, _)) = run.first() else { continue };
            let segs: Vec<u32> = run.iter().map(|(_, k)| *k).collect();
            let Some(slot) = nodes.get_mut(e as usize) else {
                continue;
            };
            if let Err(err) = edges::split_segments(b, e as usize, slot, &segs) {
                refused.edge(edge_faces, e as usize, &err);
            }
            redo.extend(edge_faces.get(e as usize).into_iter().flatten().copied());
        }
        redo.sort_unstable();
        redo.dedup();
        tangled.clear();
        for fi in redo {
            if let Some(c) = rebuild(fi, evals, nodes, boundaries, face_boundary, refused) {
                let c: Vec<(u32, u32)> = boundaries
                    .get(fi)
                    .map(|fb| {
                        c.into_iter()
                            .filter_map(|(l, k)| fb.segments.get(l)?.get(k).copied())
                            .collect()
                    })
                    .unwrap_or_default();
                if !c.is_empty() {
                    tangled.push((fi, c));
                }
            }
        }
    }
    Vec::new()
}

/// Face `fi`'s boundary built again from `nodes` into its slot of `boundaries`, with its
/// crossings (loop, node), as [`tangle::crossing`] finds them; `None` for a face that is refused
/// (in `refused`), or is refused now because its boundary cannot be built: its boundary is then
/// emptied, so it crosses nothing.
fn rebuild(
    fi: usize,
    evals: &[Option<EvalSurface>],
    nodes: &[edges::Nodes],
    boundaries: &mut [Boundary],
    face_boundary: &(dyn Fn(usize, &[edges::Nodes]) -> Result<Boundary> + Sync),
    refused: &mut Refused,
) -> Option<Vec<(usize, usize)>> {
    let fb = if refused.is(fi) {
        None
    } else {
        refused.take(fi, face_boundary(fi, nodes))
    };
    let slot = boundaries.get_mut(fi)?;
    *slot = fb.unwrap_or_default();
    if refused.is(fi) {
        return None;
    }
    let s = evals.get(fi).and_then(Option::as_ref)?;
    match tangle::crossing(s, slot.reach, &slot.loops) {
        Ok(c) => Some(c),
        Err(u) => {
            refused.face(fi, unclassified_face(refused.b, fi, &u));
            *slot = Boundary::default();
            None
        }
    }
}

/// Face `fi`'s refusal where its poles cannot be told at a point ([`Unclassified`]).
fn unclassified_face(b: &brep::Brep, fi: usize, u: &Unclassified) -> TessError {
    let kind = b.faces.get(fi).map_or("?", |f| kind(&f.surface));
    unsupported(format!("face {fi} ({kind}): {u}"))
}

/// A face whose boundary crosses itself, by index, with its crossing edge segments (edge, segment).
type Tangled = (usize, Vec<(u32, u32)>);

/// Rounds of [`snap_touches`] before a face that still crosses itself is refused.
const MAX_SNAP: u32 = 8;

/// The faces `faces` whose boundaries cross themselves where two curves touch or cross (halving
/// their chords does not part them, see [`untangle`]), with every node that lies within the chord
/// tolerance `tol` of a crossing chord of another edge put onto that edge as well (see
/// [`edges::snap`]): the two edges share the point, the face's boundary passes it twice (a pinch,
/// as at a vertex two loops share), and the chords meet there instead of crossing. Where the
/// curves run within the tolerance of each other for a stretch (a sliver between two touches),
/// the nodes on either side are shared in turn, so the boundary runs out and back along one
/// polyline there: a slit, which bounds the face on neither side. A node is not snapped where the
/// pinch would turn part of its loop inside out by more than the tolerance (two curves crossing
/// at an angle: the boundary is wrong there, not within the tolerance). Every face of a changed
/// edge has its boundary built again. Returns the faces whose boundaries still cross, with the
/// crossing segments.
/// An edge that cannot take a node, or a boundary that cannot be built again, refuses its faces
/// (in `refused`), which are passed over from then on.
#[allow(clippy::too_many_arguments)]
fn snap_touches(
    evals: &[Option<EvalSurface>],
    edge_faces: &[Vec<usize>],
    tol: f64,
    nodes: &mut [edges::Nodes],
    boundaries: &mut [Boundary],
    face_boundary: &(dyn Fn(usize, &[edges::Nodes]) -> Result<Boundary> + Sync),
    faces: &[usize],
    refused: &mut Refused,
) -> Vec<Tangled> {
    let crossing =
        |fi: usize, fb: &Boundary| -> std::result::Result<Vec<(usize, usize)>, Unclassified> {
            let Some(s) = evals.get(fi).and_then(Option::as_ref) else {
                return Ok(Vec::new());
            };
            tangle::crossing(s, fb.reach, &fb.loops)
        };
    let mut tangled: Vec<usize> = faces.to_vec();
    for _ in 0..MAX_SNAP {
        // By edge, in order: which edge refuses a face first does not hang on hashing.
        let mut add: std::collections::BTreeMap<u32, Vec<(f64, Vec3)>> =
            std::collections::BTreeMap::new();
        for &fi in &tangled {
            let Some(fb) = boundaries.get(fi) else {
                continue;
            };
            let found = match crossing(fi, fb) {
                Ok(c) => c,
                Err(u) => {
                    refused.face(fi, unclassified_face(refused.b, fi, &u));
                    continue;
                }
            };
            for (l, k) in found {
                let Some(&(e, j)) = fb.segments.get(l).and_then(|s| s.get(k)) else {
                    continue;
                };
                if let Some((t, p)) = touch(fb, nodes, (l, k), (e, j), tol) {
                    add.entry(e).or_default().push((t, p));
                }
            }
        }
        if add.is_empty() {
            break;
        }
        let mut redo: Vec<usize> = Vec::new();
        for (e, points) in &add {
            let Some(slot) = nodes.get_mut(*e as usize) else {
                continue;
            };
            if let Err(err) = edges::snap(*e as usize, slot, points) {
                refused.edge(edge_faces, *e as usize, &err);
            }
            redo.extend(edge_faces.get(*e as usize).into_iter().flatten().copied());
        }
        redo.extend(tangled.iter().copied());
        redo.sort_unstable();
        redo.dedup();
        tangled.clear();
        for fi in redo {
            if rebuild(fi, evals, nodes, boundaries, face_boundary, refused)
                .is_some_and(|c| !c.is_empty())
            {
                tangled.push(fi);
            }
        }
        if tangled.is_empty() {
            return Vec::new();
        }
    }
    let mut out: Vec<Tangled> = Vec::new();
    for fi in tangled {
        if refused.is(fi) {
            continue;
        }
        let Some(fb) = boundaries.get(fi) else {
            continue;
        };
        match crossing(fi, fb) {
            Ok(c) => {
                let c: Vec<(u32, u32)> = c
                    .into_iter()
                    .filter_map(|(l, k)| fb.segments.get(l)?.get(k).copied())
                    .collect();
                if !c.is_empty() {
                    out.push((fi, c));
                }
            }
            Err(u) => refused.face(fi, unclassified_face(refused.b, fi, &u)),
        }
    }
    out
}

/// The node of face boundary `fb` nearest the crossing chord `at` (loop, node) of its boundary,
/// segment `seg` (edge, segment from its start) of that edge, within `tol` of the chord strictly
/// between its ends, on another edge, and not yet on this one; with the parameter on the edge
/// where it projects onto the chord. `None` when there is none, or snapping it would turn part
/// of its loop inside out by more than the tolerance (see [`snap_touches`]).
fn touch(
    fb: &Boundary,
    nodes: &[edges::Nodes],
    at: (usize, usize),
    seg: (u32, u32),
    tol: f64,
) -> Option<(f64, Vec3)> {
    let (e, j) = seg;
    let n = nodes.get(e as usize)?;
    let j = j as usize;
    let (a, c) = (*n.points.get(j)?, *n.points.get(j + 1)?);
    let (ta, tc) = (*n.params.get(j)?, *n.params.get(j + 1)?);
    let ac = c - a;
    let len2 = ac.dot(ac);
    if len2.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return None;
    }
    let mut best: Option<(f64, f64, Vec3)> = None;
    for (l, lp) in fb.loops.iter().enumerate() {
        let segs = fb.segments.get(l)?;
        let m = lp.pts.len();
        for (k, &p) in lp.pts.iter().enumerate() {
            // The node's edges: the segment starting at it and the one ending there.
            let edges_at = [
                segs.get(k).map(|s| s.0),
                // Proven: m = lp.pts.len() > 0 while enumerating `lp.pts`.
                segs.get((k + m - 1) % m).map(|s| s.0),
            ];
            if edges_at.contains(&Some(e)) || n.points.contains(&p) {
                continue;
            }
            let f = (p - a).dot(ac) / len2;
            let d = p.dist_to_segment(a, c);
            if !(f > 1e-6 && f < 1.0 - 1e-6 && d <= tol) || best.is_some_and(|(bd, _, _)| bd <= d) {
                continue;
            }
            if l == at.0 && inside_out(lp, at.1, k, tol) {
                continue;
            }
            best = Some((d, ta + f * (tc - ta), p));
        }
    }
    best.map(|(_, t, p)| (t, p))
}

/// Whether pinching loop `lp` at node `k` and a new node (the same point) on its chord from node
/// `at` would turn either part inside out by more than `tol`: a part whose winding in (u, v) is
/// against the loop's, and whose area is more than `tol` across (twice its area in 3D over its
/// length).
fn inside_out(lp: &seam::Loop, at: usize, k: usize, tol: f64) -> bool {
    let m = lp.uv.len();
    if m < 3 {
        return false;
    }
    // The two parts: from the node after the chord's start round to `k`, and from `k` round to the
    // chord's start.
    let part = |from: usize, to: usize| -> Vec<usize> {
        let mut out = Vec::new();
        // Proven (the three `% m` here): m = lp.uv.len() >= 3, checked above.
        let mut i = from % m;
        loop {
            out.push(i);
            if i == to % m || out.len() > m {
                break;
            }
            // Proven: m >= 3, checked above.
            i = (i + 1) % m;
        }
        out
    };
    let whole = area2(&lp.uv);
    [part(at + 1, k), part(k, at)].iter().any(|ids| {
        let uv: Vec<[f64; 2]> = ids.iter().filter_map(|&i| lp.uv.get(i).copied()).collect();
        let pts: Vec<Vec3> = ids.iter().filter_map(|&i| lp.pts.get(i).copied()).collect();
        if uv.len() < 3 || area2(&uv) * whole >= 0.0 {
            return false;
        }
        // Newell's vector: twice the area of the polygon in 3D.
        let twice = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .fold(Vec3::ZERO, |acc, (p, q)| acc + p.cross(*q))
            .len();
        let length: f64 = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .map(|(p, q)| p.dist(*q))
            .sum();
        twice > tol * length
    })
}

/// Bounded work for the facet angle: a face whose boundary would get more than
/// [`size::MAX_ANGLE_BOUNDARY`] segments has its curved edges sampled again at twice the angle,
/// as often as it takes, down to the tolerance alone (a plate with a thousand small holes is
/// not worth a hundred times the work to draw each hole round). An edge keeps one set of nodes
/// for both its faces, at the coarser of their levels, so every rim of a crowded face gets the
/// Segments an edge loop needs to enclose an area in its face's (u, v).
const MIN_LOOP_SEGMENTS: usize = 3;

/// Every edge loop with fewer than [`MIN_LOOP_SEGMENTS`] segments has its edges' segments halved
/// until it has them: two short arcs (a lens), each within the chord tolerance and so one
/// segment, would otherwise be two chords on one line, a boundary of no area. The new nodes are
/// on the edges' curves and shared by every face that runs them, as all edge nodes are.
fn split_thin_loops(
    b: &brep::Brep,
    nodes: &mut [edges::Nodes],
    skip: &[bool],
) -> Vec<(usize, TessError)> {
    let mut failed: Vec<(usize, TessError)> = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        if skip.get(fi).copied().unwrap_or(true) {
            continue;
        }
        for l in &f.loops {
            let Loop::Edges { edges: uses } = l else {
                continue;
            };
            let mut es: Vec<usize> = uses.iter().map(|u| u.edge as usize).collect();
            es.sort_unstable();
            es.dedup();
            let count = |nodes: &[edges::Nodes]| -> usize {
                uses.iter()
                    .map(|u| {
                        nodes
                            .get(u.edge as usize)
                            .map_or(0, |n| n.points.len().saturating_sub(1))
                    })
                    .sum()
            };
            // Each round at least doubles every edge's segments: a few rounds always do.
            for _ in 0..MIN_LOOP_SEGMENTS {
                if count(nodes) >= MIN_LOOP_SEGMENTS {
                    break;
                }
                for &e in &es {
                    if failed.iter().any(|(x, _)| *x == e) {
                        continue;
                    }
                    let Some(slot) = nodes.get_mut(e) else {
                        continue;
                    };
                    // Proven: an edge has at most MAX_EDGE_NODES (20 000) nodes (sample_edge,
                    // split_segments and snap keep within it), so every segment index fits a u32.
                    let segs: Vec<u32> = (0..slot.points.len().saturating_sub(1))
                        .filter_map(|k| u32::try_from(k).ok())
                        .collect();
                    if let Err(err) = edges::split_segments(b, e, slot, &segs) {
                        failed.push((e, err));
                    }
                }
            }
        }
    }
    failed
}

/// same share whatever the order of the faces.
fn relax_crowded_faces(
    b: &brep::Brep,
    jobs: &[(usize, f64)],
    rules: &size::Rules,
    nodes: &mut [edges::Nodes],
    skip: &[bool],
) -> Vec<RefusedEdge> {
    // Edges whose relaxed sampling failed: they keep their nodes at the rules' own level.
    let mut strict: Vec<RefusedEdge> = Vec::new();
    let Some(turn) = rules.turn else {
        return strict;
    };
    let boundaries: Vec<Vec<usize>> = b
        .faces
        .iter()
        .enumerate()
        .filter(|(fi, _)| !skip.get(*fi).copied().unwrap_or(true))
        .map(|(_, f)| face_edges(f).map(|oe| oe.edge as usize).collect())
        .collect();
    let curved = |e: usize| {
        b.edges
            .get(e)
            .is_some_and(|e| !matches!(e.curve, Curve::Line { .. }))
    };
    let mut level: Vec<u8> = vec![0; nodes.len()];
    for l in 1..=size::MAX_RELAX {
        let segments = |e: &usize| {
            nodes
                .get(*e)
                .map_or(0, |n| n.points.len().saturating_sub(1))
        };
        let mut crowded: Vec<usize> = boundaries
            .iter()
            .filter(|es| es.iter().map(segments).sum::<usize>() > MAX_ANGLE_BOUNDARY)
            .flatten()
            .filter(|e| level.get(**e).copied().unwrap_or(0) < l && curved(**e))
            .copied()
            .collect();
        crowded.sort_unstable();
        crowded.dedup();
        if crowded.is_empty() {
            break;
        }
        let relaxed = size::Rules {
            turn: turn.relaxed(l),
            ..*rules
        };
        for e in crowded {
            let Some(&(_, h)) = jobs.get(e) else { continue };
            if let Some(slot) = level.get_mut(e) {
                *slot = l;
            }
            if strict.iter().any(|k| k.edge == e) {
                continue;
            }
            // The edge's nodes at the rules' own level are valid whatever the relaxed sampling
            // meets: an edge whose relaxed sampling fails keeps them (its faces are only more
            // finely bounded), refuses nothing, is not relaxed again, and is named with the
            // error. The relaxed parameters are a subset of the strict ones (all evaluated)
            // unless the strict sampling stopped at MAX_EDGE_NODES: the relaxed one, with fewer
            // nodes, then halves segments the strict one kept whole.
            if let Some(slot) = nodes.get_mut(e) {
                match edges::sample_edge(b, e, &relaxed, h) {
                    Ok(n) => *slot = n,
                    Err(err) => strict.push(RefusedEdge {
                        edge: e,
                        reason: err.to_string(),
                    }),
                }
            }
        }
    }
    strict
}

/// Twice the signed area of a parameter polygon.
fn area2(uv: &[[f64; 2]]) -> f64 {
    let n = uv.len();
    (0..n)
        .map(|k| {
            // Proven: n > 0 inside `0..n`.
            uv.get(k)
                .zip(uv.get((k + 1) % n))
                .map_or(0.0, |(a, b)| a[0] * b[1] - b[0] * a[1])
        })
        .sum()
}

/// A loop through both poles that turns straight back at one of them (it runs along a seam
/// and back: a whole sphere bounded by its meridian, or a sphere face whose split meridian it
/// uses both ways) retraces that seam once projected, since the periodic parameter carries on
/// unchanged across the pole. Across a pole it may jump by any amount, so the run from the
/// second pole back to the first is moved one period along, whichever way keeps the loop
/// within one period with a positive area: the seam's two uses then lie a period apart.
fn open_pole_to_pole(
    s: &EvalSurface,
    dom: &solidcore::eval::uv::Domain,
    reach: f64,
    l: &mut seam::Loop,
) -> std::result::Result<(), Unclassified> {
    let n = l.uv.len();
    let mut poles: Vec<usize> = Vec::new();
    for (k, q) in l.uv.iter().enumerate() {
        if pole_dir(s, *q, reach)?.is_some() {
            poles.push(k);
        }
    }
    let [p0, p1] = poles.as_slice() else {
        return Ok(());
    };
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        let near = |a: Option<&[f64; 2]>, b: Option<&[f64; 2]>| {
            a.zip(b)
                .is_some_and(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1e-9 * per)
        };
        // Proven: n > 0, the poles p0 and p1 having been found in `0..n`.
        let turns_back = [*p0, *p1]
            .iter()
            .any(|&p| near(l.uv.get((p + n - 1) % n), l.uv.get((p + 1) % n)));
        if !turns_back {
            continue;
        }
        // Along the seam the periodic parameter is constant between the poles.
        let second: Vec<usize> = (0..n)
            .filter(|k| (k > p1 || k < p0) && *k != *p0 && *k != *p1)
            .collect();
        if second.is_empty() {
            continue;
        }
        for sign in [1.0, -1.0] {
            let mut moved = l.uv.clone();
            for &k in &second {
                if let Some(q) = moved.get_mut(k) {
                    // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
                    #[allow(clippy::indexing_slicing)]
                    {
                        q[d] += sign * per;
                    }
                }
            }
            // The poles as the planner splits them: one copy at each neighbour's parameter.
            // Proven: capacity n = l.uv.len(), allocated with the same element type (the two
            // pole copies grow it if need be).
            let mut ring = Vec::with_capacity(n);
            for k in 0..n {
                let (Some(q), Some(a), Some(b)) = (
                    // Proven (both `% n` here): n > 0 inside `for k in 0..n`.
                    moved.get(k),
                    moved.get((k + n - 1) % n),
                    moved.get((k + 1) % n),
                ) else {
                    continue;
                };
                if k == *p0 || k == *p1 {
                    let (mut qa, mut qb) = (*q, *q);
                    // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
                    #[allow(clippy::indexing_slicing)]
                    {
                        qa[d] = a[d];
                        qb[d] = b[d];
                    }
                    ring.extend([qa, qb]);
                } else {
                    ring.push(*q);
                }
            }
            // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
            #[allow(clippy::indexing_slicing)]
            let (lo, hi) = ring
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                    (a.min(q[d]), b.max(q[d]))
                });
            if hi - lo <= per * (1.0 + 1e-9) && area2(&ring) > 0.0 {
                l.uv = moved;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// The poles of a loop where it turns straight back along a seam: the nodes either side have
/// the same parameters.
fn turning_poles(
    s: &EvalSurface,
    reach: f64,
    per: f64,
    uv: &[[f64; 2]],
) -> std::result::Result<Vec<usize>, Unclassified> {
    let n = uv.len();
    let near = |a: Option<&[f64; 2]>, b: Option<&[f64; 2]>| {
        a.zip(b)
            .is_some_and(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1e-9 * per)
    };
    let mut out = Vec::new();
    for (k, q) in uv.iter().enumerate() {
        // Proven: n > 0 while enumerating `uv`.
        if pole_dir(s, *q, reach)?.is_some() && near(uv.get((k + n - 1) % n), uv.get((k + 1) % n)) {
            out.push(k);
        }
    }
    Ok(out)
}

/// A loop with one pole, where it turns straight back along a seam, that winds once around the
/// periodic direction as projected: a half sphere's rim, then its seam down to the pole and up
/// again. Unwrapped along the loop, both runs of the seam lie on one parameter line, and the
/// period the loop is short of is closed by the rim's jump back. The face lies between the
/// seam's two uses a period apart, so the loop is unwrapped from the node after the pole round
/// to the pole, and starts at the node before it: the seam's uses then bound the face's parameter box on either side, and share
/// their nodes as every other edge does. Kept as it was unless the opened loop spans at most a
/// period with a positive area.
fn open_at_pole(
    s: &EvalSurface,
    dom: &solidcore::eval::uv::Domain,
    reach: f64,
    l: &mut seam::Loop,
) -> std::result::Result<(), Unclassified> {
    let n = l.uv.len();
    if n < 4 || l.pts.len() != n {
        return Ok(());
    }
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        if (seam::drift(l, d, per).abs() - per).abs() > per * 1e-6 {
            continue;
        }
        let [pole] = turning_poles(s, reach, per, &l.uv)?[..] else {
            continue;
        };
        // From the node after the pole round to the pole, unwrapped along the way.
        // Proven: n = l.uv.len() >= 4, checked above.
        let order: Vec<usize> = (1..=n).map(|i| (pole + i) % n).collect();
        // Proven: n = l.uv.len(), allocated with the same element type.
        let mut uv: Vec<[f64; 2]> = Vec::with_capacity(n);
        for &k in &order {
            let Some(mut q) = l.uv.get(k).copied() else {
                return Ok(());
            };
            if let Some(pr) = uv.last() {
                for ((x, y), per) in q.iter_mut().zip(pr).zip(dom.period) {
                    *x = solidcore::eval::uv::unwrap(*x, *y, per);
                }
            }
            uv.push(q);
        }
        // The pole's free parameter: its predecessor's (the poles are split from it later).
        if let (Some(prev), Some(q)) = (
            uv.len().checked_sub(2).and_then(|i| uv.get(i)).copied(),
            uv.last_mut(),
        ) {
            // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
            #[allow(clippy::indexing_slicing)]
            {
                q[d] = prev[d];
            }
        }
        // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
        #[allow(clippy::indexing_slicing)]
        let (lo, hi) = uv
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                (a.min(q[d]), b.max(q[d]))
            });
        // Started at the node before the pole, so the loop closes without a jump.
        let mut opened = seam::Loop {
            uv,
            pts: order
                .iter()
                .filter_map(|&k| l.pts.get(k).copied())
                .collect(),
        };
        // Proven: both hold n >= 4 nodes: every `order` node was in `l.uv` (or this returned),
        // and `l.pts.len() == n` was checked above.
        opened.uv.rotate_right(2);
        opened.pts.rotate_right(2);
        if opened.pts.len() == n
            && hi - lo <= per * (1.0 + 1e-9)
            && area2(&opened.uv) > 0.0
            && !seam::winds(std::slice::from_ref(&opened), d, per)
        {
            *l = opened;
            return Ok(());
        }
    }
    Ok(())
}

/// Each loop is projected on its own, so along a closed direction a hole can come out a whole
/// period away from the outer loop: a dome's outer loop opened at its pole spans v in [-1, 0]
/// while the hole projects into [0, 1], outside it (tangency-labs/solidcore#433). Every other
/// loop is moved by whole periods to the copy whose middle is nearest the outer loop's (the
/// loop of largest area); a face that does not wind lies within one period, so that is the
/// copy inside it.
fn holes_beside_outer(dom: &solidcore::eval::uv::Domain, loops: &mut [seam::Loop]) {
    let Some(outer) = loops
        .iter()
        .map(|l| area2(&l.uv).abs())
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
    else {
        return;
    };
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        // The middle of a loop's range along `d`.
        // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
        #[allow(clippy::indexing_slicing)]
        let middle = |l: &seam::Loop| {
            let (lo, hi) =
                l.uv.iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                        (a.min(q[d]), b.max(q[d]))
                    });
            0.5 * (lo + hi)
        };
        let Some(centre) = loops.get(outer).map(middle) else {
            continue;
        };
        for (i, l) in loops.iter_mut().enumerate() {
            let k = ((middle(l) - centre) / per).round();
            if i == outer || !k.is_finite() || k == 0.0 {
                continue;
            }
            for q in &mut l.uv {
                // Proven: `d` < 2 enumerates the periods; parameters are `[f64; 2]`.
                #[allow(clippy::indexing_slicing)]
                {
                    q[d] -= k * per;
                }
            }
        }
    }
}

/// One face, planned by [`plan_face`]; a face it refuses because its loops cross in (u, v) is
/// planned once more leniently (see [`Lenient`]) and named in the stats with the first refusal,
/// so the defect upstream is still reported. When the lenient fill refuses too, the first
/// refusal is the error (and [`plan`] leaves the face out, named). `refused`: a face whose
/// boundary still crosses itself after [`untangle`] (two edges meeting tangentially at a cusp,
/// whose chords cross however often they are halved), filled leniently straight away with that
/// reason.
fn plan_or_fill(
    fi: u32,
    kind: &str,
    s: &EvalSurface,
    loops: &[seam::Loop],
    rules: &size::Rules,
    size: &size::FaceSize,
    reach: f64,
    refused: Option<&str>,
) -> Result<(FacePlan, PlanStats)> {
    let mut stats = PlanStats::default();
    let strict = match refused {
        Some(why) => Err(FaceError::Layout(unsupported(why.to_string()))),
        None => plan_face(
            fi,
            kind,
            s,
            loops,
            rules,
            size,
            reach,
            &mut stats,
            Lenient::No,
        ),
    };
    match strict {
        Ok(fp) => Ok((fp, stats)),
        // The evaluator refuses the face: no fill can mesh it.
        Err(FaceError::Refused(e)) => Err(e),
        Err(FaceError::Layout(first)) => {
            let mut stats = PlanStats::default();
            match plan_face(
                fi,
                kind,
                s,
                loops,
                rules,
                size,
                reach,
                &mut stats,
                Lenient::Yes,
            ) {
                Ok(fp) => {
                    stats.lenient.push(RefusedFace {
                        face: fi as usize,
                        reason: first.to_string(),
                    });
                    Ok((fp, stats))
                }
                // The lenient fill reached a point the surface does not evaluate at: that, not
                // the layout, is why the face is left out.
                Err(FaceError::Refused(e)) => Err(e),
                Err(FaceError::Layout(_)) => Err(first),
            }
        }
    }
}

/// One face: boundary loops in 3D (shared edge nodes) to parameters and triangles.
/// `size`: the face's target side length (mm) and the surface's mean speed along u and v.
/// `reach`: how near a pole a point is taken as one ([`Boundary::reach`]). `lenient`: the last resort
/// for a face no method could plan (see [`Lenient`]).
#[allow(clippy::too_many_arguments)]
fn plan_face(
    fi: u32,
    kind: &str,
    s: &EvalSurface,
    loops: &[seam::Loop],
    rules: &size::Rules,
    size: &size::FaceSize,
    reach: f64,
    stats: &mut PlanStats,
    lenient: Lenient,
) -> std::result::Result<FacePlan, FaceError> {
    let tol = rules.tol;
    let (h, speed) = (size.h, size.speed);
    // The memory bounds that limit the face, if any (see [`PlanStats::over_tolerance`]).
    let mut caps: Vec<Cap> = Vec::new();
    let dom = solidcore::eval::uv::domain(s);
    let fail = |why: &str| unsupported(format!("face {fi} ({kind}): {why}"));
    let no_value = |NoValue(q): NoValue| {
        FaceError::Refused(unsupported(format!(
            "face {fi} ({kind}): the surface does not evaluate at {q:?}"
        )))
    };
    // A point whose poles the kernel's classifier cannot tell: refused with its reason.
    let unclassified = |u: Unclassified| FaceError::Refused(fail(&u.to_string()));
    let sag_stop = |e: sag::Stop, nodes: usize| match e {
        sag::Stop::NoValue(q) => no_value(q),
        sag::Stop::Unclassified(u) => unclassified(u),
        sag::Stop::Corner(k) => FaceError::Refused(fail(&format!(
            "triangle {k} has a corner past its {nodes} nodes"
        ))),
    };
    // A point the surface does not evaluate at that the face's loops, spanning more than
    // MAX_PERIODS periods, cannot place inside the face or out.
    let too_many = |TooManyCopies(n): TooManyCopies, q: [f64; 2]| {
        FaceError::Refused(fail(&format!(
            "its loops span {n} periods, more than the {MAX_PERIODS} a point the surface does not evaluate at ({q:?}) is looked for in"
        )))
    };
    let unmeasured = |e: uv::Unmeasured| match e {
        uv::Unmeasured::NoValue(q) => no_value(q),
        uv::Unmeasured::TooManyCopies(c, q) => too_many(c, q),
    };
    let mut projected: Vec<seam::Loop> = loops.to_vec();
    // A loop that winds around a periodic direction does not close in (u, v): the face is cut
    // along a parameter line into one period (a seamless cylinder face, a helical trim).
    // A loop that runs up a seam from a pole and straight back down it (a half sphere bounded
    // by its rim and its seam used both ways) winds once as projected: it is opened at the pole.
    for l in &mut projected {
        open_at_pole(s, &dom, reach, l).map_err(unclassified)?;
    }
    let wound: Vec<(usize, f64)> = dom
        .period
        .into_iter()
        .enumerate()
        .filter_map(|(d, per)| {
            per.filter(|&per| seam::winds(&projected, d, per))
                .map(|per| (d, per))
        })
        .collect();
    // A face cut along a seam is a long winding strip whose sides are the cut lines and the
    // boundary in turn: Delaunay with sag refinement meshes it (the patch layouts below assume
    // boundary all round, and leave fans across the strip's corners off the surface).
    let seamed = !wound.is_empty();
    match wound.as_slice() {
        [] => {}
        &[(d, per)] => {
            projected = seam::cut(s, &projected, d, per, rules, h).map_err(|e| match e {
                seam::Cut::Unclean => FaceError::Layout(fail("boundary winds around the surface")),
                seam::Cut::NoValue(q) => no_value(q),
            })?;
        }
        _ => return Err(fail("boundary winds around the surface both ways").into()),
    }
    // A loop from pole to pole and back along one seam (a whole sphere as one face, as another
    // kernel writes it) projects onto one parameter line: its second run is a period along.
    if !seamed {
        for l in &mut projected {
            open_pole_to_pole(s, &dom, reach, l).map_err(unclassified)?;
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
            let (prev, next) = (
                // Proven: n = luv.len() > 0 while enumerating `luv`.
                luv.get((k + n - 1) % n).copied(),
                luv.get((k + 1) % n).copied(),
            );
            let pole = pole_dir(s, *q, reach).map_err(unclassified)?;
            if let (Some(d), Some(a), Some(b)) = (pole, prev, next) {
                // The node with its parameter along the pole's direction `d` (0 or 1, from
                // `0..2` above) taken from `from`.
                let with = |from: [f64; 2]| {
                    if d == 0 {
                        [from[0], q[1]]
                    } else {
                        [q[0], from[1]]
                    }
                };
                let (qa, qb) = (with(a), with(b));
                uvs.push(qa);
                fixed.push(*p);
                let gap = if d == 0 { qa[0] - qb[0] } else { qa[1] - qb[1] };
                if gap.abs() > 1e-12 {
                    uvs.push(qb);
                    fixed.push(*p);
                }
            } else {
                uvs.push(*q);
                fixed.push(*p);
            }
        }
        ranges.push(start..uvs.len());
    }
    if uvs.len() < 3 {
        return Err(fail("degenerate boundary").into());
    }
    // Every index of the face's nodes is a u32: its boundary, and what the layouts and the sag
    // refinement add, stay within MAX_FACE_NODES (2^28), far below u32::MAX.
    if let Some(why) = too_many_nodes(uvs.len(), 0) {
        return Err(FaceError::Refused(fail(&why)));
    }
    let mut bbox = [(f64::MAX, f64::MIN); 2];
    for p in &uvs {
        for (b, &x) in bbox.iter_mut().zip(p) {
            *b = (b.0.min(x), b.1.max(x));
        }
    }
    let polys: Vec<&[[f64; 2]]> = ranges.iter().filter_map(|r| uvs.get(r.clone())).collect();
    // A size sample the surface does not evaluate at refuses the face where it lies inside the
    // face's loops (as the planner meshes them, a whole number of periods along where the
    // surface is closed); outside them it was never the face's.
    for q in &size.unevaluated {
        match within_loops(&polys, bbox, dom.period, *q) {
            Ok(false) => {}
            Ok(true) => return Err(no_value(NoValue(*q))),
            Err(c) => return Err(too_many(c, *q)),
        }
    }
    // Constrained Delaunay in (u, v) scaled to millimetres: equilateral seeds of side `h`,
    // refined where triangles leave the surface, are too long or bend too much, then shaped by
    // Ruppert refinement.
    let step = [h / speed[0], h / speed[1]];
    let delaunay = || -> std::result::Result<_, FaceError> {
        let seeds = cdt::lattice(bbox, step, &polys, speed);
        let eval = |u: f64, v: f64| sag_eval(s, u, v);
        let sag = cdt::Sag {
            eval: &eval,
            tol,
            h,
            turn: rules.turn,
            scale: speed,
        };
        let fill = cdt::Fill {
            seeds: &seeds,
            refine: Some((sag, MAX_REFINE)),
            shape: !matches!(s, EvalSurface::Plane { .. }),
            lenient: lenient == Lenient::Yes,
        };
        cdt::triangulate(&uvs, &ranges, fill)
            .map_err(no_value)?
            .ok_or_else(|| fail("boundary crosses itself").into())
    };
    // Planar and curved faces alike: a structured grid where the boundary runs along the
    // parameter box (a rectangle, a cylinder half), a Coons grid on four corners, else Delaunay.
    let planar = matches!(s, EvalSurface::Plane { .. });
    // The lenient fill is the constrained Delaunay alone: the caller names the face in the stats.
    let (extra, triangles) = if lenient == Lenient::Yes {
        delaunay()?
    } else {
        // A point of the box outside the face's loops is not the face's (as for the size
        // samples above); one they cannot tell about, past MAX_PERIODS, refuses the face.
        let mine = |q: [f64; 2]| within_loops(&polys, bbox, dom.period, q);
        let mut us =
            uv::divide(s, 0, bbox[0], bbox[1], rules, h, MAX_LINES, &mine).map_err(unmeasured)?;
        let mut vs =
            uv::divide(s, 1, bbox[1], bbox[0], rules, h, MAX_LINES, &mine).map_err(unmeasured)?;
        let asked = [us.len(), vs.len()];
        grid::thin(&mut us, &mut vs, grid::MAX_FACE_GRID);
        let kept = [us.len(), vs.len()];
        // A plane is flat however few lines it keeps.
        if kept != asked && !planar {
            caps.push(Cap::GridThinned { asked, kept });
        }
        let structured = match (ranges.len(), if seamed { None } else { grid::quad(&uvs) }) {
            (1, Some(q)) => {
                // Interior lines: the boundary's own where opposite sides agree (refined where the
                // surface turns more between them inside the face), else the division.
                // Along `d`, whose range is `(lo, hi)`, across the range `across`.
                let lines = |d: usize, (lo, hi): (f64, f64), across: (f64, f64), div: &Vec<f64>| {
                    let inner = |l: Vec<f64>| {
                        l.into_iter()
                            .filter(|x| *x > lo && *x < hi)
                            .collect::<Vec<f64>>()
                    };
                    match grid::aligned_lines(&uvs, &q, d) {
                        Some(aligned) => {
                            let ends: Vec<f64> = std::iter::once(lo)
                                .chain(aligned)
                                .chain(std::iter::once(hi))
                                .collect();
                            uv::refine(s, d, &ends, across, rules, h, MAX_LINES, &mine).map(inner)
                        }
                        None => Ok(inner(div.clone())),
                    }
                };
                let (lu, lv) = (
                    lines(0, bbox[0], bbox[1], &us).map_err(unmeasured)?,
                    lines(1, bbox[1], bbox[0], &vs).map_err(unmeasured)?,
                );
                let strip = match (lu.is_empty(), lv.is_empty()) {
                    (false, true) => grid::strip(&uvs, &q, 0),
                    (true, false) => grid::strip(&uvs, &q, 1),
                    _ => None,
                };
                strip.map(|t| (Vec::new(), t)).or_else(|| {
                    grid::structured(
                        &uvs,
                        &q,
                        &grid::interior(lu, bbox[0]),
                        &grid::interior(lv, bbox[1]),
                    )
                })
            }
            _ => None,
        };
        let structured = match structured {
            None if ranges.len() == 1 && !seamed => {
                let fine = |a: [f64; 2], b: [f64; 2]| uv::fine_between(s, rules, h, a, b);
                coons::coons(&uvs, &fixed, speed, grid::MAX_FACE_GRID, h, Some(&fine))
                    .map_err(no_value)?
            }
            other => other,
        };
        if let Some(r) = structured {
            if planar {
                stats.planar += 1;
            } else {
                stats.structured += 1;
            }
            r
        } else {
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
            let face = Region {
                s,
                uvs: &uvs,
                fixed: &fixed,
                ranges: &ranges,
                rules,
                h,
                speed,
                us: &us,
                vs: &vs,
            };
            let strip = !planar
                && (us.len() <= 2 || vs.len() <= 2)
                && !cdt::has_room(bbox, step, &polys, speed);
            let trimmed = if seamed || strip {
                None
            } else if planar {
                if ranges.len() == 1 {
                    plan_pieces(&face).map_err(no_value)?
                } else {
                    None
                }
            } else {
                plan_pieces(&face).map_err(no_value)?.or_else(|| {
                    let traces = motorcycle::traces(&uvs, &ranges, speed, 0.25 * h);
                    cutgrid::cut_grid(&uvs, &ranges, &us, &vs, &traces, speed)
                })
            };
            if let Some(r) = trimmed {
                stats.structured += 1;
                r
            } else {
                if planar {
                    stats.planar += 1;
                } else {
                    stats.delaunay += 1;
                }
                delaunay()?
            }
        }
    };
    // At a pole the boundary has two copies of one node: a triangle on both copies has two
    // corners at one point and no area (its other two sides are one segment both ways).
    let pole_pair = |i: u32, j: u32| {
        i != j
            && fixed
                .get(i as usize)
                .zip(fixed.get(j as usize))
                .is_some_and(|(p, q)| p == q)
    };
    let mut triangles = triangles;
    triangles
        .retain(|t| !(pole_pair(t[0], t[1]) || pole_pair(t[1], t[2]) || pole_pair(t[2], t[0])));
    // A lenient fill leaves out the sides that cross: the boundary is followed everywhere else.
    if lenient == Lenient::No && !closed_boundary(&ranges, &fixed, &uvs, &triangles) {
        return Err(fail("triangulation does not follow the boundary").into());
    }
    let mut all = uvs;
    all.extend(extra);
    if let Some(why) = too_many_nodes(all.len(), MAX_REFINE) {
        return Err(FaceError::Refused(fail(&why)));
    }
    // Whatever the layout, no triangle leaves the surface by more than the tolerance.
    if !planar {
        if sag::refine(s, tol, reach, &fixed, &mut all, &mut triangles, MAX_REFINE)
            .map_err(|e| sag_stop(e, all.len()))?
        {
            caps.push(Cap::Refine);
        }
        sag::flip(s, tol, reach, &fixed, &all, &mut triangles)
            .map_err(|e| sag_stop(e, all.len()))?;
        // A face a bound limited is named with the sag its triangles reach.
        if !caps.is_empty() {
            let (sag, unmeasured) = sag::worst(s, reach, &fixed, &all, &triangles)
                .map_err(|e| sag_stop(e, all.len()))?;
            stats.over_tolerance.push(OverTolerance {
                face: fi as usize,
                sag,
                unmeasured,
                tol,
                within: sag <= tol && unmeasured == 0,
                caps: std::mem::take(&mut caps),
            });
        }
    }
    // Both evaluators take a closed spline's parameters inside its active interval.
    for q in &mut all {
        *q = s.wrap(*q);
    }
    Ok(FacePlan {
        face: fi,
        surface: None,
        uv: all,
        fixed,
        normals: Vec::new(),
        flip: false,
        triangles,
        poles: Vec::new(),
    })
}

/// Most copies of a point, a whole number of periods along, [`within_loops`] looks for inside a
/// face's loops (in both directions together): a coil's or a thread's wall winds a few dozen
/// turns.
const MAX_PERIODS: usize = 4096;

/// The loops of a face span more periods (the count) than [`within_loops`] looks through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TooManyCopies(pub f64);

/// Why a face of `nodes` nodes, and `more` its refinement may add, is refused: more than
/// [`MAX_FACE_NODES`] in all. `None` when they fit.
fn too_many_nodes(nodes: usize, more: usize) -> Option<String> {
    (nodes.saturating_add(more) > MAX_FACE_NODES).then(|| {
        if more == 0 {
            format!("its {nodes} boundary nodes are more than the {MAX_FACE_NODES} a face may have")
        } else {
            format!(
                "its {nodes} nodes, with the {more} refinement may add, are more than the {MAX_FACE_NODES} a face may have"
            )
        }
    })
}

/// Whether the parameter point `q`, or a copy of it a whole number of `period`s along, lies
/// inside the region the loops `polys` bound (even–odd, the rule [`cdt::lattice`] keeps its
/// seeds by), within their box `bbox`. Every copy inside the box is tried; a box spanning more
/// than [`MAX_PERIODS`] copies (or one not finite) is the error, never "not inside".
fn within_loops(
    polys: &[&[[f64; 2]]],
    bbox: [(f64, f64); 2],
    period: [Option<f64>; 2],
    q: [f64; 2],
) -> std::result::Result<bool, TooManyCopies> {
    // Whole numbers up to 2^53 are exact in f64, and well inside i64.
    const LIMIT: f64 = 9_007_199_254_740_992.0;
    // The copies of the coordinate `x` of `q` along a direction (range `(lo, hi)` in the box,
    // period `period`) inside the box.
    let copies = |(lo, hi): (f64, f64),
                  period: Option<f64>,
                  x: f64|
     -> std::result::Result<Vec<f64>, TooManyCopies> {
        match period {
            Some(per) if per > 0.0 && per.is_finite() => {
                let first = ((lo - x) / per).ceil();
                let last = ((hi - x) / per).floor();
                let count = last - first + 1.0;
                // Compared in f64 before the casts: NaN fails it.
                if count.is_nan() || count > MAX_PERIODS as f64 {
                    return Err(TooManyCopies(count));
                }
                if count < 1.0 {
                    return Ok(Vec::new());
                }
                // Proven: whole numbers of magnitude at most 2^53 (checked here, NaN failing
                // it), so both casts are exact.
                if !(first.abs() <= LIMIT && last.abs() <= LIMIT) {
                    return Err(TooManyCopies(count));
                }
                Ok((first as i64..=last as i64)
                    .map(|k| x + per * k as f64)
                    .collect())
            }
            _ => Ok([x].into_iter().filter(|x| *x >= lo && *x <= hi).collect()),
        }
    };
    let us = copies(bbox[0], period[0], q[0])?;
    let vs = copies(bbox[1], period[1], q[1])?;
    // Proven: each is at most MAX_PERIODS long, so the product does not overflow.
    let count = us.len() * vs.len();
    if count > MAX_PERIODS {
        return Err(TooManyCopies(count as f64));
    }
    Ok(vs
        .into_iter()
        .any(|v| us.iter().any(|&u| cdt::inside(polys, [u, v]))))
}

/// The point and unit normal (`None` at a pole) of `s` at (u, v), for the Delaunay sag check;
/// the parameters where the surface does not evaluate are the error.
fn sag_eval(s: &EvalSurface, u: f64, v: f64) -> std::result::Result<(Vec3, Option<Vec3>), NoValue> {
    s.derivs(u, v)
        .map(kernel::record_triple)
        .map(|(p, su, sv)| (p, su.cross(sv).normalized()))
        .ok_or(NoValue([u, v]))
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
/// cut nodes and patch interiors follow the boundary nodes. `Ok(None)` when the cuts do not make
/// patches; the error where a cut node, or a point the size rules or the sag check need, does not
/// evaluate.
#[allow(clippy::type_complexity)] // the layouts' own (parameters, triangles) pair, as Option, in a Result
fn plan_pieces(f: &Region) -> std::result::Result<Option<UvMesh>, NoValue> {
    layout(|stopped| plan_pieces_in(f, stopped))
}

/// [`plan_pieces`]'s body: a point the surface does not evaluate at goes to `stopped`.
fn plan_pieces_in(f: &Region, stopped: &mut Option<NoValue>) -> Option<UvMesh> {
    // The face's global seed: one spacing in every direction, the finest its size rules ask for
    // along either parameter (the median gap between the grid lines they give, in mm). On a
    // cylinder that is the spacing round it, which the cuts along its axis then get too.
    let median_gap = |lines: &[f64], speed: f64| -> f64 {
        let mut gaps: Vec<f64> = lines
            // Proven: the window size is the constant 2, not zero.
            .windows(2)
            .filter_map(|w| Some((w.get(1)? - w.first()?) * speed))
            .filter(|g| *g > 0.0)
            .collect();
        gaps.sort_by(f64::total_cmp);
        // Proven: the divisor is the constant 2, not zero.
        gaps.get(gaps.len() / 2).copied().unwrap_or(f64::INFINITY)
    };
    let seed = median_gap(f.us, f.speed[0])
        .min(median_gap(f.vs, f.speed[1]))
        .min(f.h);
    let segments = collar::segments(
        f.uvs,
        f.ranges,
        f.speed,
        seed,
        matches!(f.s, EvalSurface::Plane { .. }),
    )?;
    let s = f.s;
    let fine = |a: [f64; 2], b: [f64; 2]| uv::fine_between(s, f.rules, f.h, a, b);
    // A cut's seeds: spaced from the boundary's own spacing at each end, growing geometrically
    // between them, never wider than the global seed, and split further wherever the surface
    // between two seeds fails the size rules.
    let seeds =
        |a: [f64; 2], b: [f64; 2], sa: f64, sb: f64| graded(a, b, sa, sb, seed, f.speed, &fine);
    let (cut_uv, pieces) = stop(
        stopped,
        pieces::cut(f.uvs, f.ranges, &segments, f.speed, &seeds),
    )??;
    let n = f.uvs.len();
    let mut uv_all: Vec<[f64; 2]> = f.uvs.to_vec();
    uv_all.extend_from_slice(&cut_uv);
    let mut pos: Vec<Vec3> = f.fixed.to_vec();
    for q in &cut_uv {
        let p = s.derivs(q[0], q[1]).map(|d| kernel::record_vec3(d.0));
        pos.push(stop(stopped, p.ok_or(NoValue(*q)))?);
    }
    let mut extra: Vec<[f64; 2]> = cut_uv;
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for pc in &pieces {
        // The patch's own loops, numbered from zero.
        let mut ids: Vec<u32> = pc.ring.to_vec();
        let mut ranges = vec![std::ops::Range {
            start: 0,
            end: pc.ring.len(),
        }];
        for hole in &pc.holes {
            ranges.push(ids.len()..ids.len() + hole.len());
            ids.extend_from_slice(hole);
        }
        let luv: Vec<[f64; 2]> = ids
            .iter()
            .map(|&k| uv_all.get(k as usize).copied())
            .collect::<Option<_>>()?;
        let lpos: Vec<Vec3> = ids
            .iter()
            .map(|&k| pos.get(k as usize).copied())
            .collect::<Option<_>>()?;
        let mut bbox = [(f64::MAX, f64::MIN); 2];
        for p in &luv {
            for (b, &x) in bbox.iter_mut().zip(p) {
                *b = (b.0.min(x), b.1.max(x));
            }
        }
        let clip = |lines: &[f64], (a, b): (f64, f64)| -> Vec<f64> {
            std::iter::once(a)
                .chain(lines.iter().copied().filter(|x| *x > a && *x < b))
                .chain(std::iter::once(b))
                .collect()
        };
        // A patch whose outline changes between boundary and cut at exactly four nodes (a
        // quarter of a ring, a butterfly wing round a hole) takes those as its corners, so each
        // side is all boundary or all cut; any other patch takes the best four of its corners.
        let is_cut = |a: u32, b: u32| -> bool {
            let (a, b) = (a as usize, b as usize);
            a >= n
                || b >= n
                || !f.ranges.iter().any(|r| {
                    r.contains(&a)
                        && r.contains(&b)
                        && (a.abs_diff(b) == 1 || a.abs_diff(b) == r.len() - 1)
                })
        };
        let ring = &pc.ring;
        let switches: Vec<usize> = ring
            .corners()
            .enumerate()
            .filter(|&(_, (&p, &c, &x))| is_cut(p, c) != is_cut(c, x))
            .map(|(k, _)| k)
            .collect();
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
                for ((l, h), x) in lo.iter_mut().zip(&mut hi).zip(q) {
                    *l = l.min(x);
                    *h = h.max(x);
                }
            }
            let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
            if !(w > 0.0 && h > 0.0) {
                return None;
            }
            let mut pick = [0usize; 4];
            for (slot, dir) in
                pick.iter_mut()
                    .zip([[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]])
            {
                *slot = (0..m).max_by(|&x, &y| {
                    let reach = |k: usize| {
                        at(k).map_or(f64::MIN, |q| {
                            dir[0] * (q[0] - lo[0]) / w + dir[1] * (q[1] - lo[1]) / h
                        })
                    };
                    reach(x).total_cmp(&reach(y))
                })?;
            }
            // Four distinct corners in the loop's (counter-clockwise) order.
            let start = pick[0];
            // Proven: m > 0, as `pick` came from `(0..m).max_by(..)?`.
            let rel = pick.map(|k| (k + m - start) % m);
            (rel[0] < rel[1] && rel[1] < rel[2] && rel[2] < rel[3]).then_some(pick)
        };
        let quad = if pc.holes.is_empty() {
            // Every patch is four-sided by construction: its corners are where its outline turns.
            let at = |pick: [usize; 4]| {
                infallible(coons::coons_at(
                    &luv,
                    &lpos,
                    pick,
                    grid::MAX_FACE_GRID,
                    f.h,
                    None,
                ))
            };
            infallible(coons::coons(
                &luv,
                &lpos,
                f.speed,
                grid::MAX_FACE_GRID,
                f.h,
                None,
            ))
            .or_else(|| match switches[..] {
                [a, b, c, d] => at([a, b, c, d]),
                _ => None,
            })
            .or_else(|| completed().and_then(at))
        } else {
            None
        };
        let local = quad
            .or_else(|| {
                if pc.holes.is_empty() {
                    coons::coons_best(&luv, &lpos, f.speed, grid::MAX_FACE_GRID, f.h)
                } else {
                    None
                }
            })
            .or_else(|| {
                cutgrid::cut_grid(
                    &luv,
                    &ranges,
                    &clip(f.us, bbox[0]),
                    &clip(f.vs, bbox[1]),
                    &[],
                    f.speed,
                )
            })
            .or_else(|| {
                let polys: Vec<&[[f64; 2]]> =
                    ranges.iter().filter_map(|r| luv.get(r.clone())).collect();
                let seeds =
                    cdt::lattice(bbox, [f.h / f.speed[0], f.h / f.speed[1]], &polys, f.speed);
                let eval = |u: f64, v: f64| sag_eval(s, u, v);
                let sag = cdt::Sag {
                    eval: &eval,
                    tol: f.rules.tol,
                    h: f.h,
                    turn: f.rules.turn,
                    scale: f.speed,
                };
                let fill = cdt::Fill {
                    seeds: &seeds,
                    refine: Some((sag, MAX_REFINE)),
                    shape: true,
                    lenient: false,
                };
                stop(stopped, cdt::triangulate(&luv, &ranges, fill)).flatten()
            })?;
        let (lextra, ltris) = local;
        let base = u32::try_from(n + extra.len()).ok()?;
        let m = ids.len();
        for t in ltris {
            let map = |i: u32| -> Option<u32> {
                let i = i as usize;
                if i < m {
                    ids.get(i).copied()
                } else {
                    base.checked_add(u32::try_from(i - m).ok()?)
                }
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
/// `fine` fails between its ends; `fine`'s error ends it.
fn graded(
    a: [f64; 2],
    b: [f64; 2],
    sa: f64,
    sb: f64,
    h: f64,
    speed: [f64; 2],
    fine: &dyn Fn([f64; 2], [f64; 2]) -> std::result::Result<bool, NoValue>,
) -> std::result::Result<Vec<f64>, NoValue> {
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
        let sizes: Vec<f64> = if (q - 1.0).abs() < 1e-3
            || q.partial_cmp(&0.0) != Some(Ordering::Greater)
            || !q.is_finite()
        {
            // Even spacing at the mean of the two.
            // Proven: the clamp bounds are the constants 1.0 <= 4096.0, neither NaN.
            let k = (len / (0.5 * (sa + sb))).round().clamp(1.0, 4096.0) as usize;
            vec![len / k as f64; k]
        } else {
            // Proven: the clamp bounds are the constants 1.0 <= 4096.0, neither NaN.
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
    let mut all: Vec<f64> = std::iter::once(0.0)
        .chain(cuts)
        .chain(std::iter::once(1.0))
        .collect();
    for _ in 0..12 {
        // Proven: all.len() <= 4097 here (at most 4095 cuts, and the loop stops past 4096).
        let mut next = Vec::with_capacity(all.len() * 2);
        let mut split = false;
        for w in all.windows(2) {
            let (Some(&x), Some(&y)) = (w.first(), w.get(1)) else {
                continue;
            };
            next.push(x);
            if !fine(point(x), point(y))? {
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
    Ok(all.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect())
}

/// Every boundary segment (consecutive nodes of a loop) is the side of exactly one triangle (two
/// for a slit the loops run along both ways):
/// the face covers its boundary with nothing missing or doubled, so it meets its neighbours
/// exactly. Some boundary nodes are one vertex, a side ending on any of them the same side: pole
/// copies of one node (consecutive in a loop, at one point, the segment between them of no
/// length), and nodes at the very same (u, v), which the triangulation takes as one point: two
/// loops touching at a vertex they share (a ring standing on a perforated plate, tangent to the
/// holes beside it). A seam's two uses are at one point in space but a period apart in (u, v),
/// and stay two.
fn closed_boundary(
    ranges: &[std::ops::Range<usize>],
    fixed: &[Vec3],
    uvs: &[[f64; 2]],
    triangles: &[[u32; 3]],
) -> bool {
    // Proven (the u32 casts here): `fixed` and the loops' ranges are a face's boundary nodes, at
    // most MAX_FACE_NODES (checked by `plan_face`), so every index fits a u32.
    let mut one: Vec<u32> = (0..fixed.len() as u32).collect();
    let root = |one: &[u32], mut i: u32| {
        while let Some(&j) = one.get(i as usize).filter(|j| **j != i) {
            i = j;
        }
        i
    };
    // Proven: only called inside `for k in 0..r.len()`, so r.len() > 0.
    let segment = |r: &std::ops::Range<usize>, k: usize| {
        ((r.start + k) as u32, (r.start + (k + 1) % r.len()) as u32)
    };
    let join = |one: &mut [u32], a: u32, b: u32| {
        let (ra, rb) = (root(one, a), root(one, b));
        if let Some(x) = one.get_mut(rb.max(ra) as usize) {
            *x = ra.min(rb);
        }
    };
    for r in ranges {
        for k in 0..r.len() {
            let (a, b) = segment(r, k);
            if fixed
                .get(a as usize)
                .zip(fixed.get(b as usize))
                .is_some_and(|(p, q)| p == q)
            {
                join(&mut one, a, b);
            }
        }
    }
    // By value: −0.0 and 0.0 are one point.
    let key = |q: &[f64; 2]| [(q[0] + 0.0).to_bits(), (q[1] + 0.0).to_bits()];
    let mut first: HashMap<[u64; 2], u32> = HashMap::new();
    for (i, q) in uvs.iter().enumerate().take(fixed.len()) {
        // Proven: below MAX_FACE_NODES, as above.
        let i = i as u32;
        let f = *first.entry(key(q)).or_insert(i);
        if f != i {
            join(&mut one, f, i);
        }
    }
    let side = |a: u32, b: u32| {
        let (a, b) = (root(&one, a), root(&one, b));
        (a != b).then_some((a.min(b), a.max(b)))
    };
    let mut sides: HashMap<(u32, u32), u32> = HashMap::new();
    for t in triangles {
        // Each corner with the next one round the triangle.
        for (&a, &b) in t.iter().zip(t.iter().cycle().skip(1)) {
            if let Some(k) = side(a, b) {
                *sides.entry(k).or_default() += 1;
            }
        }
    }
    // Each boundary side has a triangle for every time the loops run along it: one, or two for
    // a slit (a side run out and back, where the face meets itself along a line: a hole's wall
    // touching the wall round it), one triangle either side.
    let mut runs: HashMap<(u32, u32), u32> = HashMap::new();
    for r in ranges {
        for k in 0..r.len() {
            let (a, b) = segment(r, k);
            if let Some(k) = side(a, b) {
                *runs.entry(k).or_default() += 1;
            }
        }
    }
    runs.iter().all(|(k, n)| sides.get(k).copied() == Some(*n))
}

#[cfg(test)]
mod refusals;
#[cfg(test)]
mod tests;
