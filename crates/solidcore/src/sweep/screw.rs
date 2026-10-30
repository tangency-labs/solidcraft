//! Screw sweeps: a profile carried by a screw motion about an axis (a thread's groove, a
//! coil), each wall its profile curve turned exactly and lifted within a tolerance of
//! its true screw surface, capped at both ends.
//!
//! Source: the SolidCraft fork's C API, `capi/src/smlib_sweeps.cpp` of
//! tangency-labs/smlib at `83748ba` (Apache-2.0), commits `d6d44b4` (the screw sweep),
//! `9ddb350` (walls within the tolerance asked, end spans included, #1162), `a19e7bf`
//! (a profile line along the axis sweeps an exact cylinder, #1163), `9354003`
//! (`smlib_screw_sweep_radial`) and `9d665d0` (no radial law adds no offset). It is not
//! part of SMLib: the construction uses only kernel calls that are.
//!
//! | Rust                         | Fork (`capi/src/smlib_sweeps.cpp`)  |
//! |------------------------------|-------------------------------------|
//! | [`screw_sweep`]              | `smlib_screw_sweep`                 |
//! | [`screw_sweep_radial`]       | `smlib_screw_sweep_radial`          |
//! | [`screw_sweep_body`], [`screw_sweep_radial_body`] | the same, up to `finish` (not included) |
//! | `Screw` (private)            | `Screw`                             |
//! | `Stations` (private)         | `Stations`                          |
//! | `lift_error` (private)       | `lift_error`                        |
//! | `offset_error` (private)     | `offset_error`                      |
//! | `path` (private)             | `path`                              |
//! | `nurbs_of` (private)         | `nurbs_of`                          |
//! | `path_curve` (private)       | `path_curve`                        |
//! | `axial_radius` (private)     | `axial_radius`                      |
//! | `cylinder_piece` (private)   | `cylinder_piece`                    |
//! | `screw_wall` (private)       | `screw_wall`                        |
//! | `sweep` (private)            | `screw_sweep`                       |
//!
//! Deviations:
//! - **Inputs.** The profile is a slice of [`Curve`]s (the fork parses a flat curve
//!   list); each must be an `SmBSplineCurve` (a B-spline, line or conic) of dimension 3.
//!   The radial law is `(turn, offset)` pairs. The result is the new [`Brep`]; a
//!   rejected input is [`SolidError::InvalidInput`] (`SMLIB_ERR_ARG`), and a result that
//!   is not a closed manifold solid is `SM_ERR` (`SMLIB_ERR_NOT_SOLID`). The `_body`
//!   variants stop where the fork hands the body to its `finish`, without that check,
//!   so a C API port can give `SMLIB_ERR_EMPTY` / `SMLIB_ERR_NOT_SOLID` itself.
//! - **Bounds.** As the fork: at most [`MAX_TURNS`] turns, [`MAX_STATIONS`] spans in all
//!   and [`MAX_LAW`] law pairs. Added: the span width's shrinking loop stops after
//!   [`MAX_SHRINK`] steps, and a wall's control net holds at most [`MAX_NET_POINTS`]
//!   points; past either the sweep is `InvalidInput`.
//! - **Faces.** Faces are made under arena notification identities and stitched under
//!   them, as the kernel's own primitives are; the result's cached topology is
//!   refreshed ([`Brep::refresh_cached_topology`]).

// The fork's range tests are NaN-safe negations (`!(x > y)`); kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::f64::consts::TAU;

use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::cone::Cone;
use crate::surface::surface::Surface;
use crate::topology::brep::p8::{self, FaceBuildEnv};
use crate::topology::{Brep, FaceId};

const OP: &str = "sweep::screw::screw_sweep";

/// The most turns a screw sweep takes (`SMLIB_SCREW_MAX_TURNS`).
pub const MAX_TURNS: f64 = 200.0;
/// The most spans a screw sweep's walls take in all (`SMLIB_SCREW_MAX_STATIONS`).
pub const MAX_STATIONS: f64 = 12000.0;
/// The most `(turn, offset)` pairs of a radial law (`SMLIB_SCREW_MAX_LAW`).
pub const MAX_LAW: usize = 1024;
/// The most steps the span width shrinks by 1% to meet the tolerance (not in the fork,
/// whose loop ends because the lift error vanishes with the span).
pub const MAX_SHRINK: usize = 20_000;
/// The most control points of one wall (profile curve control points times
/// `2 * spans + 1`; not in the fork).
pub const MAX_NET_POINTS: usize = 4_000_000;
/// A coordinate no larger than this (the fork's `MAX_COORD`: mm, 1000 km).
const MAX_COORD: f64 = 1e9;

fn bad(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// `std::min`: `b` when it is less, else `a`.
fn min2(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `std::max`: `b` when `a` is less, else `a`.
fn max2(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// The screw motion: about the axis through `o` along unit `k`, rising `pitch` per turn,
/// the section carried by angle `a` (radians, right-handed about `k`), and first moved
/// off the axis by `offset(a)` along `e` (the radial variant: the unit direction from the
/// axis into the profile's half-plane), `offset(a)` piecewise linear through (`angles`,
/// `offsets`) and held beyond them. No offsets: none.
struct Screw {
    o: Point3,
    k: Vec3,
    e: Vec3,
    pitch: f64,
    angles: Vec<f64>,
    offsets: Vec<f64>,
}

impl Screw {
    /// Rodrigues: `v` turned right-handed about `k` by angle `a`.
    fn turn(&self, v: Vec3, a: f64) -> Vec3 {
        v * a.cos() + self.k.cross(v) * a.sin() + self.k * (self.k.dot(v) * (1.0 - a.cos()))
    }

    fn lift(&self, a: f64) -> f64 {
        self.pitch * a / TAU
    }

    fn offset(&self, a: f64) -> f64 {
        let (Some(&a0), Some(&o0)) = (self.angles.first(), self.offsets.first()) else {
            return 0.0;
        };
        if !(a > a0) {
            return o0;
        }
        for (wa, wo) in self.angles.windows(2).zip(self.offsets.windows(2)) {
            if let (&[a_prev, a_i], &[o_prev, o_i]) = (wa, wo) {
                if a <= a_i {
                    return o_prev + (o_i - o_prev) * (a - a_prev) / (a_i - a_prev);
                }
            }
        }
        self.offsets.last().copied().unwrap_or(0.0)
    }

    /// The move off the axis at angle `a`, before the turn: `e * offset(a)`, or none
    /// without a law (`e` is then unset).
    fn shift(&self, a: f64) -> Vec3 {
        if self.offsets.is_empty() {
            Vec3::ZERO
        } else {
            self.e * self.offset(a)
        }
    }

    /// Where `p` goes at angle `a`.
    fn at(&self, p: Point3, a: f64) -> Point3 {
        self.o + self.turn(p - self.o + self.shift(a), a) + self.k * self.lift(a)
    }

    /// How far `p` lies from the axis.
    fn reach(&self, p: Point3) -> f64 {
        let w = p - self.o;
        (w - self.k * w.dot(self.k)).length()
    }

    /// The motion as a placement of the section:
    /// `p -> o + R (p - o + e offset) + k lift`, its origin
    /// `o - R o + R e offset + k lift`.
    fn placement(&self, a: f64) -> Axis2Placement {
        let origin = self.o - self.turn(Vec3::new(self.o.x, self.o.y, self.o.z), a)
            + self.turn(self.shift(a), a)
            + self.k * self.lift(a);
        Axis2Placement::new(origin, self.turn(Vec3::X, a), self.turn(Vec3::Y, a))
    }
}

/// The stations of a piece of the turns: `n` spans of `h` radians each, from `t[0]` to
/// `t[n]`. Each span carries the profile round as an exact rational quadratic arc does
/// a point (its middle control point at the span's middle angle, out by
/// `1 / cos(h / 2)`, weighted `cos(h / 2)`), and lifts it by the same rational
/// quadratic of the lifts at the span's ends and middle: every point of a wall lies
/// exactly as far from the axis as its profile point and exactly at the angle the arc
/// turns it to, only lifted off the screw surface there, by at most
/// `|pitch| / (2 pi) * lift_error(h)`. A radial offset that changes by `d` across a
/// span strays at most `|d| * offset_error(h)` more.
struct Stations {
    t: Vec<f64>,
    /// `h / 2`.
    half: f64,
}

impl Stations {
    fn new(from: f64, to: f64, n: usize) -> Self {
        let mut t = Vec::new();
        for i in 0..=n {
            t.push(if i == n {
                to
            } else {
                from + (to - from) * i as f64 / n as f64
            });
        }
        Self {
            t,
            half: 0.5 * (to - from) / n as f64,
        }
    }

    fn spans(&self) -> usize {
        self.t.len().saturating_sub(1)
    }

    /// The knots across the stations, as unique values and multiplicities: double
    /// inside (the arcs meet tangent), triple at the ends.
    fn knots(&self) -> (Vec<f64>, Vec<usize>) {
        let n = self.t.len();
        let mult = (0..n)
            .map(|i| if i == 0 || i + 1 == n { 3 } else { 2 })
            .collect();
        (self.t.clone(), mult)
    }

    /// The weight of control point `j` (of `2 * spans + 1`) across the stations.
    fn weight(&self, j: usize) -> f64 {
        if j % 2 == 0 {
            1.0
        } else {
            self.half.cos()
        }
    }
}

/// How far, in radians of turn, the rational quadratic lift of a span of half-angle
/// `half` leads or lags the true lift, at most (sampled: the difference is smooth, odd
/// and nought at the span's middle and ends, about `0.008 h^3`).
fn lift_error(half: f64) -> f64 {
    let mut worst: f64 = 0.0;
    for i in 0..=2048 {
        let s = f64::from(i) / 2048.0;
        let lift = half * s / (1.0 - (1.0 - s * s) * (1.0 - half.cos()) / 2.0);
        worst = max2(worst, (lift - 2.0 * ((half / 2.0).tan() * s).atan()).abs());
    }
    worst
}

/// How far, per unit change of offset across a span of half-angle `half`, the rational
/// quadratic offset strays from the true offset turned with the profile, at most
/// (about `h / 8`).
fn offset_error(half: f64) -> f64 {
    let c = half.cos();
    let h = 2.0 * half;
    let mut worst: f64 = 0.0;
    for i in 0..=4096 {
        let s = -1.0 + f64::from(i) / 2048.0;
        let t = 0.5 * (s + 1.0);
        let b0 = (1.0 - t) * (1.0 - t);
        let b1 = 2.0 * t * (1.0 - t);
        let b2 = t * t;
        let den = b0 + c * b1 + b2;
        let ax = 0.5 * (b2 - b0) * c / den;
        let ay = 0.5 * (b2 + b0) * half.sin() / den;
        let phi = 2.0 * ((half / 2.0).tan() * s).atan();
        worst = max2(
            worst,
            (ax - phi / h * phi.cos()).hypot(ay - phi / h * phi.sin()),
        );
    }
    worst
}

/// The control points of `p`'s path under the screw motion over `st` (`2 * spans + 1`
/// of them).
fn path(m: &Screw, p: Point3, st: &Stations) -> Vec<Point3> {
    let w = p - m.o;
    let axial = w.dot(m.k);
    let radial = w - m.k * axial;
    let mut d = Vec::new();
    for pair in st.t.windows(2) {
        if let [t0, t1] = *pair {
            d.push(m.at(p, t0));
            let mid = 0.5 * (t0 + t1);
            d.push(
                m.o + m.turn(radial + m.shift(mid), mid) * (1.0 / st.half.cos())
                    + m.k * (axial + m.lift(mid)),
            );
        }
    }
    if let Some(&last) = st.t.last() {
        d.push(m.at(p, last));
    }
    d
}

/// A profile curve as the NURBS its control points pair up in.
struct Nurbs {
    degree: usize,
    points: Vec<Point3>,
    mult: Vec<usize>,
    unique: Vec<f64>,
    weights: Vec<f64>,
}

/// The `SmBSplineCurve` part of a profile curve (`SM_CAST_PTR(SmBSplineCurve, ..)`).
fn bspline_of(curve: &Curve) -> Result<BSplineCurve> {
    match curve.clone().into_kind() {
        CurveKind::BSpline(b) => Ok(b),
        CurveKind::Line(l) => Ok(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Ok(e.base().clone()),
        CurveKind::Hyperbola(h) => Ok(h.base().clone()),
        CurveKind::Parabola(p) => Ok(p.base().clone()),
        _ => Err(SolidError::sm_err(OP)),
    }
}

fn nurbs_of(curve: &Curve) -> Result<Nurbs> {
    let c = bspline_of(curve)?
        .canonical()
        .map_err(|_| SolidError::sm_err(OP))?;
    if c.dim != 3 {
        return Err(SolidError::sm_err(OP));
    }
    if c.control_points
        .iter()
        .any(|p| !(p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD && p.z.abs() <= MAX_COORD))
    {
        return Err(bad("a profile control point is not finite or out of range"));
    }
    Ok(Nurbs {
        degree: c.degree,
        points: c.control_points,
        mult: c.knot_multiplicities,
        unique: c.knots,
        weights: c.weights,
    })
}

/// The path of `p` over `st` as a curve: the edge a profile vertex traces, exactly as
/// the walls on either side of it have it.
fn path_curve(m: &Screw, p: Point3, st: &Stations) -> Result<Curve> {
    let d = path(m, p, st);
    let weights: Vec<f64> = (0..d.len()).map(|j| st.weight(j)).collect();
    let (knots, mult) = st.knots();
    let c = BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: 2,
        control_points: &d,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mult,
        knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        trim: None,
    })?;
    Ok(Curve::from(c))
}

/// A profile line parallel to the axis sweeps the cylinder its points turn on: its
/// radius when `line` runs within a thousandth of `tolerance` of parallel to the axis,
/// at least `1e-6` off it; `0` otherwise.
fn axial_radius(k: &Kernel, m: &Screw, line: &Curve, tolerance: f64) -> Result<f64> {
    if !matches!(line.kind(), CurveKind::Line(_)) {
        return Ok(0.0);
    }
    let ends = line.ends(k)?;
    let (a, b) = (ends.start, ends.end);
    // How far a point of the line strays from the axis lies between the ends' and the
    // nearest point's (the distance is convex along the line).
    let w = a - m.o;
    let d = b - a;
    let wp = w - m.k * w.dot(m.k);
    let dp = d - m.k * d.dot(m.k);
    let t = if dp.dot(dp) > 0.0 {
        (-wp.dot(dp) / dp.dot(dp)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let ra = m.reach(a);
    let rb = m.reach(b);
    let nearest = (wp + dp * t).length();
    let far = max2(ra, rb);
    let near = min2(nearest, min2(ra, rb));
    if !(near >= 1e-6 && far - near <= 1e-3 * tolerance) {
        return Ok(0.0);
    }
    Ok(0.5 * (ra + rb))
}

/// Runs `body` with a face-construction environment over an arena-only notification
/// store, as the kernel's primitives build their faces.
fn with_face_env<T>(
    k: &mut Kernel,
    b: &mut Brep,
    body: impl FnOnce(&mut Kernel, &mut Brep, &mut FaceBuildEnv<'_, '_>) -> Result<T>,
) -> Result<T> {
    let mut state = crate::topology::brep::p2::ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = crate::topology::topology_traverser::Marks::default();
    let this_face = |f: FaceId| crate::topology::loopuse::notify_ref(EntityRef::Face(f));
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    body(k, b, &mut fenv)
}

/// The piece of the cylinder of `radius` the profile line from `a` to `b` sweeps over
/// the stations of `st` (at most 202.5 degrees of turn): an exact cylinder about the
/// axis, bounded by the line placed at both ends of the piece and its ends' paths. The
/// cylinder's own turn starts a quarter turn before the piece and its height runs past
/// it, so the piece lies inside its domain.
fn cylinder_piece(
    k: &mut Kernel,
    brep: &mut Brep,
    m: &Screw,
    a: Point3,
    b: Point3,
    radius: f64,
    st: &Stations,
) -> Result<()> {
    let (Some(&a0), Some(&a1)) = (st.t.first(), st.t.last()) else {
        return Err(SolidError::sm_err(OP));
    };
    let w = a - m.o;
    let radial = w - m.k * w.dot(m.k);
    let e = radial * (1.0 / radial.length());
    let x = m.turn(e, a0 - 0.25 * TAU);
    let y = m.k.cross(x);
    let z0 = min2((a - m.o).dot(m.k), (b - m.o).dot(m.k)) + min2(m.lift(a0), m.lift(a1));
    let z1 = max2((a - m.o).dot(m.k), (b - m.o).dot(m.k)) + max2(m.lift(a0), m.lift(a1));
    let margin = 0.25 * (z1 - z0) + 1e-3;
    let base = m.o + m.k * (z0 - margin);
    let cylinder = Cone::new_cylinder_bounded(
        base,
        x,
        y,
        radius,
        0.0,
        360.0,
        z1 - z0 + 2.0 * margin,
        false,
        false,
        false,
    );
    let line = |at: f64| -> Result<Curve> {
        Ok(Curve::from(Line::create_line_segment(
            3,
            m.at(a, at),
            m.at(b, at),
            None,
        )?))
    };
    let curves = vec![
        Some(line(a0)?),
        Some(path_curve(m, b, st)?),
        Some(line(a1)?),
        Some(path_curve(m, a, st)?),
    ];
    let orient = [
        OrientType::Same,
        OrientType::Same,
        OrientType::Opposite,
        OrientType::Opposite,
    ];
    let made = with_face_env(k, brep, |k, brep, fenv| {
        let region = brep.infinite_region();
        p8::create_faces_with_curves(
            k,
            brep,
            region,
            &[4],
            Some(curves),
            &orient,
            Some(Surface::from(cylinder)),
            fenv,
        )
    })?;
    if made.faces.len() != 1 {
        return Err(SolidError::sm_err(OP));
    }
    Ok(())
}

/// The wall one profile curve sweeps over `st`: across the profile curve its NURBS,
/// across the turns each control point's path, the weights the curve's times the
/// arcs'. A wall's point is the curve's weighted mean of its control points' paths at
/// the same parameter, which all turn and lift alike, so the wall is the profile curve
/// turned and lifted as one.
fn screw_wall(m: &Screw, c: &Nurbs, st: &Stations) -> Result<BSplineSurface> {
    let count = c.points.len();
    let across = 2 * st.spans() + 1;
    let size = count
        .checked_mul(across)
        .filter(|&n| n <= MAX_NET_POINTS)
        .ok_or_else(|| bad("a wall's control net is too large"))?;
    let mut net = Vec::new();
    let mut weights = Vec::new();
    for (l, &p) in c.points.iter().enumerate() {
        let w = c.weights.get(l).copied().unwrap_or(1.0);
        for (j, q) in path(m, p, st).into_iter().enumerate() {
            net.push(q);
            weights.push(w * st.weight(j));
        }
    }
    if net.len() != size {
        return Err(SolidError::sm_err(OP));
    }
    let (vknots, vmult) = st.knots();
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: c.degree,
        v_degree: 2,
        control_points: &net,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &c.mult,
        v_multiplicities: &vmult,
        u_knots: &c.unique,
        v_knots: &vknots,
        knot_type: KnotType::Unspecified,
        weights: Some(&weights),
        uv_domain: None,
    })
}

/// Stitches and orients the B-rep under the identities its faces were made with.
fn stitch(k: &mut Kernel, b: &mut Brep) -> Result<()> {
    use crate::query::brep::BrepIdentity;
    use crate::topology::brep::p5::{stitch_and_orient_with_identity, StitchAndOrientOptions};
    let key_of: &dyn Fn(EntityRef) -> crate::geom::object::ObjectRef =
        &crate::topology::loopuse::notify_ref;
    let keys = |object| crate::topology::brep::p2::caller_cache_key(key_of, object);
    stitch_and_orient_with_identity(
        k,
        b,
        &StitchAndOrientOptions::default(),
        Some(BrepIdentity {
            keys: &keys,
            ..BrepIdentity::default()
        }),
    )
}

/// The common checks of both entry points; the unit axis.
fn screw_of(origin: Point3, axis: Vec3, pitch: f64, turns: f64, tolerance: f64) -> Result<Screw> {
    let coord_ok =
        |v: Point3| v.x.abs() <= MAX_COORD && v.y.abs() <= MAX_COORD && v.z.abs() <= MAX_COORD;
    let in_range = coord_ok(origin)
        && pitch.is_finite()
        && pitch.abs() > 1e-6
        && pitch.abs() < MAX_COORD
        && turns.is_finite()
        && (1e-3..=MAX_TURNS).contains(&turns)
        && tolerance.is_finite()
        && (1e-6..1e3).contains(&tolerance);
    if !in_range {
        return Err(bad("origin, pitch, turns or tolerance out of range"));
    }
    if !(axis.x.is_finite() && axis.y.is_finite() && axis.z.is_finite()) {
        return Err(bad("axis is not finite"));
    }
    let n = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
    if !(n > 1e-12) || !n.is_finite() {
        return Err(bad("axis has no usable length"));
    }
    Ok(Screw {
        o: origin,
        k: Vec3::new(axis.x / n, axis.y / n, axis.z / n),
        e: Vec3::ZERO,
        pitch,
        angles: Vec::new(),
        offsets: Vec::new(),
    })
}

/// The screw sweep of `profile` (closed, planar, in a half-plane bounded by the axis
/// through `origin` along `axis`): `turns` (`0.001` to [`MAX_TURNS`]) turns right-handed
/// about the axis, rising `pitch` (signed, non-zero) per turn, capped at both ends. Each
/// wall is its profile curve turned exactly (rational quadratic arcs across the turns:
/// every point of it lies as far from the axis as its profile point) and lifted within
/// `tolerance` (`>= 1e-6`) of its true screw surface, end to end. A profile line
/// parallel to the axis sweeps an exact cylinder; then every wall comes in faces of
/// half a turn (a cylinder face cannot turn on itself). At most [`MAX_STATIONS`] spans.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an input out of range (as the fork's
/// `SMLIB_ERR_ARG`), a profile on the axis or too many spans; `SM_ERR` for a profile
/// curve that is not a 3D `SmBSplineCurve` or a result that is not a closed manifold
/// solid; those of the face construction and the stitch.
pub fn screw_sweep(
    k: &mut Kernel,
    profile: &[Curve],
    origin: Point3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    tolerance: f64,
) -> Result<Brep> {
    solid(screw_sweep_body(
        k, profile, origin, axis, pitch, turns, tolerance,
    )?)
}

/// [`screw_sweep`] up to the fork's `finish`: the stitched and oriented body, its cached
/// topology refreshed, without the check that it has faces and is a closed manifold
/// solid.
///
/// # Errors
///
/// Those of [`screw_sweep`] but the closed-solid check.
pub fn screw_sweep_body(
    k: &mut Kernel,
    profile: &[Curve],
    origin: Point3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    tolerance: f64,
) -> Result<Brep> {
    let mut m = screw_of(origin, axis, pitch, turns, tolerance)?;
    sweep(k, profile, &mut m, turns, tolerance)
}

/// [`screw_sweep`] with the section moved off the axis as it turns: a thread's groove
/// fading in or out over a runout, or a taper. At turn `t` (`0` to `turns`) every point
/// of the profile is first moved `offset(t)` away from the axis (negative: towards it)
/// within the profile's half-plane, then carried by the screw motion. `offset` is
/// piecewise linear through the `law` pairs `(turn, offset)` (1 to [`MAX_LAW`]), turns
/// strictly ascending within `[0, turns]`, and held at its first and last offsets
/// before and after them.
///
/// # Errors
///
/// Those of [`screw_sweep`]; [`SolidError::InvalidInput`] for a law out of range, a
/// profile not within one half-plane of the axis, or an offset that moves it onto the
/// axis.
pub fn screw_sweep_radial(
    k: &mut Kernel,
    profile: &[Curve],
    origin: Point3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
    tolerance: f64,
) -> Result<Brep> {
    solid(screw_sweep_radial_body(
        k, profile, origin, axis, pitch, turns, law, tolerance,
    )?)
}

/// [`screw_sweep_radial`] up to the fork's `finish`, as [`screw_sweep_body`].
///
/// # Errors
///
/// Those of [`screw_sweep_radial`] but the closed-solid check.
pub fn screw_sweep_radial_body(
    k: &mut Kernel,
    profile: &[Curve],
    origin: Point3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
    tolerance: f64,
) -> Result<Brep> {
    let mut m = screw_of(origin, axis, pitch, turns, tolerance)?;
    if law.is_empty() || law.len() > MAX_LAW {
        return Err(bad("radial law size out of range"));
    }
    let mut prev: Option<f64> = None;
    for &(turn, offset) in law {
        if !(turn.is_finite() && offset.is_finite()) {
            return Err(bad("radial law is not finite"));
        }
        if !(0.0..=turns).contains(&turn)
            || prev.is_some_and(|p| !(turn > p))
            || !(offset.abs() < MAX_COORD)
        {
            return Err(bad("radial law out of range"));
        }
        prev = Some(turn);
    }
    for &(turn, offset) in law {
        m.angles.push(TAU * turn);
        m.offsets.push(offset);
    }
    sweep(k, profile, &mut m, turns, tolerance)
}

/// `screw_sweep` of the fork: both entry points, the motion `m` set but for its radial
/// direction.
fn sweep(
    k: &mut Kernel,
    profile: &[Curve],
    m: &mut Screw,
    turns: f64,
    tolerance: f64,
) -> Result<Brep> {
    if profile.is_empty() {
        return Err(bad("empty profile"));
    }
    // Each profile curve's wall is built from the motion itself (Stations, screw_wall):
    // turned exactly, lifted within |pitch| / (2 pi) * lift_error(h) of the screw
    // surface, the spans h no wider than that keeps within `tolerance`, nor than an
    // eighth of a turn.
    let mut walls = Vec::new();
    let mut reach = 0.0;
    let mut farthest = Vec3::ZERO;
    for curve in profile {
        let w = nurbs_of(curve)?;
        for &p in &w.points {
            let mut v = p - m.o;
            v -= m.k * v.dot(m.k);
            if v.length() > reach {
                reach = v.length();
                farthest = v;
            }
        }
        walls.push(w);
    }
    if !(reach > 1e-6) {
        return Err(bad("the profile lies on the axis"));
    }
    // An offset moves the profile within its half-plane: every control point must lie
    // in the one the farthest of them spans with the axis, and stay off the axis moved.
    if !m.offsets.is_empty() {
        let mut nearest = reach;
        m.e = farthest * (1.0 / reach);
        for w in &walls {
            for &p in &w.points {
                let mut v = p - m.o;
                v -= m.k * v.dot(m.k);
                let along = v.dot(m.e);
                if !((v - m.e * along).length() <= 1e-9 * (1.0 + reach)
                    && along >= -1e-9 * (1.0 + reach))
                {
                    return Err(bad("the profile is not within one half-plane of the axis"));
                }
                nearest = min2(nearest, along);
            }
        }
        for &d in &m.offsets {
            if !(nearest + d > 1e-6) {
                return Err(bad("an offset moves the profile onto the axis"));
            }
        }
    }
    // Walls of lines parallel to the axis are exact cylinders, which cannot turn on
    // themselves: then every wall comes in pieces of half a turn (the last one up to an
    // eighth longer), each its own face, so the walls on both sides of an edge share
    // its vertices.
    let mut radius = Vec::new();
    let mut cylinders = false;
    for curve in profile {
        let r = axial_radius(k, m, curve, tolerance)?;
        cylinders = cylinders || r > 0.0;
        radius.push(r);
    }
    // The offset changes slope only at its own angles: they bound pieces too, and a
    // half turn within a sixteenth of a turn of one is left out.
    let total = TAU * turns;
    let mut breaks = vec![0.0];
    for &a in &m.angles {
        if a > 0.0 && a < total {
            breaks.push(a);
        }
    }
    let mut half = 0.5 * TAU;
    while cylinders && half < total - TAU / 16.0 {
        let near = m.angles.iter().any(|&a| (a - half).abs() < TAU / 16.0);
        if !near {
            breaks.push(half);
        }
        half += 0.5 * TAU;
    }
    breaks.push(total);
    breaks.sort_by(f64::total_cmp);
    let rise = m.pitch.abs() / TAU;
    let mut h = min2(TAU / 8.0, (tolerance / (0.008 * rise)).cbrt());
    let mut shrink = 0;
    while rise * lift_error(0.5 * h) > tolerance {
        h *= 0.99;
        shrink += 1;
        if shrink > MAX_SHRINK {
            return Err(bad("no span width meets the tolerance"));
        }
    }
    let mut pieces = Vec::new();
    let mut spans = 0.0;
    for pair in breaks.windows(2) {
        let [b0, b1] = *pair else {
            continue;
        };
        // Where the offset changes, its spans are narrower still.
        let rate = (m.offset(b1) - m.offset(b0)).abs() / (b1 - b0);
        let mut piece = h;
        while rise * lift_error(0.5 * piece) + rate * piece * offset_error(0.5 * piece) > tolerance
            && piece > 1e-4
        {
            piece *= 0.99;
        }
        let n = ((b1 - b0) / piece).ceil();
        spans += n;
        if !(spans <= MAX_STATIONS) {
            return Err(bad("too many spans"));
        }
        pieces.push(Stations::new(b0, b1, n as usize));
    }
    let mut brep = crate::topology::brep::p3::new_brep(
        k,
        crate::geom::tol::USE_DEFAULT,
        crate::geom::tol::USE_DEFAULT,
    )?;
    for st in &pieces {
        let (Some(&t0), Some(&t1)) = (st.t.first(), st.t.last()) else {
            return Err(SolidError::sm_err(OP));
        };
        for ((wall, curve), &r) in walls.iter().zip(profile).zip(&radius) {
            if r > 0.0 && m.offset(t0) == m.offset(t1) {
                let ends = curve.ends(k)?;
                cylinder_piece(k, &mut brep, m, ends.start, ends.end, r + m.offset(t0), st)?;
                continue;
            }
            let surface = screw_wall(m, wall, st)?;
            let domain: Extent2d = surface.natural_uv_domain();
            with_face_env(k, &mut brep, |k, brep, fenv| {
                p8::create_face_from_surface(k, brep, Some(Surface::from(surface)), &domain, fenv)
            })?;
        }
    }
    // The caps: the profile where the motion starts and where it ends, placed exactly.
    for a in [0.0, total] {
        let placement = m.placement(a);
        let mut cap = Vec::new();
        for src in profile {
            let mut copy = src.copy(k).map_err(|_| SolidError::sm_err(OP))?;
            copy.transform(k, &placement, None)
                .map_err(|_| SolidError::sm_err(OP))?;
            cap.push(copy);
        }
        with_face_env(k, &mut brep, |k, brep, fenv| {
            let tol = brep.tolerance();
            let region = brep.infinite_region();
            p8::create_planar_faces_with_3d_curves(k, brep, region, &cap, tol, fenv)
        })?;
    }
    stitch(k, &mut brep)?;
    brep.refresh_cached_topology();
    Ok(brep)
}

/// The fork's `finish` for a solid: the body must have faces and be a closed manifold
/// solid (`SM_ERR` otherwise).
fn solid(brep: Brep) -> Result<Brep> {
    if crate::topology::brep::p1::faces(&brep, None)?.is_empty()
        || !crate::topology::brep::p2::is_manifold_solid(&brep)?
    {
        return Err(SolidError::sm_err(OP));
    }
    Ok(brep)
}
