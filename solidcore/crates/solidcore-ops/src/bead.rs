//! Beads laid along joints, each a body of its own (#15130): the bead an application's welds,
//! sealant and glue share. The kernel builds them ([`beads_apart`]); ops picks the joints'
//! entities, makes one kernel call per body, on a copy of it, and gives the refusals as
//! [`Error`]s. The parts are never changed or merged into.
//!
//! So far a joint is a straight edge between two planes in an inside corner of one body, sized
//! by its two legs; one sagitta gives every bead of a call its profile (one call, one weld).

use solidcore::blend::weld_bead::beads_apart;
use solidcore::error::{FitMeasure, TargetEdge};
use solidcore::surface::procedural_blend::chamfer::ChamferSize;
use solidcore::{Brep, Kernel, SolidError};

use crate::blend::{sized_on, MAX_SIZE};
use crate::body::{finish_under, Body, Contract, Expect};
use crate::session::measure_body;
use crate::{Error, FitKind, Result};

/// A joint a bead is laid along, on one of the bodies of a [`beads`] call.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Joint {
    /// Edge `edge` (its index in the body's native edge list) of `bodies[body]`: straight,
    /// between two planes, in an inside corner. `legs` run from the edge to the bead's toes,
    /// the first along face `along` (its index in the body's native face list, one of the
    /// edge's two faces) when given, else along the face the kernel takes first. Each joint's
    /// `along` is its own: it never orders another joint's legs.
    Edge {
        /// The body's index in the call's `bodies`.
        body: usize,
        /// The edge's index in the body's native edge list.
        edge: usize,
        /// The legs, each finite, positive and at most [`MAX_SIZE`].
        legs: [f64; 2],
        /// The face the first leg runs along.
        along: Option<usize>,
    },
}

/// The beads along `joints`, one body each in the order of `joints`, every one with the
/// profile `sagitta` (the arc's height over the chord between the toes: `> 0` convex, `< 0`
/// concave, `0` flat). The beads are built on a copy of each body, so `bodies` are left as they
/// were, their proofs and caches included, whether the beads are built or refused. Each body
/// is a distinct body; the joints refer to them by index.
///
/// # Errors
/// [`Error::Argument`] for no joints, a sagitta not finite, a body listed twice, a body,
/// edge or face index past its list, or a leg not finite, positive and at most
/// [`MAX_SIZE`]; [`Error::TooLarge`] with the largest leg that builds ([`FitKind::Leg`]: `at`
/// the middle of its edge, `face` a point on the face its toe runs off, so the leg to shrink
/// is the one along that face) or the largest sagitta that builds ([`FitKind::Sagitta`], a
/// magnitude, `at` the middle of the edge it was found on); `size` 0 where none builds;
/// [`Error::BlendFailed`] with the kernel's statement for joints it does not build yet (two
/// beads meeting at a vertex, an edge not straight between two planes, an obtuse toe) and a
/// leg below the body's resolution; [`Error::Refused`] for an edge listed twice or not in an
/// inside corner and an `along` that is not one of its edge's faces; the kernel's other errors.
pub fn beads(bodies: &[&Body], joints: &[Joint], sagitta: f64) -> Result<Vec<Body>> {
    let leg_ok = |l: f64| l.is_finite() && l > 0.0 && l <= MAX_SIZE;
    let distinct = bodies
        .iter()
        .enumerate()
        .all(|(i, a)| !bodies.iter().skip(i + 1).any(|b| std::ptr::eq(*a, *b)));
    if joints.is_empty() || !sagitta.is_finite() || !distinct {
        return Err(Error::Argument);
    }
    for &Joint::Edge { body, legs, .. } in joints {
        if body >= bodies.len() || !legs.into_iter().all(leg_ok) {
            return Err(Error::Argument);
        }
    }
    let mut made: Vec<(usize, Body)> = Vec::with_capacity(joints.len());
    for (i, body) in bodies.iter().enumerate() {
        let on: Vec<(usize, Joint)> = joints
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, Joint::Edge { body: of, .. })| *of == i)
            .collect();
        if on.is_empty() {
            continue;
        }
        let beads = measure_body("beads", body, |k, b| on_body(k, b, &on, sagitta))??;
        made.extend(on.iter().map(|(j, _)| *j).zip(beads));
    }
    made.sort_by_key(|(j, _)| *j);
    Ok(made.into_iter().map(|(_, bead)| bead).collect())
}

/// The beads along `on` (each joint with its place in the call), all on `b`.
fn on_body(
    k: &mut Kernel,
    b: &mut Brep,
    on: &[(usize, Joint)],
    sagitta: f64,
) -> solidcore::Result<Result<Vec<Body>>> {
    let (mut edges, mut sizes, mut along) = (Vec::new(), Vec::new(), Vec::new());
    for &(
        _,
        Joint::Edge {
            edge,
            legs,
            along: face,
            ..
        },
    ) in on
    {
        let Some(&e) = b.edge_list().get(edge) else {
            return Ok(Err(Error::Argument));
        };
        edges.push(e);
        sizes.push(ChamferSize::Distances(legs));
        along.push(match face {
            Some(f) => match b.face_list().get(f) {
                Some(&f) => Some(f),
                None => return Ok(Err(Error::Argument)),
            },
            None => None,
        });
    }
    let beads = match beads_apart(k, b, &edges, &sizes, sagitta, &along) {
        Ok(beads) => beads,
        Err(e) => return refused(k, b, e).map(Err),
    };
    let mut out = Vec::with_capacity(beads.len());
    for bead in beads {
        match finish_under(k, bead, Expect::SOLID, Contract::Closed)? {
            Ok((bead, proof)) => out.push(Body::held(bead, proof)),
            Err(e) => return Ok(Err(e)),
        }
    }
    Ok(Ok(out))
}

/// The kernel's refusal `e` of the beads on `b` as ops': a leg or a sagitta too large with
/// the size that builds, its edge's middle and the point the kernel names on the limiting
/// face; a failure the kernel states in words with its statement ([`crate::blend::failure`]);
/// any other error as it is.
fn refused(k: &Kernel, b: &Brep, e: SolidError) -> solidcore::Result<Error> {
    if let Some(fit) = e.fit() {
        let kind = match fit.measure {
            FitMeasure::Leg => Some(FitKind::Leg),
            FitMeasure::Sagitta => Some(FitKind::Sagitta),
            _ => None,
        };
        if let Some(kind) = kind {
            let site = e.site();
            let edge = fit.edge.or_else(|| site.and_then(|s| s.edge));
            let too_large = Error::TooLarge {
                size: fit.size,
                kind,
                at: None,
                face: site.and_then(|s| s.face.and(s.at)).map(|p| [p.x, p.y, p.z]),
                why: None,
            };
            return sized_on(k, b, edge.map(TargetEdge::id), too_large);
        }
    }
    crate::blend::failure(&e).ok_or(e)
}
