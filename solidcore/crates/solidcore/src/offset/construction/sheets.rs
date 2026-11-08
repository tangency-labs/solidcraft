//! S1 of the one shell construction: each face's offset sheet, exact or certified.
//!
//! Upstream: none (not upstream; solidcore-native, tangency-labs/solidcore#2350 §1.1 and
//! §6 step 1; opt-in `UpstreamFixes::shell_one_construction`, WR-718).
//!
//! | Rust                         | Upstream                                              |
//! |------------------------------|-------------------------------------------------------|
//! | [`SheetClass`], [`classify_radius`] | the three-way split of `OffsetCollapses` (§1.1) |
//! | [`certified_offset`]         | replaces `N_SrfOffset` in `ReplaceImplicitOffsets`    |
//! | [`certify_regular`]          | replaces `FaceuseOffset`'s self-intersection fold check |
//!
//! **The certified offset.** `O(u, v) = S(u, v) + d n(u, v)` of a (rational) B-spline
//! surface `S` is approximated by a bicubic B-spline `G` that interpolates exact points
//! of `O` at the Greville abscissae of its knots: `S`'s breakpoints (with the
//! multiplicity that keeps `G` as smooth as `n`), plus inserted knots. `G` is then
//! certified (Elber & Cohen 1991) on every Bezier patch of the common refinement, from
//! `S = P / w` and its homogeneous derivatives `A_u = P_u w - P w_u`, `A_v`, with
//! `D' = G w - P` (so `G - S = D' / w`), all exact Bernstein polynomials:
//!
//! * `e = |G - S|^2 - d^2`, bounded by the coefficients of `D'.D' - d^2 w^2` over `w^2`;
//! * the tangential part `tau` of `G - S`: `|tau|^2 = T / (|A_u x A_v|^2 w^2)` with
//!   `T = (A_v.A_v)(D'.A_u)^2 - 2 (A_u.A_v)(D'.A_u)(D'.A_v) + (A_u.A_u)(D'.A_v)^2`;
//! * `G - S` on the side `d` names: `sign(d) D'.(A_u x A_v) > 0`.
//!
//! Then `|G - O| <= |tau| + (|e| + |tau|^2) / |d|` at every parameter (with
//! `lambda = (G - S).n`: `lambda^2 = d^2 + e - |tau|^2` and `|lambda + d| >= |d|`). A patch
//! whose bound exceeds the tolerance is halved where that helps most, by a knot of `G`,
//! and `G` is interpolated again.
//!
//! **Regularity.** The offset is regular where `det(I - d W) > 0` (`W` the shape
//! operator; Patrikalakis & Maekawa Eq. 11.28, Farouki 1986). Multiplied out with the
//! homogeneous forms it is `Q = X^4 - d w^2 X H + d^2 w^4 K > 0`, where `X^2 =
//! |A_u x A_v|^2`, `H = E'n'' + G'l'' - 2F'm''`, `K = l''n'' - m''^2`, `E' = A_u.A_u`, ...
//! and `l'' = (P_uu w - w_uu P).(A_u x A_v)`, ...: polynomials and one square root,
//! bounded per patch and subdivided until certified, or until a point with `Q < 0` (a
//! fold) is found. The homogeneous normal carries a factor w, `A_u x A_v = w N'`, so
//! `X = w X'`, `H = w H'`, `K = w^2 K'` (`X'`, `H'`, `K'` the same forms on `N'`) and
//! `Q = w^4 (X'^4 - d X' H' + d^2 w^2 K')`: the bracket, of the same sign, is what is
//! bounded, of degree `4 deg w` less in each parameter (#2487: a G2 round's rational
//! bicubic-by-septic net reaches degree 112 in Q, past the Bernstein forms' cap of 96, and
//! 80 in the bracket). A face not certified regular is refused (D5), naming the largest
//! wall that works: the smallest radius of curvature on the displaced side.
//!
//! Deviations: not upstream. Bounds: at most [`MAX_ROUNDS`] refinement rounds and
//! [`MAX_SPANS`] spans per direction for the offset; at most [`MAX_REGULARITY_PATCHES`]
//! subdivided patches for the regularity test (a loop over an explicit stack, no
//! recursion; the kernel's cancel token is polled per round, per row of patches and per
//! patch examined). A face is refused when a point with `Q < 0` is found (it folds) or
//! when the budget runs out uncertified (D5: not certified regular), which includes a
//! base whose normal vanishes on the face (a pole), where no regular sheet is certified.

use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::Vec3;
use crate::nurbs::knots::{KnotVector, Side};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::nurbs::nurbs_srf::decompose_srf;
use crate::nurbs::srf_basic::Surface as Nurb;

use super::bernstein::{
    cross, deriv_u3, deriv_v3, dot, eval3, mul3, restrict3, sub3, Bernstein2, Bernstein3, Binomials,
};

const OP: &str = "offset::construction::sheets";

/// The most rounds of knot insertion [`certified_offset`] makes.
pub const MAX_ROUNDS: usize = 14;
/// The most spans in either direction of a certified offset.
pub const MAX_SPANS: usize = 2048;
/// The most patches [`certify_regular`] examines.
pub const MAX_REGULARITY_PATCHES: usize = 20_000;
/// The degree of a certified offset in both directions.
const DEGREE: usize = 3;

/// `x > 0` (false for NaN).
fn positive(x: f64) -> bool {
    x > 0.0
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

/// How an analytic face's offset relates to it (§1.1, radius through zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetClass {
    /// The offset radius stays positive: the offset is the same kind of surface.
    Regular,
    /// The offset radius is negative: the same surface of radius `|r + d|`, its
    /// orientation reversed (the swallowtail; the cavity rule discards it).
    Inverted,
    /// The offset radius is zero to tolerance: the offset is the spine (an axis, a
    /// centre or a centre circle) and bounds nothing; the face gets no sheet.
    Spine,
}

/// The class of an analytic offset whose radius becomes `radius_after` (`r + d` along
/// the face's normal), to `tol`.
#[must_use]
pub fn classify_radius(radius_after: f64, tol: f64) -> SheetClass {
    if radius_after.abs() < tol {
        SheetClass::Spine
    } else if radius_after < 0.0 {
        SheetClass::Inverted
    } else {
        SheetClass::Regular
    }
}

/// One Bezier patch of a NURBS surface, in homogeneous form: `S = p / w`.
#[derive(Debug, Clone)]
struct Patch {
    /// Parameter range `[u0, u1] x [v0, v1]` of the patch in the surface's domain.
    range: [f64; 4],
    p: Bernstein3,
    /// `None` for a non-rational patch (`w = 1`).
    w: Option<Bernstein2>,
}

/// The distinct breakpoints of the knots `k` of degree `deg` over the domain, with the
/// multiplicity of each interior one.
fn breakpoints(k: &[f64], deg: usize) -> Result<(Vec<f64>, Vec<usize>)> {
    let n = k
        .len()
        .checked_sub(deg + 2)
        .ok_or_else(|| invalid("too few knots"))?;
    let lo = *k.get(deg).ok_or_else(|| invalid("too few knots"))?;
    let hi = *k.get(n + 1).ok_or_else(|| invalid("too few knots"))?;
    let mut values: Vec<f64> = vec![lo];
    let mut mults: Vec<usize> = Vec::new();
    for &x in k.iter().take(n + 1).skip(deg + 1) {
        if x <= lo || x >= hi {
            continue;
        }
        match values.last() {
            Some(&last) if x == last => {
                if let Some(m) = mults.last_mut() {
                    *m += 1;
                }
            }
            _ => {
                values.push(x);
                mults.push(1);
            }
        }
    }
    values.push(hi);
    Ok((values, mults))
}

fn coordinates(c: CPoint, rational: bool) -> Result<(Vec3, f64)> {
    if c.z == NOZ {
        return Err(invalid("a planar (2D) control net"));
    }
    if !rational || c.w == NOW {
        return Ok((Vec3::new(c.x, c.y, c.z), 1.0));
    }
    Ok((Vec3::new(c.x, c.y, c.z), c.w))
}

/// The Bezier patches of `s`, `[iu][iv]`, with the breakpoints they span.
fn patches(s: &Nurb) -> Result<Patches> {
    let (p, q) = s.degrees();
    let (ub, _) = breakpoints(s.u_knots(), p)?;
    let (vb, _) = breakpoints(s.v_knots(), q)?;
    let rational = s.is_rational();
    let bez = decompose_srf(s)?;
    if bez.len() + 1 != ub.len() {
        return Err(invalid("Bezier patches do not match the u breakpoints"));
    }
    let mut out = Vec::new();
    for (iu, row) in bez.iter().enumerate() {
        if row.len() + 1 != vb.len() {
            return Err(invalid("Bezier patches do not match the v breakpoints"));
        }
        let mut prow = Vec::new();
        for (iv, patch) in row.iter().enumerate() {
            let (pp, pq) = patch.degrees();
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            let mut zs = Vec::new();
            let mut ws = Vec::new();
            for i in 0..=pp {
                for j in 0..=pq {
                    let (v, w) = coordinates(patch.cpt(i, j)?, rational)?;
                    xs.push(v.x);
                    ys.push(v.y);
                    zs.push(v.z);
                    ws.push(w);
                }
            }
            let w0 = ws.first().copied().unwrap_or(1.0);
            let weighted = ws.iter().any(|&w| w != w0);
            if !positive(w0) || ws.iter().any(|&w| !positive(w)) {
                return Err(invalid("a weight that is not positive"));
            }
            let (xs, ys, zs) = if weighted {
                (xs, ys, zs)
            } else {
                let s = 1.0 / w0;
                (
                    xs.iter().map(|x| x * s).collect(),
                    ys.iter().map(|x| x * s).collect(),
                    zs.iter().map(|x| x * s).collect(),
                )
            };
            let range = [
                *ub.get(iu).ok_or_else(|| invalid("u breakpoint"))?,
                *ub.get(iu + 1).ok_or_else(|| invalid("u breakpoint"))?,
                *vb.get(iv).ok_or_else(|| invalid("v breakpoint"))?,
                *vb.get(iv + 1).ok_or_else(|| invalid("v breakpoint"))?,
            ];
            prow.push(Patch {
                range,
                p: [
                    Bernstein2::new(pp, pq, xs)?,
                    Bernstein2::new(pp, pq, ys)?,
                    Bernstein2::new(pp, pq, zs)?,
                ],
                w: if weighted {
                    Some(Bernstein2::new(pp, pq, ws)?)
                } else {
                    None
                },
            });
        }
        out.push(prow);
    }
    Ok((ub, vb, out))
}

/// A surface's u and v breakpoints and its Bezier patches `[iu][iv]`.
type Patches = (Vec<f64>, Vec<f64>, Vec<Vec<Patch>>);

/// The index of the span of `breaks` holding `x` (the last span at its end).
fn span_of(breaks: &[f64], x: f64) -> usize {
    let last = breaks.len().saturating_sub(2);
    breaks
        .windows(2)
        .position(|w| matches!(*w, [_, b] if x < b))
        .unwrap_or(last)
        .min(last)
}

fn local(range: [f64; 4], u: f64, v: f64) -> (f64, f64) {
    let [u0, u1, v0, v1] = range;
    (
        ((u - u0) / (u1 - u0)).clamp(0.0, 1.0),
        ((v - v0) / (v1 - v0)).clamp(0.0, 1.0),
    )
}

/// The homogeneous first derivatives `A_u = P_u w - P w_u`, `A_v` of a patch (`P_u`,
/// `P_v` for a non-rational one).
fn homogeneous_tangents(patch: &Patch, b: &Binomials) -> Result<(Bernstein3, Bernstein3)> {
    let pu = deriv_u3(&patch.p);
    let pv = deriv_v3(&patch.p);
    match &patch.w {
        None => Ok((pu, pv)),
        Some(w) => Ok((
            sub3(&mul3(&pu, w, b)?, &mul3(&patch.p, &w.deriv_u(), b)?, b)?,
            sub3(&mul3(&pv, w, b)?, &mul3(&patch.p, &w.deriv_v(), b)?, b)?,
        )),
    }
}

/// The patch's homogeneous normal with its factor `w` divided out: `A_u x A_v = w N'`,
/// `N' = w P_u x P_v - w_v P_u x P - w_u P x P_v` (the `w_u w_v P x P` term vanishes), so
/// `N'` is of degree `w`'s less than `A_u x A_v` in each parameter, and the regularity
/// forms built on it four times that less ([`RegularityForms`]); `P_u x P_v` for a
/// polynomial patch.
fn reduced_normal(patch: &Patch, b: &Binomials) -> Result<Bernstein3> {
    let pu = deriv_u3(&patch.p);
    let pv = deriv_v3(&patch.p);
    let n = cross(&pu, &pv, b)?;
    match &patch.w {
        None => Ok(n),
        Some(w) => sub3(
            &sub3(
                &mul3(&n, w, b)?,
                &mul3(&cross(&pu, &patch.p, b)?, &w.deriv_v(), b)?,
                b,
            )?,
            &mul3(&cross(&patch.p, &pv, b)?, &w.deriv_u(), b)?,
            b,
        ),
    }
}

/// The point `S` and unit normal `n` (`S_u x S_v`, normalised) of the surface whose
/// patches are `ps` at `(u, v)`.
fn point_and_normal(
    ub: &[f64],
    vb: &[f64],
    ps: &[Vec<Patch>],
    u: f64,
    v: f64,
) -> Result<(Vec3, Vec3)> {
    let patch = ps
        .get(span_of(ub, u))
        .and_then(|r| r.get(span_of(vb, v)))
        .ok_or_else(|| invalid("no patch at the parameter"))?;
    let (s, t) = local(patch.range, u, v);
    let p = eval3(&patch.p, s, t);
    let pu = eval3(&deriv_u3(&patch.p), s, t);
    let pv = eval3(&deriv_v3(&patch.p), s, t);
    let (point, au, av) = match &patch.w {
        None => (p, pu, pv),
        Some(w) => {
            let wv = w.eval(s, t);
            let wu = w.deriv_u().eval(s, t);
            let wdv = w.deriv_v().eval(s, t);
            (p * (1.0 / wv), pu * wv - p * wu, pv * wv - p * wdv)
        }
    };
    let n = au.cross(av);
    let len = n.length();
    if !positive(len) {
        return Err(SolidError::Degenerate {
            operation: OP,
            entity: None,
        });
    }
    Ok((point, n * (1.0 / len)))
}

/// A certified offset: the B-spline and the bound on its distance from the exact
/// offset point at every parameter.
#[derive(Debug, Clone)]
pub struct CertifiedOffset {
    /// The bicubic B-spline `G`, over the base's parameter domain.
    pub surface: Nurb,
    /// The certified bound of `|G(u, v) - (S + d n)(u, v)|` over the domain.
    pub bound: f64,
}

/// The knots of `G` in one direction: the base's breakpoints `breaks` (an interior one
/// of multiplicity `m` in the base's degree `deg` taken `3 - min(deg - m - 1, 2)` times,
/// so `G` keeps the normal's continuity) and the inserted `extra` knots (simple). An
/// interior breakpoint of multiplicity `deg` (the net only `C0` in its parameter) is a
/// crease unless `smooth` says its joint was proved tangent continuous ([`joint_meets`]):
/// then the normal is `C0` there and `G` takes it `3` times.
fn offset_knots(
    breaks: &[f64],
    mults: &[usize],
    smooth: &[bool],
    deg: usize,
    extra: &[f64],
) -> Result<Vec<f64>> {
    let (Some(&lo), Some(&hi)) = (breaks.first(), breaks.last()) else {
        return Err(invalid("no breakpoints"));
    };
    let mut interior: Vec<(f64, usize)> = Vec::new();
    for ((&x, &m), at) in breaks
        .iter()
        .skip(1)
        .take(breaks.len().saturating_sub(2))
        .zip(mults)
        .zip(0..)
    {
        // the normal is C^(deg - m - 1) there; at multiplicity deg C0 where the joint
        // was proved tangent continuous
        let cont = match deg.checked_sub(m + 1) {
            Some(c) => c,
            None if m == deg && smooth.get(at) == Some(&true) => 0,
            None => {
                return Err(invalid(
                    "a crease inside the face: its offset is not one surface",
                ))
            }
        };
        interior.push((x, DEGREE - cont.min(DEGREE - 1)));
    }
    interior.extend(extra.iter().map(|&x| (x, 1)));
    interior.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut k = vec![lo; DEGREE + 1];
    for (x, m) in interior {
        k.extend(core::iter::repeat_n(x, m));
    }
    k.extend(core::iter::repeat_n(hi, DEGREE + 1));
    Ok(k)
}

/// The most pieces [`joint_meets`] splits one joint's edge into.
const MAX_JOINT_PIECES: usize = 256;

/// Whether the joint of `ps` at the interior breakpoint `at` of the u (`along_u`) or v
/// breakpoints, a knot of multiplicity the base's degree (where its net is only `C0` in the
/// parameter), is tangent continuous closely enough that the offsets of its two sides meet
/// there within half the offset's tolerance `tol` at the distance `d`: then its offset is
/// one surface, `C0` across the joint, and [`certified_offset`] certifies it as any other
/// breakpoint. A knot's multiplicity is not the joint's geometry: an exact extension's joint
/// (Natural, Tangent, Ruled) is of multiplicity `p` and tangent continuous.
///
/// The comparison is of the two one-sided normals along the joint's edge, `n_L` and `n_R`
/// (the patches' reduced normals [`reduced_normal`], of the one-sided derivatives across
/// the joint and the common one along it), as Bernstein polynomials of the edge's parameter:
/// `D = n_L . n_R`, `C = n_L x n_R` and `Y = |n_L|^2 |n_R|^2`. Where every coefficient of
/// `D` is positive the angle `theta` between the unit normals is below a right angle, and
/// where `|C| <= s sqrt(Y)` on the piece (the norm of `C`'s components' largest coefficients
/// against `Y`'s smallest, each with its rounding) `sin theta <= s`, so the offset points'
/// gap `|d| |n_L - n_R| = 2 |d| sin(theta / 2) <= sqrt 2 |d| sin theta`; `s = tol / (2 sqrt
/// 2 |d|)` holds the gap to `tol / 2`. An edge piece not proved so is halved (an explicit
/// stack, at most [`MAX_JOINT_PIECES`] pieces); a piece whose end shows the jump (`D` not
/// positive, or `|C| > s sqrt(Y)` there) ends the test: a crease.
fn joint_meets(
    ps: &[Vec<Patch>],
    along_u: bool,
    at: usize,
    d: f64,
    tol: f64,
    b: &Binomials,
) -> Result<bool> {
    let before = at
        .checked_sub(1)
        .ok_or_else(|| invalid("a joint at the domain's end"))?;
    let s = tol / (2.0 * core::f64::consts::SQRT_2 * d.abs());
    if !(s.is_finite() && positive(s)) {
        return Ok(false);
    }
    let pairs: Vec<(&Patch, &Patch)> = if along_u {
        let (Some(left), Some(right)) = (ps.get(before), ps.get(at)) else {
            return Err(invalid("no patch row at the joint"));
        };
        left.iter().zip(right.iter()).collect()
    } else {
        ps.iter()
            .map(|row| match (row.get(before), row.get(at)) {
                (Some(l), Some(r)) => Ok((l, r)),
                _ => Err(invalid("no patch column at the joint")),
            })
            .collect::<Result<_>>()?
    };
    for (left, right) in pairs {
        let nl = reduced_normal(left, b)?;
        let nr = reduced_normal(right, b)?;
        // the edge: the left patch's high end, the right patch's low end, in the joint's
        // direction (a polynomial constant in it, of the edge's parameter in the other)
        let (el, er) = if along_u {
            (
                restrict3(&nl, 1.0, 1.0, 0.0, 1.0),
                restrict3(&nr, 0.0, 0.0, 0.0, 1.0),
            )
        } else {
            (
                restrict3(&nl, 0.0, 1.0, 1.0, 1.0),
                restrict3(&nr, 0.0, 1.0, 0.0, 0.0),
            )
        };
        let mut stack = vec![(el, er)];
        let mut pieces = 1_usize;
        while let Some((a, c)) = stack.pop() {
            let dd = dot(&a, &c, b)?;
            let x = cross(&a, &c, b)?;
            let y = dot(&a, &a, b)?.mul(&dot(&c, &c, b)?, b)?;
            // every coefficient of these forms is a weighted mean of products of the normals'
            // coefficients (the Bernstein product's weights sum to 1): at most 2 M for a
            // component of the cross product, 3 M for D and 9 M^2 for Y, with M the product of
            // the two normals' largest coefficients; each is formed in a few dozen operations
            // and the splits are convex combinations: gamma_64 of that bounds their rounding
            let m = a
                .iter()
                .map(Bernstein2::max_abs_coefficient)
                .fold(0.0_f64, f64::max)
                * c.iter()
                    .map(Bernstein2::max_abs_coefficient)
                    .fold(0.0_f64, f64::max);
            let (rc, rd, ry) = (
                gamma(64) * 2.0 * m,
                gamma(64) * 3.0 * m,
                gamma(64) * 9.0 * m * m,
            );
            // |n_L x n_R| over the piece: at most the norm of the components' largest
            // coefficients (each component within the hull of its coefficients)
            let cmax = x
                .iter()
                .map(|f| f.max_abs_coefficient().powi(2))
                .sum::<f64>()
                .sqrt();
            let floor = |v: f64| (v - ry).max(0.0).sqrt();
            if dd.min_coefficient() > rd && cmax + rc <= s * floor(y.min_coefficient()) {
                continue;
            }
            // a jump at an end of the piece (its end values are coefficients): a crease, no
            // split proves it smooth
            let first = |f: &Bernstein2| f.coefficients().first().copied().unwrap_or(0.0);
            let last = |f: &Bernstein2| f.coefficients().last().copied().unwrap_or(0.0);
            let norm = |pick: &dyn Fn(&Bernstein2) -> f64| {
                x.iter().map(|f| pick(f).powi(2)).sum::<f64>().sqrt()
            };
            let creased = |dv: f64, cv: f64, yv: f64| dv <= -rd || cv - rc > s * (yv + ry).sqrt();
            if creased(first(&dd), norm(&first), first(&y))
                || creased(last(&dd), norm(&last), last(&y))
            {
                return Ok(false);
            }
            pieces = pieces.saturating_add(1);
            if pieces > MAX_JOINT_PIECES {
                return Ok(false);
            }
            let halves = |f: &Bernstein3| -> (Bernstein3, Bernstein3) {
                let [x, yy, zz] = f;
                let sp = |g: &Bernstein2| {
                    if along_u {
                        g.split_v(0.5)
                    } else {
                        g.split_u(0.5)
                    }
                };
                let (x0, x1) = sp(x);
                let (y0, y1) = sp(yy);
                let (z0, z1) = sp(zz);
                ([x0, y0, z0], [x1, y1, z1])
            };
            let (a0, a1) = halves(&a);
            let (c0, c1) = halves(&c);
            stack.push((a0, c0));
            stack.push((a1, c1));
        }
    }
    Ok(true)
}

/// The Greville abscissae of the knots `k` of degree [`DEGREE`].
fn greville(k: &[f64]) -> Vec<f64> {
    k.windows(DEGREE + 2)
        .map(|w| w.iter().skip(1).take(DEGREE).sum::<f64>() / DEGREE as f64)
        .collect()
}

/// The control points of the degree-[`DEGREE`] B-spline on the knots `k` taking the
/// values `values[i]` at `params[i]` (Greville abscissae: the collocation matrix is
/// banded, `|j - i| <= DEGREE`, and totally positive, so elimination needs no pivoting;
/// de Boor, A Practical Guide to Splines, XIII).
fn interpolate_1d(k: &[f64], params: &[f64], values: &[Vec3]) -> Result<Vec<Vec3>> {
    let n = params.len();
    if values.len() != n || k.len() != n + DEGREE + 1 {
        return Err(invalid("interpolation sizes"));
    }
    let knots = KnotVector::from_slice(k)?;
    let width = 2 * DEGREE + 1;
    // row i holds columns i - DEGREE ..= i + DEGREE at offsets 0 ..= 2 DEGREE
    let mut band = vec![0.0; n * width];
    for (i, &t) in params.iter().enumerate() {
        let basis = knots.basis_eval(DEGREE, t, Side::Left)?;
        for (jj, &b) in basis.values.iter().take(DEGREE + 1).enumerate() {
            let col = (basis.span + jj)
                .checked_sub(DEGREE)
                .ok_or_else(|| invalid("basis span"))?;
            let off = (col + DEGREE)
                .checked_sub(i)
                .filter(|&o| o < width)
                .ok_or_else(|| invalid("collocation outside its band"))?;
            *band
                .get_mut(i * width + off)
                .ok_or_else(|| invalid("band"))? = b;
        }
    }
    let at = |band: &[f64], r: usize, c: usize| -> f64 {
        (c + DEGREE)
            .checked_sub(r)
            .filter(|&o| o < width)
            .and_then(|o| band.get(r * width + o))
            .copied()
            .unwrap_or(0.0)
    };
    let mut rhs = values.to_vec();
    for c in 0..n {
        let pivot = at(&band, c, c);
        if !positive(pivot.abs()) {
            return Err(invalid("singular collocation"));
        }
        let pivot_row = rhs.get(c).copied().ok_or_else(|| invalid("rhs"))?;
        for r in c + 1..n.min(c + DEGREE + 1) {
            let f = at(&band, r, c) / pivot;
            if f == 0.0 {
                continue;
            }
            for cc in c..n.min(c + DEGREE + 1) {
                let v = at(&band, r, cc) - f * at(&band, c, cc);
                if let Some(slot) = (cc + DEGREE)
                    .checked_sub(r)
                    .filter(|&o| o < width)
                    .and_then(|o| band.get_mut(r * width + o))
                {
                    *slot = v;
                }
            }
            if let Some(slot) = rhs.get_mut(r) {
                *slot -= pivot_row * f;
            }
        }
    }
    let mut x = vec![Vec3::ZERO; n];
    for r in (0..n).rev() {
        let mut v = rhs.get(r).copied().ok_or_else(|| invalid("rhs"))?;
        for c in r + 1..n.min(r + DEGREE + 1) {
            v -= x.get(c).copied().unwrap_or(Vec3::ZERO) * at(&band, r, c);
        }
        if let Some(slot) = x.get_mut(r) {
            *slot = v * (1.0 / at(&band, r, r));
        }
    }
    Ok(x)
}

/// The bicubic B-spline on the knots `ku`, `kv` interpolating `grid[i][j]` at
/// `(gu[i], gv[j])`: columns in u, then rows in v.
fn interpolate(grid: &[Vec<Vec3>], gu: &[f64], gv: &[f64], ku: &[f64], kv: &[f64]) -> Result<Nurb> {
    let m = gv.len();
    let mut cols: Vec<Vec<Vec3>> = Vec::new();
    for j in 0..m {
        let column: Vec<Vec3> = grid
            .iter()
            .map(|row| row.get(j).copied().ok_or_else(|| invalid("grid")))
            .collect::<Result<_>>()?;
        cols.push(interpolate_1d(ku, gu, &column)?);
    }
    let mut net = Vec::new();
    for i in 0..gu.len() {
        let row: Vec<Vec3> = cols
            .iter()
            .map(|c| c.get(i).copied().ok_or_else(|| invalid("grid")))
            .collect::<Result<_>>()?;
        net.push(
            interpolate_1d(kv, gv, &row)?
                .into_iter()
                .map(|p| CPoint::new(p.x, p.y, p.z, NOW))
                .collect(),
        );
    }
    Nurb::new(net, DEGREE, DEGREE, ku.to_vec(), kv.to_vec())
}

/// The polynomials whose coefficients bound the error of `G` on one patch.
struct ErrorForms {
    /// `D'.D' - d^2 w^2`.
    e: Bernstein2,
    /// `T`.
    t: Bernstein2,
    /// `|A_u x A_v|^2`.
    x2: Bernstein2,
    /// `sign(d) D'.(A_u x A_v)`.
    side: Bernstein2,
    /// `w`, or `None` for a non-rational patch.
    w: Option<Bernstein2>,
}

impl ErrorForms {
    fn new(s: &Patch, g: &Bernstein3, d: f64, b: &Binomials) -> Result<Self> {
        let (au, av) = homogeneous_tangents(s, b)?;
        let dn = match &s.w {
            None => sub3(g, &s.p, b)?,
            Some(w) => sub3(&mul3(g, w, b)?, &s.p, b)?,
        };
        let ax = cross(&au, &av, b)?;
        let dd = dot(&dn, &dn, b)?;
        let e = match &s.w {
            None => dd.add_scaled(&Bernstein2::constant(1.0), -d * d, b)?,
            Some(w) => dd.add_scaled(&w.mul(w, b)?, -d * d, b)?,
        };
        let ru = dot(&dn, &au, b)?;
        let rv = dot(&dn, &av, b)?;
        let t = dot(&av, &av, b)?
            .mul(&ru.mul(&ru, b)?, b)?
            .add_scaled(&dot(&au, &av, b)?.mul(&ru.mul(&rv, b)?, b)?, -2.0, b)?
            .add(&dot(&au, &au, b)?.mul(&rv.mul(&rv, b)?, b)?, b)?;
        Ok(Self {
            e,
            t,
            x2: dot(&ax, &ax, b)?,
            side: dot(&dn, &ax, b)?.scale(d.signum()),
            w: s.w.clone(),
        })
    }

    /// The certified bound on this patch (infinite where a denominator or the side is
    /// not certified).
    fn bound(&self, d: f64) -> f64 {
        let wmin = self.w.as_ref().map_or(1.0, Bernstein2::min_coefficient);
        let x2 = self.x2.min_coefficient();
        if !positive(wmin) || !positive(x2) || !positive(self.side.min_coefficient()) {
            return f64::INFINITY;
        }
        let e = self.e.max_abs_coefficient() / (wmin * wmin);
        let tau2 = self.t.max_coefficient().max(0.0) / (x2 * wmin * wmin);
        let tau = tau2.sqrt();
        tau + (e + tau2) / d.abs()
    }

    fn restrict(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> Self {
        Self {
            e: self.e.restrict(u0, u1, v0, v1),
            t: self.t.restrict(u0, u1, v0, v1),
            x2: self.x2.restrict(u0, u1, v0, v1),
            side: self.side.restrict(u0, u1, v0, v1),
            w: self.w.as_ref().map(|w| w.restrict(u0, u1, v0, v1)),
        }
    }

    /// The bound after splitting into quarters, and after halving in u and in v.
    fn split_bounds(&self, d: f64) -> (f64, f64, f64) {
        let q = [
            self.restrict(0.0, 0.5, 0.0, 0.5),
            self.restrict(0.5, 1.0, 0.0, 0.5),
            self.restrict(0.0, 0.5, 0.5, 1.0),
            self.restrict(0.5, 1.0, 0.5, 1.0),
        ];
        let quarters = q.iter().map(|f| f.bound(d)).fold(0.0, f64::max);
        let u = [
            self.restrict(0.0, 0.5, 0.0, 1.0),
            self.restrict(0.5, 1.0, 0.0, 1.0),
        ]
        .iter()
        .map(|f| f.bound(d))
        .fold(0.0, f64::max);
        let v = [
            self.restrict(0.0, 1.0, 0.0, 0.5),
            self.restrict(0.0, 1.0, 0.5, 1.0),
        ]
        .iter()
        .map(|f| f.bound(d))
        .fold(0.0, f64::max);
        (quarters, u, v)
    }
}

/// The non-rational Bezier patches of `g` (whose breakpoints are `gu`, `gv`).
fn offset_patches(g: &Nurb) -> Result<Vec<Vec<Bernstein3>>> {
    let mut out = Vec::new();
    for row in decompose_srf(g)? {
        let mut prow = Vec::new();
        for patch in row {
            let (pp, pq) = patch.degrees();
            let mut c: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            for i in 0..=pp {
                for j in 0..=pq {
                    let (v, _) = coordinates(patch.cpt(i, j)?, false)?;
                    let [x, y, z] = &mut c;
                    x.push(v.x);
                    y.push(v.y);
                    z.push(v.z);
                }
            }
            let [x, y, z] = c;
            prow.push([
                Bernstein2::new(pp, pq, x)?,
                Bernstein2::new(pp, pq, y)?,
                Bernstein2::new(pp, pq, z)?,
            ]);
        }
        out.push(prow);
    }
    Ok(out)
}

/// The certified offset of `s` by `d` along its normal (`S_u x S_v`, as the NURB's),
/// to within `tol` at every parameter, over `s`'s whole domain and in its
/// parametrisation (`O(u, v) = S(u, v) + d n(u, v)`).
///
/// # Errors
///
/// `InvalidInput` for a surface with a crease inside it (the normal jumps), a weight
/// that is not positive or a 2D net; [`SolidError::Degenerate`] where the normal
/// vanishes; [`SolidError::NotConverged`] when no certificate within `tol` is reached
/// in [`MAX_ROUNDS`] rounds or [`MAX_SPANS`] spans (a pole, or a fold the regularity
/// test let through).
pub fn certified_offset(k: &Kernel, s: &Nurb, d: f64, tol: f64) -> Result<CertifiedOffset> {
    if !positive(d.abs()) || !positive(tol) {
        return Err(invalid("a zero offset distance or tolerance"));
    }
    let b = Binomials::new();
    let (p, q) = s.degrees();
    let (sub, sub_mults) = breakpoints(s.u_knots(), p)?;
    let (svb, svb_mults) = breakpoints(s.v_knots(), q)?;
    let (ub, vb, ps) = patches(s)?;
    // the joints at multiplicity p (q) proved tangent continuous (an exact extension's)
    let smooth = |mults: &[usize], deg: usize, along_u: bool| -> Result<Vec<bool>> {
        mults
            .iter()
            .zip(1..)
            .map(|(&m, at)| {
                if m == deg {
                    joint_meets(&ps, along_u, at, d, tol, &b)
                } else {
                    Ok(false)
                }
            })
            .collect()
    };
    let smooth_u = smooth(&sub_mults, p, true)?;
    let smooth_v = smooth(&svb_mults, q, false)?;
    let mut extra_u: Vec<f64> = Vec::new();
    let mut extra_v: Vec<f64> = Vec::new();
    for _round in 0..MAX_ROUNDS {
        k.check_cancelled(OP)?;
        let ku = offset_knots(&sub, &sub_mults, &smooth_u, p, &extra_u)?;
        let kv = offset_knots(&svb, &svb_mults, &smooth_v, q, &extra_v)?;
        let gu = greville(&ku);
        let gv = greville(&kv);
        let mut grid: Vec<Vec<Vec3>> = Vec::new();
        for &u in &gu {
            let mut row = Vec::new();
            for &v in &gv {
                let (point, n) = point_and_normal(&ub, &vb, &ps, u, v)?;
                row.push(point + n * d);
            }
            grid.push(row);
        }
        let g = interpolate(&grid, &gu, &gv, &ku, &kv)?;
        let (gub, _) = breakpoints(&ku, DEGREE)?;
        let (gvb, _) = breakpoints(&kv, DEGREE)?;
        let gp = offset_patches(&g)?;
        let mut worst: f64 = 0.0;
        let mut split_u: Vec<f64> = Vec::new();
        let mut split_v: Vec<f64> = Vec::new();
        for (iu, row) in gp.iter().enumerate() {
            let (Some(&a), Some(&bb)) = (gub.get(iu), gub.get(iu + 1)) else {
                return Err(invalid("offset u breakpoint"));
            };
            k.check_cancelled(OP)?;
            for (iv, gpatch) in row.iter().enumerate() {
                let (Some(&c), Some(&e)) = (gvb.get(iv), gvb.get(iv + 1)) else {
                    return Err(invalid("offset v breakpoint"));
                };
                let um = 0.5 * (a + bb);
                let vm = 0.5 * (c + e);
                let spatch = ps
                    .get(span_of(&ub, um))
                    .and_then(|r| r.get(span_of(&vb, vm)))
                    .ok_or_else(|| invalid("no base patch under an offset patch"))?;
                let (s0, t0) = local(spatch.range, a, c);
                let (s1, t1) = local(spatch.range, bb, e);
                let restricted = Patch {
                    range: [a, bb, c, e],
                    p: restrict3(&spatch.p, s0, s1, t0, t1),
                    w: spatch.w.as_ref().map(|w| w.restrict(s0, s1, t0, t1)),
                };
                let forms = ErrorForms::new(&restricted, gpatch, d, &b)?;
                let mut bound = forms.bound(d);
                if bound > tol {
                    let (quarters, by_u, by_v) = forms.split_bounds(d);
                    bound = bound.min(quarters);
                    if bound > tol {
                        if by_u <= by_v {
                            split_u.push(um);
                        } else {
                            split_v.push(vm);
                        }
                        if !by_u.is_finite() && !by_v.is_finite() {
                            split_u.push(um);
                            split_v.push(vm);
                        }
                    }
                }
                worst = worst.max(bound);
            }
        }
        if worst <= tol {
            return Ok(CertifiedOffset {
                surface: g,
                bound: worst,
            });
        }
        for (extra, mut add) in [(&mut extra_u, split_u), (&mut extra_v, split_v)] {
            add.sort_by(f64::total_cmp);
            add.dedup();
            extra.extend(add);
            if extra.len() > MAX_SPANS {
                break;
            }
        }
        if extra_u.len() > MAX_SPANS || extra_v.len() > MAX_SPANS {
            break;
        }
    }
    Err(SolidError::NotConverged {
        operation: OP,
        iterations: u32::try_from(MAX_ROUNDS).unwrap_or(u32::MAX),
    })
}

/// The regularity forms of one patch: `Q = q0 - X q1` with `X = sqrt(x2)`.
#[derive(Debug, Clone)]
struct RegularityForms {
    /// `X'^4 + d^2 w^2 K'`.
    q0: Bernstein2,
    /// `d H'`.
    q1: Bernstein2,
    /// `X'^2`.
    x2: Bernstein2,
}

impl RegularityForms {
    fn new(s: &Patch, d: f64, b: &Binomials) -> Result<Self> {
        let (au, av) = homogeneous_tangents(s, b)?;
        let ax = reduced_normal(s, b)?;
        let x2 = dot(&ax, &ax, b)?;
        let second = |pdd: Bernstein3, wdd: Option<Bernstein2>| -> Result<Bernstein2> {
            let v = match (&s.w, wdd) {
                (Some(w), Some(wdd)) => sub3(&mul3(&pdd, w, b)?, &mul3(&s.p, &wdd, b)?, b)?,
                _ => pdd,
            };
            dot(&v, &ax, b)
        };
        let puu = deriv_u3(&deriv_u3(&s.p));
        let puv = deriv_v3(&deriv_u3(&s.p));
        let pvv = deriv_v3(&deriv_v3(&s.p));
        let w = s.w.as_ref();
        let l = second(puu, w.map(|w| w.deriv_u().deriv_u()))?;
        let m = second(puv, w.map(|w| w.deriv_u().deriv_v()))?;
        let n = second(pvv, w.map(|w| w.deriv_v().deriv_v()))?;
        let e1 = dot(&au, &au, b)?;
        let f1 = dot(&au, &av, b)?;
        let g1 = dot(&av, &av, b)?;
        let h = e1
            .mul(&n, b)?
            .add(&g1.mul(&l, b)?, b)?
            .add_scaled(&f1.mul(&m, b)?, -2.0, b)?;
        let k = l.mul(&n, b)?.sub(&m.mul(&m, b)?, b)?;
        let w2 = match w {
            Some(w) => w.mul(w, b)?,
            None => Bernstein2::constant(1.0),
        };
        let q0 = x2.mul(&x2, b)?.add_scaled(&w2.mul(&k, b)?, d * d, b)?;
        let q1 = h.scale(d);
        Ok(Self { q0, q1, x2 })
    }

    /// Whether `Q > 0` is certified on the whole patch.
    fn certified(&self) -> bool {
        if !positive(self.x2.min_coefficient()) {
            return false;
        }
        let x_max = self.x2.max_coefficient().max(0.0).sqrt();
        let x_min = self.x2.min_coefficient().sqrt();
        let q1_max = self.q1.max_coefficient();
        // - X q1 >= - X_max q1_max where q1 may be positive, >= - X_min q1_max where not
        let worst = if q1_max > 0.0 {
            x_max * q1_max
        } else {
            x_min * q1_max
        };
        self.q0.min_coefficient() - worst > 0.0
    }

    /// `Q` at the middle of the patch.
    fn middle(&self) -> f64 {
        let x = self.x2.eval(0.5, 0.5).max(0.0).sqrt();
        self.q0.eval(0.5, 0.5) - x * self.q1.eval(0.5, 0.5)
    }

    fn quarters(&self) -> [Self; 4] {
        let r = |u0: f64, u1: f64, v0: f64, v1: f64| Self {
            q0: self.q0.restrict(u0, u1, v0, v1),
            q1: self.q1.restrict(u0, u1, v0, v1),
            x2: self.x2.restrict(u0, u1, v0, v1),
        };
        [
            r(0.0, 0.5, 0.0, 0.5),
            r(0.5, 1.0, 0.0, 0.5),
            r(0.0, 0.5, 0.5, 1.0),
            r(0.5, 1.0, 0.5, 1.0),
        ]
    }
}

/// The outcome of [`certify_regular`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Regularity {
    /// The offset is regular on the whole region (`(1 - k1 d)(1 - k2 d) > 0`).
    Regular,
    /// The offset folds (a point with `(1 - k1 d)(1 - k2 d) < 0` was found) or is not
    /// certified regular within the budget; `largest` is the largest distance on that
    /// side that works (the smallest radius of curvature on the displaced side), finite.
    Folds {
        /// The largest offset distance (magnitude) whose offset does not fold.
        largest: f64,
    },
    /// Not certified, and the sizing found no curvature towards that side, so no distance
    /// is named: `Q` stays non-negative but will not separate from zero, where the
    /// reduced normal vanishes, or the budget ran out.
    NotCertified,
}

/// The patches of `s` over the parameter box `[u0, u1] x [v0, v1]` (clipped to the
/// domain), each restricted to the box.
fn patches_in(s: &Nurb, uv_box: Option<[f64; 4]>) -> Result<Vec<Patch>> {
    let (_, _, ps) = patches(s)?;
    let mut out = Vec::new();
    for patch in ps.into_iter().flatten() {
        let [a, b, c, e] = patch.range;
        let [u0, u1, v0, v1] = uv_box.unwrap_or(patch.range);
        let (lo_u, hi_u, lo_v, hi_v) = (a.max(u0), b.min(u1), c.max(v0), e.min(v1));
        if !positive(hi_u - lo_u) || !positive(hi_v - lo_v) {
            continue;
        }
        let (s0, t0) = local(patch.range, lo_u, lo_v);
        let (s1, t1) = local(patch.range, hi_u, hi_v);
        out.push(Patch {
            range: [lo_u, hi_u, lo_v, hi_v],
            p: restrict3(&patch.p, s0, s1, t0, t1),
            w: patch.w.as_ref().map(|w| w.restrict(s0, s1, t0, t1)),
        });
    }
    Ok(out)
}

/// Whether the offset of `s` by `d` (along its NURB normal) is regular over the
/// parameter box `uv_box` (`[u0, u1, v0, v1]`; `None`: the whole domain).
///
/// # Errors
///
/// Those of the Bezier decomposition; `InvalidInput` for a weight that is not
/// positive or a 2D net.
pub fn certify_regular(
    k: &Kernel,
    s: &Nurb,
    d: f64,
    uv_box: Option<[f64; 4]>,
) -> Result<Regularity> {
    let b = Binomials::new();
    let region = patches_in(s, uv_box)?;
    let mut stack: Vec<RegularityForms> = Vec::new();
    for patch in &region {
        stack.push(RegularityForms::new(patch, d, &b)?);
    }
    let mut examined = 0usize;
    let mut folds = false;
    while let Some(forms) = stack.pop() {
        k.check_cancelled(OP)?;
        examined += 1;
        if forms.certified() {
            continue;
        }
        if forms.middle() < 0.0 {
            // a point where the offset folds
            folds = true;
            break;
        }
        if examined > MAX_REGULARITY_PATCHES {
            folds = true;
            break;
        }
        stack.extend(forms.quarters());
    }
    if !folds {
        return Ok(Regularity::Regular);
    }
    let largest = largest_wall(&region, d.signum())?;
    Ok(if largest.is_finite() {
        Regularity::Folds { largest }
    } else {
        Regularity::NotCertified
    })
}

/// What [`proved_singular`] found: a point of the region where the offset is no regular
/// surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Singular {
    /// The radius of curvature on the displaced side there, a radius the surface attains.
    pub radius: f64,
    /// Whether the offset collapses there (the radius is the distance, within the
    /// resolution) rather than folds (the distance is the larger by more).
    pub collapses: bool,
}

/// A proof that the offset of `s` by `d` (along its NURB normal) over the parameter box
/// `uv_box` is no regular surface, or `None` where none was found. Where the surface attains
/// a radius of curvature `r` on the displaced side, the offset's radius there is `r - |d|`:
/// at most `resolution` (the smallest length the model tells apart), the offset collapses
/// there onto a line or point (a round inset by its radius); below `-resolution`, it folds
/// over itself. The radius is the least found by sampling each patch on a 9 x 9 grid and a
/// pattern search about the least sample, at a point of the surface (one where the normal
/// vanishes is passed over), so it is attained; its rounding, a few ulps of `r`, is far
/// below any resolution. Unlike [`certify_regular`], it refuses nothing it has not seen.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a distance or resolution that is not finite or a
/// negative resolution; those of the Bezier decomposition.
pub fn proved_singular(
    s: &Nurb,
    d: f64,
    uv_box: Option<[f64; 4]>,
    resolution: f64,
) -> Result<Option<Singular>> {
    if !d.is_finite() || !resolution.is_finite() || resolution < 0.0 {
        return Err(invalid("a distance or resolution that is not finite"));
    }
    let region = patches_in(s, uv_box)?;
    if region.is_empty() || !positive(d.abs()) {
        return Ok(None);
    }
    let radius = largest_wall(&region, d.signum())?;
    let reach = d.abs();
    if radius - reach > resolution {
        return Ok(None);
    }
    Ok(Some(Singular {
        radius,
        collapses: radius - reach >= -resolution,
    }))
}

/// The principal curvatures (signed along the NURB normal) of a patch at `(s, t)`.
fn principal_curvatures(patch: &Patch, s: f64, t: f64) -> Option<(f64, f64)> {
    let p = eval3(&patch.p, s, t);
    let pu = eval3(&deriv_u3(&patch.p), s, t);
    let pv = eval3(&deriv_v3(&patch.p), s, t);
    let puu = eval3(&deriv_u3(&deriv_u3(&patch.p)), s, t);
    let puv = eval3(&deriv_v3(&deriv_u3(&patch.p)), s, t);
    let pvv = eval3(&deriv_v3(&deriv_v3(&patch.p)), s, t);
    let (w, au, av, l2, m2, n2) = match &patch.w {
        None => (1.0, pu, pv, puu, puv, pvv),
        Some(w) => {
            let wv = w.eval(s, t);
            let (wu, wvd) = (w.deriv_u().eval(s, t), w.deriv_v().eval(s, t));
            let wuu = w.deriv_u().deriv_u().eval(s, t);
            let wuv = w.deriv_u().deriv_v().eval(s, t);
            let wvv = w.deriv_v().deriv_v().eval(s, t);
            (
                wv,
                pu * wv - p * wu,
                pv * wv - p * wvd,
                puu * wv - p * wuu,
                puv * wv - p * wuv,
                pvv * wv - p * wvv,
            )
        }
    };
    let ax = au.cross(av);
    let x = ax.length();
    if !positive(x) {
        return None;
    }
    let (e, f, g) = (au.dot(au), au.dot(av), av.dot(av));
    let (l, m, n) = (l2.dot(ax), m2.dot(ax), n2.dot(ax));
    // lambda^2 X^2 - lambda H + K = 0, kappa = w^2 lambda / X
    let h = e * n + g * l - 2.0 * f * m;
    let k = l * n - m * m;
    let a = x * x;
    let disc = (h * h - 4.0 * a * k).max(0.0).sqrt();
    let scale = w * w / x;
    Some((
        scale * (h - disc) / (2.0 * a),
        scale * (h + disc) / (2.0 * a),
    ))
}

/// The radius `1 / k` of the largest curvature `k` on the side `side` (the sign of the
/// distance) of the patch at `(s, t)`; infinite where it bends away or the normal vanishes.
fn wall(patch: &Patch, side: f64, s: f64, t: f64) -> f64 {
    match principal_curvatures(patch, s, t) {
        Some((k1, k2)) => {
            let k = (k1 * side).max(k2 * side);
            if k > 0.0 {
                1.0 / k
            } else {
                f64::INFINITY
            }
        }
        None => f64::INFINITY,
    }
}

/// The smallest radius of curvature on the side `side` over `region`: a 9 x 9 sample
/// grid per patch, then a pattern search about the smallest sample. A point where the
/// normal vanishes says nothing of the curvature; infinite where no sample bends that way.
fn largest_wall(region: &[Patch], side: f64) -> Result<f64> {
    let mut best = (f64::INFINITY, 0usize, 0.5, 0.5);
    for (i, patch) in region.iter().enumerate() {
        for a in 0..=8 {
            for c in 0..=8 {
                let (s, t) = (f64::from(a) / 8.0, f64::from(c) / 8.0);
                let r = wall(patch, side, s, t);
                if r < best.0 {
                    best = (r, i, s, t);
                }
            }
        }
    }
    let (mut r, i, mut s, mut t) = best;
    let patch = region.get(i).ok_or_else(|| invalid("no patch"))?;
    let mut step = 1.0 / 16.0;
    for _ in 0..60 {
        let mut moved = false;
        for (ds, dt) in [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)] {
            let (s2, t2) = ((s + ds).clamp(0.0, 1.0), (t + dt).clamp(0.0, 1.0));
            let r2 = wall(patch, side, s2, t2);
            if r2 < r {
                (r, s, t, moved) = (r2, s2, t2, true);
            }
        }
        if !moved {
            step *= 0.5;
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quadratic Bezier `(-10, 0), (0, 10), (10, 0)` in xy extruded 20 along z
    /// (`two_spans`: a knot inserted at 1/2), u along the curve, v along z.
    pub(crate) fn parabola(two_spans: bool) -> Nurb {
        let (pts, uk): (Vec<(f64, f64)>, Vec<f64>) = if two_spans {
            (
                vec![(-10.0, 0.0), (-5.0, 5.0), (5.0, 5.0), (10.0, 0.0)],
                vec![0., 0., 0., 0.5, 1., 1., 1.],
            )
        } else {
            (
                vec![(-10.0, 0.0), (0.0, 10.0), (10.0, 0.0)],
                vec![0., 0., 0., 1., 1., 1.],
            )
        };
        let net = pts
            .iter()
            .map(|&(x, y)| vec![CPoint::new(x, y, 0.0, NOW), CPoint::new(x, y, 20.0, NOW)])
            .collect();
        Nurb::new(net, 2, 1, uk, vec![0., 0., 1., 1.]).unwrap()
    }

    /// The rational quarter cylinder r10 about z, 20 high, normal outward (u around,
    /// v up: `S_u x S_v` points away from the axis).
    fn quarter_cylinder() -> Nurb {
        let c = core::f64::consts::FRAC_1_SQRT_2;
        let net = [(10.0, 0.0, 1.0), (10.0, 10.0, c), (0.0, 10.0, 1.0)]
            .iter()
            .map(|&(x, y, w)| {
                vec![
                    CPoint::new(x * w, y * w, 0.0, w),
                    CPoint::new(x * w, y * w, 20.0 * w, w),
                ]
            })
            .collect();
        Nurb::new(
            net,
            2,
            1,
            vec![0., 0., 0., 1., 1., 1.],
            vec![0., 0., 1., 1.],
        )
        .unwrap()
    }

    /// `|G - (S + d n)|` at a fresh grid of parameters, its largest value.
    fn sampled_error(s: &Nurb, g: &Nurb, d: f64) -> f64 {
        let (ub, vb, ps) = patches(s).unwrap();
        let (gub, gvb, gp) = patches(g).unwrap();
        let mut worst: f64 = 0.0;
        for i in 0..=100 {
            for j in 0..=100 {
                // irrational-ish steps, away from the interpolation nodes
                let u = (f64::from(i) * 0.618_034 / 100.0 + 0.001).fract();
                let v = (f64::from(j) * 0.414_214 / 100.0 + 0.002).fract();
                let (p, n) = point_and_normal(&ub, &vb, &ps, u, v).unwrap();
                let patch = &gp[span_of(&gub, u)][span_of(&gvb, v)];
                let (a, b) = local(patch.range, u, v);
                let gpt = eval3(&patch.p, a, b);
                worst = worst.max((gpt - (p + n * d)).length());
            }
        }
        worst
    }

    /// `s` (a B-spline net) extended `how` past its u max by `e`: its net, the old end now an
    /// interior knot of multiplicity `p` (the joint).
    fn extended_net(s: &Nurb, how: crate::surface::extend::Continuity, e: f64) -> Nurb {
        use crate::nurbs::b_spline_surface::BSplineSurface;
        use crate::surface::side::BoxSide;
        use crate::surface::surface::{Surface, SurfaceKind};
        let k = Kernel::default();
        let src = Surface::from(BSplineSurface::from_gw_nurb(s));
        let dom = src.natural_uv_domain_with_nurb(&k).unwrap();
        let out = src
            .extended_by_distance(&k, &dom, BoxSide::UMax, how, e)
            .unwrap()
            .expect("extended")
            .surface;
        let SurfaceKind::BSpline(net) = out.kind() else {
            panic!("not a B-spline: {out:?}");
        };
        net.gw_nurb().unwrap().clone()
    }

    #[test]
    fn a_tangent_continuous_extension_joint_at_multiplicity_p_is_no_crease() {
        use crate::surface::extend::Continuity;
        let s = parabola(false);
        for how in [Continuity::Natural, Continuity::Tangent, Continuity::Ruled] {
            let g = extended_net(&s, how, 6.0);
            let (p, _) = g.degrees();
            // the joint: the old end, u = 1, now inside at multiplicity p
            let at = g.u_knots().iter().filter(|&&x| x == 1.0).count();
            assert_eq!(at, p, "{how:?}: {:?}", g.u_knots());
            for d in [2.0, -2.0] {
                let tol = 1e-7;
                let made = certified_offset(&Kernel::default(), &g, d, tol).unwrap();
                assert!(made.bound <= tol, "{how:?} d {d}: bound {}", made.bound);
                let err = sampled_error(&g, &made.surface, d);
                assert!(
                    err <= made.bound * 1.000_001 + 1e-12,
                    "{how:?} d {d}: {err}"
                );
            }
        }
    }

    #[test]
    fn a_kink_at_multiplicity_p_is_a_crease() {
        // two quadratic spans meeting at (0, 10) at multiplicity 2, their tangents there
        // (10, 5) and (10, -5): a real crease
        let pts = [
            (-10.0, 0.0),
            (-5.0, 7.5),
            (0.0, 10.0),
            (5.0, 7.5),
            (10.0, 0.0),
        ];
        let net = pts
            .iter()
            .map(|&(x, y)| vec![CPoint::new(x, y, 0.0, NOW), CPoint::new(x, y, 20.0, NOW)])
            .collect();
        let s = Nurb::new(
            net,
            2,
            1,
            vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.],
            vec![0., 0., 1., 1.],
        )
        .unwrap();
        let err = certified_offset(&Kernel::default(), &s, 2.0, 1e-7).unwrap_err();
        assert!(
            matches!(err, SolidError::InvalidInput { reason, .. } if reason.starts_with("a crease")),
            "{err:?}"
        );
        // the same joint made smooth (its neighbours on the tangent through it) is none
        let pts = [
            (-10.0, 0.0),
            (-5.0, 10.0),
            (0.0, 10.0),
            (5.0, 10.0),
            (10.0, 0.0),
        ];
        let net = pts
            .iter()
            .map(|&(x, y)| vec![CPoint::new(x, y, 0.0, NOW), CPoint::new(x, y, 20.0, NOW)])
            .collect();
        let s = Nurb::new(
            net,
            2,
            1,
            vec![0., 0., 0., 0.5, 0.5, 1., 1., 1.],
            vec![0., 0., 1., 1.],
        )
        .unwrap();
        let made = certified_offset(&Kernel::default(), &s, 2.0, 1e-7).unwrap();
        assert!(made.bound <= 1e-7);
    }

    #[test]
    fn radius_classes() {
        assert_eq!(classify_radius(1.0, 1e-6), SheetClass::Regular);
        assert_eq!(classify_radius(-1.0, 1e-6), SheetClass::Inverted);
        assert_eq!(classify_radius(1e-9, 1e-6), SheetClass::Spine);
        assert_eq!(classify_radius(-1e-9, 1e-6), SheetClass::Spine);
    }

    #[test]
    fn parabola_offsets_are_certified_and_agree_at_fresh_points() {
        for two in [false, true] {
            let s = parabola(two);
            for d in [2.0, -2.0, 5.0, -11.0] {
                let tol = 1e-7;
                let made = certified_offset(&Kernel::default(), &s, d, tol).unwrap();
                assert!(made.bound <= tol, "bound {}", made.bound);
                let err = sampled_error(&s, &made.surface, d);
                assert!(
                    err <= made.bound * 1.000_001 + 1e-12,
                    "d {d}: {err} > {}",
                    made.bound
                );
            }
        }
    }

    #[test]
    fn rational_cylinder_offset_is_certified() {
        let s = quarter_cylinder();
        for d in [1.0, -3.0] {
            let made = certified_offset(&Kernel::default(), &s, d, 1e-7).unwrap();
            assert!(made.bound <= 1e-7);
            let err = sampled_error(&s, &made.surface, d);
            assert!(err <= made.bound * 1.000_001 + 1e-12);
            // the exact offset is the cylinder of radius 10 + d
            let (ub, vb, gp) = patches(&made.surface).unwrap();
            for &(u, v) in &[(0.1, 0.3), (0.77, 0.5), (0.5, 0.9)] {
                let patch = &gp[span_of(&ub, u)][span_of(&vb, v)];
                let (a, b) = local(patch.range, u, v);
                let p = eval3(&patch.p, a, b);
                let r = (p.x * p.x + p.y * p.y).sqrt();
                assert!((r - (10.0 + d)).abs() <= 1e-7, "{r}");
            }
        }
    }

    #[test]
    fn parabola_folds_past_its_apex_radius() {
        // 1 / k at the apex (x = 0) is 10: the normal S_u x S_v points to -y, at the
        // centre of curvature (0, -5), so a positive distance moves towards it
        for two in [false, true] {
            let s = parabola(two);
            assert_eq!(
                certify_regular(&Kernel::default(), &s, 2.0, None).unwrap(),
                Regularity::Regular
            );
            assert_eq!(
                certify_regular(&Kernel::default(), &s, 5.0, None).unwrap(),
                Regularity::Regular
            );
            assert_eq!(
                certify_regular(&Kernel::default(), &s, 9.5, None).unwrap(),
                Regularity::Regular
            );
            assert_eq!(
                certify_regular(&Kernel::default(), &s, -11.0, None).unwrap(),
                Regularity::Regular
            );
            let Regularity::Folds { largest } =
                certify_regular(&Kernel::default(), &s, 11.0, None).unwrap()
            else {
                panic!("t11 towards the centre folds");
            };
            assert!((largest - 10.0).abs() < 1e-9, "{largest}");
        }
    }

    /// A flat patch (z = 0) whose top row leaves its corner along its side, so the normal
    /// vanishes there: no curvature to size a wall by, whichever side, and no proof.
    #[test]
    fn a_flat_patch_tangent_at_a_corner_names_no_wall() {
        let pts = [
            [(0.0, 0.0), (0.0, 10.0)],
            [(5.0, 0.0), (0.0, 15.0)],
            [(10.0, 0.0), (5.0, 15.0)],
        ];
        let net = pts
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&(x, y)| CPoint::new(x, y, 0.0, NOW))
                    .collect()
            })
            .collect();
        let s = Nurb::new(
            net,
            2,
            1,
            vec![0., 0., 0., 1., 1., 1.],
            vec![0., 0., 1., 1.],
        )
        .unwrap();
        for d in [1.0, -1.0] {
            assert_eq!(
                certify_regular(&Kernel::default(), &s, d, None).unwrap(),
                Regularity::NotCertified
            );
        }
    }

    #[test]
    fn rational_cylinder_folds_inside_its_radius() {
        let s = quarter_cylinder();
        assert_eq!(
            certify_regular(&Kernel::default(), &s, -9.0, None).unwrap(),
            Regularity::Regular
        );
        let Regularity::Folds { largest } =
            certify_regular(&Kernel::default(), &s, -10.5, None).unwrap()
        else {
            panic!("an inset past the radius folds");
        };
        assert!((largest - 10.0).abs() < 1e-9, "{largest}");
    }

    /// The quarter cylinder's homogeneous net degree-elevated to (`p`, `q`) (each step
    /// `Q_i = i/(n+1) P_(i-1) + (1 - i/(n+1)) P_i` on the weighted points): the same
    /// surface, of a G2 round's degrees.
    fn elevated_quarter_cylinder(p: usize, q: usize) -> Nurb {
        let lerp = |a: CPoint, b: CPoint, t: f64| {
            CPoint::new(
                a.x * t + b.x * (1.0 - t),
                a.y * t + b.y * (1.0 - t),
                a.z * t + b.z * (1.0 - t),
                a.w * t + b.w * (1.0 - t),
            )
        };
        let elevate = |row: &[CPoint]| -> Vec<CPoint> {
            let n = row.len() - 1;
            let mut out = vec![row[0]];
            for i in 1..=n {
                let t = i as f64 / (n + 1) as f64;
                out.push(lerp(row[i - 1], row[i], t));
            }
            out.push(row[n]);
            out
        };
        let base = quarter_cylinder();
        let mut net: Vec<Vec<CPoint>> = base.cpts();
        while net.len() < p + 1 {
            let columns: Vec<Vec<CPoint>> = (0..net[0].len())
                .map(|j| elevate(&net.iter().map(|r| r[j]).collect::<Vec<_>>()))
                .collect();
            net = (0..columns[0].len())
                .map(|i| columns.iter().map(|c| c[i]).collect())
                .collect();
        }
        while net[0].len() < q + 1 {
            net = net.iter().map(|r| elevate(r)).collect();
        }
        let knots = |d: usize| {
            let mut k = vec![0.0; d + 1];
            k.extend(vec![1.0; d + 1]);
            k
        };
        Nurb::new(net, p, q, knots(p), knots(q)).unwrap()
    }

    /// #2487: a rational patch of a G2 round's degrees (septic across, cubic along: the
    /// quarter cylinder elevated) is certified regular outside its radius and folds inside
    /// it at that radius. Its regularity form `Q` is of degree 112 across, past the
    /// Bernstein forms' cap, which refused it (and with it the offset of every body with a
    /// G2 round, status 1001); with `w` divided out it is of degree 80.
    #[test]
    fn a_septic_rational_patch_is_certified_as_its_cylinder() {
        let s = elevated_quarter_cylinder(7, 3);
        let k = Kernel::default();
        assert_eq!(
            certify_regular(&k, &s, 2.0, None).unwrap(),
            Regularity::Regular
        );
        assert_eq!(
            certify_regular(&k, &s, -9.0, None).unwrap(),
            Regularity::Regular
        );
        let Regularity::Folds { largest } = certify_regular(&k, &s, -10.5, None).unwrap() else {
            panic!("an inset past the radius folds");
        };
        assert!((largest - 10.0).abs() < 1e-9, "{largest}");
    }

    /// A cylinder of radius 10 offset towards its axis: by 10 the offset collapses onto
    /// the axis (refused, the radius named), short of 10 by more than the resolution it
    /// does not, past 10 by more it folds; offset away from the axis, never. A distance
    /// that is not finite is refused as input.
    #[test]
    fn a_round_offset_by_its_radius_is_proved_to_collapse() {
        let s = quarter_cylinder();
        let res = 1e-6;
        let at = proved_singular(&s, -10.0, None, res).unwrap().unwrap();
        assert!(at.collapses, "{at:?}");
        assert!((at.radius - 10.0).abs() < 1e-12, "{at:?}");
        // within the resolution either side of the radius: still a collapse
        for d in [-10.0 + 0.5e-6, -10.0 - 0.5e-6] {
            assert!(
                proved_singular(&s, d, None, res)
                    .unwrap()
                    .unwrap()
                    .collapses
            );
        }
        assert_eq!(proved_singular(&s, -9.99, None, res).unwrap(), None);
        let folds = proved_singular(&s, -10.5, None, res).unwrap().unwrap();
        assert!(
            !folds.collapses && (folds.radius - 10.0).abs() < 1e-12,
            "{folds:?}"
        );
        for d in [10.0, 25.0, 0.0] {
            assert_eq!(proved_singular(&s, d, None, res).unwrap(), None, "{d}");
        }
        assert!(proved_singular(&s, f64::NAN, None, res).is_err());
        assert!(proved_singular(&s, -10.0, None, -1.0).is_err());
    }
}
