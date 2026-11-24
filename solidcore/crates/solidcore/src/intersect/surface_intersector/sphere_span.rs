//! Not upstream (#16669): a certificate that a traced span's UV image on an analytic
//! sphere stays within a tolerance of the span's 3D curve.
//!
//! Upstream: none. `TestSpanAccuracy` (`source/SMLib/src/SmSurfaceIntersector.cpp`) samples
//! five interior parameters, and evaluates each UV point clamped to the domain. Near a
//! sphere's pole a span's UV Hermite cubic can leave the domain far behind (a span 0.37
//! long began at the seam crossing 3.3e-5 from the pole, with the exact chain-rule tangent
//! `u' ≈ δ/ρ²`, and its second control point was at u = −18.5). The clamped samples landed
//! near the pole and passed, but the trim made from the span missed its 3D curve by up to
//! 0.66.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`sphere_span_image`]      | none     |
//! | [`sphere_bezier_image`]    | none     |
//! | `net_box` (private)        | none     |
//! | [`sphere_span_sweep`], [`sphere_bezier_sweep`], [`SpanSweep`] (#17110) | none |
//! | [`fit_monotone_sweep`], [`SweepRefusal`] (#17110) | none |
//!
//! Deviations: the bisection of the span's parameter is bounded at [`MAX_DEPTH`] levels (at
//! most `2^MAX_DEPTH − 1` bisections, so at most 511 piece bounds and 257 point values per
//! span, an explicit queue, no recursion). A span the bisection does not certify by then is
//! not certified, and the trace shortens its step. That fixed budget (at most 511 piece
//! bounds, 257 point values and one pass over the net's double knots per span) is why the
//! certificate does not poll for cancellation (`Kernel::check_cancelled`); its callers do,
//! once per trace step or try. The monotone-sweep fit (#17110) makes at most `spans + 1`
//! passes over a flushed curve's spans and certifies each changed span: its work grows
//! with the square of the trace's point count, so it polls once per pass and once per
//! certified span. Its longitude test takes the sphere's frame axes as exact
//! (point intervals); the net is built from that frame, and a span the test decides by a
//! margin below the frame's rounding is one passing within rounding of the axis (ARGUED).

use std::collections::VecDeque;

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{ders_basis, Nurbs};
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::surface::surface::{Surface, SurfaceKind};

/// The deepest bisection of a span's parameter [`sphere_span_image`] makes: at most
/// `2^MAX_DEPTH` pieces.
pub const MAX_DEPTH: u32 = 8;

/// An enclosure of a point or vector of 3-space.
type Iv3 = [Ivl; 3];

/// What [`sphere_span_image`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpanImage {
    /// The surface is not an analytic sphere: no certificate is made.
    NotASphere,
    /// The image is within the returned bound of the 3D span, which is at most the tolerance.
    Certified(f64),
    /// No certificate within the tolerance, for the reason given.
    NotCertified(Refusal),
}

/// Why [`sphere_span_image`] gave no certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// At some parameter the image is farther from the 3D span than the tolerance: the
    /// enclosure of the distance there lies wholly over it.
    Off,
    /// A piece at the deepest bisection ([`MAX_DEPTH`]) did not certify.
    Depth,
    /// An enclosure failed: the net has no NURBS data, a weight's enclosure was not
    /// positive, or the net is not C¹ across a knot of full multiplicity (or is
    /// discontinuous across one of higher multiplicity).
    Enclosure,
}

/// Not upstream (#16669): whether the image on `surface` of the UV Hermite span
/// `uv = [uv0, duv0, uv1, duv1]` (from `uv0` with derivative `duv0` to `uv1` with `duv1`,
/// its parameter in `[0, 1]`) lies within `tol` of the 3D Hermite span `xyz` (the same
/// layout) at every parameter, on an analytic sphere. The UV points are in `x`, `y` (`z`
/// zero); the derivatives are those the trace scales by its step, as its span test does.
///
/// Both spans are cubic Bezier curves (their control points enclosed in rounded intervals
/// from the Hermite data). The image is the one `Surface::evaluate` gives: it evaluates a
/// sphere through its rational net (its `SmBSplineSurface` base) at the UV point clamped to
/// the net's knot range, so the image is `S(c(N(t)))`, `c` the clamp. Let
/// `D(t) = S(N(t)) − H(t)` with `S` the net's patches, the end patches continued past the
/// knots' range. On each piece `[a, b]` of the parameter, of length `w`, each component of
/// `D` lies in `hull(D(a), D(b)) + D''([a, b]) [−w²/8, 0]` (`piece_bound`), with
/// `D'' = S_uu N_u'² + 2 S_uv N_u' N_v' + S_vv N_v'² + S_u N_u'' + S_v N_v'' − H''`, the net's
/// derivatives enclosed over a UV box holding `N([a, b])` and its clamp (the hull of the
/// piece's control points) and the spans' derivatives over the piece. Where that box leaves
/// the knots' range by `o_u` and `o_v`, `S(c(N)) − S(N)` adds `S_u [−o_u, o_u] +
/// S_v [−o_v, o_v]` per component (the mean value theorem along the segment to the clamped
/// point, which stays in the box). The bound is the length of the enclosure's farthest
/// corner, rounded up. Pieces that do not certify are bisected, at most [`MAX_DEPTH`] times;
/// a parameter where the clamped image's distance lies wholly over `tol` ends the search
/// ([`Refusal::Off`]).
///
/// At a knot of full multiplicity (a double knot of the sphere's quadratic net) the
/// patches meet only C⁰ in the homogeneous net. `joints` bounds, once per span, the jump
/// `J` of the rational first derivative across each such knot: the one-sided homogeneous
/// derivatives' difference on each row, less the multiple `λ P_i` of the row's control
/// point on the knot, bounds it by `max (|r_xyz| + |r_w| M) / w_min` (`M` bounding the net's
/// points, `w_min` its least weight), and a net with `J` over `1e-9 (1 + M)` is refused
/// ([`Refusal::Enclosure`]). Where the piece's UV box holds such a knot, `D'` may jump by
/// `J |N'|` at each of at most three crossings, which adds `3 J |N'| w / 4` per component.
///
/// PROVEN: the piece bound is the Peano kernel form of linear interpolation's remainder,
/// `D(t) − L(t) = −∫ G(t, s) D''(s) ds` with `0 ≤ G` and `∫ G(t, s) ds = (t − a)(b − t) / 2
/// ≤ w² / 8`, for the part of `D` that is C¹ and piecewise C² (the pieces of `D''` hulled
/// over every patch the box meets), plus the interpolation error of each kink
/// `J (t − s)₊`, at most `J w / 4`; the convex hull property; outward rounding throughout.
/// The canonical sphere's `J` is of rounding size (the unit test
/// `the_canonical_net_is_c1_at_its_double_knots`).
///
/// [`SpanImage::NotASphere`] for any other surface kind; [`SpanImage::NotCertified`] with
/// the [`Refusal`].
#[must_use]
pub fn sphere_span_image(surface: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], tol: f64) -> SpanImage {
    certify(surface, bezier(uv), bezier(xyz), tol)
}

/// Not upstream (#16669): [`sphere_span_image`] for spans given as cubic Bezier control
/// points (`uv` and `xyz`, the parameter in `[0, 1]`), as a curve's control polygon holds
/// them after knot insertion.
#[must_use]
pub fn sphere_bezier_image(
    surface: &Surface,
    uv: [Vec3; 4],
    xyz: [Vec3; 4],
    tol: f64,
) -> SpanImage {
    certify(surface, uv.map(iv3), xyz.map(iv3), tol)
}

/// What [`sphere_span_sweep`] found: whether a span's UV sweep coordinate (the longitude
/// parameter of the sphere's net) is monotone where its 3D span's longitude is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanSweep {
    /// The surface is not an analytic sphere: no certificate is made.
    NotASphere,
    /// No constraint: an end of the UV span lies on a side of the net's domain across the
    /// sweep (a pole of the canonical net, where the sweep coordinate is a free choice and
    /// the longitude has no value), or the 3D span's longitude is not certified strictly
    /// monotone over the span (it may turn).
    Free,
    /// The 3D span's longitude is strictly monotone and the UV sweep coordinate is
    /// certified monotone over the whole span.
    Monotone,
    /// The 3D span's longitude is strictly monotone but the UV sweep coordinate is not
    /// certified monotone: its image may turn back in longitude where the 3D span does not
    /// (or the enclosures could not tell).
    TurnsBack,
}

/// The binomial weights `C(3, i) C(2, j) / C(5, i + j)` of the product of a cubic's and a
/// quadratic's Bernstein coefficients `i` and `j` in the quintic's coefficient `i + j`.
const C3: [f64; 4] = [1.0, 3.0, 3.0, 1.0];
/// See [`C3`].
const C2: [f64; 3] = [1.0, 2.0, 1.0];
/// See [`C3`].
const C5: [f64; 6] = [1.0, 5.0, 10.0, 10.0, 5.0, 1.0];

/// Not upstream (#17110): whether the UV Hermite span `uv` (layout as
/// [`sphere_span_image`]) is monotone in the sphere's sweep coordinate wherever the 3D
/// Hermite span `xyz` is monotone in longitude. Next to a pole a back-and-forth of the UV
/// span in its sweep coordinate moves its image by only `ρ Δu` (`ρ` the distance from the
/// axis), so [`sphere_span_image`] accepts a span whose trim zigzags in u while its 3D span
/// runs one way round the axis: the trim then has a 2D cusp (`SmBSplineCurve` test 13).
///
/// With `x`, `y` the coordinates of a point less the sphere's centre along its frame's X
/// and Y axes, the 3D span's longitude rate has the sign of `ω = x y' − y x'`, a quintic
/// whose Bernstein coefficients are the products of the cubic's and its derivative's
/// (`ω_k = Σ_{i+j=k} C(3,i) C(2,j) / C(5,k) (x_i y'_j − y_i x'_j)`), enclosed. When every
/// coefficient is positive (or every one negative) the longitude is strictly monotone over
/// the span (PROVEN: the Bernstein basis is non-negative and sums to one, so `ω` lies
/// between its least and greatest coefficient). The sweep coordinate is u (v when the
/// surface of revolution swaps its UV); the rational quadratic net is monotone in
/// longitude along it (each segment of the rational quadratic circle is), and the clamp to
/// the domain the evaluator applies is monotone, so the image is monotone in longitude
/// wherever the UV span is monotone in its sweep coordinate `c`. Its derivative is the
/// quadratic with Bernstein coefficients `b_i = 3 (c_{i+1} − c_i)`, which keeps one sign on
/// `[0, 1]` exactly when `b_0` and `b_2` have that sign (or vanish) and either `b_1` does
/// too or `b_1² ≤ b_0 b_2` (its extreme value `(b_0 b_2 − b_1²) / (b_0 − 2 b_1 + b_2)` at
/// the interior vertex when `b_1` has the other sign).
///
/// The direction is not imposed: a span passing the axis closer than the tolerance may be
/// trimmed on either side of the pole, and [`sphere_span_image`] bounds where the image
/// lies. Nor is a span with an end on a side of the net's v range (u when swapped)
/// constrained: on the canonical net that side is a pole, where the trace places a point
/// the curve passes within tolerance and gives it the sweep parameter of the curve's
/// direction, so the trim's sweep coordinate there is a choice, not a longitude, and the
/// 3D span's longitude swings through up to half a turn within the point's distance from
/// the axis. On a net whose side is not a pole, that span is only left unconstrained, as
/// before this certificate.
///
/// Every comparison is on outward-rounded enclosures and is written so that a NaN
/// enclosure certifies nothing: a NaN 3D span is [`SpanSweep::Free`], a NaN UV span on a
/// certified-monotone 3D span is [`SpanSweep::TurnsBack`].
#[must_use]
pub fn sphere_span_sweep(surface: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4]) -> SpanSweep {
    match SweepNet::of(surface) {
        Some(Ok(net)) => net.sweep(bezier(uv), bezier(xyz)),
        Some(Err(())) => SpanSweep::Free,
        None => SpanSweep::NotASphere,
    }
}

/// Not upstream (#17110): [`sphere_span_sweep`] for spans given as cubic Bezier control
/// points, as a curve's control polygon holds them after knot insertion.
#[must_use]
pub fn sphere_bezier_sweep(surface: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4]) -> SpanSweep {
    match SweepNet::of(surface) {
        Some(Ok(net)) => net.sweep(uv.map(iv3), xyz.map(iv3)),
        Some(Err(())) => SpanSweep::Free,
        None => SpanSweep::NotASphere,
    }
}

/// The point interval enclosure of a point.
fn iv3(v: Vec3) -> Iv3 {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

/// What [`sphere_span_sweep`] reads of a sphere: its sweep coordinate (`along`, 0 for u),
/// the other coordinate (`across`) and that one's knot range `[lo, hi]`, and its frame's
/// origin and X and Y axes, enclosed.
struct SweepNet {
    along: usize,
    across: usize,
    lo: f64,
    hi: f64,
    o: Iv3,
    ex: Iv3,
    ey: Iv3,
}

impl SweepNet {
    /// The sphere's data; `None` for a surface that is not an analytic sphere,
    /// `Some(Err(()))` for one whose net has no NURBS data or knot range.
    fn of(surface: &Surface) -> Option<Result<Self, ()>> {
        let SurfaceKind::Sphere(sphere) = surface.kind() else {
            return None;
        };
        let revolution = sphere.base();
        let (along, across) = if revolution.swap_uv() { (1, 0) } else { (0, 1) };
        let Ok(nurbs) = Nurbs::of(revolution.base()) else {
            return Some(Err(()));
        };
        let range = if across == 1 {
            knot_range(&nurbs.v, nurbs.q, nurbs.nv)
        } else {
            knot_range(&nurbs.u, nurbs.p, nurbs.nu)
        };
        let Some((lo, hi)) = range else {
            return Some(Err(()));
        };
        let frame = revolution.position();
        Some(Ok(Self {
            along,
            across,
            lo,
            hi,
            o: iv3(frame.origin()),
            ex: iv3(frame.x_axis()),
            ey: iv3(frame.y_axis()),
        }))
    }

    /// The certificate of [`sphere_span_sweep`] for enclosed Bezier control points `n` (UV)
    /// and `h` (3D).
    fn sweep(&self, n: [Iv3; 4], h: [Iv3; 4]) -> SpanSweep {
        let (lo, hi) = (self.lo, self.hi);
        // an end on a side across the sweep (or a coordinate that is not there)
        let at_side = |p: Iv3| p.get(self.across).is_none_or(|c| c.lo <= lo || c.hi >= hi);
        let [n0, _, _, n3] = n;
        if at_side(n0) || at_side(n3) || self.rate(h).is_none() {
            return SpanSweep::Free;
        }
        let along = self.along;
        let c = n.map(|p| p.get(along).copied().unwrap_or(Ivl::whole()));
        // the derivative's Bernstein coefficients over 3 (the sign test is homogeneous)
        let b = [
            difference(c[1], c[0]),
            difference(c[2], c[1]),
            difference(c[3], c[2]),
        ];
        if keeps_either_sign(b) {
            SpanSweep::Monotone
        } else {
            SpanSweep::TurnsBack
        }
    }

    /// The sign of the 3D span `h`'s longitude rate when it is certified strictly one sign
    /// over the span (`Some(true)` rising, `Some(false)` falling), else `None`: the six
    /// enclosed Bernstein coefficients of `ω = x y' − y x'`.
    fn rate(&self, h: [Iv3; 4]) -> Option<bool> {
        let (o, ex, ey) = (self.o, self.ex, self.ey);
        let dot = |a: Iv3, b: Iv3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        // the plane coordinates of a control point less the centre
        let xy = h.map(|p| {
            let a = [p[0] - o[0], p[1] - o[1], p[2] - o[2]];
            (dot(a, ex), dot(a, ey))
        });
        let three = Ivl::from(3.0);
        let mut d = [(Ivl::from(0.0), Ivl::from(0.0)); 3];
        for (slot, w) in d.iter_mut().zip(xy.windows(2)) {
            if let [(x0, y0), (x1, y1)] = *w {
                *slot = (three * (x1 - x0), three * (y1 - y0));
            }
        }
        // the 3D span's longitude rate ω = x y' − y x', its quintic Bernstein coefficients
        let mut omega = [Ivl::from(0.0); 6];
        for (i, (&(x, y), &ci)) in xy.iter().zip(&C3).enumerate() {
            for (j, (&(dx, dy), &cj)) in d.iter().zip(&C2).enumerate() {
                let (Some(&ck), Some(slot)) = (C5.get(i + j), omega.get_mut(i + j)) else {
                    return None;
                };
                let weight = Ivl::from(ci) * Ivl::from(cj) / Ivl::from(ck);
                *slot = *slot + weight * (x * dy - y * dx);
            }
        }
        let rising = omega.iter().all(|w| w.lo > 0.0);
        let falling = omega.iter().all(|w| w.hi < 0.0);
        (rising || falling).then_some(rising)
    }
}

/// An enclosure of `a − b`: the point difference when both are points and their
/// floating-point difference is exact (Knuth's two-sum error is zero), so that equal
/// control points give exactly 0 and not an interval about it; otherwise the outward
/// rounded difference.
fn difference(a: Ivl, b: Ivl) -> Ivl {
    let points = a.lo == a.hi && b.lo == b.hi;
    if points {
        let (x, y) = (a.lo, -b.lo);
        let sum = x + y;
        let y_part = sum - x;
        let error = (x - (sum - y_part)) + (y - y_part);
        if error == 0.0 && sum.is_finite() {
            return Ivl::from(sum);
        }
    }
    a - b
}

/// Whether the quadratic with the enclosed Bernstein coefficients `b` is non-negative over
/// `[0, 1]`, certified: `b_0 ≥ 0`, `b_2 ≥ 0`, and `b_1 ≥ 0` or `b_1² ≤ b_0 b_2`. A NaN
/// enclosure fails every comparison.
fn keeps_sign([b0, b1, b2]: [Ivl; 3]) -> bool {
    let ends = b0.lo >= 0.0 && b2.lo >= 0.0;
    let middle = b1.lo >= 0.0 || (b1 * b1).hi <= (b0 * b2).lo;
    ends && middle
}

/// Whether the quadratic with the enclosed Bernstein coefficients `b` keeps one sign over
/// `[0, 1]` ([`keeps_sign`] of `b` or of `−b`). The negation is exact: an outward-rounded
/// `0 − x` would widen an exact 0 coefficient past 0 and refuse a falling sweep that
/// stalls at an end.
fn keeps_either_sign(b: [Ivl; 3]) -> bool {
    keeps_sign(b) || keeps_sign(b.map(|x| Ivl::new(-x.hi, -x.lo)))
}

/// The share of a span's sweep change `Δ` a handle of a refitted span keeps at most: a
/// little under `|Δ|`. With both handle offsets `a`, `b` in `[0, Δ]` the span's derivative
/// coefficients `(a, Δ − a − b, b)` pass the sign test (`(a + b − Δ)² ≤ a b` there, the
/// Fritsch–Carlson square), and the margin keeps the rounding of the control points from
/// pushing a handle past `Δ`; the refit is then certified by the sign test itself.
const HANDLE_SHARE: f64 = 1.0 - 1.0 / 1024.0;

/// What [`fit_monotone_sweep`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SweepFit {
    /// The count of spans refitted.
    pub refitted: usize,
    /// The largest certified distance of a changed span's image from its 3D span (each at
    /// most the tolerance; 0 when no span changed): the trim's deviation the fit adds.
    pub bound: f64,
}

/// The operation [`fit_monotone_sweep`] names when it is cancelled.
const OP_FIT: &str = "intersect::surface_intersector::sphere_span::fit_monotone_sweep";

/// Why [`fit_monotone_sweep`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepRefusal {
    /// A refitted span's image is not certified within the tolerance of its 3D span
    /// ([`sphere_bezier_image`]), for the reason given.
    Off(Refusal),
    /// A refitted span still turns back in the sweep coordinate: its rounded enclosures
    /// could not certify it.
    TurnsBack,
    /// A handle of a joint whose factor is 0 (a chain end included) falls on the joint
    /// point (no offset left across the sweep either): the trim would have a zero
    /// derivative there.
    Degenerate,
    /// The chains of control points do not hold whole cubic spans of the same count.
    Shape,
}

/// Not upstream (#17110): makes each span of a traced curve's UV trim on an analytic
/// sphere monotone in the sweep coordinate wherever its 3D span is monotone in longitude,
/// with a certificate (a joint where the trace's own points run back keeps that turn; see
/// below), and returns how many spans it refitted with the largest certified distance of
/// a changed span's image from its 3D span (both 0 on any other surface, or when no span
/// turns back).
///
/// `uv` and `xyz` are the control points of the UV curve and the 3D curve, chains of
/// cubic Bezier spans joined end to end (span `j` is `[3j, 3j + 3]`), as the trace flushes
/// them from its Hermite points: at each joint the two handles are the joint point ∓ the
/// trace's tangent times a third of each span's parameter length. Each span that
/// [`sphere_bezier_sweep`] finds [`SpanSweep::TurnsBack`] is refitted by shortening the
/// sweep component of its handles: the handle offsets of the sweep coordinate at each of
/// its joints are scaled by a factor `λ ∈ [0, 1]`, the same on both sides of the joint (so
/// the joint keeps the trace's tangent direction in the other coordinate and stays G1 in
/// the sweep coordinate wherever `λ > 0`), the largest that leaves the span's handle
/// offsets of the sign of its sweep change `Δ` and at most `HANDLE_SHARE |Δ|` long (0 for a
/// handle pointing against `Δ`). The span is then monotone in the sweep coordinate (see
/// [`HANDLE_SHARE`]; ARGUED there, and certified below). A neighbouring span whose
/// shortened handle makes it turn back is refitted in the next pass; a factor only ever
/// decreases, and a refitted span that turns back again is refused, so there are at most
/// `spans + 1` passes of `spans` spans each (bounded work; the caller polls for
/// cancellation per trace step).
///
/// Every span a factor changed must then be certified: its image within `tol` of its 3D
/// span ([`sphere_bezier_image`]) and not [`SpanSweep::TurnsBack`], and a joint whose
/// factor is 0 (a chain end included) must keep a handle offset across the sweep on each
/// side it has. Otherwise the fit
/// is refused, with the reason ([`SweepRefusal`]), and the caller refuses the whole
/// intersection. The 3D curve and the points (every `uv[3j]`) are not moved, so where
/// the points the trace placed themselves run back in the sweep coordinate (within the
/// trace's tolerance of the curve, as next to a pole, where a longitude error costs only
/// `ρ Δu`) the trim keeps that turn at the joint, with each span on either side
/// monotone: a factor of 0 there flattens the sweep handles into the joint. Refusing such
/// a joint would refuse traces whose every point and span is within tolerance (the
/// thickened pole caps of #1502, the trace into a pole of WR-550). A joint no span needed
/// refitting is not checked: its spans were each accepted as they are.
///
/// The fit polls `kernel`'s cancellation once per pass and once per certified span: the passes
/// are at most `spans + 1` scans of every span, and a certificate costs up to 511 piece
/// bounds, so the work grows with the square of the span count.
///
/// # Errors
///
/// [`SolidError::Cancelled`](crate::SolidError) from `kernel`'s cancellation; otherwise `Ok`
/// with the [`SweepFit`], or the [`SweepRefusal`].
pub fn fit_monotone_sweep(
    kernel: &Kernel,
    surface: &Surface,
    uv: &mut [Vec3],
    xyz: &[Vec3],
    tol: f64,
) -> crate::Result<Result<SweepFit, SweepRefusal>> {
    // another surface, or a sphere without a net to read: no span is constrained
    let Some(Ok(net)) = SweepNet::of(surface) else {
        return Ok(Ok(SweepFit::default()));
    };
    let along = net.along;
    let sweep = |q: [Vec3; 4], h: [Vec3; 4]| net.sweep(q.map(iv3), h.map(iv3));
    if uv.len() != xyz.len() || uv.len() < 4 || (uv.len() - 1) % 3 != 0 {
        return Ok(Err(SweepRefusal::Shape));
    }
    let spans = (uv.len() - 1) / 3;
    let span = |c: &[Vec3], j: usize| -> Option<[Vec3; 4]> {
        Some([
            *c.get(3 * j)?,
            *c.get(3 * j + 1)?,
            *c.get(3 * j + 2)?,
            *c.get(3 * j + 3)?,
        ])
    };
    let sweep_of = |p: Vec3| if along == 0 { p.x } else { p.y };
    let original = uv.to_vec();
    // the sweep offsets of each joint's handles before the fit: (into it, out of it)
    let offset = |k: usize| -> (f64, f64) {
        let q = original.get(3 * k).map_or(0.0, |&p| sweep_of(p));
        let into = k
            .checked_sub(1)
            .and_then(|_| original.get(3 * k - 1))
            .map_or(0.0, |&p| q - sweep_of(p));
        let out = original.get(3 * k + 1).map_or(0.0, |&p| sweep_of(p) - q);
        (into, out)
    };
    let mut lambda = vec![1.0_f64; spans + 1];
    let mut fitted = vec![false; spans];
    let apply = |uv: &mut [Vec3], lambda: &[f64]| {
        for (k, &l) in lambda.iter().enumerate() {
            let (into, out) = offset(k);
            let Some(q) = original.get(3 * k).map(|&p| sweep_of(p)) else {
                continue;
            };
            let mut set = |i: usize, value: f64| {
                if let Some(p) = uv.get_mut(i) {
                    if along == 0 {
                        p.x = value;
                    } else {
                        p.y = value;
                    }
                }
            };
            if k > 0 {
                set(3 * k - 1, q - l * into);
            }
            if k < spans {
                set(3 * k + 1, q + l * out);
            }
        }
    };
    for _ in 0..=spans {
        kernel.check_cancelled(OP_FIT)?;
        let mut changed = false;
        for j in 0..spans {
            let (Some(q), Some(h)) = (span(uv, j), span(xyz, j)) else {
                return Ok(Err(SweepRefusal::Shape));
            };
            if sweep(q, h) != SpanSweep::TurnsBack {
                continue;
            }
            if fitted.get(j) == Some(&true) {
                // a refitted span that still turns back: its rounding could not certify it
                return Ok(Err(SweepRefusal::TurnsBack));
            }
            let Some([o0, _, _, o3]) = span(&original, j) else {
                return Ok(Err(SweepRefusal::Shape));
            };
            let delta = sweep_of(o3) - sweep_of(o0);
            let room = HANDLE_SHARE * delta.abs();
            // the largest factor leaving a handle offset `o` of the sign of Δ, within room
            let limit = |o: f64| {
                if o == 0.0 {
                    1.0
                } else if o * delta > 0.0 && o.abs() > room {
                    room / o.abs()
                } else if o * delta > 0.0 {
                    1.0
                } else {
                    0.0
                }
            };
            let (_, out) = offset(j);
            let (into, _) = offset(j + 1);
            for (k, o) in [(j, out), (j + 1, into)] {
                if let Some(l) = lambda.get_mut(k) {
                    *l = l.min(limit(o));
                }
            }
            if let Some(f) = fitted.get_mut(j) {
                *f = true;
            }
            changed = true;
        }
        if !changed {
            break;
        }
        apply(uv, &lambda);
    }
    // the joints a factor collapsed (the chain's ends included): a handle offset across
    // the sweep kept on each side. A joint where the sweep turns is left as the points the
    // trace placed have it: each span is monotone, the points are on the curve within the
    // trace's tolerance, and no choice of handles moves them (see the function doc).
    let across_of = |p: Vec3| if along == 0 { p.y } else { p.x };
    for (k, &l) in lambda.iter().enumerate() {
        if l != 0.0 {
            continue;
        }
        let Some(q) = original.get(3 * k).map(|&p| across_of(p)) else {
            return Ok(Err(SweepRefusal::Shape));
        };
        let into = k.checked_sub(1).and_then(|i| original.get(3 * i + 2));
        let out = original.get(3 * k + 1).filter(|_| k < spans);
        if [into, out].iter().flatten().any(|&&p| across_of(p) == q) {
            return Ok(Err(SweepRefusal::Degenerate));
        }
    }
    // certify every span a factor changed
    let mut fit = SweepFit::default();
    for j in 0..spans {
        let touched = [j, j + 1]
            .iter()
            .any(|&k| lambda.get(k).is_some_and(|&l| l < 1.0));
        if !touched {
            continue;
        }
        kernel.check_cancelled(OP_FIT)?;
        let (Some(q), Some(h)) = (span(uv, j), span(xyz, j)) else {
            return Ok(Err(SweepRefusal::Shape));
        };
        match sphere_bezier_image(surface, q, h, tol) {
            SpanImage::Certified(bound) => fit.bound = fit.bound.max(bound),
            SpanImage::NotASphere => {}
            SpanImage::NotCertified(why) => return Ok(Err(SweepRefusal::Off(why))),
        }
        if sweep(q, h) == SpanSweep::TurnsBack {
            return Ok(Err(SweepRefusal::TurnsBack));
        }
        if fitted.get(j) == Some(&true) {
            fit.refitted += 1;
        }
    }
    Ok(Ok(fit))
}

/// The certificate of [`sphere_span_image`] for enclosed Bezier control points.
fn certify(surface: &Surface, n: [Iv3; 4], h: [Iv3; 4], tol: f64) -> SpanImage {
    let SurfaceKind::Sphere(sphere) = surface.kind() else {
        return SpanImage::NotASphere;
    };
    let Ok(nurbs) = Nurbs::of(sphere.base().base()) else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    let domain = (
        knot_range(&nurbs.u, nurbs.p, nurbs.nu),
        knot_range(&nurbs.v, nurbs.q, nurbs.nv),
    );
    let (Some(du), Some(dv)) = domain else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    // both sides moved by the 3D span's start, so the enclosures of the net's quotients
    // are not widened by the size of the coordinates
    let [cx, cy, cz] = h[0].map(Ivl::mid);
    let origin = [Ivl::from(cx), Ivl::from(cy), Ivl::from(cz)];
    let mut pw = Vec::new();
    for c in nurbs.pw.chunks_exact(4) {
        if let [x, y, z, w] = *c {
            let w = Ivl::from(w);
            pw.extend([
                Ivl::from(x) - origin[0] * w,
                Ivl::from(y) - origin[1] * w,
                Ivl::from(z) - origin[2] * w,
                w,
            ]);
        }
    }
    let (Some(ju), Some(jv)) = (joints(&nurbs, &pw, true), joints(&nurbs, &pw, false)) else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    let net = Shifted {
        nurbs,
        pw,
        du,
        dv,
        ju,
        jv,
    };
    let h = h.map(|q| [q[0] - origin[0], q[1] - origin[1], q[2] - origin[2]]);
    let curves = Curves {
        dn: derivative(&n),
        ddn: derivative(&derivative(&n)),
        ddh: derivative(&derivative(&h)),
        n: n.to_vec(),
        h: h.to_vec(),
    };
    let ends = (
        point_value(&net, &curves, 0.0),
        point_value(&net, &curves, 1.0),
    );
    let (Some(d0), Some(d1)) = ends else {
        return SpanImage::NotCertified(Refusal::Enclosure);
    };
    if off(d0.clamped, tol) || off(d1.clamped, tol) {
        return SpanImage::NotCertified(Refusal::Off);
    }
    let mut worst = 0.0_f64;
    // breadth first: a span off the curve is found at a coarse piece's middle before the
    // pieces of a level are refined
    let mut queue = VecDeque::from([(0.0_f64, d0.continued, 1.0_f64, d1.continued, 0_u32)]);
    while let Some((a, da, b, db, depth)) = queue.pop_front() {
        // a missing bound (an enclosure that failed) does not certify
        if let Some(bound) = piece_bound(&net, &curves, a, da, b, db) {
            if bound <= tol {
                worst = worst.max(bound);
                continue;
            }
        }
        if depth >= MAX_DEPTH {
            return SpanImage::NotCertified(Refusal::Depth);
        }
        let m = 0.5 * (a + b);
        let Some(dm) = point_value(&net, &curves, m) else {
            return SpanImage::NotCertified(Refusal::Enclosure);
        };
        if off(dm.clamped, tol) {
            return SpanImage::NotCertified(Refusal::Off);
        }
        queue.push_back((a, da, m, dm.continued, depth + 1));
        queue.push_back((m, dm.continued, b, db, depth + 1));
    }
    SpanImage::Certified(worst)
}

/// The range `[knots[deg], knots[count]]` of the knot vector `knots` (degree `deg`, `count`
/// control points): the net's natural domain in that direction, which the evaluator clamps
/// to.
fn knot_range(knots: &[f64], deg: usize, count: usize) -> Option<(f64, f64)> {
    Some((*knots.get(deg)?, *knots.get(count)?))
}

/// The enclosure `x` clamped to `[lo, hi]` (the clamp is monotone, so its ends are clamped).
fn clamp_ivl(x: Ivl, (lo, hi): (f64, f64)) -> Ivl {
    Ivl::new(x.lo.max(lo).min(hi), x.hi.max(lo).min(hi))
}

/// How far the enclosure `x` reaches past `[lo, hi]`, rounded up (0 within it).
fn overshoot(x: Ivl, (lo, hi): (f64, f64)) -> f64 {
    let below = (Ivl::from(lo) - Ivl::from(x.lo)).hi;
    let above = (Ivl::from(x.hi) - Ivl::from(hi)).hi;
    below.max(above).max(0.0)
}

/// The sphere's net moved by `−c`: its homogeneous control points `(x w − c_x w, …, w)`,
/// enclosed, its knot ranges `du`, `dv` (the natural domain), and its C⁰ joints in u and v.
struct Shifted {
    nurbs: Nurbs,
    pw: Vec<Ivl>,
    du: (f64, f64),
    dv: (f64, f64),
    ju: Vec<Joint>,
    jv: Vec<Joint>,
}

/// A knot `at` of full multiplicity, where the net's patches meet C⁰ in the homogeneous
/// net, and a bound `jump` on the jump of the rational first derivative across it.
#[derive(Clone, Copy, Debug)]
struct Joint {
    at: f64,
    jump: f64,
}

/// The joints of the net `nurbs` (homogeneous control points `pw`, enclosed) across u
/// (`across_u`) or v: for each interior knot of multiplicity equal to the degree, with the
/// control point `P_i` on it, the one-sided homogeneous derivatives `p (P_i − P_{i−1}) / h_l`
/// and `p (P_{i+1} − P_i) / h_r` on every row; their difference less `λ P_i` (`λ` from the
/// first row) leaves `r`, and the rational derivative's jump is at most
/// `max_rows (|r_xyz| + |r_w| M) / w_min`. `None` for a knot of higher multiplicity (the net
/// is discontinuous there), a weight's enclosure that is not positive, or a jump over
/// `1e-9 (1 + M)` (the net is not C¹ there).
fn joints(nurbs: &Nurbs, pw: &[Ivl], across_u: bool) -> Option<Vec<Joint>> {
    let (knots, deg, count, rows) = if across_u {
        (&nurbs.u, nurbs.p, nurbs.nu, nurbs.nv)
    } else {
        (&nurbs.v, nurbs.q, nurbs.nv, nurbs.nu)
    };
    let point = |along: usize, row: usize| -> Option<[Ivl; 4]> {
        let (i, j) = if across_u { (along, row) } else { (row, along) };
        let l = 4 * (i * nurbs.nv + j);
        Some([
            *pw.get(l)?,
            *pw.get(l + 1)?,
            *pw.get(l + 2)?,
            *pw.get(l + 3)?,
        ])
    };
    // M bounds |S| (the convex hull property, weights positive); w_min bounds the weight
    let mut big = Ivl::from(0.0);
    let mut w_min = f64::INFINITY;
    for c in pw.chunks_exact(4) {
        if let [x, y, z, w] = *c {
            if !w.is_positive() {
                return None;
            }
            w_min = w_min.min(w.lo);
            let r = ((x / w) * (x / w) + (y / w) * (y / w) + (z / w) * (z / w)).sqrt();
            big = big.hull(r);
        }
    }
    let m = big.hi;
    let p = Ivl::from(f64::from(u32::try_from(deg).ok()?));
    let (lo, hi) = (*knots.get(deg)?, *knots.get(count)?);
    let mut out = Vec::new();
    let mut f = deg + 1;
    while f < count {
        let k = *knots.get(f)?;
        let mut mult = 1;
        while knots.get(f + mult) == Some(&k) {
            mult += 1;
        }
        let interior = lo < k && k < hi;
        if interior && mult > deg {
            return None;
        }
        if interior && mult == deg {
            let i = f.checked_sub(1)?;
            let hl = Ivl::from(k) - Ivl::from(*knots.get(i)?);
            let hr = Ivl::from(*knots.get(f + deg)?) - Ivl::from(k);
            let mut lambda = None;
            let mut jump = 0.0_f64;
            for row in 0..rows {
                let (pm, pi, pp) = (
                    point(i.checked_sub(1)?, row)?,
                    point(i, row)?,
                    point(i + 1, row)?,
                );
                let d: Vec<Ivl> = pm
                    .iter()
                    .zip(&pi)
                    .zip(&pp)
                    .map(|((&a, &b), &c)| p * (c - b) / hr - p * (b - a) / hl)
                    .collect();
                let [_, _, _, piw] = pi;
                let l = *lambda.get_or_insert(d.get(3)?.mid() / piw.mid());
                let l = Ivl::from(l);
                let r: Vec<Ivl> = d.iter().zip(&pi).map(|(&a, &b)| a - l * b).collect();
                let (rx, ry, rz, rw) = (*r.first()?, *r.get(1)?, *r.get(2)?, *r.get(3)?);
                let rxyz = (rx * rx + ry * ry + rz * rz).sqrt();
                let bound = (rxyz + Ivl::from(rw.mag()) * Ivl::from(m)) / Ivl::from(w_min);
                jump = jump.max(bound.hi);
            }
            let c1 = jump <= 1e-9 * (1.0 + m);
            if !c1 {
                return None;
            }
            out.push(Joint { at: k, jump });
        }
        f += mult;
    }
    Some(out)
}

/// The kink term of a piece of length `w` whose UV box spans `bu` × `bv`, the UV span's
/// first derivative within `nu`, `nv` over it: `3 J |N'| w / 4` for each joint in the box.
fn kinks(net: &Shifted, bu: Ivl, bv: Ivl, nu: Ivl, nv: Ivl, w: Ivl) -> f64 {
    let three_quarters = Ivl::from(0.75);
    let mut sum = Ivl::from(0.0);
    for (joints, b, d) in [(&net.ju, bu, nu), (&net.jv, bv, nv)] {
        for j in joints {
            if b.lo <= j.at && j.at <= b.hi {
                sum = sum + three_quarters * Ivl::from(j.jump) * Ivl::from(d.mag()) * w;
            }
        }
    }
    sum.hi
}

/// The two spans' Bezier control points and those of the UV span's first and second
/// derivatives and the 3D span's second.
struct Curves {
    n: Vec<Iv3>,
    h: Vec<Iv3>,
    dn: Vec<Iv3>,
    ddn: Vec<Iv3>,
    ddh: Vec<Iv3>,
}

/// Whether the enclosure `d` of a difference lies wholly over `tol` in length.
fn off(d: Iv3, tol: f64) -> bool {
    let mut least = Ivl::from(0.0);
    for c in d {
        let low = Ivl::from(c.mig());
        least = least + low * low;
    }
    least.sqrt().lo > tol
}

/// Enclosures of the difference at a parameter: of the clamped image's,
/// `S(c(N(t))) − H(t)`, and of the continued net's, `D(t) = S(N(t)) − H(t)`.
#[derive(Clone, Copy)]
struct PointValue {
    clamped: Iv3,
    continued: Iv3,
}

/// The [`PointValue`] at `t`; the continued value is evaluated apart only where `N(t)`'s
/// enclosure leaves the knots' range.
fn point_value(net: &Shifted, c: &Curves, t: f64) -> Option<PointValue> {
    let nt = blossom(&c.n, &[t; 3])?;
    let ht = blossom(&c.h, &[t; 3])?;
    let minus = |s: Iv3| [s[0] - ht[0], s[1] - ht[1], s[2] - ht[2]];
    let (cu, cv) = (clamp_ivl(nt[0], net.du), clamp_ivl(nt[1], net.dv));
    let clamped = minus(net_box(net, cu, cv, 0)?.s);
    let inside = cu == nt[0] && cv == nt[1];
    let continued = if inside {
        clamped
    } else {
        minus(net_box(net, nt[0], nt[1], 0)?.s)
    };
    Some(PointValue { clamped, continued })
}

/// The cubic Bezier control points of the Hermite span `[p0, d0, p1, d1]` (from `p0` with
/// derivative `d0` to `p1` with `d1`, on `[0, 1]`), enclosed.
fn bezier([p0, d0, p1, d1]: [Vec3; 4]) -> [Iv3; 4] {
    let iv = |v: Vec3| [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)];
    let third = |p: Iv3, d: Iv3, sign: f64| {
        let three = Ivl::from(3.0);
        let s = Ivl::from(sign);
        [
            p[0] + s * d[0] / three,
            p[1] + s * d[1] / three,
            p[2] + s * d[2] / three,
        ]
    };
    let (q0, q3) = (iv(p0), iv(p1));
    [q0, third(q0, iv(d0), 1.0), third(q3, iv(d1), -1.0), q3]
}

/// The control points of a Bezier curve's derivative (one degree lower).
fn derivative(q: &[Iv3]) -> Vec<Iv3> {
    // (a cubic at most: the degree is exact)
    let degree = Ivl::from(f64::from(
        u8::try_from(q.len().saturating_sub(1)).unwrap_or(0),
    ));
    q.windows(2)
        .filter_map(|w| match w {
            [a, b] => Some([
                degree * (b[0] - a[0]),
                degree * (b[1] - a[1]),
                degree * (b[2] - a[2]),
            ]),
            _ => None,
        })
        .collect()
}

/// The blossom (polar form) of the Bezier curve `q` at the arguments `ts` (as many as its
/// degree), enclosed: de Casteljau's algorithm with argument `ts[l]` at level `l`. With
/// every argument `t` it is the curve's point at `t`.
fn blossom(q: &[Iv3], ts: &[f64]) -> Option<Iv3> {
    let mut work = q.to_vec();
    for level in 1..work.len() {
        let t = Ivl::from(*ts.get(level - 1)?);
        let s = Ivl::from(1.0) - t;
        for i in 0..work.len() - level {
            let (a, b) = (*work.get(i)?, *work.get(i + 1)?);
            let slot = work.get_mut(i)?;
            *slot = [
                s * a[0] + t * b[0],
                s * a[1] + t * b[1],
                s * a[2] + t * b[2],
            ];
        }
    }
    work.first().copied()
}

/// An enclosure of the Bezier curve `q` over `[a, b]`: the hull of the control points of
/// its piece there (the blossoms at `a` and `b` repeated), which hold the piece (the convex
/// hull property). The arguments are exact, so only rounding widens the enclosure, not
/// the size of the coordinates (de Casteljau's algorithm with an interval argument widens
/// it by their size).
fn piece_hull(q: &[Iv3], a: f64, b: f64) -> Option<Iv3> {
    let degree = q.len().checked_sub(1)?;
    let mut out: Option<Iv3> = None;
    for j in 0..=degree {
        let ts: Vec<f64> = (0..degree).map(|i| if i < j { b } else { a }).collect();
        let c = blossom(q, &ts)?;
        out = Some(match out {
            None => c,
            Some(o) => hull3(o, c),
        });
    }
    out
}

/// The bound of `|S(c(N(t))) − H(t)|` over `[a, b]` ([`sphere_span_image`]) from the
/// continued differences `da` and `db` at the ends, rounded up: per component
/// `hull(D(a), D(b)) + D''(B) [−w²/8, 0] + S_u(B) [−o_u, o_u] + S_v(B) [−o_v, o_v]`,
/// `w = b − a`, `B` the hull of the piece's UV box and its clamp, `o_u` and `o_v` how far the
/// box leaves the knots' range, plus the kink term of the joints in the box (`kinks`).
/// `None` where an enclosure failed.
fn piece_bound(net: &Shifted, c: &Curves, a: f64, da: Iv3, b: f64, db: Iv3) -> Option<f64> {
    let w = Ivl::from(b) - Ivl::from(a);
    let sag = Ivl::new(-(w * w * Ivl::from(0.125)).hi, 0.0);
    let nbox = piece_hull(&c.n, a, b)?;
    let dnbox = piece_hull(&c.dn, a, b)?;
    let ddnbox = piece_hull(&c.ddn, a, b)?;
    let ddhbox = piece_hull(&c.ddh, a, b)?;
    let (ou, ov) = (overshoot(nbox[0], net.du), overshoot(nbox[1], net.dv));
    let bu = nbox[0].hull(clamp_ivl(nbox[0], net.du));
    let bv = nbox[1].hull(clamp_ivl(nbox[1], net.dv));
    let over = net_box(net, bu, bv, 2)?;
    let two = Ivl::from(2.0);
    let (nu, nv) = (dnbox[0], dnbox[1]);
    let (spread_u, spread_v) = (Ivl::new(-ou, ou), Ivl::new(-ov, ov));
    let kink = kinks(net, bu, bv, nu, nv, w);
    let kink = Ivl::new(-kink, kink);
    let mut sum = Ivl::from(0.0);
    for i in 0..3 {
        let su = *over.su.get(i)?;
        let sv = *over.sv.get(i)?;
        let d2 = *over.suu.get(i)? * nu * nu
            + two * *over.suv.get(i)? * nu * nv
            + *over.svv.get(i)? * nv * nv
            + su * ddnbox[0]
            + sv * ddnbox[1]
            - *ddhbox.get(i)?;
        let clamp = su * spread_u + sv * spread_v;
        let e = Ivl::from((da.get(i)?.hull(*db.get(i)?) + d2 * sag + clamp + kink).mag());
        sum = sum + e * e;
    }
    let bound = sum.sqrt().hi;
    bound.is_finite().then_some(bound)
}

/// The spans `k` of the knot vector `knots` (degree `deg`, `count` control points) whose
/// pieces meet `x`, the end spans continued past the knots' range.
fn spans_meeting(knots: &[f64], deg: usize, count: usize, x: Ivl) -> Vec<usize> {
    let mut out = Vec::new();
    for k in deg..count {
        let (Some(&lo), Some(&hi)) = (knots.get(k), knots.get(k + 1)) else {
            continue;
        };
        let first = k == deg;
        let last = k + 1 == count;
        let meets = hi > lo && (first || x.hi >= lo) && (last || x.lo <= hi);
        if meets {
            out.push(k);
        }
    }
    out
}

/// Enclosures of the sphere net's point and its first and second derivatives.
#[derive(Clone, Copy)]
struct Ders {
    s: Iv3,
    su: Iv3,
    sv: Iv3,
    suu: Iv3,
    suv: Iv3,
    svv: Iv3,
}

impl Ders {
    /// The componentwise hull of two enclosures.
    fn hull(self, o: Self) -> Self {
        Self {
            s: hull3(self.s, o.s),
            su: hull3(self.su, o.su),
            sv: hull3(self.sv, o.sv),
            suu: hull3(self.suu, o.suu),
            suv: hull3(self.suv, o.suv),
            svv: hull3(self.svv, o.svv),
        }
    }
}

/// Enclosures of the sphere net's point and derivatives over the UV box `u` × `v`: the hull
/// over every polynomial patch the box meets, each evaluated over the box's part in it (the
/// end patches continued past the knots' range). `None` where a weight's enclosure is not
/// positive or no patch meets the box. Derivatives past `order` are not computed and must
/// not be read.
fn net_box(net: &Shifted, u: Ivl, v: Ivl, order: usize) -> Option<Ders> {
    let mut out: Option<Ders> = None;
    for ku in spans_meeting(&net.nurbs.u, net.nurbs.p, net.nurbs.nu, u) {
        let uu = clip(&net.nurbs.u, net.nurbs.p, net.nurbs.nu, ku, u)?;
        for kv in spans_meeting(&net.nurbs.v, net.nurbs.q, net.nurbs.nv, v) {
            let vv = clip(&net.nurbs.v, net.nurbs.q, net.nurbs.nv, kv, v)?;
            let patch = patch_box(net, ku, kv, uu, vv, order)?;
            out = Some(match out {
                None => patch,
                Some(o) => o.hull(patch),
            });
        }
    }
    out
}

/// The part of `x` in span `k` (all of it past the knots' range at an end span).
fn clip(knots: &[f64], deg: usize, count: usize, k: usize, x: Ivl) -> Option<Ivl> {
    let lo = if k == deg {
        x.lo
    } else {
        x.lo.max(*knots.get(k)?)
    };
    let hi = if k + 1 == count {
        x.hi
    } else {
        x.hi.min(*knots.get(k + 1)?)
    };
    let ordered = lo <= hi;
    ordered.then(|| Ivl::new(lo, hi))
}

/// The componentwise hull of two enclosures.
fn hull3(a: Iv3, b: Iv3) -> Iv3 {
    [a[0].hull(b[0]), a[1].hull(b[1]), a[2].hull(b[2])]
}

/// The point and derivatives of the net's patch `(ku, kv)` over `u` × `v`, enclosed: the
/// homogeneous sums `A` (weight `w`) and the quotient rule, `S = A / w`,
/// `S_u = (A_u − w_u S) / w`, `S_uu = (A_uu − 2 w_u S_u − w_uu S) / w` and
/// `S_uv = (A_uv − w_u S_v − w_v S_u − w_uv S) / w`; the
/// derivatives past `order` are not computed and must not be read.
fn patch_box(net: &Shifted, ku: usize, kv: usize, u: Ivl, v: Ivl, order: usize) -> Option<Ders> {
    let order = order.min(2);
    let nu = ders_basis(&net.nurbs.u, ku, net.nurbs.p, u, order);
    let nv = ders_basis(&net.nurbs.v, kv, net.nurbs.q, v, order);
    let basis = |m: &Vec<Vec<Ivl>>, a: usize, i: usize| m.get(a).and_then(|r| r.get(i)).copied();
    let mut h = [[[Ivl::from(0.0); 4]; 3]; 3];
    for (a, row) in h.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            if a + b > order {
                continue;
            }
            for i in 0..=net.nurbs.p {
                for j in 0..=net.nurbs.q {
                    let li = 4
                        * ((ku + i).checked_sub(net.nurbs.p)? * net.nurbs.nv
                            + (kv + j).checked_sub(net.nurbs.q)?);
                    let c = basis(&nu, a, i)? * basis(&nv, b, j)?;
                    for (e, he) in hab.iter_mut().enumerate() {
                        *he = *he + c * *net.pw.get(li + e)?;
                    }
                }
            }
        }
    }
    let [[h00, h01, h02], [h10, h11, _], [h20, _, _]] = h;
    let w = h00[3];
    let positive = w.lo > 0.0;
    if !positive {
        return None;
    }
    let two = Ivl::from(2.0);
    let s = [h00[0] / w, h00[1] / w, h00[2] / w];
    let first = |d: [Ivl; 4]| {
        [
            (d[0] - d[3] * s[0]) / w,
            (d[1] - d[3] * s[1]) / w,
            (d[2] - d[3] * s[2]) / w,
        ]
    };
    let (su, sv) = (first(h10), first(h01));
    // (A_ab − w_a S_b − w_b S_a − w_ab S) / w, componentwise
    let second = |d: [Ivl; 4], wa: Ivl, sa: Iv3, wb: Ivl, sb: Iv3| {
        let [d0, d1, d2, dw] = d;
        [
            (d0 - wa * sb[0] - wb * sa[0] - dw * s[0]) / w,
            (d1 - wa * sb[1] - wb * sa[1] - dw * s[1]) / w,
            (d2 - wa * sb[2] - wb * sa[2] - dw * s[2]) / w,
        ]
    };
    // S_uu: w_a = w_b = w_u and S_a = S_b = S_u, the middle terms 2 w_u S_u
    let pure = |d: [Ivl; 4], wa: Ivl, sa: Iv3| {
        let [d0, d1, d2, dw] = d;
        [
            (d0 - two * wa * sa[0] - dw * s[0]) / w,
            (d1 - two * wa * sa[1] - dw * s[1]) / w,
            (d2 - two * wa * sa[2] - dw * s[2]) / w,
        ]
    };
    let suu = pure(h20, h10[3], su);
    let svv = pure(h02, h01[3], sv);
    let suv = second(h11, h10[3], su, h01[3], sv);
    Some(Ders {
        s,
        su,
        sv,
        suu,
        suv,
        svv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::vector2d::Point2;
    use crate::kernel::Kernel;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
    use crate::surface::sphere::Sphere;

    fn ball(k: &Kernel) -> Surface {
        Surface::from(
            Sphere::create_canonical(
                k,
                &Axis2Placement::new(
                    Vec3::new(1.0, -2.0, 3.0),
                    Vec3::new(0.0, 0.6, 0.8),
                    Vec3::new(1.0, 0.0, 0.0),
                ),
                6.0,
            )
            .unwrap(),
        )
    }

    /// The point at `t` of the Hermite span `[p0, d0, p1, d1]` on `[0, 1]`.
    fn hermite([p0, d0, p1, d1]: [Vec3; 4], t: f64) -> Vec3 {
        let (t2, t3) = (t * t, t * t * t);
        p0 * (2.0 * t3 - 3.0 * t2 + 1.0)
            + d0 * (t3 - 2.0 * t2 + t)
            + p1 * (-2.0 * t3 + 3.0 * t2)
            + d1 * (t3 - t2)
    }

    /// A UV span on the ball from `a` to `b` (its chord as both derivatives) and the 3D
    /// Hermite span through the image's end points with the chain-rule derivatives.
    fn spans(k: &Kernel, s: &Surface, a: Point2, b: Point2) -> ([Vec3; 4], [Vec3; 4]) {
        let d = b - a;
        let image = |p: Point2| {
            let m = s.evaluate(k, p, 1, 1, EvaluateFlags::default()).unwrap();
            (m[0][0], m[1][0] * d.x + m[0][1] * d.y)
        };
        let ((p0, d0), (p1, d1)) = (image(a), image(b));
        let lift = |p: Point2| Vec3::new(p.x, p.y, 0.0);
        let dv = Vec3::new(d.x, d.y, 0.0);
        ([lift(a), dv, lift(b), dv], [p0, d0, p1, d1])
    }

    /// The largest distance between the image and the 3D span at `n + 1` parameters.
    fn sampled(k: &Kernel, s: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], n: u32) -> f64 {
        (0..=n)
            .map(|i| {
                let t = f64::from(i) / f64::from(n);
                let q = hermite(uv, t);
                let p = s.evaluate_point(k, Point2::new(q.x, q.y)).unwrap();
                p.distance(hermite(xyz, t))
            })
            .fold(0.0, f64::max)
    }

    /// Whether the certificate is sound on the span: at the tolerance `tol` it certifies
    /// with a bound holding every sampled distance, and at 0.99 of the largest sampled
    /// distance (below the true largest) it does not.
    fn sound(k: &Kernel, s: &Surface, uv: [Vec3; 4], xyz: [Vec3; 4], tol: f64) -> f64 {
        let seen = sampled(k, s, uv, xyz, 4000);
        match sphere_span_image(s, uv, xyz, tol) {
            SpanImage::Certified(bound) => {
                assert!(seen <= bound && bound <= tol, "{seen} {bound} {tol}");
            }
            other => panic!("{other:?}, sampled {seen}, tolerance {tol}"),
        }
        assert!(
            matches!(
                sphere_span_image(s, uv, xyz, 0.99 * seen),
                SpanImage::NotCertified(_)
            ),
            "certified below the sampled {seen}"
        );
        seen
    }

    /// The bound holds the sampled distances and is at most the tolerance, on spans inside
    /// one patch, across a knot, and near a pole; below the largest sampled distance there
    /// is no certificate.
    #[test]
    fn the_bound_holds_every_sampled_distance() {
        let k = &Kernel::default();
        let s = ball(k);
        let cases = [
            (Point2::new(0.05, 0.3), Point2::new(0.1, 0.4)),
            (Point2::new(0.23, 0.48), Point2::new(0.27, 0.52)),
            (Point2::new(0.6, 0.97), Point2::new(0.62, 0.999)),
            (Point2::new(0.77, 0.15), Point2::new(0.73, 0.17)),
        ];
        for (a, b) in cases {
            let (uv, xyz) = spans(k, &s, a, b);
            let seen = sound(k, &s, uv, xyz, 1e-2);
            assert!(seen < 1e-2 / 4.0, "{a:?} {b:?}: sampled {seen}");
        }
    }

    /// A span whose UV Hermite crosses the seam u = 0, so the evaluator clamps part of it to
    /// u = 0: the bound holds the clamped image's distances, and below them there is no
    /// certificate.
    #[test]
    fn a_span_crossing_the_seam_is_bounded_with_its_clamp() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(k, &s, Point2::new(0.01, 0.4), Point2::new(-0.01, 0.42));
        let seen = sampled(k, &s, uv, xyz, 4000);
        assert!(seen > 1e-3, "the clamp moves the image: {seen}");
        sound(k, &s, uv, xyz, 1.0);
    }

    /// Spans reaching the pole v = 1 and running past it: the bound holds the clamped
    /// image's distances, and below them there is no certificate.
    #[test]
    fn a_span_reaching_the_pole_is_bounded_with_its_clamp() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(k, &s, Point2::new(0.3, 0.98), Point2::new(0.31, 1.0));
        sound(k, &s, uv, xyz, 1e-2);
        let (uv, xyz) = spans(k, &s, Point2::new(0.3, 0.98), Point2::new(0.31, 1.02));
        sound(k, &s, uv, xyz, 1.0);
    }

    /// A UV span whose second control point leaves the domain far behind (as the #16669
    /// span's did, u = −18.5) is not certified, though its ends are on the 3D span.
    #[test]
    fn a_span_whose_uv_image_leaves_the_curve_is_not_certified() {
        let k = &Kernel::default();
        let s = ball(k);
        let (mut uv, xyz) = spans(k, &s, Point2::new(0.97, 0.9), Point2::new(0.99, 0.99));
        uv[1] = Vec3::new(-30.0, uv[1].y, 0.0);
        assert!(sampled(k, &s, uv, xyz, 2000) > 0.1);
        assert!(matches!(
            sphere_span_image(&s, uv, xyz, 1e-2),
            SpanImage::NotCertified(_)
        ));
    }

    /// A tolerance of zero on a span so short that every point's distance encloses 0: no
    /// point is off, and the bisection ends at its depth bound.
    #[test]
    fn a_zero_tolerance_ends_at_the_depth_bound() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(
            k,
            &s,
            Point2::new(0.2, 0.3),
            Point2::new(0.2 + 1e-7, 0.3 + 1e-7),
        );
        assert_eq!(
            sphere_span_image(&s, uv, xyz, 0.0),
            SpanImage::NotCertified(Refusal::Depth)
        );
    }

    /// The net's homogeneous control points, enclosed.
    fn enclosed(net: &Nurbs) -> Vec<Ivl> {
        net.pw.iter().copied().map(Ivl::from).collect()
    }

    /// The canonical sphere's net is C¹ across its four double knots (three in u, one in
    /// v): `joints` finds each, with a derivative jump of rounding size.
    #[test]
    fn the_canonical_net_is_c1_at_its_double_knots() {
        let k = &Kernel::default();
        let s = ball(k);
        let SurfaceKind::Sphere(sphere) = s.kind() else {
            panic!("a sphere")
        };
        let net = Nurbs::of(sphere.base().base()).unwrap();
        let pw = enclosed(&net);
        let ju = joints(&net, &pw, true).unwrap();
        let jv = joints(&net, &pw, false).unwrap();
        assert_eq!((ju.len(), jv.len()), (3, 1), "{ju:?} {jv:?}");
        for j in ju.iter().chain(&jv) {
            assert!(j.jump < 1e-12, "{j:?}");
        }
    }

    /// A quadratic net (degree 1 across) whose double knot at u = 1/2 joins two straight
    /// spans: C¹ when the knots split the two spans' lengths alike, refused when they do
    /// not (unequal arcs: the speed jumps) or when the spans turn a corner.
    #[test]
    fn a_net_that_is_only_c0_at_a_double_knot_is_refused() {
        let net = |u: Vec<f64>, pts: [[f64; 3]; 5]| {
            let mut pw = Vec::new();
            for p in pts {
                for dz in [0.0, 1.0] {
                    pw.extend([p[0], p[1], p[2] + dz, 1.0]);
                }
            }
            Nurbs {
                p: 2,
                q: 1,
                nu: 5,
                nv: 2,
                u,
                v: vec![0.0, 0.0, 1.0, 1.0],
                pw,
            }
        };
        let straight = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
        ];
        let even = net(vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], straight);
        let j = joints(&even, &enclosed(&even), true).unwrap();
        assert!(j.len() == 1 && j[0].jump < 1e-12, "{j:?}");
        let uneven = net(vec![0.0, 0.0, 0.0, 0.25, 0.25, 1.0, 1.0, 1.0], straight);
        assert!(joints(&uneven, &enclosed(&uneven), true).is_none());
        let corner = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 0.0],
        ];
        let turned = net(vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0], corner);
        assert!(joints(&turned, &enclosed(&turned), true).is_none());
    }

    /// Other surface kinds get no certificate.
    #[test]
    fn a_plane_is_not_a_sphere() {
        let s = Surface::from(
            BSplineSurface::create_bilinear_surface(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let z = [Vec3::ZERO; 4];
        assert_eq!(sphere_span_image(&s, z, z, 1.0), SpanImage::NotASphere);
        assert_eq!(sphere_span_sweep(&s, z, z), SpanSweep::NotASphere);
        let mut uv = [Vec3::ZERO; 4];
        assert_eq!(
            fit_monotone_sweep(&Kernel::default(), &s, &mut uv, &z, 1.0).unwrap(),
            Ok(SweepFit::default())
        );
    }

    /// The least of the UV Hermite span's sweep derivative `u'` at `n + 1` parameters, as a
    /// share of its chord `u(1) − u(0)`: negative where it turns back.
    fn least_rate(uv: [Vec3; 4], n: u32) -> f64 {
        let du = |t: f64| {
            let [p0, d0, p1, d1] = uv;
            let (t2,) = (t * t,);
            p0.x * (6.0 * t2 - 6.0 * t)
                + d0.x * (3.0 * t2 - 4.0 * t + 1.0)
                + p1.x * (-6.0 * t2 + 6.0 * t)
                + d1.x * (3.0 * t2 - 2.0 * t)
        };
        let chord = uv[2].x - uv[0].x;
        (0..=n)
            .map(|i| du(f64::from(i) / f64::from(n)) / chord)
            .fold(f64::INFINITY, f64::min)
    }

    /// #17110: next to a pole a UV span whose sweep handle overshoots its chord turns back in
    /// u (sampled `u' < 0`) while its image stays within the tolerance of the 3D span, whose
    /// longitude is monotone: the image certificate accepts it, the sweep certificate does
    /// not. The same span with its chord as both handles is monotone.
    #[test]
    fn a_span_zigzagging_in_u_next_to_a_pole_turns_back() {
        let k = &Kernel::default();
        let s = ball(k);
        let (mut uv, xyz) = spans(k, &s, Point2::new(0.30, 0.995), Point2::new(0.32, 0.996));
        assert_eq!(sphere_span_sweep(&s, uv, xyz), SpanSweep::Monotone);
        assert!(least_rate(uv, 1000) > 0.0);
        uv[1].x *= 6.0;
        assert!(least_rate(uv, 1000) < 0.0, "the span turns back");
        assert!(
            matches!(
                sphere_span_image(&s, uv, xyz, 1e-2),
                SpanImage::Certified(_)
            ),
            "the image certificate does not see it"
        );
        assert_eq!(sphere_span_sweep(&s, uv, xyz), SpanSweep::TurnsBack);
        // a start tangent of 2.99 chords (handle offset 0.997 of the chord; the end handle
        // is a third of it): the polygon is not monotone, but the span is (the derivative's
        // vertex stays positive)
        uv[1].x = 0.02 * 2.99;
        assert_eq!(sphere_span_sweep(&s, uv, xyz), SpanSweep::Monotone);
    }

    /// No constraint where the 3D span's longitude turns (a span leaving a meridian and
    /// coming back to it, its 3D tangents the chain rule of its UV tangents), nor on a span
    /// ending at the pole v = 1, though both UV spans turn back in u.
    #[test]
    fn a_turning_span_and_a_span_to_the_pole_are_free() {
        let k = &Kernel::default();
        let s = ball(k);
        let image = |p: Point2, d: Vec3| {
            let m = s.evaluate(k, p, 1, 1, EvaluateFlags::default()).unwrap();
            (m[0][0], m[1][0] * d.x + m[0][1] * d.y)
        };
        let (a, b) = (Point2::new(0.3, 0.5), Point2::new(0.3, 0.6));
        let (da, db) = (Vec3::new(0.05, 0.1, 0.0), Vec3::new(-0.05, 0.1, 0.0));
        let ((p0, d0), (p1, d1)) = (image(a, da), image(b, db));
        let uv = [Vec3::new(a.x, a.y, 0.0), da, Vec3::new(b.x, b.y, 0.0), db];
        assert_eq!(sphere_span_sweep(&s, uv, [p0, d0, p1, d1]), SpanSweep::Free);
        let (mut uv, xyz) = spans(k, &s, Point2::new(0.30, 0.995), Point2::new(0.32, 1.0));
        uv[1].x *= 6.0;
        assert!(least_rate(uv, 1000) < 0.0);
        assert_eq!(sphere_span_sweep(&s, uv, xyz), SpanSweep::Free);
    }

    /// The sign test of a quadratic's Bernstein coefficients: either side of the interior
    /// vertex touching zero (`b_1² = b_0 b_2`, which the outward rounding leaves undecided),
    /// and a NaN certifies nothing.
    #[test]
    fn the_sign_test_holds_either_side_of_the_vertex() {
        let q = |a: f64, b: f64, c: f64| keeps_sign([Ivl::from(a), Ivl::from(b), Ivl::from(c)]);
        assert!(q(1.0, 2.0, 0.5));
        assert!(q(1.0, -0.9, 1.0));
        assert!(q(4.0, -2.0 + 1e-12, 1.0));
        assert!(!q(4.0, -2.0, 1.0));
        assert!(!q(4.0, -2.0 - 1e-12, 1.0));
        assert!(!q(-1e-300, 1.0, 1.0));
        assert!(!q(f64::NAN, 1.0, 1.0));
        // a constant sweep coordinate is monotone: equal points differ by exactly 0
        let c = Ivl::from(9.261_725_223_348_444);
        assert_eq!(difference(c, c), Ivl::from(0.0));
        assert!(keeps_sign([difference(c, c); 3]));
        assert!(difference(Ivl::from(1.0), Ivl::from(1e-30)).lo < 1.0);
        assert!(!q(1.0, f64::NAN, 1.0));
        // a falling sweep stalled at its start (#17686 gate: a meridian span of the
        // thickened pole cap whose first handle sits on its point) keeps one sign
        let e = -2.775_557_561_562_891_4e-17;
        assert!(keeps_either_sign([
            Ivl::from(0.0),
            Ivl::from(e),
            Ivl::from(e)
        ]));
        assert!(keeps_either_sign([
            Ivl::from(0.0),
            Ivl::from(-e),
            Ivl::from(-e)
        ]));
        assert!(!keeps_either_sign([
            Ivl::from(1.0),
            Ivl::from(-3.0),
            Ivl::from(1.0)
        ]));
    }

    /// Cubic Bezier chains of the UV and 3D Hermite spans through the UV points `pts` of the
    /// ball (chord handles, chain-rule 3D tangents of the same chords), joined end to end,
    /// each span of parameter length 1: one 3D tangent per point, from the mean chord.
    fn chain(k: &Kernel, s: &Surface, pts: &[Point2]) -> (Vec<Vec3>, Vec<Vec3>) {
        let n = pts.len();
        let tangent = |i: usize| {
            let (a, b) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
            let span = ((i + 1).min(n - 1) - i.saturating_sub(1)) as f64;
            (b - a) / span
        };
        let (mut uv, mut xyz) = (Vec::new(), Vec::new());
        for (i, &p) in pts.iter().enumerate() {
            let d = tangent(i);
            let m = s.evaluate(k, p, 1, 1, EvaluateFlags::default()).unwrap();
            let (q, dq) = (m[0][0], m[1][0] * d.x + m[0][1] * d.y);
            let (pu, du) = (Vec3::new(p.x, p.y, 0.0), Vec3::new(d.x, d.y, 0.0));
            if i > 0 {
                uv.push(pu - du / 3.0);
                xyz.push(q - dq / 3.0);
            }
            uv.push(pu);
            xyz.push(q);
            if i + 1 < n {
                uv.push(pu + du / 3.0);
                xyz.push(q + dq / 3.0);
            }
        }
        (uv, xyz)
    }

    /// #17110: a chain next to a pole whose middle handles overshoot in u is refitted: the
    /// spans that turned back are monotone in u after the fit, every refitted span's image
    /// is certified within the tolerance of its 3D span, the points and the 3D chain are
    /// unchanged, and the joint keeps its handles collinear (G1). A chain that is already
    /// monotone is left as it is.
    #[test]
    fn a_zigzagging_chain_next_to_a_pole_is_refitted_monotone() {
        let k = &Kernel::default();
        let s = ball(k);
        let pts = [
            Point2::new(0.30, 0.995),
            Point2::new(0.32, 0.996),
            Point2::new(0.33, 0.997),
        ];
        let (mut uv, xyz) = chain(k, &s, &pts);
        let before = uv.clone();
        assert_eq!(
            fit_monotone_sweep(k, &s, &mut uv, &xyz, 1e-2).unwrap(),
            Ok(SweepFit::default())
        );
        assert_eq!(uv, before);
        // the middle joint's u handles overshoot the second span's chord
        let q = uv[3];
        uv[2].x = q.x - 0.06;
        uv[4].x = q.x + 0.06 * (uv[4].y - q.y) / (q.y - uv[2].y);
        let span = |c: &[Vec3], j: usize| [c[3 * j], c[3 * j + 1], c[3 * j + 2], c[3 * j + 3]];
        assert_eq!(
            sphere_bezier_sweep(&s, span(&uv, 1), span(&xyz, 1)),
            SpanSweep::TurnsBack
        );
        let zig = uv.clone();
        let fitted = fit_monotone_sweep(k, &s, &mut uv, &xyz, 1e-2)
            .unwrap()
            .unwrap();
        assert!(fitted.refitted >= 1 && fitted.bound > 0.0 && fitted.bound <= 1e-2);
        for j in 0..2 {
            assert_eq!(
                sphere_bezier_sweep(&s, span(&uv, j), span(&xyz, j)),
                SpanSweep::Monotone
            );
            assert!(matches!(
                sphere_bezier_image(&s, span(&uv, j), span(&xyz, j), 1e-2),
                SpanImage::Certified(_)
            ));
        }
        for i in [0, 3, 6] {
            assert_eq!(uv[i], zig[i], "point {i} moved");
        }
        for i in [1, 2, 4, 5] {
            assert_eq!(uv[i].y, zig[i].y, "the v handle {i} moved");
        }
        let (l, r) = (uv[3] - uv[2], uv[4] - uv[3]);
        assert!((l.x * r.y - l.y * r.x).abs() <= 1e-15, "G1 at the joint");
    }

    /// #17110: a chain next to a pole whose points run back in u at the middle joint while
    /// the 3D curve's longitude keeps running one way keeps the points' turn at that joint
    /// and has every span refitted monotone; the 3D chain is the chain of the monotone
    /// points, the UV chain's middle point moved back past the first.
    #[test]
    fn a_chain_whose_points_run_back_in_u_keeps_the_turn_at_the_joint() {
        let k = &Kernel::default();
        let s = ball(k);
        let pts = [
            Point2::new(0.30, 0.995),
            Point2::new(0.32, 0.996),
            Point2::new(0.34, 0.997),
        ];
        let (mut uv, xyz) = chain(k, &s, &pts);
        // the middle point and its handles move back in u by 0.03: the first span's chord
        // turns negative, the second's stays positive
        for p in &mut uv[2..=4] {
            p.x -= 0.03;
        }
        let joints = [uv[0], uv[3], uv[6]];
        let fit = fit_monotone_sweep(k, &s, &mut uv, &xyz, 1.0)
            .unwrap()
            .unwrap();
        assert!(fit.refitted > 0 && fit.bound <= 1.0);
        assert_eq!([uv[0], uv[3], uv[6]], joints);
        // the first span runs back in u, the second forward: the turn is at the joint
        assert!(uv[3].x < uv[0].x && uv[6].x > uv[3].x);
        for j in 0..2 {
            let q = [uv[3 * j], uv[3 * j + 1], uv[3 * j + 2], uv[3 * j + 3]];
            let h = [xyz[3 * j], xyz[3 * j + 1], xyz[3 * j + 2], xyz[3 * j + 3]];
            assert_eq!(sphere_bezier_sweep(&s, q, h), SpanSweep::Monotone);
        }
    }

    /// #17110 (review of #17686): a chain end whose only handle points back in u with
    /// no offset across it gets the factor 0, which would put the handle on the end point
    /// (a zero derivative there): refused (`Degenerate`), as at an interior joint.
    #[test]
    fn a_chain_end_whose_handle_collapses_is_refused() {
        let k = &Kernel::default();
        let s = ball(k);
        let pts = [
            Point2::new(0.30, 0.995),
            Point2::new(0.32, 0.996),
            Point2::new(0.34, 0.997),
        ];
        let (mut uv, xyz) = chain(k, &s, &pts);
        uv[1] = Vec3::new(uv[0].x - 0.005, uv[0].y, 0.0);
        let q = [uv[0], uv[1], uv[2], uv[3]];
        let h = [xyz[0], xyz[1], xyz[2], xyz[3]];
        assert_eq!(sphere_bezier_sweep(&s, q, h), SpanSweep::TurnsBack);
        assert_eq!(
            fit_monotone_sweep(k, &s, &mut uv, &xyz, 1.0).unwrap(),
            Err(SweepRefusal::Degenerate)
        );
        // with an offset across the sweep the same end is refitted
        let (mut uv, xyz) = chain(k, &s, &pts);
        uv[1] = Vec3::new(uv[0].x - 0.005, uv[0].y + 1e-4, 0.0);
        assert!(fit_monotone_sweep(k, &s, &mut uv, &xyz, 1.0)
            .unwrap()
            .is_ok());
    }

    /// The fit polls the kernel's cancellation and reports it apart from its refusals.
    #[test]
    fn a_cancelled_fit_is_cancelled_not_refused() {
        let mut kernel = Kernel::default();
        let s = ball(&kernel);
        let pts = [Point2::new(0.30, 0.995), Point2::new(0.32, 0.996)];
        let (mut uv, xyz) = chain(&kernel, &s, &pts);
        uv[1].x = uv[0].x - 0.005;
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        kernel.set_cancel_token(token);
        let error = fit_monotone_sweep(&kernel, &s, &mut uv, &xyz, 1.0).unwrap_err();
        assert!(error.is_cancelled());
    }

    /// A refit that cannot keep the image within the tolerance is refused: far from the
    /// pole, the refit's u parametrization differs from the 3D span's by a share of the
    /// chord, about 0.1 on the ball, past the tolerance of 1e-3 that the span with its
    /// chord handles meets.
    #[test]
    fn a_refit_off_the_3d_curve_is_refused() {
        let k = &Kernel::default();
        let s = ball(k);
        let (uv, xyz) = spans(k, &s, Point2::new(0.30, 0.5), Point2::new(0.32, 0.5));
        let mut b = [uv[0], uv[0] + uv[1] * 2.0, uv[2] - uv[3] / 3.0, uv[2]];
        let h = [xyz[0], xyz[0] + xyz[1] / 3.0, xyz[2] - xyz[3] / 3.0, xyz[2]];
        let chord = [uv[0], uv[0] + uv[1] / 3.0, uv[2] - uv[3] / 3.0, uv[2]];
        assert!(matches!(
            sphere_bezier_image(&s, chord, h, 1e-3),
            SpanImage::Certified(_)
        ));
        assert_eq!(sphere_bezier_sweep(&s, b, h), SpanSweep::TurnsBack);
        assert!(matches!(
            fit_monotone_sweep(k, &s, &mut b, &h, 1e-3).unwrap(),
            Err(SweepRefusal::Off(_))
        ));
    }
}
