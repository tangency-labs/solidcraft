//! Booleans of a prism tool with a target that is a prism along the tool's direction only round
//! the tool: a pocket made deeper in a plate whose rim is chamfered, a ring cut round a
//! counterbored hole in a chamfered disc. The tool's walls lie on the target's there, which the
//! B-rep boolean can't intersect, and the 2D route ([`crate::prism::prism_boolean`]) declines the
//! target for a face far away (the chamfer) that is neither a wall along the direction nor a cap
//! square to it.
//!
//! The target is split by planes into a column round the tool that holds none of those faces;
//! the column takes the exact 2D boolean, and the pieces are glued back along the cut faces, as
//! [`crate::polybool::sliced_planar_boolean`] does for an all-planar tool beside curved faces.
//! The cuts are imprints (no boolean), each placed beside the tool on one of three axes: the
//! tool's direction and two square to it. A cut that leaves every offending face of the middle
//! piece beyond it goes first; otherwise one close beside the tool, which splits them (a
//! chamfer all round a disc is split into two pieces apart, which the next cut leaves out).

use solidcraft_geom::{Plane, Vec3};
use truck_modeling as mt;

use crate::body::Body;
use crate::boolean_record::{self as recorded, Traced};
use crate::{BoolOp, KernelError, Result};

/// Cuts at most, each splitting the middle piece once.
const MAX_CUTS: usize = 6;

/// Cuts made with half boxes at most (two booleans each, with their retries): an imprint traced
/// wrong across a curved face is rare, and the booleans cost seconds on curved parts.
const MAX_BOX_CUTS: usize = 2;

/// Operands a piece's history is over: `a`, `b`, then the two half boxes each cut may be made
/// with (dropped from the result, whose faces are none of theirs).
const OPERANDS: usize = 2 + 2 * MAX_CUTS;

thread_local! {
    /// Set while [`local_prism_boolean`] runs: the booleans it makes a cut with (a piece and a
    /// half box) take the general path, not this one again.
    static RUNNING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// An extent along each of three axes.
type Extent = [(f64, f64); 3];

/// [`crate::prism::prism_boolean`] of `a` and the prism `b`, taken in a column of `a` round `b`
/// (see the module notes). `None` when `b` is not a prism, `a` is one along `b`'s direction
/// already, or no column round `b` is clear of `a`'s other faces: the caller goes on as before.
pub(crate) fn local_prism_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    // A cut or an intersection: what a tool meeting the target's walls fails on. A union (a
    // shell's cavity joined with its opening) is left to the general path.
    if !matches!(op, BoolOp::Cut | BoolOp::Intersect) || a.face_count() > 4096 || RUNNING.with(std::cell::Cell::get) {
        return None;
    }
    /// Clears the flag however the boolean ends, a caught panic included.
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            RUNNING.with(|r| r.set(false));
        }
    }
    RUNNING.with(|r| r.set(true));
    let _reset = Reset;
    let d = crate::prism::prism_axis(b)?;
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9) * 10.0;
    let frame = Plane::from_normal(Vec3::ZERO, d)?;
    let axes = [frame.x, frame.y, d];
    // Extents come from meshes this close to the faces, widened by as much.
    let mesh_tol = (size * 1e-3).max(1e-4);
    let tool = widened(extent(&b.mesh(&crate::MeshTolerance::chord(mesh_tol)).ok()?.positions, axes)?, mesh_tol);
    let gap = size * 1e-4 + 1e-6;
    let pieces = survey(a, d, tol, mesh_tol, axes)?.pieces;
    if pieces.is_empty() {
        return None;
    }
    // A piece of an offending face within a hair of the tool's box: no column round the tool
    // can leave it out, so nothing is cut.
    let reach = size * 2e-3 + 2.0 * gap;
    if pieces.iter().any(|(_, e)| e.iter().zip(&tool).all(|((c0, c1), (t0, t1))| *c1 > t0 - reach && *c0 < t1 + reach)) {
        return None;
    }
    let run = || -> Result<Option<Body>> {
        let fail = |m: &str| KernelError::Failed(format!("prism boolean in a column round the tool: {m}"));
        let mut middle: Traced = (a.clone(), recorded::kept(a, 0, OPERANDS));
        let mut outside: Vec<Traced> = Vec::new();
        // The sides already cut (axis, positive side): each is cut once.
        let mut cut: Vec<(usize, bool)> = Vec::new();
        let mut box_cuts = 0;
        for _ in 0..MAX_CUTS {
            let survey = survey(&middle.0, d, tol, mesh_tol, axes).ok_or_else(|| fail("the middle piece"))?;
            let off = survey.faces();
            if off.is_empty() {
                break;
            }
            let (k, places, keep_positive) =
                next_cut(&off, &tool, &survey.here, &survey.flat, &cut, axes, size, gap).ok_or_else(|| fail("no cut leaves the other faces out"))?;
            cut.push((k, !keep_positive));
            let axis = axes[k];
            let (u, w) = (axes[(k + 1) % 3], axes[(k + 2) % 3]);
            // The first of the places where the piece splits into parts that add up to it (a
            // plane whose section through the piece doesn't close is passed over). The cut's
            // half boxes, if it takes them, are operands `slot` and `slot + 1`.
            let slot = 2 * cut.len();
            let (away, kept) = places
                .iter()
                .enumerate()
                .find_map(|(i, x)| {
                    let plane = Plane::new(axis * *x, u, w)?;
                    let with_boxes = i == 0 && box_cuts < MAX_BOX_CUTS;
                    // Only a plane across an offending face can be traced wrong (a cone's piece
                    // meshed past its bounds): the others are taken as they are.
                    let verify = survey.pieces.iter().any(|(_, e)| e[k].0 < *x && e[k].1 > *x);
                    let r = split(&middle, &plane, axis, *x, keep_positive, slot, with_boxes, verify)?;
                    box_cuts += usize::from(r.2);
                    Some((r.0, r.1))
                })
                .ok_or_else(|| fail("a cut beside the tool"))?;
            outside.extend(away);
            middle = kept;
        }
        if !survey(&middle.0, d, tol, mesh_tol, axes).is_some_and(|s| s.pieces.is_empty()) {
            return Err(fail("other faces are left round the tool"));
        }
        let r = match crate::prism::prism_boolean(&middle.0, b, op) {
            Some(r) => r?,
            None => return Err(fail("the column is not a prism along the tool")),
        };
        crate::polybool::reassembled(a, b, op, &middle, r, outside, OPERANDS, &fail)
    };
    Some(run())
}

/// `piece` split by `plane` (at `x` along `axis`, square to it): the parts away from the tool
/// and the one on its side (positive along `axis` when `keep_positive`), with their histories.
/// An imprint of the plane, taken as it is unless `verify`; else where its parts don't add up to
/// the piece (a plane across a cone can be traced wrong) and `with_boxes`, the piece's intersections with the half boxes
/// either side (the cut's operands `slot` and `slot + 1`), and then whether it took them. `None`
/// when neither adds up.
#[allow(clippy::too_many_arguments)]
fn split(
    piece: &Traced,
    plane: &Plane,
    axis: Vec3,
    x: f64,
    keep_positive: bool,
    slot: usize,
    with_boxes: bool,
    verify: bool,
) -> Option<(Vec<Traced>, Traced, bool)> {
    if !verify {
        let (away, kept) = crate::polybool::split_beside(piece, plane, axis, x, keep_positive)?;
        return Some((away, kept, false));
    }
    let whole = crate::measure(&piece.0).ok()?.volume;
    let adds_up = |away: &[Traced], kept: &Traced| {
        let sum: Option<f64> = away.iter().chain([kept]).map(|(p, _)| crate::measure(p).ok().map(|m| m.volume)).sum();
        sum.is_some_and(|sum| (sum - whole).abs() <= whole.abs() * 5e-5 + 1e-9)
    };
    if let Some((away, kept)) = crate::polybool::split_beside(piece, plane, axis, x, keep_positive)
        && adds_up(&away, &kept)
    {
        return Some((away, kept, false));
    }
    if !with_boxes {
        return None;
    }
    let boxes = (crate::ops::half_box(&piece.0, plane, !keep_positive).ok()?, crate::ops::half_box(&piece.0, plane, keep_positive).ok()?);
    let (h_away, h_kept) = (recorded::kept(&boxes.0, slot, OPERANDS), recorded::kept(&boxes.1, slot + 1, OPERANDS));
    let away = crate::ops::boolean(&piece.0, &boxes.0, BoolOp::Intersect).ok()?;
    let kept = crate::ops::boolean(&piece.0, &boxes.1, BoolOp::Intersect).ok()??;
    let away: Vec<Traced> = away
        .into_iter()
        .map(|o| {
            let h = recorded::of_pair(&o, &piece.1, &h_away);
            (o, h)
        })
        .collect();
    let kept: Traced = {
        let h = recorded::of_pair(&kept, &piece.1, &h_kept);
        (kept, h)
    };
    adds_up(&away, &kept).then_some((away, kept, true))
}

/// The extent of `points` along each of `axes`; `None` for no points.
fn extent(points: &[Vec3], axes: [Vec3; 3]) -> Option<Extent> {
    if points.is_empty() {
        return None;
    }
    let mut e = [(f64::INFINITY, f64::NEG_INFINITY); 3];
    for p in points {
        for (slot, axis) in e.iter_mut().zip(axes) {
            let t = p.dot(axis);
            *slot = (slot.0.min(t), slot.1.max(t));
        }
    }
    e.iter().all(|(lo, hi)| lo.is_finite() && hi.is_finite()).then_some(e)
}

/// What a cut is chosen from: a piece's extent (`here`), the mesh triangles of its faces that
/// are neither a wall along the direction nor a plane square to it (the faces that keep it from
/// being a prism along it: each one's index and extent), and its planar faces (unit normal,
/// offset along it) for a cut to keep out of. Extents are widened by the mesh's tolerance.
struct Survey {
    here: Extent,
    pieces: Vec<(usize, Extent)>,
    flat: Vec<(Vec3, f64)>,
}

impl Survey {
    /// The extent of each offending face.
    fn faces(&self) -> Vec<Extent> {
        let mut faces: Vec<(usize, Extent)> = Vec::new();
        for (i, e) in &self.pieces {
            match faces.iter_mut().find(|(j, _)| j == i) {
                Some((_, acc)) => {
                    for (x, y) in acc.iter_mut().zip(e) {
                        *x = (x.0.min(y.0), x.1.max(y.1));
                    }
                }
                None => faces.push((*i, *e)),
            }
        }
        faces.into_iter().map(|(_, e)| e).collect()
    }
}

/// [`Survey`] of `body` along `d`, from one mesh of it. `None` when it can't be meshed.
fn survey(body: &Body, d: Vec3, tol: f64, mesh_tol: f64, axes: [Vec3; 3]) -> Option<Survey> {
    let mut work = 262_144usize;
    let mut flat = Vec::new();
    let off: Vec<bool> = body
        .solid
        .face_iter()
        .map(|f| {
            let surface = f.oriented_surface();
            match &surface {
                mt::Surface::Plane(p) => {
                    let n = p.normal();
                    let Some(n) = Vec3::new(n.x, n.y, n.z).normalized() else { return true };
                    let o = p.origin();
                    flat.push((n, n.dot(Vec3::new(o.x, o.y, o.z))));
                    let along = n.dot(d).abs();
                    // NaN counts as inside, as the comparisons it replaces did.
                    along.is_nan() || (1e-9..=1.0 - 1e-9).contains(&along)
                }
                _ => !crate::prism::wall_along(&surface, d, tol, &mut work),
            }
        })
        .collect();
    let mesh = body.mesh(&crate::MeshTolerance::chord(mesh_tol)).ok()?;
    let here = widened(extent(&mesh.positions, axes)?, mesh_tol);
    let mut pieces = Vec::new();
    if off.iter().any(|x| *x) {
        for (t, fi) in mesh.triangles.iter().zip(&mesh.tri_face) {
            let i = *fi as usize;
            if !off.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some(tri) = mesh.tri(t) else { continue };
            if let Some(e) = extent(&tri, axes) {
                pieces.push((i, widened(e, mesh_tol)));
            }
        }
    }
    Some(Survey { here, pieces, flat })
}

/// `e` grown by `by` each way.
fn widened(mut e: Extent, by: f64) -> Extent {
    for (lo, hi) in e.iter_mut() {
        *lo -= by;
        *hi += by;
    }
    e
}

/// The next cut of the middle piece (its extent `here`, its planar faces `flat`): the axis, the
/// place along it and whether the tool's side is the positive one. A cut leaving the most
/// offending faces (`off`) wholly beyond it, on a side not `cut` yet, placed halfway to the nearest of them; failing
/// that, one beside the tool across the most of them (on an axis square to the direction
/// first). `None` when no cut beside the tool has any offending face beyond it.
#[allow(clippy::too_many_arguments)]
fn next_cut(
    off: &[Extent],
    tool: &Extent,
    here: &Extent,
    flat: &[(Vec3, f64)],
    cut: &[(usize, bool)],
    axes: [Vec3; 3],
    size: f64,
    gap: f64,
) -> Option<(usize, Vec<f64>, bool)> {
    // Places between `near` (beside the tool) and `far`, best first, out of the planes of the
    // piece's faces square to the axis.
    let place = |k: usize, near: f64, far: f64| -> Option<Vec<f64>> {
        let axis = axes[k];
        let places: Vec<f64> = [0.5, 0.37, 0.63, 0.25, 0.75, 0.1, 0.9]
            .iter()
            .map(|t| near + (far - near) * t)
            .filter(|x| !flat.iter().any(|(n, o)| n.dot(axis).abs() > 1.0 - 1e-9 && (o * n.dot(axis).signum() - x).abs() < gap))
            .collect();
        (!places.is_empty()).then_some(places)
    };
    let mut whole: Option<(usize, usize, Vec<f64>, bool)> = None;
    let mut across: Option<(usize, usize, Vec<f64>, bool)> = None;
    let beside = size * 2e-3;
    for k in [0usize, 1, 2] {
        let (t0, t1) = tool[k];
        let (h0, h1) = here[k];
        for positive_side in [true, false] {
            if cut.contains(&(k, positive_side)) {
                continue;
            }
            // Past the tool on this side: is there any of the piece to cut away?
            let (edge, room) = if positive_side { (t1 + gap, h1 - t1) } else { (t0 - gap, t0 - h0) };
            if room <= 2.0 * gap {
                continue;
            }
            let beyond: Vec<f64> = off
                .iter()
                .filter_map(|e| {
                    let (c0, c1) = e[k];
                    if positive_side { (c0 > edge).then_some(c0) } else { (c1 < edge).then_some(c1) }
                })
                .collect();
            if let Some(nearest) = beyond.iter().copied().reduce(if positive_side { f64::min } else { f64::max }) {
                let Some(x) = place(k, edge, nearest) else { continue };
                if whole.as_ref().is_none_or(|(_, n, ..)| beyond.len() > *n) {
                    whole = Some((k, beyond.len(), x, !positive_side));
                }
                continue;
            }
            if k == 2 && across.is_some() {
                continue;
            }
            let near = edge + if positive_side { beside } else { -beside };
            let crossing = off.iter().filter(|e| if positive_side { e[k].1 > near } else { e[k].0 < near }).count();
            let limit = if positive_side { h1 } else { h0 };
            if crossing == 0 {
                continue;
            }
            let Some(x) = place(k, edge, near + (near - edge))
                .map(|p| p.into_iter().filter(|x| if positive_side { *x < limit - gap } else { *x > limit + gap }).collect::<Vec<_>>())
                .filter(|p| !p.is_empty())
            else {
                continue;
            };
            if across.as_ref().is_none_or(|(_, n, ..)| crossing > *n) {
                across = Some((k, crossing, x, !positive_side));
            }
        }
    }
    whole.or(across).map(|(k, _, x, keep_positive)| (k, x, keep_positive))
}
