//! Sweeps: planar faces, extrusion (straight or drafted), revolution, sweep along a path, loft
//! through sections or to a point, the sweep of a profile placed at stations, the screw sweep
//! of a thread or coil, and curves projected onto or sectioning a face (named by its record
//! index through the body's [`Order`]). Profiles, paths and sections are lists of [`Curve`]s;
//! each sweep returns a new body.
//!
//! Each call makes its curves from the list (the kernel's constructors), checks its numbers
//! before the kernel sees them, runs the kernel's sweep and hands the result over only when it
//! is what was asked for ([`finish_closed`], [`crate::Contract::Closed`]): a closed manifold
//! solid when the ends are capped, by the topology test alone, never a body without faces. No
//! proof is made, so a caller that wants the result proved valid and facing out asks for it
//! ([`Body::check_solid`]). Two calls differ: [`sweep_placed`] proves its solid and keeps the
//! proof, and [`curve_sweep`] runs under the contract its caller names.
//!
//! What a path is (one line, arcs about one axis, lines and arcs turning with it) is the
//! kernel's certified query; a path it leaves undecided is refused, naming the piece and the
//! comparison, never fitted instead ([`sweep_placed`]).
//!
//! The arcs of a profile on one circle: whether to make them one wall is the application's
//! rule; [`arcs_on_one_circle`], [`curves_meet`] and [`join_arcs`] answer and build for it.

// The range tests refuse NaN by negation (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]

use std::collections::BTreeMap;
use std::f64::consts::PI;

use solidcore::boolean::api_brep as kboolean;
use solidcore::curve::arc::{self, ArcDefinition};
use solidcore::curve::curve::{Curve as KCurve, CurveKind};
use solidcore::curve::from_parameters::record_of;
use solidcore::curve::line::Line;
use solidcore::curve::path::{self, PathDoubt, PathPiece, PathShape, PathTolerances};
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType, USE_DEFAULT};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::kernel::Kernel;
use solidcore::math::Transform;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::SkinnedSurfaceOptions;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::prism::profile as kp;
use solidcore::surface::surface::p4::NoObject;
use solidcore::surface::surface::Surface;
use solidcore::sweep::api_brep::{
    create_curve_sweep_with_placements, create_linear_sweep, create_rotational_sweep,
    CornerRefusal as KRefusal, OffsetCornerType, TaperExtrude,
};
use solidcore::sweep::api_surfaces::{
    create_skin_primitive, create_skin_primitive_with_ends, create_skin_primitive_with_sources,
    SkinSources,
};
use solidcore::sweep::directed;
use solidcore::sweep::extrude::{self as kextrude, Draft, ExtrudeTolerances};
use solidcore::sweep::screw;
use solidcore::sweep::segments::{
    build_faces, line_placements, sweep_along_segments, sweep_round_closed_segments,
    turn_placements,
};
use solidcore::sweep::skin_ends::SkinEnd;
use solidcore::topology::brep::p1::ParallelProjectionOptions;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{
    create_face_from_surface, create_planar_face_with_3d_curves,
    create_planar_faces_with_3d_curves, FaceBuildEnv,
};
use solidcore::topology::brep::planar_section::{PlanarSectionOptions, SectionOutputs};
use solidcore::topology::brep::validation::update_tolerances_after_boolean;
use solidcore::topology::{Brep, FaceId};
use solidcore::SolidError;
use solidcore_convert as convert;

pub use solidcore::curve::arc::{Join, OnOneCircle};
/// Which comparison of a path the kernel left undecided ([`Error::PathUndecided`]).
pub use solidcore::curve::path::PathCheck;
pub use solidcore::geom::certified::Verdict;
/// Why [`DraftCorner::Curvature`] could not close a corner ([`Error::DraftCornerRefused`]): the
/// kernel's own reasons.
pub use solidcore::sweep::api_brep::TaperCornerRefusal as CornerRefusal;

use crate::body::{finish_closed, finish_proved, finish_under, Body, Contract, Expect, Outward};
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{read_body, run, with_body};
use crate::{Error, Result};

/// A call's own refusal, else the value: as the kernel's calls answer inside [`run`].
type Out<T> = std::result::Result<T, Error>;

pub mod birail;
pub mod controlled;
pub mod railed;
mod record;

/// A coordinate no larger than this (mm, 1000 km), so that the kernel's tolerances stay
/// meaningful.
pub const MAX_COORD: f64 = 1e9;

/// Most reals a curve list may hold.
pub const FLAT_MAX_LEN: usize = 1 << 27;

/// Most stations of one [`sweep_stations`].
pub const MAX_STATIONS: usize = 4096;

/// Most profile curves times stations of one [`sweep_stations`].
pub const MAX_STATION_CURVES: usize = 400_000;

/// Most curves one list may hold.
pub const MAX_CURVES: usize = 16_384;

/// Highest degree of a [`Curve::Nurbs`].
pub const CURVE_MAX_DEGREE: usize = 25;

/// Most control points of a [`Curve::Nurbs`].
pub const CURVE_MAX_POINTS: usize = 100_000;

/// The steepest draft an extrude takes, radians: just short of a quarter turn.
#[allow(
    clippy::float_arithmetic,
    reason = "an input bound ops chooses, not a geometric quantity"
)]
const DRAFT_LIMIT: f64 = PI / 2.0 - 1e-6;

/// The largest angle a revolve takes, radians: a whole turn, to rounding.
#[allow(
    clippy::float_arithmetic,
    reason = "an input bound ops chooses, not a geometric quantity"
)]
const TURN_LIMIT: f64 = std::f64::consts::TAU * (1.0 + 1e-12);

/// Most turns of a screw sweep: the kernel's, as many as its spans hold
/// (`sweep::screw::MAX_TURNS`).
pub const SCREW_MAX_TURNS: f64 = screw::MAX_TURNS;

/// Most pairs of a screw sweep's radial law.
pub const SCREW_MAX_LAW: usize = 1024;

/// The tolerances a sweep path is read with: ends, and a straight or round path's points from
/// its line or circle, within 1e-9; segments leaving in the direction the one before arrives
/// in within 1e-12 of parallel; lines longer than 1e-12; open when its ends are more than 1e-6
/// apart (and closed round when they meet within that).
const PATH: PathTolerances = PathTolerances {
    gap: 1e-9,
    turn: 1e-12,
    length: 1e-12,
    closed: 1e-6,
};

/// The tolerances a drafted profile is checked with: one loop to 1e-6, square to the direction
/// to 1e-9.
const DRAFT: ExtrudeTolerances = ExtrudeTolerances {
    loop_gap: 1e-6,
    parallel: 1e-9,
};

/// A 3D curve of a profile, path or section.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    /// From `start` to `end`.
    Line {
        /// Its start.
        start: [f64; 3],
        /// Its end.
        end: [f64; 3],
    },
    /// Counter-clockwise about the unit `axis` from angle `start` to `end` (radians,
    /// 0 < end - start <= 2 pi), angle 0 along the unit `reference` (square to `axis`).
    Arc {
        /// The centre.
        center: [f64; 3],
        /// The axis.
        axis: [f64; 3],
        /// The direction of angle 0.
        reference: [f64; 3],
        /// The radius.
        radius: f64,
        /// The start angle, radians.
        start: f64,
        /// The end angle, radians.
        end: f64,
    },
    /// Control points (x, y, z, weight > 0) and the clamped knot vector (points + degree + 1
    /// knots).
    Nurbs {
        /// The degree.
        degree: usize,
        /// The control points and weights.
        points: Vec<[f64; 4]>,
        /// The knots.
        knots: Vec<f64>,
    },
}

impl Curve {
    /// A planar profile segment placed on `plane` as a sweep curve.
    ///
    /// Lines and arcs stay analytic: a line keeps its two ends, and an arc keeps
    /// its centre, radius and angles, with a negative sweep expressed about the
    /// reversed normal. Cubic and higher Bézier segments become non-rational
    /// [`Curve::Nurbs`] curves; a conic becomes a rational quadratic NURBS that
    /// keeps its weight. Every control point is placed through the plane's
    /// sketch frame: `plane.x` is sketch x, `plane.y` is sketch y and
    /// `plane.origin` is sketch zero.
    ///
    /// Returns `None` for a zero-sweep arc, a conic whose weight is not finite
    /// and positive, and a Bézier whose degree is outside `1..=`[`solidcore_record::MAX_BEZIER_DEGREE`].
    #[must_use]
    pub fn from_seg2(
        segment: &solidcore_record::Seg2,
        plane: &solidcore_record::Plane,
    ) -> Option<Curve> {
        let v3 = |p: Point3| [p.x, p.y, p.z];
        // the kernel places the segment ([`solidcore::prism::profile::seg_on_frame`]); this
        // is its record, field for field
        Some(
            match kp::seg_on_frame(&convert::plane(plane), 0.0, &convert::seg2(segment))? {
                kp::PlacedSeg::Line { a, b } => Curve::Line {
                    start: v3(a),
                    end: v3(b),
                },
                kp::PlacedSeg::Arc {
                    center,
                    axis,
                    reference,
                    radius,
                    start,
                    end,
                } => Curve::Arc {
                    center: v3(center),
                    axis: v3(axis),
                    reference: v3(reference),
                    radius,
                    start,
                    end,
                },
                kp::PlacedSeg::Nurbs {
                    degree,
                    points,
                    knots,
                } => Curve::Nurbs {
                    degree,
                    points,
                    knots,
                },
            },
        )
    }
}

/// `radians` in degrees, for the kernel calls that take degrees: `radians * 180 / PI`.
#[allow(
    clippy::float_arithmetic,
    reason = "the kernel call takes degrees: a unit conversion, not geometry (ruled in #3145)"
)]
fn degrees(radians: f64) -> f64 {
    radians * 180.0 / PI
}

/// How a drafted [`extrude`] closes the gap its offset opens at a sharp corner of the profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DraftCorner {
    /// Each wall continues as itself (a line's plane, an arc's cone) until it meets its
    /// neighbour.
    #[default]
    Curvature,
    /// Each offset curve continues straight along its end tangent.
    Tangent,
    /// An arc about the corner.
    Round,
}

/// Which sweep a [`sweep_placed`] took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SweepRoute {
    /// The linear sweep along a path proved straight: exact.
    Line,
    /// The rotational sweep along arcs proved about one axis: exact.
    Arc,
    /// The sweep piece by piece along lines and arcs proved end to end (open, or closed
    /// round a loop) and turning with it: exact.
    Segments,
    /// The kernel's curve sweep along the path joined into one curve, carried as the frame
    /// says. With [`SweepFrame::Translate`] it is exact for the joined curve; with
    /// [`SweepFrame::Follow`] or [`SweepFrame::Upright`] it is fitted to a tolerance and
    /// carries no certified bound (see [`sweep_placed`]).
    Curve(SweepFrame),
}

/// How the profile is carried along a [`sweep_placed`] path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SweepFrame {
    /// It turns with the path, keeping its attitude to the path's tangent: the path's
    /// rotation-minimising frame (exact along lines and arcs; the kernel's projection-normal
    /// frame along any other path, which the kernel brings back to its start round a closed
    /// one, so the sheet closes).
    Follow,
    /// It only moves along the path, keeping its orientation.
    Translate,
    /// It stays upright: square to the path, keeping the up direction (any length; the
    /// kernel makes it unit) made square to the path's tangent, so it turns only about the up
    /// direction as the path turns (a screen or a canopy along an outline). Drawn at the
    /// path's start, it is swept as drawn there. A path running along the up direction is
    /// refused.
    Upright([f64; 3]),
}

/// How the ends of a [`sweep_stations`] close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    /// Left open: the swept sheet.
    Open,
    /// Capped by the first and last sections: a closed solid.
    Capped,
    /// Joined: the last section lies on the first, the walls' ends glued to their starts.
    Joined,
}

/// An affine map: a point `x` goes to `m[0] x + m[1] y + m[2] z + t` (`m[i]` the image of
/// axis `i`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    /// The images of the axes.
    pub m: [[f64; 3]; 3],
    /// The translation.
    pub t: [f64; 3],
}

impl Affine {
    fn of(t: &Transform) -> Affine {
        let r = |i: usize| t.m.get(i).map_or([0.0; 3], |row| [row[0], row[1], row[2]]);
        Affine {
            m: [r(0), r(1), r(2)],
            t: r(3),
        }
    }

    fn transform(&self) -> Transform {
        let [x, y, z] = self.m;
        let row = |a: [f64; 3], w: f64| [a[0], a[1], a[2], w];
        Transform {
            m: [row(x, 0.0), row(y, 0.0), row(z, 0.0), row(self.t, 1.0)],
        }
    }
}

/// Every coordinate finite and within [`MAX_COORD`].
pub(crate) fn coord_ok(p: &[f64]) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn called<T>(r: solidcore::Result<T>) -> Out<T> {
    r.map_err(status)
}

/// The kernel's refusal of a number this family passed on as given (a direction, a segment's
/// ends: `solidcore::sweep::directed`) as [`Error::Argument`], any other error as its status.
fn directed_or_status(e: SolidError) -> Error {
    match e {
        SolidError::InvalidInput { operation, .. } if operation == directed::OP => Error::Argument,
        e => status(e),
    }
}

/// A kernel refusal of the arguments as [`Error::Argument`], any other error as its status.
fn argument_or_status(e: SolidError) -> Error {
    match e {
        SolidError::InvalidInput { .. } => Error::Argument,
        e => status(e),
    }
}

fn line(start: [f64; 3], end: [f64; 3]) -> Out<KCurve> {
    if !coord_ok(&start) || !coord_ok(&end) {
        return Err(Error::Argument);
    }
    directed::segment_apart(v3(start), v3(end), 1e-9)
        .map(KCurve::from)
        .map_err(directed_or_status)
}

/// An arc's definition as the kernel takes it.
fn definition(
    center: [f64; 3],
    axis: [f64; 3],
    reference: [f64; 3],
    radius: f64,
    start: f64,
    end: f64,
) -> ArcDefinition {
    ArcDefinition {
        centre: v3(center),
        axis: v3(axis),
        reference: v3(reference),
        radius,
        start,
        end,
    }
}

/// An arc's piece: its curve made from its definition by the kernel ([`PathPiece::of_arc`]).
/// TODO(d0b44726f): an exact circle at bound 0 by its label until the kernel proves it.
fn arc(k: &Kernel, d: &ArcDefinition) -> Out<PathPiece> {
    let c = [d.centre.x, d.centre.y, d.centre.z];
    if !coord_ok(&c) || !(d.radius < MAX_COORD) || d.start.abs() > 1e6 {
        return Err(Error::Argument);
    }
    PathPiece::of_arc(k, d).map_err(argument_or_status)
}

fn nurbs(degree: usize, points: &[[f64; 4]], knots: &[f64]) -> Out<KCurve> {
    let count = points.len();
    if !(1..=CURVE_MAX_DEGREE).contains(&degree) || count < degree + 1 || count > CURVE_MAX_POINTS {
        return Err(Error::Argument);
    }
    let mut ps = Vec::new();
    let mut weights = Vec::new();
    let mut rational = false;
    for p in points {
        if !coord_ok(&p[..3]) || !p[3].is_finite() || !(p[3] > 1e-12 && p[3] < 1e12) {
            return Err(Error::Argument);
        }
        ps.push(Vec3::new(p[0], p[1], p[2]));
        weights.push(p[3]);
        rational = rational || p[3] != 1.0;
    }
    if knots.len() != count + degree + 1 || !knots.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    let (mut unique, mut mult): (Vec<f64>, Vec<usize>) = (Vec::new(), Vec::new());
    for (i, &u) in knots.iter().enumerate() {
        match (i.checked_sub(1).and_then(|j| knots.get(j)), mult.last_mut()) {
            (Some(&prev), _) if u < prev => return Err(Error::Argument),
            (Some(&prev), Some(m)) if u == prev => *m += 1,
            _ => {
                unique.push(u);
                mult.push(1);
            }
        }
    }
    let (Some(&first), Some(&last), Some(&m0), Some(&m1)) =
        (unique.first(), unique.last(), mult.first(), mult.last())
    else {
        return Err(Error::Argument);
    };
    #[allow(
        clippy::float_arithmetic,
        reason = "an input check: the knots span more than 1e-12"
    )]
    let spans = last - first > 1e-12;
    if unique.len() < 2 || m0 != degree + 1 || m1 != degree + 1 || !spans {
        return Err(Error::Argument);
    }
    if mult
        .iter()
        .skip(1)
        .take(mult.len().saturating_sub(2))
        .any(|&m| m > degree)
    {
        return Err(Error::Argument);
    }
    let data = CanonicalData {
        dim: 3,
        degree,
        control_points: &ps,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &unique,
        knot_type: KnotType::Unspecified,
        weights: rational.then_some(weights.as_slice()),
        trim: None,
    };
    called(BSplineCurve::create_canonical(&data)).map(KCurve::from)
}

/// A curve list made: each piece (an arc with its definition), and the curves alone.
struct Made {
    pieces: Vec<PathPiece>,
    curves: Vec<KCurve>,
}

impl Made {
    fn refs(&self) -> Vec<Option<&KCurve>> {
        self.curves.iter().map(Some).collect()
    }
}

/// The curves of a curve list. A list of none, more than [`MAX_CURVES`] or more reals than
/// [`FLAT_MAX_LEN`] is [`Error::Argument`].
fn parse(k: &Kernel, curves: &[Curve]) -> Out<Made> {
    if curves.is_empty() || curves.len() > MAX_CURVES {
        return Err(Error::Argument);
    }
    let mut reals = 0usize;
    let mut made = Made {
        pieces: Vec::new(),
        curves: Vec::new(),
    };
    for c in curves {
        let piece = match c {
            Curve::Line { start, end } => {
                reals += 6;
                PathPiece::of_curve(line(*start, *end)?)
            }
            Curve::Arc {
                center,
                axis,
                reference,
                radius,
                start,
                end,
            } => {
                reals += 12;
                if ![*radius, *start, *end].iter().all(|x| x.is_finite()) {
                    return Err(Error::Argument);
                }
                arc(
                    k,
                    &definition(*center, *axis, *reference, *radius, *start, *end),
                )?
            }
            Curve::Nurbs {
                degree,
                points,
                knots,
            } => {
                // (the degree, the counts and the knots are checked by `nurbs`)
                reals = reals
                    .saturating_add(points.len().saturating_mul(5))
                    .saturating_add(knots.len());
                if reals > FLAT_MAX_LEN {
                    return Err(Error::Argument);
                }
                PathPiece::of_curve(nurbs(*degree, points, knots)?)
            }
        };
        if reals > FLAT_MAX_LEN {
            return Err(Error::Argument);
        }
        made.curves.push(piece.curve().clone());
        made.pieces.push(piece);
    }
    Ok(made)
}

/// The kernel's curves of a curve list, as [`parse`] makes them, for the families outside this
/// module that take one (a face trimmed along curves on it, `crate::trim`).
pub(crate) fn kernel_curves(
    k: &Kernel,
    curves: &[Curve],
) -> std::result::Result<Vec<KCurve>, Error> {
    parse(k, curves).map(|made| made.curves)
}

/// Runs a call that makes a new body.
fn making(
    call: &'static str,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<Out<Brep>>,
) -> Result<Body> {
    run(call, f)?.map(Body::new)
}

/// [`making`], the new body keeping the proof its result test made ([`finish_proved`],
/// [`finish_under`]), so the next proved operation that takes it does not prove it again.
fn making_held(
    call: &'static str,
    f: impl FnOnce(&mut Kernel) -> solidcore::Result<Out<(Brep, Option<Outward>)>>,
) -> Result<Body> {
    run(call, f)?.map(|(b, proof)| Body::held(b, proof))
}

/// A curve list's own refusal inside a kernel call.
macro_rules! parsed {
    ($e:expr) => {
        match $e {
            Ok(c) => c,
            Err(e) => return Ok(Err(e)),
        }
    };
}

/// A sheet of planar faces bounded by the closed `loops` (`SmApiCreatePlanarFaces`).
///
/// # Errors
/// [`Error::Argument`] for a curve list refused; the kernel's status; [`Error::Empty`].
pub fn planar_faces(loops: &[Curve]) -> Result<Body> {
    making("planar_faces", |k| {
        let made = parsed!(parse(k, loops));
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let region = b.infinite_region();
        // (the tolerance is not used: the kernel takes the B-rep's)
        let built = build_faces(&mut b, |b, fenv| {
            create_planar_faces_with_3d_curves(k, b, region, &made.curves, 0.01, fenv)
        });
        if let Err(e) = built {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish_closed(k, b, Expect::FACES)
    })
}

/// The model's zone tolerance, within which consecutive profile curves meet: the tolerance
/// the arcs of a straight extrusion's profile are compared with.
///
/// # Errors
/// Those of the session.
pub fn zone_tolerance() -> Result<f64> {
    run("zone_tolerance", |k| {
        Ok(context_zone_tol_3d(k, None, SmallTopoType::NormalSize))
    })
}

/// Whether two arcs of a curve list lie on one circle to `tol` (certified), and whether they
/// share a frame; `None` when either is no arc.
///
/// # Errors
/// [`Error::Argument`] for an arc without an axis or reference.
pub fn arcs_on_one_circle(a: &Curve, b: &Curve, tol: f64) -> Result<Option<OnOneCircle>> {
    let (Some(a), Some(b)) = (arc_definition(a), arc_definition(b)) else {
        return Ok(None);
    };
    arc::arcs_on_one_circle(&a, &b, tol)
        .map(Some)
        .map_err(argument_or_status)
}

/// Whether the end of curve `a` meets the start of curve `b` to `tol` (certified).
///
/// # Errors
/// [`Error::Argument`] for a curve refused; the kernel's error.
pub fn curves_meet(a: &Curve, b: &Curve, tol: f64) -> Result<Verdict> {
    run("curves_meet", |k| {
        let made = parsed!(parse(k, &[a.clone(), b.clone()]));
        let (Some(x), Some(y)) = (made.curves.first(), made.curves.get(1)) else {
            return Ok(Err(Error::Argument));
        };
        arc::curves_meet(k, x, y, tol).map(Ok)
    })?
}

/// The one arc of a run of arcs on one circle (end to end, proved to `tol`), open or round the
/// full circle with its seam as `join` says.
///
/// # Errors
/// [`Error::Argument`] for a run that is not arcs proved on one circle and end to end.
pub fn join_arcs(run_of: &[Curve], join: Join, tol: f64) -> Result<Curve> {
    run("join_arcs", |k| {
        let made = parsed!(parse(k, run_of));
        Ok(match arc::join_arcs(k, &made.pieces, join, tol) {
            Ok((d, _)) => Ok(Curve::Arc {
                center: [d.centre.x, d.centre.y, d.centre.z],
                axis: [d.axis.x, d.axis.y, d.axis.z],
                reference: [d.reference.x, d.reference.y, d.reference.z],
                radius: d.radius,
                start: d.start,
                end: d.end,
            }),
            Err(e) => Err(argument_or_status(e)),
        })
    })?
}

/// `profile` with each run of consecutive arcs on one circle made one arc ([`join_arcs`]), so
/// that the run's wall is one face as a single arc's is: a circle drawn in pieces (split where
/// another curve ends on it) sweeps the one cylinder it lies on, not a face per piece meeting
/// the next along a line that is no edge of the shape.
///
/// The profile's loops are its runs of curves end to end ([`curves_meet`] to `tol`), each
/// closed where its last curve meets its first. Two consecutive arcs of a loop join where they
/// meet end to end and lie on one circle ([`arcs_on_one_circle`]), both proved to `tol`; a
/// verdict the kernel leaves undecided joins nothing. A run that is its whole loop is the full
/// circle, its seam at angle 0 of the pieces' frame when they share one, else at the middle of
/// the longest piece ([`Join`]); any other run is the open arc through its pieces. A run the
/// kernel's join refuses stays as its pieces. Every other curve is as given, in order.
///
/// # Errors
/// [`Error::Argument`] for a curve list refused; the kernel's error.
pub fn join_circle_runs(profile: &[Curve], tol: f64) -> Result<Vec<Curve>> {
    run("join_circle_runs", |k| {
        let made = parsed!(parse(k, profile));
        let n = made.curves.len();
        let meets = |i: usize, j: usize| -> solidcore::Result<bool> {
            Ok(match (made.curves.get(i), made.curves.get(j)) {
                (Some(a), Some(b)) => arc::curves_meet(k, a, b, tol)? == Verdict::Yes,
                _ => false,
            })
        };
        let circle = |i: usize, j: usize| -> solidcore::Result<Option<OnOneCircle>> {
            match (
                made.pieces.get(i).and_then(PathPiece::arc),
                made.pieces.get(j).and_then(PathPiece::arc),
            ) {
                (Some(a), Some(b)) => arc::arcs_on_one_circle(a, b, tol).map(Some),
                _ => Ok(None),
            }
        };
        let joins = |i: usize, j: usize| -> solidcore::Result<bool> {
            Ok(meets(i, j)? && circle(i, j)?.is_some_and(|c| c.circle == Verdict::Yes))
        };
        // The one arc of the pieces `run` (indices of `profile`), as `join` says; `None` where
        // the kernel's join refuses it.
        let joined = |run: &[usize], join: Join| -> solidcore::Result<Option<Curve>> {
            let pieces: Vec<PathPiece> = run
                .iter()
                .filter_map(|&i| made.pieces.get(i).cloned())
                .collect();
            Ok(match arc::join_arcs(k, &pieces, join, tol) {
                Ok((d, _)) => Some(Curve::Arc {
                    center: [d.centre.x, d.centre.y, d.centre.z],
                    axis: [d.axis.x, d.axis.y, d.axis.z],
                    reference: [d.reference.x, d.reference.y, d.reference.z],
                    radius: d.radius,
                    start: d.start,
                    end: d.end,
                }),
                Err(SolidError::InvalidInput { .. }) => None,
                Err(e) => return Err(e),
            })
        };
        let given = |run: &[usize]| -> Vec<Curve> {
            run.iter()
                .filter_map(|&i| profile.get(i).cloned())
                .collect()
        };
        let mut out = Vec::with_capacity(n);
        let mut first = 0;
        while first < n {
            // the loop from `first`: its curves end to end until one ends at its start
            let mut last = first;
            while last + 1 < n && !meets(last, first)? && meets(last, last + 1)? {
                last += 1;
            }
            let len = last - first + 1;
            let closed = meets(last, first)?;
            let at = |j: usize| first + j % len;
            // whether two of the loop's arcs go on on one circle
            let mut any = false;
            let mut j = 0;
            while !any && (j + 1 < len || (closed && len > 1 && j < len)) {
                any = joins(at(j), at(j + 1))?;
                j += 1;
            }
            if !any {
                out.extend(given(&(first..=last).collect::<Vec<_>>()));
                first = last + 1;
                continue;
            }
            // a run starts where its curve does not go on from the one before it round the loop
            let mut begin = 0;
            if closed {
                while begin < len && joins(at(begin + len - 1), at(begin))? {
                    begin += 1;
                }
            }
            if begin == len {
                // the whole loop: the full circle, its seam where the pieces' shared frame
                // starts, else at the middle of the longest
                let run: Vec<usize> = (first..=last).collect();
                let mut one_frame = true;
                for &i in &run {
                    one_frame =
                        one_frame && circle(first, i)?.is_some_and(|c| c.frame == Verdict::Yes);
                }
                let join = if one_frame {
                    Join::ClosedAtFrameStart
                } else {
                    Join::ClosedAtMiddleOfLongest
                };
                match joined(&run, join)? {
                    Some(c) => out.push(c),
                    None => out.extend(given(&run)),
                }
            } else {
                let mut kk = 0;
                while kk < len {
                    // the arcs from here round the loop that go on on its circle
                    let mut m = 1;
                    while kk + m < len && joins(at(begin + kk + m - 1), at(begin + kk + m))? {
                        m += 1;
                    }
                    let run: Vec<usize> = (0..m).map(|x| at(begin + kk + x)).collect();
                    if m == 1 {
                        out.extend(given(&run));
                    } else {
                        match joined(&run, Join::Open)? {
                            Some(c) => out.push(c),
                            None => out.extend(given(&run)),
                        }
                    }
                    kk += m;
                }
            }
            first = last + 1;
        }
        Ok(Ok(out))
    })?
}

fn arc_definition(c: &Curve) -> Option<ArcDefinition> {
    match c {
        Curve::Arc {
            center,
            axis,
            reference,
            radius,
            start,
            end,
        } => Some(definition(
            *center, *axis, *reference, *radius, *start, *end,
        )),
        _ => None,
    }
}

/// Join contiguous pieces of the same circular profile arc before sweeping one wall over them.
///
/// Only arcs with identical circle definitions are candidates. The kernel's round-path query
/// must certify the whole run on its first circle, including its accumulated end angles.
/// Other curves and runs not certified are kept as given. Closed loops are handled separately,
/// including runs crossing a loop's starting index. A single curve keeps its seam unchanged.
/// Callers retain the original profile for construction history.
///
/// # Errors
/// [`Error::Argument`] for invalid or oversized curve lists; errors of the kernel queries and
/// arc construction.
pub fn join_profile_arcs(profile: &[Curve]) -> Result<Vec<Curve>> {
    fn same_circle(a: &Curve, b: &Curve) -> bool {
        matches!((a, b), (
            Curve::Arc { center: a, axis: u, reference: x, radius: r, .. },
            Curve::Arc { center: b, axis: v, reference: y, radius: s, .. }
        ) if a == b && u == v && x == y && r == s)
    }
    run("join_profile_arcs", |k| {
        let made = parsed!(parse(k, profile));
        let tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
        let piece = |i: usize| {
            made.pieces
                .get(i)
                .ok_or_else(|| SolidError::invalid_input("join_profile_arcs", "piece out of range"))
        };
        let meets = |i, j| {
            arc::curves_meet(k, piece(i)?.curve(), piece(j)?.curve(), tol).map(Verdict::is_yes)
        };
        let joins = |i, j| -> solidcore::Result<bool> {
            Ok(
                matches!(profile.get(i).zip(profile.get(j)), Some((a, b)) if same_circle(a, b))
                    && meets(i, j)?,
            )
        };
        let mut out = Vec::new();
        let mut first = 0;
        while first < profile.len() {
            let mut last = first;
            while last + 1 < profile.len() && !meets(last, first)? && meets(last, last + 1)? {
                last += 1;
            }
            let len = last - first + 1;
            let at = |i: usize| first + i % len;
            let mut begin = 0;
            if len > 1 && meets(last, first)? {
                while begin < len && joins(at(begin + len - 1), at(begin))? {
                    begin += 1;
                }
                if begin == len {
                    begin = 0;
                }
            }
            let mut loop_out = Vec::new();
            let mut merged = false;
            let mut used = 0;
            while used < len {
                let mut count = 1;
                while used + count < len
                    && joins(at(begin + used + count - 1), at(begin + used + count))?
                {
                    count += 1;
                }
                let indices = (0..count).map(|j| at(begin + used + j));
                if count > 1 {
                    let pieces = indices
                        .clone()
                        .map(|i| piece(i).cloned())
                        .collect::<solidcore::Result<Vec<_>>>()?;
                    let tolerances = PathTolerances { gap: tol, ..PATH };
                    if matches!(
                        path::path_as_arc(k, &pieces, &tolerances)?,
                        PathShape::Is(_)
                    ) {
                        let (d, _) = arc::join_arcs(k, &pieces, Join::Open, tol)?;
                        loop_out.push(Curve::Arc {
                            center: [d.centre.x, d.centre.y, d.centre.z],
                            axis: [d.axis.x, d.axis.y, d.axis.z],
                            reference: [d.reference.x, d.reference.y, d.reference.z],
                            radius: d.radius,
                            start: d.start,
                            end: d.end,
                        });
                        merged = true;
                    } else {
                        loop_out.extend(indices.filter_map(|i| profile.get(i)).cloned());
                    }
                } else {
                    loop_out.extend(indices.filter_map(|i| profile.get(i)).cloned());
                }
                used += count;
            }
            if merged {
                out.extend(loop_out);
            } else {
                out.extend(profile.get(first..=last).into_iter().flatten().cloned());
            }
            first = last + 1;
        }
        Ok(Ok(out))
    })?
}

/// The planar `profile` extruded `distance` along `direction` (any length; square to the
/// profile's plane), straight (`draft` 0) or drafted (`draft` radians; positive inward), its
/// corners closed by `corner`. The arcs of a straight extrusion's profile are taken as given:
/// an application that wants a circle drawn in pieces to sweep one wall joins them first
/// ([`join_arcs`]).
///
/// # Errors
/// [`Error::Argument`] for a number or curve list refused, or a drafted profile not one closed
/// loop square to the direction; [`Error::DraftCornerRefused`] for a refused corner; the kernel's status;
/// [`Error::Empty`], [`Error::NotSolid`].
pub fn extrude(
    profile: &[Curve],
    direction: [f64; 3],
    distance: f64,
    draft: f64,
    corner: DraftCorner,
    cap: bool,
) -> Result<Body> {
    if !distance.is_finite()
        || !(distance > 1e-9 && distance < MAX_COORD)
        || !draft.is_finite()
        || draft.abs() >= DRAFT_LIMIT
    {
        return Err(Error::Argument);
    }
    making("extrude", |k| {
        let made = parsed!(parse(k, profile));
        let drafted = (draft != 0.0).then(|| Draft {
            angle_deg: degrees(draft),
            corner: match corner {
                DraftCorner::Curvature => OffsetCornerType::CurveExtension,
                DraftCorner::Tangent => OffsetCornerType::LinearExtension,
                DraftCorner::Round => OffsetCornerType::FilletCorner,
            },
        });
        let expect = if cap { Expect::SOLID } else { Expect::FACES };
        match kextrude::extrude(
            k,
            &made.curves,
            v3(direction),
            distance,
            drafted,
            cap,
            &DRAFT,
        ) {
            Ok(TaperExtrude::Solid(r)) => finish_closed(k, r, expect),
            Ok(TaperExtrude::Refused(r)) => Ok(Err(refused(r))),
            Err(SolidError::InvalidInput { operation, .. }) if operation == kextrude::OP => {
                Ok(Err(Error::Argument))
            }
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// A refused draft corner: [`Error::DraftCornerRefused`], always, with the corner's point when
/// it is finite.
fn refused(r: KRefusal) -> Error {
    let at = [r.corner.x, r.corner.y, r.corner.z];
    Error::DraftCornerRefused {
        why: r.reason,
        at: at.iter().all(|x| x.is_finite()).then_some(at),
    }
}

/// The planar `profile` turned `angle` radians (0 < angle <= 2 pi) right-handed about the axis
/// through `origin` along `axis` (`SmApiCreateRotationalSweep`).
///
/// # Errors
/// [`Error::Argument`] for a number or curve list refused; the kernel's status;
/// [`Error::Empty`], [`Error::NotSolid`].
pub fn revolve(
    profile: &[Curve],
    origin: [f64; 3],
    axis: [f64; 3],
    angle: f64,
    cap: bool,
) -> Result<Body> {
    if !coord_ok(&origin) || !angle.is_finite() || !(angle > 1e-9 && angle <= TURN_LIMIT) {
        return Err(Error::Argument);
    }
    making("revolve", |k| {
        let made = parsed!(parse(k, profile));
        // a whole turn at most
        let degrees = degrees(angle).min(360.0);
        match directed::revolve(k, &made.refs(), v3(origin), v3(axis), degrees, cap) {
            Ok(r) => finish_closed(k, r, if cap { Expect::SOLID } else { Expect::FACES }),
            Err(e) => Ok(Err(directed_or_status(e))),
        }
    })
}

/// A curve's B-spline (lines and circles are B-splines too); `None` for another kind.
fn bspline(c: &KCurve) -> solidcore::Result<Option<BSplineCurve>> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => Some(b.clone()),
        CurveKind::Line(l) => Some(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Some(e.base().clone()),
        _ => None,
    })
}

/// A path of end-to-end curves as one B-spline (the curve sweep takes one curve); `None` for a
/// curve with no B-spline form. The kernel's error of the join is returned as it is.
fn one_bspline(path: &[KCurve]) -> solidcore::Result<Option<BSplineCurve>> {
    let mut parts = Vec::new();
    for c in path {
        let Some(b) = bspline(c)? else {
            return Ok(None);
        };
        parts.push(b);
    }
    if parts.len() == 1 {
        return Ok(parts.pop());
    }
    BSplineCurve::create_by_joining(&mut parts, None).map(Some)
}

/// [`one_bspline`], a curve with no B-spline form [`Error::Argument`] and the join's error its
/// status.
fn joined(path: &[KCurve]) -> Out<BSplineCurve> {
    match one_bspline(path) {
        Ok(Some(b)) => Ok(b),
        Ok(None) => Err(Error::Argument),
        Err(e) => Err(status(e)),
    }
}

/// The identity placements of a sweep that has not run.
fn unplaced() -> [Affine; 3] {
    [Affine::of(&Transform::IDENTITY); 3]
}

/// `profile` swept round `path` turning with it ([`SweepFrame::Follow`]), when the path is one
/// [`sweep_placed`] sweeps piece by piece round a closed loop: lines and arcs proved end to end
/// round one closed loop, each joining the next (and the last the first) tangentially, and not
/// arcs about one axis (those are the rotational sweep). Then the solid (with `cap`; else the
/// sheet), as [`sweep_placed`] gives it along that path, with the motions that placed the profile
/// at the start, half way round and back at the start; `None` for any other path or a curve list
/// refused, which [`sweep_placed`] answers. For a caller with a sweep of its own along the other
/// paths, so that round a closed path of lines and arcs both sweep the same way.
///
/// Round a closed path each closed loop of the profile sweeps a tube closing on itself, with no
/// cap to join an outline to its holes: so for a solid each loop is swept on its own and the
/// holes' tubes (the loops after the first, in the profile's order) are taken from the first's.
///
/// # Errors
/// The kernel's status; [`Error::NotSolid`] for walls that do not close into a solid with `cap`;
/// the boolean's errors taking a hole's tube away.
pub fn sweep_round_closed_path(
    profile: &[Curve],
    path: &[Curve],
    cap: bool,
) -> Result<Option<(Body, [Affine; 3])>> {
    let made = run("sweep_round_closed_path", |k| {
        let (Ok(made), Ok(along)) = (parse(k, profile), parse(k, path)) else {
            return Ok(None);
        };
        let lines: Vec<&KCurve> = along.curves.iter().collect();
        if path::path_as_line(k, &lines, &PATH)? != PathShape::Not
            || path::path_as_arc(k, &along.pieces, &PATH)? != PathShape::Not
        {
            return Ok(None);
        }
        let PathShape::Is(segs) = path::path_as_closed_segments(k, &along.pieces, &PATH)? else {
            return Ok(None);
        };
        let groups = if cap {
            loops_of(k, &made.curves)?
        } else {
            std::iter::once(0..made.curves.len()).collect()
        };
        let mut tubes = Vec::with_capacity(groups.len());
        for g in groups {
            let Some(curves) = made.curves.get(g) else {
                return Ok(Some(Err(Error::Argument)));
            };
            match round_closed_segments(k, curves, &segs, cap)? {
                Ok(t) => tubes.push(t),
                Err(e) => return Ok(Some(Err(e))),
            }
        }
        Ok(Some(Ok(tubes)))
    })?;
    let tubes = match made {
        None => return Ok(None),
        Some(r) => r?,
    };
    let mut tubes = tubes.into_iter();
    let Some((first, placed)) = tubes.next() else {
        return Err(Error::Argument);
    };
    let mut whole = Body::new(first);
    for (hole, _) in tubes {
        whole = match crate::boolean::boolean(whole, Body::new(hole), crate::BooleanOp::Subtract)? {
            Some(b) => b,
            None => return Err(Error::Empty),
        };
    }
    Ok(Some((whole, placed)))
}

/// The runs of `curves` that each close a loop, in order: each run's curves proved end to end,
/// the last's end proved to meet the first's start (to the path gap). One run of all of them
/// when they do not split into closed loops.
fn loops_of(k: &Kernel, curves: &[KCurve]) -> solidcore::Result<Vec<std::ops::Range<usize>>> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, c) in curves.iter().enumerate() {
        let Some(first) = curves.get(from) else {
            break;
        };
        if arc::curves_meet(k, c, first, PATH.gap)?.is_yes() {
            out.push(from..i + 1);
            from = i + 1;
        }
    }
    if from != curves.len() || out.is_empty() {
        return Ok(std::iter::once(0..curves.len()).collect());
    }
    Ok(out)
}

/// The profile's `curves` swept round the closed `segments` ([`sweep_round_closed_segments`]),
/// finished as a solid with `cap` (else a sheet), with its motions.
fn round_closed_segments(
    k: &mut Kernel,
    curves: &[KCurve],
    segs: &[path::Segment],
    cap: bool,
) -> solidcore::Result<Out<(Brep, [Affine; 3])>> {
    let expect = if cap { Expect::SOLID } else { Expect::FACES };
    match sweep_round_closed_segments(k, curves, segs, cap, &PATH) {
        Ok((b, motions)) => {
            Ok(finish_proved(k, b, expect)?.map(|(b, _)| (b, motions.map(|t| Affine::of(&t)))))
        }
        Err(e) => Ok(Err(status(e))),
    }
}

/// `profile` swept along `path`, with the rigid motions that placed it at the start, half way
/// and the end, and the route taken ([`SweepRoute`]). Along a path proved straight the sweep
/// is the linear sweep; with [`SweepFrame::Follow`], along arcs proved about one axis the
/// rotational sweep, and along lines and arcs proved end to end the exact sweep piece by
/// piece. Any other path takes the kernel's curve sweep, once every join of the path is proved
/// to meet (to the path gap).
///
/// With [`SweepFrame::Upright`] a straight path is the linear sweep as with any frame, and any
/// other path the kernel's curve sweep kept upright.
///
/// The curve sweep is exact for [`SweepFrame::Translate`] (for the path joined into one curve)
/// but not for [`SweepFrame::Follow`]: there it samples the path and skins the sections it
/// places, to a tolerance its own sampling can widen, and returns no bound (the kernel's curve
/// sweep, filed as a solidcore issue). So with `Follow` a path the kernel leaves undecided for
/// every exact shape it is tried as is refused ([`Error::PathUndecided`]) rather than fitted;
/// a path of other curves, or of lines and arcs proved to be none of those shapes (a corner, a
/// closed path), keeps the curve sweep until that issue is fixed.
///
/// # Errors
/// [`Error::Argument`] for a curve list refused or an up direction not finite;
/// [`Error::PathUndecided`] for a `Follow` path
/// left undecided; [`Error::PathGap`] for a curve-sweep path whose join is not proved to meet,
/// or whose ends neither meet to the path gap nor are proved apart (the closing join, the last);
/// the kernel's status (an upright path running along its up direction: [`Error::Unsupported`]);
/// [`Error::Empty`], [`Error::NotSolid`].
pub fn sweep_placed(
    profile: &[Curve],
    path: &[Curve],
    frame: SweepFrame,
    cap: bool,
) -> Result<(Body, [Affine; 3], SweepRoute)> {
    if let SweepFrame::Upright(u) = frame {
        if !u.iter().all(|x| x.is_finite()) {
            return Err(Error::Argument);
        }
    }
    let mut placed = unplaced();
    let mut route = SweepRoute::Curve(frame);
    let expect = if cap { Expect::SOLID } else { Expect::FACES };
    let body = making_held("sweep", |k| {
        let (made, along) = match (parse(k, profile), parse(k, path)) {
            (Ok(c), Ok(p)) => (c, p),
            (Err(e), _) | (_, Err(e)) => return Ok(Err(e)),
        };
        let refs = made.refs();
        let lines: Vec<&KCurve> = along.curves.iter().collect();
        // The first comparison an exact shape was left undecided on.
        let mut doubt: Option<PathDoubt> = None;
        match path::path_as_line(k, &lines, &PATH)? {
            PathShape::Is(l) => {
                let r = match create_linear_sweep(k, &refs, l.direction, l.length, cap) {
                    Ok(r) => r,
                    Err(e) => return Ok(Err(status(e))),
                };
                placed = line_placements(l.direction, l.length).map(|t| Affine::of(&t));
                route = SweepRoute::Line;
                return finish_proved(k, r, expect);
            }
            PathShape::Undecided(d) => doubt = Some(d),
            PathShape::Not => {}
        }
        let pieces = &along.pieces;
        if frame == SweepFrame::Follow {
            match path::path_as_arc(k, pieces, &PATH)? {
                PathShape::Is(a) => {
                    // a whole turn at most
                    let degrees = degrees(a.angle).min(360.0);
                    let r = match create_rotational_sweep(k, &refs, a.centre, a.axis, degrees, cap)
                    {
                        Ok(r) => r,
                        Err(e) => return Ok(Err(status(e))),
                    };
                    placed = match turn_placements(a.centre, a.axis, degrees) {
                        Ok(t) => t.map(|t| Affine::of(&t)),
                        Err(_) => return Ok(Err(Error::Argument)),
                    };
                    route = SweepRoute::Arc;
                    return finish_proved(k, r, expect);
                }
                PathShape::Undecided(d) => doubt = doubt.or(Some(d)),
                PathShape::Not => {}
            }
            match path::path_as_segments(k, pieces, &PATH)? {
                PathShape::Is(segs) => {
                    return match sweep_along_segments(k, &made.curves, &segs, cap, &PATH) {
                        Ok((b, motions)) => {
                            placed = motions.map(|t| Affine::of(&t));
                            route = SweepRoute::Segments;
                            finish_proved(k, b, expect)
                        }
                        Err(e) => Ok(Err(status(e))),
                    };
                }
                PathShape::Undecided(d) => doubt = doubt.or(Some(d)),
                PathShape::Not => {}
            }
            if let Some(PathDoubt { piece, check }) = doubt {
                return Ok(Err(Error::PathUndecided { piece, check }));
            }
        }
        // The curve sweep joins the path into one curve, which would close gaps far wider than
        // the path's: every join must be proved to meet first.
        for (i, (a, b)) in along
            .curves
            .iter()
            .zip(along.curves.iter().skip(1))
            .enumerate()
        {
            if !arc::curves_meet(k, a, b, PATH.gap)?.is_yes() {
                return Ok(Err(Error::PathGap { join: i }));
            }
        }
        let path_curve = match joined(&along.curves) {
            Ok(b) => KCurve::from(b),
            Err(e) => return Ok(Err(e)),
        };
        // Round a closed path the profile comes back onto itself: the walls close on each other
        // there, where caps would lie on top of each other inside the solid. Closed only when its
        // ends are proved to meet to the path gap, open only when proved apart by more than
        // PATH.closed; between the two the closing join is a gap the stitch would close.
        let (Some(head), Some(tail)) = (along.curves.first(), along.curves.last()) else {
            return Ok(Err(Error::Argument));
        };
        let closed = if arc::curves_meet(k, tail, head, PATH.gap)?.is_yes() {
            true
        } else if arc::curves_meet(k, tail, head, PATH.closed)? == Verdict::No {
            false
        } else {
            return Ok(Err(Error::PathGap {
                join: along.curves.len().saturating_sub(1),
            }));
        };
        let options = SweepOptions {
            cap_ends: cap && !closed,
            translational_sweep: frame == SweepFrame::Translate,
            profile_at_path_start: true,
            up: match frame {
                SweepFrame::Upright(u) => Some(v3(u)),
                SweepFrame::Follow | SweepFrame::Translate => None,
            },
            ..SweepOptions::default()
        };
        Ok(
            match curve_swept(k, &made.curves, &path_curve, closed, cap, &options) {
                Ok(Ok((b, motions))) => {
                    placed = motions;
                    finish_proved(k, b, expect)?
                }
                Ok(Err(e)) => Err(e),
                Err(e) => Err(status(e)),
            },
        )
    });
    let body = match body {
        Ok(b) => b,
        // Round a closed path of lines and arcs the curve sweep cannot close (a wall shrinking
        // to an edge): the same exact pieces, the walls closing on each other. Where the curve
        // sweep builds, its result stays.
        Err(e) if frame == SweepFrame::Follow => {
            match sweep_round_closed_path(profile, path, cap) {
                Ok(Some((b, placed))) => return Ok((b, placed, SweepRoute::Segments)),
                Ok(None) | Err(_) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };
    Ok((body, placed, route))
}

/// The kernel's curve sweep of the profile `curves` along `path` (one curve), with the
/// placements of the profile's first loop at the start, half way and the end
/// ([`SweepRoute::Curve`]), before the result contract; a kernel error of any step as it is.
///
/// Round a `closed` path the walls close on each other: each tube is stitched along where
/// they meet, and a solid asked for (`cap`) must have no free edge left. There, capped, each
/// loop of the profile is swept on its own: the curve sweep makes every loop's tube bound a
/// solid of its own, so a hole's tube would add its volume instead of taking it away. The
/// holes' tubes, the loops after the first as a region lists them ([`path::loop_runs`], to
/// [`PathTolerances::closed`]), are subtracted from the first loop's ([`subtracted`]).
fn curve_swept(
    k: &mut Kernel,
    curves: &[KCurve],
    path: &KCurve,
    closed: bool,
    cap: bool,
    options: &SweepOptions,
) -> solidcore::Result<Out<(Brep, [Affine; 3])>> {
    let mut placed = unplaced();
    let runs = if closed && cap {
        path::loop_runs(k, curves, PATH.closed)?
    } else {
        std::iter::once(0..curves.len()).collect()
    };
    let mut whole: Option<Brep> = None;
    for (i, run) in runs.into_iter().enumerate() {
        let Some(curves) = curves.get(run) else {
            return Ok(Err(Error::Argument));
        };
        let refs: Vec<Option<&KCurve>> = curves.iter().map(Some).collect();
        let r =
            create_curve_sweep_with_placements(k, &refs, Some(path), None, None, Some(options))?;
        if i == 0 {
            for (m, p) in placed.iter_mut().zip(&r.placements) {
                *m = Affine::of(&Transform::of_placement(p));
            }
        }
        let mut b = r.sweep.brep;
        if closed {
            let tol = b.tolerance();
            let s = stitch_faces(k, &mut b, tol, None, false)?;
            // a solid asked for has no free edge left; a sheet (an open profile, or one not
            // capped) keeps the profile's own ends free
            if cap && s.lamina_edges != 0 {
                return Ok(Err(Error::NotSolid));
            }
            b.refresh_cached_topology();
        }
        whole = Some(match whole {
            None => b,
            Some(outer) => subtracted(k, outer, b)?,
        });
    }
    Ok(whole.map(|b| (b, placed)).ok_or(Error::Argument))
}

/// `a` less `b`, the kernel's boolean (both consumed), as [`curve_swept`] takes a hole's tube
/// from the outer loop's: the tubes are swept in one kernel along one path, so no tolerance
/// matching first; then the tolerance update a boolean's result gets
/// ([`update_tolerances_after_boolean`]).
fn subtracted(k: &mut Kernel, a: Brep, b: Brep) -> solidcore::Result<Brep> {
    let mut r = kboolean::boolean(k, a, b, BooleanOperation::Difference)?.result;
    update_tolerances_after_boolean(k, &mut r)?;
    Ok(r)
}

/// How near a [`curve_sweep`] path's last end must come to its first start for the path to
/// be closed (the plain distance, [`path::ends_meet`]).
pub const CURVE_SWEEP_CLOSED: f64 = 1e-6;

/// `profile` swept along `path` by the kernel's curve sweep alone ([`SweepRoute::Curve`]),
/// whatever shape the path has, with the rigid motions that placed the profile (its first
/// loop) at the start, half way and the end; the result under `contract`. For a caller that
/// picks the exact routes itself (the linear, rotational and piecewise sweeps
/// [`sweep_placed`] tries first), or wants the curve sweep where [`sweep_placed`] would take
/// one of them.
///
/// The path's curves are joined end to end into one B-spline as given, with no join proved
/// to meet first: [`sweep_placed`] refuses a path whose joins it cannot prove
/// ([`Error::PathGap`]), this takes the join the kernel makes. The path is closed when its
/// last end lies within [`CURVE_SWEEP_CLOSED`] of its first start; round a closed path the
/// result has no caps, the walls closing on each other, and a capped profile of several loops
/// has its holes (the loops after the first) taken away, each loop swept on its own, as
/// [`sweep_placed`] does round a closed path.
///
/// # Errors
/// [`Error::Argument`] for a curve list refused, a path curve the sweep cannot join, or an up
/// direction not finite; the kernel's status; [`Error::Empty`]; [`Error::NotSolid`] (with
/// `cap`: a result that is not a solid under `contract`, or round a closed path a tube left
/// with a free edge); under [`Contract::Proved`], [`Error::NotSolidBecause`] too.
///
/// # Examples
///
/// A 4 x 2 rectangle with a 2 x 1 hole swept round a closed rounded rectangle (60 x 40,
/// corners of radius 8): the hole's tube is taken from the outer one's.
///
/// ```
/// use solidcore_ops::body::Contract;
/// use solidcore_ops::sweep::{curve_sweep, Curve, SweepFrame};
///
/// let line = |a: [f64; 2], b: [f64; 2]| Curve::Line { start: [a[0], a[1], 0.0], end: [b[0], b[1], 0.0] };
/// let corner = |c: [f64; 2], r: [f64; 3]| Curve::Arc {
///     center: [c[0], c[1], 0.0], axis: [0.0, 0.0, 1.0], reference: r, radius: 8.0,
///     start: 0.0, end: std::f64::consts::FRAC_PI_2,
/// };
/// let path = vec![
///     line([0.0, -20.0], [22.0, -20.0]), corner([22.0, -12.0], [0.0, -1.0, 0.0]),
///     line([30.0, -12.0], [30.0, 12.0]), corner([22.0, 12.0], [1.0, 0.0, 0.0]),
///     line([22.0, 20.0], [-22.0, 20.0]), corner([-22.0, 12.0], [0.0, 1.0, 0.0]),
///     line([-30.0, 12.0], [-30.0, -12.0]), corner([-22.0, -12.0], [-1.0, 0.0, 0.0]),
///     line([-22.0, -20.0], [0.0, -20.0]),
/// ];
/// // a rectangle square to the path where it starts, (0, -20, 0)
/// let rect = |w: f64, h: f64| {
///     let p = [[0.0, -20.0 - w, -h], [0.0, -20.0 + w, -h], [0.0, -20.0 + w, h], [0.0, -20.0 - w, h]];
///     (0..4).map(|i| Curve::Line { start: p[i], end: p[(i + 1) % 4] }).collect::<Vec<_>>()
/// };
/// let sweep = |p: &[Curve]| curve_sweep(p, &path, SweepFrame::Follow, true, Contract::Closed);
/// let (holed, _placed) = sweep(&[rect(2.0, 1.0), rect(1.0, 0.5)].concat())?;
/// let (outer, _) = sweep(&rect(2.0, 1.0))?;
/// let (hole, _) = sweep(&rect(1.0, 0.5))?;
/// let (h, o, i) = (holed.volume(1e-8)?, outer.volume(1e-8)?, hole.volume(1e-8)?);
/// assert!((h - (o - i)).abs() < 1e-6 * o, "{h} = {o} - {i}");
/// # Ok::<(), solidcore_ops::Error>(())
/// ```
pub fn curve_sweep(
    profile: &[Curve],
    path: &[Curve],
    frame: SweepFrame,
    cap: bool,
    contract: Contract,
) -> Result<(Body, [Affine; 3])> {
    let mut placed = unplaced();
    let expect = if cap { Expect::SOLID } else { Expect::FACES };
    let body = making_held("curve_sweep", |k| {
        let (made, along) = match (parse(k, profile), parse(k, path)) {
            (Ok(c), Ok(p)) => (c, p),
            (Err(e), _) | (_, Err(e)) => return Ok(Err(e)),
        };
        Ok(
            match curve_sweep_in(k, &made.curves, &along.curves, frame, cap) {
                Ok(Ok((b, motions))) => {
                    placed = motions;
                    finish_under(k, b, expect, contract)?
                }
                Ok(Err(e)) => Err(e),
                Err(e) => Err(status(e)),
            },
        )
    })?;
    Ok((body, placed))
}

/// [`curve_sweep`] in the kernel `k`, of the curves made from its lists, before the result
/// contract: the swept B-rep and the placements.
///
/// # Errors
/// The kernel's error of any step, as it is (the outer error). Inside: [`Error::Argument`]
/// for no profile or path, a path curve with no B-spline form or an up direction not finite;
/// [`Error::NotSolid`] for a capped tube round a closed path left with a free edge.
fn curve_sweep_in(
    k: &mut Kernel,
    profile: &[KCurve],
    path: &[KCurve],
    frame: SweepFrame,
    cap: bool,
) -> solidcore::Result<Result<(Brep, [Affine; 3])>> {
    if let SweepFrame::Upright(u) = frame {
        if !u.iter().all(|x| x.is_finite()) {
            return Ok(Err(Error::Argument));
        }
    }
    if profile.is_empty() || path.is_empty() {
        return Ok(Err(Error::Argument));
    }
    let Some(path_curve) = one_bspline(path)? else {
        return Ok(Err(Error::Argument));
    };
    let path_curve = KCurve::from(path_curve);
    let closed = path::ends_meet(k, path, CURVE_SWEEP_CLOSED)?;
    let options = SweepOptions {
        cap_ends: cap && !closed,
        translational_sweep: frame == SweepFrame::Translate,
        profile_at_path_start: true,
        up: match frame {
            SweepFrame::Upright(u) => Some(v3(u)),
            SweepFrame::Follow | SweepFrame::Translate => None,
        },
        ..SweepOptions::default()
    };
    curve_swept(k, profile, &path_curve, closed, cap, &options)
}

/// Whether an ordered sketch chain's last and first endpoints meet within `tol`.
/// Empty chains do not close; interior joins are not rechecked here.
#[must_use]
pub fn loft_section_closed(chain: &[solidcore_record::Seg2], tol: f64) -> bool {
    use solidcore::sweep::loft_sections::section_ends_meet;
    chain
        .first()
        .zip(chain.last())
        .is_some_and(|(first, last)| {
            section_ends_meet(&convert::seg2(first), &convert::seg2(last), tol)
        })
}

/// A closed loft section for [`match_loft_sections`]: its sketch plane and its loop in the
/// plane's coordinates, end to end.
#[derive(Clone, Debug)]
pub struct LoftSection {
    /// The section's plane.
    pub plane: solidcore_record::Plane,
    /// The loop, in the plane's coordinates.
    pub segs: Vec<solidcore_record::Seg2>,
}

/// A section as a loft's walls are built through it ([`match_loft_sections`]): its segments,
/// and for each the indices of the given section's segments it runs over (increasing).
#[derive(Clone, Debug, PartialEq)]
pub struct MatchedSection {
    /// The segments, in the plane's coordinates.
    pub segs: Vec<solidcore_record::Seg2>,
    /// For each segment, the given segments it runs over.
    pub from: Vec<Vec<usize>>,
}

/// A loft's closed sections, in loft order, matched as its walls are built through them (the
/// kernel's `sweep::loft_sections::match_loft_sections`): each wound the way the one before it
/// is, cut to the vertices it shares with the others (a corner pairs with the corner it lines up
/// with, a vertex only one section has splits the others' segments where it lands, a full circle
/// splits at the directions of the section it follows), and started at the segment that joins the
/// first segment of the section before it. `None` when the match would give a section more than
/// `max_segments` segments, or the sections are too large to pair: valid sections the caller
/// builds another way.
///
/// # Errors
/// [`Error::Argument`] for a section with no segments, no length or a point that is not finite,
/// plane axes that move its points by more than the zone tolerance (their skew from a frame of
/// perpendicular unit vectors times the section's extent past 1e-5), or a circle whose centre,
/// radius or angles are not finite; [`Error::Invariant`] when the pairing breaks down.
pub fn match_loft_sections(
    sections: &[LoftSection],
    max_segments: usize,
) -> Result<Option<Vec<MatchedSection>>> {
    use solidcore::sweep::loft_sections as kl;
    let given: Vec<kl::LoftSection> = sections
        .iter()
        .map(|s| kl::LoftSection {
            origin: convert::vec3(s.plane.origin),
            x: convert::vec3(s.plane.x),
            y: convert::vec3(s.plane.y),
            segs: s.segs.iter().map(convert::seg2).collect(),
        })
        .collect();
    let matched = match kl::match_loft_sections(&given, max_segments) {
        Err(e) if kl::too_large(&e) => return Ok(None),
        r => r.map_err(argument_or_status)?,
    };
    Ok(Some(
        matched
            .into_iter()
            .map(|m| MatchedSection {
                segs: m.segs.iter().map(convert::record_seg2).collect(),
                from: m.from,
            })
            .collect(),
    ))
}

/// For a loft's sections as its walls are built through them (each section's segments in its
/// plane's coordinates, the same number in every section, in the order [`loft`] takes their
/// curves), which segments the skin should take joined to the segment before them, as one curve
/// (the kernel's `sweep::loft_sections::one_curve_runs`): `true` at `i` for segment `i` of every
/// section. Those are the free-form segments that in every section carry on the one before them
/// with one first derivative on the spans that keep the first section's speed, in a run with a
/// joint where the speeds differ. A caller that hands such a run to [`loft`] as one B-spline on
/// those spans, every section on the first section's knots, gets the same walls without a jump
/// of their first derivative where the segments meet.
#[must_use]
pub fn loft_one_curve_runs(sections: &[Vec<solidcore_record::Seg2>]) -> Vec<bool> {
    use solidcore::sweep::loft_sections as kl;
    let given: Vec<Vec<kp::Seg2>> = sections
        .iter()
        .map(|s| s.iter().map(convert::seg2).collect())
        .collect();
    kl::one_curve_runs(&given)
}

/// A skin through `sections` (`SmApiCreateSkinPrimitive`), `degree` (1 to 3) across them.
///
/// # Errors
/// [`Error::Argument`] for a section or number refused; the kernel's status; [`Error::Empty`],
/// [`Error::NotSolid`].
pub fn loft(sections: &[Vec<Curve>], degree: u32, cap: bool) -> Result<Body> {
    if sections.len() < 2 || sections.len() > MAX_CURVES || !(1..=3).contains(&degree) {
        return Err(Error::Argument);
    }
    let all: Vec<Curve> = sections.iter().flatten().cloned().collect();
    making("loft", |k| {
        let made = parsed!(parse(k, &all));
        if sections
            .iter()
            .any(|s| s.is_empty() || s.len() > MAX_CURVES)
        {
            return Ok(Err(Error::Argument));
        }
        let counts: Vec<usize> = sections.iter().map(Vec::len).collect();
        if let Some(planar) = planar_sides(k, &counts, &made.curves, cap)? {
            return Ok(planar);
        }
        match create_skin_primitive(k, &counts, &made.refs(), degree as usize, cap) {
            Ok(r) => finish_closed(k, r.brep, if cap { Expect::SOLID } else { Expect::FACES }),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// A loft of two sections of lines in equal number (three or more), each one closed loop,
/// whose every side (a line of the first section, its partner in the second and the lines
/// between their ends) is flat: the sides as planar faces on their lines and, with `cap`, the
/// sections as the caps, stitched and oriented. A side between parallel lines of different
/// lengths (a tapered duct) is a planar trapezoid, which the skin would make a bilinear
/// B-spline surface. `None` for any other loft, or where the kernel does not prove a section
/// closed, a side's lines apart (more than 1e-9) or its fourth corner on the plane of the other
/// three (the kernel's [`path::point_off_plane_of`] at 1e-9 of its size), or where the planar
/// faces do not build or stitch: the skin takes it.
fn planar_sides(
    k: &mut Kernel,
    counts: &[usize],
    curves: &[KCurve],
    cap: bool,
) -> solidcore::Result<Option<Out<Brep>>> {
    let &[n0, n1] = counts else {
        return Ok(None);
    };
    if n0 != n1
        || n0 < 3
        || curves.len() != n0 + n1
        || curves
            .iter()
            .any(|c| !matches!(c.kind(), CurveKind::Line(_)))
    {
        return Ok(None);
    }
    let (first, second) = curves.split_at(n0);
    if !path::closed_loop(k, first, 1e-6)?.is_yes() || !path::closed_loop(k, second, 1e-6)?.is_yes()
    {
        return Ok(None);
    }
    let (mut bottom, mut top) = (Vec::new(), Vec::new());
    for c in first {
        bottom.push(c.ends(k)?.start);
    }
    for c in second {
        top.push(c.ends(k)?.start);
    }
    let corner = |pts: &[Point3], i: usize| pts.get(i % n0).copied();
    for i in 0..n0 {
        let (Some(a0), Some(a1), Some(b0), Some(b1)) = (
            corner(&bottom, i),
            corner(&bottom, i + 1),
            corner(&top, i),
            corner(&top, i + 1),
        ) else {
            return Ok(None);
        };
        // each side's lines apart, and its fourth corner on the plane of the other three
        let (Ok(s0), Ok(s1), Ok(s2), Ok(_)) = (
            directed::segment_apart(a0, a1, 1e-9),
            directed::segment_apart(a1, b1, 1e-9),
            directed::segment_apart(b1, a0, 1e-9),
            directed::segment_apart(b1, b0, 1e-9),
        ) else {
            return Ok(None);
        };
        let triangle = [KCurve::from(s0), KCurve::from(s1), KCurve::from(s2)];
        if path::point_off_plane_of(k, &triangle, b0, 1e-9).ok() != Some(Verdict::No) {
            return Ok(None);
        }
    }
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    let region = b.infinite_region();
    let tol = 1e-6;
    let built = build_faces(&mut b, |b, fenv| -> Out<()> {
        let mut face = |b: &mut Brep, fenv: &mut FaceBuildEnv<'_, '_>, pts: &[Point3]| -> Out<()> {
            let mut edges = Vec::new();
            for (i, &p) in pts.iter().enumerate() {
                let q = pts.get(i + 1).or_else(|| pts.first()).copied().unwrap_or(p);
                edges.push(KCurve::from(called(Line::create_line_segment(
                    3, p, q, None,
                ))?));
            }
            called(create_planar_face_with_3d_curves(
                k, b, region, &edges, tol, fenv,
            ))
            .map(|_| ())
        };
        if cap {
            face(b, fenv, &bottom)?;
            face(b, fenv, &top)?;
        }
        for i in 0..n0 {
            let (Some(a0), Some(a1), Some(b0), Some(b1)) = (
                corner(&bottom, i),
                corner(&bottom, i + 1),
                corner(&top, i),
                corner(&top, i + 1),
            ) else {
                return Err(Error::Argument);
            };
            face(b, fenv, &[a0, a1, b1, b0])?;
        }
        Ok(())
    });
    // a side the face builder will not take as planar (its own test is absolute, the flatness
    // proof above is relative to the loft's size) or a stitch that fails: the skin takes the
    // loft, as it takes any loft not proved planar, rather than the loft being refused
    if built.is_err() || stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()).is_err() {
        return Ok(None);
    }
    b.refresh_cached_topology();
    finish_closed(k, b, if cap { Expect::SOLID } else { Expect::FACES }).map(Some)
}

/// Whether a loft section (its plane's normal and its middle) faces back from the section
/// before it (its normal as turned along the loft, and its middle): the kernel's
/// [`solidcore::geom::plane_height::faces_back`]. The application winds the section the other
/// way round when it does.
#[must_use]
pub fn loft_section_faces_back(
    before_normal: [f64; 3],
    before_middle: [f64; 3],
    normal: [f64; 3],
    middle: [f64; 3],
) -> bool {
    solidcore::geom::plane_height::faces_back(
        v3(before_normal),
        v3(before_middle),
        v3(normal),
        v3(middle),
    )
}

/// Where the polyline `points` leaves the plane through `origin` with unit normal `normal`
/// (within `tol`), in order: the index of the segment and the point. The kernel's
/// [`solidcore::geom::plane_height::plane_departures`]; a loft opens a closed guide rail at
/// the first of them on its first section.
#[must_use]
pub fn polyline_plane_departures(
    points: &[[f64; 3]],
    origin: [f64; 3],
    normal: [f64; 3],
    tol: f64,
) -> Vec<(usize, [f64; 3])> {
    let points: Vec<Vec3> = points.iter().map(|p| v3(*p)).collect();
    solidcore::geom::plane_height::plane_departures(&points, v3(origin), v3(normal), tol)
        .into_iter()
        .map(|(k, p)| (k, [p.x, p.y, p.z]))
        .collect()
}

/// The kernel's trace of a skin: each side face with the curves it is skinned from, if traced.
type Traced = Option<Vec<(FaceId, Vec<usize>)>>;

/// A sheet skinned through `sections` as [`loft`] skins them, uncapped, and which curves each of
/// its record faces (in order) is skinned from: indices, ascending, into the sections' curves
/// taken section by section (curve `k` of section `s` is `s n + k`, for `n` curves a section);
/// `None` when the kernel did not trace the skin. Every section has the same number of curves,
/// joined end to end (open or closed). One skin takes its parameters across the sections from
/// every column at once, so neighbouring faces share their seam; columns skinned each on its own
/// take parameters of their own, and their seams part (tangency-labs/solidcraft#14048). The skin
/// joins each section's curves into one, each over its own parameter span, so a line is first a
/// NURBS line and every B-spline is reparameterised onto [0, 1] (the same curve): the sections
/// then join at the same knots. The sources are the kernel's trace of the skin
/// (`create_skin_primitive_with_sources`): a curve with a kink of its own runs along several
/// faces, curves that continue each other at the same speed can share one, and a closed section
/// the skin turns to match its neighbour gives a face other curves there than in the others.
///
/// # Errors
/// [`Error::Argument`] for fewer than two sections, sections of different counts of curves, a
/// degree outside 1 to 3, more than [`MAX_CURVES`] curves in all or a curve refused; the
/// kernel's status; [`Error::Empty`].
pub fn loft_sheet(sections: &[Vec<Curve>], degree: u32) -> Result<(Body, Option<Vec<Vec<usize>>>)> {
    let n = sections.first().map_or(0, Vec::len);
    if sections.len() < 2
        || !(1..=3).contains(&degree)
        || n == 0
        || sections.iter().any(|s| s.len() != n)
    {
        return Err(Error::Argument);
    }
    let all: Vec<Curve> = sections.iter().flatten().map(nurbs_line).collect();
    let made = run(
        "loft_sheet",
        |k| -> solidcore::Result<Out<(Brep, Traced)>> {
            let mut made = parsed!(parse(k, &all));
            let unit = Extent1d::from_min_max(0.0, 1.0);
            for c in &mut made.curves {
                if matches!(c.kind(), CurveKind::BSpline(_)) {
                    c.edit_parameterization(k, &unit)?;
                }
            }
            let counts = vec![n; sections.len()];
            let (skin, sources) = match create_skin_primitive_with_sources(
                k,
                &counts,
                &made.refs(),
                degree as usize,
                false,
            ) {
                Ok(r) => r,
                Err(e) => return Ok(Err(status(e))),
            };
            let traced = match sources {
                SkinSources::Traced(faces) => Some(faces),
                SkinSources::NotTraced { .. } => None,
            };
            Ok(finish_closed(k, skin.brep, Expect::FACES)?.map(|b| (b, traced)))
        },
    )?;
    let (brep, traced) = made?;
    let body = Body::new(brep);
    let Some(traced) = traced else {
        return Ok((body, None));
    };
    let (_, order) = body.to_record()?;
    let mut sources: BTreeMap<FaceId, Vec<usize>> = traced.into_iter().collect();
    // every face of an uncapped skin is a side face, traced
    let faces = order
        .faces
        .iter()
        .map(|f| sources.remove(f))
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::Exception)?;
    Ok((body, Some(faces)))
}

/// An exported sheet through two closed planar sections, tangent at one end along their centre connector.
/// `tangent_at` selects the section (0 or 1); `crown` scales the derivative at that end only.
/// With `faced`, orient the walls out of the sectioned region. The source lists are in the
/// returned record's face order, indexing the original curves flattened in section order.
///
/// # Errors
/// The kernel's refusals for invalid sections, a degenerate tangent or an invalid sheet.
pub fn loft_sheet_planar_g1(
    sections: &[LoftSection; 2],
    tangent_at: usize,
    crown: f64,
    faced: bool,
) -> Result<(solidcore_record::brep::Brep, Vec<Vec<usize>>)> {
    use solidcore::sweep::{loft_sections::LoftSection as Input, skin_planar_g1::skin_planar_g1};
    let given = sections.each_ref().map(|s| Input {
        origin: convert::vec3(s.plane.origin),
        x: convert::vec3(s.plane.x),
        y: convert::vec3(s.plane.y),
        segs: s.segs.iter().map(convert::seg2).collect(),
    });
    let made = run("loft_sheet_planar_g1", |k| {
        match skin_planar_g1(k, &given, tangent_at, crown, faced, MAX_CURVES) {
            Ok(skin) => Ok(finish_closed(k, skin.brep, Expect::FACES)?.map(|b| (b, skin.sources))),
            Err(e) => Ok(Err(status(e))),
        }
    })??;
    let (brep, sources) = made;
    let body = Body::new(brep);
    let report = body.check(crate::CheckLevel::All)?;
    if !report.is_valid() {
        return Err(Error::Internal(format!("loft_sheet_planar_g1: {report}")));
    }
    let (record, order) = body.to_record()?;
    let mut sources: BTreeMap<FaceId, Vec<usize>> = sources.into_iter().collect();
    let faces = order
        .faces
        .iter()
        .map(|f| sources.remove(f))
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::Exception)?;
    Ok((record, faces))
}

/// `c` as [`loft_sheet`] skins it: a line as a NURBS line over [0, 1], any other curve as it is.
fn nurbs_line(c: &Curve) -> Curve {
    match c {
        Curve::Line { start, end } => Curve::Nurbs {
            degree: 1,
            points: vec![
                [start[0], start[1], start[2], 1.0],
                [end[0], end[1], end[2], 1.0],
            ],
            knots: vec![0.0, 0.0, 1.0, 1.0],
        },
        _ => c.clone(),
    }
}

/// How a [`loft_with_ends`] leaves its first section or arrives at its last.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LoftEnd {
    /// As the skin through the sections runs: no condition.
    #[default]
    Free,
    /// Square to the section's plane, towards the side the next section is on. Each point
    /// of the section leaves with `weight` times the speed of its straight way to the point it
    /// is joined to in the next section: 1 is that speed, less turns towards the next section
    /// sooner, more runs on square to the plane for longer.
    Square {
        /// The weight, above 0.
        weight: f64,
    },
}

/// A capped skin through `sections` as [`loft`] makes it, cubic across them, with how it
/// leaves its first section and arrives at its last (`ends`; at least one of them
/// [`LoftEnd::Square`]). Two sections and one square end make a quadratic skin: three
/// conditions.
///
/// # Errors
/// [`Error::Argument`] for a section or number refused, both ends free, and a weight that is
/// not a number above 0; the kernel's status (sections that are not closed planar loops, an
/// end whose next section's centre lies in its plane); [`Error::Empty`], [`Error::NotSolid`].
pub fn loft_with_ends(sections: &[Vec<Curve>], cap: bool, ends: [LoftEnd; 2]) -> Result<Body> {
    if sections.len() < 2 || sections.len() > MAX_CURVES {
        return Err(Error::Argument);
    }
    let all: Vec<Curve> = sections.iter().flatten().cloned().collect();
    making("loft_with_ends", |k| {
        let made = parsed!(parse(k, &all));
        if sections
            .iter()
            .any(|s| s.is_empty() || s.len() > MAX_CURVES)
        {
            return Ok(Err(Error::Argument));
        }
        let counts: Vec<usize> = sections.iter().map(Vec::len).collect();
        match skin_with_ends(k, &counts, &made.refs(), cap, ends) {
            Ok(Ok(b)) => finish_closed(k, b, if cap { Expect::SOLID } else { Expect::FACES }),
            Ok(Err(e)) => Ok(Err(e)),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// The skin of [`loft_with_ends`] on the caller's kernel `k`, as the kernel made it (not yet
/// checked for faces or closedness): the profiles of `curves`, `counts` curves each, cubic
/// across them, `ends` as [`loft_with_ends`] takes them. One recipe for [`loft_with_ends`] and
/// for an application that runs its sweeps on its own kernel.
///
/// # Errors
/// The kernel's, as it reports them; inside, [`Error::Argument`] for both ends free and a
/// weight that is not a number above 0.
pub fn skin_with_ends(
    k: &mut Kernel,
    counts: &[usize],
    curves: &[Option<&KCurve>],
    cap: bool,
    ends: [LoftEnd; 2],
) -> solidcore::Result<std::result::Result<Brep, Error>> {
    if ends == [LoftEnd::Free; 2] {
        return Ok(Err(Error::Argument));
    }
    let mut kernel_ends = [SkinEnd::Free; 2];
    for (to, end) in kernel_ends.iter_mut().zip(ends) {
        if let LoftEnd::Square { weight } = end {
            if !(weight > 0.0 && weight.is_finite()) {
                return Ok(Err(Error::Argument));
            }
            *to = SkinEnd::Square { weight };
        }
    }
    create_skin_primitive_with_ends(k, counts, curves, 3, cap, kernel_ends).map(|r| Ok(r.brep))
}

/// A pyramid or cone from the planar `section` (one closed loop of curves, in order) to `apex`
/// (off the section's plane): the base on the section's own curves, a planar triangle on each
/// line, and on each arc or spline the ruled surface from it to the apex (to a curve of no
/// length there, the wall's pole), its face the surface's whole box; stitched and oriented. A
/// circle alone is a cone.
///
/// # Errors
/// [`Error::Argument`] for a section that is not one closed loop of lines, arcs and splines, or
/// an apex not proved off its plane; the kernel's status; [`Error::Empty`], [`Error::NotSolid`].
pub fn loft_to_point(section: &[Curve], apex: [f64; 3]) -> Result<Body> {
    if !coord_ok(&apex) {
        return Err(Error::Argument);
    }
    making("loft_to_point", |k| {
        let made = parsed!(parse(k, section));
        let curves = &made.curves;
        if curves.is_empty() || !path::closed_loop(k, curves, 1e-6)?.is_yes() {
            return Ok(Err(Error::Argument));
        }
        // each piece's start, the base's corners; a curved piece's B-spline, for its wall
        let mut corners = Vec::new();
        let mut walls: Vec<Option<BSplineCurve>> = Vec::new();
        for c in curves {
            corners.push(c.ends(k)?.start);
            walls.push(match c.kind() {
                CurveKind::Line(_) => None,
                CurveKind::Ellipse(e) => Some(e.base().clone()),
                CurveKind::BSpline(s) => Some(s.clone()),
                _ => return Ok(Err(Error::Argument)),
            });
        }
        let top: Point3 = v3(apex);
        match path::point_off_plane_of(k, curves, top, 1e-9) {
            Ok(v) if v.is_yes() => {}
            _ => return Ok(Err(Error::Argument)),
        }
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let region = b.infinite_region();
        let tol = 1e-6;
        let built = build_faces(&mut b, |b, fenv| -> Out<()> {
            // the base, on the section's own curves
            called(create_planar_face_with_3d_curves(
                k, b, region, curves, tol, fenv,
            ))?;
            for (i, (&p, wall)) in corners.iter().zip(&walls).enumerate() {
                let q = corners
                    .get(i + 1)
                    .or_else(|| corners.first())
                    .copied()
                    .unwrap_or(p);
                match wall {
                    // a line: the planar triangle to the apex
                    None => {
                        let mut edges = Vec::new();
                        for (a, z) in [(p, q), (q, top), (top, p)] {
                            edges.push(KCurve::from(called(Line::create_line_segment(
                                3, a, z, None,
                            ))?));
                        }
                        called(create_planar_face_with_3d_curves(
                            k, b, region, &edges, tol, fenv,
                        ))?;
                    }
                    // an arc or a spline: the ruled surface to a curve of no length at the
                    // apex; u = 0 and u = 1 are the rulings from the piece's ends, so the face
                    // is the surface's whole box
                    Some(base) => {
                        let at_apex = [top, top];
                        let data = CanonicalData {
                            dim: 3,
                            degree: 1,
                            control_points: &at_apex,
                            form: BSplineCurveForm::Unspecified,
                            knot_multiplicities: &[2, 2],
                            knots: &[0.0, 1.0],
                            knot_type: KnotType::Unspecified,
                            weights: None,
                            trim: None,
                        };
                        let tip = called(BSplineCurve::create_canonical(&data))?;
                        let ruled = called(BSplineSurface::create_ruled_surface(
                            base,
                            &tip,
                            solidcore::nurbs::b_spline_surface::p1::SurfParamType::Unknown,
                        ))?;
                        let surface = Surface::from(ruled);
                        let bx = called(surface.natural_uv_domain_with_nurb(k))?;
                        called(create_face_from_surface(k, b, Some(surface), &bx, fenv))?;
                    }
                }
            }
            Ok(())
        });
        if let Err(e) = built {
            return Ok(Err(e));
        }
        if let Err(e) = stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()) {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish_closed(k, b, Expect::SOLID)
    })
}

/// A loft from the planar first of `sections` (one closed loop of curves, in order) through the
/// rest, its stations (each as many curves as the first, need not be planar), to `apex` (off the
/// first's plane): the base on the first section's curves, and on each of its curves one wall
/// skinned (cubic across, or as high as the rows allow) through that curve, the stations' curves
/// in its column, and the curve shrunk onto the apex (the wall's pole), one face over the
/// surface's whole box; stitched and oriented. `params` places each row along the loft, the
/// sections' then the apex's (increasing, from 0 at the first section to 1 at the apex): every
/// wall is skinned at those parameters, so neighbouring walls, whose curves share their ends,
/// meet on the same boundary curves. They do as long as, at each corner, the weights of the two
/// curves that meet there keep one ratio through the rows, as rows moved from one profile (its
/// weights kept) do; walls that do not meet leave no solid, and the call is refused. A loft to a
/// point that follows guide rails gives the stations and where they lie.
///
/// # Errors
/// [`Error::Argument`] for fewer than two sections or more than [`MAX_STATIONS`], sections of
/// different counts, parameters that are not one a row increasing from 0 to 1, a curve list
/// refused, a first section that is not one closed loop, or an apex not proved off its plane;
/// the kernel's status; [`Error::Empty`], [`Error::NotSolid`].
pub fn loft_to_point_through(
    sections: &[Vec<Curve>],
    apex: [f64; 3],
    params: &[f64],
) -> Result<Body> {
    if !coord_ok(&apex) || !(2..=MAX_STATIONS).contains(&sections.len()) {
        return Err(Error::Argument);
    }
    if params.len() != sections.len() + 1
        || params.first() != Some(&0.0)
        || params.last() != Some(&1.0)
        || params.windows(2).any(|w| !matches!(w, [a, b] if a < b))
    {
        return Err(Error::Argument);
    }
    let n = sections.first().map_or(0, Vec::len);
    if n == 0
        || sections.iter().any(|s| s.len() != n)
        || sections.len().saturating_mul(n) > MAX_STATION_CURVES
    {
        return Err(Error::Argument);
    }
    making("loft_to_point_through", |k| {
        let mut rows = Vec::with_capacity(sections.len() + 1);
        for s in sections {
            let made = parsed!(parse(k, s));
            let mut row = Vec::with_capacity(n);
            for c in &made.curves {
                let Some(b) = bspline(c)? else {
                    return Ok(Err(Error::Argument));
                };
                row.push(b);
            }
            rows.push((made.curves, row));
        }
        let Some((base, base_splines)) = rows.first() else {
            return Ok(Err(Error::Argument));
        };
        if !path::closed_loop(k, base, 1e-6)?.is_yes() {
            return Ok(Err(Error::Argument));
        }
        let top: Point3 = v3(apex);
        match path::point_off_plane_of(k, base, top, 1e-9) {
            Ok(v) if v.is_yes() => {}
            _ => return Ok(Err(Error::Argument)),
        }
        // The apex row: each base curve with every control point on the apex (its own weights
        // and knots).
        let mut tip = Vec::with_capacity(n);
        for b in base_splines {
            let c = b.canonical()?;
            let at_apex = vec![top; c.control_points.len()];
            let data = CanonicalData {
                dim: 3,
                degree: c.degree,
                control_points: &at_apex,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &c.knot_multiplicities,
                knots: &c.knots,
                knot_type: KnotType::Unspecified,
                weights: (!c.weights.is_empty()).then_some(c.weights.as_slice()),
                trim: None,
            };
            tip.push(BSplineCurve::create_canonical(&data)?);
        }
        let mut columns: Vec<Vec<BSplineCurve>> = vec![Vec::with_capacity(rows.len() + 1); n];
        for (_, row) in &rows {
            for (column, b) in columns.iter_mut().zip(row) {
                column.push(b.clone());
            }
        }
        for (column, b) in columns.iter_mut().zip(tip) {
            column.push(b);
        }
        let degree = (params.len() - 1).min(3);
        let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        let region = b.infinite_region();
        let tol = 1e-6;
        let built = build_faces(&mut b, |b, fenv| -> Out<()> {
            called(create_planar_face_with_3d_curves(
                k, b, region, base, tol, fenv,
            ))?;
            for column in &columns {
                let wall = called(BSplineSurface::create_skinned_surface(
                    k,
                    column,
                    SkinnedSurfaceOptions {
                        deriv_surfs: Some([None, None]),
                        degree,
                        section_params: Some(params),
                        ..Default::default()
                    },
                ))?;
                let surface = Surface::from(wall);
                let domain = called(surface.natural_uv_domain_with_nurb(k))?;
                called(create_face_from_surface(k, b, Some(surface), &domain, fenv))?;
            }
            Ok(())
        });
        if let Err(e) = built {
            return Ok(Err(e));
        }
        if let Err(e) = stitch_and_orient(k, &mut b, &StitchAndOrientOptions::default()) {
            return Ok(Err(status(e)));
        }
        b.refresh_cached_topology();
        finish_closed(k, b, Expect::SOLID)
    })
}

/// Each station map finite, its translation within [`MAX_COORD`], its rows shorter than 1e6,
/// and proved regular to 1e-9 of its rows' lengths' product.
fn maps_ok(stations: &[Affine]) -> bool {
    stations.iter().all(|s| {
        let t = s.transform();
        coord_ok(&s.t) && t.row_lengths().iter().all(|&l| l < 1e6) && t.is_regular(1e-9).is_yes()
    })
}

/// The profile placed by each station (an affine map, carried exactly by its control points),
/// its sections skinned through (cubic across them or as high as their number allows), the
/// ends capped, or joined by `SmApiStitchIntoSolid`.
///
/// # Errors
/// [`Error::Argument`] for a station or curve list refused; the kernel's status;
/// [`Error::Empty`], [`Error::NotSolid`].
pub fn sweep_stations(profile: &[Curve], stations: &[Affine], end: Ends) -> Result<Body> {
    if !(2..=MAX_STATIONS).contains(&stations.len()) {
        return Err(Error::Argument);
    }
    making("sweep_stations", |k| {
        if !maps_ok(stations) {
            return Ok(Err(Error::Argument));
        }
        let made = parsed!(parse(k, profile));
        if made.curves.len().saturating_mul(stations.len()) > MAX_STATION_CURVES {
            return Ok(Err(Error::Argument));
        }
        let mut sections = Vec::new();
        let mut counts = Vec::new();
        for s in stations {
            let t = s.transform();
            for c in &made.curves {
                let Some(b) = bspline(c)? else {
                    return Ok(Err(Error::Argument));
                };
                match b.affine_image(&t) {
                    Ok((image, _)) => sections.push(KCurve::from(image)),
                    Err(e) => return Ok(Err(argument_or_status(e))),
                }
            }
            counts.push(made.curves.len());
        }
        let degree = (stations.len() - 1).min(3);
        let refs: Vec<Option<&KCurve>> = sections.iter().map(Some).collect();
        let mut r = match create_skin_primitive(k, &counts, &refs, degree, end == Ends::Capped) {
            Ok(r) => r.brep,
            Err(e) => return Ok(Err(status(e))),
        };
        if end == Ends::Joined {
            match api_stitch_into_solid(k, Some(&mut r)) {
                Ok(s) if s.produces_a_solid => {}
                Ok(_) => return Ok(Err(Error::NotSolid)),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        finish_closed(
            k,
            r,
            if end == Ends::Open {
                Expect::FACES
            } else {
                Expect::SOLID
            },
        )
    })
}

/// The screw sweep of `profile` about the axis through `origin` along `axis`: `turns` turns
/// rising `pitch` each, the walls within `tolerance` of their screw surfaces, capped; with a
/// `law`, the section moved off the axis by the piecewise linear offset of its (turn, offset)
/// pairs as it turns (`sweep::screw`). Its walls are laid out a face per piece
/// ([`ScrewWalls::Pieces`]); [`screw_sweep_with`] chooses.
///
/// # Errors
/// [`Error::Argument`] for a number, law or curve list refused; [`Error::Refused`] for the
/// kernel's refusals, in its words (an axis without a usable length, a profile on the axis,
/// more turns than the spans hold at the tolerance); the kernel's status; [`Error::Empty`],
/// [`Error::NotSolid`].
pub fn screw_sweep(
    profile: &[Curve],
    origin: [f64; 3],
    axis: [f64; 3],
    pitch: f64,
    turns: f64,
    law: Option<&[(f64, f64)]>,
    tolerance: f64,
) -> Result<Body> {
    screw_sweep_with(
        profile,
        origin,
        axis,
        pitch,
        turns,
        &ScrewSweepOptions {
            law,
            tolerance,
            walls: ScrewWalls::Pieces,
        },
    )
}

/// How a screw sweep lays its B-spline walls out in faces (`sweep::screw::Walls`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrewWalls {
    /// A face per piece: every wall breaks at a radial law's angles and, when a profile line
    /// along the axis sweeps a cylinder, every half turn.
    #[default]
    Pieces,
    /// A B-spline wall runs on as one face across the half turns only the cylinders need,
    /// breaking at a law's angles only: a thread's flanks as one continuous face each. The
    /// surface and its edges are the same curves as the pieces'; cylinder walls keep their
    /// half-turn faces.
    Joined,
}

/// The options of [`screw_sweep_with`].
#[derive(Debug, Clone, Copy)]
pub struct ScrewSweepOptions<'a> {
    /// The radial law as (turn, offset) pairs, turns ascending within `[0, turns]`; `None`
    /// for none.
    pub law: Option<&'a [(f64, f64)]>,
    /// How far a wall may stray from its screw surface, mm (`1e-6` up to `1e3`).
    pub tolerance: f64,
    /// The walls' faces.
    pub walls: ScrewWalls,
}

/// [`screw_sweep`] with its options: the law, the tolerance and the walls' layout.
///
/// ```
/// use solidcore_ops::sweep::{screw_sweep_with, Curve, ScrewSweepOptions, ScrewWalls};
/// // a square thread 1 x 1 from radius 5 to 6, two turns of pitch 2: its inner and outer
/// // walls are cylinders, which come in half turns
/// let corners = [[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, 0.0, 1.0], [5.0, 0.0, 1.0]];
/// let profile: Vec<Curve> = (0..4)
///     .map(|i| Curve::Line { start: corners[i], end: corners[(i + 1) % 4] })
///     .collect();
/// let thread = |walls| {
///     let options = ScrewSweepOptions { law: None, tolerance: 1e-5, walls };
///     screw_sweep_with(&profile, [0.0; 3], [0.0, 0.0, 1.0], 2.0, 2.0, &options)
/// };
/// // four half turns of four walls, and two caps
/// assert_eq!(thread(ScrewWalls::Pieces)?.counts()?[0], 18);
/// // the flanks run on as one face each; the cylinders keep their half turns
/// assert_eq!(thread(ScrewWalls::Joined)?.counts()?[0], 12);
/// # Ok::<(), solidcore_ops::Error>(())
/// ```
///
/// # Errors
/// Those of [`screw_sweep`].
pub fn screw_sweep_with(
    profile: &[Curve],
    origin: [f64; 3],
    axis: [f64; 3],
    pitch: f64,
    turns: f64,
    options: &ScrewSweepOptions<'_>,
) -> Result<Body> {
    let ScrewSweepOptions {
        law,
        tolerance,
        walls,
    } = *options;
    let ok = coord_ok(&origin)
        && pitch.is_finite()
        && pitch.abs() > 1e-6
        && pitch.abs() < MAX_COORD
        && turns.is_finite()
        && (1e-3..=SCREW_MAX_TURNS).contains(&turns)
        && tolerance.is_finite()
        && (1e-6..1e3).contains(&tolerance);
    if !ok {
        return Err(Error::Argument);
    }
    if let Some(law) = law {
        if law.is_empty()
            || law.len() > SCREW_MAX_LAW
            || !law.iter().all(|(t, o)| t.is_finite() && o.is_finite())
        {
            return Err(Error::Argument);
        }
        for (i, (t, o)) in law.iter().enumerate() {
            let ascending = i == 0 || law.get(i - 1).is_some_and(|(p, _)| t > p);
            if !(*t >= 0.0 && *t <= turns) || !ascending || !(o.abs() < MAX_COORD) {
                return Err(Error::Argument);
            }
        }
    }
    let call = if law.is_some() {
        "screw_sweep_radial"
    } else {
        "screw_sweep"
    };
    let walls = match walls {
        ScrewWalls::Pieces => screw::Walls::Pieces,
        ScrewWalls::Joined => screw::Walls::Joined,
    };
    making(call, |k| {
        let made = parsed!(parse(k, profile));
        let built = screw::screw_sweep_with_body(
            k,
            &made.curves,
            v3(origin),
            v3(axis),
            pitch,
            turns,
            law,
            tolerance,
            walls,
        );
        match built {
            Ok(b) => finish_closed(k, b, Expect::SOLID),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// `c` as the NURBS it is, with its interval; [`Error::Unsupported`] for a curve that is no
/// B-spline or whose knots are not clamped.
fn put_curve(c: &KCurve) -> solidcore::Result<Out<(Curve, [f64; 2])>> {
    let Some(b) = bspline(c)? else {
        return Ok(Err(Error::Unsupported("a curve that is no B-spline")));
    };
    let can = b.canonical()?;
    let degree = can.degree;
    let count = can.control_points.len();
    let mut knots = Vec::new();
    for (&u, &m) in can.knots.iter().zip(&can.knot_multiplicities) {
        knots.extend(std::iter::repeat_n(u, m));
    }
    let clamped = can.knot_multiplicities.first().is_some_and(|&m| m > degree)
        && can.knot_multiplicities.last().is_some_and(|&m| m > degree);
    if !(1..=CURVE_MAX_DEGREE).contains(&degree)
        || count < degree + 1
        || count > CURVE_MAX_POINTS
        || knots.len() != count + degree + 1
        || !clamped
    {
        return Ok(Err(Error::Unsupported(
            "a curve whose degree, points or knots are out of range or not clamped",
        )));
    }
    let points = can
        .control_points
        .iter()
        .enumerate()
        .map(|(i, p)| [p.x, p.y, p.z, can.weights.get(i).copied().unwrap_or(1.0)])
        .collect();
    let ivl = c.natural_interval();
    Ok(Ok((
        Curve::Nurbs {
            degree,
            points,
            knots,
        },
        [ivl.min(), ivl.max()],
    )))
}

/// The kernel face of `b` that record index `i` names through `order` (the [`Order`] of `b`'s
/// record): an index past the record's faces, or one naming a face `b` does not have (an
/// order taken before an edit removed it), is [`Error::Argument`].
fn face_of(b: &Brep, order: &Order, i: usize) -> Result<FaceId> {
    let f = order::face(order, i)?;
    if b.face_list().contains(&f) {
        Ok(f)
    } else {
        Err(Error::Argument)
    }
}

/// The curve tolerance of [`project_curves`] and [`face_section`], and of the curves a face is
/// trimmed along (`crate::trim`).
pub(crate) fn tolerance_ok(tolerance: f64) -> bool {
    tolerance.is_finite() && (1e-9..1e3).contains(&tolerance)
}

/// `curves` projected along `direction` (any length; from them toward the face) onto record
/// face `face` of `body` (through `order`, its record's [`Order`]:
/// `SmBrep::CreateParallelProjectionCurves` on that face), each within `tolerance` of the true
/// projection, as NURBS with their intervals.
///
/// # Errors
/// [`Error::Argument`] for a number or curve list refused, or an index past the record's faces
/// or naming a face the body does not have; the kernel's status;
/// [`Error::Empty`] when nothing projects.
pub fn project_curves(
    body: &Body,
    order: &Order,
    face: usize,
    curves: &[Curve],
    direction: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Curve, [f64; 2])>> {
    if !direction.iter().all(|x| x.is_finite()) || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    with_body("project_curves", body, |k, b| {
        let given = parsed!(parse(k, curves));
        let face = parsed!(face_of(b, order, face));
        let faces = [face];
        let opts = ParallelProjectionOptions {
            approx_tol_3d: Some(tolerance),
            subset_faces: Some(&faces),
            want_3d: true,
            ..ParallelProjectionOptions::default()
        };
        let mut out = Vec::new();
        for c in &given.curves {
            // a line, an arc and a B-spline are their own root curve, and go in as themselves
            if !matches!(
                c.kind(),
                CurveKind::BSpline(_) | CurveKind::Line(_) | CurveKind::Ellipse(_)
            ) {
                return Ok(Err(Error::Argument));
            }
            let projected = match directed::project_along(k, b, c, v3(direction), &opts) {
                Ok(m) => m.curves_3d.unwrap_or_default(),
                Err(e) => return Ok(Err(directed_or_status(e))),
            };
            for m in &projected {
                match put_curve(m)? {
                    Ok(x) => out.push(x),
                    Err(e) => return Ok(Err(e)),
                }
            }
        }
        Ok(if out.is_empty() {
            Err(Error::Empty)
        } else {
            Ok(out)
        })
    })?
}

/// The section of record face `face` of `body` (through `order`, its record's [`Order`]) by the
/// plane through `plane_point` square to `plane_normal` (any length), as NURBS with their
/// intervals.
///
/// # Errors
/// [`Error::Argument`] for a number refused, or an index past the record's faces or naming a
/// face the body does not have; the kernel's status;
/// [`Error::Unsupported`] when the kernel does not say which face each curve lies on;
/// [`Error::Empty`] when the plane misses the face.
pub fn face_section(
    body: &Body,
    order: &Order,
    face: usize,
    plane_point: [f64; 3],
    plane_normal: [f64; 3],
    tolerance: f64,
) -> Result<Vec<(Curve, [f64; 2])>> {
    if !plane_point
        .iter()
        .chain(&plane_normal)
        .all(|x| x.is_finite())
        || !tolerance_ok(tolerance)
    {
        return Err(Error::Argument);
    }
    with_body("face_section", body, |k, b| {
        let face = parsed!(face_of(b, order, face));
        // UV curves are asked for, so the classify is forced: an analytic face's intersector
        // makes none, and without it the trim drops the whole section (solidcore#1769).
        let opts = PlanarSectionOptions {
            approx_tol_3d: Some(tolerance),
            outputs: SectionOutputs {
                curves_3d: true,
                uv_curves: true,
                faces: true,
            },
            force_uv_classify: true,
            ..PlanarSectionOptions::default()
        };
        let made = match directed::section_square_to(k, b, v3(plane_point), v3(plane_normal), &opts)
        {
            Ok(m) => m,
            Err(e) => return Ok(Err(directed_or_status(e))),
        };
        let curves = made.curves_3d.unwrap_or_default();
        let faces = made.faces.unwrap_or_default();
        if faces.len() != curves.len() {
            return Ok(Err(Error::Unsupported(
                "the kernel does not say which face each section curve lies on",
            )));
        }
        let mut out = Vec::new();
        for (c, &f) in curves.iter().zip(&faces) {
            if f == face {
                match put_curve(c)? {
                    Ok(x) => out.push(x),
                    Err(e) => return Ok(Err(e)),
                }
            }
        }
        Ok(if out.is_empty() {
            Err(Error::Empty)
        } else {
            Ok(out)
        })
    })?
}

/// `curves` dropped onto record face `face` of `body` (through `order`, its record's [`Order`])
/// square to its surface: the kernel's surface drop of a curve (`DropAndTrimCurve` over the
/// face's parameter box, every piece kept, on a standalone copy of the face's surface: the face's
/// trims do not cut it) lifted back to 3D (`LiftCurve`), each within
/// `tolerance`, as NURBS with their intervals. A curve over the face lands on the foot of each of
/// its points.
///
/// # Errors
/// [`Error::Argument`] for a number or curve list refused, or an index past the record's faces
/// or naming a face the body does not have; the kernel's status; [`Error::Empty`] when no curve
/// lands on the face.
pub fn drop_curves(
    body: &Body,
    order: &Order,
    face: usize,
    curves: &[Curve],
    tolerance: f64,
) -> Result<Vec<(Curve, [f64; 2])>> {
    if !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    read_body("drop_curves", body, |k, b| {
        let given = parsed!(parse(k, curves));
        let face = parsed!(face_of(b, order, face));
        let dropped = parsed!(dropped_on(k, b, face, &given.curves, tolerance)?);
        let mut out = Vec::new();
        for c in dropped.iter().flatten() {
            match put_curve(c)? {
                Ok(x) => out.push(x),
                Err(e) => return Ok(Err(e)),
            }
        }
        Ok(if out.is_empty() {
            Err(Error::Empty)
        } else {
            Ok(out)
        })
    })?
}

/// The kernel's curves of [`drop_curves`]: for each curve of `given`, in order, its pieces
/// dropped square onto the surface of `face` of `b` over the face's parameter box and lifted
/// back to 3D, each within `tolerance`; none for a curve that does not land on the box. For
/// the families outside this module that cut a face along them (`crate::trim`).
pub(crate) fn dropped_on(
    k: &Kernel,
    b: &Brep,
    face: FaceId,
    given: &[KCurve],
    tolerance: f64,
) -> solidcore::Result<Out<Vec<Vec<KCurve>>>> {
    let domain = b.face(face)?.uv_domain;
    let Some(owned) = b.face(face)?.surface.as_ref() else {
        return Ok(Err(Error::Argument));
    };
    // A standalone copy (its owner not copied): the drop and lift work over the face's
    // parameter box, and a face-owned surface's solves would ask for the face's trimmed
    // cache, which they are not given here.
    let surface = owned.copy(k)?;
    let mut out = Vec::new();
    for c in given {
        let dropped = match surface.drop_and_trim_curve::<NoObject>(
            k,
            &domain,
            c,
            &c.natural_interval(),
            tolerance,
            true,
            false,
            None,
        ) {
            Ok(d) => d,
            Err(e) => return Ok(Err(status(e))),
        };
        let mut pieces = Vec::new();
        for uv in &dropped.uv_curves {
            match surface.lift_curve(k, &domain, uv, &uv.natural_interval(), tolerance, false) {
                Ok(l) => pieces.push(l.curve),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        out.push(pieces);
    }
    Ok(Ok(out))
}

/// A curve's record form ([`Curve::to_record`]): what it is, the NURBS form a circle's
/// parameter is on, and its interval in the record's parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedCurve {
    /// A line or circle label, or the spline (rational or not) itself.
    pub curve: solidcore_record::brep::Curve,
    /// The NURBS form of a circle, which `interval` is on.
    pub nurbs: Option<solidcore_record::brep::Curve>,
    /// The curve's interval in the record's parameter.
    pub interval: [f64; 2],
}

impl Curve {
    /// The record form of this curve over its natural range: the kernel's record of the curve it
    /// makes ([`solidcore::curve::from_parameters::record_of`]: a line as itself, an arc as its
    /// circle with that circle's NURBS form and the arc's interval, a NURBS as a spline, or as a
    /// rational spline of homogeneous points when weighted), converted as the record holds it.
    ///
    /// # Errors
    /// [`Error::Argument`] for a curve refused (as the sweeps refuse it); [`Error::Record`] for a
    /// curve the record cannot hold; the kernel's status.
    pub fn to_record(&self) -> Result<RecordedCurve> {
        let made = run("sweep::Curve::to_record", |k| {
            let given = parsed!(parse(k, std::slice::from_ref(self)));
            let Some(c) = given.curves.first() else {
                return Ok(Err(Error::Argument));
            };
            Ok(match record_of(k, c, &c.natural_interval()) {
                Ok(r) => Ok(r),
                Err(e) => Err(status(e)),
            })
        })??;
        let back = |c| convert::record_curve(c).map_err(|why| Error::Record(why.0.to_owned()));
        Ok(RecordedCurve {
            curve: back(made.curve)?,
            nurbs: made.nurbs.map(back).transpose()?,
            interval: made.interval,
        })
    }
}

#[cfg(test)]
mod tests {
    //! Tests for profile-segment placement and the loft it feeds.

    use super::*;
    use solidcore_record::profile::Seg2;

    /// A 60 x 40 rounded rectangle (corner radius 8), closed, from the middle of its bottom
    /// line: Pappus' length 2 (44 + 24) + 4 (pi/2) 8.
    fn rounded_rectangle() -> (Vec<Curve>, f64) {
        let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
        let arc = |c: [f64; 3], r: [f64; 3]| Curve::Arc {
            center: c,
            axis: [0.0, 0.0, 1.0],
            reference: r,
            radius: 8.0,
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        };
        let path = vec![
            line([0.0, -20.0, 0.0], [22.0, -20.0, 0.0]),
            arc([22.0, -12.0, 0.0], [0.0, -1.0, 0.0]),
            line([30.0, -12.0, 0.0], [30.0, 12.0, 0.0]),
            arc([22.0, 12.0, 0.0], [1.0, 0.0, 0.0]),
            line([22.0, 20.0, 0.0], [-22.0, 20.0, 0.0]),
            arc([-22.0, 12.0, 0.0], [0.0, 1.0, 0.0]),
            line([-30.0, 12.0, 0.0], [-30.0, -12.0, 0.0]),
            arc([-22.0, -12.0, 0.0], [-1.0, 0.0, 0.0]),
            line([-22.0, -20.0, 0.0], [0.0, -20.0, 0.0]),
        ];
        (path, 2.0 * (44.0 + 24.0) + 16.0 * PI)
    }

    /// A `w` x `h` rectangle round the path's start (0, -20, 0), square to it, either way round.
    fn rectangle(w: f64, h: f64, backwards: bool) -> Vec<Curve> {
        let (y, z) = (w / 2.0, h / 2.0);
        let mut p = [
            [0.0, -20.0 - y, -z],
            [0.0, -20.0 + y, -z],
            [0.0, -20.0 + y, z],
            [0.0, -20.0 - y, z],
        ];
        if backwards {
            p.reverse();
        }
        (0..4)
            .map(|i| Curve::Line {
                start: p[i],
                end: p[(i + 1) % 4],
            })
            .collect()
    }

    /// A 4 x 3 rectangle with a 2 x 1 hole swept round a closed path, which takes the curve
    /// sweep: each loop is swept on its own and the hole's tube taken from the outer one's, so
    /// the solid is the area between them times the path's length (Pappus: both loops are
    /// centred on the path). Until each loop was swept on its own, every loop's tube bounded a
    /// solid of its own and the hole's added its volume; `curve_sweep` takes the holes the same
    /// way.
    #[test]
    fn a_profile_s_holes_are_taken_away_round_a_closed_path() {
        let (path, length) = rounded_rectangle();
        let expected = (12.0 - 2.0) * length;
        for backwards in [false, true] {
            let profile = [rectangle(4.0, 3.0, false), rectangle(2.0, 1.0, backwards)].concat();
            let (body, _, route) = sweep_placed(&profile, &path, SweepFrame::Follow, true).unwrap();
            assert_eq!(route, SweepRoute::Curve(SweepFrame::Follow));
            let v = body.volume(1e-8).unwrap();
            assert!(
                (v - expected).abs() < 1e-4 * expected,
                "sweep_placed: {v}, not {expected}"
            );
            let (body, _) =
                curve_sweep(&profile, &path, SweepFrame::Follow, true, Contract::Closed).unwrap();
            let v = body.volume(1e-8).unwrap();
            assert!(
                (v - expected).abs() < 1e-4 * expected,
                "curve_sweep: {v}, not {expected}"
            );
        }
        // one loop is swept as before, whole
        let (body, ..) =
            sweep_placed(&rectangle(4.0, 3.0, false), &path, SweepFrame::Follow, true).unwrap();
        let v = body.volume(1e-8).unwrap();
        assert!((v - 12.0 * length).abs() < 1e-4 * 12.0 * length, "{v}");
    }

    /// A swept solid keeps the proof its result test made: `sweep_placed`'s, and `curve_sweep`'s
    /// under [`Contract::Proved`], so the next proved operation does not prove it again. Under
    /// [`Contract::Closed`] no proof is made, so none is held.
    #[test]
    fn a_proved_sweep_keeps_its_proof() {
        let (path, _) = rounded_rectangle();
        let profile = rectangle(4.0, 3.0, false);
        let (body, ..) = sweep_placed(&profile, &path, SweepFrame::Follow, true).unwrap();
        assert!(body.holds_proof(), "sweep_placed");
        let (body, _) =
            curve_sweep(&profile, &path, SweepFrame::Follow, true, Contract::Proved).unwrap();
        assert!(body.holds_proof(), "curve_sweep, proved");
        let (body, _) =
            curve_sweep(&profile, &path, SweepFrame::Follow, true, Contract::Closed).unwrap();
        assert!(!body.holds_proof(), "curve_sweep, closed");
    }

    /// The four lines of a square profile with the given half-width, in order.
    fn square(half: f64) -> Vec<Seg2> {
        let p = [
            solidcore_record::Vec2::new(-half, -half),
            solidcore_record::Vec2::new(half, -half),
            solidcore_record::Vec2::new(half, half),
            solidcore_record::Vec2::new(-half, half),
        ];
        (0..4)
            .map(|i| Seg2::Line {
                a: p[i],
                b: p[(i + 1) % 4],
            })
            .collect()
    }

    /// A profile line and arc take the plane's origin, axes and normal.
    #[test]
    fn a_profile_line_and_arc_place_on_the_plane() {
        let line = Curve::from_seg2(
            &Seg2::Line {
                a: solidcore_record::Vec2::new(0.0, 0.0),
                b: solidcore_record::Vec2::new(2.0, 0.0),
            },
            &solidcore_record::Plane::XY,
        )
        .expect("line");
        assert!(matches!(
            line,
            Curve::Line {
                start: [0.0, 0.0, 0.0],
                end: [2.0, 0.0, 0.0]
            }
        ));

        let arc = Curve::from_seg2(
            &Seg2::Arc {
                center: solidcore_record::Vec2::new(0.0, 0.0),
                radius: 1.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            },
            &solidcore_record::Plane::XY,
        )
        .expect("arc");
        let Curve::Arc {
            axis, start, end, ..
        } = arc
        else {
            panic!("arc");
        };
        assert_eq!(axis, [0.0, 0.0, 1.0]);
        assert!((end - start - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    }

    /// Cubic, conic and higher Bézier segments become NURBS curves.
    #[test]
    fn curved_profile_segments_become_nurbs() {
        let v = |x, y| solidcore_record::Vec2::new(x, y);
        let cubic = Curve::from_seg2(
            &Seg2::Cubic {
                p0: v(0.0, 0.0),
                p1: v(1.0, 0.0),
                p2: v(1.0, 1.0),
                p3: v(0.0, 1.0),
            },
            &solidcore_record::Plane::XY,
        )
        .expect("cubic");
        let Curve::Nurbs {
            degree,
            points,
            knots,
        } = cubic
        else {
            panic!("cubic is a NURBS");
        };
        assert_eq!(degree, 3);
        assert_eq!(points.len(), 4);
        assert_eq!(knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(points[1], [1.0, 0.0, 0.0, 1.0]);

        let conic = Curve::from_seg2(
            &Seg2::Conic {
                a: v(0.0, 0.0),
                apex: v(1.0, 1.0),
                b: v(2.0, 0.0),
                w: 0.5,
            },
            &solidcore_record::Plane::XY,
        )
        .expect("conic");
        let Curve::Nurbs {
            degree,
            points,
            knots,
        } = conic
        else {
            panic!("conic is a NURBS");
        };
        assert_eq!(degree, 2);
        assert_eq!(points[1], [1.0, 1.0, 0.0, 0.5]);
        assert_eq!(knots, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        // At t = 1/2 the rational quadratic's Euclidean point is
        // (a + 2 w apex + b) / (2 + 2 w), with the ends at weight 1.
        let at = |index: usize| points[index][..3].to_vec();
        let weight = points[1][3];
        let mid = [
            (at(0)[0] + 2.0 * weight * at(1)[0] + at(2)[0]) / (2.0 + 2.0 * weight),
            (at(0)[1] + 2.0 * weight * at(1)[1] + at(2)[1]) / (2.0 + 2.0 * weight),
            (at(0)[2] + 2.0 * weight * at(1)[2] + at(2)[2]) / (2.0 + 2.0 * weight),
        ];
        assert!((mid[0] - 1.0).abs() < 1.0e-12);
        assert!((mid[1] - 1.0 / 3.0).abs() < 1.0e-12);

        let mut bezier_points = [v(0.0, 0.0); solidcore_record::MAX_BEZIER_CTRL];
        for (index, point) in bezier_points.iter_mut().enumerate().take(5) {
            *point = v(index as f64, 0.0);
        }
        let bezier = Curve::from_seg2(
            &Seg2::Bezier {
                n: 4,
                p: bezier_points,
            },
            &solidcore_record::Plane::XY,
        )
        .expect("bezier");
        let Curve::Nurbs {
            degree,
            points,
            knots,
        } = bezier
        else {
            panic!("bezier is a NURBS");
        };
        assert_eq!(degree, 4);
        assert_eq!(points.len(), 5);
        assert_eq!(points[4], [4.0, 0.0, 0.0, 1.0]);
        assert_eq!(knots.len(), 10);

        assert!(Curve::from_seg2(
            &Seg2::Bezier {
                n: 0,
                p: bezier_points,
            },
            &solidcore_record::Plane::XY,
        )
        .is_none());
        assert!(Curve::from_seg2(
            &Seg2::Conic {
                a: v(0.0, 0.0),
                apex: v(1.0, 1.0),
                b: v(2.0, 0.0),
                w: 0.0,
            },
            &solidcore_record::Plane::XY,
        )
        .is_none());
    }

    /// Two square profile sections loft into the expected closed box.
    #[test]
    fn two_profile_sections_loft_to_a_closed_box() {
        let lower: Vec<Curve> = square(2.0)
            .iter()
            .map(|s| Curve::from_seg2(s, &solidcore_record::Plane::XY).expect("lower"))
            .collect();
        let upper: Vec<Curve> = square(2.0)
            .iter()
            .map(|s| {
                Curve::from_seg2(
                    s,
                    &solidcore_record::Plane {
                        origin: solidcore_record::Vec3::new(0.0, 0.0, 6.0),
                        ..solidcore_record::Plane::XY
                    },
                )
                .expect("upper")
            })
            .collect();
        let body = loft(&[lower, upper], 1, true).expect("loft");
        assert_eq!(body.counts().expect("counts"), [6, 12, 8]);
        assert!((body.volume(1.0e-3).expect("volume") - 96.0).abs() < 1.0e-6);
    }

    /// #22655: a sweep's plain call tests its solid for closure only ([`finish_closed`],
    /// `Contract::Closed`), so it decides no volume sign. Since #21968 every extrusion and
    /// revolution SolidCraft made, a cut's tools among them, was proved valid and facing out,
    /// one decided volume sign each, and nothing read the proof. Counted on the kernel, so the
    /// same on every machine. A caller that wants the proof asks for it: `check_solid` on the
    /// same body decides one.
    #[test]
    fn a_plain_sweep_decides_no_volume_sign() {
        use crate::session::counted;
        use solidcore::kernel::WorkCounters;
        const Z: [f64; 3] = [0.0, 0.0, 1.0];
        let circle = [Curve::Arc {
            center: [0.0, 0.0, 0.0],
            axis: Z,
            reference: [1.0, 0.0, 0.0],
            radius: 3.0,
            start: 0.0,
            end: 2.0 * PI,
        }];
        let (cylinder, decided) = counted(WorkCounters::volume_sign_decisions, || {
            extrude(&circle, Z, 2.0, 0.0, DraftCorner::default(), true)
        });
        let cylinder = cylinder.expect("extrude");
        assert_eq!(decided, 0, "the extrusion");
        // x 10 to 20, z 0 to 30 in the XZ plane, turned 270 degrees about Z
        let p = [[10.0, 0.0], [20.0, 0.0], [20.0, 30.0], [10.0, 30.0]];
        let rectangle: Vec<Curve> = (0..4)
            .map(|i| Curve::Line {
                start: [p[i][0], 0.0, p[i][1]],
                end: [p[(i + 1) % 4][0], 0.0, p[(i + 1) % 4][1]],
            })
            .collect();
        let (ring, decided) = counted(WorkCounters::volume_sign_decisions, || {
            revolve(&rectangle, [0.0; 3], Z, 1.5 * PI, true)
        });
        ring.expect("revolve");
        assert_eq!(decided, 0, "the revolution");
        let (checked, decided) = counted(WorkCounters::volume_sign_decisions, || {
            cylinder.check_solid()
        });
        assert_eq!(checked, Ok(()));
        assert_eq!(decided, 1, "the proof asked for");
    }
}
