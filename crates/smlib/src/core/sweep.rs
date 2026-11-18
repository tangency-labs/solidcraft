//! Extrusions, revolutions, sweeps, lofts, screw sweeps and planar faces: solidcore-ops' sweeps
//! (`solidcore_ops::sweep`), each run as an ops call ([`call::ops`]) and its error passed on
//! as this crate's. Ops makes the curves from the list, checks the numbers, runs the kernel's
//! sweep and hands the result over only when it is what was asked for: a closed solid when the
//! ends are capped ([`Error::NotSolid`] otherwise), never a body without faces
//! ([`Error::Empty`]).
//!
//! Two things are this crate's own, neither of them geometry. A straight extrusion sweeps one
//! wall per circle ([`one_wall_per_circle`]): which arcs go on from one another is ops' answer,
//! that such a run is one wall is SolidCraft's rule. And a face is named by a point
//! ([`super::offset::picked_faces`]), where ops takes its index.

use solidcore::EntityRef;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::brep::p8::FaceBuildEnv;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{Brep, FaceId};
use solidcore_ops::sweep as ops_sweep;

use super::{Body, call};
use crate::sweeps::{Affine, Curve as Profile, DraftCorner, Ends, MAX_CURVES, SweepFrame};
use crate::{Error, Result};

/// Each face's identity, for the face builders of the families still on the kernel crate (split,
/// trim): their arena identity.
pub(super) fn build_faces<T>(b: &mut Brep, f: impl FnOnce(&mut Brep, &mut FaceBuildEnv<'_, '_>) -> T) -> T {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv { env: &mut env, marks: &mut marks, this_face: &this_face };
    f(b, &mut fenv)
}

/// A sheet of planar faces bounded by the closed `loops`.
pub(crate) fn planar_faces(loops: &[Profile]) -> Result<Body> {
    call::ops("planar_faces", || ops_sweep::planar_faces(loops))
}

/// The arcs of `given` that go on from one another on one circle within a loop, made one arc
/// each, so that the run's wall is one face as a single arc's is: a circle drawn in pieces (split
/// where another sketch curve ends on it) sweeps the one cylinder it lies on. A run that is its
/// whole loop is the full circle, its seam at the drawn circle's start when the arcs share its
/// frame, else at the middle of the run's longest arc. Which curves meet, which arcs lie on one
/// circle and the joined arc are solidcore-ops' (`curves_meet`, `arcs_on_one_circle`,
/// `join_arcs`), to the model's zone tolerance `tol`; a run ops does not prove joins stays as
/// drawn.
fn one_wall_per_circle(given: &[Profile], tol: f64) -> Result<Vec<Profile>> {
    use ops_sweep::Join;
    let n = given.len();
    if !given.iter().any(|c| matches!(c, Profile::Arc { .. })) {
        return Ok(given.to_vec());
    }
    let at = |i: usize| given.get(i).ok_or(Error::Argument);
    // Whether the end of curve `i` meets the start of curve `j`; whether arc `j` goes on from
    // arc `i`, on its circle.
    let meets = |i: usize, j: usize| -> Result<bool> {
        Ok({
            let (a, b) = (at(i)?, at(j)?);
            call::ops("extrude", || ops_sweep::curves_meet(a, b, tol))?
        }
        .is_yes())
    };
    let joins = |i: usize, j: usize| -> Result<bool> {
        let (a, b) = (at(i)?, at(j)?);
        match call::ops("extrude", || ops_sweep::arcs_on_one_circle(a, b, tol))? {
            Some(on) => Ok(on.circle.is_yes() && meets(i, j)?),
            None => Ok(false),
        }
    };
    let mut out = Vec::with_capacity(n);
    let mut first = 0;
    while first < n {
        // The loop from `first`: its curves run end to end until one ends at its start.
        let mut last = first;
        while last + 1 < n && !meets(last, first)? && meets(last, last + 1)? {
            last += 1;
        }
        let len = last - first + 1;
        let closed = meets(last, first)?;
        // Which of the loop's curves goes on from the one before it round the loop.
        let mut goes_on = vec![false; len];
        for (j, on) in goes_on.iter_mut().enumerate() {
            if j > 0 || (closed && len > 1) {
                *on = joins(first + (j + len - 1) % len, first + j)?;
            }
        }
        if !goes_on.contains(&true) {
            out.extend_from_slice(given.get(first..=last).ok_or(Error::Argument)?);
            first = last + 1;
            continue;
        }
        // A run starts where its curve does not go on from the one before it.
        let begin = goes_on.iter().position(|on| !on);
        let Some(begin) = begin else {
            // The whole loop is arcs of one circle: the full circle.
            let run = given.get(first..=last).ok_or(Error::Argument)?;
            let mut one_frame = true;
            for b in run {
                one_frame = one_frame && {
                    let a = at(first)?;
                    call::ops("extrude", || ops_sweep::arcs_on_one_circle(a, b, tol))?
                }
                .is_some_and(|on| on.frame.is_yes());
            }
            let join = if one_frame { Join::ClosedAtFrameStart } else { Join::ClosedAtMiddleOfLongest };
            match call::ops("extrude", || ops_sweep::join_arcs(run, join, tol)) {
                Ok(c) => out.push(c),
                Err(_) => out.extend_from_slice(run),
            }
            first = last + 1;
            continue;
        };
        let mut kk = 0;
        while kk < len {
            let mut m = 1;
            while kk + m < len && goes_on.get((begin + kk + m) % len).copied().unwrap_or(false) {
                m += 1;
            }
            let run: Vec<Profile> = (0..m).map(|j| at(first + (begin + kk + j) % len).cloned()).collect::<Result<_>>()?;
            if m == 1 {
                out.extend(run);
            } else {
                match call::ops("extrude", || ops_sweep::join_arcs(&run, Join::Open, tol)) {
                    Ok(c) => out.push(c),
                    Err(_) => out.extend(run),
                }
            }
            kk += m;
        }
        first = last + 1;
    }
    Ok(out)
}

/// The planar `profile` extruded `distance` along `direction`, straight or drafted.
pub(crate) fn extrude(profile: &[Profile], direction: [f64; 3], distance: f64, draft: f64, corner: DraftCorner, cap: bool) -> Result<Body> {
    if draft == 0.0 && !profile.is_empty() && profile.len() <= MAX_CURVES {
        let tol = call::ops("zone_tolerance", ops_sweep::zone_tolerance)?;
        let walls = one_wall_per_circle(profile, tol)?;
        return call::ops("extrude", || ops_sweep::extrude(&walls, direction, distance, draft, corner, cap));
    }
    call::ops("extrude", || ops_sweep::extrude(profile, direction, distance, draft, corner, cap))
}

/// The planar `profile` turned `angle` radians about the axis through `origin` along `axis`.
pub(crate) fn revolve(profile: &[Profile], origin: [f64; 3], axis: [f64; 3], angle: f64, cap: bool) -> Result<Body> {
    call::ops("revolve", || ops_sweep::revolve(profile, origin, axis, angle, cap))
}

/// `profile` swept along `path`, with the rigid motions that placed it at the start, half way
/// and the end.
pub(crate) fn sweep_placed(profile: &[Profile], path: &[Profile], frame: SweepFrame, cap: bool) -> Result<(Body, [Affine; 3])> {
    let frame = match frame {
        SweepFrame::Follow => ops_sweep::SweepFrame::Follow,
        SweepFrame::Translate => ops_sweep::SweepFrame::Translate,
    };
    call::ops("sweep", || ops_sweep::sweep_placed(profile, path, frame, cap)).map(|(b, placed, _)| (b, placed))
}

/// A skin through `sections`, `degree` across them.
pub(crate) fn loft(sections: &[Vec<Profile>], degree: u32, cap: bool) -> Result<Body> {
    call::ops("loft", || ops_sweep::loft(sections, degree, cap))
}

/// The planar `section` lofted to `apex`.
pub(crate) fn loft_to_point(section: &[Profile], apex: [f64; 3]) -> Result<Body> {
    call::ops("loft_to_point", || ops_sweep::loft_to_point(section, apex))
}

/// The profile placed by each station, its sections skinned through.
pub(crate) fn sweep_stations(profile: &[Profile], stations: &[Affine], end: Ends) -> Result<Body> {
    call::ops("sweep_stations", || ops_sweep::sweep_stations(profile, stations, end))
}

/// The screw sweep of `profile`, with its radial `law` when it has one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn screw_sweep(
    profile: &[Profile],
    origin: [f64; 3],
    axis: [f64; 3],
    pitch: f64,
    turns: f64,
    law: Option<&[(f64, f64)]>,
    tolerance: f64,
    join_spline_walls: bool,
) -> Result<Body> {
    let walls = if join_spline_walls { ops_sweep::SplineWalls::Joined } else { ops_sweep::SplineWalls::PerPiece };
    call::ops("screw_sweep", || ops_sweep::screw_sweep_walls(profile, origin, axis, pitch, turns, law, tolerance, walls))
}

/// The pick tolerance and the curve tolerance of [`project_curves`] and [`face_section`].
fn tolerances_ok(pick: f64, tolerance: f64) -> bool {
    pick.is_finite() && pick > 0.0 && pick < 1e6 && tolerance.is_finite() && (1e-9..1e3).contains(&tolerance)
}

/// Whether `v` has a length to speak of (above 1e-12): a direction with none names no direction,
/// refused before a face is picked.
fn has_length(v: [f64; 3]) -> bool {
    v.iter().map(|x| x * x).sum::<f64>() > 1e-24
}

/// The one face of `body` nearest `point`, with the body's order.
fn picked_face(call: &'static str, body: &Body, point: [f64; 3], tol: f64) -> Result<(solidcore_ops::order::Order, usize)> {
    let (order, faces) = super::offset::picked_faces(call, body, &[point], tol)?;
    let face = faces.first().copied().ok_or(Error::NoMatch { point: 0 })?;
    Ok((order, face))
}

/// `curves` projected along `direction` onto the face nearest `face_point`, as NURBS with their
/// intervals; none is [`Error::Empty`].
pub(crate) fn project_curves(
    body: &Body,
    face_point: [f64; 3],
    pick_tolerance: f64,
    curves: &[Profile],
    direction: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Profile, [f64; 2])>> {
    if !face_point.iter().chain(&direction).all(|x| x.is_finite()) || !tolerances_ok(pick_tolerance, tolerance) || !has_length(direction) {
        return Err(Error::Argument);
    }
    let (order, face) = picked_face("project_curves", body, face_point, pick_tolerance)?;
    call::ops("project_curves", || ops_sweep::project_curves(body, &order, face, curves, direction, tolerance))
}

/// The section of the face nearest `face_point` by the plane through `plane_point` square to
/// `plane_normal`, as NURBS with their intervals; none is [`Error::Empty`].
pub(crate) fn face_section(
    body: &Body,
    face_point: [f64; 3],
    pick_tolerance: f64,
    plane_point: [f64; 3],
    plane_normal: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Profile, [f64; 2])>> {
    if !face_point.iter().chain(&plane_point).chain(&plane_normal).all(|x| x.is_finite())
        || !tolerances_ok(pick_tolerance, tolerance)
        || !has_length(plane_normal)
    {
        return Err(Error::Argument);
    }
    let (order, face) = picked_face("face_section", body, face_point, pick_tolerance)?;
    call::ops("face_section", || ops_sweep::face_section(body, &order, face, plane_point, plane_normal, tolerance))
}
