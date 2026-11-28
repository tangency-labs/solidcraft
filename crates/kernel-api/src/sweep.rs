//! The inputs of sweeps and lofts ([`crate::Modelling::sweep`], [`crate::Modelling::loft`]):
//! paths as plain data, how the profile turns along them, how guide rails reshape it.

use solidcore_record::{Plane, Seg2, Vec3};

/// How the profile turns as it follows a sweep path.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SweepOrientation {
    /// Square to the path, turned along it by a rotation-minimising frame.
    #[default]
    Perpendicular,
    /// As drawn: the profile only moves along the path, never turning.
    Parallel,
    /// Square to the path, its turn about the path fixed by a direction: the direction's
    /// projection onto the profile's plane stays where it was at the start.
    Direction(Vec3),
}

/// How guide rails reshape the profile at each station.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RailFit {
    /// The profile grows or shrinks evenly with its distance across to the rail (between the
    /// two rails).
    #[default]
    Scale,
    /// The profile stretches only across to the rail (between the rails), keeping its size the
    /// other way.
    Stretch,
}

/// A rational Bézier span in space: 2 to 10 homogeneous control points `[x·w, y·w, z·w, w]`
/// with positive weights, on 0…1.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceSpan {
    /// The homogeneous control points.
    pub ctrl: Vec<[f64; 4]>,
}

/// A 3D path segment of a sweep.
#[derive(Clone, Debug, PartialEq)]
pub enum PathSpan {
    /// A line from `a` to `b`.
    Line {
        /// Its start.
        a: Vec3,
        /// Its end.
        b: Vec3,
    },
    /// An arc around `center`, turning by `angle` about `axis` (right hand) from `a`.
    Arc {
        /// Its start.
        a: Vec3,
        /// Its centre.
        center: Vec3,
        /// The axis it turns about.
        axis: Vec3,
        /// The angle it turns by, radians.
        angle: f64,
    },
    /// A free-form curve of a sketch (a Bézier or conic piece): `seg` on `plane`.
    Curve {
        /// The sketch plane.
        plane: Plane,
        /// The piece.
        seg: Seg2,
    },
    /// A rational Bézier span in space.
    Bezier(SpaceSpan),
}

/// One curve of a [`PointPath`], in the path's direction of travel.
#[derive(Clone, Debug, PartialEq)]
pub enum PointPiece {
    /// Points joined by straight lines.
    Polyline(Vec<Vec3>),
    /// A drawn 3D spline: the centripetal Catmull–Rom spline through `fit`, from its last fit
    /// point back to its first when `reversed`.
    Spline {
        /// The points it passes through.
        fit: Vec<Vec3>,
        /// Whether it runs from the last point to the first.
        reversed: bool,
    },
    /// A curve held exactly as path segments end to end.
    Spans(Vec<PathSpan>),
}

/// A sweep path through 3D sketch curves: its pieces end to end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointPath {
    /// The pieces, each starting where the one before ends.
    pub pieces: Vec<PointPiece>,
}

/// Sweep options beyond following the path: the profile's turn, a twist about the path over its
/// length (radians, right hand about the direction of travel), its size at the end (about the
/// path's start), and one or two guide rails (polylines from the end nearest the profile) with
/// how they reshape it. The default is a plain sweep.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepOptions {
    /// How the profile turns along the path.
    pub orientation: SweepOrientation,
    /// The twist over the path's length, radians.
    pub twist: f64,
    /// The profile's size at the end.
    pub scale: f64,
    /// Guide rails.
    pub rails: Vec<Vec<Vec3>>,
    /// How the rails reshape the profile.
    pub rail_fit: RailFit,
}

impl Default for SweepOptions {
    fn default() -> Self {
        SweepOptions { orientation: SweepOrientation::Perpendicular, twist: 0.0, scale: 1.0, rails: Vec::new(), rail_fit: RailFit::Scale }
    }
}

/// A helical sweep: `turns` revolutions about `axis` through `origin` (right-handed), rising
/// `pitch` per turn, the profile keeping its attitude to the axis as a screw motion does. With a
/// `law`, the profile moves off the axis as it turns (piecewise linear through its (turn,
/// offset) pairs), and `continuous_flanks` joins the spline walls across cylinder seams.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Helix<'a> {
    /// A point on the axis.
    pub origin: Vec3,
    /// The axis.
    pub axis: Vec3,
    /// The rise per turn.
    pub pitch: f64,
    /// How many turns.
    pub turns: f64,
    /// The radial law, if the profile moves off the axis.
    pub law: Option<&'a [(f64, f64)]>,
    /// With a law: spline walls joined across cylinder seams.
    pub continuous_flanks: bool,
}
