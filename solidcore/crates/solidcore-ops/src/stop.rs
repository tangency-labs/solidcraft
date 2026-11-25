//! Walls stopped where they first run into bodies (a surface extrude To Next): the bodies in
//! the walls' way imprinted on them one by one, the pieces joined to the walls' start kept, and
//! each kept piece checked to lie on the start side of every edge a body cut.
//!
//! Each face and edge of the walls, and each edge of a body, is tagged before the imprints
//! ([`crate::provenance`]), and the kernel's edits carry the tags through: so after the imprints
//! each face says which face of the walls it is a piece of, and each edge whether a body cut it
//! (an edge new where the faces met, or a body's own edge). Ops sequences the kernel's calls (the
//! imprint, the record, the side of each cut edge a kept face lies on, proved by the kernel along
//! the whole edge, the faces taken out) and compares what they
//! return; it computes nothing itself.
//!
//! Upstream: none (a SolidCraft workflow over `SmApiBooleanWithOptions(SM_BO_IMPRINT)`,
//! `SmEdgeuse::EvaluateBinormal` and `SmBrep::RemoveFaces`).
//!
//! Deviations: none.

use std::collections::BTreeSet;

use solidcore::query::api_queries::{unit_direction, vertex_match_tolerance};
use solidcore::query::tangent_edge::{side_along, SideAlong};
use solidcore::topology::{edge, face, EdgeId};
use solidcore::{EntityRef, Vec3};
use solidcore_record::brep::Brep as Record;

use crate::body::Body;
use crate::boolean::imprint;
use crate::measure::{box_max_distance_to_point, points_meet};
use crate::order::{self, Order};
use crate::provenance::{self, Provenance, FIRST_ID};
use crate::session::with_body;
use crate::trim::remove_faces;
use crate::{Error, Result};

/// The most bodies one [`stop_at_bodies`] cuts with.
pub const MAX_BODIES: usize = 10_000;

/// The most start points one [`stop_at_bodies`] takes.
pub const MAX_STARTS: usize = 100_000;

/// How far a kept piece may run on along the walls' direction across a cut edge: the cosine of
/// the binormal into it with that direction.
const ON_PAST: f64 = 1e-6;

/// What a face or edge stands for through the imprints: for a face, the record faces of the
/// walls it is a piece of; for an edge, whether it is a body's own edge. An edge carrying none
/// is new: where a body's face met the walls.
#[derive(Clone, Debug, PartialEq)]
struct Piece {
    faces: BTreeSet<usize>,
    body: bool,
}

impl Piece {
    fn joined(&self, other: &Piece) -> Piece {
        Piece {
            faces: self.faces.union(&other.faces).copied().collect(),
            body: self.body || other.body,
        }
    }

    fn join(&self, with: Option<&Piece>) -> Piece {
        with.map_or_else(|| self.clone(), |w| self.joined(w))
    }
}

/// A split child, a merge's result and the entity a coincidence lands on each take the origins
/// of both: so an edge of the walls that a body's edge lies on is cut, as the body's edge is.
impl Provenance for Piece {
    const ID: u64 = FIRST_ID + 20;
    fn split(&self, child: Option<&Self>) -> Option<Self> {
        Some(self.join(child))
    }
    fn merge(&self, result: Option<&Self>) -> Option<Self> {
        Some(self.join(result))
    }
    fn coincident(&self, onto: Option<&Self>) -> Option<Self> {
        Some(self.join(onto))
    }
}

/// The walls [`stop_at_bodies`] kept: their record, and for each of its faces (in record order)
/// the record face of the walls it is a piece of.
#[derive(Clone, Debug)]
pub struct Stopped {
    /// The kept walls.
    pub record: Record,
    /// For each face of `record`, the face of the walls it is a piece of.
    pub from: Vec<usize>,
}

/// The walls of a surface extrude (`walls`, its record order `order`, swept along the
/// direction `along`, of any length) cut where they first run into `bodies`: each body's faces
/// exactly as they are, bounded where they end and never extended (the kernel's imprint of each body in turn).
/// The pieces kept are those joined to the walls' start without crossing an edge a body cut:
/// `starts` are corners of the walls at their start, one on each chain, matched to the vertices
/// within ten times the largest tolerance (the record's, an edge's, or 1e-9 of the walls' size).
///
/// Each kept piece must lie on the start side of every cut edge bounding it: the kernel's
/// binormal of the edge use, into the piece, never runs on with `along`, proved over the whole
/// edge (the kernel's `side_along`, never at sample points). A piece that runs on past a body beside it (a body covering only part of a wall, the
/// wall running past its side) is refused: keeping it would keep wall above that body.
///
/// `None` when no body cut the walls. The bodies and the walls are unchanged.
///
/// # Errors
/// - [`Error::Argument`]: no start, more than [`MAX_STARTS`] starts or [`MAX_BODIES`] bodies, a
///   coordinate that is not finite, or an `along` of no length;
/// - [`Error::Unsupported`]: a kept piece that runs on past the side of a body in its way, or one
///   whose side of a cut edge is not proved;
/// - [`Error::Internal`]: a piece that comes from no face of the walls, or no piece holding a
///   start;
/// - the kernel's errors (those of [`imprint`] and [`remove_faces`] too).
pub fn stop_at_bodies(
    walls: &Body,
    order: &Order,
    bodies: &[&Body],
    starts: &[[f64; 3]],
    along: [f64; 3],
) -> Result<Option<Stopped>> {
    let finite = |p: &[f64; 3]| p.iter().all(|x| x.is_finite());
    if starts.is_empty()
        || starts.len() > MAX_STARTS
        || bodies.len() > MAX_BODIES
        || !starts.iter().all(finite)
        || !finite(&along)
    {
        return Err(Error::Argument);
    }
    let along =
        unit_direction(Vec3::new(along[0], along[1], along[2])).map_err(|_| Error::Argument)?;
    provenance::take::<Piece>()?;
    let (mut cur, mapped) = walls.copy_with_order(order)?;
    {
        let mut b = cur.lock();
        for (i, &f) in mapped.faces.iter().enumerate() {
            let piece = Piece {
                faces: BTreeSet::from([i]),
                body: false,
            };
            provenance::attach(&mut b, EntityRef::Face(f), piece)?;
        }
        for &e in &mapped.edges {
            let piece = Piece {
                faces: BTreeSet::new(),
                body: false,
            };
            provenance::attach(&mut b, EntityRef::Edge(e), piece)?;
        }
    }
    let mut met = false;
    for body in bodies {
        let tool = body.copy()?;
        {
            let mut t = tool.lock();
            for e in t.edge_list().to_vec() {
                let piece = Piece {
                    faces: BTreeSet::new(),
                    body: true,
                };
                provenance::attach(&mut t, EntityRef::Edge(e), piece)?;
            }
        }
        let (next, changed) = imprint(cur, tool)?;
        cur = next;
        met |= changed;
    }
    if !met {
        return Ok(None);
    }
    let (record, rorder) = cur.to_record()?;
    // The record edges a body cut, and their kernel edges.
    let cut: Vec<bool> = {
        let b = cur.lock();
        rorder
            .edges
            .iter()
            .map(|&e| provenance::read::<Piece>(&b, EntityRef::Edge(e)).is_none_or(|p| p.body))
            .collect()
    };
    let cut_edges: Vec<EdgeId> = rorder
        .edges
        .iter()
        .zip(&cut)
        .filter(|(_, c)| **c)
        .map(|(e, _)| *e)
        .collect();
    let kept = kept_regions(&cur, &record, &cut, starts)?;
    let kept_faces: Vec<usize> = (0..kept.len())
        .filter(|f| kept.get(*f).copied().unwrap_or(false))
        .collect();
    runs_on_past(&cur, &rorder, &kept_faces, &cut_edges, along)?;
    let dropped: Vec<usize> = (0..kept.len())
        .filter(|f| !kept.get(*f).copied().unwrap_or(true))
        .collect();
    let left = if dropped.is_empty() {
        cur
    } else {
        remove_faces(&cur, &rorder, &dropped)?
    };
    let (record, lorder) = left.to_record()?;
    let from = {
        let b = left.lock();
        lorder
            .faces
            .iter()
            .map(|&f| {
                provenance::read::<Piece>(&b, EntityRef::Face(f))
                    .and_then(|p| p.faces.first().copied())
            })
            .collect::<Option<Vec<usize>>>()
    };
    let from = from.ok_or_else(|| {
        Error::Internal("stop_at_bodies: a piece comes from no face of the walls".into())
    })?;
    Ok(Some(Stopped { record, from }))
}

/// The region face `i` belongs to: the root of its chain in `parent` (union-find).
fn root(parent: &[usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i).filter(|p| **p != i) {
        i = p;
    }
    i
}

/// Which record faces of the imprinted walls `cur` (record `record`, the edges a body cut
/// `cut`) are kept: the regions, faces joined across edges no body cut, that hold a vertex at
/// one of `starts`.
fn kept_regions(
    cur: &Body,
    record: &Record,
    cut: &[bool],
    starts: &[[f64; 3]],
) -> Result<Vec<bool>> {
    let n = record.faces.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for (e, edge) in record.edges.iter().enumerate() {
        if cut.get(e).copied().unwrap_or(true) {
            continue;
        }
        let faces: Vec<usize> = edge.uses.iter().map(|u| u.face as usize).collect();
        for pair in faces.windows(2) {
            let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let (a, b) = (root(&parent, a), root(&parent, b));
            if let Some(slot) = parent.get_mut(a.max(b)) {
                *slot = a.min(b);
            }
        }
    }
    let (lo, hi) = cur.bounds()?;
    let size = box_max_distance_to_point(lo, hi, lo)?;
    let largest = record
        .edges
        .iter()
        .map(|e| e.tolerance)
        .fold(record.tolerance, f64::max);
    let accept = vertex_match_tolerance(largest, size);
    let mut at_start = Vec::new();
    for v in &record.vertices {
        let mut at = false;
        for s in starts {
            if points_meet(v.point, *s, accept)? {
                at = true;
                break;
            }
        }
        at_start.push(at);
    }
    let mut wanted = BTreeSet::new();
    for edge in &record.edges {
        if edge
            .vertices
            .iter()
            .any(|v| at_start.get(*v as usize).copied().unwrap_or(false))
        {
            wanted.extend(edge.uses.iter().map(|u| root(&parent, u.face as usize)));
        }
    }
    if wanted.is_empty() {
        return Err(Error::Internal(
            "stop_at_bodies: no piece of the walls holds their start".into(),
        ));
    }
    Ok((0..n).map(|f| wanted.contains(&root(&parent, f))).collect())
}

/// Refuses a kept face (record faces `kept` of `cur`, order `order`) that runs on with `along`
/// across one of the edges a body cut (`cut`): the kernel's binormal of the face's edge use,
/// pointing into the face, has a part along `along` over [`ON_PAST`] somewhere on the edge. That
/// is proved over the whole edge, never sampled (the kernel's `side_along`): an edge it cannot
/// place on either side is refused too, as not proved.
fn runs_on_past(
    cur: &Body,
    order: &Order,
    kept: &[usize],
    cut: &[EdgeId],
    along: Vec3,
) -> Result<()> {
    let faces = kept
        .iter()
        .map(|&f| order::face(order, f))
        .collect::<Result<Vec<_>>>()?;
    with_body("stop_at_bodies", cur, |k, b| {
        for &f in &faces {
            for e in face::edges(b, f, None, false, None)? {
                if !cut.contains(&e) {
                    continue;
                }
                let Some(eu) = edge::edgeuse_of_face(b, e, Some(f))? else {
                    continue;
                };
                match side_along(k, b, eu, along, ON_PAST)? {
                    SideAlong::Within => {}
                    SideAlong::RunsOn { .. } => {
                        return Ok(Err(Error::Unsupported(
                            "part of a wall runs on past the side of a body in its way (the body covers only part of the wall)",
                        )));
                    }
                    SideAlong::Undecided { .. } => {
                        return Ok(Err(Error::Unsupported(
                            "which side of a body part of a wall lies on is not proved along the edge where they meet",
                        )));
                    }
                }
            }
        }
        Ok(Ok(()))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sweep::{extrude, Curve, DraftCorner};
    use crate::{make_box, session};

    fn all_fixes() {
        session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    /// The line from `a` to `b` swept `distance` along `direction`: a sheet of one face.
    fn sheet(a: [f64; 3], b: [f64; 3], direction: [f64; 3], distance: f64) -> Body {
        let line = [Curve::Line { start: a, end: b }];
        extrude(
            &line,
            direction,
            distance,
            0.0,
            DraftCorner::Curvature,
            false,
        )
        .unwrap()
    }

    /// The kept walls' area and their highest level.
    fn area_and_top(s: &Stopped) -> (f64, f64) {
        let (b, _) = Body::from_record(&s.record, true).unwrap();
        (b.area(1e-9).unwrap(), b.bounds().unwrap().1[2])
    }

    /// A wall at y = 0 from x = 0 to 10, swept from z = 0 to 50, stops where it first runs into
    /// a body, each body's faces as they are. The areas are worked out from the shapes: 10 wide,
    /// up to the first body's underside.
    #[test]
    fn walls_stop_at_the_first_body_in_their_way() {
        all_fixes();
        let up = [0.0, 0.0, 1.0];
        let wall = sheet([0.0; 3], [10.0, 0.0, 0.0], up, 50.0);
        let (_, order) = wall.to_record().unwrap();
        let starts = [[0.0; 3]];
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-8 * b.abs().max(1.0);
        // A slab across the wall from z = 20 to 22: 10 x 20 is left, ending at z = 20, each
        // kept face a piece of the wall's one face.
        let slab = make_box([-1.0, -1.0, 20.0], [12.0, 2.0, 2.0]).unwrap();
        let s = stop_at_bodies(&wall, &order, &[&slab], &starts, up)
            .unwrap()
            .unwrap();
        let (area, top) = area_and_top(&s);
        assert!(close(area, 200.0) && close(top, 20.0), "{area} {top}");
        assert!(
            !s.from.is_empty() && s.from.iter().all(|f| *f == 0),
            "{:?}",
            s.from
        );
        // A direction of another length is the same direction.
        let s = stop_at_bodies(&wall, &order, &[&slab], &starts, [0.0, 0.0, 2.0])
            .unwrap()
            .unwrap();
        let (area, top) = area_and_top(&s);
        assert!(close(area, 200.0) && close(top, 20.0), "{area} {top}");
        // A surface body across it at z = 8 is nearer, whichever body comes first: 10 x 8.
        let lid = sheet([-1.0, -1.0, 8.0], [11.0, -1.0, 8.0], [0.0, 1.0, 0.0], 2.0);
        for bodies in [[&slab, &lid], [&lid, &slab]] {
            let s = stop_at_bodies(&wall, &order, &bodies, &starts, up)
                .unwrap()
                .unwrap();
            let (area, top) = area_and_top(&s);
            assert!(close(area, 80.0) && close(top, 8.0), "{area} {top}");
        }
        // A body out of the way cuts nothing.
        let aside = make_box([40.0, 40.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
        assert!(stop_at_bodies(&wall, &order, &[&aside], &starts, up)
            .unwrap()
            .is_none());
        // A block over the left half only, from z = 20 to 22, under a sheet across at z = 30:
        // the piece beside the block runs on over it up to z = 30. That is refused, never kept.
        let half = make_box([-1.0, -1.0, 20.0], [6.0, 2.0, 2.0]).unwrap();
        let high = sheet([-1.0, -1.0, 30.0], [11.0, -1.0, 30.0], [0.0, 1.0, 0.0], 2.0);
        let e = stop_at_bodies(&wall, &order, &[&half, &high], &starts, up).unwrap_err();
        assert!(matches!(e, Error::Unsupported(_)), "{e}");
        // The walls and the bodies are unchanged.
        assert!(close(wall.area(1e-9).unwrap(), 500.0));
        assert_eq!(slab.counts().unwrap()[0], 6);
    }

    /// The side of an edge is proved along all of it, not at points. A plate bounded at the top
    /// by the cubic Bezier through (0, 10), (3.1535, 11), (0.682, 12), (2.5855, 13): its `x'` is
    /// `30 (t - 0.53) (t - 0.595)`, so the curve turns back (x falling as y rises) only for t in
    /// (0.53, 0.595), between the old sample points 1/2 and 5/8. There the plate runs on past the
    /// curve along +y: at t = 0.5625 the binormal into it is `(y', -x') / |T|` with `x' =
    /// -0.0317`, `y' = 3`, a part of 0.0106 along +y. At the samples t = k/8 that part is at most
    /// -0.0285, so a check at them passes it. The same plate under a curve that never turns back
    /// (the middle points (1, 11), (2, 12)) is within.
    #[test]
    fn the_side_of_a_cut_edge_is_proved_along_all_of_it() {
        all_fixes();
        let plate = |p1: [f64; 2], p2: [f64; 2]| {
            let at = |p: [f64; 2]| [p[0], p[1], 0.0];
            let profile = [
                Curve::Nurbs {
                    degree: 3,
                    points: [[0.0, 10.0], p1, p2, [2.5855, 13.0]]
                        .iter()
                        .map(|p| [p[0], p[1], 0.0, 1.0])
                        .collect(),
                    knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                },
                Curve::Line {
                    start: at([2.5855, 13.0]),
                    end: at([2.5855, 0.0]),
                },
                Curve::Line {
                    start: at([2.5855, 0.0]),
                    end: at([0.0, 0.0]),
                },
                Curve::Line {
                    start: at([0.0, 0.0]),
                    end: at([0.0, 10.0]),
                },
            ];
            extrude(
                &profile,
                [0.0, 0.0, 1.0],
                1.0,
                0.0,
                DraftCorner::Curvature,
                true,
            )
            .unwrap()
        };
        // each use of the curve's edge on the plate at z = 0 (the side face's use runs along +z,
        // square to +y): the old samples' largest part, and the proof's answer
        let along = Vec3::new(0.0, 1.0, 0.0);
        let answers = |body: &Body| {
            with_body("test", body, |k, b| {
                let mut out = Vec::new();
                for e in b.edge_list().to_vec() {
                    let curved = edge::curve(b, e)?.is_some_and(|c| {
                        !matches!(c.kind(), solidcore::curve::curve::CurveKind::Line(_))
                    });
                    let ivl = edge::interval(b, e)?;
                    let mid = edge::curve(b, e)?
                        .map(|c| c.evaluate_point(k, 0.5 * (ivl.min() + ivl.max())))
                        .transpose()?;
                    if !curved || mid.is_none_or(|p| p.z.abs() > 1e-9) {
                        continue;
                    }
                    let mut uses = Vec::new();
                    for f in b.face_list().to_vec() {
                        if face::edges(b, f, None, false, None)?.contains(&e) {
                            uses.extend(edge::edgeuse_of_face(b, e, Some(f))?);
                        }
                    }
                    for eu in uses {
                        let mut sampled = f64::NEG_INFINITY;
                        for s in [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875] {
                            let t = ivl.evaluate(s);
                            let at = solidcore::topology::edgeuse::p3::evaluate_binormal(
                                k,
                                b,
                                eu,
                                t,
                                true,
                                &Default::default(),
                            )?;
                            sampled = sampled.max(at.binormal.dot(along));
                        }
                        out.push((sampled, side_along(k, b, eu, along, ON_PAST)?));
                    }
                }
                Ok(Some(out))
            })
            .unwrap()
            .unwrap()
        };
        let folded = answers(&plate([3.1535, 11.0], [0.682, 12.0]));
        assert!(!folded.is_empty());
        assert!(
            folded.iter().all(|(sampled, _)| *sampled <= ON_PAST),
            "{folded:?}"
        );
        assert!(
            folded.iter().any(
                |(_, side)| matches!(side, SideAlong::RunsOn { at } if (0.53..0.595).contains(at))
            ),
            "{folded:?}"
        );
        let plain = answers(&plate([1.0, 11.0], [2.0, 12.0]));
        assert!(!plain.is_empty());
        assert!(
            plain.iter().all(|(_, side)| *side == SideAlong::Within),
            "{plain:?}"
        );
    }

    /// Hostile input is refused, not a crash.
    #[test]
    fn hostile_stops_are_refused() {
        all_fixes();
        let up = [0.0, 0.0, 1.0];
        let wall = sheet([0.0; 3], [10.0, 0.0, 0.0], up, 50.0);
        let (_, order) = wall.to_record().unwrap();
        let slab = make_box([-1.0, -1.0, 20.0], [12.0, 2.0, 2.0]).unwrap();
        let starts = [[0.0; 3]];
        for (starts, along) in [
            (&[][..], up),
            (&starts[..], [0.0; 3]),
            (&starts[..], [f64::INFINITY, 0.0, 1.0]),
            (&starts[..], [f64::NAN, 0.0, 1.0]),
            (&[[f64::NAN, 0.0, 0.0]][..], up),
        ] {
            assert_eq!(
                stop_at_bodies(&wall, &order, &[&slab], starts, along).unwrap_err(),
                Error::Argument
            );
        }
    }
}
