//! Sweeps: planar faces, extrusion (straight or drafted), revolution, sweep along a path, loft
//! through sections or to a point, the sweep of a profile placed at stations, the screw sweep
//! of a thread or coil, and curves projected onto or sectioning a face (named by its record
//! index through the body's [`Order`]). Profiles, paths and sections are lists of [`Curve`]s;
//! each sweep returns a new body.
//!
//! Each call makes its curves from the list (the kernel's constructors), checks its numbers
//! before the kernel sees them, runs the kernel's sweep and hands the result over only when it
//! is what was asked for ([`finish`]): a closed manifold solid when the ends are capped, never a
//! body without faces. What a path is (one line, arcs about one axis, lines and arcs turning
//! with it) is the kernel's certified query; a path it leaves undecided is refused, naming the
//! piece and the comparison, never fitted instead ([`sweep_placed`]).
//!
//! The arcs of a profile on one circle: whether to make them one wall is the application's
//! rule; [`arcs_on_one_circle`], [`curves_meet`] and [`join_arcs`] answer and build for it.

// The range tests refuse NaN by negation (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]

use std::f64::consts::PI;

use solidcore::curve::arc::{self, ArcDefinition};
use solidcore::curve::curve::{Curve as KCurve, CurveKind};
use solidcore::curve::from_parameters::record_of;
use solidcore::curve::line::Line;
use solidcore::curve::path::{self, PathDoubt, PathPiece, PathShape, PathTolerances};
use solidcore::geom::tol::{context_zone_tol_3d, SmallTopoType, USE_DEFAULT};
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::heal::stitch::p1::api_stitch_into_solid;
use solidcore::kernel::Kernel;
use solidcore::math::Transform;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::prism::profile as kp;
use solidcore::surface::surface::p4::NoObject;
use solidcore::sweep::api_brep::{
    create_curve_sweep_with_placements, create_linear_sweep, create_rotational_sweep,
    CornerRefusal as KRefusal, OffsetCornerType, TaperExtrude,
};
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::sweep::directed;
use solidcore::sweep::extrude::{self as kextrude, Draft, ExtrudeTolerances};
use solidcore::sweep::screw;
use solidcore::sweep::segments::{
    build_faces, line_placements, sweep_along_segments, turn_placements,
};
use solidcore::topology::brep::p1::ParallelProjectionOptions;
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::p4::stitch_faces;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::p8::{
    create_planar_face_with_3d_curves, create_planar_faces_with_3d_curves, FaceBuildEnv,
};
use solidcore::topology::brep::planar_section::{PlanarSectionOptions, SectionOutputs};
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

use crate::body::{finish, Body, Expect};
use crate::error::status;
use crate::order::{self, Order};
use crate::session::{run, with_body};
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

/// Most turns of a screw sweep.
pub const SCREW_MAX_TURNS: f64 = 200.0;

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
    /// The sweep piece by piece along lines and arcs proved end to end and turning with it:
    /// exact.
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
        finish(k, b, Expect::FACES)
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
            Ok(TaperExtrude::Solid(r)) => finish(k, r, expect),
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
            Ok(r) => finish(k, r, if cap { Expect::SOLID } else { Expect::FACES }),
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

/// A path of end-to-end curves as one B-spline (the curve sweep takes one curve).
fn joined(path: &[KCurve]) -> solidcore::Result<Out<BSplineCurve>> {
    let mut parts = Vec::new();
    for c in path {
        let Some(b) = bspline(c)? else {
            return Ok(Err(Error::Argument));
        };
        parts.push(b);
    }
    if parts.len() == 1 {
        return Ok(parts.pop().ok_or(Error::Argument));
    }
    Ok(called(BSplineCurve::create_by_joining(&mut parts, None)))
}

/// The identity placements of a sweep that has not run.
fn unplaced() -> [Affine; 3] {
    [Affine::of(&Transform::IDENTITY); 3]
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
    let body = making("sweep", |k| {
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
                return finish(k, r, expect);
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
                    return finish(k, r, expect);
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
                            finish(k, b, expect)
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
        let path_curve = match joined(&along.curves)? {
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
        let r = match create_curve_sweep_with_placements(
            k,
            &refs,
            Some(&path_curve),
            None,
            None,
            Some(&options),
        ) {
            Ok(r) => r,
            Err(e) => return Ok(Err(status(e))),
        };
        for (m, p) in placed.iter_mut().zip(&r.placements) {
            *m = Affine::of(&Transform::of_placement(p));
        }
        let mut b = r.sweep.brep;
        if closed {
            let tol = b.tolerance();
            match stitch_faces(k, &mut b, tol, None, false) {
                // a solid asked for has no free edge left; a sheet (an open profile, or one
                // not capped) keeps the profile's own ends free
                Ok(s) if cap && s.lamina_edges != 0 => return Ok(Err(Error::NotSolid)),
                Ok(_) => b.refresh_cached_topology(),
                Err(e) => return Ok(Err(status(e))),
            }
        }
        finish(k, b, expect)
    })?;
    Ok((body, placed, route))
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
/// first segment of the section before it. A section matched to more than `max_segments`
/// segments is refused.
///
/// # Errors
/// [`Error::Argument`] for a section with no segments, no length or a point that is not finite,
/// a plane axis that is not a finite unit vector, or a match past `max_segments`; [`Error::Invariant`]
/// when the pairing breaks down.
pub fn match_loft_sections(
    sections: &[LoftSection],
    max_segments: usize,
) -> Result<Vec<MatchedSection>> {
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
    let matched = kl::match_loft_sections(&given, max_segments).map_err(argument_or_status)?;
    Ok(matched
        .into_iter()
        .map(|m| MatchedSection {
            segs: m.segs.iter().map(convert::record_seg2).collect(),
            from: m.from,
        })
        .collect())
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
        match create_skin_primitive(k, &counts, &made.refs(), degree as usize, cap) {
            Ok(r) => finish(k, r.brep, if cap { Expect::SOLID } else { Expect::FACES }),
            Err(e) => Ok(Err(status(e))),
        }
    })
}

/// A pyramid from the planar `section` (one closed loop of lines, in order) to `apex` (off the
/// section's plane): the base and a triangle on each line, stitched and oriented.
///
/// # Errors
/// [`Error::Argument`] for a section that is not one closed loop of lines, or an apex not
/// proved off its plane; the kernel's status; [`Error::Empty`], [`Error::NotSolid`].
pub fn loft_to_point(section: &[Curve], apex: [f64; 3]) -> Result<Body> {
    if !coord_ok(&apex) {
        return Err(Error::Argument);
    }
    making("loft_to_point", |k| {
        let made = parsed!(parse(k, section));
        let curves = &made.curves;
        if curves.len() < 3 || !path::closed_loop(k, curves, 1e-6)?.is_yes() {
            return Ok(Err(Error::Argument));
        }
        let mut corners = Vec::new();
        for c in curves {
            if !matches!(c.kind(), CurveKind::Line(_)) {
                return Ok(Err(Error::Argument));
            }
            corners.push(c.ends(k)?.start);
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
            let mut face =
                |b: &mut Brep, fenv: &mut FaceBuildEnv<'_, '_>, pts: &[Point3]| -> Out<()> {
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
            face(b, fenv, &corners)?;
            for (i, &p) in corners.iter().enumerate() {
                let q = corners
                    .get(i + 1)
                    .or_else(|| corners.first())
                    .copied()
                    .unwrap_or(p);
                face(b, fenv, &[p, q, top])?;
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
        finish(k, b, Expect::SOLID)
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
        finish(
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
/// pairs as it turns (`sweep::screw`).
///
/// # Errors
/// [`Error::Argument`] for a number, law or curve list refused (and the kernel's refusals of
/// them); the kernel's status; [`Error::Empty`], [`Error::NotSolid`].
pub fn screw_sweep(
    profile: &[Curve],
    origin: [f64; 3],
    axis: [f64; 3],
    pitch: f64,
    turns: f64,
    law: Option<&[(f64, f64)]>,
    tolerance: f64,
) -> Result<Body> {
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
    making(call, |k| {
        let made = parsed!(parse(k, profile));
        let built = match law {
            None => screw::screw_sweep_body(
                k,
                &made.curves,
                v3(origin),
                v3(axis),
                pitch,
                turns,
                tolerance,
            ),
            Some(l) => screw::screw_sweep_radial_body(
                k,
                &made.curves,
                v3(origin),
                v3(axis),
                pitch,
                turns,
                l,
                tolerance,
            ),
        };
        match built {
            Ok(b) => finish(k, b, Expect::SOLID),
            Err(e) => Ok(Err(argument_or_status(e))),
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
    with_body("drop_curves", body, |k, b| {
        let given = parsed!(parse(k, curves));
        let face = parsed!(face_of(b, order, face));
        let domain = b.face(face)?.uv_domain;
        let Some(owned) = b.face(face)?.surface.as_ref() else {
            return Ok(Err(Error::Argument));
        };
        // A standalone copy (its owner not copied): the drop and lift work over the face's
        // parameter box, and a face-owned surface's solves would ask for the face's trimmed
        // cache, which they are not given here.
        let surface = owned.copy(k)?;
        let mut out = Vec::new();
        for c in &given.curves {
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
            for uv in &dropped.uv_curves {
                let lifted = match surface.lift_curve(
                    k,
                    &domain,
                    uv,
                    &uv.natural_interval(),
                    tolerance,
                    false,
                ) {
                    Ok(l) => l,
                    Err(e) => return Ok(Err(status(e))),
                };
                match put_curve(&lifted.curve)? {
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
}
