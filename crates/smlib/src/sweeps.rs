//! SMLib's sweeps: extrusion (straight or drafted), revolution, sweep along a path, loft through
//! sections or to a point, the screw sweep of a thread or coil, and the sweep of a profile placed
//! at stations (one that twists, scales or follows guide rails along its path). Profiles, paths and
//! sections are lists of
//! [`Curve`]s; each sweep returns a new body.
//!
//! With `cap`, the profile's loops close the ends and the result must be a closed manifold solid
//! ([`Error::NotSolid`] otherwise); without, it is the swept sheet.

use crate::{Brep, Error, Result, error, ffi, init};

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
struct List {
    kinds: Vec<i64>,
    reals: Vec<f64>,
}

impl List {
    fn of<'a>(curves: impl IntoIterator<Item = &'a Curve>) -> Result<List> {
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

    fn raw(&self) -> ffi::sweeps::List<'_> {
        ffi::sweeps::List::new(&self.kinds, &self.reals)
    }
}

impl Brep {
    /// The planar `profile` extruded `distance` along the unit `direction` (square to its plane).
    /// `draft` (radians) leans the walls: positive inward, negative outward; a drafted profile is
    /// one closed loop, its curves in order, and `direction` is its normal (either way round).
    pub fn extrude(profile: &[Curve], direction: [f64; 3], distance: f64, draft: f64, cap: bool) -> Result<Brep> {
        init()?;
        let l = List::of(profile)?;
        ffi::sweeps::extrude(&l.raw(), &direction, distance, draft, cap).map(Brep).map_err(error)
    }

    /// The planar `profile` turned `angle` radians (0 < angle <= 2 pi) right-handed about the
    /// axis through `origin` along `direction`, in the profile's plane (touching it at most).
    pub fn revolve(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], angle: f64, cap: bool) -> Result<Brep> {
        init()?;
        let l = List::of(profile)?;
        ffi::sweeps::revolve(&l.raw(), &origin, &direction, angle, cap).map(Brep).map_err(error)
    }

    /// `profile` swept along `path` (end-to-end curves), carried with the path's start: the
    /// profile keeps its place relative to it. The walls are B-spline approximations, one per
    /// stretch of profile between tangent breaks.
    pub fn sweep(profile: &[Curve], path: &[Curve], frame: SweepFrame, cap: bool) -> Result<Brep> {
        init()?;
        let (l, p) = (List::of(profile)?, List::of(path)?);
        let frame = match frame {
            SweepFrame::Follow => ffi::sweeps::SWEEP_FOLLOW,
            SweepFrame::Translate => ffi::sweeps::SWEEP_TRANSLATE,
        };
        ffi::sweeps::sweep(&l.raw(), &p.raw(), frame, cap).map(Brep).map_err(error)
    }

    /// A loft (skin) through `sections` (at least two closed loops), `degree` (1 ruled, 2 or 3)
    /// across them.
    pub fn loft(sections: &[Vec<Curve>], degree: u32, cap: bool) -> Result<Brep> {
        init()?;
        if sections.len() < 2 || sections.len() > MAX_CURVES || !(1..=3).contains(&degree) {
            return Err(Error::Argument);
        }
        let l = List::of(sections.iter().flatten())?;
        let sizes: Vec<i64> = sections.iter().map(|s| i64::try_from(s.len()).map_err(|_| Error::Argument)).collect::<Result<_>>()?;
        let degree = i32::try_from(degree).map_err(|_| Error::Argument)?;
        ffi::sweeps::loft(&l.raw(), &sizes, degree, cap).map(Brep).map_err(error)
    }

    /// The planar `profile` placed by each of `stations` in turn (2…[`MAX_STATIONS`], each
    /// section exactly the profile's image), its walls skinned through the sections, cubic
    /// across them: one wall per profile curve. `ends` says how the ends close.
    pub fn sweep_stations(profile: &[Curve], stations: &[Affine], ends: Ends) -> Result<Brep> {
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
        ffi::sweeps::sweep_stations(&l.raw(), &maps, ends).map(Brep).map_err(error)
    }

    /// A loft from the planar `section` (one closed loop of lines, in order) to the point `apex`
    /// off its plane: a pyramid, its base the section and a triangle on each line.
    pub fn loft_to_point(section: &[Curve], apex: [f64; 3]) -> Result<Brep> {
        init()?;
        let l = List::of(section)?;
        ffi::sweeps::loft_to_point(&l.raw(), &apex).map(Brep).map_err(error)
    }

    /// `profile` carried by a screw motion about the axis through `origin` along `direction`:
    /// `turns` right-handed turns about it, rising `pitch` (signed) per turn, capped at both ends,
    /// its walls within about `tolerance` of the true screw surfaces.
    pub fn screw_sweep(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], pitch: f64, turns: f64, tolerance: f64) -> Result<Brep> {
        init()?;
        let l = List::of(profile)?;
        ffi::sweeps::screw_sweep(&l.raw(), &origin, &direction, pitch, turns, tolerance).map(Brep).map_err(error)
    }
}

#[cfg(test)]
#[path = "sweeps_tests.rs"]
mod tests;
