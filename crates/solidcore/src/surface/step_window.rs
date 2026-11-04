//! (not upstream) A face's STEP parameter window from its boundary: the box in the
//! surface's STEP (u, v) parameters that the face's bounding edges span on it, each
//! edge's extremes found exactly, unwound along the surface's periodic directions.
//!
//! Upstream: none (solidcore's STEP reader, #2578). The idea is SolidCraft's reader
//! (`crates/step/src/topology/swept.rs` `span`, `range`, `trimmed_range`,
//! `widest_gap`, `common_angle`, `crates/step/src/topology.rs` `Builder::extent` and
//! `face_extent`, and `crates/geom/src/brep/form.rs` `extent`, at a5b71ca2; MIT OR
//! Apache-2.0), which samples each edge; here each edge's extremes are its
//! stationary points. The computations are the kernel's own: the STEP
//! parameterisations of [`Surface::evaluate_step_point`] (in closed form for planes,
//! cones, cylinders, spheres and tori, and the angle of a surface of revolution),
//! [`Surface::step_inversion`] and [`Surface::evaluate_step`] for the other kinds and
//! a revolution's profile parameter, [`Curve::evaluate`] and the line's and
//! ellipse's STEP evaluators for the edges, and [`LocalSolve1d::solve_by_bisection`]
//! for the stationary points without a closed form.
//!
//! | Rust                 | SolidCraft                                                  |
//! |----------------------|-------------------------------------------------------------|
//! | [`face_window`]      | `form::extent`, `Builder::extent`, `face_extent`; `swept::span`, `range`, `trimmed_range`, `widest_gap`, `common_angle` (a swept surface's profile range is its window's v) |
//! | [`BoundaryEdge`]     | `form::extent`'s `(&Curve, [f64; 2], bool)` loop entries    |
//! | [`WindowMargin`]     | `swept::MARGIN` (and step-brep's 5 %): ops' choice now      |
//!
//! Deviations: not a port. Recursion: halving a piece of an edge to decide its turn
//! round a period stops at [`MAX_SPLIT_DEPTH`] levels.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::local_solve1d::{EvalFunction, FunctionValue, LocalSolve1d};
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::EvaluateFlags;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::step_window::face_window";

/// The STEP period of an angle, in degrees.
const PERIOD: f64 = 360.0;

/// Levels a piece of an edge is halved to decide its turn round a period (each level
/// halves the piece's parameter range; a piece is decided once it turns by at most
/// half a period).
pub const MAX_SPLIT_DEPTH: usize = 12;

/// Halvings a piece of an edge always gets before its turn is read off its ends: a
/// whole turn has equal ends, its quarters do not.
const MIN_SPLIT_DEPTH: usize = 2;

/// Regula falsi steps for a stationary point without a closed form (each step keeps
/// at most 85 % of the bracket, so 200 steps reach the parameter's rounding).
const ROOT_STEPS: u64 = 200;

/// One edge of a face's boundary loop, as the loop uses it: the edge's curve, its
/// STEP interval on that curve and the direction the loop runs along it.
///
/// A face's loops are lists of these in loop order, each edge running on from where
/// the one before it ends. The loops are oriented as they bound the face: the face
/// lies on their left about the surface's STEP normal `Su x Sv` (a file's
/// `FACE_BOUND.orientation` applied, and every loop reversed when the face's
/// `same_sense` is false).
#[derive(Debug, Clone, Copy)]
pub struct BoundaryEdge<'a> {
    /// The edge's curve (for a `CIRCLE` or an `ELLIPSE` the full conic), whose STEP
    /// parameters [`Self::interval`] is in (those of [`Curve::evaluate_step_point`]).
    pub curve: &'a Curve,
    /// The edge's STEP interval on [`Self::curve`], lower parameter first: the
    /// [`crate::curve::step_edge::StepEdgeInterval::interval`] of
    /// [`crate::curve::step_edge::edge_interval`].
    pub interval: Extent1d,
    /// The loop runs along the curve's parameter, from the interval's minimum to its
    /// maximum; when false, from the maximum down to the minimum.
    pub forward: bool,
}

/// The margin [`face_window`] adds on each side of the face's box, in the surface's
/// STEP units per direction (degrees for an angle, lengths or the profile's own
/// parameter otherwise). Ops chooses it; the default is none.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WindowMargin {
    /// Added below the box's u minimum and above its maximum.
    pub u: f64,
    /// Added below the box's v minimum and above its maximum.
    pub v: f64,
}

/// The STEP (u, v) window of a face on `surface`: the box its boundary spans, plus
/// `margin`, as [`Surface::adjust_step_uv_domain`] takes it.
///
/// `loops` are the face's edge loops ([`BoundaryEdge`]), the outer loop first;
/// `vertices` the points of its vertex loops (an apex or a pole); `tol` the distance
/// within which the boundary lies on the surface.
///
/// **Exact.** Between its stationary points an edge's u and v are monotone, so an
/// edge's extremes are at its ends or where `du/dt` or `dv/dt` vanishes. Those are
/// found in closed form for lines and conics (`ELLIPSE`, `CIRCLE`) on a plane (the
/// extremes of `(P - O) . X` and `(P - O) . Y`), on a cone or cylinder (the height)
/// and on a sphere (the latitude), and for the angle about the axis of every surface
/// of revolution (`((P - O) x P') . Z = 0`, a sine equation on a conic); a circle
/// about the axis is an iso line, a line keeps its angle, and a curve in a plane
/// through the axis keeps its angle and its profile parameter is monotone. The rest
/// (other curve kinds; a torus's tube angle and a revolution's profile parameter off
/// those cases; B-spline and other surfaces) are the sign changes of the parameter's
/// rate along the edge, bracketed on the edge's knot spans (`2 (degree + 1)`
/// brackets each) and solved by [`LocalSolve1d::solve_by_bisection`].
///
/// **Units and parameterisations** are the kernel's STEP ones
/// ([`Surface::evaluate_step_point`]): a plane's `O + u sx X + v sy Y`, a cone's or
/// cylinder's angle in degrees and height along the axis, a sphere's longitude and
/// latitude in degrees, a torus's angle about the axis and round the tube in degrees,
/// a surface of revolution's angle in degrees and its profile's STEP parameter, and
/// the natural parameters of the other kinds.
///
/// **Periodic directions** (the angle of every surface of revolution, a torus's tube
/// angle and the profile angle of a revolved conic) are unwound along each loop, and
/// each later loop is moved by whole periods to where the box over it and the loops
/// before it is smallest (into the period of the first). A direction that a loop goes
/// round, or that the boundary spans by a full turn, is exactly one period. A face
/// whose loops go round the angle once more one way than the other contains what lies
/// on their left: its box reaches it (a sphere's pole, a cone's apex, the end of a
/// revolution's profile on the axis; a torus's whole tube). At a point on the axis
/// the angle is undefined: the loop passes round it on the face's side, from the angle
/// it arrives along to the one it leaves along. A face without edge loops on a sphere
/// or a torus is the whole surface.
///
/// **The result's angles** are moved by whole periods to start in `[0, 360)`, and
/// then one period down if they end above 360, within the kernel's angular limits
/// `-360 <= min <= max <= 360` (as [`crate::curve::step_edge::edge_interval`] keeps an
/// arc): a band from 300 degrees over the seam to 60 is `-60..60`, a full turn from 90
/// degrees `-270..90`.
///
/// **The margin** is added on each side of every direction that is not a full turn,
/// never beyond a sphere's poles, a cone's apex or, for the other kinds, the
/// surface's maximum analytic domain ([`Surface::max_analytic_domain`]) in its
/// directions that are not periodic; an angle widened to a full turn or more is one
/// period.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` or a margin that is negative or not
/// finite, an empty loop, an edge interval that is not finite or ends below its start,
/// an edge or vertex farther than `tol` from a plane, cone, cylinder, sphere or torus,
/// no edge loop on a surface that is not closed, a face whose loops wind round the
/// angle more than once more one way than the other, or that reaches a side its
/// surface does not close (a cylinder bounded by one circle).
/// [`SolidError::Unsupported`] for a loop through a point on the axis along the axis.
/// Those of [`Surface::step_inversion`], [`Surface::evaluate_step`],
/// [`Surface::max_analytic_domain`] and the curves' evaluations.
pub fn face_window(
    k: &Kernel,
    surface: &Surface,
    loops: &[Vec<BoundaryEdge<'_>>],
    vertices: &[Point3],
    margin: WindowMargin,
    tol: f64,
) -> Result<Extent2d> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "tolerance negative or not finite",
        ));
    }
    if !(margin.u.is_finite() && margin.u >= 0.0 && margin.v.is_finite() && margin.v >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "margin negative or not finite",
        ));
    }
    let map = Map::of(surface, tol);
    if loops.is_empty() {
        return map.whole();
    }

    let mut boxes: Vec<LoopBox> = Vec::new();
    for edges in loops {
        boxes.push(loop_box(k, &map, edges)?);
    }

    let mut window = [Range::empty(), Range::empty()];
    for (d, range) in window.iter_mut().enumerate() {
        for b in &boxes {
            let Some(r) = b.ranges.get(d).copied().flatten() else {
                continue;
            };
            *range = if map.periodic(d) && range.is_set() {
                range.union(&r.shifted_beside(range))
            } else {
                range.union(&r)
            };
        }
    }
    for &p in vertices {
        let q = map.params(k, p, true)?;
        for (d, range) in window.iter_mut().enumerate() {
            if let (false, Some(x)) = (map.periodic(d), q.get(d)) {
                range.add(x);
            }
        }
    }

    // a full turn round a periodic direction is one period
    let mut full = [false, false];
    for (d, f) in full.iter_mut().enumerate() {
        let wound = boxes.iter().any(|b| b.winding(d) != 0);
        let span = window.get(d).map_or(0.0, Range::length);
        *f = map.periodic(d) && (wound || span >= PERIOD);
    }

    // loops round the axis more one way than the other: the face reaches what closes
    // the surface on their left
    for d in 0..2 {
        let net: i64 = boxes.iter().map(|b| b.winding(d)).sum();
        if net == 0 {
            continue;
        }
        if net.abs() > 1 {
            return Err(SolidError::invalid_input(
                OP,
                "loops winding round the surface more than once",
            ));
        }
        // the left of a loop running up u is above it; of one running up v, below u
        let other = 1 - d;
        if map.periodic(other) {
            if let Some(f) = full.get_mut(other) {
                *f = true;
            }
            continue;
        }
        let up = d == 0 && net > 0;
        let bound = map.closing_bound(k, other, up)?;
        if let Some(r) = window.get_mut(other) {
            r.add(bound);
        }
    }

    let max = map.max_domain(k)?;
    let mut out = [Range::empty(), Range::empty()];
    for (d, o) in out.iter_mut().enumerate() {
        let r = window.get(d).copied().unwrap_or_default();
        if !r.is_set() {
            return Err(SolidError::invalid_input(OP, "a boundary without points"));
        }
        let m = if d == 0 { margin.u } else { margin.v };
        let (mut lo, mut hi) = (r.lo, r.hi);
        if full.get(d).copied().unwrap_or(false) {
            hi = lo + PERIOD;
        } else if map.periodic(d) {
            lo -= m;
            hi += m;
            if hi - lo >= PERIOD {
                hi = lo + PERIOD;
            }
        } else {
            let (min, max) = map.limits(d, max.as_ref());
            lo = lo.min((lo - m).max(min));
            hi = hi.max((hi + m).min(max));
        }
        if map.periodic(d) {
            let turns = (lo / PERIOD).floor();
            lo -= turns * PERIOD;
            hi -= turns * PERIOD;
            if hi > PERIOD {
                lo -= PERIOD;
                hi -= PERIOD;
            }
        }
        *o = Range { lo, hi };
    }
    let [u, v] = out;
    Ok(Extent2d::from_bounds(u.lo, v.lo, u.hi, v.hi))
}

/// A closed range, empty while `lo > hi`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Range {
    lo: f64,
    hi: f64,
}

impl Default for Range {
    fn default() -> Self {
        Self::empty()
    }
}

impl Range {
    const fn empty() -> Self {
        Self {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        }
    }

    fn is_set(&self) -> bool {
        self.lo <= self.hi
    }

    fn add(&mut self, x: f64) {
        self.lo = self.lo.min(x);
        self.hi = self.hi.max(x);
    }

    fn union(&self, other: &Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
        }
    }

    fn length(&self) -> f64 {
        if self.is_set() {
            self.hi - self.lo
        } else {
            0.0
        }
    }

    fn mid(&self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    fn shift(&self, by: f64) -> Self {
        Self {
            lo: self.lo + by,
            hi: self.hi + by,
        }
    }

    /// This range moved by the whole periods that make the box over it and `other`
    /// smallest: of the shift that brings its middle nearest `other`'s and the periods
    /// either side, the first smallest.
    fn shifted_beside(&self, other: &Self) -> Self {
        let n = ((other.mid() - self.mid()) / PERIOD).round();
        let mut best = self.shift(n * PERIOD);
        let mut best_len = best.union(other).length();
        for m in [n - 1.0, n + 1.0] {
            let cand = self.shift(m * PERIOD);
            let len = cand.union(other).length();
            if len < best_len {
                best = cand;
                best_len = len;
            }
        }
        best
    }
}

/// A loop's ranges (unwound in the periodic directions; `None` where no point of the
/// loop has the parameter) and how many times it goes round each periodic direction.
struct LoopBox {
    ranges: [Option<Range>; 2],
    windings: [i64; 2],
}

impl LoopBox {
    fn winding(&self, d: usize) -> i64 {
        self.windings.get(d).copied().unwrap_or(0)
    }
}

/// The surface's frame: origin, unit X, Y and Z (the axis of a surface of revolution).
#[derive(Debug, Clone, Copy)]
struct Frame {
    o: Point3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl Frame {
    fn of(p: &Axis2Placement) -> Self {
        Self {
            o: p.origin(),
            x: p.x_axis(),
            y: p.y_axis(),
            z: p.z_axis(),
        }
    }

    /// The angle of `w` about Z from X, in degrees.
    fn angle(&self, w: Vec3) -> f64 {
        w.dot(self.y).atan2(w.dot(self.x)).to_degrees()
    }

    /// `w` without its component along Z.
    fn radial(&self, w: Vec3) -> Vec3 {
        w - w.dot(self.z) * self.z
    }
}

/// A surface of revolution's v.
#[derive(Debug, Clone, Copy)]
enum Along {
    /// A cone's or cylinder's height along the axis; the radius is `radius + v tan`.
    Height { radius: f64, tan: f64 },
    /// A sphere's latitude.
    Latitude { radius: f64 },
    /// A torus's angle round its tube.
    Tube { major: f64, minor: f64 },
    /// A surface of revolution's profile parameter, by the kernel's inversion;
    /// periodic for a conic profile.
    Profile { periodic: bool },
}

/// How a point's STEP parameters on the surface are computed.
enum Map<'a> {
    /// `O + u sx X + v sy Y`.
    Plane {
        frame: Frame,
        scale: Point2,
        tol: f64,
    },
    /// u the angle about Z (undefined on the axis), v by `along`.
    Revolved {
        frame: Frame,
        along: Along,
        surface: &'a Surface,
        tol: f64,
    },
    /// The kernel's inversion in both directions, neither periodic.
    Inverted { surface: &'a Surface, tol: f64 },
}

impl<'a> Map<'a> {
    fn of(surface: &'a Surface, tol: f64) -> Self {
        let revolved = |frame, along| Map::Revolved {
            frame,
            along,
            surface,
            tol,
        };
        match surface.kind() {
            SurfaceKind::Plane(s) => Map::Plane {
                frame: Frame::of(s.position()),
                scale: Point2::new(s.uv_scale().x, s.uv_scale().y),
                tol,
            },
            SurfaceKind::Cone(s) => {
                let c = s.canonical();
                revolved(
                    Frame::of(&c.origin),
                    Along::Height {
                        radius: c.radius,
                        tan: c.semi_angle_deg.to_radians().tan(),
                    },
                )
            }
            SurfaceKind::Sphere(s) => revolved(
                Frame::of(s.base().position()),
                Along::Latitude { radius: s.radius() },
            ),
            SurfaceKind::Torus(s) => revolved(
                Frame::of(s.base().position()),
                Along::Tube {
                    major: s.major_radius(),
                    minor: s.minor_radius(),
                },
            ),
            SurfaceKind::SurfOfRevolution(s) => revolved(
                Frame::of(s.position()),
                Along::Profile {
                    periodic: matches!(s.gen_curve().map(Curve::kind), Some(CurveKind::Ellipse(_))),
                },
            ),
            _ => Map::Inverted { surface, tol },
        }
    }

    const fn surface(&self) -> Option<&'a Surface> {
        match self {
            Map::Plane { .. } => None,
            Map::Revolved { surface, .. } | Map::Inverted { surface, .. } => Some(surface),
        }
    }

    const fn periodic(&self, d: usize) -> bool {
        match self {
            Map::Revolved { along, .. } => {
                d == 0
                    || matches!(
                        along,
                        Along::Tube { .. } | Along::Profile { periodic: true }
                    )
            }
            Map::Plane { .. } | Map::Inverted { .. } => false,
        }
    }

    /// The window of a face without edge loops: the whole of a closed surface.
    fn whole(&self) -> Result<Extent2d> {
        match self {
            Map::Revolved {
                along: Along::Latitude { .. },
                ..
            } => Ok(Extent2d::from_bounds(0.0, -90.0, PERIOD, 90.0)),
            Map::Revolved {
                along: Along::Tube { .. },
                ..
            } => Ok(Extent2d::from_bounds(0.0, 0.0, PERIOD, PERIOD)),
            _ => Err(SolidError::invalid_input(
                OP,
                "no edge loop on a surface that is not closed",
            )),
        }
    }

    fn max_domain(&self, k: &Kernel) -> Result<Option<Extent2d>> {
        match self.surface() {
            Some(s) => s.max_analytic_domain(k).map(Some),
            None => Ok(None),
        }
    }

    /// How far a margin may move the ends of direction `d` (not periodic).
    fn limits(&self, d: usize, max: Option<&Extent2d>) -> (f64, f64) {
        match self {
            Map::Plane { .. } => (f64::NEG_INFINITY, f64::INFINITY),
            Map::Revolved {
                along: Along::Latitude { .. },
                ..
            } => (-90.0, 90.0),
            Map::Revolved {
                along: Along::Height { radius, tan },
                ..
            } => {
                if *tan == 0.0 {
                    (f64::NEG_INFINITY, f64::INFINITY)
                } else if *tan > 0.0 {
                    // the side of the apex where the radius is positive
                    (-radius / tan, f64::INFINITY)
                } else {
                    (f64::NEG_INFINITY, -radius / tan)
                }
            }
            _ => match max {
                Some(m) => {
                    let ivl = if d == 0 {
                        m.u_interval()
                    } else {
                        m.v_interval()
                    };
                    (ivl.min(), ivl.max())
                }
                None => (f64::NEG_INFINITY, f64::INFINITY),
            },
        }
    }

    /// The end of direction `d` (not periodic) that closes the surface on the side
    /// `up` (towards larger values): a sphere's pole, a cone's apex, the end of a
    /// revolution's profile on its axis.
    fn closing_bound(&self, k: &Kernel, d: usize, up: bool) -> Result<f64> {
        let open =
            || SolidError::invalid_input(OP, "a face reaching a side its surface does not close");
        if d != 1 {
            return Err(open());
        }
        match self {
            Map::Revolved {
                along: Along::Latitude { .. },
                ..
            } => Ok(if up { 90.0 } else { -90.0 }),
            Map::Revolved {
                along: Along::Height { radius, tan },
                ..
            } => {
                // the apex is above where the radius shrinks upwards
                if *tan == 0.0 || (*tan < 0.0) != up {
                    return Err(open());
                }
                Ok(-radius / tan)
            }
            Map::Revolved {
                frame,
                along: Along::Profile { .. },
                surface,
                tol,
            } => {
                let m = surface.max_analytic_domain(k)?;
                let v = if up { m.v_max() } else { m.v_min() };
                let p = surface.evaluate_step_point(k, Point2::new(m.u_min(), v))?;
                if frame.radial(p - frame.o).length() <= *tol {
                    Ok(v)
                } else {
                    Err(open())
                }
            }
            _ => Err(open()),
        }
    }

    /// The STEP parameters of `p` (u `None` on the axis of a surface of revolution);
    /// with `check`, refused farther than the tolerance from an analytic surface.
    fn params(&self, k: &Kernel, p: Point3, check: bool) -> Result<Params> {
        let off = || SolidError::invalid_input(OP, "a boundary point off its surface");
        match self {
            Map::Plane { frame, scale, tol } => {
                let w = p - frame.o;
                if check && w.dot(frame.z).abs() > *tol {
                    return Err(off());
                }
                Ok(Params {
                    u: Some(w.dot(frame.x) / scale.x),
                    v: w.dot(frame.y) / scale.y,
                })
            }
            Map::Revolved {
                frame,
                along,
                surface,
                tol,
            } => {
                let w = p - frame.o;
                let h = w.dot(frame.z);
                let rho = frame.radial(w).length();
                let u = (rho > *tol).then(|| frame.angle(w));
                let (v, gap) = match *along {
                    // the distance across the generator
                    Along::Height { radius, tan } => {
                        (h, (rho - (radius + h * tan)).abs() / tan.hypot(1.0))
                    }
                    Along::Latitude { radius } => {
                        (h.atan2(rho).to_degrees(), (w.length() - radius).abs())
                    }
                    Along::Tube { major, minor } => (
                        h.atan2(rho - major).to_degrees(),
                        ((rho - major).hypot(h) - minor).abs(),
                    ),
                    Along::Profile { .. } => {
                        let dom = surface.max_analytic_domain(k)?;
                        let (uv, _) = surface.step_inversion(k, &dom, p, *tol, None)?;
                        (uv.y, 0.0)
                    }
                };
                if check && gap > *tol {
                    return Err(off());
                }
                Ok(Params { u, v })
            }
            Map::Inverted { surface, tol } => {
                let dom = surface.max_analytic_domain(k)?;
                let (uv, _) = surface.step_inversion(k, &dom, p, *tol, None)?;
                Ok(Params {
                    u: Some(uv.x),
                    v: uv.y,
                })
            }
        }
    }

    /// The rates of u and v along the tangent `dp` at `p` (whose parameters are `q`),
    /// each up to a positive factor.
    fn rates(&self, k: &Kernel, p: Point3, dp: Vec3, q: &Params) -> Result<[f64; 2]> {
        match self {
            Map::Plane { frame, scale, .. } => {
                Ok([dp.dot(frame.x) / scale.x, dp.dot(frame.y) / scale.y])
            }
            Map::Revolved { frame, along, .. } => {
                let w = p - frame.o;
                let rad = frame.radial(w);
                let rho2 = rad.length_squared();
                let du = w.cross(dp).dot(frame.z) / rho2;
                let dv = match *along {
                    Along::Height { .. } | Along::Latitude { .. } => dp.dot(frame.z),
                    Along::Tube { major, .. } => {
                        let rho = rho2.sqrt();
                        let drho = rad.dot(dp) / rho;
                        (rho - major) * dp.dot(frame.z) - w.dot(frame.z) * drho
                    }
                    Along::Profile { .. } => self.chain(k, dp, q)?[1],
                };
                Ok([du, dv])
            }
            Map::Inverted { .. } => self.chain(k, dp, q),
        }
    }

    /// The rates of the kernel's STEP parameters along `dp`: the least-squares
    /// `(Su, Sv) . r = dp` with the STEP derivatives at `q`.
    fn chain(&self, k: &Kernel, dp: Vec3, q: &Params) -> Result<[f64; 2]> {
        let none = || SolidError::invalid_input(OP, "no surface derivative");
        let surface = self.surface().ok_or_else(none)?;
        let uv = Point2::new(q.u.unwrap_or(0.0), q.v);
        let d = surface.evaluate_step(k, uv, 1, 1, EvaluateFlags::default())?;
        let su = d.get(1).and_then(|r| r.first()).copied().ok_or_else(none)?;
        let sv = d.first().and_then(|r| r.get(1)).copied().ok_or_else(none)?;
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let (r1, r2) = (su.dot(dp), sv.dot(dp));
        let det = a * c - b * b;
        if det == 0.0 {
            return Ok([0.0, 0.0]);
        }
        Ok([(c * r1 - b * r2) / det, (a * r2 - b * r1) / det])
    }
}

/// A point's STEP parameters.
#[derive(Debug, Clone, Copy)]
struct Params {
    u: Option<f64>,
    v: f64,
}

impl Params {
    fn get(&self, d: usize) -> Option<f64> {
        if d == 0 {
            self.u
        } else {
            Some(self.v)
        }
    }
}

/// An edge's curve in the parameter its stationary points are found in (the STEP
/// parameter of a line or a conic, else the natural one), and where the loop enters
/// and leaves it.
struct EdgeCurve<'a> {
    curve: &'a Curve,
    from: f64,
    to: f64,
}

impl<'a> EdgeCurve<'a> {
    fn new(k: &Kernel, e: &BoundaryEdge<'a>) -> Result<Self> {
        let (lo, hi) = (e.interval.min(), e.interval.max());
        if !(lo.is_finite() && hi.is_finite() && lo <= hi) {
            return Err(SolidError::invalid_input(
                OP,
                "an edge interval not finite or ending below its start",
            ));
        }
        let (a, b) = if e.forward { (lo, hi) } else { (hi, lo) };
        let (from, to) = if matches!(e.curve.kind(), CurveKind::Line(_) | CurveKind::Ellipse(_)) {
            (a, b)
        } else {
            (
                e.curve.convert_t_from_step_to_nurbs(k, a)?,
                e.curve.convert_t_from_step_to_nurbs(k, b)?,
            )
        };
        Ok(Self {
            curve: e.curve,
            from,
            to,
        })
    }

    /// The point and the tangent along the parameter at `t` (from the left when
    /// `left`).
    fn at(&self, k: &Kernel, t: f64, left: bool) -> Result<(Point3, Vec3)> {
        let d = match self.curve.kind() {
            CurveKind::Line(l) => l.evaluate_step(t, 1),
            CurveKind::Ellipse(e) => e.evaluate_step(t, 1)?,
            _ => self.curve.evaluate(k, t, 1, left, false)?,
        };
        match (d.first(), d.get(1)) {
            (Some(&p), Some(&dp)) => Ok((p, dp)),
            _ => Err(SolidError::invalid_input(OP, "no curve derivative")),
        }
    }

    /// The tangent at `t` pointing into the piece towards `other`: where the piece's
    /// points lie seen from `t`.
    fn toward(&self, k: &Kernel, t: f64, other: f64) -> Result<Vec3> {
        let left = other < t;
        let (_, dp) = self.at(k, t, left)?;
        Ok(if left { -dp } else { dp })
    }

    /// The parameters strictly between `from` and `to` where u or v is stationary (or
    /// the curve meets the axis), in loop order.
    fn stationary(&self, k: &Kernel, map: &Map<'_>) -> Result<Vec<f64>> {
        let (lo, hi) = (self.from.min(self.to), self.from.max(self.to));
        let mut ts = Vec::new();
        for d in 0..2 {
            match closed_form(self.curve, map, d) {
                Some(Roots::Monotone) => {}
                Some(Roots::Every { base, step }) => push_every(&mut ts, base, step, lo, hi),
                Some(Roots::Turns(roots)) => {
                    for r in roots.into_iter().flatten() {
                        push_every(&mut ts, r, PERIOD, lo, hi);
                    }
                }
                Some(Roots::Once(t)) => {
                    if t > lo && t < hi {
                        ts.push(t);
                    }
                }
                None => ts.extend(self.numeric(k, map, d, lo, hi)?),
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup();
        if self.to < self.from {
            ts.reverse();
        }
        Ok(ts)
    }

    /// The sign changes of direction `d`'s rate strictly between `lo` and `hi`,
    /// bracketed on the knot spans and solved.
    fn numeric(&self, k: &Kernel, map: &Map<'_>, d: usize, lo: f64, hi: f64) -> Result<Vec<f64>> {
        if lo >= hi {
            return Ok(Vec::new());
        }
        // the spans in the edge's parameter: a line's one, a conic's quarter turns (its
        // NURB's arcs), else the curve's knot spans
        let mut breaks = vec![lo];
        let degree = match self.curve.kind() {
            CurveKind::Line(_) => 1,
            CurveKind::Ellipse(_) => {
                push_every(&mut breaks, 0.0, PERIOD / 4.0, lo, hi);
                2
            }
            _ => {
                let (knots, _) = self.curve.knots(k, Some(&Extent1d::from_min_max(lo, hi)))?;
                breaks.extend(knots.into_iter().filter(|t| *t > lo && *t < hi));
                self.curve.degree().unwrap_or(3)
            }
        };
        breaks.push(hi);
        let pieces = 2 * (degree.clamp(1, 32) + 1);
        let mut samples = Vec::new();
        for w in breaks.windows(2) {
            if let [a, b] = w {
                samples.extend((0..pieces).map(|i| a + (b - a) * (i as f64) / (pieces as f64)));
            }
        }
        samples.push(hi);

        let mut rate = Rate {
            k,
            map,
            edge: self,
            d,
            sign: 1.0,
        };
        let mut roots = Vec::new();
        let mut prev: Option<(f64, f64)> = None;
        for t in samples {
            let f = rate.value(t)?;
            if let Some((pt, pf)) = prev {
                roots.extend(rate.root((pt, pf), (t, f), lo, hi)?);
            }
            prev = Some((t, f));
        }
        Ok(roots)
    }
}

/// Direction `d`'s rate along an edge, for [`LocalSolve1d`] (times `sign`).
struct Rate<'a, 'b> {
    k: &'a Kernel,
    map: &'a Map<'b>,
    edge: &'a EdgeCurve<'a>,
    d: usize,
    sign: f64,
}

impl Rate<'_, '_> {
    fn value(&self, t: f64) -> Result<f64> {
        let (p, dp) = self.edge.at(self.k, t, false)?;
        let q = self.map.params(self.k, p, false)?;
        let f = self
            .map
            .rates(self.k, p, dp, &q)?
            .get(self.d)
            .copied()
            .unwrap_or(0.0);
        // on the axis the angle's rate is undefined: a stationary point
        Ok(if f.is_finite() { f } else { 0.0 })
    }

    /// The root between the samples `a` and `b` (`b` itself where its rate is zero), if
    /// it is strictly inside `lo..hi`.
    fn root(
        &mut self,
        (a, fa): (f64, f64),
        (b, fb): (f64, f64),
        lo: f64,
        hi: f64,
    ) -> Result<Option<f64>> {
        let inside = |t: f64| (t > lo && t < hi).then_some(t);
        if fb == 0.0 {
            return Ok(inside(b));
        }
        if fa == 0.0 || (fa < 0.0) == (fb < 0.0) {
            return Ok(None);
        }
        // regula falsi from the negative end
        self.sign = if fa < 0.0 { 1.0 } else { -1.0 };
        let (fl, fu) = (self.sign * fa, self.sign * fb);
        let found = LocalSolve1d::new_with_kernel(self.k)
            .solve_by_bisection(self, ROOT_STEPS, a, b, fl, fu);
        self.sign = 1.0;
        Ok(inside(found?.0))
    }
}

impl EvalFunction for Rate<'_, '_> {
    fn evaluate(&mut self, t: f64, out: &mut FunctionValue, _signal_errors: bool) -> Result<()> {
        out.f = self.sign * self.value(t)?;
        out.f_prime = 0.0;
        out.found_answer = false;
        Ok(())
    }
}

/// A direction's stationary parameters along a curve, in closed form.
enum Roots {
    /// None: the parameter is monotone (or constant) along the curve.
    Monotone,
    /// `base + i * step` for every whole `i`.
    Every { base: f64, step: f64 },
    /// Up to two roots, each repeating every turn.
    Turns([Option<f64>; 2]),
    /// One root (where a line meets the axis).
    Once(f64),
}

/// Pushes `base + i * step` strictly inside `lo..hi`.
fn push_every(ts: &mut Vec<f64>, base: f64, step: f64, lo: f64, hi: f64) {
    if !(step > 0.0 && base.is_finite()) {
        return;
    }
    let first = ((lo - base) / step).floor();
    // a conic's interval spans at most two turns: a handful of candidates
    let count = ((hi - lo) / step).ceil().clamp(0.0, 8.0) as usize + 1;
    for i in 0..=count {
        let t = base + (first + i as f64) * step;
        if t > lo && t < hi {
            ts.push(t);
        }
    }
}

/// The stationary parameters of direction `d` of `map` along `curve` in closed form,
/// or `None` where there is none.
fn closed_form(curve: &Curve, map: &Map<'_>, d: usize) -> Option<Roots> {
    match curve.kind() {
        CurveKind::Line(l) => {
            let c = l.canonical();
            let (p, v) = (c.point, c.vector);
            match map {
                // linear in the parameter
                Map::Plane { .. } => Some(Roots::Monotone),
                Map::Revolved { frame, along, .. } => {
                    let w = p - frame.o;
                    // ((P - O) x V) . Z is constant along the line: zero when it lies in
                    // a plane through the axis
                    let through = w.cross(v).dot(frame.z).abs()
                        <= EFF_ZERO * w.length().max(1.0) * v.length();
                    if d == 0 {
                        if !through {
                            return Some(Roots::Monotone);
                        }
                        // in a plane through the axis its angle is constant, but where it
                        // meets the axis
                        let (rw, rv) = (frame.radial(w), frame.radial(v));
                        let rv2 = rv.length_squared();
                        if rv2 == 0.0 {
                            return Some(Roots::Monotone);
                        }
                        return Some(Roots::Once(-rw.dot(rv) / rv2));
                    }
                    match along {
                        // linear in the parameter
                        Along::Height { .. } => Some(Roots::Monotone),
                        // a line in a plane through the axis is the profile's
                        Along::Profile { .. } if through => Some(Roots::Monotone),
                        _ => None,
                    }
                }
                Map::Inverted { .. } => None,
            }
        }
        CurveKind::Ellipse(e) => {
            let c = e.canonical();
            let (cx, xa, ya) = (
                c.position.origin(),
                c.position.x_axis(),
                c.position.y_axis(),
            );
            let (a, b) = (c.radius_x, c.radius_y);
            let size = a.max(b);
            // the extremes of (P - O) . w: -a sin t (X . w) + b cos t (Y . w) = 0
            let linear = |w: Vec3| -> Roots {
                let (s, t) = (a * xa.dot(w), b * ya.dot(w));
                if s.hypot(t) <= EFF_ZERO * size {
                    Roots::Monotone
                } else {
                    Roots::Every {
                        base: t.atan2(s).to_degrees(),
                        step: PERIOD / 2.0,
                    }
                }
            };
            match map {
                Map::Plane { frame, .. } => Some(linear(if d == 0 { frame.x } else { frame.y })),
                Map::Revolved { frame, along, .. } => {
                    let w = cx - frame.o;
                    let n = xa.cross(ya);
                    // ((P - O) x P') . Z = al cos t + be sin t + ga (per radian)
                    let al = b * w.cross(ya).dot(frame.z);
                    let be = -a * w.cross(xa).dot(frame.z);
                    let ga = a * b * n.dot(frame.z);
                    let scale = size * (w.length() + size);
                    let through = al.hypot(be) <= EFF_ZERO * scale && ga.abs() <= EFF_ZERO * scale;
                    let about = frame.radial(w).length() <= EFF_ZERO * (w.length() + size)
                        && frame.radial(n).length() <= EFF_ZERO;
                    if d == 0 {
                        if through {
                            // in a plane through the axis its angle is constant, but where
                            // it meets the axis: (P - O) . m = 0 across the plane
                            let m = frame.z.cross(n).unitize().ok()?;
                            return Some(sine_roots(a * xa.dot(m), b * ya.dot(m), w.dot(m)));
                        }
                        return Some(sine_roots(al, be, ga));
                    }
                    match along {
                        Along::Height { .. } | Along::Latitude { .. } => Some(linear(frame.z)),
                        // an iso circle, or the profile (the tube) itself
                        Along::Tube { .. } | Along::Profile { .. } if about || through => {
                            Some(Roots::Monotone)
                        }
                        _ => None,
                    }
                }
                Map::Inverted { .. } => None,
            }
        }
        _ => None,
    }
}

/// The roots in degrees of `p cos t + q sin t + r = 0` (none where it holds for every
/// or no `t`).
fn sine_roots(p: f64, q: f64, r: f64) -> Roots {
    let amp = p.hypot(q);
    if amp == 0.0 {
        return Roots::Monotone;
    }
    let c = -r / amp;
    if c.abs() > 1.0 {
        return Roots::Monotone;
    }
    let phi = q.atan2(p).to_degrees();
    let delta = c.acos().to_degrees();
    Roots::Turns([Some(phi + delta), (delta != 0.0).then_some(phi - delta)])
}

/// A point of a loop: the edge it is on (an index into the loop), its parameter
/// there, and its STEP parameters.
struct Node {
    edge: usize,
    t: f64,
    params: Params,
}

/// The ranges and windings of one loop.
fn loop_box(k: &Kernel, map: &Map<'_>, edges: &[BoundaryEdge<'_>]) -> Result<LoopBox> {
    if edges.is_empty() {
        return Err(SolidError::invalid_input(OP, "an empty loop"));
    }
    let mut curves = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        let c = EdgeCurve::new(k, e)?;
        let mut ts = vec![c.from];
        ts.extend(c.stationary(k, map)?);
        ts.push(c.to);
        for t in ts {
            let (p, _) = c.at(k, t, false)?;
            nodes.push(Node {
                edge: i,
                t,
                params: map.params(k, p, true)?,
            });
        }
        curves.push(c);
    }

    let mut ranges = [None, None];
    let mut windings = [0, 0];
    for (d, (range, winding)) in ranges.iter_mut().zip(windings.iter_mut()).enumerate() {
        let (r, turns) = if map.periodic(d) {
            unwind(k, map, &curves, &nodes, d)?
        } else {
            let mut r = Range::empty();
            for x in nodes.iter().filter_map(|n| n.params.get(d)) {
                r.add(x);
            }
            (r, 0)
        };
        *range = r.is_set().then_some(r);
        *winding = turns;
    }
    Ok(LoopBox { ranges, windings })
}

/// The values of periodic direction `d` along a loop, unwound, and how many periods
/// the loop goes round.
fn unwind(
    k: &Kernel,
    map: &Map<'_>,
    curves: &[EdgeCurve<'_>],
    nodes: &[Node],
    d: usize,
) -> Result<(Range, i64)> {
    let n = nodes.len();
    let Some(start) = nodes.iter().position(|x| x.params.get(d).is_some()) else {
        return Ok((Range::empty(), 0));
    };
    let mut c = nodes
        .get(start)
        .and_then(|x| x.params.get(d))
        .unwrap_or(0.0);
    let origin = c;
    let mut range = Range::empty();
    range.add(c);
    // the value the loop arrived at an axis point with, and whether the point is at
    // the top of v
    let mut pole: Option<(f64, bool)> = None;
    for step in 0..n {
        let (ia, ib) = ((start + step) % n, (start + step + 1) % n);
        let (Some(a), Some(b)) = (nodes.get(ia), nodes.get(ib)) else {
            continue;
        };
        // a piece along one edge (the next node on it), or the join of two edges (also
        // the end of a loop's only edge back to its start)
        let edge = curves
            .get(a.edge)
            .filter(|_| ib == ia + 1 && a.edge == b.edge && a.t != b.t);
        match (a.params.get(d), b.params.get(d), edge) {
            (Some(x), Some(y), Some(e)) => {
                c += turn(k, map, e, (a.t, x), (b.t, y), d, 0)?;
                range.add(c);
            }
            (Some(x), Some(y), None) => {
                c += nearest(y - x);
                range.add(c);
            }
            (Some(x), None, e) => {
                // onto the axis, along the angle it arrives from
                if let Some(e) = e {
                    let arrival = limit(map, e.toward(k, b.t, a.t)?)?;
                    c += turn(k, map, e, (a.t, x), (b.t, arrival), d, 0)?;
                    range.add(c);
                }
                pole = Some((c, b.params.v > a.params.v));
            }
            (None, _, None) => {}
            (None, y, Some(e)) => {
                let departure = limit(map, e.toward(k, a.t, b.t)?)?;
                if let Some((arrived, top)) = pole.take() {
                    // round the axis point on the face's side, the loop's left: down the
                    // angle at the top of v, up it at the bottom
                    c = arrived
                        + if top {
                            -(arrived - departure).rem_euclid(PERIOD)
                        } else {
                            (departure - arrived).rem_euclid(PERIOD)
                        };
                    range.add(c);
                }
                if let Some(y) = y {
                    c += turn(k, map, e, (a.t, departure), (b.t, y), d, 0)?;
                    range.add(c);
                } else {
                    // from the axis back to it
                    let arrival = limit(map, e.toward(k, b.t, a.t)?)?;
                    c += turn(k, map, e, (a.t, departure), (b.t, arrival), d, 0)?;
                    range.add(c);
                    pole = Some((c, b.params.v > a.params.v));
                }
            }
        }
    }
    let turns = ((c - origin) / PERIOD).round() as i64;
    Ok((range, turns))
}

/// `x` moved by whole periods into `(-180, 180]`.
fn nearest(x: f64) -> f64 {
    let r = x.rem_euclid(PERIOD);
    if r > PERIOD / 2.0 {
        r - PERIOD
    } else {
        r
    }
}

/// The angle about the axis of the direction `w` (a loop's tangent at a point on the
/// axis, pointing into the piece).
fn limit(map: &Map<'_>, w: Vec3) -> Result<f64> {
    let Map::Revolved { frame, .. } = map else {
        return Err(SolidError::invalid_input(
            OP,
            "an axis point on a surface without axis",
        ));
    };
    let r = frame.radial(w);
    if r.length() <= EFF_ZERO * w.length() {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "a loop through a point on the axis along the axis",
        });
    }
    Ok(frame.angle(r))
}

/// How far periodic direction `d` turns along `e` from `(ta, xa)` to `(tb, xb)`, a piece
/// on which it is monotone: the difference of the values taken round the period the
/// way the rate at the middle runs, over the piece's quarters ([`MIN_SPLIT_DEPTH`]
/// halvings), each halved further until it turns by at most half a period (past
/// [`MAX_SPLIT_DEPTH`] halvings, the nearest way).
fn turn(
    k: &Kernel,
    map: &Map<'_>,
    e: &EdgeCurve<'_>,
    (ta, xa): (f64, f64),
    (tb, xb): (f64, f64),
    d: usize,
    depth: usize,
) -> Result<f64> {
    let tm = 0.5 * (ta + tb);
    let (p, dp) = e.at(k, tm, false)?;
    let q = map.params(k, p, false)?;
    let rate = map.rates(k, p, dp, &q)?.get(d).copied().unwrap_or(0.0) * (tb - ta).signum();
    if rate == 0.0 || !rate.is_finite() {
        return Ok(nearest(xb - xa));
    }
    let s = rate.signum();
    let along = (s * (xb - xa)).rem_euclid(PERIOD);
    // a piece of a whole turn has equal ends: it is never taken whole
    if along <= PERIOD / 2.0 && depth >= MIN_SPLIT_DEPTH {
        return Ok(s * along);
    }
    let (Some(xm), true) = (q.get(d), depth < MAX_SPLIT_DEPTH) else {
        return Ok(nearest(xb - xa));
    };
    Ok(turn(k, map, e, (ta, xa), (tm, xm), d, depth + 1)?
        + turn(k, map, e, (tm, xm), (tb, xb), d, depth + 1)?)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::curve::ellipse::Ellipse;
    use crate::curve::line::Line;
    use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
    use crate::surface::cone::Cone;
    use crate::surface::plane::Plane;
    use crate::surface::sphere::Sphere;
    use crate::surface::surf_of_revolution::SurfOfRevolution;
    use crate::surface::torus::{Torus, TorusOptions};

    const TOL: f64 = 1e-9;

    fn k() -> Kernel {
        Kernel::default()
    }

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn rad(deg: f64) -> f64 {
        deg.to_radians()
    }

    /// `a` equals the closed form `b` to rounding.
    #[track_caller]
    fn assert_close(a: f64, b: f64) {
        assert!(
            (a - b).abs() <= 1e-12 * (1.0 + b.abs()),
            "{a} is not {b} (difference {})",
            a - b
        );
    }

    #[track_caller]
    fn assert_window(w: &Extent2d, u: [f64; 2], v: [f64; 2]) {
        assert_close(w.u_min(), u[0]);
        assert_close(w.u_max(), u[1]);
        assert_close(w.v_min(), v[0]);
        assert_close(w.v_max(), v[1]);
    }

    /// The circle of radius `r` about `center`, its 0 degree point along `x` and its
    /// 90 degree point along `y`.
    fn circle(k: &Kernel, center: Point3, x: Vec3, y: Vec3, r: f64) -> Curve {
        let frame = Axis2Placement::new(center, x, y);
        Curve::from(Ellipse::create_canonical_circle(k, &frame, r, None, None).unwrap())
    }

    /// The circle of radius `r` about Z at height `z`.
    fn level(k: &Kernel, z: f64, r: f64) -> Curve {
        circle(k, p(0.0, 0.0, z), Vec3::X, Vec3::Y, r)
    }

    /// The point at `deg` degrees on the circle of radius `r` about Z at height `z`.
    fn round(r: f64, deg: f64, z: f64) -> Point3 {
        p(r * rad(deg).cos(), r * rad(deg).sin(), z)
    }

    /// The segment from `a` to `b`, its STEP parameter 0 at `a` and 1 at `b`.
    fn segment(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
    }

    fn edge(curve: &Curve, lo: f64, hi: f64, forward: bool) -> BoundaryEdge<'_> {
        BoundaryEdge {
            curve,
            interval: Extent1d::from_min_max(lo, hi),
            forward,
        }
    }

    /// A segment used from its start to its end.
    fn along(curve: &Curve) -> BoundaryEdge<'_> {
        edge(curve, 0.0, 1.0, true)
    }

    fn window(k: &Kernel, s: &Surface, loops: &[Vec<BoundaryEdge<'_>>]) -> Extent2d {
        face_window(k, s, loops, &[], WindowMargin::default(), TOL).unwrap()
    }

    fn cylinder(k: &Kernel) -> Surface {
        Surface::from(Cone::create_canonical_cylinder(k, &Axis2Placement::IDENTITY, 1.0).unwrap())
    }

    /// The edges of a band of the cylinder r = 1 between z = 0 and z = 3 and the angles
    /// `a` and `b`.
    struct Band {
        bottom: Curve,
        top: Curve,
        up: Curve,
        down: Curve,
        a: f64,
        b: f64,
    }

    impl Band {
        fn new(k: &Kernel, a: f64, b: f64) -> Self {
            Self {
                bottom: level(k, 0.0, 1.0),
                top: level(k, 3.0, 1.0),
                up: segment(round(1.0, b, 0.0), round(1.0, b, 3.0)),
                down: segment(round(1.0, a, 3.0), round(1.0, a, 0.0)),
                a,
                b,
            }
        }

        /// Counterclockwise in (u, v): along the bottom, up, back along the top, down.
        fn outer(&self) -> Vec<BoundaryEdge<'_>> {
            vec![
                edge(&self.bottom, self.a, self.b, true),
                along(&self.up),
                edge(&self.top, self.a, self.b, false),
                along(&self.down),
            ]
        }
    }

    /// Cylinder r = 1, the band z in [0, 3] over a quarter: u = [0, 90], v = [0, 3].
    #[test]
    fn a_cylinder_band_over_a_quarter() {
        let k = k();
        let band = Band::new(&k, 0.0, 90.0);
        let w = window(&k, &cylinder(&k), &[band.outer()]);
        assert_window(&w, [0.0, 90.0], [0.0, 3.0]);
    }

    /// The band from 300 degrees over the seam to 60, its arcs as #2609 gives them
    /// (-60..60) or split at the seam (300..360 and 0..60): either way u = [-60, 60],
    /// the kernel's convention for an angle over the seam.
    #[test]
    fn an_arc_across_the_seam() {
        let k = k();
        let s = cylinder(&k);
        let band = Band::new(&k, -60.0, 60.0);
        assert_window(&window(&k, &s, &[band.outer()]), [-60.0, 60.0], [0.0, 3.0]);

        let split = vec![
            edge(&band.bottom, 300.0, 360.0, true),
            edge(&band.bottom, 0.0, 60.0, true),
            along(&band.up),
            edge(&band.top, 0.0, 60.0, false),
            edge(&band.top, 300.0, 360.0, false),
            along(&band.down),
        ];
        assert_window(&window(&k, &s, &[split]), [-60.0, 60.0], [0.0, 3.0]);
    }

    /// Two full circles bound the whole band: u is exactly one period. From 0 degrees
    /// it is 0..360; from 90 degrees (#2609's -270..90 for a full turn from there) it
    /// starts at 90, one period down: -270..90.
    #[test]
    fn a_full_circle_is_one_period() {
        let k = k();
        let s = cylinder(&k);
        let (bottom, top) = (level(&k, 0.0, 1.0), level(&k, 3.0, 1.0));
        let full = |lo: f64, hi: f64| {
            vec![
                vec![edge(&bottom, lo, hi, true)],
                vec![edge(&top, lo, hi, false)],
            ]
        };
        assert_window(&window(&k, &s, &full(0.0, 360.0)), [0.0, 360.0], [0.0, 3.0]);
        assert_window(
            &window(&k, &s, &full(-270.0, 90.0)),
            [-270.0, 90.0],
            [0.0, 3.0],
        );
    }

    /// A rectangle on a plane whose frame is moved and turned: its box exactly.
    #[test]
    fn a_rectangle_on_a_plane() {
        let k = k();
        let (o, x, y) = (p(1.0, -2.0, 0.5), Vec3::Y, Vec3::Z);
        let s = Surface::from(Plane::create_canonical(&Axis2Placement::new(o, x, y)).unwrap());
        let at = |u: f64, v: f64| o + u * x + v * y;
        let sides = [
            segment(at(1.0, 2.0), at(4.0, 2.0)),
            segment(at(4.0, 2.0), at(4.0, 5.0)),
            segment(at(4.0, 5.0), at(1.0, 5.0)),
            segment(at(1.0, 5.0), at(1.0, 2.0)),
        ];
        let l: Vec<_> = sides.iter().map(along).collect();
        assert_window(&window(&k, &s, &[l]), [1.0, 4.0], [2.0, 5.0]);
    }

    /// An ellipse (a = 3, b = 1) turned by 30 degrees in a plane tilted by 45 degrees,
    /// centred at (2, 1) in the plane's frame: its box is the centre plus or minus
    /// sqrt(a^2 cos^2 30 + b^2 sin^2 30) = sqrt(7) in u and
    /// sqrt(a^2 sin^2 30 + b^2 cos^2 30) = sqrt(3) in v. On the arc -45..135 closed by
    /// its chord, the u and v maxima are inside the arc (at -10.9 and 30 degrees) and
    /// the u minimum is at an end.
    #[test]
    fn a_turned_ellipse_on_a_tilted_plane() {
        let k = k();
        let h = 0.5f64.sqrt();
        let (x, y) = (Vec3::new(h, 0.0, h), Vec3::Y);
        let s =
            Surface::from(Plane::create_canonical(&Axis2Placement::new(Vec3::ZERO, x, y)).unwrap());
        let (c30, s30) = (rad(30.0).cos(), rad(30.0).sin());
        let (ex, ey) = (c30 * x + s30 * y, -s30 * x + c30 * y);
        let centre = 2.0 * x + y;
        let frame = Axis2Placement::new(centre, ex, ey);
        let e = Curve::from(Ellipse::create_canonical(&k, &frame, 3.0, 1.0, None, None).unwrap());
        let (r7, r3) = (7f64.sqrt(), 3f64.sqrt());
        let w = window(&k, &s, &[vec![edge(&e, 0.0, 360.0, true)]]);
        assert_window(&w, [2.0 - r7, 2.0 + r7], [1.0 - r3, 1.0 + r3]);

        let at = |t: f64| centre + 3.0 * rad(t).cos() * ex + rad(t).sin() * ey;
        let chord = segment(at(135.0), at(-45.0));
        let w = window(&k, &s, &[vec![edge(&e, -45.0, 135.0, true), along(&chord)]]);
        let u = |t: f64| at(t).dot(x);
        let v = |t: f64| at(t).dot(y);
        assert_window(
            &w,
            [u(135.0).min(u(-45.0)), 2.0 + r7],
            [v(135.0).min(v(-45.0)), 1.0 + r3],
        );
    }

    fn sphere(k: &Kernel) -> Surface {
        Surface::from(Sphere::create_canonical(k, &Axis2Placement::IDENTITY, 2.0).unwrap())
    }

    /// Sphere r = 2: the cap above latitude 30, bounded by its rim alone (running up u,
    /// the pole on its left), reaches the pole: u one period, v = [30, 90]. The quarter
    /// of it between the meridians at 0 and 90 degrees, up one to the pole and down the
    /// other: u = [0, 90], v = [30, 90]. The cap below latitude -30, its rim running
    /// down u: v = [-90, -30].
    #[test]
    fn a_spherical_cap_to_the_pole() {
        let k = k();
        let s = sphere(&k);
        let (z, r) = (2.0 * rad(30.0).sin(), 2.0 * rad(30.0).cos());
        let rim = level(&k, z, r);
        let w = window(&k, &s, &[vec![edge(&rim, 0.0, 360.0, true)]]);
        assert_window(&w, [0.0, 360.0], [30.0, 90.0]);

        let east = circle(&k, Vec3::ZERO, Vec3::Y, Vec3::Z, 2.0);
        let west = circle(&k, Vec3::ZERO, Vec3::X, Vec3::Z, 2.0);
        let quarter = vec![
            edge(&rim, 0.0, 90.0, true),
            edge(&east, 30.0, 90.0, true),
            edge(&west, 30.0, 90.0, false),
        ];
        assert_window(&window(&k, &s, &[quarter]), [0.0, 90.0], [30.0, 90.0]);

        let south = level(&k, -z, r);
        let w = window(&k, &s, &[vec![edge(&south, 0.0, 360.0, false)]]);
        assert_window(&w, [0.0, 360.0], [-90.0, -30.0]);
    }

    /// A great circle of the sphere tilted by 20 degrees about Y: its latitude reaches
    /// +-20 inside the edge (at 0 and 180 degrees on it) and its longitude goes round
    /// once, so the face on its left reaches the north pole: v = [-20, 90].
    #[test]
    fn a_tilted_great_circle_on_a_sphere() {
        let k = k();
        let tilt = Vec3::new(rad(20.0).cos(), 0.0, rad(20.0).sin());
        let g = circle(&k, Vec3::ZERO, tilt, Vec3::Y, 2.0);
        let w = window(&k, &sphere(&k), &[vec![edge(&g, 0.0, 360.0, true)]]);
        assert_window(&w, [0.0, 360.0], [-20.0, 90.0]);
    }

    fn frustum(k: &Kernel) -> Surface {
        Surface::from(
            Cone::create_canonical(k, &Axis2Placement::IDENTITY, 2.0, 1.0, 2.0, false, false)
                .unwrap(),
        )
    }

    /// The cone from radius 2 at z = 0 to radius 1 at z = 2 (apex at z = 4): the band
    /// between its rims is u one period, v = [0, 2]; the part between the generators
    /// at 0 and 120 degrees is u = [0, 120]; bounded by its lower rim alone (running up
    /// u), the face reaches the apex: v = [0, 4].
    #[test]
    fn a_cone_frustum() {
        let k = k();
        let s = frustum(&k);
        let (bottom, top) = (level(&k, 0.0, 2.0), level(&k, 2.0, 1.0));
        let w = window(
            &k,
            &s,
            &[
                vec![edge(&bottom, 0.0, 360.0, true)],
                vec![edge(&top, 0.0, 360.0, false)],
            ],
        );
        assert_window(&w, [0.0, 360.0], [0.0, 2.0]);

        let up = segment(round(2.0, 120.0, 0.0), round(1.0, 120.0, 2.0));
        let down = segment(round(1.0, 0.0, 2.0), round(2.0, 0.0, 0.0));
        let part = vec![
            edge(&bottom, 0.0, 120.0, true),
            along(&up),
            edge(&top, 0.0, 120.0, false),
            along(&down),
        ];
        assert_window(&window(&k, &s, &[part]), [0.0, 120.0], [0.0, 2.0]);

        let w = window(&k, &s, &[vec![edge(&bottom, 0.0, 360.0, true)]]);
        assert_window(&w, [0.0, 360.0], [0.0, 4.0]);
    }

    fn torus(k: &Kernel) -> Surface {
        let opts = TorusOptions::default();
        Surface::from(
            Torus::create_canonical(k, &Axis2Placement::IDENTITY, 3.0, 1.0, opts, None).unwrap(),
        )
    }

    /// The tube circle of the torus (R = 3, r = 1) at `deg` degrees about Z, its angle
    /// the torus's v.
    fn tube(k: &Kernel, deg: f64) -> Curve {
        let radial = Vec3::new(rad(deg).cos(), rad(deg).sin(), 0.0);
        circle(k, 3.0 * radial, radial, Vec3::Z, 1.0)
    }

    /// Torus R = 3, r = 1: the quarter between 0 and 90 degrees over the upper half of
    /// the tube (outer equator over the top to the inner one): u = [0, 90],
    /// v = [0, 180]; the outer half of the tube (v from -90 over the seam to 90):
    /// v = [-90, 90].
    #[test]
    fn a_torus_section() {
        let k = k();
        let s = torus(&k);
        let (outer, inner) = (level(&k, 0.0, 4.0), level(&k, 0.0, 2.0));
        let (t0, t90) = (tube(&k, 0.0), tube(&k, 90.0));
        let upper = vec![
            edge(&outer, 0.0, 90.0, true),
            edge(&t90, 0.0, 180.0, true),
            edge(&inner, 0.0, 90.0, false),
            edge(&t0, 0.0, 180.0, false),
        ];
        assert_window(&window(&k, &s, &[upper]), [0.0, 90.0], [0.0, 180.0]);

        let (low, high) = (level(&k, -1.0, 3.0), level(&k, 1.0, 3.0));
        let outside = vec![
            edge(&low, 0.0, 90.0, true),
            edge(&t90, -90.0, 90.0, true),
            edge(&high, 0.0, 90.0, false),
            edge(&t0, -90.0, 90.0, false),
        ];
        assert_window(&window(&k, &s, &[outside]), [0.0, 90.0], [-90.0, 90.0]);
    }

    /// The profile segment from (1, 0, 0) to (2, 0, 1) turned about Z: the band between
    /// its end circles over a quarter is u = [0, 90] and v the profile's [0, 1].
    #[test]
    fn a_revolution_of_a_line() {
        let k = k();
        let profile = segment(p(1.0, 0.0, 0.0), p(2.0, 0.0, 1.0));
        let s = Surface::from(
            SurfOfRevolution::create_canonical(&k, profile, Vec3::ZERO, Vec3::Z, false).unwrap(),
        );
        let (bottom, top) = (level(&k, 0.0, 1.0), level(&k, 1.0, 2.0));
        let up = segment(round(1.0, 90.0, 0.0), round(2.0, 90.0, 1.0));
        let down = segment(round(2.0, 0.0, 1.0), round(1.0, 0.0, 0.0));
        let l = vec![
            edge(&bottom, 0.0, 90.0, true),
            along(&up),
            edge(&top, 0.0, 90.0, false),
            along(&down),
        ];
        assert_window(&window(&k, &s, &[l]), [0.0, 90.0], [0.0, 1.0]);
    }

    /// A band of the cylinder from 150 to 270 degrees (across the angle's branch at 180)
    /// with a hole between 200 and 220 at z in [1, 2]: the hole's angles come out
    /// -160..-140 on their own and are moved into the outer loop's period, so the
    /// window stays u = [150, 270]. The hole alone is 200..220.
    #[test]
    fn a_hole_in_the_period_of_the_outer_loop() {
        let k = k();
        let s = cylinder(&k);
        let band = Band::new(&k, 150.0, 270.0);
        let (lower, upper) = (level(&k, 1.0, 1.0), level(&k, 2.0, 1.0));
        let left = segment(round(1.0, 200.0, 1.0), round(1.0, 200.0, 2.0));
        let right = segment(round(1.0, 220.0, 2.0), round(1.0, 220.0, 1.0));
        // clockwise in (u, v): the face on its left is outside it
        let hole = vec![
            along(&left),
            edge(&upper, 200.0, 220.0, true),
            along(&right),
            edge(&lower, 200.0, 220.0, false),
        ];
        let w = window(&k, &s, &[band.outer(), hole.clone()]);
        assert_window(&w, [150.0, 270.0], [0.0, 3.0]);
        assert_window(&window(&k, &s, &[hole]), [200.0, 220.0], [1.0, 2.0]);
    }

    /// A margin widens every direction that is not a full turn by exactly itself, but
    /// never past a pole.
    #[test]
    fn a_margin_widens_by_exactly_itself() {
        let k = k();
        let band = Band::new(&k, 0.0, 90.0);
        let m = WindowMargin { u: 5.0, v: 0.5 };
        let w = face_window(&k, &cylinder(&k), &[band.outer()], &[], m, TOL).unwrap();
        assert_window(&w, [-5.0, 95.0], [-0.5, 3.5]);

        let rim = level(&k, 2.0 * rad(30.0).sin(), 2.0 * rad(30.0).cos());
        let m = WindowMargin { u: 5.0, v: 10.0 };
        let cap = [vec![edge(&rim, 0.0, 360.0, true)]];
        let w = face_window(&k, &sphere(&k), &cap, &[], m, TOL).unwrap();
        assert_window(&w, [0.0, 360.0], [20.0, 90.0]);
    }

    /// A vertex loop bounds the directions that are not periodic: a cone's apex.
    #[test]
    fn a_vertex_loop_at_the_apex() {
        let k = k();
        let bottom = level(&k, 0.0, 2.0);
        let rim = [vec![edge(&bottom, 0.0, 360.0, true)]];
        let apex = [p(0.0, 0.0, 4.0)];
        let w = face_window(&k, &frustum(&k), &rim, &apex, WindowMargin::default(), TOL).unwrap();
        assert_window(&w, [0.0, 360.0], [0.0, 4.0]);
    }

    /// Faces without edge loops: the whole sphere and torus; a plane has no whole.
    #[test]
    fn faces_without_edge_loops() {
        let k = k();
        assert_window(&window(&k, &sphere(&k), &[]), [0.0, 360.0], [-90.0, 90.0]);
        assert_window(&window(&k, &torus(&k), &[]), [0.0, 360.0], [0.0, 360.0]);
        let plane = Surface::from(Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap());
        assert!(matches!(
            face_window(&k, &plane, &[], &[], WindowMargin::default(), TOL),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// The quadratic Bezier from (0, 0) over (1, 2) to (2, 0) on the XY plane, closed
    /// by its chord: y = 4 t (1 - t) peaks at 1 inside the edge, found as the root of
    /// its rate (a spline has no closed form here): u = [0, 2], v = [0, 1].
    #[test]
    fn a_spline_arc_on_a_plane() {
        let k = k();
        let s = Surface::from(Plane::create_canonical(&Axis2Placement::IDENTITY).unwrap());
        let arc = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &[p(0.0, 0.0, 0.0), p(1.0, 2.0, 0.0), p(2.0, 0.0, 0.0)],
                form: BSplineCurveForm::PolylineForm,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        );
        let chord = segment(p(2.0, 0.0, 0.0), p(0.0, 0.0, 0.0));
        let w = window(&k, &s, &[vec![edge(&arc, 0.0, 1.0, true), along(&chord)]]);
        assert_window(&w, [0.0, 2.0], [0.0, 1.0]);
    }

    /// `circle` as its rational B-spline (the kernel's NURB of it), over its natural
    /// interval.
    fn as_spline(c: &Curve) -> (Curve, Extent1d) {
        let CurveKind::Ellipse(e) = c.kind() else {
            panic!("not a conic");
        };
        let b = Curve::from(e.base().clone());
        let ivl = b.natural_interval();
        (b, ivl)
    }

    /// The cylinder band, a torus section and the tilted great circle of the sphere
    /// with their circles as B-splines: the angles unwind along the splines' natural
    /// parameters, and the great circle's latitude extremes (+-20) are the roots of its
    /// rate.
    #[test]
    fn spline_circles_on_a_cylinder_and_a_sphere() {
        let k = k();
        let (bottom, bi) = as_spline(&level(&k, 0.0, 1.0));
        let (top, ti) = as_spline(&level(&k, 3.0, 1.0));
        let spline = |c, i: Extent1d, forward| edge(c, i.min(), i.max(), forward);
        let band = [
            vec![spline(&bottom, bi, true)],
            vec![spline(&top, ti, false)],
        ];
        assert_window(&window(&k, &cylinder(&k), &band), [0.0, 360.0], [0.0, 3.0]);

        // the upper quarter of `a_torus_section`, its tube circles' halves 0..180 as
        // the splines' 0..0.5 (their knots at the quarter turns)
        let (outer, inner) = (level(&k, 0.0, 4.0), level(&k, 0.0, 2.0));
        let ((t0, _), (t90, _)) = (as_spline(&tube(&k, 0.0)), as_spline(&tube(&k, 90.0)));
        let upper = vec![
            edge(&outer, 0.0, 90.0, true),
            edge(&t90, 0.0, 0.5, true),
            edge(&inner, 0.0, 90.0, false),
            edge(&t0, 0.0, 0.5, false),
        ];
        assert_window(&window(&k, &torus(&k), &[upper]), [0.0, 90.0], [0.0, 180.0]);

        let tilt = Vec3::new(rad(20.0).cos(), 0.0, rad(20.0).sin());
        let (g, gi) = as_spline(&circle(&k, Vec3::ZERO, tilt, Vec3::Y, 2.0));
        let w = window(&k, &sphere(&k), &[vec![spline(&g, gi, true)]]);
        assert_window(&w, [0.0, 360.0], [-20.0, 90.0]);
    }

    /// The closed forms of the plane, cone, cylinder, sphere and torus, and the
    /// revolution's angle, are the kernel's STEP parameters: at points the kernel
    /// evaluates at (u, v) they give (u, v) back.
    #[test]
    fn the_parameters_are_the_kernels_step_parameters() {
        let k = k();
        let frame = Axis2Placement::new(
            p(1.0, 2.0, 3.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
        );
        let mut plane = Plane::new(
            frame.origin(),
            frame.x_axis(),
            frame.y_axis(),
            Point2::new(2.0, 0.5),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        );
        plane.make_nurb().unwrap();
        let revolution = SurfOfRevolution::create_canonical(
            &k,
            segment(p(1.0, 0.0, 0.0), p(2.0, 0.0, 1.0)),
            Vec3::ZERO,
            Vec3::Z,
            false,
        )
        .unwrap();
        let surfaces = [
            Surface::from(plane),
            Surface::from(Cone::create_canonical(&k, &frame, 2.0, 1.0, 2.0, false, false).unwrap()),
            Surface::from(Cone::create_canonical_cylinder(&k, &frame, 1.5).unwrap()),
            Surface::from(Sphere::create_canonical(&k, &frame, 2.0).unwrap()),
            Surface::from(
                Torus::create_canonical(&k, &frame, 3.0, 1.0, TorusOptions::default(), None)
                    .unwrap(),
            ),
            Surface::from(revolution),
        ];
        for (i, s) in surfaces.iter().enumerate() {
            let dom = s.step_uv_domain();
            let map = Map::of(s, TOL);
            for (fu, fv) in [(0.1, 0.2), (0.45, 0.5), (0.7, 0.9), (0.95, 0.3)] {
                let u = dom.u_min().max(-1e3) + fu * (dom.u_max().min(1e3) - dom.u_min().max(-1e3));
                let v = dom.v_min().max(-1e3) + fv * (dom.v_max().min(1e3) - dom.v_min().max(-1e3));
                let q = map
                    .params(
                        &k,
                        s.evaluate_step_point(&k, Point2::new(u, v)).unwrap(),
                        true,
                    )
                    .unwrap();
                let du = q.u.unwrap() - u;
                let du = if map.periodic(0) { nearest(du) } else { du };
                assert!(
                    du.abs() <= 1e-12 * (1.0 + u.abs()),
                    "surface {i}: u {u} -> {q:?}"
                );
                let dv = if map.periodic(1) {
                    nearest(q.v - v)
                } else {
                    q.v - v
                };
                assert!(
                    dv.abs() <= 1e-12 * (1.0 + v.abs()),
                    "surface {i}: v {v} -> {q:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_inputs_are_refused() {
        let k = k();
        let s = cylinder(&k);
        let (bottom, top) = (level(&k, 0.0, 1.0), level(&k, 3.0, 1.0));
        let band = vec![
            vec![edge(&bottom, 0.0, 360.0, true)],
            vec![edge(&top, 0.0, 360.0, false)],
        ];
        let refused = |loops: &[Vec<BoundaryEdge<'_>>], m: WindowMargin, tol: f64| {
            matches!(
                face_window(&k, &s, loops, &[], m, tol),
                Err(SolidError::InvalidInput { .. })
            )
        };
        let none = WindowMargin::default();
        assert!(refused(&band, none, -1.0));
        assert!(refused(&band, none, f64::NAN));
        assert!(refused(&band, WindowMargin { u: -1.0, v: 0.0 }, TOL));
        assert!(refused(
            &band,
            WindowMargin {
                u: 0.0,
                v: f64::INFINITY
            },
            TOL
        ));
        assert!(refused(&[vec![]], none, TOL));
        let not_finite = BoundaryEdge {
            curve: &bottom,
            interval: Extent1d::from_raw(0.0, f64::NAN),
            forward: true,
        };
        assert!(refused(&[vec![not_finite]], none, TOL));
        let reversed = BoundaryEdge {
            curve: &bottom,
            interval: Extent1d::from_raw(90.0, 0.0),
            forward: true,
        };
        assert!(refused(&[vec![reversed]], none, TOL));
        // a circle off the cylinder
        let off = level(&k, 0.0, 1.5);
        assert!(refused(&[vec![edge(&off, 0.0, 360.0, true)]], none, TOL));
        // one circle does not bound a cylinder
        assert!(refused(&[vec![edge(&bottom, 0.0, 360.0, true)]], none, TOL));
        // both circles round the same way
        let twice = vec![
            vec![edge(&bottom, 0.0, 360.0, true)],
            vec![edge(&top, 0.0, 360.0, true)],
        ];
        assert!(refused(&twice, none, TOL));
    }
}
