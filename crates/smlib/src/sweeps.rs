//! SMLib's sweeps: extrusion (straight or drafted), revolution, sweep along a path, loft through
//! sections or to a point, the screw sweep of a thread or coil, and the sweep of a profile placed
//! at stations (one that twists, scales or follows guide rails along its path). Profiles, paths and
//! sections are lists of [`Curve`]s; each sweep returns a new body.
//!
//! With `cap`, the profile's loops close the ends and the result must be a closed manifold solid
//! ([`Error::NotSolid`] otherwise); without, it is the swept sheet.

use serde_json::json;

use crate::trace;
use crate::{Brep, Result, core};

/// Screw-sweep topology and its approximation tolerance.
#[derive(Clone, Copy, Debug)]
pub struct ScrewSweepOptions<'a> {
    /// Optional radial law as `(turn, offset)` pairs.
    pub law: Option<&'a [(f64, f64)]>,
    /// Join spline walls across cylinder seams; false retains the historical faces.
    pub join_spline_walls: bool,
    /// Maximum wall approximation error, mm.
    pub tolerance: f64,
}

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

/// Most stations of one [`Brep::sweep_stations`].
pub const MAX_STATIONS: usize = 4096;
/// Most profile curves times stations of one [`Brep::sweep_stations`].
pub const MAX_STATION_CURVES: usize = 400_000;

/// Most curves one list may hold.
pub const MAX_CURVES: usize = 16_384;
/// Highest degree of a [`Curve::Nurbs`].
pub(crate) const CURVE_MAX_DEGREE: usize = 25;
/// Most control points of a [`Curve::Nurbs`].
pub(crate) const CURVE_MAX_POINTS: usize = 100_000;

impl Brep {
    /// A sheet of planar faces bounded by `loops`: closed loops of curves in one plane, in any
    /// order, nested as outer loops, their holes and islands in the holes.
    pub fn planar_faces(loops: &[Curve]) -> Result<Brep> {
        let r = core::sweep::planar_faces(loops).map(Brep::made_core);
        trace::made("planar_faces", &[], &r, || json!({"loops": trace::curves(loops)}));
        r
    }

    /// The planar `profile` extruded `distance` along the unit `direction` (square to its plane).
    /// `draft` (radians) leans the walls: positive inward, negative outward; a drafted profile is
    /// one closed loop, its curves in order, and `direction` is its normal (either way round).
    /// `corner` closes a drafted profile's sharp corners; a corner it cannot close is
    /// [`Error::Corner`] (with [`CornerRefusal::NotExact`] where, under
    /// [`DraftCorner::Curvature`], a curve of a profile with a sharp corner is neither a line nor
    /// an arc).
    pub fn extrude(profile: &[Curve], direction: [f64; 3], distance: f64, draft: f64, corner: DraftCorner, cap: bool) -> Result<Brep> {
        let r = core::sweep::extrude(profile, direction, distance, draft, corner, cap).map(Brep::made_core);
        trace::made("extrude", &[], &r, || {
            let corner = match corner {
                DraftCorner::Curvature => "curvature",
                DraftCorner::Tangent => "tangent",
                DraftCorner::Round => "round",
            };
            json!({"profile": trace::curves(profile), "direction": direction, "distance": distance, "draft": draft, "corner": corner, "cap": cap})
        });
        r
    }

    /// The planar `profile` turned `angle` radians (0 < angle <= 2 pi) right-handed about the
    /// axis through `origin` along `direction`, in the profile's plane (touching it at most).
    pub fn revolve(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], angle: f64, cap: bool) -> Result<Brep> {
        let r = core::sweep::revolve(profile, origin, direction, angle, cap).map(Brep::made_core);
        trace::made(
            "revolve",
            &[],
            &r,
            || json!({"profile": trace::curves(profile), "origin": origin, "direction": direction, "angle": angle, "cap": cap}),
        );
        r
    }

    /// `profile` swept along `path` (end-to-end curves), carried with the path's start: the
    /// profile keeps its place relative to it. The walls are B-spline approximations, one per
    /// stretch of profile between tangent breaks.
    pub fn sweep(profile: &[Curve], path: &[Curve], frame: SweepFrame, cap: bool) -> Result<Brep> {
        let r = core::sweep::sweep_placed(profile, path, frame, cap).map(|(b, _)| Brep::made_core(b));
        trace::made(
            "sweep",
            &[],
            &r,
            || json!({"profile": trace::curves(profile), "path": trace::curves(path), "frame": frame_word(frame), "cap": cap}),
        );
        r
    }

    /// [`Brep::sweep`], with where the sweep placed the profile: the rigid motions carrying the
    /// profile as given onto its first section, a section about half way along and its last
    /// section. Only the sweep knows them (the curve sweep turns the profile on a frame it works
    /// out from point to point along the path); a sweep that carries on from where this one left
    /// the profile starts from the last.
    pub fn sweep_placed(profile: &[Curve], path: &[Curve], frame: SweepFrame, cap: bool) -> Result<(Brep, [Affine; 3])> {
        let r = core::sweep::sweep_placed(profile, path, frame, cap).map(|(b, m)| (Brep::made_core(b), m));
        trace::call(
            "sweep",
            &[],
            &r,
            |(b, _)| vec![b.id],
            || json!({"profile": trace::curves(profile), "path": trace::curves(path), "frame": frame_word(frame), "cap": cap}),
        );
        r
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
        core::sweep::project_curves(self.core(), face_point, pick_tolerance, curves, direction, tolerance)
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
        core::sweep::face_section(self.core(), face_point, pick_tolerance, plane_point, plane_normal, tolerance)
    }

    /// A loft (skin) through `sections` (at least two closed loops), `degree` (1 ruled, 2 or 3)
    /// across them.
    pub fn loft(sections: &[Vec<Curve>], degree: u32, cap: bool) -> Result<Brep> {
        let r = core::sweep::loft(sections, degree, cap).map(Brep::made_core);
        trace::made(
            "loft",
            &[],
            &r,
            || json!({"sections": sections.iter().map(|s| trace::curves(s)).collect::<Vec<_>>(), "degree": degree, "cap": cap}),
        );
        r
    }

    /// The planar `profile` placed by each of `stations` in turn (2…[`MAX_STATIONS`], each
    /// section exactly the profile's image), its walls skinned through the sections, cubic
    /// across them: one wall per profile curve. `ends` says how the ends close.
    pub fn sweep_stations(profile: &[Curve], stations: &[Affine], ends: Ends) -> Result<Brep> {
        let r = core::sweep::sweep_stations(profile, stations, ends).map(Brep::made_core);
        trace::made("sweep_stations", &[], &r, || {
            let ends = match ends {
                Ends::Open => "open",
                Ends::Capped => "capped",
                Ends::Joined => "joined",
            };
            json!({"profile": trace::curves(profile), "stations": stations.iter().map(trace::affine).collect::<Vec<_>>(), "ends": ends})
        });
        r
    }

    /// A loft from the planar `section` (one closed loop of lines, in order) to the point `apex`
    /// off its plane: a pyramid, its base the section and a triangle on each line.
    pub fn loft_to_point(section: &[Curve], apex: [f64; 3]) -> Result<Brep> {
        let r = core::sweep::loft_to_point(section, apex).map(Brep::made_core);
        trace::made("loft_to_point", &[], &r, || json!({"section": trace::curves(section), "apex": apex}));
        r
    }

    /// `profile` carried by a screw motion about the axis through `origin` along `direction`:
    /// `turns` right-handed turns about it, rising `pitch` (signed) per turn, capped at both ends,
    /// its walls within about `tolerance` of the true screw surfaces.
    pub fn screw_sweep(profile: &[Curve], origin: [f64; 3], direction: [f64; 3], pitch: f64, turns: f64, tolerance: f64) -> Result<Brep> {
        let r = core::sweep::screw_sweep(profile, origin, direction, pitch, turns, None, tolerance, false).map(Brep::made_core);
        trace::made(
            "screw_sweep",
            &[],
            &r,
            || json!({"profile": trace::curves(profile), "origin": origin, "direction": direction, "pitch": pitch, "turns": turns, "tolerance": tolerance}),
        );
        r
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
        let r = core::sweep::screw_sweep(profile, origin, direction, pitch, turns, Some(law), tolerance, false).map(Brep::made_core);
        trace::made("screw_sweep", &[], &r, || {
            json!({"profile": trace::curves(profile), "origin": origin, "direction": direction, "pitch": pitch, "turns": turns,
                "law": law, "tolerance": tolerance})
        });
        r
    }

    /// A screw sweep with explicit face layout. The historical entry points retain
    /// half-turn faces; new thread features request continuous spline walls here.
    pub fn screw_sweep_with_options(
        profile: &[Curve],
        origin: [f64; 3],
        direction: [f64; 3],
        pitch: f64,
        turns: f64,
        options: ScrewSweepOptions<'_>,
    ) -> Result<Brep> {
        let r = core::sweep::screw_sweep(profile, origin, direction, pitch, turns, options.law, options.tolerance, options.join_spline_walls)
            .map(Brep::made_core);
        trace::made("screw_sweep", &[], &r, || {
            json!({
                "profile": trace::curves(profile), "origin": origin, "direction": direction,
                "pitch": pitch, "turns": turns, "law": options.law,
                "tolerance": options.tolerance, "join_spline_walls": options.join_spline_walls,
            })
        });
        r
    }
}

/// A sweep frame's name in a trace.
fn frame_word(f: SweepFrame) -> &'static str {
    match f {
        SweepFrame::Follow => "follow",
        SweepFrame::Translate => "translate",
    }
}

#[cfg(test)]
#[path = "sweeps_tests.rs"]
mod tests;
