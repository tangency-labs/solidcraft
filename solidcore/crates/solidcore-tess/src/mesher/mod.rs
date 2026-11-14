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
//! 3. a face even that refuses because its loops cross in (u, v) (a cusp, a sliver whose sides
//!    sag across each other) is still a defect upstream, in the record's pcurves or seams, and is
//!    named in the plan's stats ([`PlanStats::lenient`], with why it was refused) so it is
//!    reported and fixed there; it is filled leniently in (u, v) (the sides crossing one already
//!    kept are left out, see [`Lenient`]) so the display has no hole. A face that refuses even
//!    that (its loops do not close in (u, v), or wind round a seam or pole) is left out of the
//!    plan with no triangles and named in [`PlanStats::unmeshed`], and the rest of the body is
//!    meshed; there is no other mesher to fall back to. A face whose pcurves do not lie on its edges is an error naming the face and
//!    the edge, as is a body none of whose faces can be planned.

// SolidCraft's mesher, moved here as it is (RFC 0003, item 1.1) so it stays the same text as its
// history. Its slice indexing is checked access, and a panic in planning is not caught (planning
// returns its failures as errors); the rest of solidcore's lints come in the next step of the item.
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::type_complexity,
    clippy::single_match_else,
    clippy::items_after_statements,
    clippy::redundant_closure_for_method_calls,
    clippy::doc_markdown,
    clippy::needless_for_each,
    clippy::nonminimal_bool,
    clippy::semicolon_if_nothing_returned,
    clippy::explicit_iter_loop,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::cast_lossless,
    clippy::map_unwrap_or
)]

pub(crate) mod cdt;
pub(crate) mod collar;
pub(crate) mod coons;
pub(crate) mod cutgrid;
pub(crate) mod edges;
pub(crate) mod grid;
pub(crate) mod motorcycle;
pub(crate) mod pieces;
pub(crate) mod poles;
pub(crate) mod sag;
pub(crate) mod seam;
pub(crate) mod shape;
pub(crate) mod size;
pub(crate) mod surface;
pub(crate) mod tangle;
pub(crate) mod uv;

pub use size::MAX_ANGLE_BOUNDARY;
pub use crate::stats::{PlanStats, RefusedFace};

use crate::vec::VecExt as _;
use std::collections::HashMap;

use solidcore_convert as kernel;

use solidcore::Vec3;
use solidcore_record::brep::{self, Curve, Loop, Shell};
use solidcore::eval::plan::EvalSurface;
use crate::{FacePlan, TessPlan};

use crate::{Result, TessError};

/// The three of a triangle's corners (or sides) read from number `k` on, wrapping round.
pub(crate) fn rot3<T>(mut a: [T; 3], k: usize) -> [T; 3] {
    a.rotate_left(k % 3);
    a
}

/// Component `d` of a parameter pair. `d` is 0 or 1 by construction everywhere it is passed; any
/// other direction reads as NaN, so no comparison on it holds and nothing is planned from it.
pub(crate) fn comp(p: &[f64; 2], d: usize) -> f64 {
    p.get(d).copied().unwrap_or(f64::NAN)
}

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
/// The smallest angle the triangles of a flat face with no size keep, where its boundary
/// allows. Its boundary nodes alone make slivers (a disc's rim fanned to one node); at 15 degrees
/// the p99 aspect of such faces is below 2.6 for about a quarter more triangles (10 degrees
/// leaves it above 3, 20 adds a third), measured on the bake-off corpus.
const FLAT_MIN_ANGLE_DEG: f64 = 15.0;
/// Most points Ruppert refinement may add to one face, beyond its seeds.
const MAX_REFINE: usize = 20_000;
/// Most points the mesh path's refinement for sag, side length and shape may add to one face,
/// beyond its seeds ([`Purpose::Mesh`]; as many as its lattice may have): past it the face is
/// named in [`PlanStats::unshaped`] ([`crate::ShapeLimit::Points`]).
const MAX_MESH_REFINE: usize = 200_000;

/// What a plan is for: the display's planner ([`plan`]), which takes the cheapest layout that
/// suits each face, or the mesh path ([`crate::mesh()`]), which sends every face to constrained
/// Delaunay with lattice seeds, refinement and shaping, and fills nothing leniently.
#[derive(Clone, Copy)]
pub(crate) enum Purpose<'a> {
    Display,
    Mesh(Shape<'a>),
}

/// The mesh path's rules beside the chord tolerance ([`crate::MeshSpec`]).
#[derive(Clone, Copy)]
pub(crate) struct Shape<'a> {
    /// The smallest angle Ruppert refinement aims for (degrees, in (0, 30]).
    pub(crate) min_angle: f64,
    /// The longest side asked anywhere (mm); `None`: the rules' longest side
    /// ([`size::Rules::max_len`]).
    pub(crate) side: Option<f64>,
    /// The caller's size function, where it gives one.
    pub(crate) size_at: Option<size::SizeAt<'a>>,
    /// Whether the face's shaping may split its boundary sides.
    pub(crate) sides: Sides,
}

/// Whether the mesh path's shaping of a face may split its boundary sides ([`cdt::mesh`]).
/// Ruppert refinement reaches its angle by splitting the sides a new point would encroach on;
/// a side is an edge's segment, which the faces either side share, so a split is not kept in
/// the face: it is put on the edge, as a node both faces take, and they are planned again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sides {
    /// It may: the sides it splits are returned ([`Planned::Split`]).
    Split,
    /// It may not: its shaping split only sides on no edge (a pole's collapsed side, the line a
    /// seamless face is cut along), which no node can be put on.
    Kept,
    /// It may not: the rounds of splitting ran out ([`MAX_SPLIT_ROUNDS`]); where that leaves
    /// it short, the face is named ([`crate::ShapeLimit::Rounds`]).
    Exhausted,
}

/// Rounds of putting the boundary sides the mesh path's shaping splits on their edges and
/// planning the faces of those edges again.
const MAX_SPLIT_ROUNDS: usize = 16;

/// A face planned: its plan, or on the mesh path the boundary sides its shaping split.
pub(crate) enum Planned {
    Face(FacePlan),
    Split(Vec<SplitSide>),
}

/// A boundary side the mesh path's shaping split: its two end points (the shared edge nodes, in
/// the face's loop order) and the fractions along it, in the triangulation's metric, where it
/// was split.
pub(crate) struct SplitSide {
    from: Vec3,
    to: Vec3,
    at: Vec<f64>,
}

/// Most rounds of splitting the edge segments where a face's sampled boundary crosses itself:
/// each round halves them, so their chords sag a quarter as far.
const MAX_UNTANGLE: u32 = 16;

/// An error's own words, without its kind's prefix, for a message that names where it happened.
fn detail(e: &TessError) -> std::borrow::Cow<'_, str> {
    match e {
        TessError::Invalid(m) | TessError::Failed(m) | TessError::Internal(m) => std::borrow::Cow::Borrowed(m),
        TessError::TooManyVertices { .. } => std::borrow::Cow::Owned(e.to_string()),
    }
}

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
        brep::Surface::Blend { .. } => "blend",
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
/// tolerance `t` ([`crate::MeshTolerance`]): faces and edges are the record's, in its order.
pub fn plan(b: &brep::Brep, t: &crate::MeshTolerance, size: f64) -> Result<(TessPlan, PlanStats)> {
    let (tol, turn) = t.rules(size);
    plan_at(b, tol, turn, Purpose::Display)
}

/// What the planner reads of a body before it plans any face, in the record's order: each face's
/// surface (in the evaluator's parameters), target size and boundary loops in (u, v), every
/// edge's shared nodes, and why each face left out is. [`plan_at`] plans the faces from it.
pub(crate) struct Prepared {
    /// The rules every edge was sampled to.
    pub(crate) rules: size::Rules,
    /// Whether each face's normal is its surface's natural normal.
    pub(crate) forward: Vec<bool>,
    /// The faces each edge bounds.
    pub(crate) edge_faces: Vec<Vec<usize>>,
    /// Each face's surface (a plane in the place of a face left out).
    pub(crate) evals: Vec<poles::Face>,
    /// Each face's target side length and its surface's mean speed along u and v.
    pub(crate) sizes: Vec<(f64, [f64; 2])>,
    /// Every edge's shared nodes.
    pub(crate) nodes: Vec<edges::Nodes>,
    /// The edges that bound no face and could not be sampled.
    pub(crate) unsampled_edges: Vec<crate::RefusedEdge>,
    /// The edges kept to the strict rules though their faces are crowded.
    pub(crate) kept_strict: Vec<crate::RefusedEdge>,
    boundaries: Vec<Boundary>,
    /// How each face's pcurves map to its surface's parameters.
    params: Vec<surface::Params>,
    /// Why each face is left out, where it is.
    pub(crate) why: Vec<Option<TessError>>,
    /// The faces whose boundary still crosses itself, and why.
    pub(crate) tangled: Vec<(usize, String)>,
}

/// Plan the tessellation of `b` with chord tolerance `tol` and, when given, a facet angle no
/// facet may turn more than (where it sags above the angle's floor), for `purpose`. Faces and
/// edges are the record's, in its order.
pub(crate) fn plan_at(b: &brep::Brep, tol: f64, turn: Option<size::Turn>, purpose: Purpose<'_>) -> Result<(TessPlan, PlanStats)> {
    let faces = &b.faces;
    let Prepared { rules, forward, edge_faces, evals, sizes, mut nodes, unsampled_edges, kept_strict, mut boundaries, params, why, tangled } =
        prepare(b, tol, turn, purpose)?;
    let curved = evals.iter().filter(|e| !matches!(***e, EvalSurface::Plane { .. })).count();
    // One face for `purpose`: its plan, oriented, with its poles of higher order; or the boundary
    // sides the mesh path's shaping split.
    let plan_one = |fi: usize, purpose, boundaries: &[Boundary]| -> Result<(Planned, PlanStats)> {
        if let Some(Some(e)) = why.get(fi) {
            return Err(e.clone());
        }
        let (Some(face), Some(eval), Some(size), Some(fb)) = (faces.get(fi), evals.get(fi), sizes.get(fi), boundaries.get(fi)) else {
            return Err(unsupported(format!("face {fi}: missing")));
        };
        let refused = tangled.iter().find(|(f, _)| *f == fi).map(|(_, why)| why.as_str());
        let result = plan_or_fill(fi, kind(face.parameters()), eval, &fb.loops, &rules, *size, fb.reach, refused, purpose);
        // A point whose poles the kernel could not tell refuses the face: nothing is taken for
        // its answer. Read before the plan's own error, which may be a later symptom of it.
        if let Some(why) = eval.refusal() {
            return Err(unsupported(format!("face {fi} ({}): {why}", kind(&face.surface))));
        }
        let (fp, stats) = match result? {
            (Planned::Face(fp), stats) => (fp, stats),
            split => return Ok(split),
        };
        let fwd = forward.get(fi).copied().unwrap_or(true);
        let fp =
            FacePlan { flip: !fwd, triangles: if fwd { fp.triangles } else { fp.triangles.into_iter().map(|[a, b, c]| [a, c, b]).collect() }, ..fp };
        // Its poles of higher order and the normals they get, on the oriented triangles, by the
        // kernel's evaluator ([`crate::pole`]).
        let (poles, averaged) = crate::pole::plan_poles(fi, eval.kernel(), &fp, fb.reach);
        let mut stats = stats;
        stats.averaged_poles = averaged;
        Ok((Planned::Face(FacePlan { poles, ..fp }), stats))
    };
    // Every face once; on the mesh path, the boundary sides a face's shaping splits are put on
    // their edges as nodes, and every face of those edges is planned again, until none splits
    // one (or the rounds run out, when the faces left keep their sides whole and are named).
    let shape = match purpose {
        Purpose::Mesh(shape) => Some(shape),
        Purpose::Display => None,
    };
    let mut slots: Vec<Option<Result<(FacePlan, PlanStats)>>> = faces.iter().map(|_| None).collect();
    // The faces whose shaping split only sides on no edge: planned with their sides whole.
    let mut kept = vec![false; faces.len()];
    let mut todo: Vec<usize> = (0..faces.len()).collect();
    for round in 0..=MAX_SPLIT_ROUNDS {
        let last = round == MAX_SPLIT_ROUNDS;
        let done = crate::par::map("face tessellation", &todo, curved >= PARALLEL_FACES, |&fi| {
            let purpose = match shape {
                Some(shape) => {
                    let sides = if last { Sides::Exhausted } else if kept.get(fi).copied().unwrap_or(true) { Sides::Kept } else { Sides::Split };
                    Purpose::Mesh(Shape { sides, ..shape })
                }
                None => Purpose::Display,
            };
            plan_one(fi, purpose, &boundaries)
        });
        // The new nodes of each edge (curve parameters), in edge order.
        let mut split: std::collections::BTreeMap<usize, Vec<f64>> = std::collections::BTreeMap::new();
        let mut again: Vec<usize> = Vec::new();
        for (&fi, r) in todo.iter().zip(done) {
            let r = match r {
                Ok((Planned::Face(fp), s)) => Ok((fp, s)),
                Ok((Planned::Split(sides), _)) => {
                    let on = faces.get(fi).map_or_else(Vec::new, |f| on_edges(f, &nodes, &sides));
                    if on.is_empty() {
                        if let Some(k) = kept.get_mut(fi) {
                            *k = true;
                        }
                        again.push(fi);
                    }
                    for (e, t) in on {
                        split.entry(e).or_default().push(t);
                    }
                    continue;
                }
                Err(e) => Err(e),
            };
            if let Some(slot) = slots.get_mut(fi) {
                *slot = Some(r);
            }
        }
        let (mut moved, mut lost): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        for (e, ts) in &split {
            let fs = edge_faces.get(*e).map_or(&[][..], Vec::as_slice);
            let inserted = nodes.get_mut(*e).map_or(Ok(()), |n| edges::insert_at(b, *e, n, ts));
            match inserted {
                Ok(()) => moved.extend(fs),
                // An edge that cannot take the nodes leaves out the faces it bounds, naming it.
                Err(why) => {
                    for &fi in fs {
                        if let (Some(slot), Some(f)) = (slots.get_mut(fi), faces.get(fi)) {
                            *slot = Some(Err(unsupported(format!("face {fi} ({}): {}", kind(&f.surface), detail(&why)))));
                        }
                    }
                    lost.extend(fs);
                }
            }
        }
        moved.sort_unstable();
        moved.dedup();
        // The faces of an edge that has new nodes read their boundary again, and may split once more.
        for &fi in &moved {
            if let Some(k) = kept.get_mut(fi) {
                *k = false;
            }
            let rebuilt = match (faces.get(fi), evals.get(fi), params.get(fi)) {
                (Some(face), Some(eval), Some(map)) => boundary(b, fi, face, eval, map, &nodes, &rules),
                _ => Err(unsupported(format!("face {fi}: missing"))),
            };
            match rebuilt {
                Ok(fb) => {
                    if let Some(slot) = boundaries.get_mut(fi) {
                        *slot = fb;
                    }
                    again.push(fi);
                }
                Err(e) => {
                    if let Some(slot) = slots.get_mut(fi) {
                        *slot = Some(Err(e));
                    }
                }
            }
        }
        // A face whose edge could not take its nodes stays left out.
        again.retain(|fi| !lost.contains(fi));
        again.sort_unstable();
        again.dedup();
        if again.is_empty() {
            break;
        }
        todo = again;
    }
    let planned: Vec<Result<(FacePlan, PlanStats)>> =
        slots.into_iter().enumerate().map(|(fi, s)| s.unwrap_or_else(|| Err(unsupported(format!("face {fi}: not planned"))))).collect();
    let mut plan = TessPlan::default();
    let mut stats = PlanStats { unsampled_edges, kept_strict, ..PlanStats::default() };
    // A face no method plans is left out, named in the stats, and the rest of the body is
    // meshed; a body none of whose faces plan is the first face's error.
    let mut first_error: Option<TessError> = None;
    for (fi, (r, eval)) in planned.into_iter().zip(evals.into_iter().map(poles::Face::into_eval)).enumerate() {
        let surface = u32::try_from(plan.surfaces.len()).map_err(|_| unsupported("too many faces"))?;
        plan.surfaces.push(eval);
        let (fp, s) = match r {
            Ok(done) => done,
            Err(e) => {
                stats.unmeshed.push(RefusedFace { face: fi, reason: e.to_string() });
                first_error.get_or_insert(e);
                (FacePlan { face: u32::try_from(fi).unwrap_or(u32::MAX), ..FacePlan::default() }, PlanStats::default())
            }
        };
        plan.faces.push(FacePlan { surface: Some(surface), ..fp });
        stats.planar += s.planar;
        stats.structured += s.structured;
        stats.delaunay += s.delaunay;
        stats.lenient.extend(s.lenient);
        stats.averaged_poles.extend(s.averaged_poles);
        stats.over_tolerance.extend(s.over_tolerance);
        stats.unshaped.extend(s.unshaped);
    }
    if let Some(e) = first_error
        && stats.unmeshed.len() == faces.len()
    {
        return Err(e);
    }
    // Edges a bound of their sampling stopped, named with how far their chords lie off them.
    for (edge, n) in nodes.iter().enumerate() {
        if let Some((cap, sag, excess)) = n.capped {
            stats.edges_capped.push(crate::CappedEdge { edge, cap, sag, excess, tol: rules.tol, within: sag <= rules.tol });
        }
    }
    plan.edges = nodes.into_iter().map(|n| n.points.into_iter().map(kernel::record_vec3).collect()).collect();
    plan.edge_faces = edge_faces.into_iter().map(|fs| fs.into_iter().map(|f| u32::try_from(f).unwrap_or(u32::MAX)).collect()).collect();
    Ok((plan, stats))
}

/// The curve parameters on face `f`'s edges where its shaping split boundary sides `sides`:
/// each side is found as a segment of one of its edges (its two end nodes, either way round),
/// each fraction along it taken to the same fraction of the segment's parameters. A side on no
/// edge (a pole's collapsed side, a seam cut's line) has none.
fn on_edges(f: &brep::Face, nodes: &[edges::Nodes], sides: &[SplitSide]) -> Vec<(usize, f64)> {
    let mut out = Vec::new();
    for side in sides.iter().filter(|s| s.from != s.to) {
        let found = face_edges(f).find_map(|oe| {
            let e = oe.edge as usize;
            let n = nodes.get(e)?;
            n.points.windows(2).zip(n.params.windows(2)).find_map(|(p, t)| match (p, t) {
                ([a, b], [ta, tb]) if *a == side.from && *b == side.to => Some((e, *ta, *tb, false)),
                ([a, b], [ta, tb]) if *a == side.to && *b == side.from => Some((e, *ta, *tb, true)),
                _ => None,
            })
        });
        if let Some((e, ta, tb, reversed)) = found {
            out.extend(side.at.iter().map(|&f| (e, ta + (if reversed { 1.0 - f } else { f }) * (tb - ta))));
        }
    }
    out
}

/// Every face's surface, size and boundary and every edge's nodes, as [`plan_at`] reads them
/// ([`Prepared`]), with chord tolerance `tol` and the facet angle `turn`, for `purpose`: on the
/// mesh path every face, planar ones included, asks for the spec's side where its surface asks
/// for no less, and every edge follows the caller's size function too.
pub(crate) fn prepare(b: &brep::Brep, tol: f64, turn: Option<size::Turn>, purpose: Purpose<'_>) -> Result<Prepared> {
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
    // Why each face is left out of the plan, where one is: its surface does not evaluate, an
    // edge of it cannot be sampled, or its boundary cannot be read. The rest of the body is
    // meshed; each such face is named in the stats with why.
    let mut why: Vec<Option<TessError>> = vec![None; faces.len()];
    // A face left out keeps a plane in its place, so the plan's surfaces stay one per face.
    let stand_in = || (EvalSurface::Plane { origin: Vec3::ZERO, du: Vec3::X, dv: Vec3::Y }, surface::Params::Same);
    let evals: Vec<(EvalSurface, surface::Params)> = faces
        .iter()
        .enumerate()
        .map(|(fi, f)| match surface::eval_surface(f.parameters(), f.domain) {
            Ok(e) => e,
            Err(e) => {
                if let Some(slot) = why.get_mut(fi) {
                    *slot = Some(unsupported(format!("face {fi} ({}): {}", kind(&f.surface), detail(&e))));
                }
                stand_in()
            }
        })
        .collect();
    let (evals, params): (Vec<EvalSurface>, Vec<surface::Params>) = evals.into_iter().unzip();
    // Each face's surface with the kernel's form of it, whose classifier decides its poles.
    // A face's tolerance: the largest of its edges' and their vertices', as the record carries
    // them (a STEP file's uncertainty, an edge's own); 0 where none is given.
    let face_tol = |f: &brep::Face| {
        face_edges(f)
            .filter_map(|oe| b.edges.get(oe.edge as usize))
            .flat_map(|e| std::iter::once(e.tolerance).chain(e.vertices.iter().filter_map(|v| b.vertices.get(*v as usize)).map(|v| v.tolerance)))
            .filter(|t| t.is_finite())
            .fold(0.0f64, f64::max)
    };
    let evals: Vec<poles::Face> = evals.into_iter().zip(faces).map(|(e, f)| poles::Face::new(e, face_tol(f))).collect();
    let curved = evals.iter().filter(|e| !matches!(***e, EvalSurface::Plane { .. })).count();
    let sizes: Vec<(f64, [f64; 2])> =
        crate::par::map("face sizes", &evals, curved >= PARALLEL_SIZES, |s| Ok(size::face_size(s, &uv::domain(s), &rules)))
            .into_iter()
            .collect::<Result<_>>()?;
    let (sizes, size_at) = match purpose {
        Purpose::Display => (sizes, None),
        Purpose::Mesh(shape) => {
            let side = shape.side.unwrap_or(rules.max_len);
            (sizes.into_iter().map(|(h, speed)| (h.min(side), speed)).collect(), shape.size_at)
        }
    };
    let face_h = |f: usize| sizes.get(f).map_or(rules.max_len, |(h, _)| *h);
    // A flat face asks for no size (infinite): an edge between flat faces is whole, one beside
    // a curved face takes the curved face's size, so both meet at the same nodes.
    let edge_h: Vec<f64> =
        edge_faces.iter().map(|fs| if fs.is_empty() { rules.max_len } else { fs.iter().map(|&f| face_h(f)).fold(f64::INFINITY, f64::min) }).collect();
    let jobs: Vec<(usize, f64)> = edge_h.into_iter().enumerate().collect();
    let solved = b.edges.iter().filter(|e| matches!(e.curve, Curve::SurfaceIntersection { .. })).count();
    // An edge that cannot be sampled leaves out every face it bounds, each naming it, and has no
    // nodes in the plan; one that bounds no face is named in the stats on its own.
    let mut unsampled_edges: Vec<crate::RefusedEdge> = Vec::new();
    let mut nodes: Vec<edges::Nodes> = Vec::with_capacity(jobs.len());
    for (ei, r) in crate::par::map("edge nodes", &jobs, solved >= PARALLEL_CURVES || jobs.len() >= PARALLEL_EDGES, |(e, h)| {
        edges::sample_edge_sized(b, *e, &rules, *h, size_at)
    })
    .into_iter()
    .enumerate()
    {
        nodes.push(match r {
            Ok(n) => n,
            Err(e) => {
                let fs = edge_faces.get(ei).map_or(&[][..], Vec::as_slice);
                if fs.is_empty() {
                    unsampled_edges.push(crate::RefusedEdge { edge: ei, reason: e.to_string() });
                }
                for &fi in fs {
                    if let (Some(slot @ None), Some(f)) = (why.get_mut(fi), faces.get(fi)) {
                        *slot = Some(unsupported(format!("face {fi} ({}): {}", kind(&f.surface), detail(&e))));
                    }
                }
                edges::Nodes::default()
            }
        });
    }
    let kept_strict = relax_crowded_faces(b, &jobs, &rules, &mut nodes);
    for (ei, e) in split_thin_loops(b, &mut nodes) {
        for &fi in edge_faces.get(ei).map_or(&[][..], Vec::as_slice) {
            if let (Some(slot @ None), Some(f)) = (why.get_mut(fi), faces.get(fi)) {
                *slot = Some(unsupported(format!("face {fi} ({}): {}", kind(&f.surface), detail(&e))));
            }
        }
    }
    let indexed: Vec<usize> = (0..faces.len()).collect();
    let face_boundary = |fi: usize, nodes: &[edges::Nodes]| {
        let (Some(face), Some(eval), Some(map)) = (faces.get(fi), evals.get(fi), params.get(fi)) else {
            return Err(unsupported(format!("face {fi}: missing")));
        };
        boundary(b, fi, face, eval, map, nodes, &rules)
    };
    let skip: Vec<bool> = why.iter().map(Option::is_some).collect();
    let mut boundaries: Vec<Boundary> = Vec::with_capacity(faces.len());
    for (fi, r) in crate::par::map("face boundaries", &indexed, curved >= PARALLEL_FACES, |&fi| {
        if skip.get(fi).copied().unwrap_or(true) {
            return Ok(Boundary::default());
        }
        face_boundary(fi, &nodes)
    })
    .into_iter()
    .enumerate()
    {
        boundaries.push(match r {
            Ok(fb) => fb,
            Err(e) => {
                if let Some(slot @ None) = why.get_mut(fi) {
                    *slot = Some(e);
                }
                Boundary::default()
            }
        });
    }
    let tangled = untangle(b, &evals, &edge_faces, rules.tol, &mut nodes, &mut boundaries, &face_boundary, &mut why)?;
    Ok(Prepared { rules, forward, edge_faces, evals, sizes, nodes, unsampled_edges, kept_strict, boundaries, params, why, tangled })
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
    s: &poles::Face,
    map: &surface::Params,
    nodes: &[edges::Nodes],
    rules: &size::Rules,
) -> Result<Boundary> {
    let dom = uv::domain(s);
    let fail = |e: u32, why: String| unsupported(format!("face {fi} ({}): edge {e}: {why}", kind(&face.surface)));
    // How near a pole a node is one: the tolerance of a vertex of the face that lies on a patch
    // side collapsed to within it, not to a point, as the record check takes a loop's pole; at
    // most the chord tolerance, finer than which the mesh resolves nothing (a vertex tolerance
    // as large as the surface's features would make a pole of every node near one).
    let mut reach: f64 = 0.0;
    for oe in face_edges(face) {
        let (Some(edge), Some([p0, p1])) = (b.edges.get(oe.edge as usize), oe.pcurve.interval()) else { continue };
        let tol = edge.vertices.get(usize::from(!oe.forward)).and_then(|v| b.vertices.get(*v as usize)).map_or(0.0, |v| v.tolerance.min(rules.tol));
        let Some(q) = solidcore_convert::curve2(&oe.pcurve).eval(if oe.forward { p0 } else { p1 }).map(|q| map.to_eval(q)) else { continue };
        if tol.is_finite() && tol > reach && (0..2).any(|d| s.pole(q, d, tol)) {
            reach = tol;
        }
    }
    let pole_along = |q: [f64; 2], d: usize| s.pole(q, d, reach);
    let mut out = Boundary { loops: Vec::with_capacity(face.loops.len()), segments: Vec::with_capacity(face.loops.len()), reach };
    // Each vertex's parameters, from the first edge use of the face to start there.
    let mut at_vertex: HashMap<u32, [f64; 2]> = HashMap::new();
    // The face's vertices placed so far, by cell of a grid as fine as the chord tolerance: the
    // nearness below is never farther than that, so the vertex's own cell and its neighbours hold
    // every candidate (a face of 100,000 vertices is not searched once per vertex).
    let cell = if rules.tol.is_finite() && rules.tol > 0.0 { rules.tol } else { f64::INFINITY };
    let key = |p: Vec3| [p.x, p.y, p.z].map(|x| if cell.is_finite() { (x / cell).floor() as i64 } else { 0 });
    let mut placed: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    // The vertex pairs an edge of the face joins.
    let joined: std::collections::HashSet<(u32, u32)> = face_edges(face)
        .filter_map(|oe| b.edges.get(oe.edge as usize))
        .map(|e| (e.vertices[0].min(e.vertices[1]), e.vertices[0].max(e.vertices[1])))
        .collect();
    let vertex_point = |v: u32| b.vertices.get(v as usize).map(|x| (Vec3::new(x.point[0], x.point[1], x.point[2]), x.tolerance.max(0.0)));
    // Each loop's nodes that are another edge's node (see [`edges::snap`]).
    let mut snapped: Vec<Vec<bool>> = Vec::with_capacity(face.loops.len());
    for l in &face.loops {
        let Loop::Edges { edges } = l else { continue };
        let (mut pts, mut uvs): (Vec<Vec3>, Vec<[f64; 2]>) = (Vec::new(), Vec::new());
        let mut segs: Vec<(u32, u32)> = Vec::new();
        let mut shared: Vec<bool> = Vec::new();
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
            let mut es: Vec<bool> = n.params.iter().map(|t| n.snapped.contains(t)).collect();
            let mut eq: Vec<[f64; 2]> = Vec::with_capacity(ep.len());
            for (k, (t, p)) in n.params.iter().zip(&n.points).enumerate() {
                let q = solidcore_convert::curve2(&oe.pcurve).eval(t.clamp(p0, p1))
                    .ok_or_else(|| fail(oe.edge, format!("its pcurve does not evaluate at t = {t}")))?;
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
                es.reverse();
            }
            if let (Some(e), Some(&start)) = (end, eq.first()) {
                for (d, per) in dom.period.into_iter().enumerate() {
                    let Some(per) = per else { continue };
                    if !(pole_along(e, d) || pole_along(start, d))
                        && let Some(sh) = shift.get_mut(d)
                    {
                        *sh = per * ((comp(&e, d) - comp(&start, d)) / per).round();
                    }
                }
            }
            for q in &mut eq {
                q[0] += shift[0];
                q[1] += shift[1];
            }
            if let (Some(&v), Some(q)) = (edge.vertices.get(usize::from(!oe.forward)), eq.first_mut()) {
                match at_vertex.get(&v) {
                    Some(first) => {
                        if first.iter().zip(q.iter()).all(|(f, x)| (f - x).abs() <= 1e-9 * f.abs().max(x.abs()).max(1.0)) {
                            *q = *first;
                        }
                    }
                    None => {
                        // Another vertex of the face within the tolerance of both (and of the
                        // chord tolerance), and not joined to this one by an edge (which makes
                        // them two points however near: a step that small), is the same point:
                        // the record's corners a hair apart (a planar boolean's, where a cut-out's
                        // corner touches another loop's corner) are one point of the face, so its
                        // loops touch there instead of crossing.
                        let same = vertex_point(v).and_then(|(p, tv)| {
                            let [x, y, z] = key(p);
                            let near = (-1..=1).flat_map(|i| (-1..=1).flat_map(move |j| (-1..=1).map(move |k| [x + i, y + j, z + k])));
                            near.filter_map(|c| placed.get(&c))
                                .flatten()
                                .filter(|w| **w != v && !joined.contains(&(v.min(**w), v.max(**w))))
                                .filter_map(|w| {
                                    let (pw, tw) = vertex_point(*w)?;
                                    let d = p.dist(pw);
                                    (d <= tv.max(tw).min(rules.tol)).then_some((d, *w))
                                })
                                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                                .and_then(|(_, w)| at_vertex.get(&w).copied())
                        });
                        if let Some(mut w) = same.filter(|w| !(0..2).any(|d| pole_along(*w, d) || pole_along(*q, d))) {
                            for (d, per) in dom.period.into_iter().enumerate() {
                                if let Some(wd) = w.get_mut(d) {
                                    *wd = uv::unwrap(*wd, comp(q, d), per);
                                }
                            }
                            *q = w;
                        }
                        at_vertex.insert(v, *q);
                        if let Some((p, _)) = vertex_point(v) {
                            placed.entry(key(p)).or_default().push(v);
                        }
                    }
                }
            }
            end = eq.last().copied();
            ep.pop();
            eq.pop();
            es.pop();
            // The segment from each node on: the edge's own, counted from its start.
            let m = ep.len() as u32;
            segs.extend((0..m).map(|j| (oe.edge, if oe.forward { j } else { m - 1 - j })));
            pts.extend(ep);
            uvs.extend(eq);
            shared.extend(es);
        }
        let n = uvs.len();
        let pole = |q: &[f64; 2]| (0..2).any(|d| pole_along(*q, d));
        let off = |k: usize| uvs.get(k).is_some_and(|q| !pole(q));
        // A node off the poles whose predecessor is off them too; failing that (a whole sphere's
        // seam in two segments: pole, equator, pole), one whose predecessor is a pole on the same
        // edge, so the step into it is the edge's own and the step into that pole, from the
        // edge before, is in the loop where it is unwrapped like any other.
        let same_edge = |k: usize| segs.get((k + n - 1) % n).zip(segs.get(k)).is_some_and(|(a, b)| a.0 == b.0);
        let start = (0..n).find(|&k| off(k) && off((k + n - 1) % n)).or_else(|| (0..n).find(|&k| off(k) && same_edge(k)));
        if let Some(k) = start {
            uvs.rotate_left(k);
            pts.rotate_left(k);
            segs.rotate_left(k);
            shared.rotate_left(k);
        }
        if pts.len() >= 2 {
            out.loops.push(seam::Loop { uv: uvs, pts });
            out.segments.push(segs);
            snapped.push(shared);
        }
    }
    // A node snapped from another edge is that edge's node, the same point: where the face runs
    // that edge too, the node takes the other's parameters, so the face's boundary passes one
    // point of its (u, v) twice (a pinch) and does not cross itself there. Its own pcurve puts it
    // within the chord tolerance of that point, not on it.
    for (l, flags) in snapped.iter().enumerate() {
        for (k, _) in flags.iter().enumerate().filter(|(_, f)| **f) {
            let Some((&p, &q)) = out.loops.get(l).and_then(|lp| lp.pts.get(k).zip(lp.uv.get(k))) else { continue };
            let partner = out
                .loops
                .iter()
                .zip(&snapped)
                .flat_map(|(lp, fs)| lp.pts.iter().zip(&lp.uv).zip(fs))
                .filter(|((r, _), f)| !**f && **r == p)
                .map(|((_, w), _)| *w)
                .min_by(|a, b| (a[0] - q[0]).hypot(a[1] - q[1]).total_cmp(&(b[0] - q[0]).hypot(b[1] - q[1])));
            if let (Some(mut w), Some(slot)) = (partner, out.loops.get_mut(l).and_then(|lp| lp.uv.get_mut(k))) {
                for (d, per) in dom.period.into_iter().enumerate() {
                    if per.is_some()
                        && let Some(wd) = w.get_mut(d)
                    {
                        *wd = uv::unwrap(*wd, comp(&q, d), per);
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
/// point of the face is taken as one (see [`poles::Face::pole`]): the tolerance of a vertex
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
/// put more than [`edges::MAX_EDGE_NODES`] on an edge, are an error naming the face and edges.
#[allow(clippy::too_many_arguments)]
fn untangle(
    b: &brep::Brep,
    evals: &[poles::Face],
    edge_faces: &[Vec<usize>],
    tol: f64,
    nodes: &mut [edges::Nodes],
    boundaries: &mut [Boundary],
    face_boundary: &(dyn Fn(usize, &[edges::Nodes]) -> Result<Boundary> + Sync),
    why: &mut [Option<TessError>],
) -> Result<Vec<(usize, String)>> {
    // An edge that cannot be split further (it has all the nodes an edge may have) leaves out
    // the faces it bounds, each naming it; so does a face whose boundary cannot then be read.
    let refuse = |fi: usize, e: &TessError, why: &mut [Option<TessError>]| {
        if let (Some(slot @ None), Some(f)) = (why.get_mut(fi), b.faces.get(fi)) {
            *slot = Some(unsupported(format!("face {fi} ({}): {}", kind(&f.surface), detail(e))));
        }
    };
    let crossing = |fi: usize, fb: &Boundary| -> Vec<(u32, u32)> {
        let Some(s) = evals.get(fi) else { return Vec::new() };
        tangle::crossing(s, fb.reach, &fb.loops).into_iter().filter_map(|(l, k)| fb.segments.get(l)?.get(k).copied()).collect()
    };
    let indexed: Vec<usize> = (0..boundaries.len()).collect();
    let curved = evals.iter().filter(|e| !matches!(***e, EvalSurface::Plane { .. })).count();
    let left_out: Vec<bool> = why.iter().map(Option::is_some).collect();
    let found: Vec<Vec<(u32, u32)>> = crate::par::map("boundary crossings", &indexed, curved >= PARALLEL_FACES, |&fi| {
        if left_out.get(fi).copied().unwrap_or(false) {
            return Ok(Vec::new());
        }
        Ok(boundaries.get(fi).map_or_else(Vec::new, |fb| crossing(fi, fb)))
    })
    .into_iter()
    .collect::<Result<_>>()?;
    let mut tangled: Vec<(usize, Vec<(u32, u32)>)> = found.into_iter().enumerate().filter(|(_, c)| !c.is_empty()).collect();
    for round in 0..=MAX_UNTANGLE {
        if tangled.is_empty() {
            return Ok(Vec::new());
        }
        if round == MAX_UNTANGLE {
            // Halving cannot part two curves that touch or cross: where they come within the chord
            // tolerance of each other, they share nodes.
            let faces: Vec<usize> = tangled.iter().map(|(fi, _)| *fi).collect();
            tangled = snap_touches(evals, edge_faces, tol, nodes, boundaries, face_boundary, &faces)?;
            // Each face still crossing, with why: planned leniently, and named (`plan_or_fill`).
            return Ok(tangled
                .iter()
                .map(|(fi, c)| {
                    let mut es: Vec<u32> = c.iter().map(|(e, _)| *e).collect();
                    es.sort_unstable();
                    es.dedup();
                    let kind = b.faces.get(*fi).map_or("?", |f| kind(&f.surface));
                    (*fi, format!("face {fi} ({kind}): boundary crosses itself: edges {es:?} run closer together than their chords can follow after {MAX_UNTANGLE} halvings"))
                })
                .collect());
        }
        let mut split: Vec<(u32, u32)> = tangled.iter().flat_map(|(_, c)| c.iter().copied()).collect();
        split.sort_unstable();
        split.dedup();
        let mut redo: Vec<usize> = Vec::new();
        for run in split.chunk_by(|a, c| a.0 == c.0) {
            let Some(&(e, _)) = run.first() else { continue };
            let segs: Vec<u32> = run.iter().map(|(_, k)| *k).collect();
            let Some(slot) = nodes.get_mut(e as usize) else { continue };
            if let Err(err) = edges::split_segments(b, e as usize, slot, &segs) {
                for &fi in edge_faces.get(e as usize).into_iter().flatten() {
                    refuse(fi, &err, why);
                }
                continue;
            }
            redo.extend(edge_faces.get(e as usize).into_iter().flatten().copied());
        }
        redo.sort_unstable();
        redo.dedup();
        tangled.clear();
        for fi in redo {
            if why.get(fi).is_some_and(Option::is_some) {
                continue;
            }
            let fb = match face_boundary(fi, nodes) {
                Ok(fb) => fb,
                Err(err) => {
                    if let Some(slot @ None) = why.get_mut(fi) {
                        *slot = Some(err);
                    }
                    continue;
                }
            };
            let c = crossing(fi, &fb);
            if let Some(slot) = boundaries.get_mut(fi) {
                *slot = fb;
            }
            if !c.is_empty() {
                tangled.push((fi, c));
            }
        }
    }
    Ok(Vec::new())
}

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
#[allow(clippy::too_many_arguments)]
fn snap_touches(
    evals: &[poles::Face],
    edge_faces: &[Vec<usize>],
    tol: f64,
    nodes: &mut [edges::Nodes],
    boundaries: &mut [Boundary],
    face_boundary: &(dyn Fn(usize, &[edges::Nodes]) -> Result<Boundary> + Sync),
    faces: &[usize],
) -> Result<Vec<(usize, Vec<(u32, u32)>)>> {
    let crossing = |fi: usize, fb: &Boundary| -> Vec<(usize, usize)> {
        let Some(s) = evals.get(fi) else { return Vec::new() };
        tangle::crossing(s, fb.reach, &fb.loops)
    };
    let mut tangled: Vec<usize> = faces.to_vec();
    for _ in 0..MAX_SNAP {
        // In edge order, so the first edge that cannot take its nodes is the same every run.
        let mut add: std::collections::BTreeMap<u32, Vec<(f64, Vec3)>> = std::collections::BTreeMap::new();
        for &fi in &tangled {
            let Some(fb) = boundaries.get(fi) else { continue };
            for (l, k) in crossing(fi, fb) {
                let Some(&(e, j)) = fb.segments.get(l).and_then(|s| s.get(k)) else { continue };
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
            let Some(slot) = nodes.get_mut(*e as usize) else { continue };
            edges::snap(*e as usize, slot, points)?;
            redo.extend(edge_faces.get(*e as usize).into_iter().flatten().copied());
        }
        redo.extend(tangled.iter().copied());
        redo.sort_unstable();
        redo.dedup();
        tangled.clear();
        for fi in redo {
            let fb = face_boundary(fi, nodes)?;
            let crosses = !crossing(fi, &fb).is_empty();
            if let Some(slot) = boundaries.get_mut(fi) {
                *slot = fb;
            }
            if crosses {
                tangled.push(fi);
            }
        }
        if tangled.is_empty() {
            return Ok(Vec::new());
        }
    }
    Ok(tangled
        .into_iter()
        .filter_map(|fi| {
            let fb = boundaries.get(fi)?;
            let c: Vec<(u32, u32)> = crossing(fi, fb).into_iter().filter_map(|(l, k)| fb.segments.get(l)?.get(k).copied()).collect();
            (!c.is_empty()).then_some((fi, c))
        })
        .collect())
}

/// The node of face boundary `fb` nearest the crossing chord `at` (loop, node) of its boundary,
/// segment `seg` (edge, segment from its start) of that edge, within `tol` of the chord strictly
/// between its ends, on another edge, and not yet on this one; with the parameter on the edge
/// where it projects onto the chord. `None` when there is none, or snapping it would turn part
/// of its loop inside out by more than the tolerance (see [`snap_touches`]).
fn touch(fb: &Boundary, nodes: &[edges::Nodes], at: (usize, usize), seg: (u32, u32), tol: f64) -> Option<(f64, Vec3)> {
    let (e, j) = seg;
    let n = nodes.get(e as usize)?;
    let j = j as usize;
    let (a, c) = (*n.points.get(j)?, *n.points.get(j + 1)?);
    let (ta, tc) = (*n.params.get(j)?, *n.params.get(j + 1)?);
    let ac = c - a;
    let len2 = ac.dot(ac);
    if !(len2 > 0.0) {
        return None;
    }
    let mut best: Option<(f64, f64, Vec3)> = None;
    for (l, lp) in fb.loops.iter().enumerate() {
        let segs = fb.segments.get(l)?;
        let m = lp.pts.len();
        for (k, &p) in lp.pts.iter().enumerate() {
            // The node's edges: the segment starting at it and the one ending there.
            let edges_at = [segs.get(k).map(|s| s.0), segs.get((k + m - 1) % m).map(|s| s.0)];
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
        let mut i = from % m;
        loop {
            out.push(i);
            if i == to % m || out.len() > m {
                break;
            }
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
        let twice = pts.iter().zip(pts.iter().cycle().skip(1)).fold(Vec3::ZERO, |acc, (p, q)| acc + p.cross(*q)).len();
        let length: f64 = pts.iter().zip(pts.iter().cycle().skip(1)).map(|(p, q)| p.dist(*q)).sum();
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
fn split_thin_loops(b: &brep::Brep, nodes: &mut [edges::Nodes]) -> Vec<(usize, TessError)> {
    let mut failed: Vec<(usize, TessError)> = Vec::new();
    for f in &b.faces {
        for l in &f.loops {
            let Loop::Edges { edges: uses } = l else { continue };
            let mut es: Vec<usize> = uses.iter().map(|u| u.edge as usize).collect();
            es.sort_unstable();
            es.dedup();
            let count = |nodes: &[edges::Nodes]| -> usize {
                uses.iter().map(|u| nodes.get(u.edge as usize).map_or(0, |n| n.points.len().saturating_sub(1))).sum()
            };
            // Each round at least doubles every edge's segments: a few rounds always do.
            for _ in 0..MIN_LOOP_SEGMENTS {
                if count(nodes) >= MIN_LOOP_SEGMENTS {
                    break;
                }
                for &e in &es {
                    if failed.iter().any(|(f, _)| *f == e) {
                        continue;
                    }
                    let Some(slot) = nodes.get_mut(e) else { continue };
                    let segs: Vec<u32> = (0..slot.points.len().saturating_sub(1)).filter_map(|k| u32::try_from(k).ok()).collect();
                    // An edge that cannot be split further leaves out the faces it bounds.
                    if let Err(why) = edges::split_segments(b, e, slot, &segs) {
                        failed.push((e, why));
                    }
                }
            }
        }
    }
    failed
}

/// same share whatever the order of the faces.
fn relax_crowded_faces(b: &brep::Brep, jobs: &[(usize, f64)], rules: &size::Rules, nodes: &mut [edges::Nodes]) -> Vec<crate::RefusedEdge> {
    let mut kept = Vec::new();
    let Some(turn) = rules.turn else { return kept };
    let boundaries: Vec<Vec<usize>> = b.faces.iter().map(|f| face_edges(f).map(|oe| oe.edge as usize).collect()).collect();
    let curved = |e: usize| b.edges.get(e).is_some_and(|e| !matches!(e.curve, Curve::Line { .. }));
    let mut level: Vec<u8> = vec![0; nodes.len()];
    for l in 1..=size::MAX_RELAX {
        let segments = |e: &usize| nodes.get(*e).map_or(0, |n| n.points.len().saturating_sub(1));
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
        let relaxed = size::Rules { turn: turn.relaxed(l), ..*rules };
        for e in crowded {
            let Some(&(_, h)) = jobs.get(e) else { continue };
            // An edge whose coarser sampling fails keeps the nodes it has (valid, only finer),
            // named with why.
            if let Some(slot) = nodes.get_mut(e) {
                match edges::sample_edge(b, e, &relaxed, h) {
                    Ok(n) => *slot = n,
                    Err(why) => kept.push(crate::RefusedEdge { edge: e, reason: why.to_string() }),
                }
            }
            if let Some(slot) = level.get_mut(e) {
                *slot = l;
            }
        }
    }
    kept
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
fn open_pole_to_pole(s: &poles::Face, dom: &uv::Domain, reach: f64, l: &mut seam::Loop) {
    let n = l.uv.len();
    let poles: Vec<usize> = (0..n).filter(|&k| l.uv.get(k).is_some_and(|q| (0..2).any(|d| s.pole(*q, d, reach)))).collect();
    let [p0, p1] = poles.as_slice() else { return };
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
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
                if let Some(x) = moved.get_mut(k).and_then(|q| q.get_mut(d)) {
                    *x += sign * per;
                }
            }
            // The poles as the planner splits them: one copy at each neighbour's parameter.
            let mut ring = Vec::with_capacity(n + 2);
            for k in 0..n {
                let (Some(q), Some(a), Some(b)) = (moved.get(k), moved.get((k + n - 1) % n), moved.get((k + 1) % n)) else { continue };
                if k == *p0 || k == *p1 {
                    let (mut qa, mut qb) = (*q, *q);
                    if let Some(x) = qa.get_mut(d) {
                        *x = comp(a, d);
                    }
                    if let Some(x) = qb.get_mut(d) {
                        *x = comp(b, d);
                    }
                    ring.extend([qa, qb]);
                } else {
                    ring.push(*q);
                }
            }
            let (lo, hi) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(comp(q, d)), b.max(comp(q, d))));
            if hi - lo <= per * (1.0 + 1e-9) && area2(&ring) > 0.0 {
                l.uv = moved;
                return;
            }
        }
    }
}

/// The poles of a loop where it turns straight back along a seam: the nodes either side have
/// the same parameters.
fn turning_poles(s: &poles::Face, reach: f64, per: f64, uv: &[[f64; 2]]) -> Vec<usize> {
    let n = uv.len();
    let near = |a: Option<&[f64; 2]>, b: Option<&[f64; 2]>| a.zip(b).is_some_and(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()) <= 1e-9 * per);
    (0..n)
        .filter(|&k| uv.get(k).is_some_and(|q| (0..2).any(|d| s.pole(*q, d, reach))))
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
fn open_at_pole(s: &poles::Face, dom: &uv::Domain, reach: f64, l: &mut seam::Loop) {
    let n = l.uv.len();
    if n < 4 || l.pts.len() != n {
        return;
    }
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        if (seam::drift(l, d, per).abs() - per).abs() > per * 1e-6 {
            continue;
        }
        let [pole] = turning_poles(s, reach, per, &l.uv)[..] else { continue };
        // From the node after the pole round to the pole, unwrapped along the way.
        let order: Vec<usize> = (1..=n).map(|i| (pole + i) % n).collect();
        let mut uv: Vec<[f64; 2]> = Vec::with_capacity(n);
        for &k in &order {
            let Some(mut q) = l.uv.get(k).copied() else { return };
            if let Some(pr) = uv.last() {
                for ((qe, pe), per) in q.iter_mut().zip(pr).zip(dom.period) {
                    *qe = uv::unwrap(*qe, *pe, per);
                }
            }
            uv.push(q);
        }
        // The pole's free parameter: its predecessor's (the poles are split from it later).
        if let (Some(prev), Some(x)) = (uv.len().checked_sub(2).and_then(|i| uv.get(i)).copied(), uv.last_mut().and_then(|q| q.get_mut(d))) {
            *x = comp(&prev, d);
        }
        let (lo, hi) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(comp(q, d)), b.max(comp(q, d))));
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
        let (lo, hi) = l.uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(comp(q, d)), b.max(comp(q, d))));
        0.5 * (lo + hi)
    };
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        let Some(centre) = loops.get(outer).map(|l| middle(l, d)) else { continue };
        for (i, l) in loops.iter_mut().enumerate() {
            let k = ((middle(l, d) - centre) / per).round();
            if i == outer || !k.is_finite() || k == 0.0 {
                continue;
            }
            for x in l.uv.iter_mut().filter_map(|q| q.get_mut(d)) {
                *x -= k * per;
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
/// reason. On the mesh path ([`Purpose::Mesh`]) nothing is filled leniently: the first refusal
/// is the error.
#[allow(clippy::too_many_arguments)]
fn plan_or_fill(
    fi: usize,
    kind: &str,
    s: &poles::Face,
    loops: &[seam::Loop],
    rules: &size::Rules,
    size: (f64, [f64; 2]),
    reach: f64,
    refused: Option<&str>,
    purpose: Purpose<'_>,
) -> Result<(Planned, PlanStats)> {
    let mut stats = PlanStats::default();
    let strict = match refused {
        Some(why) => Err(unsupported(why.to_string())),
        None => plan_face(fi, kind, s, loops, rules, size, reach, &mut stats, Lenient::No, purpose),
    };
    if let Purpose::Mesh(_) = purpose {
        return strict.map(|planned| (planned, stats));
    }
    match strict {
        Ok(fp) => Ok((fp, stats)),
        Err(first) => {
            let mut stats = PlanStats::default();
            match plan_face(fi, kind, s, loops, rules, size, reach, &mut stats, Lenient::Yes, purpose) {
                Ok(fp) => {
                    stats.lenient.push(RefusedFace { face: fi, reason: first.to_string() });
                    Ok((fp, stats))
                }
                Err(_) => Err(first),
            }
        }
    }
}

/// The triangles of a fan over the loop `range` of `uvs` from its first node, leaving out each
/// one with two corners on one node (`same`) or no area in (u, v), each counter-clockwise.
fn fan(range: std::ops::Range<usize>, uvs: &[[f64; 2]], same: &dyn Fn(u32, u32) -> bool) -> Vec<[u32; 3]> {
    let (Ok(start), Ok(end)) = (u32::try_from(range.start), u32::try_from(range.end)) else { return Vec::new() };
    let at = |i: u32| uvs.get(i as usize).copied();
    (start + 1..end.saturating_sub(1))
        .filter_map(|i| {
            let t = [start, i, i + 1];
            let (a, b, c) = (at(t[0])?, at(t[1])?, at(t[2])?);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if same(t[0], t[1]) || same(t[1], t[2]) || same(t[2], t[0]) || area == 0.0 || !area.is_finite() {
                return None;
            }
            Some(if area > 0.0 { t } else { [t[0], t[2], t[1]] })
        })
        .collect()
}

/// A face's boundary loops as closed polygons in its surface's (u, v), as the planner
/// triangulates them: every node in order, the
/// 3D point each stands for, and each loop's range.
pub(crate) struct Polygons {
    /// Each node's (u, v).
    pub(crate) uvs: Vec<[f64; 2]>,
    /// Each node's 3D point (the shared edge node it stands for).
    pub(crate) fixed: Vec<Vec3>,
    /// Each loop's nodes.
    pub(crate) ranges: Vec<std::ops::Range<usize>>,
    /// Whether the face was cut along a seam into one period.
    pub(crate) seamed: bool,
}

/// Face loops `loops` on `s` made closed polygons in (u, v): a loop
/// up a seam to a pole and back down it opened at the pole; a face whose boundary winds round a
/// closed direction cut along a parameter line into one period ([`seam::cut`], at side `h`); a
/// loop from pole to pole and back along one seam opened, and holes moved by whole periods
/// beside the outer loop; and at a pole (within `reach`, [`Boundary::reach`]) the free parameter
/// split into the previous node's value and the next one's, so the polygon runs along the
/// collapsed side. Why not, where the boundary winds round the surface and cannot be cut.
pub(crate) fn polygons(s: &poles::Face, loops: &[seam::Loop], rules: &size::Rules, h: f64, reach: f64) -> std::result::Result<Polygons, &'static str> {
    let dom = uv::domain(s);
    let mut projected: Vec<seam::Loop> = loops.to_vec();
    // A loop that winds around a periodic direction does not close in (u, v): the face is cut
    // along a parameter line into one period (a seamless cylinder face, a helical trim).
    // A loop that runs up a seam from a pole and straight back down it (a half sphere bounded
    // by its rim and its seam used both ways) winds once as projected: it is opened at the pole.
    for l in projected.iter_mut() {
        open_at_pole(s, &dom, reach, l);
    }
    let wound: Vec<usize> = (0..2).filter(|&d| dom.period.get(d).copied().flatten().is_some_and(|per| seam::winds(&projected, d, per))).collect();
    // A face cut along a seam is a long winding strip whose sides are the cut lines and the
    // boundary in turn: Delaunay with sag refinement meshes it (the patch layouts below assume
    // boundary all round, and leave fans across the strip's corners off the surface).
    let seamed = !wound.is_empty();
    match wound.as_slice() {
        [] => {}
        [d] => {
            let per = dom.period.get(*d).copied().flatten().unwrap_or(std::f64::consts::TAU);
            projected = seam::cut(s, &projected, *d, per, rules, h).ok_or("boundary winds around the surface")?;
        }
        _ => return Err("boundary winds around the surface both ways"),
    }
    // A loop from pole to pole and back along one seam (a whole sphere as one face, as another
    // kernel writes it) projects onto one parameter line: its second run is a period along.
    if !seamed {
        for l in projected.iter_mut() {
            open_pole_to_pole(s, &dom, reach, l);
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
            let pole = (0..2).find(|&d| s.pole(*q, d, reach));
            match (pole, prev, next) {
                (Some(d), Some(a), Some(b)) => {
                    let mut qa = *q;
                    let mut qb = *q;
                    if let Some(x) = qa.get_mut(d) {
                        *x = comp(&a, d);
                    }
                    if let Some(x) = qb.get_mut(d) {
                        *x = comp(&b, d);
                    }
                    uvs.push(qa);
                    fixed.push(*p);
                    if (comp(&qa, d) - comp(&qb, d)).abs() > 1e-12 {
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
    Ok(Polygons { uvs, fixed, ranges, seamed })
}

/// A pole of the surface on a face's boundary, which the mesh path fans on its own
/// ([`pole_caps`]).
struct PoleCap {
    /// The loop it is on (index into the face's loops).
    lp: usize,
    /// Its boundary nodes, in loop order: one, or its copies where the planner split the free
    /// parameter there ([`polygons`]).
    copies: Vec<usize>,
    /// The boundary nodes before and after them in the loop.
    before: usize,
    after: usize,
}

/// The poles of the surface `s` on a face's boundary loops (`uvs` in `ranges`, at the 3D points
/// `fixed`), as the display finds them ([`poles::Face::pole`], within `reach`): each run of
/// boundary nodes at one pole, with the nodes either side. The parameters collapse there, so a
/// triangulation in (u, v) meets the pole along a side that is one point in 3D, and its
/// triangles crowd and fold toward it. The mesh path fans each pole's cap from the pole instead
/// ([`cap_ring`], [`cap_fan`]). An error where a pole's neighbour on its loop is a pole too: no
/// ring lies between them.
fn pole_caps(s: &poles::Face, uvs: &[[f64; 2]], fixed: &[Vec3], ranges: &[std::ops::Range<usize>], reach: f64) -> std::result::Result<Vec<PoleCap>, String> {
    let is_pole = |k: usize| uvs.get(k).is_some_and(|q| (0..2).any(|d| s.pole(*q, d, reach)));
    let mut caps = Vec::new();
    for (lp, r) in ranges.iter().enumerate() {
        let n = r.len();
        if n == 0 {
            continue;
        }
        let at = |k: usize| r.start + k % n;
        for k in 0..n {
            // A run starts at a pole node whose predecessor is not one of its copies.
            let same = |a: usize, b: usize| is_pole(a) && is_pole(b) && fixed.get(a) == fixed.get(b);
            if !is_pole(at(k)) || same(at(k + n - 1), at(k)) {
                continue;
            }
            let mut copies = vec![at(k)];
            while copies.len() < n && same(at(k + copies.len() - 1), at(k + copies.len())) {
                copies.push(at(k + copies.len()));
            }
            if copies.len() + 2 > n {
                return Err(format!("a loop of {n} nodes all at one pole"));
            }
            let (before, after) = (at(k + n - 1), at(k + copies.len()));
            if is_pole(before) || is_pole(after) {
                return Err("a pole next to another pole on its loop: no ring lies between them".into());
            }
            caps.push(PoleCap { lp, copies, before, after });
        }
    }
    Ok(caps)
}

/// How much wider than the spec's angle a fan's angle at the pole is planned in the tangent
/// plane: on a curved surface the ring's nodes lean away from it, so the angle in 3D is a little
/// less ([`cap_ring`] checks it in 3D).
const RING_MARGIN: f64 = 0.02;

/// How near the farthest a ring may lie ([`ring_reach`]) the planner puts the nodes it asks for:
/// a node put on an edge at a fraction of a chord lies about there, so the ring's chords keep the
/// tolerance with room.
const RING_ROOM: f64 = 0.9;

/// How far the ring line of `cap` (straight in (u, v) from the boundary node before the pole to
/// the one after it) turns round the pole in 3D, sampled, with the pole's point and the line's
/// (u, v) ends.
fn ring_turn(s: &EvalSurface, cap: &PoleCap, uvs: &[[f64; 2]], fixed: &[Vec3]) -> std::result::Result<(f64, Vec3, [f64; 2], [f64; 2]), String> {
    const SAMPLES: usize = 64;
    let (Some(&a), Some(&b), Some(&pole)) = (uvs.get(cap.before), uvs.get(cap.after), cap.copies.first().and_then(|&c| fixed.get(c))) else {
        return Err("a pole's cap off its loop".into());
    };
    let point = |t: f64| {
        let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        s.derivs(q[0], q[1]).map(|d| d.0).ok_or_else(|| format!("the surface does not evaluate on a pole's ring at {q:?}"))
    };
    let mut turn = 0.0;
    let mut last = point(0.0)?;
    for i in 1..=SAMPLES {
        let p = point(i as f64 / SAMPLES as f64)?;
        turn += edges::turn(last - pole, p - pole);
        last = p;
    }
    Ok((turn, pole, a, b))
}

/// The segments of a ring turning `turn` round its pole: as many as keep the fan's angle at the
/// pole at least `min_angle` (with [`RING_MARGIN`]), and at least one per quarter turn, so the
/// fan's triangles keep the angle at the pole by construction and are never obtuse there. A full
/// turn at 25 degrees: 14 segments.
fn ring_segments(turn: f64, min_angle: f64) -> usize {
    let most = (turn / (min_angle * (1.0 + RING_MARGIN))).floor();
    let least = (turn / std::f64::consts::FRAC_PI_2).ceil();
    most.max(least).clamp(1.0, edges::MAX_EDGE_NODES as f64) as usize
}

/// The farthest from its pole a ring of `n` segments turning `turn` may lie (mm): each chord turns
/// `turn / n` round the pole and sags `r (1 - cos(turn / 2n))` from a ring of radius `r`, which
/// keeps the tolerance `tol` no farther than `tol / (1 - cos(turn / 2n))`; nor farther than the
/// side asked `side`.
fn ring_reach(tol: f64, side: f64, turn: f64, n: usize) -> f64 {
    let half = 0.5 * turn / n.max(1) as f64;
    (tol / (1.0 - half.cos())).min(side)
}

/// Where the boundary nodes either side of `cap`'s pole lie farther from it than its ring may
/// (`reach`): the sides from them to the pole, each to be split on its edge at [`RING_ROOM`] of
/// the reach, so the next plan's ring passes there. Empty where the ring may pass through them.
fn ring_too_far(cap: &PoleCap, fixed: &[Vec3], reach: f64) -> Vec<SplitSide> {
    let Some(&c0) = cap.copies.first() else { return Vec::new() };
    let (Some(&pole), Some(&before), Some(&after)) = (fixed.get(c0), fixed.get(cap.before), fixed.get(cap.after)) else { return Vec::new() };
    let mut out = Vec::new();
    let (db, da) = (pole.dist(before), pole.dist(after));
    let at = RING_ROOM * reach;
    if db > reach {
        out.push(SplitSide { from: before, to: pole, at: vec![1.0 - at / db] });
    }
    if da > reach {
        out.push(SplitSide { from: pole, to: after, at: vec![at / da] });
    }
    out
}

/// The inner nodes of the ring of `cap`: the line in (u, v) from the boundary node before the
/// pole to the one after it (both ends count among its segments: a seam's two nodes are its
/// ends, one point), divided evenly by the turn round the pole in 3D, not by its parameter (a
/// rational spline's runs unevenly round its arcs), into [`ring_segments`] for the spec's angle
/// `min_angle`, then fewer while
/// the fan's angle at the pole between two neighbours falls short of it in 3D (down to one per
/// quarter turn), so the fan keeps the angle. Where the ring lies farther out than its chords can
/// keep the tolerance `tol` (the planner could not put nodes nearer the pole), it is divided more
/// finely instead, so its chords keep the tolerance, and the face is named short of the angle.
/// Each node's (u, v) and point; an error where the surface does not evaluate on the ring.
fn cap_ring(s: &EvalSurface, cap: &PoleCap, uvs: &[[f64; 2]], fixed: &[Vec3], tol: f64, min_angle: f64) -> std::result::Result<Vec<([f64; 2], Vec3)>, String> {
    /// Rounds of dividing the ring more finely where a chord still sags past the tolerance.
    const MAX_FINER: usize = 16;
    let (turn, pole, a, b) = ring_turn(s, cap, uvs, fixed)?;
    let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let point = |t: f64| {
        let q = at(t);
        s.derivs(q[0], q[1]).map(|d| d.0).ok_or_else(|| format!("the surface does not evaluate on a pole's ring at {q:?}"))
    };
    // The turn round the pole from the ring's start, sampled along it: each node is placed by
    // its share of the whole turn, then bisected between the samples either side of it.
    const SAMPLES: usize = 256;
    const BISECTIONS: usize = 48;
    let mut table: Vec<(f64, Vec3, f64)> = Vec::new();
    let mut so_far = 0.0;
    for i in 0..=SAMPLES {
        let t = i as f64 / SAMPLES as f64;
        let p = point(t)?;
        if let Some(&(_, last, _)) = table.last() {
            so_far += edges::turn(last - pole, p - pole);
        }
        table.push((t, p, so_far));
    }
    let whole = so_far;
    let place = |share: f64| -> std::result::Result<f64, String> {
        let goal = whole * share;
        let Some(k) = table.windows(2).position(|w| matches!(w, [_, (_, _, b)] if *b >= goal)) else { return Ok(1.0) };
        let (Some(&(t0, p0, c0)), Some(&(t1, _, _))) = (table.get(k), table.get(k + 1)) else { return Ok(1.0) };
        let (mut lo, mut hi) = (t0, t1);
        for _ in 0..BISECTIONS {
            let mid = 0.5 * (lo + hi);
            if c0 + edges::turn(p0 - pole, point(mid)? - pole) < goal {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Ok(0.5 * (lo + hi))
    };
    let nodes = |n: usize| -> std::result::Result<Vec<(f64, [f64; 2], Vec3)>, String> {
        (0..=n)
            .map(|i| {
                let t = if i == 0 { 0.0 } else if i == n { 1.0 } else { place(i as f64 / n as f64)? };
                Ok((t, at(t), point(t)?))
            })
            .collect()
    };
    let least = ring_segments(turn, std::f64::consts::FRAC_PI_2);
    let mut n = ring_segments(turn, min_angle);
    let mut ring = nodes(n)?;
    // Fewer while a neighbour pair's angle at the pole in 3D falls short of the spec's.
    while n > least && ring.windows(2).any(|w| matches!(w, [x, y] if edges::turn(x.2 - pole, y.2 - pole) < min_angle)) {
        n -= 1;
        ring = nodes(n)?;
    }
    // More finely where a chord sags past the tolerance.
    for _ in 0..MAX_FINER {
        let sags = ring.windows(2).any(|w| {
            let [x, y] = w else { return false };
            [0.25, 0.5, 0.75].iter().any(|f| point(x.0 + f * (y.0 - x.0)).is_ok_and(|p| p.dist_to_segment(x.2, y.2) > tol))
        });
        if !sags || n >= edges::MAX_EDGE_NODES {
            break;
        }
        n = (n + n.div_ceil(4)).min(edges::MAX_EDGE_NODES);
        ring = nodes(n)?;
    }
    // The inner nodes: the ends are the boundary's.
    Ok(ring.into_iter().skip(1).take(n.saturating_sub(1)).map(|(_, q, p)| (q, p)).collect())
}

/// The fan of a pole's cap: the triangles from the pole's `copies` (boundary nodes, in loop
/// order) to the ring `ring` (from the node before the pole to the one after it), in the
/// numbering of `uv`, counter-clockwise in (u, v): the first copy to every ring segment but the
/// last, the copies to the ring's last inner node (triangles with two corners at the pole, which
/// the planner leaves out, so the fan covers the cap in (u, v)), and the last copy to the last
/// segment. `None` where a triangle has no area in (u, v).
fn cap_fan(copies: &[usize], ring: &[usize], uv: &[[f64; 2]]) -> Option<Vec<[u32; 3]>> {
    let id = |k: usize| u32::try_from(k).ok();
    let (&c0, &cm) = (copies.first()?, copies.last()?);
    let m = ring.len().checked_sub(1)?;
    let last = *ring.get(m.checked_sub(1)?)?;
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for k in 0..m.saturating_sub(1) {
        tris.push([c0, *ring.get(k)?, *ring.get(k + 1)?]);
    }
    for w in copies.windows(2) {
        let (&a, &b) = (w.first()?, w.get(1)?);
        tris.push([a, b, last]);
    }
    tris.push([cm, last, *ring.get(m)?]);
    tris.into_iter()
        .map(|[a, b, c]| {
            let (qa, qb, qc) = (*uv.get(a)?, *uv.get(b)?, *uv.get(c)?);
            let area = (qb[0] - qa[0]) * (qc[1] - qa[1]) - (qb[1] - qa[1]) * (qc[0] - qa[0]);
            if !(area.abs() > 0.0) {
                return None;
            }
            Some(if area > 0.0 { [id(a)?, id(b)?, id(c)?] } else { [id(a)?, id(c)?, id(b)?] })
        })
        .collect()
}

/// The fastest the surface `s` runs along u and along v (mm per parameter unit) over the box
/// `bbox`, sampled on a 9 x 9 grid, and at least `mean`: a lattice of side `h / fast` in each
/// parameter is no wider than `h` in 3D anywhere the samples see.
fn fastest(s: &EvalSurface, bbox: [(f64, f64); 2], mean: [f64; 2]) -> [f64; 2] {
    const SAMPLES: usize = 9;
    let mut fast = mean;
    let [(u0, u1), (v0, v1)] = bbox;
    for i in 0..SAMPLES {
        for j in 0..SAMPLES {
            let u = u0 + (u1 - u0) * (i as f64 + 0.5) / SAMPLES as f64;
            let v = v0 + (v1 - v0) * (j as f64 + 0.5) / SAMPLES as f64;
            if let Some((_, su, sv)) = s.derivs(u, v) {
                fast = [fast[0].max(su.len()), fast[1].max(sv.len())];
            }
        }
    }
    fast.map(|x| if x.is_finite() && x > 0.0 { x } else { 1.0 })
}

/// How near in 3D, as a fraction of the side asked there, a seed may lie to another seed or a
/// boundary node ([`mesh_seeds`]).
const SEED_GAP: f64 = 0.8;

/// The mesh path's seeds of a face: the equilateral lattice of side `h / fast` over `bbox`
/// inside the loops `polys` (no wider than `h` in 3D, see [`fastest`]), taken in its order
/// where its point lies at least [`SEED_GAP`] of the side asked there (`h`, or less where
/// `size_at` asks) from the boundary nodes `fixed` and the seeds taken before it, in 3D: where
/// the surface runs slower than its fastest (towards a sphere's pole, round a torus's inside)
/// the lattice crowds in 3D, and the seeds are thinned to the side. Whether the lattice was
/// looked through ([`cdt::lattice_scanned`]).
fn mesh_seeds(s: &EvalSurface, bbox: [(f64, f64); 2], h: f64, polys: &[&[[f64; 2]]], fast: [f64; 2], fixed: &[Vec3], size_at: Option<size::SizeAt<'_>>) -> (Vec<[f64; 2]>, bool) {
    let (lattice, whole) = cdt::lattice_scanned(bbox, [h / fast[0], h / fast[1]], polys, fast);
    if !(h.is_finite() && h > 0.0) {
        return (lattice, whole);
    }
    // The points taken so far, by cell of a grid a side wide: every point nearer than the gap
    // is in the cell of the seed or a neighbour.
    let cell = |p: Vec3| [p.x, p.y, p.z].map(|x| (x / h).floor() as i64);
    let mut taken: crate::hash::BitsMap<[i64; 3], Vec<Vec3>> = crate::hash::BitsMap::default();
    for p in fixed {
        taken.entry(cell(*p)).or_default().push(*p);
    }
    let mut seeds = Vec::new();
    for q in lattice {
        let Some((p, _, _)) = s.derivs(q[0], q[1]) else { continue };
        let gap = SEED_GAP * size::local(h, size_at, p);
        let [x, y, z] = cell(p);
        let crowded = (-1..=1).any(|i| (-1..=1).any(|j| (-1..=1).any(|k| taken.get(&[x + i, y + j, z + k]).is_some_and(|ps| ps.iter().any(|o| o.dist(p) < gap)))));
        if !crowded {
            taken.entry([x, y, z]).or_default().push(p);
            seeds.push(q);
        }
    }
    (seeds, whole)
}

/// One face: boundary loops in 3D (shared edge nodes) to parameters and triangles.
/// `size`: the face's target side length (mm) and the surface's mean speed along u and v.
/// `reach`: how near a pole a point is taken as one ([`Boundary::reach`]). `lenient`: the last resort
/// for a face no method could plan (see [`Lenient`]). `purpose`: on the mesh path every face goes
/// to constrained Delaunay ([`cdt::mesh`]), however it suits a cheaper layout.
#[allow(clippy::too_many_arguments)]
fn plan_face(
    fi: usize,
    kind: &str,
    s: &poles::Face,
    loops: &[seam::Loop],
    rules: &size::Rules,
    size: (f64, [f64; 2]),
    reach: f64,
    stats: &mut PlanStats,
    lenient: Lenient,
    purpose: Purpose<'_>,
) -> Result<Planned> {
    let tol = rules.tol;
    let (h, speed) = size;
    // The memory bounds that limit this face, if any ([`crate::Cap`]).
    let mut caps: Vec<crate::Cap> = Vec::new();
    let fail = |why: &str| unsupported(format!("face {fi} ({kind}): {why}"));
    let Polygons { uvs, fixed, ranges, seamed } = polygons(s, loops, rules, h, reach).map_err(fail)?;
    if uvs.len() < 3 {
        return Err(fail("degenerate boundary"));
    }
    let mut bbox = [(f64::MAX, f64::MIN); 2];
    for p in &uvs {
        for (bb, x) in bbox.iter_mut().zip(p) {
            *bb = (bb.0.min(*x), bb.1.max(*x));
        }
    }
    // Constrained Delaunay in (u, v) scaled to millimetres: equilateral seeds of side `h`,
    // refined where triangles leave the surface, are too long or bend too much, then shaped by
    // Ruppert refinement.
    let planar = matches!(**s, EvalSurface::Plane { .. });
    let bare = planar && h.is_infinite();
    let polys: Vec<&[[f64; 2]]> = ranges.iter().filter_map(|r| uvs.get(r.clone())).collect();
    let step = [h / speed[0], h / speed[1]];
    let delaunay = || {
        // A flat face with no size asked of it: the boundary nodes alone, no interior seeds.
        let seeds = if bare { Vec::new() } else { cdt::lattice(bbox, step, &polys, speed) };
        let eval = |u: f64, v: f64| s.derivs(u, v).map(|(p, su, sv)| (p, su.cross(sv).normalized()));
        let sag = cdt::Sag { eval: &eval, tol, h, turn: rules.turn, scale: speed };
        let fill = cdt::Fill {
            seeds: &seeds,
            refine: if bare { None } else { Some((sag, MAX_REFINE)) },
            shape: !matches!(**s, EvalSurface::Plane { .. }),
            lenient: lenient == Lenient::Yes,
            flat: (bare && lenient == Lenient::No).then(|| cdt::Flat { min_angle: FLAT_MIN_ANGLE_DEG, max_points: uvs.len(), min_area: cdt::smallest_area(tol) }),
        };
        cdt::triangulate(&uvs, &ranges, fill).ok_or_else(|| fail("boundary crosses itself"))
    };
    // Planar and curved faces alike: a structured grid where the boundary runs along the
    // parameter box (a rectangle, a cylinder half), a Coons grid on four corners, else Delaunay.
    // The lenient fill is the constrained Delaunay alone: the caller names the face in the stats.
    // What limited the mesh path's shaping of the face, where it was shaped.
    let mut unshaped: Option<Vec<crate::ShapeLimit>> = None;
    let (extra, triangles) = if let Purpose::Mesh(shape) = purpose {
        // The mesh path: every face, planar ones too, by constrained Delaunay with seeds a side
        // apart in 3D, refined for sag and side length, then shaped by its triangles in 3D.
        if planar {
            stats.planar += 1;
        } else {
            stats.delaunay += 1;
        }
        let fast = fastest(s, bbox, speed);
        // Each pole of the surface on the boundary is fanned on its own, from the pole to a
        // ring through the boundary nodes either side of it; the rest of the face is bounded
        // by the rings instead. Nodes are numbered: the boundary's, the rings', then the ones
        // refinement adds.
        let nb = uvs.len();
        let caps = pole_caps(s, &uvs, &fixed, &ranges, reach).map_err(|why| fail(&why))?;
        // A ring through boundary nodes farther from the pole than a ring may lie would have
        // chords off the surface by more than the tolerance at the fan's angle: the edges take
        // nodes at the ring's reach first, and the face is planned again ([`plan_at`]).
        if shape.sides == Sides::Split {
            let far: Vec<SplitSide> = caps
                .iter()
                .map(|cap| {
                    let (turn, pole, _, _) = ring_turn(s, cap, &uvs, &fixed)?;
                    let reach = ring_reach(tol, size::local(h, shape.size_at, pole), turn, ring_segments(turn, shape.min_angle.to_radians()));
                    Ok(ring_too_far(cap, &fixed, reach))
                })
                .collect::<std::result::Result<Vec<Vec<SplitSide>>, String>>()
                .map_err(|why| fail(&why))?
                .into_iter()
                .flatten()
                .collect();
            if !far.is_empty() {
                return Ok(Planned::Split(far));
            }
        }
        let (mut nodes, mut pos): (Vec<[f64; 2]>, Vec<Vec3>) = (uvs.clone(), fixed.clone());
        let mut fan: Vec<[u32; 3]> = Vec::new();
        // The rings' sides, in the face's numbering.
        let mut ring_sides: Vec<(u32, u32)> = Vec::new();
        // The poles' copies, the fans' corners at the poles.
        let pole_ids: std::collections::BTreeSet<u32> = caps.iter().flat_map(|c| c.copies.iter().filter_map(|&k| u32::try_from(k).ok())).collect();
        // Each loop as the rest of the face runs it: its nodes, a cap's copies of its pole
        // replaced by the cap's ring.
        let mut runs: Vec<Vec<usize>> = ranges.iter().map(|r| r.clone().collect()).collect();
        for cap in &caps {
            let ring = cap_ring(s, cap, &uvs, &fixed, tol, shape.min_angle.to_radians()).map_err(|why| fail(&why))?;
            let first = nodes.len();
            for (q, p) in &ring {
                nodes.push(*q);
                pos.push(*p);
            }
            // The ring from the node before the pole to the one after it.
            let ring_ids: Vec<usize> = std::iter::once(cap.before).chain(first..nodes.len()).chain(std::iter::once(cap.after)).collect();
            fan.extend(cap_fan(&cap.copies, &ring_ids, &nodes).ok_or_else(|| fail("a pole's cap cannot be fanned in (u, v)"))?);
            ring_sides.extend(ring_ids.windows(2).filter_map(|w| Some((u32::try_from(*w.first()?).ok()?, u32::try_from(*w.get(1)?).ok()?))));
            if let Some(run) = runs.get_mut(cap.lp) {
                // Started at the node after the pole, the loop ends with the node before it and
                // the copies; the copies leave it, and the ring's inner nodes take their place.
                let from = run.iter().position(|&k| k == cap.after).ok_or_else(|| fail("a pole's cap off its loop"))?;
                run.rotate_left(from);
                let at = run.iter().position(|&k| Some(&k) == cap.copies.first()).ok_or_else(|| fail("a pole's cap off its loop"))?;
                if run.get(at..) != Some(cap.copies.as_slice()) {
                    return Err(fail("a pole's copies apart on its loop"));
                }
                run.truncate(at);
                run.extend(first..nodes.len());
            }
        }
        // The rest of the face in its own numbering: its loops' nodes in order.
        let mut map: Vec<usize> = Vec::new();
        let mut mranges: Vec<std::ops::Range<usize>> = Vec::new();
        for run in &runs {
            let start = map.len();
            map.extend(run.iter().copied());
            mranges.push(start..map.len());
        }
        let muv: Vec<[f64; 2]> = map.iter().filter_map(|&k| nodes.get(k).copied()).collect();
        if muv.len() != map.len() {
            return Err(fail("a loop's node is missing"));
        }
        let mpolys: Vec<&[[f64; 2]]> = mranges.iter().filter_map(|r| muv.get(r.clone())).collect();
        let (seeds, whole) = mesh_seeds(s, bbox, h, &mpolys, fast, &pos, shape.size_at);
        let eval = |u: f64, v: f64| s.derivs(u, v).map(|(p, su, sv)| (p, su.cross(sv).normalized()));
        let sag = cdt::Sag { eval: &eval, tol, h, turn: rules.turn, scale: fast };
        let meshed = cdt::mesh(&muv, &mranges, &seeds, &sag, shape.size_at, MAX_MESH_REFINE).ok_or_else(|| fail("boundary crosses itself"))?;
        let mut limits: Vec<crate::ShapeLimit> = Vec::new();
        if !whole {
            limits.push(crate::ShapeLimit::Seeds);
        }
        if meshed.capped {
            limits.push(crate::ShapeLimit::Points);
        }
        // Its triangles in the face's numbering, with the caps' fans.
        let base = nodes.len();
        let global = |i: u32| -> Option<u32> {
            let i = i as usize;
            let g = if i < map.len() { *map.get(i)? } else { base + (i - map.len()) };
            u32::try_from(g).ok()
        };
        let mut tris: Vec<[u32; 3]> = Vec::new();
        for t in &meshed.triangles {
            let (Some(a), Some(b), Some(c)) = (global(t[0]), global(t[1]), global(t[2])) else { return Err(fail("a triangle's node is missing")) };
            tris.push([a, b, c]);
        }
        let fans = tris.len()..tris.len() + fan.len();
        tris.extend(fan);
        for q in &meshed.extra {
            nodes.push(*q);
            pos.push(s.derivs(q[0], q[1]).map(|d| d.0).ok_or_else(|| fail(&format!("the surface does not evaluate at {q:?}")))?);
        }
        let point = |q: [f64; 2]| s.derivs(q[0], q[1]).map(|d| d.0);
        // A 3D point's parameters on the surface, by the kernel's closest point, unwrapped to
        // the parameters it is looked for from.
        let dom = uv::domain(s);
        let project = |p: Vec3, from: [f64; 2], accept: f64| -> Option<[f64; 2]> {
            let q = solidcore::eval::plan::uv::project(s, &dom, p, Some(from), accept)?;
            Some([uv::unwrap(q[0], from[0], dom.period[0]), uv::unwrap(q[1], from[1], dom.period[1])])
        };
        let whole_sides = match shape.sides {
            Sides::Split => None,
            Sides::Kept => Some(crate::ShapeLimit::SidesOnNoEdge),
            Sides::Exhausted => Some(crate::ShapeLimit::Rounds),
        };
        let shaping = shape::Rules {
            min_angle: shape.min_angle.to_radians(),
            tol,
            h,
            size_at: shape.size_at,
            budget: MAX_MESH_REFINE.saturating_sub(meshed.extra.len()),
            whole: whole_sides,
            project: &project,
        };
        // Shaped whole: the fans kept as they are, the rings split in the face where they must,
        // the face's own boundary sides on their edges.
        match shape::shape(&mut nodes, &mut pos, &mut tris, &ranges, (ring_sides.as_slice(), fans, &pole_ids), &point, &shaping) {
            // The sides its shaping would split go on their edges first ([`plan_at`]).
            shape::Shaped::Split(split) => {
                let sides = split.into_iter().filter_map(|(i, j, at)| Some(SplitSide { from: *fixed.get(i)?, to: *fixed.get(j)?, at })).collect();
                return Ok(Planned::Split(sides));
            }
            shape::Shaped::Done(more) => {
                for l in more {
                    if !limits.contains(&l) {
                        limits.push(l);
                    }
                }
            }
        }
        unshaped = Some(limits);
        (nodes.split_off(nb), tris)
    } else if lenient == Lenient::Yes {
        delaunay()?
    } else {
        let mut us = uv::divide(s, 0, bbox[0], bbox[1], rules, h, MAX_LINES);
        let mut vs = uv::divide(s, 1, bbox[1], bbox[0], rules, h, MAX_LINES);
        let asked = [us.len(), vs.len()];
        grid::thin(&mut us, &mut vs, grid::MAX_FACE_GRID);
        if [us.len(), vs.len()] != asked {
            caps.push(crate::Cap::GridThinned { asked, kept: [us.len(), vs.len()] });
        }
        let structured = match (ranges.len(), if seamed || bare { None } else { grid::quad(&uvs) }) {
            (1, Some(q)) => {
                // Interior lines: the boundary's own where opposite sides agree (refined where the
                // surface turns more between them inside the face), else the division.
                let lines = |d: usize, div: &Vec<f64>| {
                    let (Some(&(b0, b1)), Some(&across)) = (bbox.get(d), bbox.get(1 - d)) else { return Vec::new() };
                    let inner = |l: Vec<f64>| l.into_iter().filter(|x| *x > b0 && *x < b1).collect::<Vec<f64>>();
                    match grid::aligned_lines(&uvs, &q, d) {
                        Some(aligned) => {
                            let ends: Vec<f64> = std::iter::once(b0).chain(aligned).chain(std::iter::once(b1)).collect();
                            inner(uv::refine(s, d, &ends, across, rules, h, MAX_LINES))
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
            // A flat face with no size asked of it is its boundary alone (the Delaunay below).
            None if ranges.len() == 1 && !seamed && !bare => {
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
                    if ranges.len() == 1 && h.is_finite() { plan_pieces(&face) } else { None }
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
    // A lenient fill left with nothing (a sliver below the tolerance whose boundary nodes are
    // few points in space, each twice, its pcurves apart in (u, v) where its edges meet: every
    // Delaunay triangle had two corners on one point): the outer loop fanned from its first node, the
    // triangles with no two corners on one node, counter-clockwise in (u, v). The face is drawn
    // between its own edges' nodes, and still named in the stats as a defect upstream.
    if lenient == Lenient::Yes && triangles.is_empty() {
        triangles = fan(ranges.first().cloned().unwrap_or(0..0), &uvs, &pole_pair);
    }
    // A lenient fill leaves out the sides that cross: the boundary is followed everywhere else.
    if lenient == Lenient::No && !closed_boundary(&ranges, &fixed, &uvs, &triangles) {
        return Err(fail("triangulation does not follow the boundary"));
    }
    let mut all = uvs;
    all.extend(extra);
    // Whatever the layout, no triangle leaves the surface by more than the tolerance.
    if !planar {
        // Which nodes are poles, asked once per node over both passes.
        let poles = sag::Poles::new(s, reach);
        if sag::refine(s, tol, &poles, &fixed, &mut all, &mut triangles, MAX_REFINE) {
            caps.push(crate::Cap::Refine);
        }
        // The mesh path's triangles are the surface's Delaunay ones, shaped in 3D: flips toward
        // the surface's normals, for shading, would undo their shape.
        if matches!(purpose, Purpose::Display) {
            sag::flip(s, tol, &poles, &fixed, &all, &mut triangles);
        }
        // A face a bound limited is named with the sag its triangles reach.
        if !caps.is_empty() {
            let (sag, unmeasured) = sag::worst(s, &poles, &fixed, &all, &triangles);
            stats.over_tolerance.push(crate::OverTolerance { face: fi, sag, unmeasured, tol, within: sag <= tol && unmeasured == 0, caps: std::mem::take(&mut caps) });
        }
    }
    // A face the mesh path left short of its shape is named with its smallest angle in 3D.
    if let (Some(limits), Purpose::Mesh(shape)) = (unshaped, purpose) {
        let at = |i: u32| fixed.get(i as usize).copied().or_else(|| all.get(i as usize).and_then(|q| s.derivs(q[0], q[1])).map(|d| d.0));
        let smallest =
            triangles.iter().filter_map(|t| Some(shape::smallest([at(t[0])?, at(t[1])?, at(t[2])?]))).fold(f64::INFINITY, f64::min).to_degrees();
        if !limits.is_empty() || smallest < shape.min_angle {
            stats.unshaped.push(crate::Unshaped { face: fi, smallest_angle: smallest, limits });
        }
    }
    // Both evaluators take a closed spline's parameters inside its active interval.
    for q in &mut all {
        *q = s.wrap(*q);
    }
    Ok(Planned::Face(FacePlan { face: u32::try_from(fi).unwrap_or(u32::MAX), surface: None, uv: all, fixed: fixed.into_iter().map(kernel::record_vec3).collect(), normals: Vec::new(), flip: false, triangles, poles: Vec::new() }))
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
            for (bb, x) in bbox.iter_mut().zip(p) {
                *bb = (bb.0.min(*x), bb.1.max(*x));
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
                for ((lo_d, hi_d), qd) in lo.iter_mut().zip(hi.iter_mut()).zip(q) {
                    *lo_d = lo_d.min(qd);
                    *hi_d = hi_d.max(qd);
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
                cdt::triangulate(&luv, &ranges, cdt::Fill { seeds: &seeds, refine: Some((sag, MAX_REFINE)), shape: true, lenient: false, flat: None })
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

/// Every boundary segment (consecutive nodes of a loop) is the side of exactly one triangle (two
/// for a slit the loops run along both ways):
/// the face covers its boundary with nothing missing or doubled, so it meets its neighbours
/// exactly. Some boundary nodes are one vertex, a side ending on any of them the same side: pole
/// copies of one node (consecutive in a loop, at one point, the segment between them of no
/// length), and nodes at the very same (u, v), which the triangulation takes as one point: two
/// loops touching at a vertex they share (a ring standing on a perforated plate, tangent to the
/// holes beside it). A seam's two uses are at one point in space but a period apart in (u, v),
/// and stay two.
fn closed_boundary(ranges: &[std::ops::Range<usize>], fixed: &[Vec3], uvs: &[[f64; 2]], triangles: &[[u32; 3]]) -> bool {
    let mut one: Vec<u32> = (0..fixed.len() as u32).collect();
    let root = |one: &[u32], mut i: u32| {
        while let Some(&j) = one.get(i as usize).filter(|j| **j != i) {
            i = j;
        }
        i
    };
    let segment = |r: &std::ops::Range<usize>, k: usize| ((r.start + k) as u32, (r.start + (k + 1) % r.len()) as u32);
    let join = |one: &mut [u32], a: u32, b: u32| {
        let (ra, rb) = (root(one, a), root(one, b));
        if let Some(x) = one.get_mut(rb.max(ra) as usize) {
            *x = ra.min(rb);
        }
    };
    for r in ranges {
        for k in 0..r.len() {
            let (a, b) = segment(r, k);
            if fixed.get(a as usize).zip(fixed.get(b as usize)).is_some_and(|(p, q)| p == q) {
                join(&mut one, a, b);
            }
        }
    }
    // By value: −0.0 and 0.0 are one point.
    let key = |q: &[f64; 2]| [(q[0] + 0.0).to_bits(), (q[1] + 0.0).to_bits()];
    let mut first: HashMap<[u64; 2], u32> = HashMap::with_capacity(uvs.len());
    for (i, q) in uvs.iter().enumerate().take(fixed.len()) {
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
    let mut sides: HashMap<(u32, u32), u32> = HashMap::with_capacity(triangles.len() * 3);
    for t in triangles {
        for [a, b, _] in (0..3).map(|e| rot3(*t, e)) {
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
mod tests;
#[cfg(test)]
mod mesh_tests;
