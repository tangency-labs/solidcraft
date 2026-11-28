//! A certified test that a surface's continuation past its natural domain does not fold
//! over the domain a cover would merge onto: the acceptance half of
//! `CoverCoincidentSurface`'s fold rule (WR-407).
//!
//! Upstream: none (not in SMLib; the fork's rule, tangency-labs/smlib 1b6f5fa, only
//! samples a grid, which can step over a fold).
//!
//! | Rust                  | Upstream |
//! |-----------------------|----------|
//! | [`certify_no_fold`]   | none     |
//! | [`FoldCertificate`]   | none     |
//! | [`NotProvedReason`]   | none     |
//!
//! **What is proved.** A fold of the continuation is where its normal `N = Du x Dv`
//! vanishes (the map stops being an immersion there), or where `N` reverses across the
//! natural domain's side (the continuation turns back over the face at its edge, with
//! `N` zero along that edge) or across a knot line where the net is only `C0` (`N`
//! jumps there without vanishing). The certificate excludes these. Below, "the computed
//! `N . d > E`" means every Bernstein coefficient of `N . d`, as computed, is above its
//! arithmetic bound `E` (so the exact `N . d > 0` on the piece), and "the computed
//! `N . d >= -E`" means none is below `-E` (so the exact `N . d >= -2 E`); a coefficient
//! or bound that is not finite proves nothing:
//! - every piece of the merge domain outside the closed natural domain has the computed
//!   `N . d > E` for one direction `d` (so `N` never vanishes there);
//! - a piece that touches the natural domain, once split down to the last level, has the
//!   computed `N . d >= -E` on it instead (a zero of `N` on the natural domain's side, a
//!   degenerate edge or corner of the surface's own domain, is allowed);
//! - for every piece that touches the natural domain, the computed `N . d >= -E` also
//!   holds on its mirror across the sides it touches, inside the natural domain, with
//!   the same `d`: `N` does not reverse across the side;
//! - for every piece with a side on a `C0` knot line (a knot of multiplicity at least
//!   the degree) inside the merge domain, the computed `N . d >= -E` holds on the strip
//!   of the same width across that line, with the same `d`: `N` does not reverse across
//!   the knot line.
//!
//! The pieces start as the cells of the fold rule's `20 x 20` sample grid over the merge
//! domain, cut at knot lines and at the natural domain's sides; `d` is the normal at a
//! piece's centre (any direction is valid, the choice only makes the proof succeed or
//! not). A piece not proved is quartered, at most [`MAX_SPLITS`] times, each quarter
//! with its own `d`; past that the certificate refuses
//! ([`FoldCertificate::NotProved`]), as it does for a surface kind with no certificate
//! here. On the last split's pieces along the natural domain's sides (`1 / 16` of a
//! cell wide) only the computed `N . d >= -E` is shown: a zero of `N` there (an
//! even-order zero of `N . d` up to `1 / 16` of a cell off the face, for one), or a
//! reversal within `2 E`, is not excluded.
//!
//! **What is not proved.** That the continuation does not come back over the face
//! without folding (a revolution swept past a full turn, a sheet curling round onto
//! itself): the certificate is local, it excludes folds, not overlaps at a distance.
//!
//! **How (B-spline nets).** The evaluated surface is its NURBS form, continued past its
//! knot range by its end spans' polynomials (NLib's span search clamps to the end span).
//! The form's Bezier patches ([`decompose_to_bez`]) are restricted to each piece by
//! blossoming, which extrapolates exactly past a patch's own interval. On a piece, with
//! homogeneous coordinates `H` and weight `w`, the normal's direction is that of
//! `H_u x H_v` (polynomial form) or `(w H_u - w_u H) x (w H_v - w_v H)` (rational form,
//! `w^4 N`; the weight must be shown positive on the piece), written in Bernstein form
//! ([`crate::offset::construction::bernstein`]); `N . d` lies within its coefficients'
//! hull (Farouki and Rajan 1987). The derivatives are taken on the whole patch and then
//! restricted (a restricted derivative is the piece's derivative over its positive
//! widths), so a piece only an ulp wide, where a knot or a side falls an ulp from a
//! grid line, keeps the precision its coefficients' differences would lose.
//!
//! **The arithmetic's bound.** Every coefficient is computed twice more, on absolute
//! values: with the net's magnitudes `|c|` (twin `T`) and with `|c| + e0` (twin `Te`),
//! `e0` the extraction's error per coefficient ([`decompose_to_bez`] inserts knots
//! within `gamma_(16 (p + q))` of the net's largest coefficient, the bound
//! [`crate::query::distance_bound`] takes). The computation is a tree of `K` levels of
//! roundings (`K` counted from the degrees: `3` per de Casteljau level of the
//! restriction, `2` per derivative, `N + 7` per Bernstein product of `N`-term sums, `1`
//! per difference, `3` for the dot with `d`), so a computed coefficient is within
//! `gamma_K T` of the exact coefficient of the extracted net (Higham, *Accuracy and
//! Stability of Numerical Algorithms*, 2nd ed., §3.1), and the extraction's error moves
//! it by at most `Te - T` (the twin is a polynomial with nonnegative coefficients in the
//! magnitudes). With the twins' own rounding, `E = (Te - T) + 3 gamma_(2K) Te`. A twin
//! that overflows gives a bound that is not finite, which proves nothing. Far past a
//! short end span the restriction's magnitude weights `|1 - t|`, `|t|` grow the twins
//! like `(2 |t| + 1)^degree`; a piece whose coefficients all have the right sign but not
//! by more than `E` is refused for precision ([`NotProvedReason::Precision`]), not as a
//! zero of the normal.
//!
//! **Other kinds.** A plane's STEP evaluation is affine, so its normal is constant:
//! proved. Spheres, cones and tori: the normal is a scalar closed form times a unit
//! normal field that never vanishes, so the continuation folds exactly where the scalar
//! changes sign (see [`analytic`]). Extrusions and revolutions: proved along the sweep,
//! along a line generator's continuation by a closed form ([`swept`]), and along an
//! extrusion's B-spline generator's continuation as the B-spline net of the extrusion
//! ([`extruded_net`]). Every other kind, and a continuation of any other generator
//! (a revolution's B-spline generator; an ellipse, parabola or hyperbola, whose
//! evaluation clamps to its own interval rather than continuing), is not proved.
//!
//! **Failures.** A construction the certificate cannot make (a Bezier extraction, a
//! net or a Bernstein product refused as invalid input) refuses the continuation
//! ([`NotProvedReason::Construction`]) rather than failing the caller: the certificate
//! is the acceptance half of a rule whose other answer is "not covered". Cancellation,
//! an unported call and the port's invariants still propagate.
//!
//! Deviations: none (no upstream counterpart). No recursion: pieces wait on a stack.
//! [`MAX_PIECES`] bounds the normals computed per call (each piece's own, and those of
//! its mirrors and its knot strips), and the cancel flag is polled before each.

use crate::curve::curve::{Curve, CurveKind};
use crate::error::Result;
use crate::geom::certified::above;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::b_spline_surface::p1::{BSplineSurface, BSplineSurfaceForm};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::Surface as NlSurface;
use crate::nurbs::srf_tool::p1::decompose_to_bez;
use crate::offset::construction::bernstein::{Bernstein2, Binomials, MAX_DEGREE};
use crate::surface::surface::{Surface, SurfaceKind};

/// The cells per side of the merge domain: the fold rule's `21 x 21` samples' cells.
pub const GRID: usize = 20;

/// The most times a piece the coefficients do not decide is quartered.
pub const MAX_SPLITS: usize = 4;

/// The most pieces one certificate computes (bounded work; past it, nothing is proved).
pub const MAX_PIECES: usize = 1 << 16;

/// The work of one piece of a bicubic polynomial net, `((p + 1) (q + 1))^2`: a piece of
/// a net of higher degree counts for more, so [`MAX_PIECES`] such pieces bound the
/// work at every degree (a net of very high degree is refused for it).
const UNIT_WORK: usize = 256;

/// The operation path of the certificate's cancel polls.
const OP: &str = "surface::fold_certificate::certify_no_fold";

/// The answer of [`certify_no_fold`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoldCertificate {
    /// The continuation is proved free of folds over the merge domain.
    Proved,
    /// Nothing is proved near `uv` (the centre of the piece that failed, or of
    /// the merge domain): `reason` says why.
    NotProved {
        /// Where.
        uv: Point2,
        /// Why.
        reason: NotProvedReason,
    },
}

/// Why [`certify_no_fold`] proved nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotProvedReason {
    /// The merge domain is not a finite rectangle of positive width.
    DomainNotFinite,
    /// No certificate for the surface kind (of the kinds that reach the fold rule, a
    /// procedural blend).
    KindNotCertified,
    /// A swept surface's generator is continued past its natural interval, and that
    /// generator's continuation has no certificate (a conic, or a revolution's B-spline
    /// generator).
    GeneratorNotCertified,
    /// A line generator's closed form is not finite.
    GeneratorNotFinite,
    /// A revolved line's normal turns back or vanishes along its continuation.
    GeneratorTurnsOrVanishes,
    /// The net is in power-basis form, has no NURBS form or is two-dimensional.
    NetForm,
    /// A control point is not finite or a weight not positive.
    NetNotFinite,
    /// The normal's degrees exceed the Bernstein products'.
    Degrees,
    /// The Bezier patches are empty or degenerate, or an end span is narrower than the
    /// span search's cut-off (whose continuation the patches would not follow).
    Patches,
    /// A construction of the certificate was refused as invalid input.
    Construction,
    /// The work bound ([`MAX_PIECES`] normals) was reached.
    WorkBound,
    /// A rational net's weight is not proved positive on a piece.
    WeightNotPositive,
    /// A coefficient or its bound overflowed (is not finite).
    NotFinite,
    /// Every coefficient has the right sign, but some not by more than its rounding
    /// bound: the arithmetic is not precise enough to decide (typically far past a
    /// short end span, where the twins grow like `(2 |t| + 1)^degree`).
    Precision,
    /// A zero of the normal on a piece of the continuation is not excluded.
    NormalZero,
    /// A turn back across the natural domain's side is not excluded.
    TurnBackAcrossSide,
    /// A turn back across a `C0` knot line of the continuation is not excluded.
    TurnBackAcrossKnot,
    /// A sphere's, cone's or torus's closed form is not finite.
    AnalyticNotFinite,
    /// A sphere's, cone's or torus's normal turns back.
    AnalyticTurnsBack,
    /// A sphere's, cone's or torus's normal vanishes past the natural domain.
    AnalyticVanishes,
    /// A sphere's, cone's or torus's normal sign is decided nowhere.
    AnalyticUndecided,
}

impl core::fmt::Display for NotProvedReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::DomainNotFinite => "the merge domain is not a finite rectangle",
            Self::KindNotCertified => "no fold certificate for this surface kind",
            Self::GeneratorNotCertified => "the generator's continuation is not proved",
            Self::GeneratorNotFinite => "the generator's closed form is not finite",
            Self::GeneratorTurnsOrVanishes => {
                "the normal turns back or vanishes along the generator's continuation"
            }
            Self::NetForm => "a power-basis, two-dimensional or missing NURBS form",
            Self::NetNotFinite => "a coefficient not finite or a weight not positive",
            Self::Degrees => "degrees past the certificate's",
            Self::Patches => "degenerate Bezier patches or a too narrow end span",
            Self::Construction => "a construction of the certificate was refused",
            Self::WorkBound => "the certificate's work bound",
            Self::WeightNotPositive => "the weight is not proved positive",
            Self::NotFinite => "the arithmetic overflowed",
            Self::Precision => "the arithmetic's bound does not decide the normal's sign",
            Self::NormalZero => "a zero of the normal is not excluded",
            Self::TurnBackAcrossSide => {
                "a turn back across the natural domain's side is not excluded"
            }
            Self::TurnBackAcrossKnot => "a turn back across a C0 knot line is not excluded",
            Self::AnalyticNotFinite => "a closed form not finite",
            Self::AnalyticTurnsBack => "the normal turns back",
            Self::AnalyticVanishes => "the normal vanishes past the natural domain",
            Self::AnalyticUndecided => "the normal's sign is not decided",
        })
    }
}

/// Whether `s` (evaluated with out-of-bounds evaluation on, in its STEP parameters) is
/// proved not to fold over `domain` past `natural`, its natural (current STEP) domain.
///
/// # Errors
///
/// Cancellation, and the errors [`crate::error::native_failure`] passes on (an
/// unported call, the port's invariants); every other failure of a construction the
/// certificate makes refuses ([`NotProvedReason::Construction`]).
pub fn certify_no_fold(
    k: &Kernel,
    s: &Surface,
    natural: &Extent2d,
    domain: &Extent2d,
) -> Result<FoldCertificate> {
    Ok(
        crate::error::native_failure(dispatch(k, s, natural, domain))?.unwrap_or_else(|| {
            FoldCertificate::NotProved {
                uv: domain.evaluate(0.5, 0.5),
                reason: NotProvedReason::Construction,
            }
        }),
    )
}

/// [`certify_no_fold`] by surface kind.
fn dispatch(
    k: &Kernel,
    s: &Surface,
    natural: &Extent2d,
    domain: &Extent2d,
) -> Result<FoldCertificate> {
    let centre = domain.evaluate(0.5, 0.5);
    let not = |reason| FoldCertificate::NotProved { uv: centre, reason };
    let (lo, hi) = (domain.min(), domain.max());
    if !(lo.x.is_finite()
        && lo.y.is_finite()
        && hi.x.is_finite()
        && hi.y.is_finite()
        && above(hi.x, lo.x)
        && above(hi.y, lo.y))
    {
        return Ok(not(NotProvedReason::DomainNotFinite));
    }
    match s.kind() {
        // `O + u sx X + v sy Y`: a constant normal
        SurfaceKind::Plane(_) => Ok(FoldCertificate::Proved),
        SurfaceKind::BSpline(b) => polynomial(k, b, natural, domain),
        SurfaceKind::BallPatch(bp) => polynomial(k, bp.net(), natural, domain),
        SurfaceKind::Sphere(_) | SurfaceKind::Cone(_) | SurfaceKind::Torus(_) => {
            analytic::certify(k, s, natural, domain)
        }
        // the normal depends on the generator parameter alone
        SurfaceKind::SurfOfExtrusion(_) | SurfaceKind::SurfOfRevolution(_) => {
            swept(k, s, natural, domain)
        }
        SurfaceKind::ProceduralBlend(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::SrfInVolume(_) => Ok(not(NotProvedReason::KindNotCertified)),
    }
}

/// The certificate of a swept surface: an extrusion `C(u) + v E` (generator parameter
/// `u`) or a revolution of `C(v)` about an axis (generator parameter `v`). Its normal is
/// a rigid copy of one that depends on the generator parameter alone: `C'(u) x E` for the
/// extrusion, and `M(v) = (a x (C(v) - A)) x C'(v)` turned by `u` about the axis (`A` a
/// point of it, `a` its unit direction; times `pi / 180` for degrees) for the revolution.
/// So a continuation along the sweep adds no normal, and no zero of one, the face does
/// not have: proved. A continuation of the generator is proved only for a line
/// generator, whose continuation is the line itself: the extrusion's normal is then
/// constant, and the revolution's `M` is linear in `v`, so `M . d` (`d = M` at the natural
/// domain's middle generator parameter) is positive over the merge domain's generator
/// interval when it is at that interval's ends and at the natural interval's ends inside
/// it (an undecided value is allowed only on the natural interval's closed ends: a pole
/// the face itself has). An extrusion's B-spline generator goes to [`extruded_net`].
/// Other generators' continuations are not proved.
///
/// # Errors
///
/// Those of [`extruded_net`].
fn swept(
    k: &Kernel,
    s: &Surface,
    natural: &Extent2d,
    domain: &Extent2d,
) -> Result<FoldCertificate> {
    let not = |reason| FoldCertificate::NotProved {
        uv: domain.evaluate(0.5, 0.5),
        reason,
    };
    let (generator, generator_is_u, revolution, extrusion) = match s.kind() {
        SurfaceKind::SurfOfExtrusion(e) => (e.gen_curve(), true, None, e.extrusion_vector()),
        SurfaceKind::SurfOfRevolution(r) => (
            r.gen_curve(),
            false,
            Some((r.position().origin(), r.position().z_axis())),
            Vec3::ZERO,
        ),
        _ => return Ok(not(NotProvedReason::KindNotCertified)),
    };
    let (d, n) = if generator_is_u {
        (domain.u_interval(), natural.u_interval())
    } else {
        (domain.v_interval(), natural.v_interval())
    };
    if d.min() >= n.min() && d.max() <= n.max() {
        return Ok(FoldCertificate::Proved);
    }
    let line = match (generator.map(Curve::kind), revolution) {
        (Some(CurveKind::Line(line)), _) => line,
        (Some(CurveKind::BSpline(c)), None) => {
            return extruded_net(k, c, extrusion, n, d);
        }
        _ => return Ok(not(NotProvedReason::GeneratorNotCertified)),
    };
    let Some((origin, axis)) = revolution else {
        // `C' x E`, the same everywhere
        return Ok(FoldCertificate::Proved);
    };
    // `M(t)` and the rounding bound of `M(t) . d`, from the evaluation's own terms
    // (`Line::evaluate_step`: `C = P + t V s`, `C' = V s`) in 1-norms `|.|_1`, which bound
    // every sum of products of components below. Each coordinate of `C` is within
    // `gamma_3 (|P|_1 + |t| |V|_1 |s|)` of `P + t V s` (two products and a sum), `C'`
    // within `gamma_1 |V|_1 |s|` of `V s`, and `C - A` adds `gamma_1 (|C|_1 + |A|_1)`. Each
    // cross product's coordinate is a difference of two products of these (`gamma_2` of
    // `|x|_1 |y|_1`) and the dot a sum of three products (`gamma_3`). Over the at most 11
    // rounding levels, the computed `M . d` is within `gamma_11 S |d|_1` of the exact
    // `M(t) . d` (Higham §3.1), `S = (|P|_1 + |t| |V|_1 |s| + |A|_1) |a|_1 |V|_1 |s|`;
    // `gamma_32` also covers the rounding of `S`, `|d|_1` and their product.
    let canonical = line.canonical_unit();
    let l1 = |v: Vec3| v.x.abs() + v.y.abs() + v.z.abs();
    let (p1, v1, s1) = (
        l1(canonical.point),
        l1(canonical.vector),
        canonical.scale.abs(),
    );
    let m = |t: f64| -> (Vec3, f64) {
        let pv = line.evaluate_step(t, 1);
        let (Some(&c), Some(&tangent)) = (pv.first(), pv.get(1)) else {
            return (Vec3::new(f64::NAN, f64::NAN, f64::NAN), f64::NAN);
        };
        let size = (p1 + t.abs() * v1 * s1 + l1(origin)) * l1(axis) * v1 * s1;
        (axis.cross(c - origin).cross(tangent), size)
    };
    let (middle, _) = m(0.5 * (n.min() + n.max()));
    let dlen = l1(middle);
    let mut ts = vec![d.min(), d.max()];
    ts.extend(
        [n.min(), n.max()]
            .into_iter()
            .filter(|&t| t > d.min() && t < d.max()),
    );
    for t in ts {
        let (mt, size) = m(t);
        let x = mt.dot(middle);
        let e = gamma(32) * size * dlen;
        if !(x.is_finite() && e.is_finite()) {
            return Ok(not(NotProvedReason::GeneratorNotFinite));
        }
        // positive, or undecided on the natural interval's closed ends
        let on_natural = t >= n.min() && t <= n.max();
        if !(above(x, e) || (on_natural && !above(-x, e))) {
            return Ok(not(NotProvedReason::GeneratorTurnsOrVanishes));
        }
    }
    Ok(FoldCertificate::Proved)
}

/// The certificate of an extrusion `C(u) + v E` of a B-spline generator past the
/// generator's natural interval `n`, over the merge domain's generator interval `d`.
/// The extrusion's evaluation continues `C` by its NURBS form's end spans (with
/// out-of-bounds evaluation on), as a B-spline surface's does, and its normal
/// `C'(u) x E` does not depend on `v`; so it is the certificate of the B-spline surface
/// whose net has, for each control point `c` of `C` (homogeneous, weight `w`), the
/// two columns `c` and `c + w E` (degree 1 in `v` over `[0, 1]`: exactly `C(u) + v E`),
/// over `n x [0, 1]` and `d x [0, 1]`. The second column's coordinates are rounded
/// sums, within `gamma_2 (|c| + |w| |E|)` of the exact ones: that is the net's
/// `extra` error ([`net_certificate`]). The normal does not depend on `v`, so the pieces
/// span `[0, 1]` in `v` and are split in `u` only.
fn extruded_net(
    k: &Kernel,
    c: &crate::nurbs::b_spline_curve::p1::BSplineCurve,
    e: Vec3,
    n: crate::geom::extent1d::Extent1d,
    d: crate::geom::extent1d::Extent1d,
) -> Result<FoldCertificate> {
    let not = |reason| FoldCertificate::NotProved {
        uv: Point2::new(0.5 * (d.min() + d.max()), 0.5),
        reason,
    };
    let Some(nurb) = c.gw_nurb() else {
        return Ok(not(NotProvedReason::NetForm));
    };
    let emax = e.x.abs().max(e.y.abs()).max(e.z.abs());
    let mut extra = 0.0_f64;
    let mut net = Vec::new();
    for &cp in nurb.cpts() {
        if cp.z == NOZ {
            return Ok(not(NotProvedReason::NetForm));
        }
        let w = if cp.w == NOW { 1.0 } else { cp.w };
        let moved = CPoint::new(cp.x + w * e.x, cp.y + w * e.y, cp.z + w * e.z, cp.w);
        let size = cp.x.abs().max(cp.y.abs()).max(cp.z.abs()) + w.abs() * emax;
        extra = extra.max(gamma(2) * size);
        net.push(vec![cp, moved]);
    }
    let surface = NlSurface::new(
        net,
        nurb.degree(),
        1,
        nurb.knots().to_vec(),
        vec![0.0, 0.0, 1.0, 1.0],
    )?;
    let natural = Extent2d::from_bounds(n.min(), 0.0, n.max(), 1.0);
    let domain = Extent2d::from_bounds(d.min(), 0.0, d.max(), 1.0);
    net_certificate(k, &surface, &natural, &domain, extra, false)
}

/// A polynomial with its two magnitude twins (see the module header).
#[derive(Debug, Clone)]
struct Net {
    v: Bernstein2,
    t: Bernstein2,
    te: Bernstein2,
}

impl Net {
    fn deriv_u(&self) -> Result<Self> {
        Ok(Self {
            v: self.v.deriv_u(),
            t: twin_deriv(&self.t, true)?,
            te: twin_deriv(&self.te, true)?,
        })
    }

    fn deriv_v(&self) -> Result<Self> {
        Ok(Self {
            v: self.v.deriv_v(),
            t: twin_deriv(&self.t, false)?,
            te: twin_deriv(&self.te, false)?,
        })
    }

    fn mul(&self, o: &Self, b: &Binomials) -> Result<Self> {
        Ok(Self {
            v: self.v.mul(&o.v, b)?,
            t: self.t.mul(&o.t, b)?,
            te: self.te.mul(&o.te, b)?,
        })
    }

    fn sub(&self, o: &Self, b: &Binomials) -> Result<Self> {
        Ok(Self {
            v: self.v.sub(&o.v, b)?,
            t: self.t.add(&o.t, b)?,
            te: self.te.add(&o.te, b)?,
        })
    }

    /// `x self + y o + z p`.
    fn combine(nets: &[Self; 3], d: Vec3, b: &Binomials) -> Result<Self> {
        let [nx, ny, nz] = nets;
        Ok(Self {
            v: nx
                .v
                .scale(d.x)
                .add(&ny.v.scale(d.y), b)?
                .add(&nz.v.scale(d.z), b)?,
            t: nx
                .t
                .scale(d.x.abs())
                .add(&ny.t.scale(d.y.abs()), b)?
                .add(&nz.t.scale(d.z.abs()), b)?,
            te: nx
                .te
                .scale(d.x.abs())
                .add(&ny.te.scale(d.y.abs()), b)?
                .add(&nz.te.scale(d.z.abs()), b)?,
        })
    }

    /// The bound `E` of each coefficient (see the module header), for `k` levels. A twin
    /// that overflowed gives a bound that is not finite (`inf - inf` stays NaN: a
    /// negative difference, the twins' rounding, is the only one taken as `0`).
    fn bounds(&self, k: usize) -> Vec<f64> {
        let g = 3.0 * gamma(2 * k);
        self.t
            .coefficients()
            .iter()
            .zip(self.te.coefficients())
            .map(|(&t, &te)| {
                let gap = te - t;
                let gap = if gap < 0.0 { 0.0 } else { gap };
                (gap + g * te) * (1.0 + gamma(4))
            })
            .collect()
    }
}

/// The derivative of a twin: `n (x + y)` for `n (y - x)`.
fn twin_deriv(t: &Bernstein2, in_u: bool) -> Result<Bernstein2> {
    let (du, dv) = t.degrees();
    let c = t.coefficients();
    let at = |i: usize, j: usize| c.get(i * (dv + 1) + j).copied().unwrap_or(0.0);
    if in_u {
        if du == 0 {
            return Bernstein2::new(0, dv, vec![0.0; dv + 1]);
        }
        let n = du as f64;
        let mut out = Vec::new();
        for i in 0..du {
            for j in 0..=dv {
                out.push(n * (at(i, j) + at(i + 1, j)));
            }
        }
        Bernstein2::new(du - 1, dv, out)
    } else {
        if dv == 0 {
            return Bernstein2::new(du, 0, vec![0.0; du + 1]);
        }
        let n = dv as f64;
        let mut out = Vec::new();
        for i in 0..=du {
            for j in 0..dv {
                out.push(n * (at(i, j) + at(i, j + 1)));
            }
        }
        Bernstein2::new(du, dv - 1, out)
    }
}

/// The Bernstein coefficients on `[a, b]` of the polynomial with coefficients `c` on
/// `[0, 1]` (any `a < b`: blossoming extrapolates): coefficient `k` is the blossom at
/// `n - k` copies of `a` and `k` of `b`. With `twin`, the same on magnitudes, with the
/// weights `|1 - t|` and `|t|`.
fn restrict_1d(c: &[f64], a: f64, b: f64, twin: bool) -> Vec<f64> {
    let n = c.len().saturating_sub(1);
    (0..=n)
        .map(|k| {
            let mut cur = c.to_vec();
            for step in 0..n {
                let t = if step < n - k { a } else { b };
                let (w0, w1) = if twin {
                    ((1.0 - t).abs(), t.abs())
                } else {
                    (1.0 - t, t)
                };
                cur = cur
                    .windows(2)
                    .map(|w| match *w {
                        [x, y] => w0 * x + w1 * y,
                        _ => 0.0,
                    })
                    .collect();
            }
            cur.first().copied().unwrap_or(0.0)
        })
        .collect()
}

/// [`restrict_1d`] of a tensor polynomial to `[u0, u1] x [v0, v1]`.
fn restrict_2d(p: &Bernstein2, r: [f64; 4], twin: bool) -> Result<Bernstein2> {
    let [u0, u1, v0, v1] = r;
    let (du, dv) = p.degrees();
    let c = p.coefficients();
    // row `i` holds the coefficients `i (dv + 1) ..= i (dv + 1) + dv`
    let rows: Vec<Vec<f64>> = (0..=du)
        .map(|i| {
            let row: Vec<f64> = (0..=dv)
                .map(|j| c.get(i * (dv + 1) + j).copied().unwrap_or(0.0))
                .collect();
            restrict_1d(&row, v0, v1, twin)
        })
        .collect();
    let mut out = vec![0.0; (du + 1) * (dv + 1)];
    for j in 0..=dv {
        let column: Vec<f64> = rows
            .iter()
            .map(|row| row.get(j).copied().unwrap_or(0.0))
            .collect();
        for (i, x) in restrict_1d(&column, u0, u1, twin).into_iter().enumerate() {
            if let Some(slot) = out.get_mut(i * (dv + 1) + j) {
                *slot = x;
            }
        }
    }
    Bernstein2::new(du, dv, out)
}

/// One Bezier patch of the NURBS form: its homogeneous coordinates (`x, y, z, w`) and
/// their magnitudes, and its knot intervals.
struct Patch {
    h: [Bernstein2; 4],
    abs: [Bernstein2; 4],
    u: [f64; 2],
    v: [f64; 2],
}

/// The polynomial pieces' common data.
struct Form {
    /// `patches[i][j]`: u index `i`, v index `j`.
    patches: Vec<Vec<Patch>>,
    rational: bool,
    degrees: (usize, usize),
    /// The extraction's error per coefficient.
    e0: f64,
    /// The rounding levels of a piece's `N . d`.
    levels: usize,
    /// The most normals computed ([`MAX_PIECES`] scaled by the degrees' work).
    max_pieces: usize,
    /// The `u` and `v` knots of multiplicity at least the degree (`C0` lines where they
    /// fall between two spans).
    c0_lines: (Vec<f64>, Vec<f64>),
    /// Whether the normal depends on `v`: if not (an extrusion's net), the pieces span
    /// the merge domain in `v` and are split in `u` only.
    split_v: bool,
}

/// The certificate of a B-spline net's continuation.
fn polynomial(
    k: &Kernel,
    b: &BSplineSurface,
    natural: &Extent2d,
    domain: &Extent2d,
) -> Result<FoldCertificate> {
    let centre = domain.evaluate(0.5, 0.5);
    let not = |reason| FoldCertificate::NotProved { uv: centre, reason };
    if b.form() == BSplineSurfaceForm::Polynomial {
        return Ok(not(NotProvedReason::NetForm));
    }
    let Some(nurb) = b.gw_nurb() else {
        return Ok(not(NotProvedReason::NetForm));
    };
    net_certificate(k, nurb, natural, domain, 0.0, true)
}

/// The certificate of the NURBS net `nurb`'s continuation; `extra` bounds, per
/// coefficient, how far the net may be from the exact net of the surface certified
/// (added to the extraction's error `e0`: knot insertion takes convex combinations, so
/// it moves no coefficient by more than the largest input error). `split_v`: whether
/// the normal depends on `v` ([`Form::split_v`]).
fn net_certificate(
    k: &Kernel,
    nurb: &NlSurface,
    natural: &Extent2d,
    domain: &Extent2d,
    extra: f64,
    split_v: bool,
) -> Result<FoldCertificate> {
    let centre = domain.evaluate(0.5, 0.5);
    let not = |reason| FoldCertificate::NotProved { uv: centre, reason };
    let (p, q) = (nurb.p(), nurb.q());
    let mut rational = false;
    let mut net_size = 0.0_f64;
    for row in nurb.rows() {
        for c in row {
            if c.z == NOZ {
                return Ok(not(NotProvedReason::NetForm));
            }
            let w = if c.w == NOW {
                1.0
            } else {
                rational = true;
                c.w
            };
            if !(c.x.is_finite()
                && c.y.is_finite()
                && c.z.is_finite()
                && above(w, 0.0)
                && w.is_finite())
            {
                return Ok(not(NotProvedReason::NetNotFinite));
            }
            net_size = net_size.max(c.x.abs()).max(c.y.abs()).max(c.z.abs()).max(w);
        }
    }
    let (du, dv) = if rational {
        (4 * p, 4 * q)
    } else {
        (2 * p, 2 * q)
    };
    if du > MAX_DEGREE || dv > MAX_DEGREE || p == 0 || q == 0 {
        return Ok(not(NotProvedReason::Degrees));
    }
    // an interior knot below full multiplicity is where the extraction inserts
    let inserts = |knots: &[f64], degree: usize| {
        let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
            return false;
        };
        let mut i = 0;
        while let Some(&u) = knots.get(i) {
            let mut m = 1;
            while knots.get(i + m) == Some(&u) {
                m += 1;
            }
            if u > first && u < last && m < degree {
                return true;
            }
            i += m;
        }
        false
    };
    // the knots of multiplicity at least the degree: there the net is at most `C0`, and
    // its normal can jump (filtered to the lines between spans in `certify_pieces`)
    let c0 = |knots: &[f64], degree: usize| {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(&u) = knots.get(i) {
            let mut m = 1;
            while knots.get(i + m) == Some(&u) {
                m += 1;
            }
            if m >= degree {
                out.push(u);
            }
            i += m;
        }
        out
    };
    let c0_lines = (c0(nurb.u_knots(), p), c0(nurb.v_knots(), q));
    let extraction = if inserts(nurb.u_knots(), p) || inserts(nurb.v_knots(), q) {
        gamma(16 * (p + q)) * net_size
    } else {
        0.0
    };
    // (rounded up: the sum's own rounding)
    let e0 = if extra > 0.0 {
        (extraction + extra) * (1.0 + gamma(1))
    } else {
        extraction
    };
    if !e0.is_finite() {
        return Ok(not(NotProvedReason::NetNotFinite));
    }
    let mut patches = Vec::new();
    for column in decompose_to_bez(nurb)? {
        let mut out = Vec::new();
        for patch in column {
            let (pu, pv) = patch.degrees();
            let (uk, vk) = (patch.u_knots(), patch.v_knots());
            let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
                (uk.first(), uk.last(), vk.first(), vk.last())
            else {
                return Ok(not(NotProvedReason::Patches));
            };
            if !(above(u1, u0) && above(v1, v0)) {
                return Ok(not(NotProvedReason::Patches));
            }
            let mut c: [Vec<f64>; 4] = Default::default();
            for row in patch.rows() {
                for cp in row {
                    let w = if cp.w == NOW { 1.0 } else { cp.w };
                    for (dst, x) in c.iter_mut().zip([cp.x, cp.y, cp.z, w]) {
                        dst.push(x);
                    }
                }
            }
            let [cx, cy, cz, cw] = c;
            let abs = |v: &[f64]| v.iter().map(|x| x.abs()).collect::<Vec<_>>();
            out.push(Patch {
                abs: [
                    Bernstein2::new(pu, pv, abs(&cx))?,
                    Bernstein2::new(pu, pv, abs(&cy))?,
                    Bernstein2::new(pu, pv, abs(&cz))?,
                    Bernstein2::new(pu, pv, abs(&cw))?,
                ],
                h: [
                    Bernstein2::new(pu, pv, cx)?,
                    Bernstein2::new(pu, pv, cy)?,
                    Bernstein2::new(pu, pv, cz)?,
                    Bernstein2::new(pu, pv, cw)?,
                ],
                u: [u0, u1],
                v: [v0, v1],
            });
        }
        patches.push(out);
    }
    if patches.first().is_none_or(Vec::is_empty) {
        return Ok(not(NotProvedReason::Patches));
    }
    // NLib's span search skips an end span narrower than `1e-12` (it continues the next
    // one past the knot range), which the patches here would not: refused
    let narrow = |a: Option<[f64; 2]>| a.is_none_or(|s| !above(s[1] - s[0], 1e-12));
    let column = patches.first().map(Vec::as_slice).unwrap_or_default();
    if narrow(patches.first().and_then(|c| c.first()).map(|x| x.u))
        || narrow(patches.last().and_then(|c| c.first()).map(|x| x.u))
        || narrow(column.first().map(|x| x.v))
        || narrow(column.last().map(|x| x.v))
    {
        return Ok(not(NotProvedReason::Patches));
    }
    // rounding levels: restriction, derivative, products and differences, the dot
    let product = |a: (usize, usize)| (a.0 + 1) * (a.1 + 1) + 7;
    let mut levels = 3 * (p + q) + 2;
    if rational {
        levels += product((p, q)) + 1 + product((2 * p, 2 * q)) + 1;
    } else {
        levels += product((p, q)) + 1;
    }
    levels += 3 + 4;
    let (ep, eq) = if rational { (2 * p, 2 * q) } else { (p, q) };
    let cost = ((ep + 1) * (eq + 1)).saturating_pow(2);
    let form = Form {
        patches,
        rational,
        degrees: (p, q),
        e0,
        levels,
        max_pieces: MAX_PIECES
            .saturating_mul(UNIT_WORK)
            .checked_div(cost)
            .unwrap_or(0),
        c0_lines,
        split_v,
    };
    certify_pieces(k, &form, natural, domain)
}

/// A rectangle `[x0, x1] x [y0, y1]` of STEP parameters.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: [f64; 2],
    y: [f64; 2],
}

impl Rect {
    fn centre(&self) -> Point2 {
        Point2::new(0.5 * (self.x[0] + self.x[1]), 0.5 * (self.y[0] + self.y[1]))
    }

    /// Whether it meets the closed rectangle `e`.
    fn touches(&self, e: &Extent2d) -> bool {
        let (lo, hi) = (e.min(), e.max());
        self.x[0] <= hi.x && self.x[1] >= lo.x && self.y[0] <= hi.y && self.y[1] >= lo.y
    }

    /// Its quarters, or (without `split_v`) its halves in `x`.
    fn splits(&self, split_v: bool) -> Vec<Self> {
        let (xm, ym) = (0.5 * (self.x[0] + self.x[1]), 0.5 * (self.y[0] + self.y[1]));
        if !split_v {
            return vec![
                Self {
                    x: [self.x[0], xm],
                    y: self.y,
                },
                Self {
                    x: [xm, self.x[1]],
                    y: self.y,
                },
            ];
        }
        vec![
            Self {
                x: [self.x[0], xm],
                y: [self.y[0], ym],
            },
            Self {
                x: [xm, self.x[1]],
                y: [self.y[0], ym],
            },
            Self {
                x: [self.x[0], xm],
                y: [ym, self.y[1]],
            },
            Self {
                x: [xm, self.x[1]],
                y: [ym, self.y[1]],
            },
        ]
    }
}

/// How a computed `N . d` compares with its bounds ([`verdict`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Every coefficient holds its bound.
    Holds,
    /// Every coefficient has the right sign, but some not by more than their bound.
    Precision,
    /// Some coefficient has the wrong sign.
    Fails,
    /// Some coefficient or bound is not finite.
    NotFinite,
}

/// The cuts of `[a, b]` at the values of `breaks` strictly inside it.
fn cuts(a: f64, b: f64, breaks: &[f64]) -> Vec<[f64; 2]> {
    let mut points = vec![a];
    let mut inner: Vec<f64> = breaks
        .iter()
        .copied()
        .filter(|&x| above(x, a) && above(b, x))
        .collect();
    inner.sort_by(f64::total_cmp);
    inner.dedup();
    points.extend(inner);
    points.push(b);
    points
        .windows(2)
        .filter_map(|w| match *w {
            [x, y] => Some([x, y]),
            _ => None,
        })
        .collect()
}

/// The index of the interval of `spans` holding `x`, the end ones taking everything
/// past them (the end spans continue the surface).
fn span_of(spans: &[[f64; 2]], x: f64) -> usize {
    spans
        .iter()
        .position(|s| x < s[1])
        .unwrap_or_else(|| spans.len().saturating_sub(1))
}

/// The normal's homogeneous direction (`H_u x H_v`, or `A x B` for a rational form) on
/// the piece `r` of `patch`; `None` when the weight is not proved positive there.
fn normal_on(form: &Form, patch: &Patch, r: &Rect, b: &Binomials) -> Result<Option<[Net; 3]>> {
    let (su, sv) = (patch.u[1] - patch.u[0], patch.v[1] - patch.v[0]);
    let local = [
        (r.x[0] - patch.u[0]) / su,
        (r.x[1] - patch.u[0]) / su,
        (r.y[0] - patch.v[0]) / sv,
        (r.y[1] - patch.v[0]) / sv,
    ];
    // Each coordinate on the whole patch, and its derivatives taken there and only then
    // restricted to the piece: the restriction of a derivative is the piece's derivative
    // divided by the piece's (positive) widths, so the directions are kept, and a thin
    // piece keeps its precision (a derivative of a thin piece's coefficients would be a
    // difference of nearly equal numbers, all of its size lost to their rounding).
    let restrict = |n: &Net| -> Result<Net> {
        Ok(Net {
            v: restrict_2d(&n.v, local, false)?,
            t: restrict_2d(&n.t, local, true)?,
            te: restrict_2d(&n.te, local, true)?,
        })
    };
    let mut h: Vec<[Net; 3]> = Vec::with_capacity(4);
    for (value, abs) in patch.h.iter().zip(&patch.abs) {
        let te = if form.e0 > 0.0 {
            let (pu, pv) = abs.degrees();
            let raised: Vec<f64> = abs.coefficients().iter().map(|x| x + form.e0).collect();
            Bernstein2::new(pu, pv, raised)?
        } else {
            abs.clone()
        };
        let whole = Net {
            v: value.clone(),
            t: abs.clone(),
            te,
        };
        h.push([
            restrict(&whole)?,
            restrict(&whole.deriv_u()?)?,
            restrict(&whole.deriv_v()?)?,
        ]);
    }
    let Ok([x, y, z, w]) = <[[Net; 3]; 4]>::try_from(h) else {
        return Ok(None);
    };
    let (hu, hv);
    if form.rational {
        let [w, wu, wv] = &w;
        // the weight positive on the piece
        let restriction_levels = 3 * (form.degrees.0 + form.degrees.1) + 4;
        let bound = w.bounds(restriction_levels);
        if w.v
            .coefficients()
            .iter()
            .zip(&bound)
            .any(|(&c, &e)| !above(c, e))
        {
            return Ok(None);
        }
        let a = |[c, cu, _]: &[Net; 3]| -> Result<Net> { w.mul(cu, b)?.sub(&wu.mul(c, b)?, b) };
        let bb = |[c, _, cv]: &[Net; 3]| -> Result<Net> { w.mul(cv, b)?.sub(&wv.mul(c, b)?, b) };
        hu = [a(&x)?, a(&y)?, a(&z)?];
        hv = [bb(&x)?, bb(&y)?, bb(&z)?];
    } else {
        let [[_, xu, xv], [_, yu, yv], [_, zu, zv]] = [x, y, z];
        hu = [xu, yu, zu];
        hv = [xv, yv, zv];
    }
    let [ax, ay, az] = &hu;
    let [bx, by, bz] = &hv;
    Ok(Some([
        ay.mul(bz, b)?.sub(&az.mul(by, b)?, b)?,
        az.mul(bx, b)?.sub(&ax.mul(bz, b)?, b)?,
        ax.mul(by, b)?.sub(&ay.mul(bx, b)?, b)?,
    ]))
}

/// How the computed `N . d` on a piece compares with its bounds `E`: with `strict`,
/// whether every coefficient is above its `E` (so the exact `N . d > 0` on the piece);
/// else whether none is below `-E` (so the exact `N . d >= -2 E`: the computed
/// coefficient is within `E` of the exact one). A coefficient or bound that is not
/// finite (an overflow, NaN) decides nothing.
fn verdict(f: &Net, levels: usize, strict: bool) -> Verdict {
    let bound = f.bounds(levels);
    let mut out = Verdict::Holds;
    for (&x, &e) in f.v.coefficients().iter().zip(&bound) {
        if !(x.is_finite() && e.is_finite()) {
            return Verdict::NotFinite;
        }
        let (holds, sign) = if strict {
            (above(x, e), above(x, 0.0))
        } else {
            (!above(-x, e), !above(-x, 0.0))
        };
        if !holds {
            out = if sign && out != Verdict::Fails {
                Verdict::Precision
            } else {
                Verdict::Fails
            };
        }
    }
    out
}

/// The pieces of the continuation (see the module header).
fn certify_pieces(
    k: &Kernel,
    form: &Form,
    natural: &Extent2d,
    domain: &Extent2d,
) -> Result<FoldCertificate> {
    let b = Binomials::new();
    let uspans: Vec<[f64; 2]> = form
        .patches
        .iter()
        .filter_map(|c| c.first().map(|p| p.u))
        .collect();
    let vspans: Vec<[f64; 2]> = form
        .patches
        .first()
        .map(|c| c.iter().map(|p| p.v).collect())
        .unwrap_or_default();
    let (nlo, nhi) = (natural.min(), natural.max());
    let mut ubreaks: Vec<f64> = uspans.iter().flat_map(|s| *s).collect();
    ubreaks.extend([nlo.x, nhi.x]);
    let mut vbreaks: Vec<f64> = vspans.iter().flat_map(|s| *s).collect();
    vbreaks.extend([nlo.y, nhi.y]);
    let (lo, hi) = (domain.min(), domain.max());
    // (one cell in `v` when the normal does not depend on `v`)
    let vcells = if form.split_v { GRID } else { 1 };
    let at = |a: f64, b: f64, i: usize, cells: usize| {
        if i == cells {
            b
        } else {
            a + (b - a) * (i as f64 / cells as f64)
        }
    };
    // the `C0` knot lines strictly between the first span's start and the last's end
    let between = |lines: &[f64], spans: &[[f64; 2]]| -> Vec<f64> {
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            return Vec::new();
        };
        lines
            .iter()
            .copied()
            .filter(|&x| above(x, first[0]) && above(last[1], x))
            .collect()
    };
    let (c0u, c0v) = (
        between(&form.c0_lines.0, &uspans),
        between(&form.c0_lines.1, &vspans),
    );
    let inside =
        |r: &Rect| r.x[0] >= nlo.x && r.x[1] <= nhi.x && r.y[0] >= nlo.y && r.y[1] <= nhi.y;
    // a piece cut at knot lines and the natural domain's sides lies in one patch's
    // span (or past an end span, which continues the surface)
    let patch_of = |r: &Rect| {
        let m = r.centre();
        form.patches
            .get(span_of(&uspans, m.x))
            .and_then(|col| col.get(span_of(&vspans, m.y)))
    };
    // the normal on the piece `r`: one unit of the work bound, after a cancel poll
    let work = core::cell::Cell::new(0_usize);
    let normal = |r: &Rect| -> Result<core::result::Result<[Net; 3], NotProvedReason>> {
        k.check_cancelled(OP)?;
        work.set(work.get().saturating_add(1));
        if work.get() > form.max_pieces {
            return Ok(Err(NotProvedReason::WorkBound));
        }
        let Some(patch) = patch_of(r) else {
            return Ok(Err(NotProvedReason::Construction));
        };
        Ok(normal_on(form, patch, r, &b)?.ok_or(NotProvedReason::WeightNotPositive))
    };
    // `None` when the computed `N . d` holds its bound ([`verdict`]); else why not,
    // `failed` when a coefficient has the wrong sign
    let check = |n: &[Net; 3], d: Vec3, strict: bool, failed: NotProvedReason| {
        Ok::<_, crate::error::SolidError>(
            match verdict(&Net::combine(n, d, &b)?, form.levels, strict) {
                Verdict::Holds => None,
                Verdict::Precision => Some(NotProvedReason::Precision),
                Verdict::NotFinite => Some(NotProvedReason::NotFinite),
                Verdict::Fails => Some(failed),
            },
        )
    };
    // `None` when the computed `N . d >= -E` on every piece of the strip `s` (cut at
    // the knot lines and the natural domain's sides); else why not
    let strip = |s: &Rect, d: Vec3, failed: NotProvedReason| -> Result<Option<NotProvedReason>> {
        for x in cuts(s.x[0], s.x[1], &ubreaks) {
            for y in cuts(s.y[0], s.y[1], &vbreaks) {
                let n = match normal(&Rect { x, y })? {
                    Ok(n) => n,
                    Err(why) => return Ok(Some(why)),
                };
                if let Some(why) = check(&n, d, false, failed)? {
                    return Ok(Some(why));
                }
            }
        }
        Ok(None)
    };
    // the strips of `r`'s width across each `C0` knot line one of its sides lies on,
    // inside the merge domain
    let knot_strips = |r: &Rect| -> Vec<Rect> {
        let mut out = Vec::new();
        let (w, h) = (r.x[1] - r.x[0], r.y[1] - r.y[0]);
        for &x in &c0u {
            if x == r.x[1] && above(hi.x, x) {
                out.push(Rect {
                    x: [x, (x + w).min(hi.x)],
                    y: r.y,
                });
            }
            if x == r.x[0] && above(x, lo.x) {
                out.push(Rect {
                    x: [(x - w).max(lo.x), x],
                    y: r.y,
                });
            }
        }
        for &y in &c0v {
            if y == r.y[1] && above(hi.y, y) {
                out.push(Rect {
                    x: r.x,
                    y: [y, (y + h).min(hi.y)],
                });
            }
            if y == r.y[0] && above(y, lo.y) {
                out.push(Rect {
                    x: r.x,
                    y: [(y - h).max(lo.y), y],
                });
            }
        }
        out
    };
    // the mirror of `r` across the natural domain's sides it touches, inside it
    let mirror = |r: &Rect| -> Option<Rect> {
        let side = |x: [f64; 2], n0: f64, n1: f64| -> Option<[f64; 2]> {
            let w = x[1] - x[0];
            let m = if x[1] <= n0 {
                [n0, (n0 + w).min(n1)]
            } else if x[0] >= n1 {
                [(n1 - w).max(n0), n1]
            } else {
                x
            };
            above(m[1], m[0]).then_some(m)
        };
        Some(Rect {
            x: side(r.x, nlo.x, nhi.x)?,
            y: side(r.y, nlo.y, nhi.y)?,
        })
    };
    let mut stack: Vec<(Rect, usize)> = Vec::new();
    for i in 0..GRID {
        for j in 0..vcells {
            for x in cuts(
                at(lo.x, hi.x, i, GRID),
                at(lo.x, hi.x, i + 1, GRID),
                &ubreaks,
            ) {
                for y in cuts(
                    at(lo.y, hi.y, j, vcells),
                    at(lo.y, hi.y, j + 1, vcells),
                    &vbreaks,
                ) {
                    let r = Rect { x, y };
                    if !inside(&r) {
                        stack.push((r, 0));
                    }
                }
            }
        }
    }
    while let Some((r, depth)) = stack.pop() {
        let touching = r.touches(natural);
        let last = depth >= MAX_SPLITS;
        let failure = match normal(&r)? {
            Err(why) => Some(why),
            Ok(n) => {
                // the direction: the normal at the piece's centre (the restriction's
                // derivatives are the patch's scaled by the piece's positive widths, so
                // the direction at its centre (0.5, 0.5) is kept)
                let d = Vec3::new(
                    n[0].v.eval(0.5, 0.5),
                    n[1].v.eval(0.5, 0.5),
                    n[2].v.eval(0.5, 0.5),
                );
                if !(d.x.is_finite() && d.y.is_finite() && d.z.is_finite()) {
                    Some(NotProvedReason::NotFinite)
                } else if d == Vec3::ZERO {
                    Some(NotProvedReason::NormalZero)
                } else {
                    // away from the natural domain: a normal that never vanishes;
                    // touching it, the same unless the piece is down to the last split,
                    // and then a normal that does not turn back against `d`
                    let mut why = check(&n, d, !touching || !last, NotProvedReason::NormalZero)?;
                    // a touching piece's mirror inside the natural domain must not turn
                    // back against `d` either: no reversal across the natural domain's
                    // side (no mirror, a natural domain without width: nothing proved)
                    if why.is_none() && touching {
                        why = match mirror(&r) {
                            Some(m) => strip(&m, d, NotProvedReason::TurnBackAcrossSide)?,
                            None => Some(NotProvedReason::TurnBackAcrossSide),
                        };
                    }
                    // nor across a `C0` knot line, where the normal may jump
                    if why.is_none() {
                        for s in knot_strips(&r) {
                            why = strip(&s, d, NotProvedReason::TurnBackAcrossKnot)?;
                            if why.is_some() {
                                break;
                            }
                        }
                    }
                    why
                }
            }
        };
        let Some(why) = failure else {
            continue;
        };
        if last || why == NotProvedReason::WorkBound {
            return Ok(FoldCertificate::NotProved {
                uv: r.centre(),
                reason: why,
            });
        }
        for q in r.splits(form.split_v) {
            stack.push((q, depth + 1));
        }
    }
    Ok(FoldCertificate::Proved)
}

/// Closed-form certificates of the analytic kinds. In their STEP parameters (`u` the
/// angle about the axis, `v` the latitude, height or minor angle; angles in degrees)
/// `Du x Dv = lambda(v) n(u, v)` with `n` a normal field that never vanishes:
/// - sphere: `Du = r cos(v) t`, `Dv = r (-sin(v) rho + cos(v) z)`: `lambda = r^2 cos(v)`;
/// - cone: `Du = r(v) t`, `Dv = r' rho + z` (`|Dv| >= 1`): `lambda = r(v)`, linear;
/// - torus: `Du = (R + r cos(v)) t`, `Dv = r (-sin(v) rho + cos(v) z)`:
///   `lambda = r (R + r cos(v))`.
///
/// So the continuation folds exactly where `lambda` changes sign; `lambda` is shown not
/// to vanish over the merge domain except on the natural domain's closed `v` interval
/// (a pole or apex the face itself has). Its rounding is bounded by `gamma_16` of the
/// terms' sizes, a generous count of the operations in each closed form.
pub mod analytic {
    use super::{FoldCertificate, NotProvedReason};
    use crate::error::Result;
    use crate::geom::certified::above;
    use crate::geom::extent2d::Extent2d;
    use crate::kernel::Kernel;
    use crate::math::det;
    use crate::nurbs::math::gamma;
    use crate::surface::surface::{Surface, SurfaceKind};

    /// The certificate of a sphere, cone or torus; nothing is proved for other kinds.
    ///
    /// # Errors
    ///
    /// None (a `Result` for the dispatcher's signature).
    pub fn certify(
        _k: &Kernel,
        s: &Surface,
        natural: &Extent2d,
        domain: &Extent2d,
    ) -> Result<FoldCertificate> {
        let not = |reason| FoldCertificate::NotProved {
            uv: domain.evaluate(0.5, 0.5),
            reason,
        };
        let (v0, v1) = (domain.min().y, domain.max().y);
        let (n0, n1) = (natural.min().y, natural.max().y);
        let on_natural = |v: f64| v >= n0 && v <= n1;
        // `lambda` at the ends and, for the trigonometric forms, at the extremes of
        // `cos(v)` inside the interval; each with its rounding bound
        let samples: Vec<(f64, f64, f64)> = match s.kind() {
            SurfaceKind::Sphere(sp) => {
                let r2 = sp.radius() * sp.radius();
                cos_extremes(v0, v1)
                    .into_iter()
                    .map(|(v, c)| (v, r2 * c, gamma(16) * r2))
                    .collect()
            }
            SurfaceKind::Cone(c) => [v0, v1]
                .into_iter()
                .map(|v| {
                    let r = c.radius(v);
                    let size = c.radius(0.0).abs() + (c.radius(v) - c.radius(0.0)).abs();
                    (v, r, gamma(16) * size)
                })
                .collect(),
            SurfaceKind::Torus(t) => {
                let (big, small) = (t.major_radius(), t.minor_radius());
                cos_extremes(v0, v1)
                    .into_iter()
                    .map(|(v, c)| {
                        (
                            v,
                            small * (big + small * c),
                            gamma(16) * small.abs() * (big.abs() + small.abs()),
                        )
                    })
                    .collect()
            }
            _ => return Ok(not(NotProvedReason::KindNotCertified)),
        };
        // the sign of `lambda` where it is decided; an undecided value only on the
        // natural domain's closed `v` interval
        let mut sign = 0.0_f64;
        for &(v, x, e) in &samples {
            if !(x.is_finite() && e.is_finite()) {
                return Ok(not(NotProvedReason::AnalyticNotFinite));
            }
            if above(x.abs(), e) {
                let here = x.signum();
                if sign != 0.0 && here != sign {
                    return Ok(not(NotProvedReason::AnalyticTurnsBack));
                }
                sign = here;
            } else if !on_natural(v) {
                return Ok(not(NotProvedReason::AnalyticVanishes));
            }
        }
        if sign == 0.0 {
            return Ok(not(NotProvedReason::AnalyticUndecided));
        }
        Ok(FoldCertificate::Proved)
    }

    /// `v0`, `v1` and every multiple of 180 degrees between them (the extremes of
    /// `cos`), with `cos` at each: every value of `cos` on `[v0, v1]` lies between
    /// the least and the greatest of them, and a sign change of `a + b cos` shows at
    /// them. At most 64 extremes (an interval of more than 32 turns is refused by the
    /// caller seeing an undecided sign).
    fn cos_extremes(v0: f64, v1: f64) -> Vec<(f64, f64)> {
        let cos = |v: f64| det::cos(v * core::f64::consts::PI / 180.0);
        let mut out = vec![(v0, cos(v0)), (v1, cos(v1))];
        let turns = (v1 - v0) / 180.0;
        if turns.is_nan() || turns > 64.0 {
            // too many turns (or no width): an undecided entry off the natural domain
            out.push((f64::NAN, 0.0));
            return out;
        }
        let first = (v0 / 180.0).ceil();
        let mut m = first;
        while m * 180.0 < v1 && out.len() < 66 {
            // the exact extreme: +-1 at a multiple of 180 degrees
            // (`m` is an integer: halving and doubling it is exact)
            let c = if (0.5 * m).floor() * 2.0 == m {
                1.0
            } else {
                -1.0
            };
            out.push((m * 180.0, c));
            m += 1.0;
        }
        if m * 180.0 < v1 {
            // too many turns: an undecided entry off the natural domain
            out.push((f64::NAN, 0.0));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A constant `N . d` (degree 0) with twins `t`, `te`.
    fn net(x: f64, t: f64, te: f64) -> Net {
        let c = |v| Bernstein2::new(0, 0, vec![v]).unwrap();
        Net {
            v: c(x),
            t: c(t),
            te: c(te),
        }
    }

    /// A coefficient or bound that is not finite decides nothing, strict or not
    /// (#17724 review: `!above(-x, e)` held for a NaN `x` or an infinite `e`, and
    /// `(te - t).max(0.0)` turned an overflowed `inf - inf` into `0`).
    #[test]
    fn a_coefficient_or_bound_not_finite_proves_nothing() {
        for strict in [true, false] {
            for f in [
                net(f64::NAN, 1.0, 1.0),
                net(1.0, 1.0, f64::INFINITY),
                net(f64::INFINITY, f64::INFINITY, f64::INFINITY),
                net(1.0, f64::NAN, 1.0),
            ] {
                assert_eq!(verdict(&f, 10, strict), Verdict::NotFinite, "{f:?}");
            }
        }
        let overflowed = net(1.0, f64::INFINITY, f64::INFINITY);
        assert!(overflowed.bounds(10).iter().all(|e| e.is_nan()));
        // finite: decided as before
        assert_eq!(verdict(&net(1.0, 1.0, 1.0), 10, true), Verdict::Holds);
        assert_eq!(verdict(&net(-1e-20, 1.0, 1.0), 10, false), Verdict::Holds);
        assert_eq!(verdict(&net(1e-20, 1.0, 1.0), 10, true), Verdict::Precision);
        assert_eq!(verdict(&net(-1.0, 1.0, 1.0), 10, true), Verdict::Fails);
        assert_eq!(verdict(&net(-1.0, 1.0, 1.0), 10, false), Verdict::Fails);
    }
}
