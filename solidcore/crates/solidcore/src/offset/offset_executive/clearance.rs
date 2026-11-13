//! Not upstream: proving that the edges a local operation kept are clear of the faces it
//! moved.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`separated`], [`all_clear`] | none |
//! | [`endpoint_contact::separated_ending_on_face`] | none |
//!
//! What is tested (issue #2971's certified bound): a kept edge's whole curve against the
//! trimmed domain of each moved result face, the surface restricted to the box of the
//! face's trim curves in its parameters (`surface_distance_lower_bound`'s `uv_box`), never
//! the surface's whole extension: a plane's net is hundreds of thousands of units wide and
//! crosses edges of solids the face never reaches. A curve interval is enclosed by rounded interval evaluation, then by a ball B(c,r).
//! Distance to a set is 1-Lipschitz: d(C,S) >= d(c,S) - r. The surface distance is the
//! existing certified bound (positive-weight Bernstein hulls, including the definition's
//! deviation and arithmetic rounding), not a projection or a sample. Thus every accepted
//! cell proves all its curve points clear, including between evaluated parameters.
//!
//! What is tested (issue #13622's endpoint contact): a rebuilt edge may legitimately END
//! ON a moved face — the repaired corner's bottom edge ends on the moved top face's
//! corner — which the strict predicate above refuses. [`endpoint_contact::separated_ending_on_face`]
//! certifies that this contact is the edge's only one: the contact is within the
//! tolerance and in-or-on the face's trimmed region (certified proximity to a loop
//! vertex or a trim curve, else crossing parity over every loop's UV trim curves); the
//! system `S(u, v) - F(t) = 0` has exactly one solution over the grown loop enclosures
//! near the contact (Krawczyk and Banach on the 3 x 3 system; the window's remaining
//! slabs excluded or subdivided); and outside the window every cell proves no further
//! contact (a plane's signed side, a general surface's certified distance bound over the
//! face's UV box less the cell's ball radius). Nothing unproved is ever accepted.
//!
//! Deviations: no recursion; at most 255 curve cells and twelve halvings per branch (a cell is split only while its
//! ball is not proved clear, so the cells gather where the curve passes near the face). A kind
//! without an enclosure, a nonfinite enclosure, or an exhausted budget proves nothing.
//! NURBS degree is at most 18 here: the factorials in `curve_derivs_over` are then exact
//! binary64 integers. Lines are evaluated on their definition, not a rounded fitted form.
//! The endpoint contact runs the same budgets per stage as at most 4096 boxes and
//! thirty-two halvings per branch (the contact box halves until its Krawczyk test
//! certifies, within the same bound). Its window and contact box reach past the edge's
//! interval onto the curve's exact extension (the end span's polynomial continued,
//! docs/definitions-and-certificates.md §6), so the uniqueness claim covers the
//! extension too: a stronger claim, never a weaker one. The contact's UV, the
//! isolation's Jacobian and the UV-to-face distances are certified on a plane only (the
//! evaluator's own affine definition, `Plane::evaluate_point`): a moved face of another
//! kind is refused at the contact's UV. A general surface's net does have interval
//! partials (`certificate::nurbs_partials_over_box`), but the deviation of an evaluated
//! definition from its net is certified in value only (`definition_bound`,
//! docs/definitions-and-certificates.md §4.1), never in derivatives, so a general
//! Jacobian has no certified enclosure here; the general remainder's whole-cell distance
//! bound is implemented and tested at stage level, and runs from the entry point once a
//! general isolation exists.

use crate::blend::fillet_section::certificate::{curve_derivs_over, NurbsCurve};
use crate::blend::fillet_section::interval::{round_up, Ivl};
use crate::curve::curve::{Curve, CurveKind};
use crate::curve::line::Line;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::{NOW, NOZ};
use crate::query::distance_bound::surface_distance_lower_bound;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::{Result, SolidError};

const OP: &str = "offset::offset_executive::clearance";

/// An edge a local operation kept as it was: its curve over its interval, and the
/// tolerance (its own, at least the body's) it must stay clear within.
#[derive(Debug, Clone)]
pub(crate) struct KeptEdge {
    pub(crate) curve: Curve,
    pub(crate) interval: Extent1d,
    pub(crate) tolerance: f64,
}

/// A face a local operation moved, as the result has it: its surface, the box of its trim
/// curves in the surface's parameters (`None` when a trim curve has no box: the whole
/// surface is then tested), and its tolerance (at least the body's).
#[derive(Debug, Clone)]
pub(crate) struct MovedFace {
    pub(crate) surface: Surface,
    pub(crate) uv_box: Option<Extent2d>,
    pub(crate) tolerance: f64,
}

/// Every kept edge is proved clear of every moved face by more than their summed
/// tolerances (rounded up). `false` means some pair is unproved, not necessarily crossing.
///
/// # Errors
/// Cancellation.
pub(crate) fn all_clear(kernel: &Kernel, kept: &[KeptEdge], moved: &[MovedFace]) -> Result<bool> {
    for edge in kept {
        for face in moved {
            let tolerance = (Ivl::from(edge.tolerance) + Ivl::from(face.tolerance)).hi;
            if !separated(
                kernel,
                &edge.curve,
                &edge.interval,
                &face.surface,
                face.uv_box.as_ref(),
                tolerance,
            )? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
const MAX_DEPTH: usize = 12;
const MAX_CELLS: usize = 255;

pub(crate) enum EnclosedCurve<'a> {
    Line(&'a Line),
    Nurbs(NurbsCurve),
}

impl<'a> EnclosedCurve<'a> {
    fn of(curve: &'a Curve) -> Option<Self> {
        let base = match curve.kind() {
            CurveKind::Line(line) => return Some(Self::Line(line)),
            CurveKind::BSpline(c) => c,
            // Curve::evaluate_point reads these conics' stored rational forms too.
            CurveKind::Ellipse(c) => c.base(),
            CurveKind::Parabola(c) => c.base(),
            CurveKind::Hyperbola(c) => c.base(),
            _ => return None,
        };
        let net = base.gw_nurb()?;
        let (p, n) = (net.degree(), net.cpts().len());
        if !(1..=18).contains(&p) || n <= p || net.knots().len() != n + p + 1 {
            return None;
        }
        // Read the stored homogeneous coefficients directly: control_polygon followed
        // by NurbsCurve::of would divide and multiply rational coordinates again.
        let mut pw = Vec::new();
        for q in net.cpts() {
            let w = if q.w == NOW { 1.0 } else { q.w };
            if !(q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && w.is_finite())
                || q.z == NOZ
                || w <= 0.0
            {
                return None;
            }
            pw.extend_from_slice(&[q.x, q.y, q.z, w]);
        }
        let u = net.knots().to_vec();
        if u.iter().any(|x| !x.is_finite()) || u.iter().zip(u.iter().skip(1)).any(|(a, b)| a > b) {
            return None;
        }
        Some(Self::Nurbs(NurbsCurve { p, n, u, pw }))
    }

    fn over(&self, t: Ivl) -> Result<Option<[Ivl; 3]>> {
        match self {
            Self::Line(line) => Ok(Self::line_over(line, t)),
            Self::Nurbs(net) => {
                let (Some(&lo), Some(&hi)) = (net.u.get(net.p), net.u.get(net.n)) else {
                    return Ok(None);
                };
                if !(t.lo >= lo && t.hi <= hi) {
                    return Ok(None);
                }
                let derivatives = curve_derivs_over(net, t, 0, None)?;
                Ok(derivatives.first().map(|p| [p.x, p.y, p.z]))
            }
        }
    }

    /// [`Self::over`] with the curve's exact extension past its knots' range (the end
    /// span's polynomial continued, `sm_Span`'s end spans): a window around an endpoint
    /// contact reaches past the edge's interval, and the uniqueness claim over the
    /// extension is a stronger claim than the edge needs.
    fn over_ext(&self, t: Ivl) -> Result<Option<[Ivl; 3]>> {
        match self {
            Self::Line(line) => Ok(Self::line_over(line, t)),
            Self::Nurbs(net) => {
                let derivatives = curve_derivs_over(net, t, 0, None)?;
                Ok(derivatives.first().map(|p| [p.x, p.y, p.z]))
            }
        }
    }

    /// The derivative enclosure over `t` (as [`Self::over_ext`], the end spans
    /// continued): a line's mapped slope times its vector, a B-spline's
    /// `curve_derivs_over` order 1. Never a plain slope wrapped after.
    fn derivs_over(&self, t: Ivl) -> Result<Option<[Ivl; 3]>> {
        match self {
            Self::Line(line) => Ok(Self::line_deriv(line, t)),
            Self::Nurbs(net) => {
                let derivatives = curve_derivs_over(net, t, 1, None)?;
                Ok(derivatives.get(1).map(|p| [p.x, p.y, p.z]))
            }
        }
    }

    /// The line's affine NURBS-to-STEP parameter map over `t`, enclosed: the step the
    /// point maps to and the map's slope (the map is affine, so the slope is its
    /// derivative). One shape for [`Self::line_over`] and [`Self::line_deriv`]: the
    /// bounded / `inside_out` / zero-length validation, then the enclosed quotient and
    /// products (`Line::evaluate_point`). `None` for a bounded line of no length.
    fn line_map(line: &Line, t: Ivl) -> Option<(Ivl, Ivl)> {
        let mut step = t;
        let mut slope = Ivl::from(1.0);
        if line.is_bounded() {
            let nurb = line.natural_interval();
            let domain = line.step_interval();
            let length = Ivl::from(nurb.max()) - Ivl::from(nurb.min());
            if length.lo.is_nan() || length.lo <= 0.0 || crate::geom::tol::is_zero(nurb.length()) {
                return None;
            }
            let span = Ivl::from(domain.max()) - Ivl::from(domain.min());
            let (sign, along) = if line.inside_out() {
                (-1.0, (Ivl::from(nurb.max()) - t) / length)
            } else {
                (1.0, (t - Ivl::from(nurb.min())) / length)
            };
            step = Ivl::from(domain.min()) + along * span;
            slope = Ivl::from(sign) / length * span;
        } else if line.inside_out() {
            step = Ivl::from(0.0) - step;
            slope = Ivl::from(-1.0);
        }
        Some((step, slope))
    }

    /// The line's enclosure over `t`: the affine NURBS-to-STEP parameter map
    /// ([`Self::line_map`]), with every subtraction, division and multiplication
    /// enclosed (`Line::evaluate_point`).
    fn line_over(line: &Line, t: Ivl) -> Option<[Ivl; 3]> {
        let (step, _) = Self::line_map(line, t)?;
        let p = line.line_point();
        let v = line.line_vector();
        let s = step * Ivl::from(line.line_scale());
        Some([
            Ivl::from(p.x) + s * Ivl::from(v.x),
            Ivl::from(p.y) + s * Ivl::from(v.y),
            Ivl::from(p.z) + s * Ivl::from(v.z),
        ])
    }

    /// The line's derivative: [`Self::line_map`]'s slope (the map is affine), the
    /// quotient and products enclosed.
    fn line_deriv(line: &Line, t: Ivl) -> Option<[Ivl; 3]> {
        let (_, slope) = Self::line_map(line, t)?;
        let p = line.line_vector();
        let s = slope * Ivl::from(line.line_scale());
        Some([s * Ivl::from(p.x), s * Ivl::from(p.y), s * Ivl::from(p.z)])
    }
}

/// Every point of `curve` over `interval` is proved farther than `tolerance` from the
/// part of `surface` over `uv_box` (the whole finite image when `None`).
/// `false` means unproved, not necessarily intersecting. No curve or surface samples can
/// make this return true.
///
/// # Errors
/// Cancellation is propagated. An unavailable certificate returns `false`, so the
/// executive takes the ordinary re-intersection path instead of keeping this edge.
pub(crate) fn separated(
    kernel: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    surface: &Surface,
    uv_box: Option<&Extent2d>,
    tolerance: f64,
) -> Result<bool> {
    let Some(curve) = EnclosedCurve::of(curve) else {
        return Ok(false);
    };
    let (lo, hi) = (interval.min(), interval.max());
    if !(lo.is_finite() && hi.is_finite() && lo < hi && tolerance.is_finite() && tolerance >= 0.0) {
        return Ok(false);
    }
    let mut stack = vec![(Ivl::new(lo, hi), 0_usize)];
    let mut cells = 0;
    while let Some((t, depth)) = stack.pop() {
        kernel.check_cancelled(OP)?;
        cells += 1;
        if cells > MAX_CELLS {
            return Ok(false);
        }
        let box_ = match curve.over(t) {
            Ok(Some(box_)) => box_,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            _ => return Ok(false),
        };
        if box_
            .iter()
            .any(|x| !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi))
        {
            return Ok(false);
        }
        // The centre need not lie on the curve. Its computed binary64 value is exact
        // data for both bounds: interval subtraction encloses the ball's radius.
        let centre = box_.map(|x| 0.5 * x.lo + 0.5 * x.hi);
        let mut radius2 = Ivl::from(0.0);
        for (x, c) in box_.into_iter().zip(centre) {
            let delta = x - Ivl::from(c);
            radius2 = radius2 + delta * delta;
        }
        let radius = radius2.sqrt().hi;
        let want = (Ivl::from(radius) + Ivl::from(tolerance)).hi;
        if !want.is_finite() {
            return Ok(false);
        }
        let [cx, cy, cz] = centre;
        let lower = match surface_distance_lower_bound(
            kernel,
            surface,
            uv_box,
            Point3::new(cx, cy, cz),
            want,
        ) {
            Ok(lower) => lower,
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            Err(_) => return Ok(false),
        };
        if (Ivl::from(lower) - Ivl::from(radius)).lo > tolerance {
            continue;
        }
        if depth >= MAX_DEPTH {
            return Ok(false);
        }
        let mid = 0.5 * t.lo + 0.5 * t.hi;
        if !(mid > t.lo && mid < t.hi) {
            return Ok(false);
        }
        stack.push((Ivl::new(mid, t.hi), depth + 1));
        stack.push((Ivl::new(t.lo, mid), depth + 1));
    }
    Ok(true)
}

// ---- Endpoint contact (issue #13622) ----------------------------------------------------

// The section certifies the clips of the crossed-boundary reconstruction
// (`topology::brep::local_operation`'s `contact_proof` builds these requests, and its
// `validate_completed_boundary` re-checks the records the kept-edge gate reads); the
// regression tests below call the stages directly.
pub(crate) mod endpoint_contact {
    use super::{
        round_up, surface_distance_lower_bound, Curve, EnclosedCurve, Extent1d, Ivl, Kernel,
        MovedFace, Point3, Result, SolidError, Surface, SurfaceKind, Vec3, OP,
    };

    /// At most this many boxes per stage and halvings per branch of an endpoint-contact
    /// proof (a budget hit never accepts).
    const CONTACT_BUDGET: usize = 4096;
    /// The halvings one branch may take before it proves nothing.
    const MAX_HALVINGS: usize = 32;
    /// The refinement steps run on the enclosed root.
    const ROOT_REFINEMENTS: usize = 4;
    /// The growth of the loop enclosures that makes the contact's UV strictly interior.
    const LOOP_GROWTH: f64 = 0.05;

    /// A loop's trim segment: the UV curve of one edge use (its points' first two
    /// coordinates are the surface's parameters) over the interval it covers.
    #[derive(Debug, Clone)]
    pub(crate) struct TrimSegment {
        /// The UV trim curve.
        pub(crate) curve: Curve,
        /// The part of the curve the segment covers.
        pub(crate) interval: Extent1d,
    }

    /// A loop of a face's trimmed region: its trim segments in order. A loop without
    /// segments refuses (nothing is proved from an untrimmed loop).
    #[derive(Debug, Clone)]
    pub(crate) struct TrimLoop {
        /// The loop's trim segments.
        pub(crate) segments: Vec<TrimSegment>,
    }

    /// A face's trimmed region: its loops, the outer one and the inner ones (a point inside
    /// an inner loop is in a hole and not in the region).
    #[derive(Debug, Clone)]
    pub(crate) struct TrimRegion {
        /// The region's loops.
        pub(crate) loops: Vec<TrimLoop>,
    }

    /// What an endpoint-contact certificate proved. `Proved` iff the rebuilt edge touches
    /// the moved face exactly where it is meant to: the contact is within the tolerance,
    /// in-or-on the trimmed region, the only contact near the edge's end and the rest of the
    /// edge never touches. `Outside` is proved too (the contact is certified off the trimmed
    /// region: the edge ends on the surface, not on the face). `Unproved` refuses: an
    /// unproved pair is never kept.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) enum ContactProof {
        /// The edge's only contact with the face is its contact end.
        Proved(ContactReport),
        /// The contact's UV is certified off the face's trimmed region.
        Outside(ContactReport),
        /// Nothing is proved; `reason` names the certificate that failed.
        Unproved(ContactRefusal, ContactReport),
    }

    /// The certificate that failed (every one of them refuses: a cap never accepts).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ContactRefusal {
        /// The edge's curve or a trim curve has no enclosure (a kind without one).
        NoCurveEnclosure,
        /// The arguments do not describe a contact on the edge's interval.
        Inputs,
        /// `|F(contact_t) - contact_point|` is not within the tolerance.
        Incidence,
        /// The face has no certified interval form here: the contact's UV, the isolation's
        /// Jacobian and the UV-to-face distances are certified on a plane only.
        SurfaceUncertified,
        /// A loop has no trim segments (the trimmed region is unknown).
        RegionMissing,
        /// The region stage's budget was exhausted.
        RegionBudget,
        /// A region cell proved nothing within its halvings.
        RegionUnresolved,
        /// The contact's UV is not strictly interior to the grown loop enclosures, or the
        /// contact box cannot be sized around it.
        IsolationGeometry,
        /// The Jacobian at the contact does not invert (its determinant encloses 0).
        SingularJacobian,
        /// The contact box's Krawczyk test never certified within its halvings.
        IsolationUnresolved,
        /// The isolation stage's budget was exhausted.
        IsolationBudget,
        /// The enclosed root is not the intended contact.
        RootNotContact,
        /// A remainder cell proved nothing within its halvings.
        RemainderUnresolved,
        /// The remainder stage's budget was exhausted.
        RemainderBudget,
    }

    /// What a proof measured: the boxes each stage examined (against the stage's budget) and
    /// the enclosed root's parameter interval.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct ContactReport {
        /// `|F(contact_t) - contact_point|`'s certified upper bound.
        pub(crate) incidence: f64,
        /// The boxes the isolation stage examined (the contact box counts once).
        pub(crate) isolation_boxes: usize,
        /// The boxes the remainder stage examined.
        pub(crate) remainder_boxes: usize,
        /// The boxes the region-membership stage examined.
        pub(crate) region_boxes: usize,
        /// The enclosed root's parameter interval, when one was enclosed.
        pub(crate) root_t: Option<(f64, f64)>,
    }

    /// `x * x` enclosed both ways: the square's range over the interval, every end rounded
    /// outward (the product `x * x` alone takes its lower end below 0 where `x` holds 0).
    fn square(x: Ivl) -> Ivl {
        Ivl::checked(x.mig() * x.mig(), x.mag() * x.mag())
    }

    /// The intersection of two intervals (empty where its lower end passes its upper).
    fn meet(a: Ivl, b: Ivl) -> Ivl {
        Ivl::new(a.lo.max(b.lo), a.hi.min(b.hi))
    }

    /// The two halves of a parameter interval, `None` where the bisection stagnates.
    fn bisect(t: Ivl) -> Option<(Ivl, Ivl)> {
        let mid = t.mid();
        if !(mid > t.lo && mid < t.hi) {
            return None;
        }
        Some((Ivl::new(t.lo, mid), Ivl::new(mid, t.hi)))
    }

    /// The radius of the ball centred at `centre` holding every point of the cell `box_`
    /// (`separated`'s ball).
    fn ball_radius(box_: [Ivl; 2], centre: [f64; 2]) -> f64 {
        let mut radius2 = Ivl::from(0.0);
        for (x, c) in box_.iter().zip(centre) {
            let delta = *x - Ivl::from(c);
            radius2 = radius2 + square(delta);
        }
        radius2.sqrt().hi
    }

    /// `|a - b|`'s certified upper bound (rounded interval evaluation).
    fn point_distance_hi(a: [Ivl; 3], b: Point3) -> Option<f64> {
        let mut d2 = Ivl::from(0.0);
        for (x, y) in a.iter().zip([b.x, b.y, b.z]) {
            d2 = d2 + square(*x - Ivl::from(y));
        }
        let d = d2.sqrt();
        if d.hi.is_finite() {
            Some(d.hi)
        } else {
            None
        }
    }

    /// The rounded inverse of a 3 x 3 matrix (the Krawczyk preconditioner's `C`): the
    /// adjugate over the determinant, every entry an outward-rounded interval (the blend
    /// certificate's `inverse4`, in 3 x 3). `None` where the determinant encloses 0.
    fn inverse3(m: [[Ivl; 3]; 3]) -> Option<[[Ivl; 3]; 3]> {
        let [[a, b, c], [d, e, f], [g, h, i]] = m;
        let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if !(det.lo > 0.0 || det.hi < 0.0) {
            return None;
        }
        Some([
            [
                (e * i - f * h) / det,
                (c * h - b * i) / det,
                (b * f - c * e) / det,
            ],
            [
                (f * g - d * i) / det,
                (a * i - c * g) / det,
                (c * d - a * f) / det,
            ],
            [
                (d * h - e * g) / det,
                (b * g - a * h) / det,
                (a * e - b * d) / det,
            ],
        ])
    }

    /// The plane of a moved face as its own evaluator's affine definition,
    /// `S(u, v) = origin + u * su * X + v * sv * Y` (`Plane::evaluate_point`), so every
    /// enclosure here holds the function the face evaluates, not a fitted form.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct PlaneOver {
        origin: Point3,
        x_axis: Vec3,
        y_axis: Vec3,
        su: f64,
        sv: f64,
        normal: Vec3,
    }

    impl PlaneOver {
        /// The plane's definition, or `None` for a face that is not planar.
        fn of(surface: &Surface) -> Option<Self> {
            match surface.kind() {
                SurfaceKind::Plane(p) => {
                    let scale = p.uv_scale();
                    Some(Self {
                        origin: p.position().origin(),
                        x_axis: p.position().x_axis(),
                        y_axis: p.position().y_axis(),
                        su: scale.x,
                        sv: scale.y,
                        normal: p.evaluate_normal(),
                    })
                }
                _ => None,
            }
        }

        /// `S` over `[u] x [v]`, every product rounded outward.
        fn s_over(&self, u: Ivl, v: Ivl) -> [Ivl; 3] {
            let du = u * Ivl::from(self.su);
            let dv = v * Ivl::from(self.sv);
            let mut out = [Ivl::from(0.0); 3];
            for ((x, y, o), row) in [
                (self.x_axis.x, self.y_axis.x, self.origin.x),
                (self.x_axis.y, self.y_axis.y, self.origin.y),
                (self.x_axis.z, self.y_axis.z, self.origin.z),
            ]
            .into_iter()
            .zip(out.iter_mut())
            {
                *row = Ivl::from(o) + du * Ivl::from(x) + dv * Ivl::from(y);
            }
            out
        }

        /// The UV of a 3D point on the plane, enclosed: the exact inverse of the affine
        /// map (`Plane::project_point_to_uv_domain`), never a projection solved in plain
        /// arithmetic and wrapped after.
        fn uv_of(&self, p: Point3) -> [Ivl; 2] {
            let d = [
                Ivl::from(p.x) - Ivl::from(self.origin.x),
                Ivl::from(p.y) - Ivl::from(self.origin.y),
                Ivl::from(p.z) - Ivl::from(self.origin.z),
            ];
            let u = (d[0] * Ivl::from(self.x_axis.x)
                + d[1] * Ivl::from(self.x_axis.y)
                + d[2] * Ivl::from(self.x_axis.z))
                / Ivl::from(self.su);
            let v = (d[0] * Ivl::from(self.y_axis.x)
                + d[1] * Ivl::from(self.y_axis.y)
                + d[2] * Ivl::from(self.y_axis.z))
                / Ivl::from(self.sv);
            [u, v]
        }

        /// The surface's Jacobian columns `[S_u, S_v]` (constant; the products rounded
        /// outward).
        fn jacobian(&self) -> [[Ivl; 3]; 2] {
            let du = Ivl::from(self.su);
            let dv = Ivl::from(self.sv);
            [
                [
                    du * Ivl::from(self.x_axis.x),
                    du * Ivl::from(self.x_axis.y),
                    du * Ivl::from(self.x_axis.z),
                ],
                [
                    dv * Ivl::from(self.y_axis.x),
                    dv * Ivl::from(self.y_axis.y),
                    dv * Ivl::from(self.y_axis.z),
                ],
            ]
        }

        /// The signed side of a 3D point relative to the plane, `n * (p - origin)`
        /// enclosed: `g.lo > 0` or `g.hi < 0` proves the point off the plane.
        fn side(&self, p: [Ivl; 3]) -> Ivl {
            let mut g = Ivl::from(0.0);
            for ((x, o), n) in p
                .iter()
                .zip([self.origin.x, self.origin.y, self.origin.z])
                .zip([self.normal.x, self.normal.y, self.normal.z])
            {
                g = g + Ivl::from(n) * (*x - Ivl::from(o));
            }
            g
        }

        /// The face's 3D distance from the contact's UV enclosure to the UV ball
        /// `B(centre, radius)`, enclosed both ways: `hi` bounds the distance to the ball's
        /// points (proximity of the contact to anything the ball holds), `lo` the distance
        /// to all of them (exclusion). The plane's scales carry UV to the face:
        /// `|S(p) - S(q)| = |(su du, sv dv)|`.
        fn uv_ball_distance(&self, p: [Ivl; 2], centre: [f64; 2], radius: f64) -> Option<Ivl> {
            let spread = Ivl::new(0.0 - radius, radius);
            let du = (p[0] - Ivl::from(centre[0])) + spread;
            let dv = (p[1] - Ivl::from(centre[1])) + spread;
            let su = Ivl::from(self.su);
            let sv = Ivl::from(self.sv);
            let d = (su * su * square(du) + sv * sv * square(dv)).sqrt();
            if d.lo.is_nan() || d.hi.is_nan() {
                None
            } else {
                Some(d)
            }
        }
    }

    /// What the region machinery proved about the contact's UV.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum RegionVerdict {
        /// Inside the outer loop and outside every inner one (crossing parity odd).
        Inside,
        /// Within the tolerance of a loop vertex or of a trim curve.
        Boundary,
        /// Certified off the trimmed region and outside its tolerance band.
        Outside,
        /// Nothing is proved (fail closed).
        Refused(ContactRefusal),
    }

    /// The contact's UV certified in-or-on the trimmed region. Proximity to a loop vertex or
    /// to a trim curve is [`RegionVerdict::Boundary`] (the curve's own point and ball
    /// enclosures, never a box: a point in a boundary curve's box but off the curve is not
    /// on the region); failing that, certified crossing parity over every loop's trim curves
    /// gives [`RegionVerdict::Inside`] (odd) or [`RegionVerdict::Outside`] (even, the
    /// tolerance band proved clear of the curves). A cell proving none of these subdivides
    /// (the budget and the halvings bound it); anything unresolved refuses.
    fn region_membership(
        kernel: &Kernel,
        segments: &[(EnclosedCurve<'_>, Extent1d)],
        plane: &PlaneOver,
        p: [Ivl; 2],
        tolerance: f64,
        budget: usize,
        boxes: &mut usize,
    ) -> Result<RegionVerdict> {
        // 1. proximity to a loop vertex: the segments' interval ends, interval-evaluated
        for (segment, ivl) in segments {
            for t in [ivl.min(), ivl.max()] {
                kernel.check_cancelled(OP)?;
                *boxes += 1;
                if *boxes > budget {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionBudget));
                }
                let Some(v) = segment.over_ext(Ivl::from(t))? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let centre = [v[0].mid(), v[1].mid()];
                let radius = ball_radius([v[0], v[1]], centre);
                let Some(d) = plane.uv_ball_distance(p, centre, radius) else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                };
                if d.hi <= tolerance {
                    return Ok(RegionVerdict::Boundary);
                }
            }
        }
        // 2. proximity to a trim curve: a cell's ball excludes it, the curve's point at the
        // cell's middle accepts it
        for (segment, ivl) in segments {
            let mut stack = vec![(Ivl::new(ivl.min(), ivl.max()), 0_usize)];
            while let Some((t, depth)) = stack.pop() {
                kernel.check_cancelled(OP)?;
                *boxes += 1;
                if *boxes > budget {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionBudget));
                }
                let Some(b) = segment.over_ext(t)? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let cell = [b[0], b[1]];
                if cell
                    .iter()
                    .any(|x| !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi))
                {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                }
                let centre = [cell[0].mid(), cell[1].mid()];
                let radius = ball_radius(cell, centre);
                let Some(d) = plane.uv_ball_distance(p, centre, radius) else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                };
                if d.lo > tolerance {
                    continue; // the cell's whole piece is outside the tolerance band
                }
                let Some(m) = segment.over_ext(Ivl::from(t.mid()))? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let m_centre = [m[0].mid(), m[1].mid()];
                let m_radius = ball_radius([m[0], m[1]], m_centre);
                let Some(d_point) = plane.uv_ball_distance(p, m_centre, m_radius) else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                };
                if d_point.hi <= tolerance {
                    return Ok(RegionVerdict::Boundary);
                }
                if depth >= MAX_HALVINGS {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                }
                let Some((lo, hi)) = bisect(t) else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                };
                stack.push((lo, depth + 1));
                stack.push((hi, depth + 1));
            }
        }
        // 3. crossing parity of the ray {(u, v_p) : u >= u_p} with every trim curve: a
        // cell whose ends lie on one side of the band crosses it evenly (nothing to count),
        // one whose ends straddle it crosses oddly and counts 1 where the cell lies beyond
        // the point, 0 where it lies behind; anything else subdivides
        let mut crossings = 0_usize;
        for (segment, ivl) in segments {
            let mut stack = vec![(Ivl::new(ivl.min(), ivl.max()), 0_usize)];
            while let Some((t, depth)) = stack.pop() {
                kernel.check_cancelled(OP)?;
                *boxes += 1;
                if *boxes > budget {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionBudget));
                }
                let Some(b) = segment.over_ext(t)? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let Some(v0) = segment.over_ext(Ivl::from(t.lo))? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let Some(v1) = segment.over_ext(Ivl::from(t.hi))? else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::NoCurveEnclosure));
                };
                let (below0, above0) = (v0[1].hi < p[1].lo, v0[1].lo > p[1].hi);
                let (below1, above1) = (v1[1].hi < p[1].lo, v1[1].lo > p[1].hi);
                if (below0 && below1) || (above0 && above1) {
                    continue; // even crossings of the band: none counts
                }
                if (below0 && above1) || (above0 && below1) {
                    if b[0].lo > p[0].hi {
                        crossings += 1; // odd crossings, all beyond the point: one on the ray
                        continue;
                    }
                    if b[0].hi < p[0].lo {
                        continue; // odd crossings, all behind the point
                    }
                }
                if depth >= MAX_HALVINGS {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                }
                let Some((lo, hi)) = bisect(t) else {
                    return Ok(RegionVerdict::Refused(ContactRefusal::RegionUnresolved));
                };
                stack.push((lo, depth + 1));
                stack.push((hi, depth + 1));
            }
        }
        Ok(if crossings % 2 == 1 {
            RegionVerdict::Inside
        } else {
            RegionVerdict::Outside
        })
    }

    /// `K(B) = x0 - C G(x0) + (I - C G'(B)) (B - x0)` for the system `G(u, v, t) =
    /// S(u, v) - F(t)` on the plane, with the contraction row-sum
    /// `q = max_i sum_j |(I - C G'(B))_ij|` (each term rounded up): the Krawczyk and Banach
    /// test of docs/definitions-and-certificates.md §4.3 — `K(B)` strictly inside `B` and
    /// `q < 1` prove `B` holds exactly one root. `None` where the curve has no derivative
    /// enclosure over the box's parameter.
    fn krawczyk_image(
        plane: &PlaneOver,
        curve: &EnclosedCurve<'_>,
        x0: [f64; 3],
        c: [[Ivl; 3]; 3],
        b: [Ivl; 3],
        g0: [Ivl; 3],
    ) -> Result<Option<([Ivl; 3], f64)>> {
        let Some(fp) = curve.derivs_over(b[2])? else {
            return Ok(None);
        };
        // G'(B)'s columns: S_u, S_v, -F'
        let jac = plane.jacobian();
        let cols = [
            jac[0],
            jac[1],
            [
                Ivl::from(0.0) - fp[0],
                Ivl::from(0.0) - fp[1],
                Ivl::from(0.0) - fp[2],
            ],
        ];
        // R = I - C G'(B) and q, its largest row sum
        let mut r = [[Ivl::from(0.0); 3]; 3];
        let mut q = 0.0_f64;
        for (row_index, (r_row, c_row)) in r.iter_mut().zip(c.iter()).enumerate() {
            let mut row_sum = 0.0_f64;
            for (col_index, (rij, col)) in r_row.iter_mut().zip(cols.iter()).enumerate() {
                let mut acc = Ivl::from(if row_index == col_index { 1.0 } else { 0.0 });
                for (ck, jk) in c_row.iter().zip(col.iter()) {
                    acc = acc - *ck * *jk;
                }
                *rij = acc;
                row_sum = round_up(row_sum + round_up(acc.mag()));
            }
            q = q.max(row_sum);
        }
        // K(B)
        let mut k = [Ivl::from(0.0); 3];
        for ((ki, &x0i), (c_row, r_row)) in k.iter_mut().zip(x0.iter()).zip(c.iter().zip(r.iter()))
        {
            let mut acc = Ivl::from(x0i);
            for (ck, gk) in c_row.iter().zip(g0.iter()) {
                acc = acc - *ck * *gk;
            }
            for (rij, (bj, bxj)) in r_row.iter().zip(b.iter().zip(x0.iter())) {
                acc = acc + *rij * (*bj - Ivl::from(*bxj));
            }
            *ki = acc;
        }
        Ok(Some((k, q)))
    }

    /// The remainder stage: every parameter of `rest` (the part of the edge's interval
    /// outside the near-contact window) is proved off the surface — a plane by its signed
    /// side `g(t) = n * (F(t) - origin)` (`g.lo > 0` or `g.hi < 0` per cell), a general
    /// surface by `surface_distance_lower_bound` over the face's UV box less the cell's ball
    /// radius (the whole-cell bound, not a distance at a representative point). A cell
    /// proving neither subdivides; anything unresolved or over budget refuses.
    pub(crate) fn remainder_clearance(
        kernel: &Kernel,
        curve: &EnclosedCurve<'_>,
        face: &MovedFace,
        plane: Option<&PlaneOver>,
        rest: &[(f64, f64)],
        budget: usize,
        boxes: &mut usize,
    ) -> Result<Option<ContactRefusal>> {
        for &(rlo, rhi) in rest {
            if rhi <= rlo {
                continue;
            }
            let mut stack = vec![(Ivl::new(rlo, rhi), 0_usize)];
            while let Some((t, depth)) = stack.pop() {
                kernel.check_cancelled(OP)?;
                *boxes += 1;
                if *boxes > budget {
                    return Ok(Some(ContactRefusal::RemainderBudget));
                }
                let Some(f) = curve.over_ext(t)? else {
                    return Ok(Some(ContactRefusal::NoCurveEnclosure));
                };
                let proved = if let Some(plane) = plane {
                    let g = plane.side(f);
                    g.lo > 0.0 || g.hi < 0.0
                } else {
                    let centre = [f[0].mid(), f[1].mid(), f[2].mid()];
                    let mut radius2 = Ivl::from(0.0);
                    for (x, c) in f.iter().zip(centre) {
                        radius2 = radius2 + square(*x - Ivl::from(c));
                    }
                    let radius = radius2.sqrt().hi;
                    if !radius.is_finite() {
                        return Ok(Some(ContactRefusal::RemainderUnresolved));
                    }
                    let lower = match surface_distance_lower_bound(
                        kernel,
                        &face.surface,
                        face.uv_box.as_ref(),
                        Point3::new(centre[0], centre[1], centre[2]),
                        radius,
                    ) {
                        Ok(lower) => lower,
                        Err(e @ SolidError::Cancelled { .. }) => return Err(e),
                        Err(_) => return Ok(Some(ContactRefusal::SurfaceUncertified)),
                    };
                    (Ivl::from(lower) - Ivl::from(radius)).lo > 0.0
                };
                if proved {
                    continue;
                }
                if depth >= MAX_HALVINGS {
                    return Ok(Some(ContactRefusal::RemainderUnresolved));
                }
                let Some((lo, hi)) = bisect(t) else {
                    return Ok(Some(ContactRefusal::RemainderUnresolved));
                };
                stack.push((lo, depth + 1));
                stack.push((hi, depth + 1));
            }
        }
        Ok(None)
    }

    /// One endpoint-contact certificate's request (issue #13622): the edge's curve over
    /// the interval to certify, its contact on the moved face (the parameter and the
    /// intended point), the face with its trimmed region and the summed tolerance bound
    /// ([`all_clear`](super::all_clear)'s). It bundles the parameter clump
    /// [`separated_ending_on_face`] and the reconstruction's `contact_proof` share.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct ContactRequest<'a> {
        /// The edge's curve over `interval`.
        pub(crate) curve: &'a Curve,
        /// The interval to certify (the rebuilt edge's retained part).
        pub(crate) interval: &'a Extent1d,
        /// The contact's parameter on the curve.
        pub(crate) contact_t: f64,
        /// The contact's intended point.
        pub(crate) contact_point: Point3,
        /// The moved face the contact is on (`face.uv_box` bounds the remainder's tests).
        pub(crate) face: &'a MovedFace,
        /// The face's trimmed region (`region`'s loops' UV trim curves).
        pub(crate) region: &'a TrimRegion,
        /// The summed tolerance bound, as [`all_clear`](super::all_clear) computes it.
        /// It is not summed with `face.tolerance` here.
        pub(crate) tolerance: f64,
    }

    /// The ENDPOINT-CONTACT certificate (issue #13622): a rebuilt edge may legitimately END
    /// ON a moved face, which [`separated`](super::separated) refuses. `Proved` iff the edge's contact at
    /// the request's `contact_t` (the point `contact_point`) is proved to be its only contact with the
    /// face's part over `face.uv_box`:
    ///
    /// 1. INCIDENCE: `|F(contact_t) - contact_point| <= tolerance`, rounded interval
    ///    evaluation.
    /// 2. REGION MEMBERSHIP: the contact's UV is certified in-or-on the face's trimmed
    ///    region (`region`'s loops' UV trim curves): proximity to a loop vertex or a trim
    ///    curve within `tolerance`, else crossing parity (`Outside` where the contact is
    ///    certified off the region).
    /// 3. ISOLATION: the system `S(u, v) - F(t) = 0` has exactly one solution over the
    ///    loops' certified enclosures grown `LOOP_GROWTH` times `[contact_t - window,
    ///    contact_t + window]`, proved by Krawczyk and Banach around the contact (the root
    ///    identified as the contact) and by root-exclusion or subdivision of the window's
    ///    remaining slabs: a second contact is never excluded, so the budget or a
    ///    stagnating bisection refuses.
    /// 4. REMAINDER SAFETY: outside the window every cell of the edge proves no further
    ///    contact with the surface (`remainder_clearance`).
    ///
    /// Anything unproved refuses (`Unproved` names the certificate): a cap exceeded never
    /// accepts. `window` is the isolation window's half-width around `contact_t`; `budget`
    /// caps each stage's boxes (at most [`CONTACT_BUDGET`]).
    ///
    /// # Errors
    /// Cancellation is propagated; every other failure proves nothing (`Unproved`).
    pub(crate) fn separated_ending_on_face(
        kernel: &Kernel,
        request: ContactRequest<'_>,
        window: f64,
        budget: usize,
    ) -> Result<ContactProof> {
        let ContactRequest {
            curve,
            interval,
            contact_t,
            contact_point,
            face,
            region,
            tolerance,
        } = request;
        let budget = budget.min(CONTACT_BUDGET);
        let mut report = ContactReport {
            incidence: f64::INFINITY,
            isolation_boxes: 0,
            remainder_boxes: 0,
            region_boxes: 0,
            root_t: None,
        };
        // ---- the inputs and the guarded extractions
        let (t0, t1) = (interval.min(), interval.max());
        let inputs_ok = t0.is_finite()
            && t1.is_finite()
            && t0 < t1
            && contact_t.is_finite()
            && contact_t >= t0
            && contact_t <= t1
            && contact_point.x.is_finite()
            && contact_point.y.is_finite()
            && contact_point.z.is_finite()
            && tolerance.is_finite()
            && tolerance >= 0.0
            && window.is_finite()
            && window > 0.0;
        if !inputs_ok {
            return Ok(ContactProof::Unproved(ContactRefusal::Inputs, report));
        }
        let Some(curve) = EnclosedCurve::of(curve) else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::NoCurveEnclosure,
                report,
            ));
        };
        let Some(plane) = PlaneOver::of(&face.surface) else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::SurfaceUncertified,
                report,
            ));
        };
        let mut segments: Vec<(EnclosedCurve<'_>, Extent1d)> = Vec::new();
        for loop_ in &region.loops {
            if loop_.segments.is_empty() {
                return Ok(ContactProof::Unproved(
                    ContactRefusal::RegionMissing,
                    report,
                ));
            }
            for segment in &loop_.segments {
                let Some(enc) = EnclosedCurve::of(&segment.curve) else {
                    return Ok(ContactProof::Unproved(
                        ContactRefusal::NoCurveEnclosure,
                        report,
                    ));
                };
                let (slo, shi) = (segment.interval.min(), segment.interval.max());
                if !(slo.is_finite() && shi.is_finite() && slo < shi) {
                    return Ok(ContactProof::Unproved(ContactRefusal::Inputs, report));
                }
                segments.push((enc, segment.interval));
            }
        }
        if segments.is_empty() {
            return Ok(ContactProof::Unproved(
                ContactRefusal::RegionMissing,
                report,
            ));
        }
        // ---- 1. INCIDENCE
        kernel.check_cancelled(OP)?;
        let Some(f_a) = curve.over_ext(Ivl::from(contact_t))? else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::NoCurveEnclosure,
                report,
            ));
        };
        let Some(incidence) = point_distance_hi(f_a, contact_point) else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::NoCurveEnclosure,
                report,
            ));
        };
        report.incidence = incidence;
        if incidence > tolerance {
            return Ok(ContactProof::Unproved(ContactRefusal::Incidence, report));
        }
        // ---- 2. REGION MEMBERSHIP
        kernel.check_cancelled(OP)?;
        let p = plane.uv_of(contact_point);
        if p.iter()
            .any(|x| !(x.lo.is_finite() && x.hi.is_finite() && x.lo <= x.hi))
        {
            return Ok(ContactProof::Unproved(
                ContactRefusal::SurfaceUncertified,
                report,
            ));
        }
        let verdict = region_membership(
            kernel,
            &segments,
            &plane,
            p,
            tolerance,
            budget,
            &mut report.region_boxes,
        )?;
        match verdict {
            RegionVerdict::Inside | RegionVerdict::Boundary => {}
            RegionVerdict::Outside => return Ok(ContactProof::Outside(report)),
            RegionVerdict::Refused(reason) => {
                return Ok(ContactProof::Unproved(reason, report));
            }
        }
        // ---- 3. ISOLATION: the grown loop enclosures, the contact's UV strictly interior
        let mut loop_box = [Ivl::new(f64::INFINITY, f64::NEG_INFINITY); 2];
        for (segment, ivl) in &segments {
            let Some(b) = segment.over_ext(Ivl::new(ivl.min(), ivl.max()))? else {
                return Ok(ContactProof::Unproved(
                    ContactRefusal::NoCurveEnclosure,
                    report,
                ));
            };
            loop_box[0] = loop_box[0].hull(b[0]);
            loop_box[1] = loop_box[1].hull(b[1]);
        }
        let grow = |x: Ivl| {
            let g = LOOP_GROWTH * (x.hi - x.lo);
            (x.lo - g, x.hi + g)
        };
        let (ulo, uhi) = grow(loop_box[0]);
        let (vlo, vhi) = grow(loop_box[1]);
        if !(p[0].lo > ulo && p[0].hi < uhi && p[1].lo > vlo && p[1].hi < vhi) {
            return Ok(ContactProof::Unproved(
                ContactRefusal::IsolationGeometry,
                report,
            ));
        }
        let (wlo, whi) = (contact_t - window, contact_t + window);
        let g_over = |u: Ivl, v: Ivl, t: Ivl| -> Result<Option<[Ivl; 3]>> {
            let Some(f) = curve.over_ext(t)? else {
                return Ok(None);
            };
            let s = plane.s_over(u, v);
            Ok(Some([s[0] - f[0], s[1] - f[1], s[2] - f[2]]))
        };
        let x0 = [p[0].mid(), p[1].mid(), contact_t];
        let Some(g0) = g_over(Ivl::from(x0[0]), Ivl::from(x0[1]), Ivl::from(x0[2]))? else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::NoCurveEnclosure,
                report,
            ));
        };
        let Some(fprime) = curve.derivs_over(Ivl::from(contact_t))? else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::NoCurveEnclosure,
                report,
            ));
        };
        let jac = plane.jacobian();
        let j0 = [
            [jac[0][0], jac[1][0], Ivl::from(0.0) - fprime[0]],
            [jac[0][1], jac[1][1], Ivl::from(0.0) - fprime[1]],
            [jac[0][2], jac[1][2], Ivl::from(0.0) - fprime[2]],
        ];
        let Some(c) = inverse3(j0) else {
            return Ok(ContactProof::Unproved(
                ContactRefusal::SingularJacobian,
                report,
            ));
        };
        // the contact box around x0: half-widths 0.4 of x0's margin to the window's faces
        let mut h = [
            0.4 * (x0[0] - ulo).min(uhi - x0[0]),
            0.4 * (x0[1] - vlo).min(vhi - x0[1]),
            0.4 * (x0[2] - wlo).min(whi - x0[2]),
        ];
        let sized = h.iter().all(|x| x.is_finite() && *x > 0.0);
        if !sized {
            return Ok(ContactProof::Unproved(
                ContactRefusal::IsolationGeometry,
                report,
            ));
        }
        report.isolation_boxes = 1;
        // Krawczyk and Banach on the contact box: exactly one root in it
        let mut k_contact = [Ivl::from(0.0); 3];
        let mut certified = false;
        for _ in 0..MAX_HALVINGS {
            let b = [
                Ivl::new(x0[0] - h[0], x0[0] + h[0]),
                Ivl::new(x0[1] - h[1], x0[1] + h[1]),
                Ivl::new(x0[2] - h[2], x0[2] + h[2]),
            ];
            let Some((k, q)) = krawczyk_image(&plane, &curve, x0, c, b, g0)? else {
                return Ok(ContactProof::Unproved(
                    ContactRefusal::NoCurveEnclosure,
                    report,
                ));
            };
            let inside = k
                .iter()
                .zip(b.iter())
                .all(|(ki, bi)| ki.lo > bi.lo && ki.hi < bi.hi);
            if q < 1.0 && inside {
                k_contact = k;
                certified = true;
                break;
            }
            h = h.map(|x| 0.5 * x);
        }
        if !certified {
            return Ok(ContactProof::Unproved(
                ContactRefusal::IsolationUnresolved,
                report,
            ));
        }
        // the window's rest: the six slabs around the contact box, each excluded (G(B)
        // misses 0 in a coordinate) or subdivided; a second contact is never excluded, so
        // the budget or a stagnating bisection refuses
        let slabs = [
            (ulo, x0[0] - h[0], vlo, vhi, wlo, whi),
            (x0[0] + h[0], uhi, vlo, vhi, wlo, whi),
            (x0[0] - h[0], x0[0] + h[0], vlo, x0[1] - h[1], wlo, whi),
            (x0[0] - h[0], x0[0] + h[0], x0[1] + h[1], vhi, wlo, whi),
            (
                x0[0] - h[0],
                x0[0] + h[0],
                x0[1] - h[1],
                x0[1] + h[1],
                wlo,
                x0[2] - h[2],
            ),
            (
                x0[0] - h[0],
                x0[0] + h[0],
                x0[1] - h[1],
                x0[1] + h[1],
                x0[2] + h[2],
                whi,
            ),
        ];
        for (a0, a1, b0, b1, s0, s1) in slabs {
            if !(a1 > a0 && b1 > b0 && s1 > s0) {
                continue;
            }
            let mut stack = vec![(a0, a1, b0, b1, s0, s1, 0_usize)];
            while let Some((sa0, sa1, sb0, sb1, ss0, ss1, depth)) = stack.pop() {
                kernel.check_cancelled(OP)?;
                report.isolation_boxes += 1;
                if report.isolation_boxes > budget {
                    return Ok(ContactProof::Unproved(
                        ContactRefusal::IsolationBudget,
                        report,
                    ));
                }
                let Some(g) = g_over(Ivl::new(sa0, sa1), Ivl::new(sb0, sb1), Ivl::new(ss0, ss1))?
                else {
                    return Ok(ContactProof::Unproved(
                        ContactRefusal::NoCurveEnclosure,
                        report,
                    ));
                };
                if g.iter().any(|x| x.lo > 0.0 || x.hi < 0.0) {
                    continue; // the cell holds no root
                }
                if depth >= MAX_HALVINGS {
                    return Ok(ContactProof::Unproved(
                        ContactRefusal::IsolationUnresolved,
                        report,
                    ));
                }
                let widths = [sa1 - sa0, sb1 - sb0, ss1 - ss0];
                let axis = widths
                    .iter()
                    .enumerate()
                    .max_by(|x, y| x.1.partial_cmp(y.1).unwrap_or(core::cmp::Ordering::Equal))
                    .map_or(2, |(i, _)| i);
                let at = |t: (f64, f64, f64, f64, f64, f64), axis: usize| match axis {
                    0 => (t.0, t.1),
                    1 => (t.2, t.3),
                    _ => (t.4, t.5),
                };
                let set = |t: &mut (f64, f64, f64, f64, f64, f64), v: (f64, f64)| match axis {
                    0 => (t.0, t.1) = v,
                    1 => (t.2, t.3) = v,
                    _ => (t.4, t.5) = v,
                };
                let whole = (sa0, sa1, sb0, sb1, ss0, ss1);
                let (p0, p1) = at(whole, axis);
                let mid = 0.5 * p0 + 0.5 * p1;
                if !(mid > p0 && mid < p1) {
                    return Ok(ContactProof::Unproved(
                        ContactRefusal::IsolationUnresolved,
                        report,
                    ));
                }
                let mut lo = whole;
                let mut hi = whole;
                set(&mut lo, (p0, mid));
                set(&mut hi, (mid, p1));
                stack.push((lo.0, lo.1, lo.2, lo.3, lo.4, lo.5, depth + 1));
                stack.push((hi.0, hi.1, hi.2, hi.3, hi.4, hi.5, depth + 1));
            }
        }
        // the enclosed root is the intended contact: its parameter holds contact_t and its
        // point is within the tolerance of contact_point
        let mut root = k_contact;
        for _ in 0..ROOT_REFINEMENTS {
            let Some((k, _)) = krawczyk_image(&plane, &curve, x0, c, root, g0)? else {
                return Ok(ContactProof::Unproved(
                    ContactRefusal::NoCurveEnclosure,
                    report,
                ));
            };
            let next = [
                meet(k[0], root[0]),
                meet(k[1], root[1]),
                meet(k[2], root[2]),
            ];
            if next.iter().any(|x| x.lo > x.hi) {
                return Ok(ContactProof::Unproved(
                    ContactRefusal::IsolationUnresolved,
                    report,
                ));
            }
            root = next;
        }
        report.root_t = Some((root[2].lo, root[2].hi));
        let holds_contact_t = root[2].lo <= contact_t && contact_t <= root[2].hi;
        let root_near = match curve.over_ext(root[2]) {
            Ok(Some(f)) => point_distance_hi(f, contact_point),
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            Ok(None) | Err(_) => None,
        };
        if !(holds_contact_t && root_near.is_some_and(|d| d <= tolerance)) {
            return Ok(ContactProof::Unproved(
                ContactRefusal::RootNotContact,
                report,
            ));
        }
        // ---- 4. REMAINDER SAFETY: the edge outside the window never touches the surface
        let mut rest = Vec::new();
        if t0 < contact_t - window {
            rest.push((t0, (contact_t - window).min(t1)));
        }
        if contact_t + window < t1 {
            rest.push(((contact_t + window).max(t0), t1));
        }
        if let Some(reason) = remainder_clearance(
            kernel,
            &curve,
            face,
            Some(&plane),
            &rest,
            budget,
            &mut report.remainder_boxes,
        )? {
            return Ok(ContactProof::Unproved(reason, report));
        }
        Ok(ContactProof::Proved(report))
    }
}

#[cfg(test)]
mod tests {
    use super::endpoint_contact::{
        remainder_clearance, separated_ending_on_face, ContactProof, ContactRefusal,
        ContactRequest, TrimLoop, TrimRegion, TrimSegment,
    };
    use super::*;
    use crate::math::{Point2, Vec3};
    use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::surface::offset_surface::OffsetSurface;
    use crate::surface::plane::Plane;

    fn floor() -> Surface {
        Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z))
    }

    #[test]
    fn a_long_clear_edge_is_proved_after_subdivision() {
        let k = Kernel::default();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        for curve in [
            Curve::from(Line::from_points(
                Point3::new(-10.0, 0.0, 1.0),
                Point3::new(10.0, 0.0, 1.0),
                3,
            )),
            Curve::from(
                BSplineCurve::create_line_segment(
                    3,
                    Point3::new(-10.0, 0.0, 1.0),
                    Point3::new(10.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
        ] {
            assert!(separated(&k, &curve, &interval, &floor(), None, 0.01).unwrap());
        }
    }

    #[test]
    fn a_crossing_contact_or_tolerance_band_is_never_certified_clear() {
        let k = Kernel::default();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        for (a, b, tolerance) in [
            (-1.0, 1.0, 0.01),
            (0.0, 0.0, 0.01),
            (0.005, 0.005, 0.01),
            (0.01, 0.01, 0.01),
        ] {
            let curve = Curve::from(Line::from_points(
                Point3::new(-1.0, 0.0, a),
                Point3::new(1.0, 0.0, b),
                3,
            ));
            assert!(!separated(&k, &curve, &interval, &floor(), None, tolerance).unwrap());
        }
    }

    #[test]
    fn rational_curve_interiors_and_the_trim_interval_are_bounded() {
        let k = Kernel::default();
        let curve = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &[
                    Point3::new(-1.0, 0.0, 1.0),
                    Point3::new(0.0, 0.0, -2.0),
                    Point3::new(1.0, 0.0, 1.0),
                ],
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&[1.0, 2.0, 1.0]),
                trim: None,
            })
            .unwrap(),
        );
        // The endpoints are above the plane but C(1/2).z = -1: it crosses.
        assert!(!separated(
            &k,
            &curve,
            &Extent1d::from_min_max(0.0, 1.0),
            &floor(),
            None,
            0.01,
        )
        .unwrap());
        assert!(separated(
            &k,
            &curve,
            &Extent1d::from_min_max(0.0, 0.03125),
            &floor(),
            None,
            0.01,
        )
        .unwrap());
    }

    /// An edge through the plane far outside the face's parameter box is clear of the face
    /// (the plane's net is far wider than any face on it), and an edge through the plane
    /// inside the box is not. Tested over the whole surface instead, the far edge would
    /// be reported crossed.
    #[test]
    fn only_the_trimmed_region_of_a_surface_is_tested() {
        let k = Kernel::default();
        let plane = floor();
        let vertical_through = |u: f64, v: f64| {
            let p = plane.evaluate_point(&k, Point2::new(u, v)).unwrap();
            Curve::from(Line::from_points(
                Point3::new(p.x, p.y, p.z - 1.0),
                Point3::new(p.x, p.y, p.z + 1.0),
                3,
            ))
        };
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let face = Extent2d::from_bounds(-1.0, -1.0, 1.0, 1.0);
        let inside = vertical_through(0.25, 0.5);
        let outside = vertical_through(500.0, 0.0);
        assert!(!separated(&k, &inside, &interval, &plane, Some(&face), 0.01).unwrap());
        assert!(separated(&k, &outside, &interval, &plane, Some(&face), 0.01).unwrap());
        assert!(!separated(&k, &outside, &interval, &plane, None, 0.01).unwrap());
    }

    #[test]
    fn unavailable_or_nonfinite_bounds_do_not_keep_an_edge() {
        let k = Kernel::default();
        let curve = Curve::from(Line::from_points(Vec3::ZERO, Vec3::X, 3));
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let implicit = Surface::from(OffsetSurface::new(&k, 2.0, floor(), true, 0).unwrap());
        assert!(!separated(&k, &curve, &interval, &implicit, None, 0.01).unwrap());
        assert!(!separated(&k, &curve, &interval, &floor(), None, f64::NAN).unwrap());
        assert!(!separated(
            &k,
            &curve,
            &Extent1d::from_min_max(-1.0, f64::INFINITY),
            &floor(),
            None,
            0.01,
        )
        .unwrap());
    }

    // ---- the named case of issue #13622 ------------------------------------------------

    /// The named case's moved face: the plane `z = x + y - 1`
    /// (`Plane::from_normal((0, 0, -1), (-1, -1, 1))`, the case's exact construction).
    fn named_face() -> Surface {
        Surface::from(Plane::from_normal(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(-1.0, -1.0, 1.0),
        ))
    }

    /// The signed side of a 3D point relative to the named plane (the spike's g).
    fn signed_side(surface: &Surface, p: Point3) -> f64 {
        match surface.kind() {
            SurfaceKind::Plane(plane) => plane.evaluate_normal().dot(p - plane.position().origin()),
            _ => panic!("the named case's face is planar"),
        }
    }

    /// The named plane's unit normal.
    fn normal_of(surface: &Surface) -> Vec3 {
        match surface.kind() {
            SurfaceKind::Plane(plane) => plane.evaluate_normal(),
            _ => panic!("the named case's face is planar"),
        }
    }

    /// The UV of a 3D point on the plane (the plane's own projection).
    fn uv_on(surface: &Surface, p: Point3) -> Point2 {
        match surface.kind() {
            SurfaceKind::Plane(plane) => plane.project_point_to_uv_domain(p),
            _ => panic!("the named case's face is planar"),
        }
    }

    /// The moved face's trimmed region: the pentagon the plane cuts from the box's walls
    /// above the bottom — A = (0, 1, 0), B = (1, 0, 0), C = (2, 0, 1), D = (2, 2, 3),
    /// E = (0, 2, 1) (the solid below it measures 25/6) — as five UV line trim segments.
    /// The plane's UV map is affine, so the pentagon's straight sides stay straight in
    /// UV, and A is the corner the rebuilt bottom edge ends on.
    fn named_region(surface: &Surface) -> TrimRegion {
        let corners = [
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
            Point3::new(2.0, 2.0, 3.0),
            Point3::new(0.0, 2.0, 1.0),
        ];
        let uv: Vec<Point2> = corners.iter().map(|&p| uv_on(surface, p)).collect();
        let mut segments = Vec::new();
        for i in 0..uv.len() {
            let a = uv[i];
            let b = uv[(i + 1) % uv.len()];
            segments.push(TrimSegment {
                curve: Curve::from(Line::from_points(
                    Point3::new(a.x, a.y, 0.0),
                    Point3::new(b.x, b.y, 0.0),
                    3,
                )),
                interval: Extent1d::from_min_max(0.0, 1.0),
            });
        }
        TrimRegion {
            loops: vec![TrimLoop { segments }],
        }
    }

    /// The moved face's trim box: the pentagon's UV tight box (the union its loops' trim
    /// boxes give, `loopuse::loop_uv_span`), measured on the repaired body as
    /// `[-0.0000 -4.2426] x [2.4495 -0.7071]`.
    fn named_trim_box(surface: &Surface) -> Extent2d {
        let corners = [
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
            Point3::new(2.0, 2.0, 3.0),
            Point3::new(0.0, 2.0, 1.0),
        ];
        let mut lo = (f64::INFINITY, f64::INFINITY);
        let mut hi = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for &p in &corners {
            let q = uv_on(surface, p);
            lo = (lo.0.min(q.x), lo.1.min(q.y));
            hi = (hi.0.max(q.x), hi.1.max(q.y));
        }
        Extent2d::from_bounds(lo.0, lo.1, hi.0, hi.1)
    }

    /// The moved face as the certificate takes it: the tilted plane over the real trim
    /// box (its tolerance 0: `tolerance` is the caller's sum).
    fn named_moved_face(surface: &Surface) -> MovedFace {
        MovedFace {
            surface: surface.clone(),
            uv_box: Some(named_trim_box(surface)),
            tolerance: 0.0,
        }
    }

    /// The rebuilt bottom edge: A = (0, 1, 0) to (0, 2, 0) (the repaired body's bottom
    /// `x = 0` edge; the body's copy runs over [1, 2] with A at 1 — the same geometry
    /// shifted), its contact at A on the moved face's corner.
    fn named_edge() -> Curve {
        Curve::from(Line::from_points(
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            3,
        ))
    }

    /// The named case's tolerances (measured on the repaired body: the corner vertex
    /// 1.00001 after the glue, the body 1e-5): the pre-tightened sum and the
    /// post-tightened one.
    const T_PRE: f64 = 1.00002e0;
    const T_POST: f64 = 2.0e-5;

    /// T1 (task A): the named case's acute contact is a proved endpoint contact at both
    /// tolerance regimes, and the gap-zone geometry (the nearest moved-face point
    /// Q(s) = (0, 1 + s/2, s/2) is s/sqrt(2) from the edge C(s) = (0, 1 + s, 0)) is
    /// covered by the certificate across the zone.
    #[test]
    fn a_rebuilt_edge_ending_on_a_moved_face_is_proved_endpoint_contact() {
        let k = Kernel::default();
        let surface = named_face();
        let region = named_region(&surface);
        let face = named_moved_face(&surface);
        let curve = named_edge();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let a = Point3::new(0.0, 1.0, 0.0);
        // the real trim box, cross-checked against the repaired body's measurement
        // (`[-0.0000 -4.2426] x [2.4495 -0.7071]`, the pentagon's UV corners on the
        // plane's axes: `[0, -3 sqrt(2)] x [sqrt(6), -1/sqrt(2)]`)
        let trim = face.uv_box.unwrap();
        let (lo, hi) = (trim.min(), trim.max());
        for (got, want) in [
            (lo.x, 0.0),
            (lo.y, -3.0 * std::f64::consts::SQRT_2),
            (hi.x, 6.0_f64.sqrt()),
            (hi.y, -std::f64::consts::FRAC_1_SQRT_2),
        ] {
            assert!((got - want).abs() <= 1.0e-3, "{got} vs {want}");
        }
        // the edge ends on the contact
        assert_eq!(curve.evaluate_point(&k, 0.0).unwrap(), a);
        // the gap-zone geometry
        for s in [0.1_f64, 0.5, 1.0] {
            let c = curve.evaluate_point(&k, s).unwrap();
            assert_eq!(c, Point3::new(0.0, 1.0 + s, 0.0));
            let q = Point3::new(0.0, 1.0 + s / 2.0, s / 2.0);
            let d = c.distance(q);
            assert!((d - s / 2.0_f64.sqrt()).abs() <= 1.0e-9, "{d} vs {s}");
        }
        // the certificate proves the contact at both regimes (the window covers the
        // whole edge, so the gap zone is covered with it)
        for (name, t) in [("pre-tightened", T_PRE), ("post-tightened", T_POST)] {
            let proof = separated_ending_on_face(
                &k,
                ContactRequest {
                    curve: &curve,
                    interval: &interval,
                    contact_t: 0.0,
                    contact_point: a,
                    face: &face,
                    region: &region,
                    tolerance: t,
                },
                1.0,
                4096,
            )
            .unwrap();
            let ContactProof::Proved(report) = &proof else {
                panic!("{name}: {proof:?}");
            };
            assert!(report.incidence <= t, "{name}: {report:?}");
        }
    }

    /// T2 (task A): the same edge at A (same value and tangent) bent to cross the face
    /// region again at s = 1/2 — `F(s) = (0, 1 + s, 2 s^2)`, signed side
    /// `g(s) = (2 s^2 - s)/sqrt(3)`. The coarse window refuses on the isolation side (a
    /// second contact is never excluded: the budget or a stagnating bisection refuses),
    /// a narrow window isolates A and refuses on the remainder side.
    #[test]
    fn a_second_crossing_inside_the_contact_window_is_not_proved() {
        let k = Kernel::default();
        let surface = named_face();
        let region = named_region(&surface);
        let face = named_moved_face(&surface);
        let bent = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &[
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 1.5, 0.0),
                    Point3::new(0.0, 2.0, 2.0),
                ],
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        );
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let a = Point3::new(0.0, 1.0, 0.0);
        // the constructed geometry: F(0) = A with F'(0) = (0, 1, 0) as the real edge,
        // and a second contact at s = 1/2
        let g = |s: f64| signed_side(&surface, bent.evaluate_point(&k, s).unwrap());
        assert!(g(0.0).abs() < 1.0e-12);
        assert!(g(0.5).abs() < 1.0e-12, "{}", g(0.5));
        assert!(g(0.25) < 0.0, "{}", g(0.25));
        // (1) the coarse window holds the second crossing: it cannot be excluded, so the
        // isolation refuses on its side
        let coarse = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &bent,
                interval: &interval,
                contact_t: 0.0,
                contact_point: a,
                face: &face,
                region: &region,
                tolerance: T_POST,
            },
            1.0,
            4096,
        )
        .unwrap();
        println!("T2 coarse window: {coarse:?}");
        assert!(
            matches!(
                &coarse,
                ContactProof::Unproved(
                    ContactRefusal::IsolationBudget | ContactRefusal::IsolationUnresolved,
                    _
                )
            ),
            "{coarse:?}"
        );
        // (2) a narrow window isolates the contact and the remainder carries the
        // crossing: refused on the remainder side
        let narrow = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &bent,
                interval: &interval,
                contact_t: 0.0,
                contact_point: a,
                face: &face,
                region: &region,
                tolerance: T_POST,
            },
            0.1,
            4096,
        )
        .unwrap();
        println!("T2 narrow window: {narrow:?}");
        assert!(
            matches!(
                &narrow,
                ContactProof::Unproved(
                    ContactRefusal::RemainderBudget | ContactRefusal::RemainderUnresolved,
                    _
                )
            ),
            "{narrow:?}"
        );
    }

    /// T3 (task A): contact at A plus a tangential touch of the face away from A —
    /// `F(s) = (0, 1 + s, 4 s^3 - 4 s^2 + 2 s)`, whose signed side `g(s) = s (2 s - 1)^2`
    /// touches 0 at s = 1/2. The touch is a root of the signed side, so no cell over it
    /// is excluded and the remainder refuses (never accepting on a cap).
    #[test]
    fn a_tangential_touch_away_from_the_contact_is_not_proved() {
        let k = Kernel::default();
        let surface = named_face();
        let region = named_region(&surface);
        let face = named_moved_face(&surface);
        let touch = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 3,
                control_points: &[
                    Point3::new(0.0, 1.0, 0.0),
                    Point3::new(0.0, 4.0 / 3.0, 2.0 / 3.0),
                    Point3::new(0.0, 5.0 / 3.0, 0.0),
                    Point3::new(0.0, 2.0, 2.0),
                ],
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[4, 4],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: None,
                trim: None,
            })
            .unwrap(),
        );
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let a = Point3::new(0.0, 1.0, 0.0);
        let g = |s: f64| signed_side(&surface, touch.evaluate_point(&k, s).unwrap());
        // contact at 0, a tangential touch at 1/2 (both asserted)
        assert!(g(0.0).abs() < 1.0e-12);
        assert!(g(0.5).abs() < 1.0e-12, "{}", g(0.5));
        assert!(g(0.4) > 0.0 && g(0.6) > 0.0, "{} {}", g(0.4), g(0.6));
        let proof = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &touch,
                interval: &interval,
                contact_t: 0.0,
                contact_point: a,
                face: &face,
                region: &region,
                tolerance: T_POST,
            },
            0.1,
            4096,
        )
        .unwrap();
        println!("T3 touch: {proof:?}");
        assert!(
            matches!(
                &proof,
                ContactProof::Unproved(
                    ContactRefusal::RemainderBudget | ContactRefusal::RemainderUnresolved,
                    _
                )
            ),
            "{proof:?}"
        );
    }

    /// T4 (task A): edges leaving A at every angle to the tilted plane, shallow to steep
    /// (the named direction (0, 1, 0) among them), are all proved endpoint contacts at
    /// both tolerance regimes — across the gap zone separation-by-T alone cannot cover
    /// (the measured `s_sep`, the smallest s from which the strict predicate proves
    /// [s, 1] separated, is 1.507e-3 at 5 degrees, 1.241e-4 at the named angle and
    /// 2.007e-5 at 89 degrees: every one of them past the tolerance).
    #[test]
    fn endpoint_contact_is_proved_from_shallow_to_steep_angles() {
        let k = Kernel::default();
        let surface = named_face();
        let region = named_region(&surface);
        let face = named_moved_face(&surface);
        let normal = normal_of(&surface);
        let a = Point3::new(0.0, 1.0, 0.0);
        let named = Vec3::new(0.0, 1.0, 0.0);
        let sin_named = named.dot(normal);
        let in_plane = named - normal * sin_named;
        let t_hat = in_plane / in_plane.length();
        let dir_at = |deg: Option<f64>| match deg {
            Some(d) => {
                let r = d.to_radians();
                t_hat * r.cos() + normal * r.sin()
            }
            None => named,
        };
        let interval = Extent1d::from_min_max(0.0, 1.0);
        for (label, deg) in [
            ("5deg", Some(5.0_f64)),
            ("15deg", Some(15.0)),
            ("25deg", Some(25.0)),
            ("named", None),
            ("50deg", Some(50.0)),
            ("70deg", Some(70.0)),
            ("89deg", Some(89.0)),
        ] {
            let curve = Curve::from(Line::from_points(a, a + dir_at(deg), 3));
            for t in [T_PRE, T_POST] {
                let proof = separated_ending_on_face(
                    &k,
                    ContactRequest {
                        curve: &curve,
                        interval: &interval,
                        contact_t: 0.0,
                        contact_point: a,
                        face: &face,
                        region: &region,
                        tolerance: t,
                    },
                    0.25,
                    4096,
                )
                .unwrap();
                assert!(
                    matches!(proof, ContactProof::Proved(_)),
                    "{label} {t}: {proof:?}"
                );
            }
            // separation-by-T alone fails on the whole edge (it touches at A) ...
            let whole = all_clear(
                &k,
                &[KeptEdge {
                    curve: curve.clone(),
                    interval,
                    tolerance: T_POST,
                }],
                &[MovedFace {
                    surface: surface.clone(),
                    uv_box: face.uv_box,
                    tolerance: 0.0,
                }],
            )
            .unwrap();
            assert!(!whole, "{label}: the touching edge was proved separated");
        }
        // the gap-zone widths, measured on the strict predicate by bisection
        for (label, deg, measured) in [
            ("5deg", Some(5.0_f64), 1.507e-3),
            ("named", None, 1.241e-4),
            ("89deg", Some(89.0), 2.007e-5),
        ] {
            let curve = Curve::from(Line::from_points(a, a + dir_at(deg), 3));
            let separated_from = |lo: f64| {
                separated(
                    &k,
                    &curve,
                    &Extent1d::from_min_max(lo, 1.0),
                    &surface,
                    face.uv_box.as_ref(),
                    T_POST,
                )
                .unwrap()
            };
            let mut lo = 0.0_f64;
            let mut hi = 1.0 - 1.0e-9;
            let s_sep = if separated_from(hi) {
                for _ in 0..40 {
                    let mid = 0.5 * lo + 0.5 * hi;
                    if separated_from(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                hi
            } else {
                f64::INFINITY
            };
            println!("{label}: s_sep={s_sep:.6e} measured={measured:.6e}");
            assert!(
                (s_sep - measured).abs() <= 1.0e-2 * measured + 1.0e-9,
                "{label}: {s_sep} vs {measured}"
            );
            // the gap zone (T_POST, s_sep) is nonempty and the certificate covers it
            assert!(s_sep > T_POST, "{label}: {s_sep}");
        }
    }

    /// T5 (task A): the certificate's budget fails closed — with budget 0 nothing is
    /// proved (a cap exceeded never accepts), and a normal run stays within the caps
    /// (the named case measured 37 isolation boxes and 0 remainder boxes).
    #[test]
    fn the_certificate_budget_fails_closed() {
        let k = Kernel::default();
        let surface = named_face();
        let region = named_region(&surface);
        let face = named_moved_face(&surface);
        let curve = named_edge();
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let a = Point3::new(0.0, 1.0, 0.0);
        for (name, t) in [("pre-tightened", T_PRE), ("post-tightened", T_POST)] {
            let proof = separated_ending_on_face(
                &k,
                ContactRequest {
                    curve: &curve,
                    interval: &interval,
                    contact_t: 0.0,
                    contact_point: a,
                    face: &face,
                    region: &region,
                    tolerance: t,
                },
                1.0,
                4096,
            )
            .unwrap();
            let ContactProof::Proved(report) = &proof else {
                panic!("{name}: {proof:?}");
            };
            println!("{name} boxes: {report:?}");
            assert!(report.region_boxes <= 256, "{name}: {report:?}");
            assert!(report.isolation_boxes <= 256, "{name}: {report:?}");
            assert!(report.remainder_boxes <= 256, "{name}: {report:?}");
            assert_eq!(report.remainder_boxes, 0, "{name}: {report:?}");
        }
        // a starved budget refuses
        let starved = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &curve,
                interval: &interval,
                contact_t: 0.0,
                contact_point: a,
                face: &face,
                region: &region,
                tolerance: T_POST,
            },
            1.0,
            0,
        )
        .unwrap();
        assert!(
            matches!(
                &starved,
                ContactProof::Unproved(ContactRefusal::RegionBudget, _)
            ),
            "{starved:?}"
        );
    }

    /// The review's required negative: a UV point inside a boundary trim curve's
    /// enclosure box but away from the curve and outside the face is NOT on the region —
    /// the enclosure box is not the curve. The region is the triangle (0, 0), (1, 0),
    /// (0, 1) in UV and the point (0.9, 0.9) lies in the hypotenuse's box.
    #[test]
    fn a_point_in_a_boundary_curves_box_but_off_the_curve_is_not_on_the_region() {
        let k = Kernel::default();
        let surface = named_face();
        let uv_segment = |a: Point2, b: Point2| TrimSegment {
            curve: Curve::from(Line::from_points(
                Point3::new(a.x, a.y, 0.0),
                Point3::new(b.x, b.y, 0.0),
                3,
            )),
            interval: Extent1d::from_min_max(0.0, 1.0),
        };
        let region = TrimRegion {
            loops: vec![TrimLoop {
                segments: vec![
                    uv_segment(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)),
                    uv_segment(Point2::new(1.0, 0.0), Point2::new(0.0, 1.0)),
                    uv_segment(Point2::new(0.0, 1.0), Point2::new(0.0, 0.0)),
                ],
            }],
        };
        let suspect_uv = Point2::new(0.9, 0.9);
        let p = surface.evaluate_point(&k, suspect_uv).unwrap();
        // the review's precondition: the point is inside the hypotenuse's enclosure box
        let hypotenuse = &region.loops[0].segments[1];
        let box_ = EnclosedCurve::of(&hypotenuse.curve)
            .unwrap()
            .over(Ivl::new(0.0, 1.0))
            .unwrap()
            .unwrap();
        assert!(
            box_[0].lo <= suspect_uv.x
                && suspect_uv.x <= box_[0].hi
                && box_[1].lo <= suspect_uv.y
                && suspect_uv.y <= box_[1].hi,
            "{box_:?}"
        );
        // the edge ends on the surface at the suspect point
        let curve = Curve::from(Line::from_points(p, Point3::new(p.x, p.y, p.z + 1.0), 3));
        let proof = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &curve,
                interval: &Extent1d::from_min_max(0.0, 1.0),
                contact_t: 0.0,
                contact_point: p,
                face: &named_moved_face(&surface),
                region: &region,
                tolerance: T_POST,
            },
            0.25,
            4096,
        )
        .unwrap();
        assert!(matches!(proof, ContactProof::Outside(_)), "{proof:?}");
    }

    /// A point inside an inner loop's box (a hole) is not in the region: crossing
    /// parity over the outer and the inner loop rejects it. Synthetic UV loops (this
    /// module's fixtures build surfaces, not faces): the square [0, 2]^2 with the hole
    /// [0.5, 1.5]^2, the point (1, 1) inside the hole.
    #[test]
    fn a_point_in_a_holes_box_is_not_in_the_region() {
        let k = Kernel::default();
        let surface = named_face();
        let uv_segment = |a: Point2, b: Point2| TrimSegment {
            curve: Curve::from(Line::from_points(
                Point3::new(a.x, a.y, 0.0),
                Point3::new(b.x, b.y, 0.0),
                3,
            )),
            interval: Extent1d::from_min_max(0.0, 1.0),
        };
        let square = |lo: f64, hi: f64| {
            let c = [
                Point2::new(lo, lo),
                Point2::new(hi, lo),
                Point2::new(hi, hi),
                Point2::new(lo, hi),
            ];
            TrimLoop {
                segments: (0..4).map(|i| uv_segment(c[i], c[(i + 1) % 4])).collect(),
            }
        };
        let region = TrimRegion {
            loops: vec![square(0.0, 2.0), square(0.5, 1.5)],
        };
        let p_uv = Point2::new(1.0, 1.0);
        let p = surface.evaluate_point(&k, p_uv).unwrap();
        // the point is inside the hole's (the inner loop's) enclosure box
        let inner = &region.loops[1];
        let mut hole_box: Option<Extent2d> = None;
        for segment in &inner.segments {
            let b = EnclosedCurve::of(&segment.curve)
                .unwrap()
                .over(Ivl::new(0.0, 1.0))
                .unwrap()
                .unwrap();
            let e = Extent2d::from_bounds(b[0].lo, b[1].lo, b[0].hi, b[1].hi);
            hole_box = Some(hole_box.map_or(e, |u| u.union(&e)));
        }
        let hole_box = hole_box.unwrap();
        assert!(
            hole_box.min().x <= p_uv.x
                && p_uv.x <= hole_box.max().x
                && hole_box.min().y <= p_uv.y
                && p_uv.y <= hole_box.max().y,
            "{hole_box:?}"
        );
        let curve = Curve::from(Line::from_points(p, Point3::new(p.x, p.y, p.z + 1.0), 3));
        let proof = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &curve,
                interval: &Extent1d::from_min_max(0.0, 1.0),
                contact_t: 0.0,
                contact_point: p,
                face: &named_moved_face(&surface),
                region: &region,
                tolerance: T_POST,
            },
            0.25,
            4096,
        )
        .unwrap();
        assert!(matches!(proof, ContactProof::Outside(_)), "{proof:?}");
    }

    /// The general-surface remainder (the certificate's (3) for a face that is not a
    /// plane): `surface_distance_lower_bound` over the face's UV box less the cell's
    /// ball radius, a whole-cell bound. Proved at stage level here: the entry point
    /// refuses a non-planar face earlier, at the contact's UV (the header's Deviations).
    #[test]
    fn a_general_surface_remainder_proves_whole_cells_or_refuses_them() {
        let k = Kernel::default();
        let face = MovedFace {
            surface: floor(),
            uv_box: Some(Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0)),
            tolerance: 0.0,
        };
        // the edge at height 1: every cell of its interval proves clear of the face
        let clear = Curve::from(Line::from_points(
            Point3::new(-1.0, 0.0, 1.0),
            Point3::new(1.0, 0.0, 1.0),
            3,
        ));
        let enc = EnclosedCurve::of(&clear).unwrap();
        let mut boxes = 0_usize;
        let refusal =
            remainder_clearance(&k, &enc, &face, None, &[(0.0, 1.0)], 4096, &mut boxes).unwrap();
        assert_eq!(refusal, None);
        assert!(boxes >= 1, "{boxes}");
        // an edge through the face: no cell over the crossing is proved clear
        let crossing = Curve::from(Line::from_points(
            Point3::new(-1.0, 0.0, -1.0),
            Point3::new(1.0, 0.0, 1.0),
            3,
        ));
        let enc = EnclosedCurve::of(&crossing).unwrap();
        let mut boxes = 0_usize;
        let refusal =
            remainder_clearance(&k, &enc, &face, None, &[(0.0, 1.0)], 4096, &mut boxes).unwrap();
        assert!(refusal.is_some(), "{boxes}");
    }

    /// A moved face without a certified interval form proves nothing (the header's
    /// Deviations: the contact's UV, the isolation's Jacobian and the UV-to-face
    /// distances are certified on a plane only).
    #[test]
    fn a_moved_face_without_a_certified_interval_form_is_not_proved() {
        let k = Kernel::default();
        let implicit = Surface::from(OffsetSurface::new(&k, 2.0, floor(), true, 0).unwrap());
        let face = MovedFace {
            surface: implicit,
            uv_box: None,
            tolerance: 0.0,
        };
        let surface = named_face();
        let region = named_region(&surface);
        let proof = separated_ending_on_face(
            &k,
            ContactRequest {
                curve: &named_edge(),
                interval: &Extent1d::from_min_max(0.0, 1.0),
                contact_t: 0.0,
                contact_point: Point3::new(0.0, 1.0, 0.0),
                face: &face,
                region: &region,
                tolerance: T_POST,
            },
            1.0,
            4096,
        )
        .unwrap();
        assert!(
            matches!(
                &proof,
                ContactProof::Unproved(ContactRefusal::SurfaceUncertified, _)
            ),
            "{proof:?}"
        );
    }
}
