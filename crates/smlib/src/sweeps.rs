//! SMLib's sweeps: extrusion (straight or drafted), revolution, sweep along a path, loft through
//! sections or to a point, the screw sweep of a thread or coil, and the sweep of a profile placed
//! at stations (one that twists, scales or follows guide rails along its path). Profiles, paths and
//! sections are lists of [`Curve`]s; each sweep returns a new body.
//!
//! With `cap`, the profile's loops close the ends and the result must be a closed manifold solid
//! ([`Error::NotSolid`] otherwise); without, it is the swept sheet.

use crate::{Brep, Error, Result, core, error, ffi, init, route};

/// A 3D curve of a profile, path or section.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Line {
        start: [f64; 3],
        end: [f64; 3],
    },
    /// Counter-clockwise about the unit `axis` from angle `start` to `end` (radians,
    /// 0 < end - start <= 2 pi), angle 0 along the unit `reference` (square to `axis`).
    Arc {
        center: [f64; 3],
        axis: [f64; 3],
        reference: [f64; 3],
        radius: f64,
        start: f64,
        end: f64,
    },
    /// Control points (x, y, z, weight > 0) and the clamped knot vector (points + degree + 1
    /// knots).
    Nurbs {
        degree: usize,
        points: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
}

/// How a drafted [`Brep::extrude`] closes the gap its offset opens at a sharp corner of the
/// profile. Where the offsets overlap instead, all three trim them where they cross.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DraftCorner {
    /// Each wall continues as itself (a line's plane, an arc's cone) until it meets its
    /// neighbour: every section is the profile's curves offset and continued as themselves.
    #[default]
    Curvature,
    /// Each offset curve continues straight along its end tangent: a flat face each side.
    Tangent,
    /// An arc about the corner: a cone about it.
    Round,
}

/// Why [`DraftCorner::Curvature`] could not close a corner ([`Error::Corner`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerRefusal {
    /// The continued curves do not meet (two parallel lines, two circles that part).
    Apart,
    /// A wall closes before the far end: its neighbours meet across it, or its arc shrinks to its
    /// centre.
    WallCloses,
    /// A curve there is neither a line nor an arc: its wall has no exact continuation.
    NotExact,
}

/// How the profile is carried along a [`Brep::sweep`] path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepFrame {
    /// It turns with the path, keeping its attitude to the path's tangent.
    Follow,
    /// It only moves along the path, keeping its orientation.
    Translate,
}

/// How the ends of a [`Brep::sweep_stations`] close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// Left open: the swept sheet.
    Open,
    /// Capped by the first and last sections: a closed solid.
    Capped,
    /// Joined: the last section lies on the first (a sweep round a closed path), the walls' ends
    /// glued to their starts into a closed solid.
    Joined,
}

/// An affine map of a [`Brep::sweep_stations`] station: a point `x` goes to `m · x + t`, `m`
/// given by its columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub m: [[f64; 3]; 3],
    pub t: [f64; 3],
}

/// Most stations of one [`Brep::sweep_stations`] (the C API's limit).
pub const MAX_STATIONS: usize = ffi::sweeps::SWEEP_MAX_STATIONS;
/// Most profile curves times stations of one [`Brep::sweep_stations`] (the C API's limit).
pub const MAX_STATION_CURVES: usize = ffi::sweeps::SWEEP_MAX_STATION_CURVES;

/// Most curves one list may hold (the C API's limit).
pub const MAX_CURVES: usize = ffi::sweeps::CURVE_MAX_CURVES;

/// The curves as the C API's curve list.
#[derive(Default)]
pub(crate) struct List {
    kinds: Vec<i64>,
    reals: Vec<f64>,
}

impl List {
    pub(crate) fn of<'a>(curves: impl IntoIterator<Item = &'a Curve>) -> Result<List> {
        let mut l = List::default();
        for c in curves {
            l.push(c)?;
        }
        Ok(l)
    }

    fn push(&mut self, c: &Curve) -> Result<()> {
        if self.kinds.len() >= 3 * MAX_CURVES {
            return Err(Error::Argument);
        }
        match c {
            Curve::Line { start, end } => {
                self.kinds.push(ffi::sweeps::CURVE_LINE);
                self.reals.extend_from_slice(start);
                self.reals.extend_from_slice(end);
            }
            Curve::Arc { center, axis, reference, radius, start, end } => {
                self.kinds.push(ffi::sweeps::CURVE_ARC);
                self.reals.extend_from_slice(center);
                self.reals.extend_from_slice(axis);
                self.reals.extend_from_slice(reference);
                self.reals.extend_from_slice(&[*radius, *start, *end]);
            }
            Curve::Nurbs { degree, points, knots } => {
                if *degree == 0
                    || *degree > ffi::sweeps::CURVE_MAX_DEGREE
                    || points.len() > ffi::sweeps::CURVE_MAX_POINTS
                    || knots.len() != points.len() + degree + 1
                {
                    return Err(Error::Argument);
                }
                let d = i64::try_from(*degree).map_err(|_| Error::Argument)?;
                let n = i64::try_from(points.len()).map_err(|_| Error::Argument)?;
                self.kinds.extend_from_slice(&[ffi::sweeps::CURVE_NURBS, d, n]);
                for p in points {
                    self.reals.extend_from_slice(p);
                }
                self.reals.extend_from_slice(knots);
            }
        }
        Ok(())
    }

    pub(crate) fn raw(&self) -> ffi::sweeps::List<'_> {
        ffi::sweeps::List::new(&self.kinds, &self.reals)
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: this thread's sweeps run on the C++ kernel whatever the routing.
    pub(crate) static ON_CPP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the sweeps run on solidcore ([`route::Op::Sweep`]).
fn on_core() -> bool {
    #[cfg(test)]
    if ON_CPP.with(std::cell::Cell::get) {
        return false;
    }
    route::Op::Sweep.backend() == route::Backend::Solidcore
}

impl Brep {
    /// A sheet of planar faces bounded by `loops`: closed loops of curves in one plane, in any
    /// order, nested as outer loops, their holes and islands in the holes.
    pub fn planar_faces(loops: &[Curve]) -> Result<Brep> {
        if on_core() {
            return core::sweep::planar_faces(loops).map(Brep::made_core);
        }
        init()?;
        let l = List::of(loops)?;
        ffi::sweeps::planar_faces(&l.raw()).map(Brep::made).map_err(error)
    }

    /// The planar `profile` extruded `distance` along the unit `direction` (square to its plane).
    /// `draft` (radians) leans the walls: positive inward, negative outward; a drafted profile is
    /// one closed loop, its curves in order, and `direction` is its normal (either way round).
    /// `corner` closes a drafted profile's sharp corners; a corner it cannot close is
    /// [`Error::Corner`] (with [`CornerRefusal::NotExact`] where, under
    /// [`DraftCorner::Curvature`], a curve of a profile with a sharp corner is neither a line nor
    /// an arc).
    pub fn extrude(profile: &[Curve], direction: [f64; 3], distance: f64, draft: f64, corner: DraftCorner, cap: bool) -> Result<Brep> {
        if on_core() {
            return core::sweep::extrude(profile, direction, distance, draft, corner, cap).map(Brep::made_core);
        }
        init()?;
        let l = List::of(profile)?;
        let rule = match corner {
            DraftCorner::Curvature => ffi::sweeps::DRAFT_CORNER_CURVATURE,
            DraftCorner::Tangent => ffi::sweeps::DRAFT_CORNER_TANGENT,
            DraftCorner::Round => ffi::sweeps::DRAFT_CORNER_ROUND,
        };
        let mut at = [f64::NAN; 3];
        ffi::sweeps::extrude(&l.raw(), &direction, distance, draft, rule, cap, &mut at).map(Brep::made).map_err(|s| {
            let why = match s {
                ffi::sweeps::ERR_CORNER_APART => Some(CornerRefusal::Apart),
                ffi::sweeps::ERR_WALL_CLOSES => Some(CornerRefusal::WallCloses),
                ffi::trim::ERR_NOT_EXACT => Some(CornerRefusal::NotExact),
                _ => None,
            };
            match why {
                Some(why) if at.iter().all(|x| x.is_finite()) => Error::Corner { why, at },
                _ => error(s),
            }
        })
    }

    /// The planar `profile` turned `angle` radians (0 < angle <= 2 pi) right-handed about the
    /// axis through `origin` along `direction`, in the profile's plane (touching it at most).
    pub fn revolve(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], angle: f64, cap: bool) -> Result<Brep> {
        if on_core() {
            return core::sweep::revolve(profile, origin, direction, angle, cap).map(Brep::made_core);
        }
        init()?;
        let l = List::of(profile)?;
        ffi::sweeps::revolve(&l.raw(), &origin, &direction, angle, cap).map(Brep::made).map_err(error)
    }

    /// `profile` swept along `path` (end-to-end curves), carried with the path's start: the
    /// profile keeps its place relative to it. The walls are B-spline approximations, one per
    /// stretch of profile between tangent breaks.
    pub fn sweep(profile: &[Curve], path: &[Curve], frame: SweepFrame, cap: bool) -> Result<Brep> {
        if on_core() {
            return core::sweep::sweep_placed(profile, path, frame, cap).map(|(b, _)| Brep::made_core(b));
        }
        init()?;
        let (l, p) = (List::of(profile)?, List::of(path)?);
        let frame = match frame {
            SweepFrame::Follow => ffi::sweeps::SWEEP_FOLLOW,
            SweepFrame::Translate => ffi::sweeps::SWEEP_TRANSLATE,
        };
        ffi::sweeps::sweep(&l.raw(), &p.raw(), frame, cap).map(Brep::made).map_err(error)
    }

    /// [`Brep::sweep`], with where the sweep placed the profile: the rigid motions carrying the
    /// profile as given onto its first section, a section about half way along and its last
    /// section. Only the sweep knows them (the curve sweep turns the profile on a frame it works
    /// out from point to point along the path); a sweep that carries on from where this one left
    /// the profile starts from the last.
    pub fn sweep_placed(profile: &[Curve], path: &[Curve], frame: SweepFrame, cap: bool) -> Result<(Brep, [Affine; 3])> {
        if on_core() {
            return core::sweep::sweep_placed(profile, path, frame, cap).map(|(b, m)| (Brep::made_core(b), m));
        }
        init()?;
        let (l, p) = (List::of(profile)?, List::of(path)?);
        let frame = match frame {
            SweepFrame::Follow => ffi::sweeps::SWEEP_FOLLOW,
            SweepFrame::Translate => ffi::sweeps::SWEEP_TRANSLATE,
        };
        let mut placed = [0.0; 36];
        let brep = ffi::sweeps::sweep_placed(&l.raw(), &p.raw(), frame, cap, &mut placed).map(Brep::made).map_err(error)?;
        let motion = |i: usize| {
            let at = |k: usize| placed.get(12 * i + k).copied().unwrap_or(0.0);
            Affine { m: [[at(0), at(1), at(2)], [at(3), at(4), at(5)], [at(6), at(7), at(8)]], t: [at(9), at(10), at(11)] }
        };
        Ok((brep, [motion(0), motion(1), motion(2)]))
    }

    /// The `curves` projected along `direction` (pointing from them toward the face) onto the
    /// face nearest `face_point` (within `pick_tolerance`; [`Error::NoMatch`] or
    /// [`Error::Ambiguous`] otherwise): SMLib's parallel projection onto the face's surface,
    /// trimmed to the face, each curve within `tolerance` of the true projection, as the NURBS
    /// SMLib made with its interval. No projection at all is [`Error::Empty`].
    pub fn project_curves(
        &self,
        face_point: [f64; 3],
        pick_tolerance: f64,
        curves: &[Curve],
        direction: [f64; 3],
        tolerance: f64,
    ) -> Result<Vec<(Curve, [f64; 2])>> {
        if on_core() {
            return core::sweep::project_curves(self.core()?, face_point, pick_tolerance, curves, direction, tolerance);
        }
        init()?;
        let l = List::of(curves)?;
        let out = ffi::sweeps::project_curves(self.cpp()?, &face_point, pick_tolerance, &l.raw(), &direction, tolerance).map_err(picked)?;
        nurbs_list(&out)
    }

    /// The section of the face nearest `face_point` by the plane through `plane_point` square to
    /// `plane_normal`, as the NURBS SMLib made with their intervals. None is [`Error::Empty`].
    pub fn face_section(
        &self,
        face_point: [f64; 3],
        pick_tolerance: f64,
        plane_point: [f64; 3],
        plane_normal: [f64; 3],
        tolerance: f64,
    ) -> Result<Vec<(Curve, [f64; 2])>> {
        if on_core() {
            return core::sweep::face_section(self.core()?, face_point, pick_tolerance, plane_point, plane_normal, tolerance);
        }
        init()?;
        let out = ffi::sweeps::face_section(self.cpp()?, &face_point, pick_tolerance, &plane_point, &plane_normal, tolerance).map_err(picked)?;
        nurbs_list(&out)
    }

    /// A loft (skin) through `sections` (at least two closed loops), `degree` (1 ruled, 2 or 3)
    /// across them.
    pub fn loft(sections: &[Vec<Curve>], degree: u32, cap: bool) -> Result<Brep> {
        if on_core() {
            return core::sweep::loft(sections, degree, cap).map(Brep::made_core);
        }
        init()?;
        if sections.len() < 2 || sections.len() > MAX_CURVES || !(1..=3).contains(&degree) {
            return Err(Error::Argument);
        }
        let l = List::of(sections.iter().flatten())?;
        let sizes: Vec<i64> = sections.iter().map(|s| i64::try_from(s.len()).map_err(|_| Error::Argument)).collect::<Result<_>>()?;
        let degree = i32::try_from(degree).map_err(|_| Error::Argument)?;
        ffi::sweeps::loft(&l.raw(), &sizes, degree, cap).map(Brep::made).map_err(error)
    }

    /// The planar `profile` placed by each of `stations` in turn (2…[`MAX_STATIONS`], each
    /// section exactly the profile's image), its walls skinned through the sections, cubic
    /// across them: one wall per profile curve. `ends` says how the ends close.
    pub fn sweep_stations(profile: &[Curve], stations: &[Affine], ends: Ends) -> Result<Brep> {
        if on_core() {
            return core::sweep::sweep_stations(profile, stations, ends).map(Brep::made_core);
        }
        init()?;
        if !(2..=MAX_STATIONS).contains(&stations.len()) || profile.len().saturating_mul(stations.len()) > MAX_STATION_CURVES {
            return Err(Error::Argument);
        }
        let l = List::of(profile)?;
        let maps: Vec<f64> = stations.iter().flat_map(|s| s.m.iter().flatten().chain(&s.t).copied()).collect();
        let ends = match ends {
            Ends::Open => ffi::sweeps::ENDS_OPEN,
            Ends::Capped => ffi::sweeps::ENDS_CAPPED,
            Ends::Joined => ffi::sweeps::ENDS_JOINED,
        };
        ffi::sweeps::sweep_stations(&l.raw(), &maps, ends).map(Brep::made).map_err(error)
    }

    /// A loft from the planar `section` (one closed loop of lines, in order) to the point `apex`
    /// off its plane: a pyramid, its base the section and a triangle on each line.
    pub fn loft_to_point(section: &[Curve], apex: [f64; 3]) -> Result<Brep> {
        if on_core() {
            return core::sweep::loft_to_point(section, apex).map(Brep::made_core);
        }
        init()?;
        let l = List::of(section)?;
        ffi::sweeps::loft_to_point(&l.raw(), &apex).map(Brep::made).map_err(error)
    }

    /// `profile` carried by a screw motion about the axis through `origin` along `direction`:
    /// `turns` right-handed turns about it, rising `pitch` (signed) per turn, capped at both ends,
    /// its walls within about `tolerance` of the true screw surfaces.
    pub fn screw_sweep(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], pitch: f64, turns: f64, tolerance: f64) -> Result<Brep> {
        if on_core() {
            return core::sweep::screw_sweep(profile, origin, direction, pitch, turns, None, tolerance).map(Brep::made_core);
        }
        init()?;
        let l = List::of(profile)?;
        ffi::sweeps::screw_sweep(&l.raw(), &origin, &direction, pitch, turns, tolerance).map(Brep::made).map_err(error)
    }

    /// [`screw_sweep`](Self::screw_sweep) with the profile moved off the axis as it turns: at turn
    /// t every point of it first moves `offset(t)` mm away from the axis (negative: towards it)
    /// in its half-plane, `offset` piecewise linear through the `(turn, offset)` pairs of `law`
    /// (turns strictly ascending within [0, `turns`], held before the first and after the last).
    /// A thread's groove fading in or out over a runout, or a taper. The profile must lie in a
    /// half-plane bounded by the axis.
    pub fn screw_sweep_radial(
        profile: &[Curve],
        origin: [f64; 3],
        direction: [f64; 3],
        pitch: f64,
        turns: f64,
        law: &[(f64, f64)],
        tolerance: f64,
    ) -> Result<Brep> {
        if on_core() {
            return core::sweep::screw_sweep(profile, origin, direction, pitch, turns, Some(law), tolerance).map(Brep::made_core);
        }
        init()?;
        let l = List::of(profile)?;
        let flat: Vec<f64> = law.iter().flat_map(|(t, o)| [*t, *o]).collect();
        ffi::sweeps::screw_sweep_radial(&l.raw(), &origin, &direction, pitch, turns, &flat, tolerance).map(Brep::made).map_err(error)
    }
}

/// The error of a call that picks faces by points.
fn picked(s: i32) -> Error {
    match s {
        ffi::ops::ERR_PICK_NONE => Error::NoMatch { point: 0 },
        ffi::ops::ERR_PICK_AMBIGUOUS => Error::Ambiguous { point: 0 },
        s => error(s),
    }
}

/// The NURBS curves of a curve list the C API made, each with its interval.
fn nurbs_list(out: &ffi::sweeps::OutCurves) -> Result<Vec<(Curve, [f64; 2])>> {
    let (mut k, mut r) = (0usize, 0usize);
    let mut curves = Vec::new();
    for ivl in out.intervals.as_chunks::<2>().0 {
        let (Some(&ffi::sweeps::CURVE_NURBS), Some(&degree), Some(&count)) = (out.kinds.get(k), out.kinds.get(k + 1), out.kinds.get(k + 2)) else {
            return Err(Error::Argument);
        };
        k += 3;
        let degree = usize::try_from(degree).map_err(|_| Error::Argument)?;
        let count = usize::try_from(count).map_err(|_| Error::Argument)?;
        let pts = out.reals.get(r..r + 4 * count).ok_or(Error::Argument)?;
        r += 4 * count;
        let knots = out.reals.get(r..r + count + degree + 1).ok_or(Error::Argument)?.to_vec();
        r += count + degree + 1;
        let points = pts.as_chunks::<4>().0.to_vec();
        curves.push((Curve::Nurbs { degree, points, knots }, [ivl[0], ivl[1]]));
    }
    if k != out.kinds.len() || r != out.reals.len() {
        return Err(Error::Argument);
    }
    Ok(curves)
}

#[cfg(test)]
#[path = "sweeps_tests.rs"]
mod tests;
