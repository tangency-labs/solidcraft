//! Fillets and chamfers through SMLib.
//!
//! Each kernel entry point ([`crate::fillet`], [`crate::chamfer`], …) hands its arguments here
//! unchanged: the body goes into SMLib as its schema 4 record (healed where SMLib's check refuses
//! it as given), tagged as operand 0; each point picks the body's nearest edge in its own edge
//! order, and SMLib is given that edge's midpoint, which no other edge is as near to (a point at
//! a vertex between two edges would leave SMLib to choose); SMLib blends exactly those edges and
//! the corners where they meet (a pick's tangent chain is the caller's to ask for,
//! [`crate::tangent_chain`]), its check must pass, and the result comes
//! back decoded with its geometry checked. An option SMLib has no construction for is refused as
//! "not supported yet".
//!
//! The history is SMLib's tags, renumbered into the body's orders; a blend face, which SMLib
//! leaves untagged, comes from the edge it replaces ([`solidcraft_smlib::history::blends`]).

use solidcraft_geom::Vec3;

use crate::blend_variable::RadiusSpline;
use crate::body::Body;
use crate::{KernelError, Result};

/// What SMLib cannot build yet.
pub(crate) fn unsupported(what: &str) -> KernelError {
    KernelError::Invalid(format!("not supported yet: {what} with the SMLib kernel"))
}

/// A constant-radius round of the edges nearest `edges` ([`crate::fillet`]).
pub(crate) fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    imp::blend(body, "fillet", Kind::Round, edges, &vec![radius; edges.len()])
}

/// [`fillet`], with the picks whose edges a larger, overflowing round removed whole.
pub(crate) fn fillet_removing(body: &Body, edges: &[Vec3], radius: f64) -> Result<(Body, Vec<Vec3>)> {
    let (b, dropped) = imp::blend_dropping(body, "fillet", Kind::Round, edges, &vec![radius; edges.len()])?;
    Ok((b, edges.iter().zip(dropped).filter(|(_, d)| *d).map(|(p, _)| *p).collect()))
}

/// Each edge rounded at its own radius ([`crate::fillet_each`]).
pub(crate) fn fillet_each(body: &Body, edges: &[Vec3], radii: &[f64]) -> Result<Body> {
    if radii.len() != edges.len() {
        return Err(KernelError::Invalid("one radius per edge".into()));
    }
    imp::blend(body, "fillet", Kind::Round, edges, radii)
}

/// A constant-radius round whose corners at `setbacks` (a point at each corner's vertex, and the
/// distance its patch starts along each edge there) are set back ([`crate::fillet_setback`]).
pub(crate) fn fillet_setback(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)]) -> Result<Body> {
    imp::blend_setback(body, edges, &vec![radius; edges.len()], setbacks)
}

/// A set-back blend whose corners SMLib leaves open ([`fillet_setback_open`]): the sheet, its
/// history over the body it was made from, and per corner the edges round its hole in order,
/// each its index in the sheet's edge order with whether the face beside it runs it forwards
/// (counter-clockwise about the solid's outward normal).
pub(crate) struct OpenCorners {
    pub(crate) sheet: Body,
    pub(crate) holes: Vec<Vec<(usize, bool)>>,
}

/// [`fillet_setback`] with each set-back corner left open: SMLib builds the rounds up to the
/// corners' setback sections and the curves on the faces between them, and leaves out the patch
/// that would fill each corner, for the caller to fill.
/// `curvature`: the rounds' sections are curvature-continuous ([`fillet_g2`]), else circular.
pub(crate) fn fillet_setback_open(body: &Body, edges: &[Vec3], radius: f64, setbacks: &[(Vec3, f64)], curvature: bool) -> Result<OpenCorners> {
    let kind = if curvature { Kind::Curvature } else { Kind::Round };
    imp::blend_setback_open(body, kind, edges, &vec![radius; edges.len()], setbacks)
}

/// Curvature-continuous rounds with the setback of a round of `radius` ([`crate::fillet_g2`]).
pub(crate) fn fillet_g2(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    imp::blend(body, "G2 fillet", Kind::Curvature, edges, &vec![radius; edges.len()])
}

/// Rounds `chord` wide from contact to contact ([`crate::fillet_chord`]).
pub(crate) fn fillet_chord(body: &Body, edges: &[Vec3], chord: f64) -> Result<Body> {
    imp::blend(body, "chord fillet", Kind::Chord, edges, &vec![chord; edges.len()])
}

/// An equal-distance chamfer ([`crate::chamfer`]).
pub(crate) fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    imp::blend(body, "chamfer", Kind::Chamfer, edges, &vec![distance; edges.len()])
}

/// A chamfer `d1` back along the face facing up most (down most with `flip`), the other side by
/// `side` ([`crate::chamfer_sides`]).
pub(crate) fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: crate::ChamferSide, flip: bool) -> Result<Body> {
    imp::sides(body, edges, d1, side, flip)
}

/// One edge rounded with the radius `spline` (checked by the caller) along it from the end
/// nearest `start` ([`crate::fillet_variable_spline`]).
pub(crate) fn fillet_variable(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
    imp::variable(body, edge_at, start, spline)
}

/// The edges `picks` select, in the body's edge order, as a blend resolves them, and per edge
/// whether a pick passes it over (a seam or a smooth join: never a blend's edge), for a caller
/// that selects edges to blend ([`crate::tangent_chain`]).
pub(crate) fn picked_edges(body: &Body, picks: &[Vec3]) -> Result<(Vec<usize>, Vec<bool>)> {
    let picked = imp::resolve(body, picks)?;
    Ok((picked.edges, picked.hidden))
}

#[derive(Clone, Copy)]
enum Kind {
    Round,
    Curvature,
    Chord,
    Chamfer,
}

mod imp {
    use std::collections::BTreeSet;

    use solidcraft_geom::Vec3;
    use solidcraft_smlib::{Blend, Brep, ChamferSecond, CheckLevel, Error as SmError};

    use super::Kind;
    use crate::blend_variable::RadiusSpline;
    use crate::body::Body;
    use crate::history::{BodyHistory, Change, EdgeOrigin, FaceOrigin, History};
    use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
    use crate::{KernelError, Result, guard, record};

    fn failed(what: &str, step: &str, e: impl std::fmt::Display) -> KernelError {
        KernelError::Failed(format!("SMLib {what}: {step}: {e}"))
    }

    /// How far a pick may lie from its edge.
    fn pick_tolerance(body: &Body) -> f64 {
        body.size() * 0.05 + 1e-3
    }

    /// The edges `picks` select: each one's index in the body's edge order and the point halfway
    /// along it, for SMLib to pick. A pick as near to a smooth join (a seam where a face meets
    /// itself, or faces meeting tangentially: never a blend's edge) as to a sharp edge, such as
    /// one at the vertex where a cylinder's seam meets its rim, takes the sharp edge.
    pub(super) fn resolve(body: &Body, picks: &[Vec3]) -> Result<Picked> {
        if picks.iter().any(|p| !p.is_finite()) {
            return Err(KernelError::Invalid("edge picks must be finite points".into()));
        }
        let chord = (body.size() * 2e-3).max(1e-3);
        let mesh = body.mesh(&crate::MeshTolerance::chord(chord))?;
        let seams = crate::topo::seam_flags(body, &mesh);
        let (smooth, _) = crate::smooth_edges::flags(body, &mesh);
        let hidden: Vec<bool> =
            (0..mesh.edges.len()).map(|i| seams.get(i).copied().unwrap_or(false) || smooth.get(i).copied().unwrap_or(false)).collect();
        let reach = pick_tolerance(body);
        let tie = (body.size() * 1e-6).max(1e-6);
        let mut out = Picked { edges: Vec::with_capacity(picks.len()), points: Vec::with_capacity(picks.len()), hidden: Vec::new() };
        for p in picks {
            // A polyline is up to `chord` off its edge, which may be more than the edges are
            // apart: those that close are measured on their curves.
            let polyline = |line: &[Vec3]| {
                line.windows(2).filter_map(|w| if let [a, b] = w { Some(p.dist_to_segment(*a, *b)) } else { None }).fold(f64::INFINITY, f64::min)
            };
            let near: Vec<f64> = mesh.edges.iter().map(|line| polyline(line)).collect();
            let nearest = near.iter().copied().fold(f64::INFINITY, f64::min);
            // The nearest edge, and the nearest that is no seam.
            let (mut any, mut edge): (Option<(f64, usize)>, Option<(f64, usize)>) = (None, None);
            for (i, d) in near.iter().copied().enumerate() {
                let d = if d <= nearest + 2.0 * chord { exact_distance(body, i, *p).unwrap_or(d) } else { d };
                let seam = hidden.get(i).copied().unwrap_or(false);
                for (slot, take) in [(&mut any, true), (&mut edge, !seam)] {
                    if take && slot.is_none_or(|(bd, _)| d < bd) {
                        *slot = Some((d, i));
                    }
                }
            }
            let chosen = match (any, edge) {
                (Some((d, _)), Some(e)) if e.0 <= d + tie => Some(e),
                (a, _) => a,
            };
            let Some((_, i)) = chosen.filter(|(d, _)| *d <= reach) else {
                return Err(KernelError::Invalid(format!("no edge near the pick at ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)));
            };
            let (mid, _) = crate::body::polyline_mid(mesh.edges.get(i).map(Vec::as_slice).unwrap_or(&[]));
            out.edges.push(i);
            out.points.push([mid.x, mid.y, mid.z]);
        }
        out.hidden = hidden;
        Ok(out)
    }

    /// How far `p` is from edge `i` itself, sampled along its curve; `None` for an edge with no
    /// curve to sample.
    fn exact_distance(body: &Body, i: usize, p: Vec3) -> Option<f64> {
        const SAMPLES: usize = 512;
        let record = body.record();
        let e = record.edges.get(i)?;
        let eval = solidcraft_geom::brep::eval::CurveEval::new(e.nurbs.as_ref().unwrap_or(&e.curve)).ok()?;
        let [t0, t1] = e.interval;
        let at = |k: usize| eval.point(t0 + (t1 - t0) * k as f64 / SAMPLES as f64);
        let mut best = f64::INFINITY;
        let mut last = at(0)?;
        for k in 1..=SAMPLES {
            let next = at(k)?;
            best = best.min(p.dist_to_segment(last, next));
            last = next;
        }
        Some(best)
    }

    pub(super) struct Picked {
        /// Each pick's edge in the body's edge order.
        pub(super) edges: Vec<usize>,
        /// A point halfway along each.
        pub(super) points: Vec<[f64; 3]>,
        /// Per edge of the body: does a pick pass it over (a seam or a smooth join)?
        pub(super) hidden: Vec<bool>,
    }

    fn sizes_ok(what: &str, sizes: &[f64]) -> Result<()> {
        if sizes.is_empty() || sizes.len() > solidcraft_smlib::MAX_POINTS {
            return Err(KernelError::Invalid(format!("{what}: select 1…{} edges", solidcraft_smlib::MAX_POINTS)));
        }
        if sizes.iter().any(|s| !(s.is_finite() && *s > 1e-6 && *s < 1e6)) {
            return Err(KernelError::Invalid(format!("{what}: the size must be positive and finite")));
        }
        Ok(())
    }

    /// SMLib's blend of each kind.
    fn blend_kind(kind: Kind) -> Blend {
        match kind {
            Kind::Round => Blend::Round,
            Kind::Curvature => Blend::Curvature,
            Kind::Chord => Blend::Chord,
            Kind::Chamfer => Blend::Chamfer,
        }
    }

    pub(super) fn blend(body: &Body, what: &str, kind: Kind, edges: &[Vec3], sizes: &[f64]) -> Result<Body> {
        blend_dropping(body, what, kind, edges, sizes).map(|(b, _)| b)
    }

    /// [`blend`], with whether each of `edges` picked an edge a larger round removed whole: a
    /// round overflowing its faces is built first, and an edge it takes away is gone.
    pub(super) fn blend_dropping(body: &Body, what: &str, kind: Kind, edges: &[Vec3], sizes: &[f64]) -> Result<(Body, Vec<bool>)> {
        body.require_brep(what)?;
        sizes_ok(what, sizes)?;
        let picked = resolve(body, edges)?;
        let next = crate::blend::unfinished_chain(body, edges, &picked.edges, &picked.hidden)?;
        if matches!(kind, Kind::Round) {
            crate::blend::rounds_too_small(body, edges, &picked.edges, sizes)?;
            crate::blend::rounds_into_hollow(body, edges, &picked.edges, sizes)?;
        }
        let tol = pick_tolerance(body);
        let Kind::Round = kind else {
            let kind = blend_kind(kind);
            return run_with(body, what, &picked.edges, &[], |b| b.blend_dropping(kind, &picked.points, sizes, tol))
                .map(|(out, dropped, _)| (out, dropped))
                .map_err(|e| crate::blend::with_continuation(e, next));
        };
        // Constant-radius rounds: the body keeps what they were made on, and a later round at a
        // corner where they meet blends them again with it (crate::blend_corner).
        let out = match crate::blend_corner::later_corners(body, &picked.edges, sizes)? {
            Some((later, new)) => reblend(body, what, &later).map(|(b, dropped_new)| {
                // A picked seam has no round of its own: only the new edges can be removed.
                let dropped = picked
                    .edges
                    .iter()
                    .map(|e| new.iter().position(|(n, _)| n == e).is_some_and(|j| dropped_new.get(j).copied().unwrap_or(false)))
                    .collect();
                (b.with_rounds(body, new), dropped)
            }),
            None => run_with(body, what, &picked.edges, &[], |b| b.blend_dropping(Blend::Round, &picked.points, sizes, tol))
                .map(|(out, dropped, _)| (out.with_rounds(body, picked.edges.iter().copied().zip(sizes.iter().copied()).collect()), dropped)),
        };
        out.map_err(|e| crate::blend::with_continuation(e, next))
    }

    /// The blend `later` makes of its base, every round from the earliest meeting the new edges'
    /// corners on and the new edges in one, with its history over `body`, the body the later
    /// round was given ([`crate::blend_corner::rebase`]), and whether each new edge (in
    /// `later`'s order) was removed whole by a larger, overflowing round.
    fn reblend(body: &Body, what: &str, later: &crate::blend_corner::Later) -> Result<(Body, Vec<bool>)> {
        let base = &later.base;
        let mesh = base.mesh(&crate::MeshTolerance::chord((base.size() * 2e-3).max(1e-3)))?;
        let mut indices = Vec::with_capacity(later.edges.len());
        let mut points = Vec::with_capacity(later.edges.len());
        let mut radii = Vec::with_capacity(later.edges.len());
        for (e, r) in &later.edges {
            let line = mesh.edges.get(*e).ok_or_else(|| failed(what, "corner", "an earlier round's edge is not in the body it was made on"))?;
            let (mid, _) = crate::body::polyline_mid(line);
            indices.push(*e);
            points.push([mid.x, mid.y, mid.z]);
            radii.push(*r);
        }
        sizes_ok(what, &radii)?;
        let tol = pick_tolerance(base);
        let (out, dropped, _) = run_with(base, what, &indices, &[], |b| b.blend_dropping(Blend::Round, &points, &radii, tol))?;
        // The new edges are the last of `later.edges`.
        let dropped_new = dropped.get(dropped.len().saturating_sub(later.new_count())..).map(<[bool]>::to_vec).unwrap_or_default();
        let history = out.history().and_then(|h| crate::blend_corner::rebase(h, later, body));
        Ok((out.with_checked_history(history, &record::counts(&[body])).inherit_regularization_protection(body), dropped_new))
    }

    /// Rounds with corner setbacks: each corner point picks the body's nearest vertex (within
    /// the pick tolerance), handed to SMLib at that vertex's own position.
    pub(super) fn blend_setback(body: &Body, edges: &[Vec3], sizes: &[f64], setbacks: &[(Vec3, f64)]) -> Result<Body> {
        let what = "fillet with corner setbacks";
        body.require_brep(what)?;
        sizes_ok(what, sizes)?;
        if setbacks.is_empty() || setbacks.len() > solidcraft_smlib::MAX_POINTS {
            return Err(KernelError::Invalid(format!("{what}: give 1…{} corners", solidcraft_smlib::MAX_POINTS)));
        }
        if setbacks.iter().any(|(p, d)| !p.is_finite() || !(d.is_finite() && *d > 1e-6 && *d < 1e6)) {
            return Err(KernelError::Invalid(format!("{what}: each corner is a finite point with a positive, finite setback")));
        }
        let picked = resolve(body, edges)?;
        let next = crate::blend::unfinished_chain(body, edges, &picked.edges, &picked.hidden)?;
        crate::blend::rounds_too_small(body, edges, &picked.edges, sizes)?;
        crate::blend::rounds_into_hollow(body, edges, &picked.edges, sizes)?;
        let corners = corner_vertices(body, setbacks.iter().map(|(p, _)| *p))?;
        let distances: Vec<f64> = setbacks.iter().map(|(_, d)| *d).collect();
        let tol = pick_tolerance(body);
        let at: Vec<Vec3> = setbacks.iter().map(|(p, _)| *p).collect();
        run_with(body, what, &picked.edges, &at, |b| b.blend_setback(Blend::Round, &picked.points, sizes, &corners, &distances, tol).map(|r| (r, ())))
            .map(|(out, (), _)| out)
            .map_err(|e| crate::blend::with_continuation(e, next))
    }

    /// [`blend_setback`] with the set-back corners left open (`super::OpenCorners`).
    pub(super) fn blend_setback_open(body: &Body, kind: Kind, edges: &[Vec3], sizes: &[f64], setbacks: &[(Vec3, f64)]) -> Result<super::OpenCorners> {
        let what = "fillet with open setback corners";
        body.require_brep(what)?;
        sizes_ok(what, sizes)?;
        if setbacks.is_empty() || setbacks.len() > solidcraft_smlib::MAX_POINTS {
            return Err(KernelError::Invalid(format!("{what}: give 1…{} corners", solidcraft_smlib::MAX_POINTS)));
        }
        if setbacks.iter().any(|(p, d)| !p.is_finite() || !(d.is_finite() && *d > 1e-6 && *d < 1e6)) {
            return Err(KernelError::Invalid(format!("{what}: each corner is a finite point with a positive, finite setback")));
        }
        let picked = resolve(body, edges)?;
        let next = crate::blend::unfinished_chain(body, edges, &picked.edges, &picked.hidden)?;
        let corners = corner_vertices(body, setbacks.iter().map(|(p, _)| *p))?;
        let distances: Vec<f64> = setbacks.iter().map(|(_, d)| *d).collect();
        let at: Vec<Vec3> = setbacks.iter().map(|(p, _)| *p).collect();
        let tol = pick_tolerance(body);
        let (sheet, holes, order) =
            run_with(body, what, &picked.edges, &at, |b| b.blend_setback_open(blend_kind(kind), &picked.points, sizes, &corners, &distances, tol))
                .map_err(|e| crate::blend::with_continuation(e, next))?;
        // SMLib's indices in the sheet's own orders.
        let place = |list: &[usize], i: usize| if list.is_empty() { Some(i) } else { list.get(i).copied() };
        let renumber = |hole: &solidcraft_smlib::Hole| -> Result<Vec<(usize, bool)>> {
            hole.edges
                .iter()
                .map(|h| {
                    let edge = match &order {
                        Some(o) => place(&o.edges, h.edge),
                        None => Some(h.edge),
                    };
                    edge.filter(|e| *e < sheet.edge_count())
                        .map(|e| (e, h.forward))
                        .ok_or_else(|| failed(what, "hole", "an edge SMLib names is not in the sheet"))
                })
                .collect()
        };
        let holes = holes.iter().map(renumber).collect::<Result<Vec<_>>>()?;
        Ok(super::OpenCorners { sheet, holes })
    }

    /// The body's vertex nearest each point, within the pick tolerance: the ends of its edges.
    fn corner_vertices(body: &Body, picks: impl Iterator<Item = Vec3>) -> Result<Vec<[f64; 3]>> {
        let mesh = body.mesh(&crate::MeshTolerance::chord((body.size() * 2e-3).max(1e-3)))?;
        let ends: Vec<Vec3> = mesh.edges.iter().flat_map(|l| l.first().into_iter().chain(l.last())).copied().collect();
        let reach = pick_tolerance(body);
        picks
            .map(|p| {
                ends.iter()
                    .map(|v| (v.dist(p), *v))
                    .filter(|(d, _)| *d <= reach)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, v)| [v.x, v.y, v.z])
                    .ok_or_else(|| KernelError::Invalid(format!("no corner near the setback point ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)))
            })
            .collect()
    }

    pub(super) fn sides(body: &Body, edges: &[Vec3], d1: f64, side: crate::ChamferSide, flip: bool) -> Result<Body> {
        body.require_brep("chamfer")?;
        sizes_ok("chamfer", &vec![d1; edges.len()])?;
        let picked = resolve(body, edges)?;
        let next = crate::blend::unfinished_chain(body, edges, &picked.edges, &picked.hidden)?;
        let second = match side {
            crate::ChamferSide::Distance(d) => ChamferSecond::Distance(d),
            crate::ChamferSide::Angle(a) => ChamferSecond::Angle(a),
        };
        let up = if flip { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] };
        let tol = pick_tolerance(body);
        run(body, "chamfer", &picked.edges, |b| b.chamfer_sides(&picked.points, tol, d1, second, up))
            .map_err(|e| crate::blend::with_continuation(e, next))
    }

    pub(super) fn variable(body: &Body, edge_at: Vec3, start: Vec3, spline: &RadiusSpline) -> Result<Body> {
        body.require_brep("fillet")?;
        if !start.is_finite() {
            return Err(KernelError::Invalid("the start point must be finite".into()));
        }
        let picked = resolve(body, &[edge_at])?;
        let next = crate::blend::unfinished_chain(body, &[edge_at], &picked.edges, &picked.hidden)?;
        let [at] = picked.points[..] else { return Err(KernelError::Invalid("one edge".into())) };
        let tol = pick_tolerance(body);
        run(body, "variable fillet", &picked.edges, |b| {
            b.fillet_variable(at, tol, [start.x, start.y, start.z], spline.degree, &spline.knots, &spline.radii)
        })
        .map_err(|e| crate::blend::with_continuation(e, next))
    }

    /// `op` on `body` in SMLib, tagged as operand 0, checked, decoded, with its history.
    fn run(body: &Body, what: &str, edges: &[usize], op: impl FnOnce(&Brep) -> solidcraft_smlib::Result<Brep>) -> Result<Body> {
        run_with(body, what, edges, &[], |b| op(b).map(|r| (r, ()))).map(|(out, (), _)| out)
    }

    /// [`run`] for an `op` that also gives something of its own, with where SMLib's faces and
    /// edges of the result are in the body's orders.
    /// `corners`: the corner points of a blend with setbacks, in the order `op` gives them to
    /// SMLib, so a refused corner is named by its place.
    fn run_with<T>(
        body: &Body,
        what: &str,
        edges: &[usize],
        corners: &[Vec3],
        op: impl FnOnce(&Brep) -> solidcraft_smlib::Result<(Brep, T)>,
    ) -> Result<(Body, T, Option<solidcraft_smlib::history::Order>)> {
        guard(what, || {
            let (sb, order) = smlib_body_ordered(body)?;
            let tagged = sb.tagged(0).map_err(|e| failed(what, "tag", e))?;
            let (r, extra) = op(&tagged).map_err(|e| match e {
                SmError::Unsupported => super::unsupported(&format!("this {what}, which SMLib cannot build exactly")),
                SmError::CornerRefused { corner } => match corners.get(corner) {
                    // The same words as `super::unsupported`, the reason after them.
                    Some(p) => KernelError::Invalid(format!(
                        "not supported yet: setting back the corner at ({:.3}, {:.3}, {:.3}) with the SMLib kernel (a G2 corner is set \
                         back): SMLib sets back a corner only where every edge meeting it is rounded and they are all convex or all concave",
                        p.x, p.y, p.z
                    )),
                    None => failed(what, "operation", SmError::CornerRefused { corner }),
                },
                SmError::NoEffect => KernelError::Failed(format!("SMLib made no change: the {what} reported success but built no face")),
                SmError::CornerUnsupported => KernelError::Invalid(format!(
                    "not supported yet: the {what} meets a corner of this kind, which SMLib's blends do not handle yet (where its edges \
                     run on tangentially into each other, or meet in a way no blend corner builds)"
                )),
                SmError::NoRoom => KernelError::TooLarge(format!("the {what} is larger than the faces beside its edges allow")),
                SmError::Unported(why) => KernelError::Invalid(format!("not supported yet: {why}")),
                e => failed(what, "operation", e),
            })?;
            // Checked here unless the evaluation's validation mode leaves it to the feature's end
            // (`crate::validation`).
            let step = crate::validation::step();
            if step == crate::validation::Step::Check {
                let check = r.check(CheckLevel::All).map_err(|e| failed(what, "check", e))?;
                if !check.is_valid() {
                    let input_fails = sb.check(CheckLevel::All).is_ok_and(|c| !c.is_valid());
                    let step = if input_fails {
                        "SMLib's check refuses the result (the input already fails it)"
                    } else {
                        "SMLib's check refuses the result"
                    };
                    return Err(failed(what, step, check));
                }
            }
            let (out, result_order) = body_from_smlib_ordered(&r)?;
            let history = match (order, &result_order) {
                (Some(o), Some(ro)) => match r.history().map_err(|e| failed(what, "history", e))?.history(&[o], ro) {
                    Ok(mut h) => {
                        let operand = History::identity(body).edges;
                        let taken_up = taken_up(body, edges, &h);
                        solidcraft_smlib::history::blends(&mut h, &operand, edges, &taken_up);
                        Some(with_corners(h))
                    }
                    Err(e) => {
                        log::debug!(target: "smlib", "{what} history: {e}");
                        None
                    }
                },
                _ => None,
            };
            let out = out.with_checked_history(history, &record::counts(&[body])).inherit_regularization_protection(body);
            let out = if step == crate::validation::Step::Defer {
                out.left_unchecked(&format!("SMLib {what}"), crate::validation::Checked::AsIs, r)
            } else {
                out
            };
            Ok((out, extra, result_order))
        })
    }

    /// `h` with each blend face still without origin (a corner patch where blends meet) made in
    /// place of all the edges of the blend faces next to it, and every
    /// edge of a face new to the blend recorded as where its faces meet (two chamfers' mitre is
    /// where the two chamfers meet).
    fn with_corners(mut h: History) -> History {
        let mut corners: Vec<(usize, BTreeSet<FaceOrigin>)> = Vec::new();
        for (i, f) in h.faces.iter().enumerate() {
            if f.change != Change::Generated || !f.origins.is_empty() {
                continue;
            }
            let around: BTreeSet<FaceOrigin> = h
                .edges
                .iter()
                .filter(|e| e.faces.contains(&i))
                .flat_map(|e| e.faces.iter().filter(|g| **g != i))
                .filter_map(|g| h.faces.get(*g))
                .flat_map(|g| g.origins.iter().copied())
                .filter(|o| matches!(o, FaceOrigin::Edge { .. }))
                .collect();
            if !around.is_empty() {
                corners.push((i, around));
            }
        }
        for (i, origins) in corners {
            if let Some(f) = h.faces.get_mut(i) {
                *f = record::generated(origins.into_iter().collect());
            }
        }
        let faces = h.faces.clone();
        for e in &mut h.edges {
            if e.faces.iter().any(|f| faces.get(*f).is_some_and(|f| f.change == Change::Generated)) {
                e.origins = record::meeting(&e.faces, &faces);
            }
        }
        h
    }

    /// The operand's edges the blend took up besides the picked ones: every edge no result edge
    /// comes from (one the blend's corners took up, or one it ran over), in the operand's edge
    /// order.
    fn taken_up(body: &Body, picked: &[usize], h: &History) -> Vec<usize> {
        let kept: BTreeSet<usize> = h
            .edges
            .iter()
            .flat_map(|e| e.origins.iter())
            .filter_map(|o| match o {
                EdgeOrigin::Edge { operand: 0, edge } => Some(*edge),
                _ => None,
            })
            .collect();
        (0..body.edge_count()).filter(|e| !kept.contains(e) && !picked.contains(e)).collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use solidcraft_geom::Vec3;

    use super::*;
    use crate::history::{Change, FaceOrigin};
    use crate::{box_solid, measure};

    const QUARTER: f64 = 1.0 - std::f64::consts::FRAC_PI_4;

    fn cube() -> Body {
        box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0)).unwrap()
    }

    fn volume(b: &Body) -> f64 {
        measure(b).unwrap().volume
    }

    /// Within the kernel measure's accuracy on a curved face (a few 1e-6 of the volume; SMLib's
    /// own exact volumes match to 1e-3 mm³ in the C API's tests).
    fn near(got: f64, want: f64) -> bool {
        (got - want).abs() <= 5e-6 * want.abs()
    }

    /// The edge of `b` nearest `p`, in its edge order.
    fn edge_at(b: &Body, p: Vec3) -> usize {
        b.nearest_edge(p, 1e-3).unwrap().unwrap().0
    }

    #[test]
    fn a_round_loses_its_corner_and_its_face_comes_from_the_edge() {
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        let r = imp::blend(&b, "fillet", Kind::Round, &[at], &[2.0]).unwrap();
        assert!(near(volume(&r), 8000.0 - QUARTER * 4.0 * 20.0), "{}", volume(&r));
        assert_eq!(r.face_count(), 7);
        let h = r.history().expect("a history");
        let blends: Vec<_> = h.faces.iter().filter(|f| f.change == Change::Generated).collect();
        assert_eq!(blends.len(), 1);
        assert_eq!(blends[0].origins, vec![FaceOrigin::Edge { operand: 0, edge: edge_at(&b, at) }]);
    }

    #[test]
    fn a_pick_at_a_vertex_takes_the_nearest_edge_by_its_midpoint() {
        // (20, 20, 10) is on the vertical edge only; (20, 20, 20) is a corner of three edges, which
        // the body's own order resolves, never left to SMLib.
        let b = cube();
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(20.0, 20.0, 20.0)], &[1.0]).is_ok());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(60.0, 60.0, 60.0)], &[1.0]).is_err());
    }

    /// A ball's rim on a block's back face, tangent to the back face's bottom edge at (2.5, 22.5, 0).
    /// Near there the two are closer than the mesh's chord, which can't tell them apart.
    #[test]
    fn a_pick_on_a_rim_tangent_to_a_straight_edge_takes_the_rim() {
        let block = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(30.0, 22.5, 17.5)).unwrap();
        let ball = crate::sphere(Vec3::new(2.5, 22.5, 5.0), 5.0).unwrap();
        let b = crate::boolean(&block, &ball, crate::BoolOp::Cut).unwrap().unwrap();
        let on_rim = |[x, y, z]: [f64; 3]| (y - 22.5).abs() < 1e-6 && ((x - 2.5).hypot(z - 5.0) - 5.0).abs() < 1e-3;
        for degrees in [2.0f64, 5.0, 10.0, 20.0, 45.0] {
            let a = degrees.to_radians();
            let pick = Vec3::new(2.5 + 5.0 * a.sin(), 22.5, 5.0 - 5.0 * a.cos());
            let picked = imp::resolve(&b, &[pick]).unwrap();
            assert!(on_rim(picked.points[0]), "{degrees} degrees from the tangent point: took the edge through {:?}", picked.points[0]);
            let beside = imp::resolve(&b, &[Vec3::new(pick.x, 22.5, 0.0)]).unwrap();
            assert!(beside.points[0][2].abs() < 1e-9, "the straight edge under it: took the edge through {:?}", beside.points[0]);
        }
    }

    #[test]
    fn a_pick_where_a_seam_meets_a_rim_takes_the_rim() {
        // Every point of the top rim's circle, the seam's top end among them, is on the rim.
        let c = crate::cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 8.0, 10.0).unwrap();
        assert!(c.edge_count() > 2, "the cylinder has a seam");
        for k in 0..8 {
            let a = k as f64 * std::f64::consts::FRAC_PI_4;
            let picked = imp::resolve(&c, &[Vec3::new(8.0 * a.cos(), 8.0 * a.sin(), 10.0)]).unwrap();
            let [_, _, z] = picked.points[0];
            assert!((z - 10.0).abs() < 1e-9, "the pick at {a} took an edge through z = {z}");
        }
    }

    #[test]
    fn a_pick_where_a_bosss_seam_meets_its_base_takes_the_base_circle() {
        // A boss joined onto a block, as the ladder's running part has (07-fillets/017 picks
        // its base circle where the seam starts): its side lies on both sides of its seam.
        let block = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 20.0)).unwrap();
        let boss = crate::cylinder(Vec3::new(15.0, 15.0, 20.0), Vec3::new(0.0, 0.0, 1.0), 8.0, 10.0).unwrap();
        let b = crate::boolean(&block, &boss, crate::BoolOp::Union).unwrap().unwrap();
        let mesh = b.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
        let seams = crate::topo::seam_flags(&b, &mesh);
        let record = b.record();
        let one_face: Vec<usize> = crate::history::edge_faces(record)
            .iter()
            .zip(&record.edges)
            .enumerate()
            .filter(|(_, (faces, e))| faces.len() == 1 && e.uses.len() == 2)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(one_face.len(), 1, "the boss's side meets itself along its seam");
        for &i in &one_face {
            assert!(seams[i], "edge {i}, the boss's side on both of its sides, is a seam");
            assert!(!mesh.edge_pickable(i), "edge {i} is drawn and picked as a seam");
            // Its end on the base circle, where a pick takes the circle.
            let line = &mesh.edges[i];
            let foot = [line[0], line[line.len() - 1]].into_iter().find(|p| (p.z - 20.0).abs() < 1e-6).unwrap();
            let picked = imp::resolve(&b, &[foot]).unwrap();
            let [_, _, z] = picked.points[0];
            assert!((z - 20.0).abs() < 1e-9, "the pick at {foot:?} took an edge through z = {z}");
        }
    }

    #[test]
    fn each_edge_takes_its_own_size() {
        let b = cube();
        let picks = [Vec3::new(10.0, 20.0, 20.0), Vec3::new(10.0, 0.0, 20.0)];
        let r = imp::blend(&b, "fillet", Kind::Round, &picks, &[2.0, 3.0]).unwrap();
        assert!(near(volume(&r), 8000.0 - QUARTER * 13.0 * 20.0), "{}", volume(&r));
        let c = imp::blend(&b, "chamfer", Kind::Chamfer, &picks, &[2.0, 3.0]).unwrap();
        assert!(near(volume(&c), 8000.0 - 0.5 * 13.0 * 20.0), "{}", volume(&c));
        assert_eq!(c.history().map(|h| h.faces.iter().filter(|f| matches!(f.origins[..], [FaceOrigin::Edge { .. }])).count()), Some(2));
    }

    #[test]
    fn a_linear_radius_runs_from_the_start_end() {
        // 3 at x = 0 to 1 at x = 20: the balls' union, a cone, takes 18.5377834 (their great
        // circles, (1 - pi/4) L (r0^2 + r0 r1 + r1^2) / 3, would take 18.5988), from either end
        // given as the start with the law reversed.
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        let law = RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], radii: vec![3.0, 1.0] };
        let r = imp::variable(&b, at, Vec3::new(0.0, 20.0, 20.0), &law).unwrap();
        let want = 8000.0 - 18.5377834;
        assert!((volume(&r) - want).abs() < 0.01, "{}", volume(&r));
        let back = imp::variable(&b, at, Vec3::new(20.0, 20.0, 20.0), &law.reversed()).unwrap();
        assert!((volume(&back) - volume(&r)).abs() < 1e-6 * 8000.0);
    }

    /// A chamfer 5 back along the top and 2 down the side of the cube's edge y = 20, z = 20 (or
    /// 2 along the top and 5 down with `flip`): a 5 × 2 / 2 triangle along the 20 mm edge, its
    /// face coming from the edge.
    #[test]
    fn a_chamfer_takes_its_two_setbacks() {
        let b = cube();
        let at = Vec3::new(10.0, 20.0, 20.0);
        for flip in [false, true] {
            let r = imp::sides(&b, &[at], 5.0, crate::ChamferSide::Distance(2.0), flip).unwrap();
            assert!(near(volume(&r), 8000.0 - 100.0), "{}", volume(&r));
            let top = if flip { 18.0 } else { 15.0 };
            assert!(
                r.edges(0.01).unwrap().iter().any(|e| e.points.iter().all(|p| (p.y - top).abs() < 1e-6 && (p.z - 20.0).abs() < 1e-6)),
                "flip {flip}"
            );
            let h = r.history().unwrap();
            assert!(h.faces.iter().any(|f| f.origins == [crate::history::FaceOrigin::Edge { operand: 0, edge: edge_at(&b, at) }]));
        }
        let r = imp::sides(&b, &[at], 5.0, crate::ChamferSide::Angle(45f64.to_radians()), false).unwrap();
        assert!(near(volume(&r), 8000.0 - 12.5 * 20.0), "{}", volume(&r));
    }

    /// The ladder's 08/006: a 2 mm chamfer of a cylinder's top rim. SMLib's cone and rail lie some
    /// 2e-5 mm off the faces they bound, more than the kernel's working tolerance; the body keeps
    /// the tolerances SMLib's record gave them, so its own record (and the saved body's) is valid
    /// and SMLib's check takes it back.
    #[test]
    fn an_smlib_blend_keeps_its_record_valid() {
        let c = crate::cylinder(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), 10.0, 20.0).unwrap();
        let r = imp::blend(&c, "chamfer", Kind::Chamfer, &[Vec3::new(10.0, 0.0, 20.0)], &[2.0]).unwrap();
        let v = crate::validity(&r);
        assert!(v.record.is_ok(), "{:?}", v.record);
        assert!(matches!(v.smlib, Some(crate::SmlibCheck::Valid)), "{:?}", v.smlib);
        let saved = crate::decode_native_body(&crate::encode_native_body(&r).unwrap()).unwrap();
        assert!(crate::validity(&saved).record.is_ok());
        assert!(
            crate::validity(
                &crate::transform_matrix(&saved, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, 0.0, 0.0, 1.0]]).unwrap()
            )
            .record
            .is_ok()
        );
    }

    #[test]
    fn options_smlib_cannot_build_are_refused() {
        assert!(unsupported("x").to_string().contains("not supported yet"));
        let b = cube();
        // A radius law down to zero inside the edge would pinch the round to a point.
        let pinch = RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 0.5, 1.0, 1.0], radii: vec![4.0, 0.0, 4.0] };
        assert!(imp::variable(&b, Vec3::new(10.0, 20.0, 20.0), Vec3::new(0.0, 20.0, 20.0), &pinch).is_err());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(10.0, 20.0, 20.0)], &[f64::NAN]).is_err());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[Vec3::new(f64::NAN, 20.0, 20.0)], &[1.0]).is_err());
        assert!(imp::blend(&b, "fillet", Kind::Round, &[], &[]).is_err());
    }

    /// A smooth edge (a round's seam with the face it runs into) has nothing to round: refused
    /// before SMLib, saying so. An edge that runs on tangentially into one left out is SMLib's to
    /// end: the end face's top edge into the round's end arc builds, its round run on through the
    /// arc's round until it leaves the material (rig fillets/1207-fillet-box-end-edge-into-
    /// unblended-round), and with its tangent chain the edge builds too; a vertical edge run on
    /// up a top round's end arc into the corner (ladder 07-fillets/018) builds.
    #[test]
    fn smooth_edges_and_unfinished_tangent_chains_are_refused_saying_why() {
        let b = cube();
        let rounded = fillet(&b, &[Vec3::new(10.0, 20.0, 20.0)], 3.0).unwrap();
        // the seam where the r3 round meets the top, 3 in from the old edge
        let seam = imp::blend(&rounded, "fillet", Kind::Round, &[Vec3::new(10.0, 17.0, 20.0)], &[1.0]).unwrap_err().to_string();
        assert!(seam.contains("is smooth: its faces meet tangentially, so there is nothing to round"), "{seam}");
        // the end face's top edge runs on tangentially into the round's end arc: its r1 round
        // goes on along y through the r3 round until that drops below it, at y = 17 + sqrt(5)
        // (the rig case's closed form: 8000 - 20 (9 - 9 pi/4) - 17 (1 - pi/4) - 0.218236)
        let top = Vec3::new(0.0, 8.0, 20.0);
        let alone = fillet(&rounded, &[top], 1.0).unwrap();
        assert!((volume(&alone) - 7957.505202).abs() < 1e-3, "{}", volume(&alone));
        let chain = crate::tangent_chain(&rounded, &[top]).unwrap();
        assert!(chain.len() > 1, "{chain:?}");
        let whole = fillet(&rounded, &chain, 1.0).unwrap();
        assert!(volume(&whole) < volume(&rounded));

        // r5 and r3 on two top edges at a corner, then r2 on the vertical edge under them: it
        // runs on up the r5 round's end arc, and SMLib ends it there
        let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(60.0, 40.0, 20.0)).unwrap();
        let b = fillet(&b, &[Vec3::new(30.0, 40.0, 20.0)], 5.0).unwrap();
        let b = fillet(&b, &[Vec3::new(60.0, 20.0, 20.0)], 3.0).unwrap();
        let vertical = Vec3::new(60.0, 40.0, 8.0);
        let chain = crate::tangent_chain(&b, &[vertical]).unwrap();
        assert!(chain.len() > 1, "{chain:?}");
        let run_on = fillet(&b, &[vertical], 2.0).unwrap();
        assert!(volume(&run_on) < volume(&b));
    }
}
