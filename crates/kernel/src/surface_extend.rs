//! Extend (Surface › Extend): boundary edges of a surface body pushed outward across their
//! face, by a distance, up to a plane, a face or a whole body, or (natural only) back to the surface's own
//! bounds. Three ways to continue past an edge, each SMLib's extension of the surface
//! (`smlib_extend_sheet`, `SmBSplineSurface::CreateExtendedSurface`):
//!
//! - natural: the surface itself continued (a B-spline's end spans as their own polynomials,
//!   which keeps a plane, a cylinder or any conic section on itself);
//! - tangent: continued tangent to the surface across the edge (G1);
//! - curvature: continued keeping the surface's curvature across the edge too (G2).
//!
//! The face is extended where it is its surface's whole parameter box and the edge is a whole
//! side of it: the box grows past that side, the sides next to it continue along the surface's
//! parameter lines, and the face stays one face on the extended surface. Edges picked on two
//! sides grow the box on both, meeting at the box's corner.
//!
//! A body of several faces grows face by face, each extended alone as above and the faces sewn
//! back together. A side that lengthens is shared with the face next to it: that face grows too,
//! past its edge continuing the picked one, when the two meet tangent along the side (the faces
//! of one smooth surface, a spline's spans extruded, grow as one); across a crease it grows only
//! when that edge is picked as well, and otherwise the extension is refused (it would run past
//! the face beside it). The lengthened sides must meet again: faces whose extensions part are
//! refused. Up to a target, the face is extended
//! past it and trimmed back where the target meets it ([`crate::trim_keep`]: SMLib's exact
//! intersection). Up to a whole body, it is cut where it first runs into the body's faces as they
//! are, bounded where they end ([`crate::surface_trim::stop_extension`]). Back to the bounds is the face untrimmed to its surface's box
//! ([`crate::untrim`]). A plane's straight edges pushed out with their neighbours lengthened
//! keep the exact planar extension ([`crate::surfaces::extend`]).
//!
//! Refused rather than built: edges between two faces, edges of holes, and (not supported yet)
//! an edge that is not a whole side of its surface's box (a trimmed edge, whose extension is no
//! curve SMLib holds exactly).
//!
//! A fourth mode, law, builds the extension from the picked edges themselves, not from the
//! surface's box, so a trimmed edge extends too: a ruled strip leaving the edge at a constant
//! angle to the surface (0: tangent), by a distance or up to a plane or face (the strip alone cut
//! there, the surface never), sewn to the surface along the edge. See [`ExtendMode::Law`] and the
//! notes at [`law_strip`].

use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

use solidcore_ops::ruled_strip::{Reach, far_tolerance, ruling_angle};
use solidcore_record::Vec3;
use solidcore_record::brep::{self};
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_smlib::trim::Continuation;

use crate::body::Body;
use crate::history::{Change, FaceOrigin};
use crate::native::{body_from_smlib_ordered, smlib_body_ordered};
use crate::surface_trim::TrimKeep;
use crate::surfaces::Parts;
use crate::{KernelError, Result, SplitTool, record};

/// Most edge picks.
const MAX_PICKS: usize = 10_000;
/// How far past its edge, in body sizes, an extension up to a target runs before it is trimmed
/// back to the target: a target farther than that is not reached.
const REACH: f64 = 50.0;
/// Samples along an edge that find the edge nearest a pick.
const PICK_SAMPLES: usize = 32;

/// How a surface continues past its edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExtendMode {
    /// The surface itself, continued.
    #[default]
    Natural,
    /// Continued tangent to the surface across the edge (G1).
    Tangent,
    /// Continued keeping the surface's curvature across the edge (G2).
    Curvature,
    /// A strip built from the picked edges themselves, ruled by straight lines leaving each edge
    /// at [`ExtendOptions::angle`] to the surface (0: tangent to it, square to the edge), so a
    /// trimmed edge extends too.
    Law,
}

/// The largest law-extension angle (radians, either way) short of a right angle: at 90° the
/// strip would stand along the surface's normal, no longer an extension of it
/// ([`solidcore_ops::ruled_strip::ANGLE_LIMIT`]).
pub const LAW_ANGLE_LIMIT: f64 = solidcore_ops::ruled_strip::ANGLE_LIMIT;

/// How far an extension goes.
#[derive(Clone, Copy, Debug)]
pub enum ExtendLimit<'a> {
    /// This far from the edge, measured across the surface.
    Distance(f64),
    /// To where it meets a plane, or the surface of a face (extended).
    UpTo(&'a SplitTool<'a>),
    /// To where it first runs into a whole body, any face of it as it is, never extended (natural
    /// only). An extension part of which never meets the body is refused.
    UpToBody(&'a Body),
    /// Back to the surface's own bounds (natural only), undoing a trim: a free-form surface's
    /// parameter box, an analytic surface's the box of the face's own span.
    Bounds,
}

/// What [`extend_surface`] does.
#[derive(Clone, Copy, Debug)]
pub struct ExtendOptions<'a> {
    pub mode: ExtendMode,
    pub limit: ExtendLimit<'a>,
    /// Lengthen the edges next to an extended edge to meet the extension. On a face that is its
    /// surface's whole box those edges are parameter lines, which the extension continues.
    pub neighbours: bool,
    /// The law extension's angle to the surface at the edge (radians, between −90° and 90°
    /// exclusive): 0 continues the surface tangent to it; positive turns the strip toward the
    /// side the face's normal points to, negative away. The other modes ignore it.
    pub angle: f64,
}

/// The boundary edges of `body` picked by points on them, extended as `opts` says. The result is
/// a surface body whose history is over `[body]`: an extended face is the face it was (bounded
/// differently) and its edges the edges they continue.
pub fn extend_surface(body: &Body, at: &[Vec3], opts: &ExtendOptions) -> Result<Body> {
    body.require_brep("extend")?;
    if at.is_empty() {
        return Err(KernelError::Invalid("pick the edges to extend".into()));
    }
    if at.len() > MAX_PICKS {
        return Err(KernelError::Invalid("too many edges picked".into()));
    }
    if at.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("an edge point is not finite".into()));
    }
    match opts.limit {
        ExtendLimit::Distance(d) if !(d.is_finite() && d > 0.0 && d <= 1e6) => {
            return Err(KernelError::Invalid("the extension distance must be positive".into()));
        }
        ExtendLimit::Bounds if opts.mode != ExtendMode::Natural => {
            return Err(KernelError::Invalid("only a natural extension goes back to the surface's bounds".into()));
        }
        // A run-off face has sides of its own, which the far end can't be told from.
        ExtendLimit::UpToBody(_) if opts.mode == ExtendMode::Law => {
            return Err(KernelError::Invalid("not supported yet: a law extension up to a whole body (pick a face of it)".into()));
        }
        // A law strip is cut only by a plane or a face; sketch curves would trim it as a split.
        ExtendLimit::UpTo(SplitTool::Curves { .. }) if opts.mode == ExtendMode::Law => {
            return Err(KernelError::Invalid("not supported yet: a law extension up to sketch curves (pick a plane or a face)".into()));
        }
        ExtendLimit::UpToBody(_) if opts.mode != ExtendMode::Natural => {
            return Err(KernelError::Invalid("not supported yet: a tangent or curvature extension up to a whole body (pick a face of it)".into()));
        }
        _ => {}
    }
    if opts.mode == ExtendMode::Law && ruling_angle(opts.angle).is_none() {
        return Err(KernelError::Invalid("the law extension's angle must lie between -90° and 90°".into()));
    }
    let record = crate::native::brep_record(body)?;
    if opts.mode == ExtendMode::Law {
        // The usual refusals: edges between two faces, edges of holes.
        let picked = picked_edges(&record, body.size(), at)?;
        let edges: BTreeSet<usize> = picked.iter().map(|(e, _)| *e).collect();
        return match opts.limit {
            ExtendLimit::Distance(d) => law(body, at, edges.len(), opts.angle, d),
            ExtendLimit::UpTo(tool) => law_up_to(body, at, &record, &edges, opts.angle, tool),
            ExtendLimit::UpToBody(_) | ExtendLimit::Bounds => {
                Err(KernelError::Invalid("not supported yet: a law extension takes a distance or a face to extend up to".into()))
            }
        };
    }
    // A plane's straight edges pushed out with their neighbours lengthened: the exact planar
    // extension (what designs saved before the other modes build).
    if let (ExtendMode::Natural, ExtendLimit::Distance(d), true) = (opts.mode, opts.limit, opts.neighbours)
        && plain_planar(&record)
    {
        return crate::surfaces::extend(body, at, d);
    }
    let picked = picked_edges(&record, body.size(), at)?;
    match opts.limit {
        ExtendLimit::Bounds => {
            let faces: BTreeSet<usize> = picked.iter().map(|(_, f)| *f).collect();
            back_to_bounds(body, &faces.into_iter().collect::<Vec<_>>())
        }
        ExtendLimit::Distance(d) => continued(body, at, &picked, d, opts.mode).map(|(out, _)| out),
        ExtendLimit::UpTo(tool) => up_to(body, at, &picked, tool, opts.mode),
        ExtendLimit::UpToBody(target) => up_to_body(body, at, &picked, target, opts.mode),
    }
}

/// One planar face, one loop of straight edges: what [`crate::surfaces::extend`] takes.
fn plain_planar(record: &brep::Brep) -> bool {
    let [face] = record.faces.as_slice() else { return false };
    let [brep::Loop::Edges { edges }] = face.loops.as_slice() else { return false };
    matches!(face.surface, brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. })
        && edges.iter().all(|u| record.edges.get(u.edge as usize).is_some_and(|e| matches!(e.curve, brep::Curve::Line { .. })))
}

/// The boundary edges the points pick, each with its face: for each point, the record edge
/// nearest it (by samples along each edge), within a twentieth of the body's size. An edge two
/// faces share, or one of a hole, is refused.
fn picked_edges(record: &brep::Brep, size: f64, at: &[Vec3]) -> Result<Vec<(usize, usize)>> {
    let curves: Vec<CurveEval> = record
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| CurveEval::new(e.parameters()).map_err(|err| KernelError::Failed(format!("extend: edge {i}'s curve: {err}"))))
        .collect::<Result<_>>()?;
    let reach = size * 0.05 + 1e-6;
    let mut picked = Vec::with_capacity(at.len());
    for p in at {
        let mut best: Option<(f64, usize)> = None;
        for (i, (e, c)) in record.edges.iter().zip(&curves).enumerate() {
            let [t0, t1] = e.interval;
            for k in 0..=PICK_SAMPLES {
                let Some(q) = c.point(t0 + (t1 - t0) * k as f64 / PICK_SAMPLES as f64) else { continue };
                let d = q.dist(*p);
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, i));
                }
            }
        }
        let (index, edge) = match best {
            Some((d, i)) if d <= reach => record.edges.get(i).map(|e| (i, e)),
            _ => None,
        }
        .ok_or_else(|| KernelError::Invalid(format!("no edge at {p}")))?;
        let [only] = edge.uses.as_slice() else {
            return Err(KernelError::Invalid("an edge between two faces can't be extended: pick edges on the surface's boundary".into()));
        };
        // A face's first loop is its outer one (as SMLib keeps them).
        if only.r#loop != 0 {
            return Err(KernelError::Invalid("not supported yet: extending an edge of a hole (Untrim takes holes away)".into()));
        }
        picked.push((index, only.face as usize));
    }
    Ok(picked)
}

/// The faces of `body` continued `distance` past the picked edges (`at`, which `picked` names
/// with their faces), as `mode` says, and the faces that grew. One face is extended by SMLib
/// ([`one_face`]); a body of several grows face by face ([`grown`] says which past which edges),
/// each face extended alone and the faces sewn back together, each the face it was. Faces whose
/// lengthened sides part (no longer one edge) are refused.
fn continued(body: &Body, at: &[Vec3], picked: &[(usize, usize)], distance: f64, mode: ExtendMode) -> Result<(Body, Vec<usize>)> {
    if body.face_count() == 1 {
        return Ok((one_face(body, at, distance, mode)?, vec![0]));
    }
    let r = body.record();
    let grow = grown(r, picked)?;
    let mut pieces = Vec::with_capacity(grow.len() + 1);
    for (&f, edges) in &grow {
        let alone = crate::surfaces::taken(body, r, &[(f, false)])?;
        let points = edges.iter().map(|e| edge_middle(r, *e)).collect::<Result<Vec<_>>>()?;
        let longer = one_face(&alone, &points, distance, mode)?;
        pieces.push((longer.record().clone(), vec![record::from_face(0, f, Change::Modified)]));
    }
    let rest: Vec<(usize, bool)> = (0..r.faces.len()).filter(|f| !grow.contains_key(f)).map(|f| (f, false)).collect();
    if !rest.is_empty() {
        let c = Parts::of(r, &rest)?.canonical()?;
        let kept = c
            .faces
            .iter()
            .map(|p| rest.get(*p).map(|(f, _)| record::from_face(0, *f, Change::Kept)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| failed("sew", "a face of the rest of the body is missing"))?;
        pieces.push((c.brep, kept));
    }
    let out = crate::surfaces::sheet_of(pieces, true, &[body])?;
    if (out.face_count(), out.edge_count()) != (body.face_count(), body.edge_count()) {
        return Err(KernelError::Invalid(
            "the extended faces part along the sides they lengthen: their extensions don't meet again (extend each face on its own)".into(),
        ));
    }
    Ok((out, grow.into_keys().collect()))
}

/// The boundary edges each face grows past (by face): the `picked` ones (edge, face), and those
/// that continue a grown edge across a side it lengthens onto a face meeting its own tangent
/// there. A side lengthening along a crease onto a face that does not grow past the edge
/// continuing the grown one is refused: the extension would run past that face.
fn grown(r: &brep::Brep, picked: &[(usize, usize)]) -> Result<BTreeMap<usize, BTreeSet<usize>>> {
    let sides = crate::surfaces::face_sides(r);
    let sign = |f: usize| sides.get(f).map(|forward| if *forward { 1.0 } else { -1.0 });
    let mut grow: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut todo = picked.to_vec();
    // (face, its edge that must grow, the creased side between): checked once all have grown.
    let mut creased = Vec::new();
    let mut count = 0usize;
    while let Some((e, f)) = todo.pop() {
        if !grow.entry(f).or_default().insert(e) {
            continue;
        }
        count += 1;
        if count > MAX_PICKS {
            return Err(KernelError::Invalid("too many edges to extend".into()));
        }
        for (side, at) in next_to(r, f, e)? {
            let uses = &record_edge(r, side)?.uses;
            let other = match uses.as_slice() {
                [_] => continue,
                [a, b] => [a.face, b.face].into_iter().map(|g| g as usize).find(|g| *g != f),
                _ => return Err(KernelError::Invalid(format!("not supported yet: extending along edge {side}, which more than two faces share"))),
            };
            // A seam: the face lengthens along it on both sides at once.
            let Some(g) = other else { continue };
            let cont = continuing(r, g, side, at)?;
            if !matches!(record_edge(r, cont)?.uses.as_slice(), [only] if only.r#loop == 0) {
                return Err(KernelError::Invalid(format!(
                    "not supported yet: extending past edge {e}, whose side {side} runs on into the inside of the face next to it"
                )));
            }
            if crate::tangent_edge::tangent_all_along(r, sign, side, crate::smooth_edges::ANGLE)? {
                todo.push((cont, g));
            } else {
                creased.push((g, cont, side));
            }
        }
    }
    for (g, cont, side) in creased {
        if !grow.get(&g).is_some_and(|edges| edges.contains(&cont)) {
            return Err(KernelError::Invalid(format!(
                "extending the edge lengthens its face along edge {side}, a crease with the face next to it: pick that face's edge {cont} too"
            )));
        }
    }
    Ok(grow)
}

fn record_edge(r: &brep::Brep, e: usize) -> Result<&brep::Edge> {
    r.edges.get(e).ok_or_else(|| failed("topology", format!("no edge {e}")))
}

/// The outer loop of face `f` and where edge `e` is in it.
fn in_outer_loop(r: &brep::Brep, f: usize, e: usize) -> Result<(&[brep::OrientedEdge], usize)> {
    let face = r.faces.get(f).ok_or_else(|| failed("topology", format!("no face {f}")))?;
    let Some(brep::Loop::Edges { edges }) = face.loops.first() else { return Err(failed("topology", format!("face {f} has no edge loop"))) };
    let i = edges.iter().position(|u| u.edge as usize == e).ok_or_else(|| failed("topology", format!("edge {e} is not on face {f}'s outer loop")))?;
    Ok((edges, i))
}

/// The vertices edge use `u` runs from and to round its loop.
fn ends(r: &brep::Brep, u: &brep::OrientedEdge) -> Result<[u32; 2]> {
    let [a, b] = record_edge(r, u.edge as usize)?.vertices;
    Ok(if u.forward { [a, b] } else { [b, a] })
}

/// The sides of face `f` next to its edge `e` round its outer loop, each with the vertex it
/// shares with `e`: the edges an extension past `e` lengthens.
fn next_to(r: &brep::Brep, f: usize, e: usize) -> Result<Vec<(usize, u32)>> {
    let (edges, i) = in_outer_loop(r, f, e)?;
    let n = edges.len();
    let (Some(prev), Some(this), Some(next)) = (edges.get((i + n - 1) % n), edges.get(i), edges.get((i + 1) % n)) else { return Ok(Vec::new()) };
    let [from, to] = ends(r, this)?;
    let mut out = Vec::with_capacity(2);
    for (u, v) in [(prev, from), (next, to)] {
        if u.edge as usize != e && !out.iter().any(|(s, _)| *s == u.edge as usize) {
            out.push((u.edge as usize, v));
        }
    }
    Ok(out)
}

/// The edge of face `g`'s outer loop that meets its side `side` at vertex `at`.
fn continuing(r: &brep::Brep, g: usize, side: usize, at: u32) -> Result<usize> {
    let (edges, i) = in_outer_loop(r, g, side)?;
    let n = edges.len();
    let this = edges.get(i).ok_or_else(|| failed("topology", "a loop position"))?;
    let [from, to] = ends(r, this)?;
    let k = if from == at {
        (i + n - 1) % n
    } else if to == at {
        (i + 1) % n
    } else {
        return Err(failed("topology", format!("edge {side} does not end at vertex {at}")));
    };
    match edges.get(k) {
        Some(u) if u.edge as usize != side => Ok(u.edge as usize),
        _ => Err(KernelError::Invalid(format!("not supported yet: extending along edge {side}, the only side of the face next to it"))),
    }
}

/// The point halfway along record edge `e`.
fn edge_middle(r: &brep::Brep, e: usize) -> Result<Vec3> {
    let edge = record_edge(r, e)?;
    let c = CurveEval::new(edge.parameters()).map_err(|err| failed("edge", format!("edge {e}'s curve: {err}")))?;
    c.point(0.5 * (edge.interval[0] + edge.interval[1])).ok_or_else(|| failed("edge", format!("edge {e} has no middle")))
}

/// The one face of `body` continued `distance` past each picked edge, as `mode` says.
fn one_face(body: &Body, at: &[Vec3], distance: f64, mode: ExtendMode) -> Result<Body> {
    let how = match mode {
        ExtendMode::Natural => Continuation::Natural,
        ExtendMode::Tangent => Continuation::Tangent,
        ExtendMode::Curvature => Continuation::Curvature,
        ExtendMode::Law => return Err(failed("extend", "a law extension is built from its edges, not by SMLib's continuation")),
    };
    let reach = body.size() * 0.05 + 1e-6;
    let (sb, ob) = smlib_body_ordered(body)?;
    let mut sheet = sb.tagged(0).map_err(|e| failed("tag", e))?;
    for p in at {
        sheet = sheet.extend_sheet([p.x, p.y, p.z], reach, distance, how).map_err(|e| match e {
            solidcraft_smlib::Error::Unsupported => KernelError::Invalid(
                "not supported yet: extending an edge that is not a whole side of its surface's parameter box (a trimmed edge)".into(),
            ),
            solidcraft_smlib::Error::NotExact => KernelError::Invalid(format!(
                "the surface cannot be extended exactly {distance} mm there: the extension would run past a pole of its surface, wrap round it, or not reach that length"
            )),
            solidcraft_smlib::Error::NoMatch { .. } => KernelError::Invalid(format!("no edge at {p}")),
            e => failed("extend", e),
        })?;
    }
    let (out, order) = body_from_smlib_ordered(&sheet)?;
    let history = match (ob, order) {
        (Some(ob), Some(or)) => sheet.history().map_err(|e| failed("history", e))?.history(&[ob], &or).ok().map(|mut h| {
            // The extended face is the face it was, bounded differently.
            for f in &mut h.faces {
                if f.origins.contains(&FaceOrigin::Face { operand: 0, face: 0 }) {
                    f.change = Change::Modified;
                }
            }
            h
        }),
        _ => None,
    };
    Ok(out.with_checked_history(history, &crate::record::counts(&[body])))
}

/// The faces extended past the target and trimmed back where the target meets them, keeping the
/// pieces the faces themselves lie in.
fn up_to(body: &Body, at: &[Vec3], picked: &[(usize, usize)], tool: &SplitTool, mode: ExtendMode) -> Result<Body> {
    let (long, faces) = continued(body, at, picked, body.size().max(1e-6) * REACH, mode)?;
    let inside = faces.iter().map(|f| crate::surface_untrim::face_point(body, *f)).collect::<Result<Vec<_>>>()?;
    let cut = crate::trim_keep(&long, tool, TrimKeep::Regions(&inside))?;
    Ok(crate::record::after(cut, &long, &crate::record::counts(&[body])))
}

/// How far past its edges (picked at `at`) `body` extends to reach past all of `target`: its own
/// reach ([`REACH`]), or past the farthest corner of the target's box from a pick, with room for the
/// body's other points, when that is farther.
fn reach_of(body: &Body, at: &[Vec3], target: &Body) -> Result<f64> {
    let size = body.size().max(1e-6);
    let own = size * REACH;
    // A target with no solidcore form (a mesh) is refused where it would cut.
    let (Some(p), Some(form)) = (at.first(), target.ops_body()) else { return Ok(own) };
    let (lo, hi) = form.0.bounds().map_err(|e| failed("target bounds", e.to_string()))?;
    let farthest =
        solidcore_ops::measure::box_max_distance_to_point(lo, hi, [p.x, p.y, p.z]).map_err(|e| failed("target distance", e.to_string()))?;
    Ok(own.max(farthest + 2.0 * size))
}

/// The faces extended past the target body and cut where they first run into it, keeping the
/// pieces the surface's own faces lie in.
fn up_to_body(body: &Body, at: &[Vec3], picked: &[(usize, usize)], target: &Body, mode: ExtendMode) -> Result<Body> {
    let (long, _) = continued(body, at, picked, reach_of(body, at, target)?, mode)?;
    let inside = (0..body.face_count()).map(|f| crate::surface_untrim::face_point(body, f)).collect::<Result<Vec<_>>>()?;
    let cut = crate::surface_trim::stop_extension(&long, body, &inside, target)?;
    Ok(crate::record::after(cut, &long, &crate::record::counts(&[body])))
}

/// The faces of the picked edges back on their surfaces' bounds: each untrimmed in place, its
/// holes kept. One already there is refused.
fn back_to_bounds(body: &Body, faces: &[usize]) -> Result<Body> {
    let [face] = faces else {
        return Err(KernelError::Invalid("not supported yet: going back to the bounds of more than one face at a time".into()));
    };
    let before = crate::measure(body)?.area;
    let back = crate::untrim(body, *face, crate::UntrimHoles::At(&[]), true, true)?;
    let after = crate::measure(&back)?.area;
    if (after - before).abs() <= 1e-9 * before.abs().max(1e-12) {
        return Err(KernelError::Invalid("the edge already lies on its surface's bounds".into()));
    }
    Ok(back)
}

// ---- Law extension -------------------------------------------------------------------------
//
// The strip is built from the picked edges' own curves, never from the source surface's
// parameter box: the ruled surface on the chain of picked edges ([`crate::surface_ruled`]), each
// ruling leaving its edge square to it at `angle` to the surface (in the plane of the face's
// normal and the face's outward direction across the edge), `distance` long. On a straight edge of
// a plane the strip is a plane; on an arc whose rulings keep their angle to its axis, a cylinder
// or cone; elsewhere the exact ruled surface to a fair far curve. The strip is then sewn to the
// source along the picked edges: the source's faces are kept as they are, the strip's faces are
// made from the edges they stand on.

/// The ruled settings of a law extension at `angle` (0: tangent) reaching `distance`: the ruled
/// surface measures its angle from the face's normal, turned away from the face
/// ([`solidcore_ops::ruled_strip::ruling_angle`]).
fn law_rulings(angle: f64, distance: f64) -> Result<crate::surface_ruled::Ruled> {
    use crate::surface_ruled::{Ruled, RuledCorners, RuledMode};
    let angle = ruling_angle(angle).ok_or_else(|| KernelError::Invalid("the law extension's angle must lie between -90° and 90°".into()))?;
    let mode = RuledMode::Angle { angle, reference: None };
    Ok(Ruled { distance, mode, flip: false, corners: RuledCorners::Mitre, face: None })
}

/// The law strip alone on the edges `at` pick (one chain), as its own surface body whose history
/// is over `[body]`; a strip that would fold over itself is refused. Where the rulings turn
/// freely along an edge, the far edge is a fair curve of the ruled surface, refined until it
/// passes within [`far_tolerance`] of the rulings' ends.
fn law_strip(body: &Body, at: &[Vec3], angle: f64, distance: f64) -> Result<Body> {
    let r = law_rulings(angle, distance)?;
    let chain = |e: KernelError| match e {
        KernelError::Invalid(m) if m.contains("chain") || m.contains("branch") || m.contains("loop") => {
            KernelError::Invalid(format!("a law extension takes one chain of edges: {m}"))
        }
        e => e,
    };
    let tol = far_tolerance(body.size(), distance);
    crate::surface_ruled::ruled_surface_close(body, at, &r, tol).map_err(chain)
}

/// `strip` sewn to `body` along the `count` edges it stands on: one surface body whose history is
/// over `[body]`, the body's faces kept, the strip's faces as its own history says. A strip that
/// does not sew along every picked edge is refused.
fn sewn_to(body: &Body, strip: &Body, count: usize) -> Result<Body> {
    let r = body.record();
    let all: Vec<(usize, bool)> = (0..r.faces.len()).map(|f| (f, false)).collect();
    let c = Parts::of(r, &all)?.canonical()?;
    let kept = c
        .faces
        .iter()
        .map(|p| all.get(*p).map(|(f, _)| record::from_face(0, *f, Change::Kept)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| failed("sew", "a face of the surface is missing"))?;
    let made = strip.history().map(|h| h.faces.clone()).ok_or_else(|| failed("sew", "the strip has no history"))?;
    let out = crate::surfaces::sheet_of(vec![(c.brep, kept), (strip.record().clone(), made)], true, &[body])?;
    if out.face_count() != body.face_count() + strip.face_count() || out.edge_count() + count != body.edge_count() + strip.edge_count() {
        return Err(KernelError::Invalid("the extension can't be sewn to the surface along the picked edges".into()));
    }
    Ok(out)
}

/// The law extension of the picked edges (`count` of them, picked by `at`) by `distance`.
fn law(body: &Body, at: &[Vec3], count: usize, angle: f64, distance: f64) -> Result<Body> {
    let strip = law_strip(body, at, angle, distance)?;
    sewn_to(body, &strip, count)
}

/// The law extension of the picked `edges` (of `record`, picked by `at`) up to a plane or face:
/// the strip built long ([`REACH`] body sizes), cut where the target meets it keeping the piece
/// on the edges, and sewn to the surface. The surface itself is never cut. A strip the target
/// misses, wholly or in part (the piece kept still holds part of the strip's far edge), is
/// refused. Where the long strip folds over itself (or its corners cut one of its strips away),
/// a shorter one is built
/// ([`solidcore_ops::ruled_strip::up_to_target`] picks the lengths): only a target the strip
/// reaches before it folds is accepted; one past the fold (or so near it the strip can't be
/// built) is refused as a fold.
fn law_up_to(body: &Body, at: &[Vec3], record: &brep::Brep, edges: &BTreeSet<usize>, angle: f64, tool: &SplitTool) -> Result<Body> {
    let reach = body.size().max(1e-6) * REACH;
    let on_edges = edges.iter().map(|e| edge_middle(record, *e)).collect::<Result<Vec<_>>>()?;
    let missed = || KernelError::Invalid("the extension never reaches the face it extends up to (all along the edges)".into());
    // A strip too long for its corners (the mitres meet and cut a strip away) is shortened as a
    // folding one is: a shorter strip keeps that strip.
    let folds = |e: &KernelError| matches!(e, KernelError::Invalid(m) if m.contains("fold over itself") || m.contains("cut a strip away entirely"));
    // The strip `length` long cut at the target, or `None` where the target misses it (wholly
    // or in part); `Err` with the fold's message where that strip folds.
    let attempt = |length: f64| -> Result<Option<Body>> {
        let long = law_strip(body, at, angle, length)?;
        let cut = match crate::trim_keep(&long, tool, TrimKeep::Regions(&on_edges)) {
            Ok(cut) => cut,
            Err(KernelError::Invalid(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        if keeps_far_edge(body, &long, &cut, far_tolerance(body.size(), length))? {
            return Ok(None);
        }
        Ok(Some(crate::record::after(cut, &long, &crate::record::counts(&[body]))))
    };
    let folded = || KernelError::Invalid("the strip would fold over itself before it reaches the face it extends up to: use a smaller angle".into());
    let cut = match solidcore_ops::ruled_strip::up_to_target(reach, attempt, folds)? {
        Reach::Cut(cut) => cut,
        Reach::Missed => return Err(missed()),
        Reach::Folded => return Err(folded()),
    };
    sewn_to(body, &cut, edges.len())
}

/// Whether `cut`, the law strip `long` cut at a target, still holds part of the strip's far edge
/// (the target missed the strip there): a point of a far edge (an edge of `long` with neither
/// end on the surface `body`) within `tol` of `cut`.
fn keeps_far_edge(body: &Body, long: &Body, cut: &Body, tol: f64) -> Result<bool> {
    for e in long.edges(tol)? {
        let (Some(a), Some(b)) = (e.points.first(), e.points.last()) else { continue };
        if body.face_at(*a, tol).is_some() || body.face_at(*b, tol).is_some() {
            continue;
        }
        if e.points.iter().chain([&e.mid]).any(|p| cut.face_at(*p, 2.0 * tol).is_some()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn failed(what: &str, e: impl crate::Cause) -> KernelError {
    KernelError::smlib(format!("SMLib extend: {what}"), e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{measure, patch_region};
    use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }
    fn area(b: &Body) -> f64 {
        measure(b).unwrap().area
    }
    fn with(mode: ExtendMode, limit: ExtendLimit, neighbours: bool) -> ExtendOptions {
        ExtendOptions { mode, limit, neighbours, angle: 0.0 }
    }
    fn natural(d: f64) -> ExtendOptions<'static> {
        with(ExtendMode::Natural, ExtendLimit::Distance(d), true)
    }
    fn edges(b: &Body) -> usize {
        b.edge_count()
    }
    fn square() -> Body {
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]),
            holes: vec![],
        };
        patch_region(&Plane::XY, &r).unwrap()
    }
    /// A strip of the cylinder of radius 10 about Z, heights 0…20, angles 0…`sweep`: the arc
    /// extruded, one face, its edges two generators and two arcs.
    fn cylinder_strip(sweep: f64) -> Body {
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep }];
        crate::extrude_sheet(&Plane::XY, &[arc], 0.0, 20.0).unwrap()
    }

    /// Arc length of z = x²/20 from 0 to x.
    fn parabola_arc(x: f64) -> f64 {
        0.5 * x * (1.0 + (x / 10.0).powi(2)).sqrt() + 5.0 * (x / 10.0).asinh()
    }
    /// The parabolic sheet z = x²/20 over 0 ≤ x, y ≤ 10: the parabola (a quadratic Bézier,
    /// sketched on the plane y = 0 whose sketch y is −z) extruded 10 along y.
    fn parabola() -> Body {
        let p = vec![Seg2::Conic { a: Vec2::new(0.0, 0.0), apex: Vec2::new(5.0, 0.0), b: Vec2::new(10.0, -5.0), w: 1.0 }];
        crate::extrude_sheet(&Plane::XZ, &[p], 0.0, 10.0).unwrap()
    }

    /// The default (natural, by a distance, neighbours lengthened) on a plane's straight edges
    /// is today's planar extension, exactly; the general path builds the same square.
    #[test]
    fn a_planar_extension_is_unchanged() {
        let s = square();
        let old = crate::extend(&s, &[Vec3::new(10.0, 5.0, 0.0)], 5.0).unwrap();
        let new = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &natural(5.0)).unwrap();
        assert_eq!(measure(&old).unwrap().area, measure(&new).unwrap().area);
        assert_eq!(edges(&old), edges(&new));
        // Up to the plane x = 15 takes the general path: the same 15 × 10, its lengthened sides
        // one edge each.
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(15.0, 0.0, 0.0), Vec3::X).unwrap());
        let up = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        assert!(rel(area(&up), 150.0) < 1e-9, "{}", area(&up));
        assert_eq!((up.face_count(), edges(&up)), (1, 4));
        // Up to x = 17: 170.
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(17.0, 0.0, 0.0), Vec3::X).unwrap());
        let up = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        assert!(rel(area(&up), 170.0) < 1e-9, "{}", area(&up));
        let bb = measure(&up).unwrap().bbox;
        assert!((bb.max.x - 17.0).abs() < 1e-9, "{bb:?}");
    }

    /// A cylinder strip (radius 10, a quarter turn, heights 0…20): extended 5 along its axis
    /// (its top arc picked), 10 · π/2 · 25; 5 around its circumference (a straight edge
    /// picked), 10 · (π/2 + ½) · 20; both, meeting at the box's corner, 10 · (π/2 + ½) · 25; up
    /// to the plane z = 30, 10 · π/2 · 30. Each stays one cylindrical face; the sides along the
    /// extension are the old edges lengthened.
    #[test]
    fn a_cylinder_strip_extended_along_its_axis_and_around() {
        let c = cylinder_strip(FRAC_PI_2);
        assert!(rel(area(&c), 100.0 * PI) < 1e-3);
        let top = Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0);
        let side = Vec3::new(0.0, 10.0, 10.0);
        let cylinder =
            |b: &Body| matches!(crate::face_surface(b, 0), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6);

        let up = extend_surface(&c, &[top], &natural(5.0)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 25.0;
        assert!(rel(area(&up), want) < 1e-3, "{} vs {want}", area(&up)); // (measured on meshes)
        assert!((measure(&up).unwrap().bbox.max.z - 25.0).abs() < 1e-6);
        assert!(up.face_count() == 1 && cylinder(&up) && edges(&up) == 4, "{}", edges(&up));

        let round = extend_surface(&c, &[side], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 20.0;
        assert!(rel(area(&round), want) < 1e-3, "{} vs {want}", area(&round)); // (measured on meshes)
        assert!(round.face_count() == 1 && cylinder(&round) && edges(&round) == 4);
        // The moved edge is at angle π/2 + ½ (5 mm of arc): x = −10 sin ½.
        assert!((measure(&round).unwrap().bbox.min.x + 10.0 * 0.5f64.sin()).abs() < 1e-6, "{:?}", measure(&round).unwrap().bbox);

        let both = extend_surface(&c, &[top, side], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 25.0;
        assert!(rel(area(&both), want) < 1e-3, "{} vs {want}", area(&both)); // (measured on meshes)

        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 30.0), Vec3::Z).unwrap());
        let to = extend_surface(&c, &[top], &with(ExtendMode::Natural, ExtendLimit::UpTo(&tool), true)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 30.0;
        assert!(rel(area(&to), want) < 1e-3, "{} vs {want}", area(&to)); // (measured on meshes)
        assert!((measure(&to).unwrap().bbox.max.z - 30.0).abs() < 1e-6);

        // The history: the face is the one it was, trimmed differently.
        let h = up.history().unwrap();
        assert_eq!(h.faces.first().map(|f| f.change), Some(crate::history::Change::Modified));
    }

    /// The same strip swept as a B-spline sheet (a rational arc extruded): its end spans
    /// continue as themselves, so the extension stays on the cylinder.
    #[test]
    fn an_extruded_cylinder_strip_continues_on_its_cylinder() {
        let sheet = cylinder_strip(FRAC_PI_2);
        let top = Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0);
        let up = extend_surface(&sheet, &[top], &natural(5.0)).unwrap();
        let want = 10.0 * FRAC_PI_2 * 25.0;
        assert!(rel(area(&up), want) < 1e-3, "{} vs {want}", area(&up)); // (measured on meshes)
        let round = extend_surface(&sheet, &[Vec3::new(0.0, 10.0, 10.0)], &natural(5.0)).unwrap();
        let want = 10.0 * (FRAC_PI_2 + 0.5) * 20.0;
        assert!(rel(area(&round), want) < 1e-3, "{} vs {want}", area(&round)); // (measured on meshes)
        assert!(matches!(crate::face_surface(&round, 0), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6));
    }

    /// The parabolic B-spline sheet z = x²/20 (0 ≤ x, y ≤ 10) extended 5 past its edge x = 10,
    /// naturally: the parabola itself continues, 5 mm of arc further (to x* where its arc
    /// length grows by 5): area 10 · (s(10) + 5), reaching z = x*²/20.
    #[test]
    fn a_bspline_extended_naturally() {
        let p = parabola();
        let base = 10.0 * parabola_arc(10.0);
        assert!(rel(area(&p), base) < 1e-4, "{} vs {base}", area(&p)); // (measured on meshes)
        let edge = [Vec3::new(10.0, 5.0, 5.0)];
        let nat = extend_surface(&p, &edge, &natural(5.0)).unwrap();
        let (mut lo, mut hi) = (10.0, 20.0);
        for _ in 0..100 {
            let m = 0.5 * (lo + hi);
            if parabola_arc(m) - parabola_arc(10.0) < 5.0 { lo = m } else { hi = m }
        }
        let x = 0.5 * (lo + hi);
        let bb = measure(&nat).unwrap().bbox;
        assert!((bb.max.x - x).abs() < 1e-6 && (bb.max.z - x * x / 20.0).abs() < 1e-6, "{bb:?} vs x* = {x}");
        assert!(rel(area(&nat), base + 50.0) < 1e-4, "{} vs {}", area(&nat), base + 50.0); // (measured on meshes)
        assert_eq!(nat.face_count(), 1);
    }

    /// Back to the surface's own bounds: the half saddle (a B-spline over 0…10 × 0…10, x from 0
    /// to 5) goes back to its whole patch; the half cylinder trimmed by a slanted plane (top
    /// from z = 15 down to 10 and back) goes back to its span, heights 0…15: 150π.
    #[test]
    fn extended_back_to_the_surfaces_bounds() {
        let front = (Plane::XZ, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }]);
        let back = (Plane { origin: Vec3::new(0.0, 10.0, 0.0), ..Plane::XZ }, vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, -10.0) }]);
        let saddle = crate::loft_sheet(&[front, back]).unwrap();
        let cut = SplitTool::Plane(Plane::from_normal(Vec3::new(5.0, 0.0, 0.0), Vec3::X).unwrap());
        let half = crate::trim(&saddle, &cut, Vec3::new(1.0, 5.0, 0.5)).unwrap();
        let bounds = with(ExtendMode::Natural, ExtendLimit::Bounds, true);
        let whole = extend_surface(&half, &[Vec3::new(5.0, 5.0, 2.5)], &bounds).unwrap();
        let bb = measure(&whole).unwrap().bbox;
        assert!((bb.max.x - 10.0).abs() < 1e-9 && (bb.max.z - 10.0).abs() < 1e-9, "{bb:?}");
        // Already there: refused.
        assert!(extend_surface(&whole, &[Vec3::new(10.0, 5.0, 5.0)], &bounds).is_err());

        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 10.0, start: 0.0, sweep: PI }];
        let sheet = crate::extrude_sheet(&Plane::XY, &[arc], 0.0, 20.0).unwrap();
        let tool = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), Vec3::new(0.0, 0.5, 1.0)).unwrap());
        let cut = crate::trim(&sheet, &tool, Vec3::new(0.0, 10.0, 1.0)).unwrap();
        let back = extend_surface(&cut, &[Vec3::new(0.0, 10.0, 10.0)], &bounds).unwrap();
        let a = area(&back);
        assert!(rel(a, 150.0 * PI) < 1e-3, "{a} vs {}", 150.0 * PI); // (measured on meshes)
        assert!((measure(&back).unwrap().bbox.max.z - 15.0).abs() < 1e-6);
    }

    /// Three cubic spans of a spline meeting tangent, extruded 0…20 along Z: a sheet of three
    /// faces, as a fit-point spline's surface is (#3830).
    fn spline_spans() -> Body {
        let v = Vec2::new;
        let c = |p0, p1, p2, p3| Seg2::Cubic { p0, p1, p2, p3 };
        let chain = vec![
            c(v(0.0, 0.0), v(5.0, 0.0), v(10.0, 5.0), v(15.0, 5.0)),
            c(v(15.0, 5.0), v(20.0, 5.0), v(25.0, 0.0), v(30.0, 0.0)),
            c(v(30.0, 0.0), v(35.0, 0.0), v(40.0, 5.0), v(45.0, 5.0)),
        ];
        crate::extrude_sheet(&Plane::XY, &[chain], 0.0, 20.0).unwrap()
    }

    /// A sheet of several faces grows face by face (#3830): its end edge (the last span's
    /// generator) 10 further, the last span continued, 10 · 20 more; the middle span's top edge
    /// 5 up takes its tangent neighbours with it, the whole spline 25 tall. The faces stay three,
    /// sewn as before, each the face it was.
    #[test]
    fn a_sheet_of_tangent_faces_grows_face_by_face() {
        let s = spline_spans();
        assert_eq!(s.face_count(), 3);
        let base = area(&s);
        let end = extend_surface(&s, &[Vec3::new(45.0, 5.0, 10.0)], &natural(10.0)).unwrap();
        assert!(rel(area(&end), base + 200.0) < 1e-4, "{} vs {}", area(&end), base + 200.0); // (measured on meshes)
        assert_eq!((end.face_count(), edges(&end)), (3, edges(&s)));
        assert!(measure(&end).unwrap().bbox.max.x > 50.0, "{:?}", measure(&end).unwrap().bbox);
        let h = end.history().unwrap();
        let changes: Vec<Change> = h.faces.iter().map(|f| f.change).collect();
        assert_eq!(changes.iter().filter(|c| **c == Change::Modified).count(), 1, "{changes:?}");

        let up = extend_surface(&s, &[Vec3::new(22.5, 2.5, 20.0)], &natural(5.0)).unwrap();
        assert!(rel(area(&up), base * 25.0 / 20.0) < 1e-4, "{} vs {}", area(&up), base * 25.0 / 20.0); // (measured on meshes)
        assert_eq!((up.face_count(), edges(&up)), (3, edges(&s)));
        assert!((measure(&up).unwrap().bbox.max.z - 25.0).abs() < 1e-6);
    }

    /// Across a crease the face next door grows only when its edge is picked too: an L of two
    /// planes, one top edge refused (it would run past the other wall), both 5 up, 25 · 20.
    #[test]
    fn a_crease_grows_only_when_both_edges_are_picked() {
        let l =
            vec![Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(10.0, 0.0) }, Seg2::Line { a: Vec2::new(10.0, 0.0), b: Vec2::new(10.0, 10.0) }];
        let s = crate::extrude_sheet(&Plane::XY, &[l], 0.0, 20.0).unwrap();
        assert_eq!(s.face_count(), 2);
        let e = extend_surface(&s, &[Vec3::new(5.0, 0.0, 20.0)], &natural(5.0)).unwrap_err();
        assert!(e.to_string().contains("crease"), "{e}");
        let both = extend_surface(&s, &[Vec3::new(5.0, 0.0, 20.0), Vec3::new(10.0, 5.0, 20.0)], &natural(5.0)).unwrap();
        assert!(rel(area(&both), 500.0) < 1e-9, "{}", area(&both));
        assert_eq!((both.face_count(), edges(&both)), (2, edges(&s)));
    }

    /// Up to a whole body: a square sheet extended in x under a stair of two risers, x = 15 for
    /// y < 5 and x = 18 for y > 5, stops on each: 5 * 15 + 5 * 18 = 165. Up to a wall over half of
    /// the edge it is refused, and so are a tangent extension and a body through the sheet.
    #[test]
    fn extends_up_to_every_face_of_a_whole_body() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        let wall = |points: &[[f64; 2]]| {
            let v = |p: &[f64; 2]| Vec2::new(p[0], p[1]);
            let chain = points.windows(2).map(|w| Seg2::Line { a: v(&w[0]), b: v(&w[1]) }).collect::<Vec<_>>();
            crate::extrude_sheet(&Plane::XY, &[chain], -5.0, 5.0).unwrap()
        };
        let to = |body: &Body, mode| extend_surface(&s, &at, &with(mode, ExtendLimit::UpToBody(body), true));
        let stair = wall(&[[15.0, -1.0], [15.0, 5.0], [18.0, 5.0], [18.0, 11.0]]);
        let out = to(&stair, ExtendMode::Natural).unwrap();
        assert!(rel(area(&out), 165.0) < 1e-9, "{}", area(&out));
        let bb = measure(&out).unwrap().bbox;
        assert!(bb.min.x.abs() < 1e-9 && (bb.max.x - 18.0).abs() < 1e-9, "{bb:?}");
        // A wall far past 50 times the sheet's size is reached: 10 * 1000.
        let distant = wall(&[[1000.0, -1.0], [1000.0, 11.0]]);
        let out = to(&distant, ExtendMode::Natural).unwrap();
        assert!(rel(area(&out), 10_000.0) < 1e-9, "{}", area(&out));
        let half = wall(&[[15.0, -1.0], [15.0, 5.0]]);
        let e = to(&half, ExtendMode::Natural).unwrap_err();
        assert!(e.to_string().contains("never meets"), "{e}");
        let through = wall(&[[5.0, -1.0], [5.0, 11.0]]);
        assert!(to(&through, ExtendMode::Natural).is_err());
        assert!(to(&stair, ExtendMode::Tangent).is_err());
    }

    fn law(angle_deg: f64, limit: ExtendLimit) -> ExtendOptions {
        ExtendOptions { mode: ExtendMode::Law, limit, neighbours: true, angle: angle_deg.to_radians() }
    }

    /// Law at 0° off the square's edge x = 10, 5 long: the natural extension's 15 × 10 (area
    /// 150, box 0…15 × 0…10 in z = 0), as the square and a 10 × 5 strip sewn along the edge (two
    /// faces, 4 + 4 − 1 edges). The square is kept as it was; the strip is made from the edge.
    #[test]
    fn a_law_extension_at_zero_is_the_natural_one() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        let nat = extend_surface(&s, &at, &natural(5.0)).unwrap();
        let out = extend_surface(&s, &at, &law(0.0, ExtendLimit::Distance(5.0))).unwrap();
        assert!(rel(area(&out), 150.0) < 1e-9, "{}", area(&out));
        assert!(rel(area(&out), area(&nat)) < 1e-9);
        let (bb, nb) = (measure(&out).unwrap().bbox, measure(&nat).unwrap().bbox);
        assert!(bb.min.dist(nb.min) < 1e-9 && bb.max.dist(nb.max) < 1e-9, "{bb:?} vs {nb:?}");
        assert!(bb.max.dist(Vec3::new(15.0, 10.0, 0.0)) < 1e-9, "{bb:?}");
        assert_eq!((out.face_count(), edges(&out)), (2, 7));
        let h = out.history().unwrap();
        let kept = h.faces.iter().filter(|f| f.change == Change::Kept && f.origins == [FaceOrigin::Face { operand: 0, face: 0 }]).count();
        let made =
            h.faces.iter().filter(|f| f.change == Change::Generated && matches!(f.origins.as_slice(), [FaceOrigin::Edge { operand: 0, .. }])).count();
        assert_eq!((kept, made), (1, 1), "{h:?}");
    }

    /// At 30° the strip leaves the edge tilted 30° toward the square's normal (+z): its far
    /// edge at x = 10 + 5 cos 30°, z = 5 sin 30° = 2.5, its area still 10 × 5 (100 + 50). At
    /// −30° it tilts the other way, down to z = −2.5.
    #[test]
    fn a_law_extension_on_a_plane_at_30_degrees() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        for (deg, z) in [(30.0, 2.5), (-30.0, -2.5)] {
            let out = extend_surface(&s, &at, &law(deg, ExtendLimit::Distance(5.0))).unwrap();
            assert!(rel(area(&out), 150.0) < 1e-9, "{deg}: {}", area(&out));
            let bb = measure(&out).unwrap().bbox;
            let x = 10.0 + 5.0 * 30f64.to_radians().cos();
            assert!((bb.max.x - x).abs() < 1e-9 && (bb.max.y - 10.0).abs() < 1e-9, "{deg}: {bb:?}");
            assert!((bb.max.z.max(0.0) + bb.min.z.min(0.0) - z).abs() < 1e-9 && (bb.max.z - bb.min.z - 2.5).abs() < 1e-9, "{deg}: {bb:?}");
            assert_eq!(out.face_count(), 2);
        }
    }

    /// The quarter cylinder strip (radius 10, heights 0…20, area 100π), its top arc extended 5:
    /// at 0° the cylinder continued, 10 · π/2 · 25 = 125π, up to z = 25, the strip a cylinder;
    /// at 30° (toward the outward normal) a quarter of the cone frustum from radius 10 to
    /// 10 + 5 sin 30° = 12.5 with slant 5, π/4 · (10 + 12.5) · 5 more, up to z = 20 + 5 cos 30°.
    /// A straight edge (the generator at angle 0) 5 out at 0°: a 20 × 5 plane, 100π + 100.
    #[test]
    fn a_law_extension_off_a_cylinder() {
        let c = cylinder_strip(FRAC_PI_2);
        let top = [Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0)];
        let up = extend_surface(&c, &top, &law(0.0, ExtendLimit::Distance(5.0))).unwrap();
        assert!(rel(area(&up), 125.0 * PI) < 1e-3, "{} vs {}", area(&up), 125.0 * PI); // (measured on meshes)
        assert!((measure(&up).unwrap().bbox.max.z - 25.0).abs() < 1e-6);
        assert!(
            matches!(crate::face_surface(&up, 1), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6)
                || matches!(crate::face_surface(&up, 0), Some(crate::FaceSurface::Cylinder { radius, .. }) if (radius - 10.0).abs() < 1e-6)
        );
        assert_eq!(up.face_count(), 2);

        let cone = extend_surface(&c, &top, &law(30.0, ExtendLimit::Distance(5.0))).unwrap();
        let want = 100.0 * PI + FRAC_PI_4 * 22.5 * 5.0;
        assert!(rel(area(&cone), want) < 1e-3, "{} vs {want}", area(&cone)); // (measured on meshes)
        let bb = measure(&cone).unwrap().bbox;
        assert!((bb.max.z - (20.0 + 5.0 * 30f64.to_radians().cos())).abs() < 1e-6, "{bb:?}");
        assert!((bb.max.x - 12.5).abs() < 1e-6 && (bb.max.y - 12.5).abs() < 1e-6, "{bb:?}");
        let half = (0..2).find_map(|f| match crate::face_surface(&cone, f) {
            Some(crate::FaceSurface::Cone { half_angle, .. }) => Some(half_angle),
            _ => None,
        });
        assert!(half.is_some_and(|h| (h - 30f64.to_radians()).abs() < 1e-6), "{half:?}");

        let side = extend_surface(&c, &[Vec3::new(10.0, 0.0, 10.0)], &law(0.0, ExtendLimit::Distance(5.0))).unwrap();
        assert!(rel(area(&side), 100.0 * PI + 100.0) < 1e-3, "{}", area(&side)); // (measured on meshes)
        assert!((measure(&side).unwrap().bbox.min.y + 5.0).abs() < 1e-6, "{:?}", measure(&side).unwrap().bbox);
    }

    /// A flat sheet trimmed by a sketch arc and offset (a Surface Offset sheet): the square 0…10
    /// cut to the quarter disc of radius 8, offset 2 (z = ±2). The law extension builds from the
    /// arc: at 0° the quarter ring from radius 8 to 10 (the quarter disc of radius 10, 25π); at
    /// 30° a quarter of the cone frustum from radius 8 to 8 + 2 cos 30°, slant 2,
    /// π/4 · (16 + 2 cos 30°) · 2 more, rising 2 sin 30° = 1.
    #[test]
    fn a_law_extension_of_an_offset_sheets_arc() {
        let s = square();
        let arc = vec![Seg2::Arc { center: Vec2::new(0.0, 0.0), radius: 8.0, start: 0.0, sweep: FRAC_PI_2 }];
        let cut = SplitTool::Curves { plane: Plane::XY, curves: vec![arc], along: None };
        let disc = crate::trim(&s, &cut, Vec3::new(1.0, 1.0, 0.0)).unwrap();
        assert!(rel(area(&disc), 16.0 * PI) < 1e-6, "{}", area(&disc));
        let off = crate::offset_surface(&disc, &[0], 2.0, 1e-3).unwrap();
        let bb = measure(&off).unwrap().bbox;
        assert!((bb.max.z.abs() - 2.0).abs() < 1e-9 && (bb.max.x - 8.0).abs() < 1e-6, "{bb:?}");
        let z = bb.max.z;
        let rim = [Vec3::new(8.0 * FRAC_PI_4.cos(), 8.0 * FRAC_PI_4.sin(), z)];
        let flat = extend_surface(&off, &rim, &law(0.0, ExtendLimit::Distance(2.0))).unwrap();
        assert!(rel(area(&flat), 25.0 * PI) < 1e-3, "{} vs {}", area(&flat), 25.0 * PI); // (measured on meshes)
        let bb = measure(&flat).unwrap().bbox;
        assert!((bb.max.x - 10.0).abs() < 1e-6 && (bb.max.y - 10.0).abs() < 1e-6 && (bb.max.z - bb.min.z).abs() < 1e-9, "{bb:?}");
        assert_eq!(flat.face_count(), 2);
        let tilted = extend_surface(&off, &rim, &law(30.0, ExtendLimit::Distance(2.0))).unwrap();
        let c30 = 30f64.to_radians().cos();
        let want = 16.0 * PI + FRAC_PI_4 * (16.0 + 2.0 * c30) * 2.0;
        assert!(rel(area(&tilted), want) < 1e-3, "{} vs {want}", area(&tilted)); // (measured on meshes)
        let bb = measure(&tilted).unwrap().bbox;
        assert!((bb.max.x - (8.0 + 2.0 * c30)).abs() < 1e-6 && (bb.max.z - bb.min.z - 1.0).abs() < 1e-6, "{bb:?}");
    }

    /// A trimmed edge of a curved Surface Offset sheet, the #17384 case: the quarter cylinder
    /// strip (radius 10) cut by the slanted plane z = 15 − y/2 and offset 2 outward. The offset is
    /// the cylinder of radius 12 under the edge C(φ) = (12 cos φ, 12 sin φ, 15 − 5 sin φ),
    /// 0 ≤ φ ≤ π/2, area ∫ 12 (15 − 5 sin φ) dφ = 90π − 60. The edge is no side of the surface's
    /// box: the other modes refuse it. The law extension at 0°, 3 long, is the ruled surface
    /// C(φ) + v D(φ), 0 ≤ v ≤ 3, D the unit direction tangent to the cylinder, square to the edge
    /// and away from the face; its area ∫∫ |C' × D + v D' × D| dv dφ is integrated here from those
    /// formulas (the far edge is the kernel's fair curve through the rulings' ends, so 1e-3).
    #[test]
    fn a_law_extension_of_a_trimmed_curved_edge() {
        let c = cylinder_strip(FRAC_PI_2);
        let cut = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 15.0), Vec3::new(0.0, 0.5, 1.0)).unwrap());
        let trimmed = crate::trim(&c, &cut, Vec3::new(7.0, 7.0, 1.0)).unwrap();
        let off = crate::offset_surface(&trimmed, &[0], 2.0, 1e-3).unwrap();
        let base = 90.0 * PI - 60.0;
        assert!(rel(area(&off), base) < 1e-3, "{} vs {base}", area(&off)); // (measured on meshes)
        let on_edge = |phi: f64| Vec3::new(12.0 * phi.cos(), 12.0 * phi.sin(), 15.0 - 5.0 * phi.sin());
        let at = [on_edge(FRAC_PI_4)];
        for mode in [ExtendMode::Natural, ExtendMode::Tangent, ExtendMode::Curvature] {
            let e = extend_surface(&off, &at, &with(mode, ExtendLimit::Distance(3.0), true)).unwrap_err();
            assert!(e.to_string().contains("trimmed"), "{mode:?}: {e}");
        }
        let out = extend_surface(&off, &at, &law(0.0, ExtendLimit::Distance(3.0))).unwrap();
        // The strip by its formulas.
        let dc = |phi: f64| Vec3::new(-12.0 * phi.sin(), 12.0 * phi.cos(), -5.0 * phi.cos());
        let dir = |phi: f64| {
            let n = Vec3::new(phi.cos(), phi.sin(), 0.0);
            let d = n.cross(dc(phi)).normalized().unwrap();
            if d.z < 0.0 { d * -1.0 } else { d }
        };
        let (m, k) = (4000, 60);
        let mut strip = 0.0;
        for i in 0..m {
            let phi = FRAC_PI_2 * (i as f64 + 0.5) / m as f64;
            let h = 1e-6;
            let (d, dd) = (dir(phi), (dir(phi + h) - dir(phi - h)) * (0.5 / h));
            for j in 0..k {
                let v = 3.0 * (j as f64 + 0.5) / k as f64;
                strip += (dc(phi).cross(d) + dd.cross(d) * v).len() * (FRAC_PI_2 / m as f64) * (3.0 / k as f64);
            }
        }
        let worst = (0..=64)
            .map(|i| FRAC_PI_2 * i as f64 / 64.0)
            .map(|phi| out.nearest_edge(on_edge(phi) + dir(phi) * 3.0, 1e-4).unwrap().unwrap().1)
            .fold(0.0, f64::max);
        // The far edge follows the rulings' ends (to the law tolerance, 1e-5 of size + length).
        assert!(worst < 1e-5 * (off.size() + 3.0) + 1e-4, "{worst}");
        assert!(rel(area(&out), base + strip) < 1e-3, "{} vs {} (strip {strip})", area(&out), base + strip); // (measured on meshes)
        // The strip's corner over φ = 0: the edge's end (12, 0, 15) plus 3 D(0) = 3 (0, 5, 12) / 13.
        let top = measure(&out).unwrap().bbox.max.z;
        let want = (0..=1000).map(|i| FRAC_PI_2 * i as f64 / 1000.0).map(|phi| on_edge(phi).z + 3.0 * dir(phi).z).fold(f64::MIN, f64::max);
        assert!((want - (15.0 + 36.0 / 13.0)).abs() < 1e-9, "{want}");
        assert!((top - want).abs() < 1e-3, "{top} vs {want}");
        assert_eq!(out.face_count(), 2);
    }

    /// Up to a target: at 0° up to the plane x = 17, the strip cut there: 170, to x = 17; at 30°
    /// up to the plane z = 2, the strip 2 / sin 30° = 4 long: 100 + 40; up to the face x = 13
    /// of a box standing across the edge: 130. The square itself is never cut, even where the
    /// target crosses it.
    #[test]
    fn a_law_extension_up_to_a_face() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        let plane = |o: Vec3, n: Vec3| SplitTool::Plane(Plane::from_normal(o, n).unwrap());
        let x17 = plane(Vec3::new(17.0, 0.0, 0.0), Vec3::X);
        let out = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&x17))).unwrap();
        assert!(rel(area(&out), 170.0) < 1e-9, "{}", area(&out));
        assert!((measure(&out).unwrap().bbox.max.x - 17.0).abs() < 1e-9);
        let z2 = plane(Vec3::new(0.0, 0.0, 2.0), Vec3::Z);
        let out = extend_surface(&s, &at, &law(30.0, ExtendLimit::UpTo(&z2))).unwrap();
        assert!(rel(area(&out), 140.0) < 1e-9, "{}", area(&out));
        let bb = measure(&out).unwrap().bbox;
        assert!((bb.max.z - 2.0).abs() < 1e-9 && (bb.max.x - (10.0 + 4.0 * 30f64.to_radians().cos())).abs() < 1e-9, "{bb:?}");
        let wall = crate::box_solid(Vec3::new(13.0, -5.0, -5.0), Vec3::new(20.0, 15.0, 5.0)).unwrap();
        let face = crate::surfaces::nearest_face(&wall, Vec3::new(13.0, 5.0, 0.0)).unwrap();
        let tool = SplitTool::Face { body: &wall, face };
        let out = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&tool))).unwrap();
        assert!(rel(area(&out), 130.0) < 1e-9, "{}", area(&out));
        // The plane x − 3z = 5 crosses the square (along x = 5) and the strip at 30° (where
        // 10 + t cos 30° − 3 t sin 30° = 5, t = 5 / (1.5 − cos 30°)): only the strip is cut, the
        // square stays whole: 100 + 10 t.
        let slant = plane(Vec3::new(5.0, 0.0, 0.0), Vec3::new(1.0, 0.0, -3.0));
        let out = extend_surface(&s, &at, &law(30.0, ExtendLimit::UpTo(&slant))).unwrap();
        let t = 5.0 / (1.5 - 30f64.to_radians().cos());
        assert!(rel(area(&out), 100.0 + 10.0 * t) < 1e-9, "{} vs {}", area(&out), 100.0 + 10.0 * t);
        assert!(measure(&out).unwrap().bbox.min.x.abs() < 1e-9);
    }

    /// Up to sketch curves (a sketch line at x = 17 on the square's plane) a law extension is
    /// refused before any strip is built; the plane x = 17 (170) and the box face x = 13 (130)
    /// still extend.
    #[test]
    fn a_law_extension_up_to_sketch_curves_is_refused() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        let line = vec![Seg2::Line { a: Vec2::new(17.0, -5.0), b: Vec2::new(17.0, 15.0) }];
        let curves = SplitTool::Curves { plane: Plane::XY, curves: vec![line], along: None };
        let e = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&curves))).unwrap_err();
        assert!(e.to_string().contains("not supported yet: a law extension up to sketch curves"), "{e}");
        let x17 = SplitTool::Plane(Plane::from_normal(Vec3::new(17.0, 0.0, 0.0), Vec3::X).unwrap());
        let out = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&x17))).unwrap();
        assert!(rel(area(&out), 170.0) < 1e-9, "{}", area(&out));
        let wall = crate::box_solid(Vec3::new(13.0, -5.0, -5.0), Vec3::new(20.0, 15.0, 5.0)).unwrap();
        let face = crate::surfaces::nearest_face(&wall, Vec3::new(13.0, 5.0, 0.0)).unwrap();
        let tool = SplitTool::Face { body: &wall, face };
        let out = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&tool))).unwrap();
        assert!(rel(area(&out), 130.0) < 1e-9, "{}", area(&out));
    }

    /// Up to a target the long strip would fold before or reach only far out: the cylinder strip's
    /// top arc leaning 60° in folds 10 / sin 60° = 11.547 out (its rulings meet on the axis); up
    /// to the plane z = 22 its rulings are 2 / cos 60° = 4 long, a quarter of the cone frustum from
    /// radius 10 to 10 − 4 sin 60° = 10 − 2√3 with slant 4, π/4 · (20 − 2√3) · 4 more: π (120 − 2√3).
    /// Up to z = 26 (rulings 12 long, past the fold) it is refused as a fold. The square at 0° up to
    /// the plane x = 500 (far out, but short of the long strip's end): 10 × 500 = 5000.
    #[test]
    fn a_law_extension_up_to_a_target_short_of_a_fold_or_far_out() {
        let c = cylinder_strip(FRAC_PI_2);
        let top = [Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0)];
        let plane = |o: Vec3, n: Vec3| SplitTool::Plane(Plane::from_normal(o, n).unwrap());
        let z22 = plane(Vec3::new(0.0, 0.0, 22.0), Vec3::Z);
        let out = extend_surface(&c, &top, &law(-60.0, ExtendLimit::UpTo(&z22))).unwrap();
        let want = PI * (120.0 - 2.0 * 3f64.sqrt());
        assert!(rel(area(&out), want) < 1e-3, "{} vs {want}", area(&out)); // (measured on meshes)
        assert!((measure(&out).unwrap().bbox.max.z - 22.0).abs() < 1e-6, "{:?}", measure(&out).unwrap().bbox);
        let z26 = plane(Vec3::new(0.0, 0.0, 26.0), Vec3::Z);
        let e = extend_surface(&c, &top, &law(-60.0, ExtendLimit::UpTo(&z26))).unwrap_err();
        assert!(e.to_string().contains("fold"), "{e}");
        let s = square();
        let x500 = plane(Vec3::new(500.0, 0.0, 0.0), Vec3::X);
        let out = extend_surface(&s, &[Vec3::new(10.0, 5.0, 0.0)], &law(0.0, ExtendLimit::UpTo(&x500))).unwrap();
        assert!(rel(area(&out), 5000.0) < 1e-9, "{}", area(&out));
        assert!((measure(&out).unwrap().bbox.max.x - 500.0).abs() < 1e-9);
    }

    /// The square with the notch [2, 10] × (4, 6) cut out, its three notch edges extended at 30°
    /// into the notch: the strips' corners at (2, 4) and (2, 6) are mitred, and the strip on the
    /// short edge x = 2 (2 long) is cut away where its rulings reach 1 across, 1 / cos 30° = 1.155
    /// long (a strip 1.5 long is refused). Up to the plane z = 0.5 the rulings are 0.5 / sin 30°
    /// = 1 long, short of that, so the long strip (whose corners cut the short strip away) is
    /// shortened as a folding one is and the target is reached: each strip loses
    /// c = 1² cos 30° / 2 per mitred corner, 84 + 1 · (8 + 2 + 8) − 4c = 102 − 2 cos 30°. Up to
    /// z = 0.7 (rulings 1.4 long, past the corners meeting) it is refused as a fold.
    #[test]
    fn a_law_extension_up_to_a_target_short_of_its_corners_meeting() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (2.0, 4.0), (2.0, 6.0), (10.0, 6.0), (10.0, 10.0), (0.0, 10.0)];
        let r = Region2 { outer: Loop2::polygon(&pts.map(|(x, y)| Vec2::new(x, y))), holes: vec![] };
        let notched = patch_region(&Plane::XY, &r).unwrap();
        let at = [Vec3::new(6.0, 4.0, 0.0), Vec3::new(2.0, 5.0, 0.0), Vec3::new(6.0, 6.0, 0.0)];
        let e = extend_surface(&notched, &at, &law(30.0, ExtendLimit::Distance(1.5))).unwrap_err();
        assert!(e.to_string().contains("cut a strip away"), "{e}");
        let plane = |z: f64| SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, z), Vec3::Z).unwrap());
        let out = extend_surface(&notched, &at, &law(30.0, ExtendLimit::UpTo(&plane(0.5)))).unwrap();
        let want = 102.0 - 2.0 * 30f64.to_radians().cos();
        assert!(rel(area(&out), want) < 1e-9, "{} vs {want}", area(&out));
        assert!((measure(&out).unwrap().bbox.max.z - 0.5).abs() < 1e-9);
        let e = extend_surface(&notched, &at, &law(30.0, ExtendLimit::UpTo(&plane(0.7)))).unwrap_err();
        assert!(e.to_string().contains("fold"), "{e}");
    }

    /// Refused: an angle at or past ±90° or not finite; a strip folding over itself (the cylinder
    /// strip's top arc leaning 60° in, 15 long: 13 mm in at a radius of 10); a target the strip
    /// never reaches (behind the edge, or parallel to the strip); up to a whole body or back to the
    /// bounds; edges between two faces and hole edges, as the other modes.
    #[test]
    fn law_extensions_refused() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        for deg in [90.0, -90.0, 120.0, f64::NAN, f64::INFINITY] {
            assert!(extend_surface(&s, &at, &law(deg, ExtendLimit::Distance(5.0))).is_err(), "{deg}");
        }
        assert!(extend_surface(&s, &at, &law(89.0, ExtendLimit::Distance(5.0))).is_ok());
        let c = cylinder_strip(FRAC_PI_2);
        let top = [Vec3::new(10.0 * FRAC_PI_4.cos(), 10.0 * FRAC_PI_4.sin(), 20.0)];
        let e = extend_surface(&c, &top, &law(-60.0, ExtendLimit::Distance(15.0))).unwrap_err();
        assert!(e.to_string().contains("fold"), "{e}");
        assert!(extend_surface(&c, &top, &law(-60.0, ExtendLimit::Distance(5.0))).is_ok());
        let behind = SplitTool::Plane(Plane::from_normal(Vec3::new(-5.0, 0.0, 0.0), Vec3::X).unwrap());
        let e = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&behind))).unwrap_err();
        assert!(e.to_string().contains("never reaches"), "{e}");
        let level = SplitTool::Plane(Plane::from_normal(Vec3::new(0.0, 0.0, 3.0), Vec3::Z).unwrap());
        let e = extend_surface(&s, &at, &law(0.0, ExtendLimit::UpTo(&level))).unwrap_err();
        assert!(e.to_string().contains("never reaches"), "{e}");
        let b = crate::box_solid(Vec3::new(20.0, 0.0, -5.0), Vec3::new(30.0, 10.0, 5.0)).unwrap();
        assert!(extend_surface(&s, &at, &law(0.0, ExtendLimit::UpToBody(&b))).is_err());
        assert!(extend_surface(&s, &at, &law(0.0, ExtendLimit::Bounds)).is_err());
        let solid = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        assert!(extend_surface(&solid, &[Vec3::new(10.0, 5.0, 10.0)], &law(0.0, ExtendLimit::Distance(1.0))).is_err());
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0), Vec2::new(0.0, 20.0)]),
            holes: vec![Loop2::circle(Vec2::new(10.0, 10.0), 3.0).reversed()],
        };
        let holed = patch_region(&Plane::XY, &r).unwrap();
        let e = extend_surface(&holed, &[Vec3::new(13.0, 10.0, 0.0)], &law(0.0, ExtendLimit::Distance(1.0))).unwrap_err();
        assert!(e.to_string().contains("hole"), "{e}");
    }

    /// Hostile and unsupported requests are refused, never panic.
    #[test]
    fn hostile_requests_are_refused() {
        let s = square();
        let at = [Vec3::new(10.0, 5.0, 0.0)];
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(extend_surface(&s, &at, &natural(d)).is_err(), "{d}");
        }
        assert!(extend_surface(&s, &[], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &[Vec3::new(f64::NAN, 0.0, 0.0)], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &vec![Vec3::ZERO; MAX_PICKS + 1], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &[Vec3::new(50.0, 50.0, 0.0)], &natural(1.0)).is_err());
        assert!(extend_surface(&s, &at, &with(ExtendMode::Tangent, ExtendLimit::Bounds, true)).is_err());
        // A solid's edges lie between two faces.
        let b = crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        assert!(extend_surface(&b, &[Vec3::new(10.0, 5.0, 10.0)], &natural(1.0)).is_err());
        // A hole's edge.
        let r = Region2 {
            outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0), Vec2::new(0.0, 20.0)]),
            holes: vec![Loop2::circle(Vec2::new(10.0, 10.0), 3.0).reversed()],
        };
        let holed = patch_region(&Plane::XY, &r).unwrap();
        let e = extend_surface(&holed, &[Vec3::new(13.0, 10.0, 0.0)], &with(ExtendMode::Tangent, ExtendLimit::Distance(1.0), true)).unwrap_err();
        assert!(e.to_string().contains("hole"), "{e}");
        // A target the extension never reaches (behind the edge).
        let behind = SplitTool::Plane(Plane::from_normal(Vec3::new(-5.0, 0.0, 0.0), Vec3::X).unwrap());
        assert!(extend_surface(&s, &at, &with(ExtendMode::Natural, ExtendLimit::UpTo(&behind), true)).is_err());
    }
}
