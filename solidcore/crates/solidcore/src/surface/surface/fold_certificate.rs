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
//!
//! **What is proved.** A fold of the continuation is where its normal `N = Du x Dv`
//! vanishes (the map stops being an immersion there), or where `N` reverses across the
//! natural domain's side (the continuation turns back over the face at its edge, with
//! `N` zero along that edge). The certificate excludes both:
//! - every piece of the merge domain outside the closed natural domain has `N . d > E`
//!   on all of it for one direction `d` (so `N` never vanishes there), `E` the bound of
//!   the arithmetic below;
//! - a piece that touches the natural domain, once split down to the last level, has
//!   `N . d >= -E` on it instead (a zero of `N` on the natural domain's side, a
//!   degenerate edge or corner of the surface's own domain, is allowed);
//! - for every piece that touches the natural domain, `N . d >= -E` also holds on its
//!   mirror across the sides it touches, inside the natural domain, with the same `d`:
//!   `N` does not reverse across the side.
//!
//! The pieces start as the cells of the fold rule's `20 x 20` sample grid over the merge
//! domain, cut at knot lines and at the natural domain's sides; `d` is the normal at a
//! piece's centre (any direction is valid, the choice only makes the proof succeed or
//! not). A piece not proved is quartered, at most [`MAX_SPLITS`] times, each quarter
//! with its own `d`; past that the certificate refuses
//! ([`FoldCertificate::NotProved`]), as it does for a surface kind with no certificate
//! here. On the last split's pieces along the natural domain's sides (`1 / 16` of a
//! cell wide) only `N . d >= -E` is shown: a zero of `N` there, or a reversal smaller
//! than the arithmetic's bound `E`, is not excluded.
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
//! magnitudes). With the twins' own rounding, `E = (Te - T) + 3 gamma_(2K) Te`.
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
//! Deviations: none (no upstream counterpart). No recursion: pieces wait on a stack, at
//! most [`MAX_PIECES`] of them per call.

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
        reason: &'static str,
    },
}

/// Whether `s` (evaluated with out-of-bounds evaluation on, in its STEP parameters) is
/// proved not to fold over `domain` past `natural`, its natural (current STEP) domain.
///
/// # Errors
///
/// Those of making the NURBS form's Bezier patches.
pub fn certify_no_fold(
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
        return Ok(not("the merge domain is not a finite rectangle"));
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
        | SurfaceKind::SrfInVolume(_) => Ok(not("no fold certificate for this surface kind")),
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
        _ => return Ok(not("not a swept surface")),
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
        _ => return Ok(not("the generator's continuation is not proved")),
    };
    let Some((origin, axis)) = revolution else {
        // `C' x E`, the same everywhere
        return Ok(FoldCertificate::Proved);
    };
    // `M(t)` and the rounding bound of `M(t) . d`: the line's point and the difference
    // with `A` (each within `gamma_4` of its terms' sizes), then two cross products and
    // a dot (generously `gamma_32` of the product of the sizes)
    let m = |t: f64| -> (Vec3, f64) {
        let pv = line.evaluate_step(t, 1);
        let (Some(&c), Some(&tangent)) = (pv.first(), pv.get(1)) else {
            return (Vec3::new(f64::NAN, f64::NAN, f64::NAN), f64::NAN);
        };
        let size = (c.length() + origin.length()) * axis.length() * tangent.length();
        (axis.cross(c - origin).cross(tangent), size)
    };
    let (middle, _) = m(0.5 * (n.min() + n.max()));
    let dlen = middle.length();
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
            return Ok(not("the generator's closed form is not finite"));
        }
        // positive, or undecided on the natural interval's closed ends
        let on_natural = t >= n.min() && t <= n.max();
        if !(above(x, e) || (on_natural && !above(-x, e))) {
            return Ok(not(
                "the normal turns back or vanishes along the generator's continuation",
            ));
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
/// `extra` error ([`net_certificate`]).
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
        return Ok(not("no NURBS form"));
    };
    let emax = e.x.abs().max(e.y.abs()).max(e.z.abs());
    let mut extra = 0.0_f64;
    let mut net = Vec::new();
    for &cp in nurb.cpts() {
        if cp.z == NOZ {
            return Ok(not("a two-dimensional form"));
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
    net_certificate(k, &surface, &natural, &domain, extra)
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

    /// The bound `E` of each coefficient (see the module header), for `k` levels.
    fn bounds(&self, k: usize) -> Vec<f64> {
        let g = 3.0 * gamma(2 * k);
        self.t
            .coefficients()
            .iter()
            .zip(self.te.coefficients())
            .map(|(&t, &te)| ((te - t).max(0.0) + g * te) * (1.0 + gamma(4)))
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
    /// The most pieces computed ([`MAX_PIECES`] scaled by the degrees' work).
    max_pieces: usize,
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
        return Ok(not("a power-basis form"));
    }
    let Some(nurb) = b.gw_nurb() else {
        return Ok(not("no NURBS form"));
    };
    net_certificate(k, nurb, natural, domain, 0.0)
}

/// The certificate of the NURBS net `nurb`'s continuation; `extra` bounds, per
/// coefficient, how far the net may be from the exact net of the surface certified
/// (added to the extraction's error `e0`: knot insertion takes convex combinations, so
/// it moves no coefficient by more than the largest input error).
fn net_certificate(
    k: &Kernel,
    nurb: &NlSurface,
    natural: &Extent2d,
    domain: &Extent2d,
    extra: f64,
) -> Result<FoldCertificate> {
    let centre = domain.evaluate(0.5, 0.5);
    let not = |reason| FoldCertificate::NotProved { uv: centre, reason };
    let (p, q) = (nurb.p(), nurb.q());
    let mut rational = false;
    let mut net_size = 0.0_f64;
    for row in nurb.rows() {
        for c in row {
            if c.z == NOZ {
                return Ok(not("a two-dimensional form"));
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
                return Ok(not("a coefficient not finite or a weight not positive"));
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
        return Ok(not("degrees past the certificate's"));
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
        return Ok(not("a coefficient not finite or a weight not positive"));
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
                return Ok(not("a patch without knots"));
            };
            if !(above(u1, u0) && above(v1, v0)) {
                return Ok(not("an empty patch"));
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
        return Ok(not("no Bezier patches"));
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
        return Ok(not("an end span narrower than the span search's cut-off"));
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

    fn quarters(&self) -> [Self; 4] {
        let (xm, ym) = (0.5 * (self.x[0] + self.x[1]), 0.5 * (self.y[0] + self.y[1]));
        [
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

/// Whether `N . d >= ` the bound (`strict`: `> E`; else `>= -E`) on every coefficient of
/// the piece's `N . d`, so on the whole piece.
fn holds(f: &Net, levels: usize, strict: bool) -> bool {
    let bound = f.bounds(levels);
    f.v.coefficients()
        .iter()
        .zip(&bound)
        .all(|(&x, &e)| if strict { above(x, e) } else { !above(-x, e) })
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
    let g = GRID as f64;
    let at = |a: f64, b: f64, i: usize| {
        if i == GRID {
            b
        } else {
            a + (b - a) * (i as f64 / g)
        }
    };
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
    // `N . d` on the piece `r` holds the bound
    let check = |r: &Rect, d: Vec3, strict: bool| -> Result<Option<bool>> {
        let Some(patch) = patch_of(r) else {
            return Ok(None);
        };
        let Some(n) = normal_on(form, patch, r, &b)? else {
            return Ok(None);
        };
        Ok(Some(holds(&Net::combine(&n, d, &b)?, form.levels, strict)))
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
        for j in 0..GRID {
            for x in cuts(at(lo.x, hi.x, i), at(lo.x, hi.x, i + 1), &ubreaks) {
                for y in cuts(at(lo.y, hi.y, j), at(lo.y, hi.y, j + 1), &vbreaks) {
                    let r = Rect { x, y };
                    if !inside(&r) {
                        stack.push((r, 0));
                    }
                }
            }
        }
    }
    let mut work = 0_usize;
    while let Some((r, depth)) = stack.pop() {
        k.check_cancelled(OP)?;
        work += 1;
        if work > form.max_pieces {
            return Ok(FoldCertificate::NotProved {
                uv: r.centre(),
                reason: "the certificate's work bound",
            });
        }
        let touching = r.touches(natural);
        let last = depth >= MAX_SPLITS;
        // the direction: the normal at the piece's centre (restricted to the whole piece:
        // the restriction's derivatives are the patch's scaled by the piece's positive
        // widths, so the direction at its centre (0.5, 0.5) is kept)
        let direction = match patch_of(&r) {
            Some(patch) => normal_on(form, patch, &r, &b)?.map(|n| {
                Vec3::new(
                    n[0].v.eval(0.5, 0.5),
                    n[1].v.eval(0.5, 0.5),
                    n[2].v.eval(0.5, 0.5),
                )
            }),
            None => None,
        }
        .filter(|d| d.x.is_finite() && d.y.is_finite() && d.z.is_finite() && *d != Vec3::ZERO);
        let proved = match direction {
            None => false,
            Some(d) => {
                // away from the natural domain: a normal that never vanishes; touching
                // it, the same unless the piece is down to the last split, and then a
                // normal that does not turn back against `d`. A touching piece's mirror
                // inside the natural domain must not turn back against `d` either: no
                // reversal across the natural domain's side.
                let own = check(&r, d, !touching || !last)?;
                let across = if touching && own == Some(true) {
                    // no mirror (a natural domain without width): nothing proved
                    let mut ok = None;
                    if let Some(m) = mirror(&r) {
                        'pieces: for x in cuts(m.x[0], m.x[1], &ubreaks) {
                            for y in cuts(m.y[0], m.y[1], &vbreaks) {
                                ok = check(&Rect { x, y }, d, false)?;
                                if ok != Some(true) {
                                    break 'pieces;
                                }
                            }
                        }
                    }
                    ok
                } else {
                    own
                };
                own == Some(true) && across == Some(true)
            }
        };
        if proved {
            continue;
        }
        if last {
            return Ok(FoldCertificate::NotProved {
                uv: r.centre(),
                reason: if touching {
                    "a turn back across the natural domain's side is not excluded"
                } else {
                    "a zero of the normal is not excluded"
                },
            });
        }
        for q in r.quarters() {
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
    use super::FoldCertificate;
    use crate::error::Result;
    use crate::geom::certified::above;
    use crate::geom::extent2d::Extent2d;
    use crate::kernel::Kernel;
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
            _ => return Ok(not("no analytic certificate for this surface kind")),
        };
        // the sign of `lambda` where it is decided; an undecided value only on the
        // natural domain's closed `v` interval
        let mut sign = 0.0_f64;
        for &(v, x, e) in &samples {
            if !(x.is_finite() && e.is_finite()) {
                return Ok(not("a closed form not finite"));
            }
            if above(x.abs(), e) {
                let here = x.signum();
                if sign != 0.0 && here != sign {
                    return Ok(not("the normal turns back"));
                }
                sign = here;
            } else if !on_natural(v) {
                return Ok(not("the normal vanishes past the natural domain"));
            }
        }
        if sign == 0.0 {
            return Ok(not("the normal's sign is not decided"));
        }
        Ok(FoldCertificate::Proved)
    }

    /// `v0`, `v1` and every multiple of 180 degrees between them (the extremes of
    /// `cos`), with `cos` at each: every value of `cos` on `[v0, v1]` lies between
    /// the least and the greatest of them, and a sign change of `a + b cos` shows at
    /// them. At most 64 extremes (an interval of more than 32 turns is refused by the
    /// caller seeing an undecided sign).
    fn cos_extremes(v0: f64, v1: f64) -> Vec<(f64, f64)> {
        let cos = |v: f64| (v * core::f64::consts::PI / 180.0).cos();
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
