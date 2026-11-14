//! The G^n section's certificate: a proved bound on how far a Hermite span between two
//! stations is from the section of the true jets, from rounded interval arithmetic (the
//! feet over the span enclosed by Krawczyk's operator, their Taylor coefficients order by
//! order, and the Hermite remainder `h^4 / 384 max |c''''|`).
//!
//! For the rational section the blend's distance from its definition (the section of the
//! true jets `c_ik(s)` at every s) is at most `max_i sum_k |c_ik - C_ik|`, `C_ik` the jets'
//! Hermite curves: f is in `[0, 1]` and `|u - 1| <= 1` in `T_i`. On a span of length h a
//! cubic Hermite interpolant of a function's values and derivatives at both ends is off it
//! by at most `h^4 / 384` times the largest fourth derivative. That bound comes from
//! rounded interval arithmetic: the feet over the span are enclosed by Krawczyk's operator,
//! which proves a unique root of G in its box for every s in the span when it maps the box
//! into its interior and the preconditioned Jacobian's norm is below one; the feet's Taylor
//! coefficients, to order 4 in s and n in tau, are then enclosed order by order through the
//! same preconditioned system.
//!
//! Fork: `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib@11ea2d9), the
//! certificate (lines 825-1430); not in the pinned upstream.
//!
//! | Rust                              | Fork (`SmFilletSection.cpp`)            |
//! |-----------------------------------|-----------------------------------------|
//! | [`NurbsCurve`], [`NurbsCurve::of`] | `sm_NurbsCurve`, `sm_NurbsCurveOf`     |
//! | [`curve_derivs_over`]             | `sm_CurveDerivsOver`                    |
//! | [`nurbs_partials_over_piece`]     | `sm_NurbsPartialsOverPiece`             |
//! | [`leader_series_over`]            | `sm_LeaderSeriesOver`                   |
//! | [`GnLaw`]                         | `sm_GnLaw` (0d43b4c)                    |
//! | [`law_series`]                    | `sm_LawSeries` (0d43b4c)                |
//! | [`law_series_over`]               | `sm_LawSeriesOver` (0d43b4c)            |
//! | [`inverse4`]                      | `sm_Inverse4`                           |
//! | [`Enclosure`], [`gn_enclose`]     | `sm_GnEnclose` (its `rF`, `pX`, `pDS`)  |
//! | [`SpanAt`], [`gn_enclose_over`], [`face_partials_over`] | `sm_GnSpanAt`, `sm_GnEncloseOver`, `sm_FacePartialsOver` (c5b6bb7) |
//! | [`gn_fourth_over`]                | `sm_GnFourthOver`                       |
//! | [`gn_certify_span`]               | `sm_GnCertifySpan`                      |
//! | [`gn_certify_span_reusing`], [`SpanReuse`], [`CertifiedLevel`], [`CertifiedPiece`] | — (not the fork's: a halved span's halves start from its pieces) |
//! | [`arc_row_terms_over`]            | `sm_ArcRowTermsOver` (cc08236; its terms the tighter of two, #14405) |
//! | [`ArcTerms`], [`ArcTermsBoth`], [`arc_terms_over`], [`centred_tm`] | — (not the fork's: the arc's row terms from interval series and from Taylor models, #14405) |
//! | `PieceRows`, `tighten_level` (private) | — (not the fork's: a level's Taylor models taken only where they can change its verdict, #14405) |
//! | [`TightPiece`], [`PieceInputs`], `resolve_pending`, `tighten_feet`, `gate` (private) | — (not the fork's: the feet's Taylor models, [`super::feet_tm`], taken only where they can change a level's verdict, #14405) |
//! | `gn_try!` (private)               | `SM_GN_TRY`                             |
//! | [`EndContinuation`], [`ConeAxis`], [`nurbs_partials_over_box_ending`] | (not the fork's: #14284) |
//! | `enclose_by_cells`, `enclose_over_cells`, `FaceCell` (private) | (not the fork's: the feet cell by cell across a knot line below `C^2`, #14284 follow-up) |
//!
//! Deviations:
//! - **Out-params.** Every function returns its results. `sm_GnEnclose`'s `rbProved` FALSE
//!   is `Ok(None)`; its optional `pX` and `pDS` are always returned (computing them changes
//!   nothing else). `sm_CurveDerivsOver`'s `lPiece = -1` is `None`.
//!   `sm_NurbsPartialsOverPiece` returns fresh partials instead of overwriting the low
//!   orders of the caller's (the orders it leaves are never read).
//! - **`SM_GN_TRY`.** A failed evaluation inside the certificate proves nothing, as in the
//!   fork: `gn_enclose` returns `Ok(None)`, `gn_fourth_over` its unproved (infinite)
//!   bounds, `gn_certify_span` the bound it has so far. Only a truncation past the series'
//!   fixed size (`n > SM_GN_MAXE`, which the fork leaves to its callers) is an `Err`.
//! - **Bounds.** Every loop is the fork's, with its fixed counts (8 Krawczyk steps, 200
//!   contraction steps, levels 0..=`SPAN_MAX_LEVEL` = 6); there is no recursion.
//! - **Cancellation (#2136, #2371).** `gn_certify_span` polls the kernel's cancel token
//!   once per level, per piece's end station and per piece's bound; a cancel passes through
//!   `SM_GN_TRY` and the unsolved-piece fallback as `Cancelled` instead of proving
//!   nothing. A token that never fires changes nothing.
//! - **The uv curves' term (#2690).** Not the fork's: a face's point on a foot's uv curve
//!   is off the true foot by at most `|S_u| |du| + |S_v| |dv|`, each uv error `du`, `dv`
//!   bounded by its own parameter's fourth derivative, where the fork takes
//!   `(|S_u| + |S_v|) max(|du|, |dv|)`. The fork's form is never smaller, and on a face
//!   whose `u` (say) moves little while `|S_u|` is large it pairs that large partial with
//!   the other parameter's error and refines spans the tolerance does not need.
//!   `gn_certify_span` certifies a level by this bound but steers its levels (the least
//!   value it stops on, the halving it goes on with) by the fork's form, so a span takes no
//!   more levels or pieces than the fork's, and is certified at the same level or sooner;
//!   steered by the tighter bound, a round on a round took more spans and solves.
//! - **A halved span's halves (not the fork's).** The fork certifies each half of a span
//!   its caller halved afresh, from level 0. [`gn_certify_span_reusing`] takes the halved
//!   span's finest proved level instead: its pieces over a half cover it, and each piece's
//!   bounds hold over all of it, so they are the half's level one coarser (the half is half
//!   as long), proved without an enclosure; the half's levels go on from the next one.
//!   Only a piece inside the half steers the least-value stop; the row terms are taken again
//!   about the half's first station. The bounds are as sound as afresh; a half skips the
//!   coarser levels (which a long span's halves never prove before the inherited one), and
//!   its bound can differ from the fork's in the last digits (its pieces' ends are the
//!   parent's stations, not its own).
//! - **A span's first level (not the fork's).** Where a span inherits no level, the section
//!   build starts its levels one coarser than the level the span before first proved
//!   ([`SpanReuse::start`]); a level not tried bounds nothing, so the bound returned is as
//!   sound as the fork's, but it can be another level's. A level's end stations that the
//!   level before solved (every other one: the same s from the same seed) are taken from
//!   it, bit for bit what solving them again gives.
//! - **The arc's row terms (#14405, not the fork's).** A piece's arc rows (the ball's
//!   circle, n = 1) are bounded from interval series over the piece, as the fork bounds
//!   them, and from Taylor models in `s - m` over it ([`super::taylor_model`],
//!   [`arc_terms_over`]); each term is the tighter of the two, so no bound is looser. The
//!   interval series take the rails and the centre, which move together, as independent
//!   ranges over the piece: on a long piece the circle's radii reach length 0 or the rows'
//!   fourth derivatives come out orders of magnitude over their true values, and a long
//!   variable round proved only on pieces some hundredths of it long. The levels are
//!   steered by the fork's terms (a level they do not prove goes on, a bound not finite by
//!   the fork's uv term skips, the halving compares their bounds), and only the stop at the
//!   target reads the tighter bound: from a given first level, a span the fork's terms
//!   certify these certify at the same level or a coarser one, and a span they leave over
//!   the target these leave with a bound no larger, or certify. A piece takes its Taylor
//!   models at once only where its interval terms do not prove it; elsewhere the level
//!   takes them only when its bound with the interval terms is not at the target
//!   (`tighten_level`), so the verdicts and the bounds over the target are those of the
//!   tighter terms everywhere, and a bound at the target can be the interval terms' (no
//!   larger than the fork's). A span's first level follows the levels these certified
//!   ([`SpanReuse`]), so the spans' sequence can differ from the fork's terms'.
//! - **The feet's Taylor models (#14405, not the fork's).** The same for the feet: a
//!   piece's feet are also enclosed as Taylor models in `s - m` over it
//!   ([`super::feet_tm::gn_enclose_tm`]), whose top coefficient is not the interval
//!   enclosure's overestimate (the faces' rational quotient and the feet's common motion
//!   taken as independent ranges); each fourth term is the tighter of the two
//!   ([`TightPiece`]), and the arc's rows are taken from the tighter series met with the
//!   interval ones'. Only on NURBS faces (not a blend by its definition), and not where
//!   the equal area thumbweight varies: those rows' own interval terms keep its levels
//!   over the target with the tighter feet or without. A piece whose interval enclosure
//!   does not prove it takes them once the level's other pieces are in
//!   (`resolve_pending`), and a level whose bound is still over the target after the
//!   arc's takes them for its other pieces (`tighten_feet`); neither where a lower bound
//!   on the level's bound whatever its pieces take (`gate`: the least values at the
//!   span's pieces' middles so far, `piece_least`, and the pieces' terms without the
//!   remainder over them, `floor_terms`) is over the target, where they could not bring
//!   the level to it. The levels are steered by the fork's terms as above, so the
//!   verdicts go only from reject to accept.
//! - **Supports' ends (not the fork's, tangency-labs/solidcraft#14284).** The fork refuses
//!   every feet's box past a face's knots, so a round ending flush with its support's end
//!   (a foot on the net's domain edge) proves nothing on its last pieces. Here a support whose
//!   kind is a line along one of its NURB directions continues that direction past the net:
//!   a plane in both, a cone (a cylinder among them) or an extrusion along its line
//!   ([`EndContinuation::of`]), where the net is of degree 1 in it; a cone's continuation
//!   may not reach its apex, the cone's one point on its axis ([`ConeAxis`], from the cone's
//!   definition): the part taken back to the end it lies past is proved off the axis, so a
//!   part wholly past the apex, on the other nappe, is refused too.
//!   The part of the box past the end takes the end span's polynomial, which for a degree-1
//!   direction is the face's edge jet continued, `P(u, v_e) + (v - v_e) P_v(u, v_e)`: the
//!   same polynomial on both sides of the end, so the feet's system is smooth to every order
//!   across it, which `FEET_BOX_CONTINUITY = 2` (the face C^2 across a line a box crosses,
//!   the system C^1 for Krawczyk's mean-value form) needs. That this continuation is the
//!   support's definition (the plane's point, the cylinder's or extrusion's line at that
//!   parameter) is the kind's, and the closed-form tests show it. Every other support, a
//!   cone's part reaching its apex, and a direction of degree above 1 are refused past their end by name
//!   ([`crate::error::Reason::FootPastDomainEnd`]), the one refusal `SM_GN_TRY` passes on;
//!   any other failed evaluation still proves nothing.
//! - **Knot lines below `C^2` (not the fork's, tangency-labs/solidcraft#14284 follow-up).**
//!   The fork refuses a feet's box across a face's knot line below `FEET_BOX_CONTINUITY` by
//!   count. A pointwise use of the feet (order 1 in s or less, a piecewise enclosure without
//!   definitions) takes them cell by cell there instead: each piece the box touches,
//!   continued, solved on its own, decided over one common box and hulled; a cell not
//!   decided, or two feet apart, is refused by name ([`gn_enclose_over`]). A rational
//!   circle's doubled quarter knot is such a line (`C^0` by count, `C^1` only to rounding).
//!   Every other box takes the fork's one solve unchanged.
//! - **Indices.** Knots and nets are read by checked indices (a knot or a coordinate
//!   outside reads NaN, which shows in the result); the fork's indices stay inside.

use super::definition_box::{definition_partials_over_box, GnDefFace, GnDefs};
use super::form::{binom, gn_form_normalized, GnSectionFormT};
use super::interval::{cmax, cmin, round_down, round_up, Ivl};
use super::series::{v_hull, GnScalar, Ser, Trunc, VSer, MAX_E, MAX_S, TAYLOR, V3, VZ};
use super::station::{
    ders_basis, gn_evaluate, gn_evaluate_vn, gn_jacobian, gn_solve_station_counted, gn_top_jets,
    knot, smax, solve4, span, GnStation, GnSystem, Nurbs, Partials,
};
use super::tay::{
    gn_half_angle, gn_invert, gn_target, tay_sqrt, GnAreaCells, Ring, Tay, TayScalar, TAY,
};
use super::taylor_model::Tm;
use crate::error::{Result, SolidError};
use crate::geom::krawczyk::{contraction, k_operator};
use crate::kernel::Kernel;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::surface::surface::Surface;

const OP: &str = "blend::fillet_section::certificate";
/// The most Krawczyk steps that tighten a proved feet's box ([`gn_enclose_over`]).
const TIGHTEN_STEPS: usize = 2;
/// The finest level [`gn_certify_span`] cuts a span to: `2^SPAN_MAX_LEVEL` equal pieces
/// (the fork's).
pub(crate) const SPAN_MAX_LEVEL: u32 = 6;
const OP_CERTIFY_SPAN: &str = "blend::fillet_section::certificate::gn_certify_span";

/// `SM_GN_TRY(a)`: a failed evaluation proves nothing, the enclosing function returns
/// `Ok($none)` (the fork's `return SM_SUCCESS` with its outputs as they stand).
macro_rules! gn_try {
    ($e:expr, $none:expr) => {
        match $e {
            Ok(v) => v,
            // a cancel (#2136) is never a failed evaluation: it ends the operation
            Err(e @ SolidError::Cancelled { .. }) => return Err(e),
            // a foot past its face's domain end is refused by name (#14284)
            Err(e) if is_foot_past_domain_end(&e) => return Err(e),
            Err(_) => return Ok($none),
        }
    };
}

/// `sm_NurbsCurve`: a NURBS curve's data (the leader), as [`Nurbs`].
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    /// The degree.
    pub p: usize,
    /// The number of control points.
    pub n: usize,
    /// The full knot vector.
    pub u: Vec<f64>,
    /// The homogeneous control points `(x w, y w, z w, w)`.
    pub pw: Vec<f64>,
}

impl NurbsCurve {
    /// `sm_NurbsCurveOf`: the data of a B-spline curve.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the curve has no NURB, its degree, knots and control points do not
    /// agree, or a weight is not positive.
    pub fn of(c: &BSplineCurve) -> Result<Self> {
        let (knots, mult) = c.knots(None)?;
        let (cps, ws) = c.control_polygon()?;
        let p = c.degree()?;
        let n = cps.len();
        let mut u = Vec::new();
        for (&k, &m) in knots.iter().zip(&mult) {
            for _ in 0..m {
                u.push(k);
            }
        }
        if p < 1 || u.len() != n + p + 1 {
            // "SmGnSectionFSG: the centre line's NURBS data do not agree"
            return Err(SolidError::blend_failed(
                OP,
                "the centre line's NURBS data do not agree",
            ));
        }
        let rational = ws.len() == cps.len();
        let mut pw = Vec::new();
        for (i, cp) in cps.iter().enumerate() {
            let w = if rational {
                ws.get(i).copied().unwrap_or(f64::NAN)
            } else {
                1.0
            };
            let positive = w > 0.0;
            if !positive {
                return Err(SolidError::blend_failed(
                    OP,
                    "the centre line's weights are not positive",
                ));
            }
            pw.extend_from_slice(&[cp.x * w, cp.y * w, cp.z * w, w]);
        }
        Ok(Self { p, n, u, pw })
    }
}

/// `crPw[i]` (NaN outside).
#[inline]
fn pw_at(pw: &[f64], i: usize) -> f64 {
    pw.get(i).copied().unwrap_or(f64::NAN)
}

/// `rM[i][j]` of a nested vector (0 outside; the fork's indices stay inside).
#[inline]
fn at2<T: Copy>(m: &[Vec<T>], i: usize, j: usize, zero: T) -> T {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(zero)
}

/// `v[i]` of a vector (0 outside).
#[inline]
fn at1<T: Copy>(v: &[T], i: usize, zero: T) -> T {
    v.get(i).copied().unwrap_or(zero)
}

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// `sm_CurveDerivsOver`: the derivatives `D[0..=ld]` of the curve's polynomial piece
/// `piece` (the span of the knots it starts at; `None`: each piece `s` covers) for every
/// parameter in `s`, enclosed (hulled over the pieces).
///
/// # Errors
///
/// `SM_ERR` when no piece covers `s`.
pub fn curve_derivs_over(
    cn: &NurbsCurve,
    s: Ivl,
    ld: usize,
    piece: Option<usize>,
) -> Result<Vec<V3<Ivl>>> {
    let k0 = piece.unwrap_or_else(|| span(&cn.u, cn.p, cn.n, s.lo));
    let k1 = piece.unwrap_or_else(|| span(&cn.u, cn.p, cn.n, s.hi));
    let fixed = piece.is_some();
    let p = cn.p;
    let mut first = true;
    let mut rd = vec![VZ; ld + 1];
    for k in k0..=k1 {
        let ki = k as isize;
        let has_span = knot(&cn.u, ki) < knot(&cn.u, ki + 1);
        if !has_span {
            continue; // a knot repeated: no span
        }
        let a = if k == p || fixed {
            s.lo
        } else {
            cmax(s.lo, knot(&cn.u, ki))
        };
        let b = if k + 1 == cn.n || fixed {
            s.hi
        } else {
            cmin(s.hi, knot(&cn.u, ki + 1))
        };
        // an interval that ends on a knot is the span it lies in, not also the next one's
        // end point
        if a > b || (a == b && s.lo < s.hi) {
            continue;
        }
        // the homogeneous curve is a polynomial of degree p on the span: its derivatives
        // over [a, b] are its Taylor polynomials at the middle m, from exact point
        // derivatives (rounding only)
        let m = 0.5 * (a + b);
        let nb = ders_basis(&cn.u, k, p, Ivl::from(m), p);
        let mut am = vec![VZ; p + 1];
        let mut wm = vec![IZ; p + 1];
        for d in 0..=p {
            let (mut x, mut y, mut z, mut w) = (IZ, IZ, IZ, IZ);
            for j in 0..=p {
                // k >= p (sm_Span)
                let li = 4 * (k + j - p);
                let nj = at2(&nb, d, j, IZ);
                x = x + nj * Ivl::from(pw_at(&cn.pw, li));
                y = y + nj * Ivl::from(pw_at(&cn.pw, li + 1));
                z = z + nj * Ivl::from(pw_at(&cn.pw, li + 2));
                w = w + nj * Ivl::from(pw_at(&cn.pw, li + 3));
            }
            if let (Some(ad), Some(wd)) = (am.get_mut(d), wm.get_mut(d)) {
                *ad = V3 { x, y, z };
                *wd = w;
            }
        }
        let dt = Ivl::new(a, b) - Ivl::from(m);
        let mut av = vec![VZ; ld + 1];
        let mut wv = vec![IZ; ld + 1];
        for d in 0..=ld {
            let (mut x, mut y, mut z, mut w, mut pwr) = (IZ, IZ, IZ, IZ, Ivl::from(1.0));
            let mut f = 1.0_f64;
            for j in d..=p {
                if j > d {
                    pwr = pwr * dt;
                    f *= (j - d) as f64;
                }
                let c = pwr * (Ivl::from(1.0) / Ivl::from(f));
                let aj = at1(&am, j, VZ);
                x = x + aj.x * c;
                y = y + aj.y * c;
                z = z + aj.z * c;
                w = w + at1(&wm, j, IZ) * c;
            }
            if let (Some(ad), Some(wd)) = (av.get_mut(d), wv.get_mut(d)) {
                *ad = V3 { x, y, z };
                *wd = w;
            }
        }
        let w0 = at1(&wv, 0, IZ);
        let mut cv = vec![VZ; ld + 1];
        for d in 0..=ld {
            let mut v = at1(&av, d, VZ);
            let mut c = 1.0_f64;
            for i in 1..=d {
                c = c * (d - i + 1) as f64 / i as f64;
                let wi = at1(&wv, i, IZ);
                let cdi = at1(&cv, d - i, VZ);
                v.x = v.x - Ivl::from(c) * wi * cdi.x;
                v.y = v.y - Ivl::from(c) * wi * cdi.y;
                v.z = v.z - Ivl::from(c) * wi * cdi.z;
            }
            if let Some(cd) = cv.get_mut(d) {
                *cd = V3 {
                    x: v.x / w0,
                    y: v.y / w0,
                    z: v.z / w0,
                };
            }
        }
        for (r, c) in rd.iter_mut().zip(&cv) {
            if first {
                *r = *c;
            } else {
                *r = v_hull(*r, *c);
            }
        }
        first = false;
    }
    if first {
        return Err(SolidError::blend_failed(
            OP,
            "no piece covers the parameter",
        ));
    }
    Ok(rd)
}

/// `sm_NurbsPartialsOverPiece`: the partials `P[a][b]`, `a + b <= lk`, of one polynomial
/// piece (spans `ku`, `kv`) over `[ua, ub] x [va, vb]`, enclosed: the homogeneous surface's
/// Taylor polynomials at the box's middle, from exact point derivatives (it is of degree
/// p, q there), then the rational quotient's (the NURBS book's A4.4) in intervals.
///
/// # Errors
///
/// `SM_ERR` where the weights are not proved positive over the box ("a face's weights are
/// not positive over a box").
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_piece(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk, None)
}

/// [`nurbs_partials_over_piece`] for a box inside the piece's closed knot span (not the
/// fork's): there the weight is a convex combination of the piece's control weights, so it
/// lies between their least and greatest, and its enclosure is cut to that range before
/// the quotient. Where the Taylor enclosure of the weight alone holds 0 (a rational piece of
/// high degree over a large box) this proves it positive. A box past the span is enclosed
/// as [`nurbs_partials_over_piece`] encloses it.
///
/// # Errors
///
/// As [`nurbs_partials_over_piece`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_within_span(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    let inside = |u: &[f64], k: usize, a: f64, b: f64| {
        knot(u, k as isize) <= a && b <= knot(u, k as isize + 1)
    };
    let hull = if inside(&cn.u, ku, ua, ub) && inside(&cn.v, kv, va, vb) {
        let mut h: Option<Ivl> = None;
        for i in (ku + 1).saturating_sub(cn.p + 1)..=ku {
            for j in (kv + 1).saturating_sub(cn.q + 1)..=kv {
                let w = Ivl::from(pw_at(&cn.pw, 4 * (i * cn.nv + j) + 3));
                h = Some(h.map_or(w, |x| x.hull(w)));
            }
        }
        h
    } else {
        None
    };
    partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk, hull)
}

/// [`nurbs_partials_over_piece`], the weight's enclosure cut to `weights` where given.
#[allow(clippy::too_many_arguments)]
fn partials_over_piece(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    weights: Option<Ivl>,
) -> Result<Partials<Ivl>> {
    let um = 0.5 * (ua + ub);
    let vm = 0.5 * (va + vb);
    let nu = ders_basis(&cn.u, ku, cn.p, Ivl::from(um), cn.p);
    let nv = ders_basis(&cn.v, kv, cn.q, Ivl::from(vm), cn.q);
    let (la, lb) = (cn.p, cn.q);
    // homogeneous point derivatives Hm[a][b] (x, y, z, w) at the middle
    let mut hm = vec![vec![[IZ; 4]; lb + 1]; la + 1];
    for (a, row) in hm.iter_mut().enumerate() {
        for (b, hab) in row.iter_mut().enumerate() {
            let mut h = [IZ; 4];
            for i in 0..=cn.p {
                for j in 0..=cn.q {
                    // ku >= p and kv >= q (sm_Span)
                    let li = 4 * ((ku + i - cn.p) * cn.nv + (kv + j - cn.q));
                    let c = at2(&nu, a, i, IZ) * at2(&nv, b, j, IZ);
                    for (e, he) in h.iter_mut().enumerate() {
                        *he = *he + c * Ivl::from(pw_at(&cn.pw, li + e));
                    }
                }
            }
            *hab = h;
        }
    }
    let du = Ivl::new(ua, ub) - Ivl::from(um);
    let dv = Ivl::new(va, vb) - Ivl::from(vm);
    let mut pu = vec![IZ; la + 1];
    let mut pv = vec![IZ; lb + 1];
    if let (Some(u0), Some(v0)) = (pu.first_mut(), pv.first_mut()) {
        *u0 = Ivl::from(1.0);
        *v0 = Ivl::from(1.0);
    }
    for i in 1..=la {
        let x = at1(&pu, i - 1, IZ) * du * (Ivl::from(1.0) / Ivl::from(i as f64));
        if let Some(pi) = pu.get_mut(i) {
            *pi = x;
        }
    }
    for j in 1..=lb {
        let x = at1(&pv, j - 1, IZ) * dv * (Ivl::from(1.0) / Ivl::from(j as f64));
        if let Some(pj) = pv.get_mut(j) {
            *pj = x;
        }
    }
    let mut av = vec![vec![VZ; lk + 1]; lk + 1];
    let mut wv = vec![vec![IZ; lk + 1]; lk + 1];
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut h = [IZ; 4];
            for i in a..=la {
                for j in b..=lb {
                    let c = at1(&pu, i - a, IZ) * at1(&pv, j - b, IZ);
                    let hij = at2(&hm, i, j, [IZ; 4]);
                    for (he, &x) in h.iter_mut().zip(&hij) {
                        *he = *he + x * c;
                    }
                }
            }
            if let (Some(ab), Some(wab)) = (
                av.get_mut(a).and_then(|r| r.get_mut(b)),
                wv.get_mut(a).and_then(|r| r.get_mut(b)),
            ) {
                let [x, y, z, w] = h;
                *ab = V3 { x, y, z };
                *wab = w;
            }
        }
    }
    let w00 = at2(&wv, 0, 0, IZ);
    let w00 = match weights {
        Some(h) => Ivl::new(w00.lo.max(h.lo), w00.hi.min(h.hi)),
        None => w00,
    };
    let positive = w00.lo > 0.0;
    if !positive {
        // "SmGnSectionFSG: a face's weights are not positive over a box"
        return Err(SolidError::blend_failed(
            OP,
            "a face's weights are not positive over a box",
        ));
    }
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let mut v3 = at2(&av, a, b, VZ);
            let mut cka = 1.0_f64;
            for i in 0..=a {
                if i > 0 {
                    cka = cka * (a - i + 1) as f64 / i as f64;
                }
                let mut cbj = 1.0_f64;
                for j in 0..=b {
                    if j > 0 {
                        cbj = cbj * (b - j + 1) as f64 / j as f64;
                    }
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let c = Ivl::from(cka * cbj) * at2(&wv, i, j, IZ);
                    let pij = out.at(a - i, b - j);
                    v3.x = v3.x - c * pij.x;
                    v3.y = v3.y - c * pij.y;
                    v3.z = v3.z - c * pij.z;
                }
            }
            out.set(
                a,
                b,
                V3 {
                    x: v3.x / w00,
                    y: v3.y / w00,
                    z: v3.z / w00,
                },
            );
        }
    }
    Ok(out)
}

/// `sm_LeaderSeriesOver`: the leader's point and unit tangent over `s` as series in sigma
/// to `tr.s`, enclosed (the span lies in one piece of the leader: the stations hold its
/// knots). `piece`: the leader's piece whose polynomial is taken over `s` (`lPiece`,
/// c5b6bb7; `None`, the fork's -1: the one the middle of `s` lies in).
///
/// # Errors
///
/// Those of [`curve_derivs_over`]; `SM_ERR` where the tangent is not proved non-zero.
pub fn leader_series_over(
    leader: &NurbsCurve,
    s: Ivl,
    tr: Trunc,
    piece: Option<usize>,
) -> Result<(VSer<Ivl>, VSer<Ivl>)> {
    let sigma = Trunc::new(tr.s(), 0)?;
    let piece = piece.unwrap_or_else(|| span(&leader.u, leader.p, leader.n, s.mid()));
    let d = curve_derivs_over(leader, s, tr.s() + 1, Some(piece))?;
    let mut l = VSer::constant(VZ);
    let mut d1 = l;
    let mut fact = 1.0_f64;
    for j in 0..=tr.s() {
        if j > 0 {
            fact *= j as f64;
        }
        let f = Ivl::from(1.0) / Ivl::from(fact);
        let dj = at1(&d, j, VZ);
        let dj1 = at1(&d, j + 1, VZ);
        let cl = [dj.x, dj.y, dj.z];
        let cd = [dj1.x, dj1.y, dj1.z];
        for ((la, da), (&x, &y)) in l.a.iter_mut().zip(d1.a.iter_mut()).zip(cl.iter().zip(&cd)) {
            la.set(j, 0, x * f);
            da.set(j, 0, y * f);
        }
    }
    let inv = d1.dot(&d1, sigma).inv_sqrt(sigma)?;
    let t = d1.times(&inv, sigma);
    Ok((l, t))
}

/// [`leader_series_over`] over an `s` that may cross the leader's knots (not upstream): `s`
/// cut at the knots strictly inside it, each part's series on its own piece, their hull
/// coefficient by coefficient. Each coefficient encloses its derivative at every point of
/// its part (at a knot, from either side), and the feet's series use them point by point
/// (Krawczyk's test is over the feet, for each s), so the hull is an enclosure whatever
/// the leader's continuity: where it has a corner in its parameter the two sides' values
/// widen the hull, and the bounds with it. Only a leader that is not continuous at a
/// crossed knot, or an `s` past its knots, is refused.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for such a knot or `s`; those of [`leader_series_over`].
pub fn leader_series_piecewise(
    leader: &NurbsCurve,
    s: Ivl,
    tr: Trunc,
) -> Result<(VSer<Ivl>, VSer<Ivl>)> {
    let hull = |a: &mut VSer<Ivl>, b: &VSer<Ivl>| {
        for (sa, sb) in a.a.iter_mut().zip(&b.a) {
            for (ra, rb) in sa.c.iter_mut().zip(&sb.c) {
                for (x, y) in ra.iter_mut().zip(rb) {
                    *x = x.hull(*y);
                }
            }
        }
    };
    let mut out: Option<(VSer<Ivl>, VSer<Ivl>)> = None;
    for (a, b, k) in knot_cuts(&leader.u, leader.p, leader.n, s.lo, s.hi, 0)? {
        let (l, t) = leader_series_over(leader, Ivl::new(a, b), tr, Some(k))?;
        out = Some(match out {
            None => (l, t),
            Some((mut hl, mut ht)) => {
                hull(&mut hl, &l);
                hull(&mut ht, &t);
                (hl, ht)
            }
        });
    }
    out.ok_or_else(|| SolidError::blend_failed(OP, "no piece encloses the box"))
}

/// `sm_GnLaw` (0d43b4c): a ball whose radius varies, its law r(s) over the leader's
/// parameter (the x of a B-spline curve), with the curve's NURBS data.
#[derive(Clone, Debug)]
pub struct GnLaw<'a> {
    /// `pCurve`.
    pub curve: &'a BSplineCurve,
    /// `sN`.
    pub n: NurbsCurve,
}

impl<'a> GnLaw<'a> {
    /// The law of `curve` (`sLaw.pCurve = pLaw; sm_NurbsCurveOf(*pLaw, sLaw.sN)`).
    ///
    /// # Errors
    ///
    /// Those of [`NurbsCurve::of`].
    pub fn of(curve: &'a BSplineCurve) -> Result<Self> {
        Ok(Self {
            curve,
            n: NurbsCurve::of(curve)?,
        })
    }
}

/// `sm_LawSeries` (0d43b4c): r at `s` to order `order` in sigma (its derivatives past a
/// polynomial law's degree zero), `None` without a law; `before`: the law's piece before
/// `s` at one of its knots.
///
/// # Errors
///
/// Those of the law's evaluation; `SM_ERR` where the radius is not positive ("the radius
/// law is not positive").
pub fn law_series(
    law: Option<&GnLaw<'_>>,
    s: f64,
    order: usize,
    before: bool,
) -> Result<Option<Ser<f64>>> {
    let Some(law) = law else {
        return Ok(None);
    };
    let mut r = Ser::constant(0.0_f64);
    let mut derivs = order;
    if !law.curve.is_rational()? {
        derivs = derivs.min(law.curve.degree()?);
    }
    if derivs > MAX_S {
        return Err(SolidError::blend_failed(
            OP,
            "the radius law's order is past the most the series hold",
        ));
    }
    let d = law.curve.evaluate(s, derivs, !before, false)?;
    let mut fact = 1.0_f64;
    for i in 0..=derivs {
        if i > 0 {
            fact *= i as f64;
        }
        let di = d.get(i).ok_or_else(|| {
            SolidError::blend_failed(OP, "the radius law has no derivative of this order")
        })?;
        r.set(i, 0, di.x / fact);
    }
    let positive = r.at(0, 0) > 0.0;
    if !positive {
        // "SmGnSectionFSG: the radius law is not positive"
        return Err(SolidError::blend_failed(
            OP,
            "the radius law is not positive",
        ));
    }
    Ok(Some(r))
}

/// `sm_LawSeriesOver` (0d43b4c): r over the span `s` (in one piece of the law: the
/// stations hold its knots) to order `order`, enclosed.
///
/// # Errors
///
/// Those of [`curve_derivs_over`]; `SM_ERR` where r is not proved positive.
pub fn law_series_over(law: &GnLaw<'_>, s: Ivl, order: usize) -> Result<Ser<Ivl>> {
    let mut r = Ser::constant(IZ);
    let piece = span(&law.n.u, law.n.p, law.n.n, s.mid());
    let d = curve_derivs_over(&law.n, s, order, Some(piece))?;
    let mut fact = 1.0_f64;
    for i in 0..=order {
        if i > 0 {
            fact *= i as f64;
        }
        r.set(i, 0, at1(&d, i, VZ).x * (Ivl::from(1.0) / Ivl::from(fact)));
    }
    if !r.at(0, 0).is_positive() {
        return Err(SolidError::blend_failed(
            OP,
            "the radius is not proved positive",
        ));
    }
    Ok(r)
}

/// `sm_Inverse4`: the inverse of a 4 x 4 matrix (the preconditioner; any approximation
/// will do), column by column with [`solve4`].
///
/// # Errors
///
/// Those of [`solve4`] (a singular matrix).
pub fn inverse4(a: &[[f64; 4]; 4]) -> Result<[[f64; 4]; 4]> {
    let mut y = [[0.0_f64; 4]; 4];
    for j in 0..4 {
        let mut e = [0.0_f64; 4];
        if let Some(ej) = e.get_mut(j) {
            *ej = 1.0;
        }
        let col = solve4(a, &e)?;
        for (row, &c) in y.iter_mut().zip(&col) {
            if let Some(x) = row.get_mut(j) {
                *x = c;
            }
        }
    }
    Ok(y)
}

/// The proved enclosure of a span (`sm_GnEnclose`'s outputs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Enclosure {
    /// The feet's series over the span (`rF`).
    pub f: [VSer<Ivl>; 2],
    /// The feet's parameters' series (`pX`).
    pub x: [Ser<Ivl>; 4],
    /// Per face, `[|S_u|, |S_v|]` over the feet's box (the fork's `pDS` is their sum):
    /// `|S_u| |du| + |S_v| |dv|` bounds `|dS|` for a uv step `(du, dv)` in the box (#2690).
    pub ds: [[f64; 2]; 2],
    /// The same, least (the fork's `pDSLo`, their sum): no smaller anywhere in the box.
    pub ds_lo: [[f64; 2]; 2],
    /// The point V's series over the span (`pV`, 7102bc5).
    pub v: VSer<Ivl>,
}

fn ivl_at(x: &[Ivl; 4], i: usize) -> Ivl {
    x.get(i).copied().unwrap_or(IZ)
}

/// The system over the partials `p` (`sSys` with `apP` at `asP`).
fn system<'a>(
    p: &'a [Partials<Ivl>; 2],
    leader: &VSer<Ivl>,
    tangent: &VSer<Ivl>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&'a Ser<Ivl>>,
) -> GnSystem<'a, Ivl> {
    let [p0, p1] = p;
    GnSystem {
        p: [p0, p1],
        leader: *leader,
        tangent: *tangent,
        r,
        side,
        hold: false,
        law,
    }
}

/// A face's partials over `[ua, ub] x [va, vb]`, enclosed (`sm_FacePartialsOver`,
/// c5b6bb7): a NURBS face's piece `(ku, kv)`, continued; a blend by its definition's on
/// the piece of its net along s that `(ku, kv)` is in
/// ([`super::definition_box::definition_partials_over_box`]).
///
/// # Errors
///
/// Those of [`nurbs_partials_over_piece`] and
/// [`super::definition_box::definition_partials_over_box`].
#[allow(clippy::too_many_arguments)]
pub fn face_partials_over(
    k: &Kernel,
    cn: &Nurbs,
    def: Option<&mut GnDefFace<'_>>,
    ku: usize,
    kv: usize,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
) -> Result<Partials<Ivl>> {
    if let Some(d) = def {
        return definition_partials_over_box(k, d, cn, ku, kv, ua, ub, va, vb, lk);
    }
    nurbs_partials_over_piece(cn, ku, kv, ua, ub, va, vb, lk)
}

/// The parts of `[lo, hi]` between the knots strictly inside it, each with the span whose
/// closed interval holds it: a knot crossed where the direction is continuous to less than
/// `continuity` (degree minus multiplicity) is refused, and so is an interval past the
/// knots' range (the face or curve is not defined there). A point `lo = hi` on an inner
/// knot lies in both spans' closed intervals: it is returned twice, once with each, so a
/// hull over the parts holds both one-sided values there (#2570).
pub(crate) fn knot_cuts(
    knots: &[f64],
    p: usize,
    n: usize,
    lo: f64,
    hi: f64,
    continuity: usize,
) -> Result<Vec<(f64, f64, usize)>> {
    let refused = |reason| SolidError::Unsupported {
        operation: OP,
        reason,
    };
    let (first, last) = (knot(knots, p as isize), knot(knots, n as isize));
    if !(lo >= first && hi <= last && lo <= hi) {
        return Err(refused("a box past the face's knots"));
    }
    let after = span(knots, p, n, lo);
    if lo == hi && after > p && knot(knots, after as isize) == lo {
        // the span that ends at the knot: the last one before it that is not empty
        let mut before = after - 1;
        while before > p && knot(knots, before as isize) >= knot(knots, before as isize + 1) {
            before -= 1;
        }
        return Ok(vec![(lo, hi, before), (lo, hi, after)]);
    }
    let mut at = vec![lo];
    let mut prev = f64::NAN;
    for &x in knots {
        if x > lo && x < hi && x != prev {
            if below_continuity(knots, p, x, continuity) {
                return Err(refused(
                    "a box crosses a knot line where the face or curve is not continuous enough",
                ));
            }
            at.push(x);
        }
        prev = x;
    }
    at.push(hi);
    Ok(at
        .iter()
        .zip(at.iter().skip(1))
        .map(|(&a, &b)| (a, b, span(knots, p, n, 0.5 * (a + b))))
        .collect())
}

/// Whether `[lo, hi]` lies on more than one piece of `knots` (it crosses an inner knot, or
/// is a point on one), or past their range.
pub(crate) fn crosses_a_knot(knots: &[f64], p: usize, n: usize, lo: f64, hi: f64) -> bool {
    knot_cuts(knots, p, n, lo, hi, 0).map_or(true, |c| c.len() > 1)
}

/// The continuity a face must have at a knot line a feet's box crosses
/// ([`nurbs_partials_over_box`]): Krawczyk's mean-value form needs the feet's system (the
/// faces' points and first partials) continuous, with its Jacobian (their second partials)
/// enclosed wherever it exists; at C^2 the system is C^1 across the line. Continuity is
/// counted (degree minus multiplicity), never measured: a net equal to rounding across a
/// line is not continuous there. A pointwise use (order 1 or less) takes a box across a
/// lower line cell by cell instead, each cell's continued piece smooth ([`gn_enclose_over`]);
/// a use of order 2 or more (a definition's partials in a mean-value form of its own, which
/// needs the definition `C^1`, so the faces `C^2`) needs this.
pub const FEET_BOX_CONTINUITY: usize = 2;

/// A NURBS face's partials `P[a][b]`, `a + b <= lk`, over `[ua, ub] x [va, vb]` wherever
/// it lies on the face's knots, enclosed (not upstream): the box cut at the knot lines
/// strictly inside it, each part enclosed on its own polynomial piece
/// ([`nurbs_partials_over_piece`] over the part, its closed span), and the enclosures'
/// hull. Every point of the box lies in some closed part, where that piece is the face, so
/// the hull holds, at every point, each partial the face has there (on a knot line, from
/// either side): an enclosure point by point, whatever the continuity. A use that also
/// integrates over the box (a mean-value form) needs the face continuous to some order
/// across the lines it crosses: a crossed line where the face's continuity (degree minus
/// the knot's multiplicity) is below `continuity` is refused, as is a box past the knots'
/// range. No station invariant is assumed.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a crossed knot line of too low a continuity or a box
/// past the knots; those of [`nurbs_partials_over_piece`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_box(
    cn: &Nurbs,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    continuity: usize,
) -> Result<Partials<Ivl>> {
    let us = knot_cuts(&cn.u, cn.p, cn.nu, ua, ub, continuity)?;
    let vs = knot_cuts(&cn.v, cn.q, cn.nv, va, vb, continuity)?;
    let mut out: Option<Partials<Ivl>> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = nurbs_partials_over_piece(cn, ku, kv, a0, a1, b0, b1, lk)?;
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.ok_or_else(|| SolidError::blend_failed(OP, "no piece encloses the box"))
}

/// The refusal of a feet's box past a face's net in a direction its kind does not continue
/// ([`nurbs_partials_over_box_ending`]), before [`gn_enclose_over`] names the support and the
/// piece in [`crate::error::Reason::FootPastDomainEnd`].
const FOOT_PAST_END: &str =
    "a feet's box past the face's domain end, in a direction its kind does not continue";

/// Which of a support's NURB directions continue past its net's ends for a feet's box
/// (not the fork's, tangency-labs/solidcraft#14284), decided by the support's kind alone: a
/// direction in which the kind is a line, so the net's degree-1 end span continued is the
/// support there. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EndContinuation {
    /// The NURB's u continues.
    pub u: bool,
    /// The NURB's v continues.
    pub v: bool,
    /// A cone's axis (not a cylinder's), which a continuation may not meet: its lines meet
    /// on it at the apex and run on onto the other nappe, not the face. `None` for a kind
    /// with no apex.
    pub apex: Option<ConeAxis>,
}

/// A cone's axis from its definition: the point `origin` on it and its unit direction
/// `axis`. The cone meets its axis only at its apex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConeAxis {
    /// A point of the axis (the cone's origin).
    pub origin: [f64; 3],
    /// The axis' unit direction.
    pub axis: [f64; 3],
}

impl EndContinuation {
    /// The directions `surface`'s kind is a line in: a plane both; a cone (a cylinder among
    /// them) its generator direction (the NURB's v, or u where its parameters are swapped),
    /// with its apex where it has one; an extrusion its extrusion direction (likewise); none
    /// for every other kind.
    #[must_use]
    pub fn of(surface: &Surface) -> Self {
        use crate::surface::surface::p2::SurfaceKind;
        let along = |swapped: bool| Self {
            u: swapped,
            v: !swapped,
            apex: None,
        };
        match surface.kind() {
            SurfaceKind::Plane(_) => Self {
                u: true,
                v: true,
                apex: None,
            },
            SurfaceKind::Cone(c) => {
                let canonical = c.canonical();
                let mut e = along(canonical.swap_uv);
                if !c.is_cylinder() {
                    let (o, z) = (canonical.origin.origin(), canonical.origin.z_axis());
                    e.apex = Some(ConeAxis {
                        origin: [o.x, o.y, o.z],
                        axis: [z.x, z.y, z.z],
                    });
                }
                e
            }
            SurfaceKind::SurfOfExtrusion(e) => along(e.swap_uv()),
            _ => Self::default(),
        }
    }
}

/// [`nurbs_partials_over_box`] for a feet's box that may run past the net's domain: in a
/// direction `ends` continues and the net is of degree 1 in, the part past an end takes the
/// end span's polynomial (the face's edge jet continued, the support there by its kind); the
/// rest of the box is cut at the knot lines as [`nurbs_partials_over_box`] cuts it, and the
/// enclosures' hull is taken. A box within the domain is exactly
/// [`nurbs_partials_over_box`]'s.
///
/// # Errors
///
/// [`SolidError::Unsupported`] ([`FOOT_PAST_END`]) for a box past an end `ends` does not
/// continue or of a direction of degree above 1; those of [`nurbs_partials_over_box`].
#[allow(clippy::too_many_arguments)]
pub fn nurbs_partials_over_box_ending(
    cn: &Nurbs,
    ends: EndContinuation,
    ua: f64,
    ub: f64,
    va: f64,
    vb: f64,
    lk: usize,
    continuity: usize,
) -> Result<Partials<Ivl>> {
    let (u0, u1) = (knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize));
    let (v0, v1) = (knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize));
    let past = |lo: f64, hi: f64, d0: f64, d1: f64| lo < d0 || hi > d1;
    let (pu, pv) = (past(ua, ub, u0, u1), past(va, vb, v0, v1));
    if !pu && !pv {
        return nurbs_partials_over_box(cn, ua, ub, va, vb, lk, continuity);
    }
    let refused = || SolidError::Unsupported {
        operation: OP,
        reason: FOOT_PAST_END,
    };
    let ordered = ua <= ub && va <= vb;
    if !ordered || (pu && !(ends.u && cn.p == 1)) || (pv && !(ends.v && cn.q == 1)) {
        return Err(refused());
    }
    // each direction's parts: before its first knot (the first span continued), within (cut
    // at the knot lines), past its last (the last span continued)
    let parts = |knots: &[f64], p: usize, n: usize, lo: f64, hi: f64, d0: f64, d1: f64| {
        let mut out = Vec::new();
        if lo < d0 {
            out.push((lo, hi.min(d0), span(knots, p, n, d0)));
        }
        let (a, b) = (lo.max(d0), hi.min(d1));
        if a <= b {
            out.extend(knot_cuts(knots, p, n, a, b, continuity)?);
        }
        if hi > d1 {
            out.push((lo.max(d1), hi, span(knots, p, n, d1)));
        }
        Ok::<_, SolidError>(out)
    };
    let us = parts(&cn.u, cn.p, cn.nu, ua, ub, u0, u1)?;
    let vs = parts(&cn.v, cn.q, cn.nv, va, vb, v0, v1)?;
    let mut out: Option<Partials<Ivl>> = None;
    for &(a0, a1, ku) in &us {
        for &(b0, b1, kv) in &vs {
            let p = nurbs_partials_over_piece(cn, ku, kv, a0, a1, b0, b1, lk)?;
            // a part past an end of a cone may not reach its apex, the one point of the cone
            // on its axis: the part taken back to the end (its range in each direction
            // hulled with the domain's end it lies past) must be proved off the axis. That
            // set is connected and holds points of the end, which are the face's, so off the
            // axis it lies wholly on the face's nappe, the part with it. A part wholly past
            // the apex (on the other nappe, off the axis itself) is refused so: the way back
            // to the end crosses the axis
            let past = a0 < u0 || a1 > u1 || b0 < v0 || b1 > v1;
            if let (true, Some(cone)) = (past, ends.apex) {
                let domain = [u0, u1, v0, v1];
                if !back_to_the_end_off_the_axis(cn, ku, kv, [a0, a1, b0, b1], domain, cone)? {
                    return Err(refused());
                }
            }
            out = Some(match out {
                None => p,
                Some(h) => partials_hull(h, &p, lk),
            });
        }
    }
    out.ok_or_else(|| SolidError::blend_failed(OP, "no piece encloses the box"))
}

/// Whether the piece `(ku, kv)` of a cone's net over the part `[a0, a1] x [b0, b1]` past its
/// domain `[u0, u1] x [v0, v1]`, taken back to the end it lies past (each range hulled with
/// that end), is proved off the cone's axis: a coordinate of its radial vector
/// `P - O - ((P - O).Z) Z` bounded away from 0 over that set. The set is connected and holds
/// points of the end, which are the face's, so off the axis the part lies on the face's
/// nappe; a part wholly past the apex is not, since the way back crosses the axis
/// ([`nurbs_partials_over_box_ending`], #14986's review).
///
/// # Errors
///
/// Those of [`nurbs_partials_over_piece`].
fn back_to_the_end_off_the_axis(
    cn: &Nurbs,
    ku: usize,
    kv: usize,
    part: [f64; 4],
    domain: [f64; 4],
    cone: ConeAxis,
) -> Result<bool> {
    let [a0, a1, b0, b1] = part;
    let [u0, u1, v0, v1] = domain;
    let back = |lo: f64, hi: f64, d0: f64, d1: f64| (lo.min(d1), hi.max(d0));
    let (c0, c1) = back(a0, a1, u0, u1);
    let (e0, e1) = back(b0, b1, v0, v1);
    let to_end = nurbs_partials_over_piece(cn, ku, kv, c0, c1, e0, e1, 0)?;
    let x = to_end.at(0, 0);
    let [ox, oy, oz] = cone.origin.map(Ivl::from);
    let [zx, zy, zz] = cone.axis.map(Ivl::from);
    let (dx, dy, dz) = (x.x - ox, x.y - oy, x.z - oz);
    let h = dx * zx + dy * zy + dz * zz;
    let radial = [dx - h * zx, dy - h * zy, dz - h * zz];
    Ok(radial.iter().any(|c| c.mig() > 0.0))
}

/// Whether `e` is the refusal of a foot past its face's domain end, which `gn_try!` passes on
/// (the one failed evaluation that is not "nothing proved", #14284).
pub(crate) fn is_foot_past_domain_end(e: &SolidError) -> bool {
    matches!(e, SolidError::Failed(f) if matches!(f.reason, crate::error::Reason::FootPastDomainEnd { .. }))
}

/// `e`, the refusal [`FOOT_PAST_END`] of support `support`'s box `uv` over the piece `s`,
/// named ([`crate::error::Reason::FootPastDomainEnd`]); any other error unchanged.
fn name_foot_past_end(
    e: SolidError,
    support: usize,
    s: Ivl,
    uv: [[f64; 2]; 2],
    cn: &Nurbs,
) -> SolidError {
    match e {
        SolidError::Unsupported { reason, .. } if reason == FOOT_PAST_END => {
            let domain = [
                [knot(&cn.u, cn.p as isize), knot(&cn.u, cn.nu as isize)],
                [knot(&cn.v, cn.q as isize), knot(&cn.v, cn.nv as isize)],
            ];
            SolidError::Failed(Box::new(crate::error::Failure::new(
                OP,
                crate::error::Reason::FootPastDomainEnd {
                    support,
                    s: [s.lo, s.hi],
                    uv,
                    domain,
                },
            )))
        }
        e => e,
    }
}

/// The hull of two enclosures of partials, `a + b <= lk`.
pub(crate) fn partials_hull(mut h: Partials<Ivl>, p: &Partials<Ivl>, lk: usize) -> Partials<Ivl> {
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            h.set(a, b, v_hull(h.at(a, b), p.at(a, b)));
        }
    }
    h
}

/// A span the feet are enclosed over (`sm_GnSpanAt`, c5b6bb7): S, the feet's guesses at
/// its ends (their hull, widened, is the first box), each face's polynomial piece over it
/// and the leader's (`None`: the one the middle of S lies in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpanAt {
    /// `sS`.
    pub s: Ivl,
    /// `axA`: the feet's guesses at S's start.
    pub xa: [f64; 4],
    /// `axB`: and at its end.
    pub xb: [f64; 4],
    /// `aku`: each face's piece in u.
    pub ku: [usize; 2],
    /// `akv`: and in v.
    pub kv: [usize; 2],
    /// `lLeader`.
    pub leader: Option<usize>,
    /// Not upstream: a NURBS face's partials over a box are enclosed piece by piece
    /// ([`nurbs_partials_over_box`]), so the feet may cross knot lines (`ku`, `kv` unused for
    /// them), and the leader's series over S on the piece `leader` names (the caller cut S
    /// at the leader's knots: a point S on a knot, the side it names) or, `None`, piece by
    /// piece ([`leader_series_piecewise`]); `false`: on the pieces `ku`, `kv` and `leader`,
    /// continued (upstream's, which relies on the station invariant).
    pub piecewise: bool,
}

/// `sm_GnEnclose`: the feet's series over `S = [a.s, b.s]` (a point when they are one
/// station), to order `order_s` in s and `n` in tau, enclosed; `None` where Krawczyk's test
/// fails on S (or an evaluation fails: nothing is proved). `defs`: the faces that are
/// blends by their definitions, enclosed over the boxes (c5b6bb7).
///
/// # Errors
///
/// `InvalidInput` when `order_s` or `n` exceeds the series' size.
#[allow(clippy::too_many_arguments)]
pub fn gn_enclose(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<Option<Enclosure>> {
    let at = span_at(faces_n, a, b);
    gn_enclose_over(
        k,
        faces_n,
        leader_n,
        &at,
        r,
        side,
        n,
        order_s,
        law,
        defs,
        [EndContinuation::default(); 2],
    )
}

/// The span `[a.s, b.s]` as [`gn_enclose`] encloses the feet over it ([`SpanAt`]): the
/// stations' feet as the guesses, each face's polynomial piece the one the guesses' middle
/// lies in, the leader's the one the span's middle lies in, continued.
pub(crate) fn span_at(faces_n: [&Nurbs; 2], a: &GnStation, b: &GnStation) -> SpanAt {
    let ax = |c: usize| a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
    let bx = |c: usize| b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
    let mut at = SpanAt {
        s: Ivl::new(a.s, b.s),
        xa: [ax(0), ax(1), ax(2), ax(3)],
        xb: [bx(0), bx(1), bx(2), bx(3)],
        ku: [0; 2],
        kv: [0; 2],
        leader: None,
        piecewise: false,
    };
    // each face's polynomial piece over the span: the one its middle lies in. A station
    // stands where a foot crosses a knot line, so every foot over the span lies in that
    // piece, and the piece's polynomial, continued over the rest of a box, is the face
    // wherever a foot can be: the root Krawczyk's test proves unique in a box is the foot
    // (a definition's piece of its net is one between two of its stations, its polynomial
    // pieces the ones the faces under it have there)
    for (f, face) in faces_n.iter().enumerate() {
        let su = span(&face.u, face.p, face.nu, 0.5 * (ax(2 * f) + bx(2 * f)));
        let sv = span(
            &face.v,
            face.q,
            face.nv,
            0.5 * (ax(2 * f + 1) + bx(2 * f + 1)),
        );
        if let (Some(u), Some(v)) = (at.ku.get_mut(f), at.kv.get_mut(f)) {
            *u = su;
            *v = sv;
        }
    }
    at
}

/// `sm_GnEncloseOver` (c5b6bb7): the feet's series over `at.s`, to order `order_s` in s and
/// `n` in tau, enclosed; `None` where Krawczyk's test fails on S (or an evaluation fails:
/// nothing is proved). The centre's residual is evaluated once: the iterations keep the
/// centre.
///
/// **Across a knot line below `C^2` (not the fork's, tangency-labs/solidcraft#14284).** The
/// piecewise enclosure ([`SpanAt::piecewise`], no definitions) for a use of order 1 or less
/// (`order_s` here; `gn_enclose_over_using`'s `use_order` where the caller asks for more
/// orders than its own use takes, as the definition box's centred form does) whose box (the
/// guesses' hull, or a box the one solve's iterations form, #15183) crosses a face's knot line below
/// [`FEET_BOX_CONTINUITY`] by count (a rational circle's doubled quarter knot is `C^0` by
/// count, `C^1` only to rounding) takes the feet cell by cell instead of refusing them: each
/// polynomial piece of such a face the box touches, continued over the whole box, is the
/// face exactly on its own closed span, and those spans cover the box, so every foot is a
/// root of some cell's continued system, and each continued system is smooth, so Krawczyk's
/// operator is exact on it. A first pass solves each cell from the guesses; `H` is the hull
/// of the first box (the guesses' hull, where every cell starts) and the boxes it proves
/// (none proved: `None`). A second pass decides every cell of `H` over `H` itself: `K(H)`
/// inside `H` (a unique root of that cell's system in `H`, its box `K(H)` tightened) or
/// `K(H)` off `H` in some coordinate (no root in `H`). Every root of a cell's system in `H`
/// is in its `K(H)`, so every foot in `H` is in the box of its cell. The result is the hull
/// of the enclosures of the cells whose root's box meets the cell's own closed span (a root
/// outside it is not on the face): the feet that exist in `H`. `None` there (no cell proved
/// in the first pass, or every cell of `H` excluded or off its own span) is, as everywhere
/// here, "not proved", never "no foot": no caller infers an absence from it (the one caller
/// on this path, the definition box's `partials_on`, refuses the box).
/// Roots of two cells kept whose boxes meet are hulled: one foot seen from both sides of the
/// line, or two feet within one box, each enclosed; uniqueness is not claimed then. It is
/// refused (`Unsupported`, by name) where a cell of `H` is not decided, where two kept
/// roots' boxes are apart (two feet the hull would join across a gap), or where `H` touches more than `CELL_PIECES_PER_DIRECTION` (2) pieces of
/// a face in a direction (it spans a whole piece: the span is to be cut); a box past a face's
/// end its kind does not continue is refused `FootPastDomainEnd` as the one solve refuses it,
/// and past an end it continues each cell there is the end span continued (#14284, a cone's
/// part kept off its apex). So a pass costs
/// at most 16 solves, one per cell, whatever the nets' knot counts, with no recursion. These
/// refusals say why a box is not proved at its width, never that the feet are not there:
/// the definition box passes them on, the tangent-edge margin halves its piece on any
/// failure, and a certificate's levels cut the span on it (`SM_GN_TRY`). A
/// box that crosses no such
/// line, a use of order 2 or more (a mean-value form of a definition over its box, which
/// needs the face `C^2` across it), and an enclosure with definitions take the one solve
/// over the hull of the cells' partials as before. Where the guesses' hull misses such a line
/// and the one solve's contraction forms a box across it (#15183: a root on the line, just
/// outside the guesses), that box is one the one solve can never prove (`knot_cuts` refuses
/// its partials), so the loop stops there before evaluating anything over it, and the cell
/// path starts from the hull of the first box and that one, each face's cells listed from
/// the hull (`from_the_crossing`).
///
/// # Errors
///
/// `InvalidInput` when `order_s` or `n` exceeds the series' size; `Unsupported` where the
/// cells across a knot line are refused (above).
#[allow(clippy::too_many_arguments)]
pub fn gn_enclose_over(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    at: &SpanAt,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
    ends: [EndContinuation; 2],
) -> Result<Option<Enclosure>> {
    gn_enclose_over_using(
        k, faces_n, leader_n, at, r, side, n, order_s, law, defs, ends, order_s,
    )
}

/// [`gn_enclose_over`] for a caller whose use of the result takes partials of order
/// `use_order` (at most `order_s`): the definition box's centred form asks for one order
/// more than its caller uses, to bound the rest, and where the feet's box crosses a knot it
/// returns those partials point by point. A use of order 1 or less takes a box across a
/// knot line below `C^2` cell by cell (see [`gn_enclose_over`]).
///
/// # Errors
///
/// As [`gn_enclose_over`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn gn_enclose_over_using(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    at: &SpanAt,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
    ends: [EndContinuation; 2],
    use_order: usize,
) -> Result<Option<Enclosure>> {
    crate::profile::profile_scope!(k, "blend::certificate::gn_enclose_over");
    let divert = at.piecewise && use_order <= 1 && defs.is_none();
    let enclose = |cells: [FaceCell; 2], over: Option<[Ivl; 4]>| {
        enclose_over_cells(
            k, faces_n, leader_n, at, r, side, n, order_s, law, None, ends, cells, over, false,
        )
    };
    let (_, x0) = guess_box(at);
    if divert {
        let low = low_lines_in(faces_n, &x0);
        if low.iter().any(|&l| l) {
            return enclose_by_cells(faces_n, ends, at.s, &x0, low, enclose);
        }
    }
    let cells = [FaceCell::Hull; 2];
    match enclose_over_cells(
        k, faces_n, leader_n, at, r, side, n, order_s, law, defs, ends, cells, None, divert,
    )? {
        Inner::Proved(e) => {
            let (enc, _) = *e;
            Ok(Some(enc))
        }
        // the one solve formed a box across such a line (#15183): from the hull of its first
        // box and that one, cell by cell, each face's cells listed from the hull
        Inner::Crosses(b) => {
            let (x1, low) = from_the_crossing(faces_n, &x0, &b);
            enclose_by_cells(faces_n, ends, at.s, &x1, low, enclose)
        }
        Inner::Excluded | Inner::Straddles(_) | Inner::Unproved => Ok(None),
    }
}

/// Whether each face's part of the box `x` crosses one of its knot lines below
/// [`FEET_BOX_CONTINUITY`].
fn low_lines_in(faces_n: [&Nurbs; 2], x: &[Ivl; 4]) -> [bool; 2] {
    let [f0, f1] = faces_n;
    [crosses_low_line_at(f0, x, 0), crosses_low_line_at(f1, x, 1)]
}

/// The cell path's first box when the one solve forms `b` across a knot line below `C^2`
/// (#15183): the hull of its first box `x0` and `b` (the loop's boxes share their centre, so
/// in each coordinate the wider), and the faces whose part of that hull crosses such a line.
/// The faces are read from the hull, not from `b`: a coordinate where `b` is the narrower can
/// cross a line in the hull alone, and a face listed whole there would be refused on every
/// cell.
fn from_the_crossing(faces_n: [&Nurbs; 2], x0: &[Ivl; 4], b: &[Ivl; 4]) -> ([Ivl; 4], [bool; 2]) {
    let x1 = core::array::from_fn(|i| ivl_at(x0, i).hull(ivl_at(b, i)));
    let low = low_lines_in(faces_n, &x1);
    (x1, low)
}

/// How [`enclose_over_cells`] takes a face's partials over a box in the piecewise
/// enclosure: the hull of its cut parts ([`nurbs_partials_over_box_ending`]), or one
/// polynomial piece `(ku, kv)` continued over the whole box (refused past the face's knots).
#[derive(Clone, Copy, Debug, PartialEq)]
enum FaceCell {
    Hull,
    Piece(usize, usize),
}

/// What [`enclose_over_cells`] decides: the feet enclosed with their box; over a given box
/// only, no root in it (`Excluded`) or `K` meeting the box without lying inside it
/// (`Straddles`, with that `K`, the step's `q < 1`); or nothing proved.
enum Inner {
    Proved(Box<(Enclosure, [Ivl; 4])>),
    Excluded,
    Straddles([Ivl; 4]),
    /// The one solve's next box, across a knot line below `C^2` (only where asked to divert).
    Crosses([Ivl; 4]),
    Unproved,
}

/// The most rounds of the second pass of [`enclose_by_cells`], each taking `H` over the
/// `K`s that straddled it, widened: the fork's count of Krawczyk steps from a box.
const SECOND_PASS_ROUNDS: usize = 8;

/// `x` widened as the Krawczyk loop widens a box it does not prove (its half widths
/// `w` to `1.5 w + 1e-12 (1 + |m|)` about the middles `m`), so the next step's `K` can
/// fall strictly inside a box that holds it.
fn widened(x: &[Ivl; 4]) -> [Ivl; 4] {
    x.map(|xi| {
        let (m, w) = (xi.mid(), 0.5 * (xi.hi - xi.lo));
        let rr = 1.5 * w + 1.0e-12 * (1.0 + m.abs());
        Ivl::new(m - rr, m + rr)
    })
}

/// The guesses' middle and the first feet's box about it ([`gn_enclose_over`]).
fn guess_box(at: &SpanAt) -> ([f64; 4], [Ivl; 4]) {
    let mut xm = [0.0_f64; 4];
    let mut bx_ = [IZ; 4];
    for (c, (m, xc)) in xm.iter_mut().zip(bx_.iter_mut()).enumerate() {
        let lo = at.xa.get(c).copied().unwrap_or(f64::NAN);
        let hi = at.xb.get(c).copied().unwrap_or(f64::NAN);
        *m = 0.5 * (lo + hi);
        let w = 0.5 * (hi - lo).abs() + 1.0e-9 * (1.0 + m.abs());
        *xc = Ivl::new(*m - 2.0 * w, *m + 2.0 * w);
    }
    (xm, bx_)
}

/// Whether the knot `x` of `knots` (degree `p`) is below `continuity` by count: degree minus
/// its multiplicity. [`knot_cuts`] refuses a crossed line by it, and the cell path's
/// trigger reads it, so the two cannot drift apart.
fn below_continuity(knots: &[f64], p: usize, x: f64, continuity: usize) -> bool {
    let m = knots.iter().filter(|&&y| y == x).count();
    p.checked_sub(m).is_none_or(|c| c < continuity)
}

/// Whether `[lo, hi]` crosses a knot line of `knots` (strictly inside it) below
/// `continuity` ([`below_continuity`]), the lines [`knot_cuts`] refuses, visited as it
/// visits them.
fn crosses_low_line(knots: &[f64], p: usize, lo: f64, hi: f64, continuity: usize) -> bool {
    let mut prev = f64::NAN;
    let mut low = false;
    for &x in knots {
        if x > lo && x < hi && x != prev {
            low |= below_continuity(knots, p, x, continuity);
        }
        prev = x;
    }
    low
}

/// Whether face `f`'s part of the feet's box `x` crosses one of its knot lines below
/// [`FEET_BOX_CONTINUITY`].
fn crosses_low_line_at(cn: &Nurbs, x: &[Ivl; 4], f: usize) -> bool {
    let (u, v) = (ivl_at(x, 2 * f), ivl_at(x, 2 * f + 1));
    crosses_low_line(&cn.u, cn.p, u.lo, u.hi, FEET_BOX_CONTINUITY)
        || crosses_low_line(&cn.v, cn.q, v.lo, v.hi, FEET_BOX_CONTINUITY)
}

/// The most pieces of a face, per direction, a feet's box is taken over cell by cell: the
/// two on either side of one knot line. A box across a knot line is the feet's box of one
/// span's piece, and a box that reaches past the next line spans a whole piece of the face
/// within it, where the span is to be cut (the certificate's levels), not enclosed whole;
/// so it is refused by name. A face's cells are then at most 2 x 2, a pass's solves at most
/// 16, whatever the net's knot counts.
const CELL_PIECES_PER_DIRECTION: usize = 2;

/// Why [`face_cells`] gives no cells.
#[derive(Clone, Copy, Debug, PartialEq)]
enum NoCells {
    /// The box runs past the face's knots in a direction its kind does not continue
    /// ([`EndContinuation`]).
    PastKnots,
    /// The box touches more than [`CELL_PIECES_PER_DIRECTION`] pieces in a direction.
    TooMany,
}

/// The cells of face `f` over the feet's box `x`: `Hull` where `piece` is false, else every
/// polynomial piece the box touches. Past the net's end in a direction `ends` continues
/// (degree 1 there, #14284) the box takes the end span, continued: its part within the
/// domain is cut, and the end span is the piece past the end.
fn face_cells(
    cn: &Nurbs,
    x: &[Ivl; 4],
    f: usize,
    piece: bool,
    ends: EndContinuation,
) -> core::result::Result<Vec<FaceCell>, NoCells> {
    if !piece {
        return Ok(vec![FaceCell::Hull]);
    }
    let (u, v) = (ivl_at(x, 2 * f), ivl_at(x, 2 * f + 1));
    // a direction's range within the domain, where the box runs past an end the kind
    // continues; past an end it does not continue, no cells
    let within = |knots: &[f64], p: usize, n: usize, lo: f64, hi: f64, on: bool| {
        let (d0, d1) = (knot(knots, p as isize), knot(knots, n as isize));
        if lo >= d0 && hi <= d1 {
            return Ok((lo, hi));
        }
        if !(on && p == 1 && lo <= hi) {
            return Err(NoCells::PastKnots);
        }
        // wholly past an end: the end itself, whose span is the one continued
        // (cmin/cmax, not f64::clamp: a NaN knot reads as NaN, never a panic)
        Ok((cmin(cmax(lo, d0), d1), cmin(cmax(hi, d0), d1)))
    };
    let (ul, uh) = within(&cn.u, cn.p, cn.nu, u.lo, u.hi, ends.u)?;
    let (vl, vh) = within(&cn.v, cn.q, cn.nv, v.lo, v.hi, ends.v)?;
    let us = knot_cuts(&cn.u, cn.p, cn.nu, ul, uh, 0).map_err(|_| NoCells::PastKnots)?;
    let vs = knot_cuts(&cn.v, cn.q, cn.nv, vl, vh, 0).map_err(|_| NoCells::PastKnots)?;
    if us.len() > CELL_PIECES_PER_DIRECTION || vs.len() > CELL_PIECES_PER_DIRECTION {
        return Err(NoCells::TooMany);
    }
    let mut out = Vec::new();
    for &(_, _, ku) in &us {
        for &(_, _, kv) in &vs {
            let c = FaceCell::Piece(ku, kv);
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

/// Whether face `f`'s part of the box `x` meets the closed span of its cell.
fn meets_own_cell(cn: &Nurbs, cell: FaceCell, x: &[Ivl; 4], f: usize) -> bool {
    let FaceCell::Piece(ku, kv) = cell else {
        return true;
    };
    let (u, v) = (ivl_at(x, 2 * f), ivl_at(x, 2 * f + 1));
    let (u0, u1) = (knot(&cn.u, ku as isize), knot(&cn.u, ku as isize + 1));
    let (v0, v1) = (knot(&cn.v, kv as isize), knot(&cn.v, kv as isize + 1));
    u.lo <= u1 && u.hi >= u0 && v.lo <= v1 && v.hi >= v0
}

/// Krawczyk's `K` against its box `x`, coordinate by coordinate.
#[derive(Clone, Copy, Debug, PartialEq)]
enum KVerdict {
    /// Every `K_i` strictly inside `x_i` (with `q < 1`, a unique root in `x`).
    Inside,
    /// Some `K_i` apart from `x_i`: no root in `x`.
    Off,
    /// Neither: nothing decided.
    Straddles,
}

/// [`KVerdict`] of `k` against `x`. Inside is strict: a `K_i` touching `x_i`'s end is not
/// inside, and a NaN end is never inside nor off.
fn classify_k(k: &[Ivl; 4], x: &[Ivl; 4]) -> KVerdict {
    if k.iter()
        .zip(x)
        .all(|(ki, xi)| ki.lo > xi.lo && ki.hi < xi.hi)
    {
        KVerdict::Inside
    } else if k
        .iter()
        .zip(x)
        .any(|(ki, xi)| ki.hi < xi.lo || ki.lo > xi.hi)
    {
        KVerdict::Off
    } else {
        KVerdict::Straddles
    }
}

/// Every interval of `a` meets `b`'s.
fn boxes_meet(a: &[Ivl; 4], b: &[Ivl; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.lo <= y.hi && y.lo <= x.hi)
}

fn ser_hull(a: &mut Ser<Ivl>, b: &Ser<Ivl>) {
    for (ra, rb) in a.c.iter_mut().zip(&b.c) {
        for (ca, cb) in ra.iter_mut().zip(rb) {
            *ca = ca.hull(*cb);
        }
    }
}

fn vser_hull(a: &mut VSer<Ivl>, b: &VSer<Ivl>) {
    for (sa, sb) in a.a.iter_mut().zip(&b.a) {
        ser_hull(sa, sb);
    }
}

/// The hull of two enclosures: each series' coefficients hulled, the greatest `ds` and the
/// least `ds_lo`.
fn enclosure_hull(a: &mut Enclosure, b: &Enclosure) {
    for (fa, fb) in a.f.iter_mut().zip(&b.f) {
        vser_hull(fa, fb);
    }
    for (xa, xb) in a.x.iter_mut().zip(&b.x) {
        ser_hull(xa, xb);
    }
    for (da, db) in a.ds.iter_mut().zip(&b.ds) {
        for (x, &y) in da.iter_mut().zip(db) {
            *x = cmax(*x, y);
        }
    }
    for (da, db) in a.ds_lo.iter_mut().zip(&b.ds_lo) {
        for (x, &y) in da.iter_mut().zip(db) {
            *x = cmin(*x, y);
        }
    }
    vser_hull(&mut a.v, &b.v);
}

/// [`gn_enclose_over`] cell by cell across knot lines below [`FEET_BOX_CONTINUITY`] (see
/// there): `low[f]`, face `f`'s first box crosses one; `enclose(cells, over)` encloses the
/// feet with each face's partials taken by its cell, from the guesses or over `over`.
fn enclose_by_cells(
    faces_n: [&Nurbs; 2],
    ends: [EndContinuation; 2],
    s: Ivl,
    x0: &[Ivl; 4],
    low: [bool; 2],
    mut enclose: impl FnMut([FaceCell; 2], Option<[Ivl; 4]>) -> Result<Inner>,
) -> Result<Option<Enclosure>> {
    let refused = |reason| SolidError::Unsupported {
        operation: OP,
        reason,
    };
    let [n0, n1] = faces_n;
    let [e0, e1] = ends;
    let combos = |x: &[Ivl; 4],
                  piece: [bool; 2]|
     -> core::result::Result<Vec<[FaceCell; 2]>, (NoCells, usize)> {
        let c0 = face_cells(n0, x, 0, piece[0], e0).map_err(|e| (e, 0))?;
        let c1 = face_cells(n1, x, 1, piece[1], e1).map_err(|e| (e, 1))?;
        let mut out = Vec::new();
        for &a in &c0 {
            for &b in &c1 {
                out.push([a, b]);
            }
        }
        Ok(out)
    };
    let too_many =
        || refused("a feet's box across a knot line spans a whole piece of the face: cut the span");
    // a box past a face's end its kind does not continue: refused by name, as the one solve
    // refuses it (FootPastDomainEnd, which SM_GN_TRY passes on)
    let past_end = |x: &[Ivl; 4], f: usize| {
        let (u, v) = (ivl_at(x, 2 * f), ivl_at(x, 2 * f + 1));
        let cn = if f == 0 { n0 } else { n1 };
        let raw = refused(FOOT_PAST_END);
        name_foot_past_end(raw, f, s, [[u.lo, u.hi], [v.lo, v.hi]], cn)
    };
    // the first pass: each cell from the guesses. H: the hull of the first box (the one
    // every cell's iterations start from) and the boxes proved; nothing proved, no H
    let first = match combos(x0, low) {
        Ok(c) => c,
        Err((NoCells::PastKnots, f)) => return Err(past_end(x0, f)),
        Err((NoCells::TooMany, _)) => return Err(too_many()),
    };
    let mut h = *x0;
    let mut any = false;
    for cells in first {
        if let Inner::Proved(e) = enclose(cells, None)? {
            let b = e.1;
            h = core::array::from_fn(|i| ivl_at(&h, i).hull(ivl_at(&b, i)));
            any = true;
        }
    }
    if !any {
        return Ok(None);
    }
    // the second pass: every cell of H decided over H. A cell whose K meets H without lying
    // inside it (a root on H's edge: H is the hull of the boxes it came from) takes H over
    // that K widened, and every cell is decided again over the wider H, at most SECOND_PASS_ROUNDS
    // rounds; a cell still undecided then is refused
    let not_decided =
        || refused("a cell of a feet's box across a knot line is not decided by Krawczyk's test");
    let mut round = 1;
    let kept = loop {
        let piece = [
            low[0] || crosses_low_line_at(n0, &h, 0),
            low[1] || crosses_low_line_at(n1, &h, 1),
        ];
        let second = match combos(&h, piece) {
            Ok(c) => c,
            Err((NoCells::PastKnots, f)) => return Err(past_end(&h, f)),
            Err((NoCells::TooMany, _)) => return Err(too_many()),
        };
        let mut kept: Vec<(Enclosure, [Ivl; 4])> = Vec::new();
        let mut grow: Option<[Ivl; 4]> = None;
        for cells in second {
            match enclose(cells, Some(h))? {
                Inner::Proved(e) => {
                    let (enc, b) = *e;
                    let [c0, c1] = cells;
                    if meets_own_cell(n0, c0, &b, 0) && meets_own_cell(n1, c1, &b, 1) {
                        if kept.iter().any(|(_, kb)| !boxes_meet(kb, &b)) {
                            return Err(refused(
                                "two feet across a knot line: two cells' roots apart, each on its own cell",
                            ));
                        }
                        kept.push((enc, b));
                    }
                }
                Inner::Excluded => {}
                Inner::Straddles(k) => {
                    // H with that K widened (the room its next step needs), never H itself
                    // widened: H's other coordinates keep their width
                    let g = grow.unwrap_or(h);
                    let wk = widened(&k);
                    grow = Some(core::array::from_fn(|i| ivl_at(&g, i).hull(ivl_at(&wk, i))));
                }
                Inner::Crosses(_) | Inner::Unproved => return Err(not_decided()),
            }
        }
        match grow {
            None => break kept,
            Some(g) if round < SECOND_PASS_ROUNDS => {
                h = g;
                round += 1;
            }
            Some(_) => return Err(not_decided()),
        }
    };
    let mut kept = kept.into_iter();
    let Some((mut acc, _)) = kept.next() else {
        return Ok(None);
    };
    for (e, _) in kept {
        enclosure_hull(&mut acc, &e);
    }
    Ok(Some(acc))
}

/// [`gn_enclose_over`]'s one solve: each face's partials by its cell ([`FaceCell`]), the
/// feet's box from the guesses (Krawczyk's iterations, as the fork) or, with `over`, that
/// box decided by one Krawczyk step: its `K` inside it (proved), off it in some coordinate
/// (excluded), else not proved.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn enclose_over_cells(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    at: &SpanAt,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order_s: usize,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
    ends: [EndContinuation; 2],
    cells: [FaceCell; 2],
    over: Option<[Ivl; 4]>,
    divert: bool,
) -> Result<Inner> {
    let s = at.s;
    let tr = Trunc::new(order_s, n)?;
    // a ball whose radius varies: its law over the span (0d43b4c)
    let law_r = match law {
        Some(l) => Some(gn_try!(law_series_over(l, s, order_s), Inner::Unproved)),
        None => None,
    };
    let law_r = law_r.as_ref();
    let (leader, tangent) = if at.piecewise && at.leader.is_none() {
        gn_try!(leader_series_piecewise(leader_n, s, tr), Inner::Unproved)
    } else {
        gn_try!(
            leader_series_over(leader_n, s, tr, at.leader),
            Inner::Unproved
        )
    };
    let [ku0, ku1] = at.ku;
    let [kv0, kv1] = at.kv;
    let mut defs = defs;
    let mut partials_over = |xl: [f64; 4], xh: [f64; 4], lk: usize| -> Result<[Partials<Ivl>; 2]> {
        let [n0, n1] = faces_n;
        let [l0, l1, l2, l3] = xl;
        let [h0, h1, h2, h3] = xh;
        let (d0, d1) = match defs.as_deref_mut() {
            Some(d) => {
                let [a0, a1] = &mut d.faces;
                (a0.as_mut(), a1.as_mut())
            }
            None => (None, None),
        };
        if at.piecewise && d0.is_none() && d1.is_none() {
            let [e0, e1] = ends;
            let [c0, c1] = cells;
            let one = |n: &Nurbs, e: EndContinuation, f: usize, uv: [f64; 4], cell: FaceCell| {
                let [ua, ub, va, vb] = uv;
                match cell {
                    FaceCell::Hull => nurbs_partials_over_box_ending(
                        n,
                        e,
                        ua,
                        ub,
                        va,
                        vb,
                        lk,
                        FEET_BOX_CONTINUITY,
                    )
                    .map_err(|err| name_foot_past_end(err, f, s, [[ua, ub], [va, vb]], n)),
                    // a cell's piece continued over the box, never past the face's knots
                    // a cell's piece continued over the box: past the net's end only in a
                    // direction the kind continues and only the end span there (#14284's
                    // continuation), a cone's part kept off its apex
                    FaceCell::Piece(ku, kv) => {
                        let (u0, u1) = (knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize));
                        let (v0, v1) = (knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize));
                        let ok = |k: usize,
                                  knots: &[f64],
                                  p: usize,
                                  nn: usize,
                                  lo: f64,
                                  hi: f64,
                                  d0: f64,
                                  d1: f64,
                                  on: bool| {
                            let low_end = lo >= d0 || (on && p == 1 && k == span(knots, p, nn, d0));
                            let high_end =
                                hi <= d1 || (on && p == 1 && k == span(knots, p, nn, d1));
                            low_end && high_end
                        };
                        let named = |err| name_foot_past_end(err, f, s, [[ua, ub], [va, vb]], n);
                        let past_end = || {
                            named(SolidError::Unsupported {
                                operation: OP,
                                reason: FOOT_PAST_END,
                            })
                        };
                        let continued = ua <= ub
                            && va <= vb
                            && ok(ku, &n.u, n.p, n.nu, ua, ub, u0, u1, e.u)
                            && ok(kv, &n.v, n.q, n.nv, va, vb, v0, v1, e.v);
                        if !continued {
                            return Err(past_end());
                        }
                        let past = ua < u0 || ub > u1 || va < v0 || vb > v1;
                        if let (true, Some(cone)) = (past, e.apex) {
                            let domain = [u0, u1, v0, v1];
                            let part = [ua, ub, va, vb];
                            if !back_to_the_end_off_the_axis(n, ku, kv, part, domain, cone)? {
                                return Err(past_end());
                            }
                        }
                        nurbs_partials_over_piece(n, ku, kv, ua, ub, va, vb, lk)
                    }
                }
            };
            return Ok([
                one(n0, e0, 0, [l0, h0, l1, h1], c0)?,
                one(n1, e1, 1, [l2, h2, l3, h3], c1)?,
            ]);
        }
        Ok([
            face_partials_over(k, n0, d0, ku0, kv0, l0, h0, l1, h1, lk)?,
            face_partials_over(k, n1, d1, ku1, kv1, l2, h2, l3, h3, lk)?,
        ])
    };

    // the feet's box: Krawczyk's operator from the guesses' hull, or over the box given
    let (mut xm, mut bx_) = guess_box(at);
    if let Some(h) = over {
        bx_ = h;
        xm = h.map(Ivl::mid);
    }
    let mut y = [[0.0_f64; 4]; 4];
    let mut cm = [[IZ; 4]; 4];
    let mut q = f64::INFINITY;
    let mut proved = false;
    let point = Trunc::new(0, 0)?;
    // G at the box's centre (which the iterations keep) for every s in the span
    let sg = {
        let pc = gn_try!(partials_over(xm, xm, 2), Inner::Unproved);
        let sxc = xm.map(|x| Ser::constant(Ivl::from(x)));
        let (sg, _) = gn_try!(
            gn_evaluate(&system(&pc, &leader, &tangent, r, side, law_r), &sxc, point),
            Inner::Unproved
        );
        sg
    };
    let mut it = 0;
    while it < 8 && !proved {
        it += 1;
        // the Jacobian over the box
        let pb = gn_try!(
            partials_over(bx_.map(|x| x.lo), bx_.map(|x| x.hi), 2),
            Inner::Unproved
        );
        let sxb = bx_.map(Ser::constant);
        let jac = gn_try!(
            gn_jacobian(&system(&pb, &leader, &tangent, r, side, law_r), &sxb),
            Inner::Unproved
        );
        let jm = jac.map(|row| row.map(Ivl::mid));
        y = gn_try!(inverse4(&jm), Inner::Unproved);
        (cm, q) = contraction(&y, &jac);
        let kk = k_operator(&xm, &y, &sg.each_ref().map(|g| g.at(0, 0)), &cm, &bx_);
        let verdict = classify_k(&kk, &bx_);
        let inside = q < 1.0 && verdict == KVerdict::Inside;
        if inside {
            for (xi, ki) in bx_.iter_mut().zip(&kk) {
                *xi = Ivl::new(cmax(xi.lo, ki.lo), cmin(xi.hi, ki.hi));
            }
            proved = true;
        } else if over.is_some() {
            // over a given box, one step decides: every root in it is in K, so K off the
            // box in some coordinate is no root there
            return Ok(match verdict {
                KVerdict::Off => Inner::Excluded,
                // K meets the box: widened over K, the box may take it inside (the second
                // pass's next round); a step that does not contract decides nothing
                KVerdict::Straddles if q < 1.0 => Inner::Straddles(kk),
                KVerdict::Straddles | KVerdict::Inside => Inner::Unproved,
            });
        } else {
            let contracts = q < 1.0;
            if !contracts {
                return Ok(Inner::Unproved); // the span is too long for its Jacobian to be one preconditioner's
            }
            // the next box from the contraction: K's radii are at most d + |C| w for a box
            // of radii w about the centre, d those of Y G, so the box of radii
            // w = (I - |C|)^-1 d holds its K; a half more, to clear rounding and the change
            // of C with the box
            let mut d = [0.0_f64; 4];
            for (di, yi) in d.iter_mut().zip(&y) {
                let mut v = IZ;
                for (&yij, gj) in yi.iter().zip(&sg) {
                    v = v + Ivl::from(yij) * gj.at(0, 0);
                }
                *di = (v - Ivl::from(v.mid())).mag() + v.mid().abs();
            }
            let mut w = d;
            for _ in 0..200 {
                let mut wn = [0.0_f64; 4];
                for ((wni, &di), ci) in wn.iter_mut().zip(&d).zip(&cm) {
                    *wni = di;
                    for (cij, &wj) in ci.iter().zip(&w) {
                        *wni += cij.mag() * wj;
                    }
                }
                w = wn;
            }
            for ((xi, &wi), &mi) in bx_.iter_mut().zip(&w).zip(&xm) {
                let rr = 1.5 * wi + 1.0e-12 * (1.0 + mi.abs());
                if !rr.is_finite() {
                    return Ok(Inner::Unproved);
                }
                *xi = Ivl::new(mi - rr, mi + rr);
            }
            // a box across a knot line below C^2, for a use of order 1 or less: the one solve
            // can never prove it (`knot_cuts` refuses its partials, so the next step's Jacobian
            // is refused and nothing is proved), so it goes cell by cell (#15183). Decided by
            // where the box lies, before anything is evaluated over it
            if divert && cells == [FaceCell::Hull; 2] && over.is_none() {
                let [f0, f1] = faces_n;
                if crosses_low_line_at(f0, &bx_, 0) || crosses_low_line_at(f1, &bx_, 1) {
                    return Ok(Inner::Crosses(bx_));
                }
            }
        }
    }
    if !proved {
        return Ok(Inner::Unproved);
    }
    // tighten the proved box (not the fork's): Krawczyk's operator again, about the box's
    // own centre and with the Jacobian over the box, intersected with it, while that
    // shrinks the box by a tenth or more. Every root in a box is in its K, so the box
    // keeps the feet; its C (over a box that holds the final one) still bounds the
    // coefficients' solve, and a narrower box is a smaller q, so the coefficients'
    // enclosures (rho / (1 - q) at every order) are tighter
    for _ in 0..TIGHTEN_STEPS {
        let width = |b: &[Ivl; 4]| b.iter().map(|x| x.hi - x.lo).fold(0.0_f64, cmax);
        let w0 = width(&bx_);
        let xc = bx_.map(Ivl::mid);
        let Ok(pc) = partials_over(xc, xc, 2) else {
            break;
        };
        let sxc = xc.map(|x| Ser::constant(Ivl::from(x)));
        let Ok((sgc, _)) =
            gn_evaluate(&system(&pc, &leader, &tangent, r, side, law_r), &sxc, point)
        else {
            break;
        };
        let Ok(pb) = partials_over(bx_.map(|x| x.lo), bx_.map(|x| x.hi), 2) else {
            break;
        };
        let sxb = bx_.map(Ser::constant);
        let Ok(jac) = gn_jacobian(&system(&pb, &leader, &tangent, r, side, law_r), &sxb) else {
            break;
        };
        let Ok(yc) = inverse4(&jac.map(|row| row.map(Ivl::mid))) else {
            break;
        };
        let (cc, qc) = contraction(&yc, &jac);
        let contracts = qc < 1.0;
        if !contracts {
            break;
        }
        let mut next = bx_;
        let mut empty = false;
        let kc = k_operator(&xc, &yc, &sgc.each_ref().map(|g| g.at(0, 0)), &cc, &bx_);
        for (xi, kv) in next.iter_mut().zip(kc) {
            let lo = cmax(xi.lo, kv.lo);
            let hi = cmin(xi.hi, kv.hi);
            let holds = lo <= hi;
            if !holds {
                empty = true;
            }
            *xi = Ivl::new(lo, hi);
        }
        if empty {
            break;
        }
        // the C and q over the box just shrunk hold for the narrower one
        y = yc;
        cm = cc;
        q = qc;
        bx_ = next;
        let shrank = width(&bx_) < 0.9 * w0;
        if !shrank {
            break;
        }
    }

    // the feet's Taylor coefficients over the span, order by order: x = -Y R + C x,
    // |x| <= rho
    let lk = tr.s() + tr.e() + 1;
    let pk = gn_try!(
        partials_over(bx_.map(|x| x.lo), bx_.map(|x| x.hi), lk),
        Inner::Unproved
    );
    let sysk = system(&pk, &leader, &tangent, r, side, law_r);
    let mut sx = bx_.map(Ser::constant);
    // the highest power of eps solved for (the section's jets of order n are the face
    // curve's)
    let eps_solved = if n >= 1 {
        cmin_usize(tr.e(), n - 1)
    } else {
        tr.e()
    };
    for tot in 1..=(tr.s() + tr.e()) {
        // this order's coefficients are those with i + k = tot, k <= eps_solved: G's at
        // them depend only on x's at lower or equal powers of sigma and eps, so G is
        // computed only there (bit for bit the whole truncation's, `Trunc::computing`),
        // and not at all at an order with none to solve (the last ones, where every
        // i = tot - k is past tr.s)
        let eps = cmin_usize(eps_solved, tot);
        if tot > tr.s() + eps {
            continue;
        }
        let (sg, _) = gn_try!(
            gn_evaluate(&sysk, &sx, tr.computing(tot, eps)),
            Inner::Unproved
        );
        for i in 0..=cmin_usize(tr.s(), tot) {
            let k = tot - i;
            // the section's jets of order n are the face curve's (gn_top_jets, 3bc54a4)
            if k > tr.e() || (n >= 1 && k >= n) {
                continue;
            }
            let mut yr = [IZ; 4];
            let mut rho = 0.0_f64;
            for (yrr, yrow) in yr.iter_mut().zip(&y) {
                let mut v = IZ;
                for (&yrc, gc) in yrow.iter().zip(&sg) {
                    v = v - Ivl::from(yrc) * gc.at(i, k);
                }
                *yrr = v;
                rho = cmax(rho, v.mag());
            }
            // 1 - q rounded down: the quotient rounded up is then at least the exact one
            let gap = round_down(1.0 - q);
            if !gap.is_finite() || gap <= 0.0 {
                return Ok(Inner::Unproved);
            }
            rho = round_up(rho / gap);
            if !rho.is_finite() {
                return Ok(Inner::Unproved);
            }
            for ((sxr, &yrr), crow) in sx.iter_mut().zip(&yr).zip(&cm) {
                let mut v = yrr;
                for &crc in crow {
                    v = v + crc * Ivl::new(-rho, rho);
                }
                sxr.set(i, k, v);
            }
        }
    }
    let (_, mut f, v, nrm) = gn_try!(gn_evaluate_vn(&sysk, &sx, tr), Inner::Unproved);
    gn_top_jets(n, tr, &nrm, &mut f);
    // the faces' first partials over the box: |S_u| |du| + |S_v| |dv| bounds |dS| for a uv
    // step (du, dv) (#2690: kept apart, where the fork sums them)
    let mut ds = [[0.0_f64; 2]; 2];
    let mut ds_lo = [[0.0_f64; 2]; 2];
    for ((dsf, dsl), pf) in ds.iter_mut().zip(ds_lo.iter_mut()).zip(&pk) {
        let su = pf.at(1, 0);
        let sv = pf.at(0, 1);
        let du = crate::geom::vector3d::sqrt(
            su.x.mag() * su.x.mag() + su.y.mag() * su.y.mag() + su.z.mag() * su.z.mag(),
        );
        let dv = crate::geom::vector3d::sqrt(
            sv.x.mag() * sv.x.mag() + sv.y.mag() * sv.y.mag() + sv.z.mag() * sv.z.mag(),
        );
        *dsf = [du, dv];
        let lu = crate::geom::vector3d::sqrt(
            su.x.mig() * su.x.mig() + su.y.mig() * su.y.mig() + su.z.mig() * su.z.mig(),
        );
        let lv = crate::geom::vector3d::sqrt(
            sv.x.mig() * sv.x.mig() + sv.y.mig() * sv.y.mig() + sv.z.mig() * sv.z.mig(),
        );
        *dsl = [lu, lv];
    }
    Ok(Inner::Proved(Box::new((
        Enclosure {
            f,
            x: sx,
            ds,
            ds_lo,
            v,
        },
        bx_,
    ))))
}

/// `std::min` of the loop bound `i <= tr.s && i <= lTot`.
#[inline]
fn cmin_usize(a: usize, b: usize) -> usize {
    if b < a {
        b
    } else {
        a
    }
}

/// The fourth s-derivatives' bounds of a span: `m4[f][k]` per face and jet, `uv4[f]` per
/// face's uv curves (infinite where not proved), then the same bounds' least values at the
/// span's middle (`pM4Lo`, `pUV4Lo`): no piece holding the middle bounds them lower. Then
/// the uv curves' bound and least value as the fork takes them,
/// `(|S_u| + |S_v|) max(|u''''|, |v''''|)` (#2690: `gn_certify_span` steers its levels by
/// these, and certifies by `uv4`).
pub type Fourth = (
    [[f64; MAX_E + 1]; 2],
    [f64; 2],
    [[f64; MAX_E + 1]; 2],
    [f64; 2],
    [f64; 2],
    [f64; 2],
);

/// The Taylor-form bound `sum_{j<M} (4+j)! |c[4+j](m)| (h/2)^j / j!` plus the remainder
/// `(4+M)! |c[4+M](box)| (h/2)^M / M!`, `M = SM_GN_TAYLOR`, of one coefficient row.
fn taylor_bound(mid: impl Fn(usize) -> f64, rem: f64, h: f64) -> f64 {
    let mut m = 0.0_f64;
    let mut fact = 24.0_f64;
    let mut pow = 1.0_f64;
    let mut jf = 1.0_f64;
    for j in 0..TAYLOR {
        if j > 0 {
            fact *= (4 + j) as f64;
            pow *= 0.5 * h;
            jf *= j as f64;
        }
        m += fact * mid(4 + j) * pow / jf;
    }
    fact *= (4 + TAYLOR) as f64;
    pow *= 0.5 * h;
    jf *= TAYLOR as f64;
    m += fact * rem * pow / jf;
    m
}

/// `sm_GnFourthOver`: the bound on the jets' fourth s-derivatives over `[a.s, b.s]`, per
/// face and jet (`m4[f][k]`, infinite where not proved), and on the feet's uv curves',
/// each parameter's times the face's partial in it over the span's box,
/// `|S_u| |u''''| + |S_v| |v''''|` (`uv4[f]`; the fork's is
/// `(|S_u| + |S_v|) max(|u''''|, |v''''|)`, #2690), in their Taylor form at the
/// interval's middle m: `c''''(s)` is within `sum_{j<M} c^(4+j)(m) (s - m)^j / j!` plus the
/// remainder `c^(4+M)(interval) (h/2)^M / M!`, `M = SM_GN_TAYLOR`, the derivatives at the
/// middle enclosed on that one point and the remainder's over the interval.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_fourth_over(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
) -> Result<Fourth> {
    let (fourth, _) = gn_fourth_over_series(
        k, faces_n, leader_n, faces, center, a, b, r, side, n, None, 0, None,
    )?;
    Ok(fourth)
}

/// The series `sm_GnFourthOver` hands a varying thumbweight's row terms (its `pF`, `pV`,
/// `pFm`, `pVm`, `pdMid`, 7102bc5): the feet's and V's over the span to order
/// `4 + SM_GN_TAYLOR` in s, at its middle to one less, and the middle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceSeries {
    /// `pF`: the feet over the span.
    pub f: [VSer<Ivl>; 2],
    /// `pV`: V over the span.
    pub v: VSer<Ivl>,
    /// `pFm`: the feet at the middle.
    pub fm: [VSer<Ivl>; 2],
    /// `pVm`: V at the middle.
    pub vm: VSer<Ivl>,
    /// `pdMid`: the middle.
    pub mid: f64,
}

/// [`gn_fourth_over`] with the series of [`PieceSeries`] (`None` where they are not all
/// proved, as the fork's out-params are then not all set), and the fork's `pLaw` and
/// `lExtra` (0d43b4c): a ball whose radius varies, and the orders in s the series take
/// past `4 + SM_GN_TAYLOR` (the touch circle's m'); and its `pDefs` (c5b6bb7): the faces
/// that are blends by their definitions, enclosed over the boxes.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_fourth_over_series(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    law: Option<&GnLaw<'_>>,
    extra: usize,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<(Fourth, Option<PieceSeries>)> {
    let pe = piece_enclosures(
        k, faces_n, leader_n, faces, center, a, b, r, side, n, law, extra, defs, false,
    )?;
    let h = b.s - a.s;
    Ok(match (pe.e5, pe.mid) {
        (Some(e5), Some((mid, e4))) => fourth_of(&e5, &e4, h, n, mid),
        _ => (UNPROVED_FOURTH, None),
    })
}

/// No fourth derivative proved ([`gn_fourth_over_series`]'s outputs as the fork leaves
/// them where an enclosure fails): infinite bounds, zero least values.
const UNPROVED_FOURTH: Fourth = (
    [[f64::INFINITY; MAX_E + 1]; 2],
    [f64::INFINITY; 2],
    [[0.0; MAX_E + 1]; 2],
    [0.0; 2],
    [f64::INFINITY; 2],
    [0.0; 2],
);

/// A piece's enclosures ([`gn_fourth_over_series`]'s): the feet over it (`e5`, to order
/// `4 + SM_GN_TAYLOR + extra`, `None` where not proved), and its middle with the feet's
/// point enclosure there (`e4`, one order less).
struct PieceEnclosures {
    e5: Option<Enclosure>,
    mid: Option<(f64, Enclosure)>,
}

/// The enclosures of [`gn_fourth_over_series`], in its order: the piece's, then the middle's
/// station and its point enclosure, the latter (`always` false) only where the piece's is
/// proved, as the fork; `always`, also where it is not (the feet's Taylor models start from
/// the middle's).
#[allow(clippy::too_many_arguments)]
fn piece_enclosures(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    law: Option<&GnLaw<'_>>,
    extra: usize,
    defs: Option<&mut GnDefs<'_>>,
    always: bool,
) -> Result<PieceEnclosures> {
    let mut defs = defs;
    let mut out = PieceEnclosures {
        e5: None,
        mid: None,
    };
    out.e5 = gn_enclose(
        k,
        faces_n,
        leader_n,
        a,
        b,
        r,
        side,
        n,
        4 + TAYLOR + extra,
        law,
        defs.as_deref_mut(),
    )?;
    if out.e5.is_none() && !always {
        return Ok(out);
    }
    // the middle's station, and the series there
    let mid = 0.5 * (a.s + b.s);
    let mut seed = [0.0_f64; 4];
    for (c, sc) in seed.iter_mut().enumerate() {
        let ac = a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
        let bc = b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
        *sc = 0.5 * (ac + bc);
    }
    let sm = gn_try!(center.evaluate_point(mid), out);
    let point = Trunc::new(0, 0)?;
    let tol = 1.0e-9 * smax(1.0, sm.length());
    let mid_r = gn_try!(law_series(law, mid, 0, false), out);
    let smid = gn_try!(
        gn_solve_station_counted(
            k,
            faces,
            center,
            mid,
            seed,
            r,
            side,
            point,
            tol,
            mid_r.as_ref()
        )
        .0,
        out
    );
    let e4 = gn_enclose(
        k,
        faces_n,
        leader_n,
        &smid,
        &smid,
        r,
        side,
        n,
        4 + TAYLOR - 1 + extra,
        law,
        defs,
    )?;
    out.mid = e4.map(|e| (mid, e));
    Ok(out)
}

/// [`gn_fourth_over_series`]'s terms from a piece's enclosure `e5` (over it, length h) and
/// its middle's `e4` (at `mid`): its body after the enclosures.
fn fourth_of(
    e5: &Enclosure,
    e4: &Enclosure,
    h: f64,
    n: usize,
    mid: f64,
) -> (Fourth, Option<PieceSeries>) {
    let series = PieceSeries {
        f: e5.f,
        v: e5.v,
        fm: e4.f,
        vm: e4.v,
        mid,
    };
    let (mut m4, mut uv4, mut m4_lo, mut uv4_lo, mut uv4_fork, mut uv4_lo_fork) = UNPROVED_FOURTH;
    // the uv curves' fourth derivatives, in the same Taylor form, each times the face's
    // partial in its parameter over the span's box (#2690)
    for (f, ((uvf, uvk), dsf)) in uv4
        .iter_mut()
        .zip(uv4_fork.iter_mut())
        .zip(&e5.ds)
        .enumerate()
    {
        let (mut sum, mut dmax) = (0.0_f64, 0.0_f64);
        for (c, &dsc) in ((2 * f)..(2 * f + 2)).zip(dsf) {
            let (Some(x4), Some(x5)) = (e4.x.get(c), e5.x.get(c)) else {
                return (UNPROVED_FOURTH, Some(series));
            };
            let m = taylor_bound(|i| x4.at(i, 0).mag(), x5.at(4 + TAYLOR, 0).mag(), h);
            sum += m * dsc;
            dmax = cmax(dmax, m);
        }
        *uvf = sum;
        // (the fork's: the largest fourth derivative times |S_u| + |S_v|)
        *uvk = dmax * dsf.iter().sum::<f64>();
    }
    // the same, least: the middle's 4! c[4] alone, each times the face's least partial
    for (f, ((uvl, uvk), dsl)) in uv4_lo
        .iter_mut()
        .zip(uv4_lo_fork.iter_mut())
        .zip(&e4.ds_lo)
        .enumerate()
    {
        let (mut sum, mut dlo) = (0.0_f64, 0.0_f64);
        for (c, &dsc) in ((2 * f)..(2 * f + 2)).zip(dsl) {
            let Some(x4) = e4.x.get(c) else {
                return (UNPROVED_FOURTH, Some(series));
            };
            sum += 24.0 * x4.at(4, 0).mig() * dsc;
            dlo = cmax(dlo, 24.0 * x4.at(4, 0).mig());
        }
        *uvl = sum;
        *uvk = dlo * dsl.iter().sum::<f64>();
    }
    for ((m4f, f4), f5) in m4.iter_mut().zip(&e4.f).zip(&e5.f) {
        for (kk, m4fk) in m4f.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for (s4, s5) in f4.a.iter().zip(&f5.a) {
                // c^(4+j)(m) = (4+j)! F[4+j]: the Taylor polynomial's bound over
                // |s - m| <= h/2, and the remainder
                let m = taylor_bound(|i| s4.at(i, kk).mag(), s5.at(4 + TAYLOR, kk).mag(), h);
                sq += m * m;
            }
            *m4fk = crate::geom::vector3d::sqrt(sq);
        }
    }
    for (m4lf, f4) in m4_lo.iter_mut().zip(&e4.f) {
        for (kk, m4lfk) in m4lf.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for s4 in &f4.a {
                let m = 24.0 * s4.at(4, kk).mig();
                sq += m * m;
            }
            *m4lfk = crate::geom::vector3d::sqrt(sq);
        }
    }
    (
        (m4, uv4, m4_lo, uv4_lo, uv4_fork, uv4_lo_fork),
        Some(series),
    )
}

/// `sm_GnVarying`: a section whose thumbweight varies along s (the equal area one,
/// resolved per station): its order, `r1 r2`, and I's enclosures on their grid.
#[derive(Clone, Debug, PartialEq)]
pub struct GnVarying {
    /// `n`.
    pub n: usize,
    /// `r1`, `r2` (the fork's `dRR` is their product, rounded; the certificate takes it
    /// in intervals from the radii).
    pub r: [f64; 2],
    /// `pCells`: I's enclosures, computed once per cell.
    pub cells: GnAreaCells,
    /// The thumbweight's proved enclosure, `[lo, hi]`, over every span certified with this
    /// ([`gn_certify_span`]: the union of its pieces' [`GnRowTerms::w`] at each level whose
    /// bound it returns); `[inf, -inf]` before any. Not upstream.
    pub w: [f64; 2],
}

/// `sm_GnRowTerms`: the largest `|(sum_ik dg_ikj c'_ik)|` (`x`) and `|W_j|` (`y`),
/// the least `W_j` (`w_lo`) per row, the largest `|S - O|` (`r`), and the equal area
/// thumbweight's proved enclosure over the pieces (`w`).
#[derive(Clone, Debug, PartialEq)]
pub struct GnRowTerms {
    /// `X`.
    pub x: Vec<f64>,
    /// `Y`.
    pub y: Vec<f64>,
    /// `Wlo`.
    pub w_lo: Vec<f64>,
    /// `R`.
    pub r: f64,
    /// The equal area thumbweight over every piece folded in, `[lo, hi]`: the union of the
    /// interval Newton roots `w0` (each holds every piece's root, for every s in it); `[inf,
    /// -inf]` before any. Not upstream: the fork discards `w0` after the inversion.
    pub w: [f64; 2],
}

/// `sm_Centred`: the coefficients `0..=SM_GN_TAY` of a series over a piece in their
/// Taylor form at its middle m: `c_i(s) = sum_{j < K - i} C(i + j, i) c_(i+j)(m) d^j +
/// C(K, i) c_K(xi) d^(K-i)`, `d = s - m`, from the middle's coefficients `mid[0..K-1]` and
/// the piece's `top` at order K.
#[must_use]
pub fn centred(mid: &[Ivl], top: Ivl, kk: usize, d: Ivl) -> Tay<Ivl> {
    let mut r = Tay::<Ivl>::zero();
    for i in 0..=TAY {
        let mut sum = Ivl::from(0.0);
        let mut pow = Ivl::from(1.0);
        let mut j = 0;
        while i + j < kk {
            if j > 0 {
                pow = pow * d;
            }
            let m = mid.get(i + j).copied().unwrap_or(IZ);
            sum = sum + Ivl::from(binom(i + j, i)) * m * pow;
            j += 1;
        }
        pow = pow * d;
        r.set(i, sum + Ivl::from(binom(kk, i)) * top * pow);
    }
    r
}

/// A vector series' coordinate q's coefficient `(i, k)`.
fn vat(v: &VSer<Ivl>, q: usize, i: usize, k: usize) -> Ivl {
    v.a.get(q).map_or(IZ, |c| c.at(i, k))
}

/// `sm_GnRowTermsOver`: the row terms over one piece, folded into `rt` (largest X, Y, R;
/// least W); `false` where w's enclosure or a term is not proved. `f`, `centre`: the
/// feet's and V's series over the piece (to order `K = 4 + SM_GN_TAYLOR` in s); `fm`,
/// `cm`: at its middle (to K - 1); `d`: s less the middle over the piece.
#[allow(clippy::too_many_arguments)]
pub fn gn_row_terms_over(
    k: &Kernel,
    vary: &mut GnVarying,
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    guess: f64,
    bar: &GnSectionFormT<Ivl>,
    o: [f64; 3],
    rt: &mut GnRowTerms,
) -> bool {
    const K: usize = 4 + TAYLOR;
    let n = vary.n;
    let [f0, f1] = f;
    let [fm0, fm1] = fm;
    // x = (F1 - V).(F2 - V) / (r1 r2): at the middle to order K - 1, over the piece at
    // order K
    let mut x_mid = [IZ; K];
    let mut x_top = IZ;
    for q in 0..3 {
        for i in 0..=K {
            for l in 0..=i {
                if i < K {
                    let t = (vat(fm0, q, l, 0) - vat(cm, q, l, 0))
                        * (vat(fm1, q, i - l, 0) - vat(cm, q, i - l, 0));
                    if let Some(xi) = x_mid.get_mut(i) {
                        *xi = *xi + t;
                    }
                } else {
                    x_top = x_top
                        + (vat(f0, q, l, 0) - vat(centre, q, l, 0))
                            * (vat(f1, q, i - l, 0) - vat(centre, q, i - l, 0));
                }
            }
        }
    }
    let [r1, r2] = vary.r;
    let inv_rr = Ivl::from(1.0) / (Ivl::from(r1) * Ivl::from(r2));
    for xi in &mut x_mid {
        *xi = *xi * inv_rr;
    }
    x_top = x_top * inv_rr;
    let u = gn_half_angle(&centred(&x_mid, x_top, K, d));
    let tg = gn_target(&u);
    let phi = vary.cells.at(k, guess) - tg.at(0);
    let mut w0 = IZ;
    let mut root = false;
    let mut rho = 1.0e-12 * (1.0 + guess.abs());
    let mut it = 0;
    while it < 40 && !root {
        let w = Ivl::new(guess - rho, guess + rho);
        let below_one = w.hi < 1.0;
        if !below_one {
            break;
        }
        let d1 = vary.cells.over(k, w).at(1);
        if d1.hi < 0.0 {
            let nn = Ivl::from(guess) - phi / d1;
            if nn.lo > w.lo && nn.hi < w.hi {
                w0 = nn;
                root = true;
            }
        }
        it += 1;
        rho *= 4.0;
    }
    if !root {
        return false;
    }
    let [w_lo, w_hi] = rt.w;
    rt.w = [cmin(w_lo, w0.lo), cmax(w_hi, w0.hi)];
    let over_w0 = vary.cells.over(k, w0);
    let sw = gn_invert(&tg, &over_w0, w0);
    let form = gn_form_normalized(n, sw);
    // the jets c'_ik as series in s over the piece
    let mut c = [[[Tay::<Ivl>::zero(); 3]; MAX_E + 1]; 2];
    let mut r_max = 0.0_f64;
    for (cf, (ff, ffm)) in c.iter_mut().zip(f.iter().zip(fm)) {
        let mut sum = 0.0_f64;
        for (kk, ck) in cf.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for (q, ckq) in ck.iter_mut().enumerate() {
                let mut ac_mid = [IZ; K];
                for (i, a) in ac_mid.iter_mut().enumerate() {
                    *a = vat(ffm, q, i, kk);
                }
                *ckq = centred(&ac_mid, vat(ff, q, K, kk), K, d);
                if kk == 0 {
                    let oq = o.get(q).copied().unwrap_or(f64::NAN);
                    ckq.set(0, ckq.at(0) - Ivl::from(oq));
                }
                sq += ckq.at(0).mag() * ckq.at(0).mag();
            }
            sum += crate::geom::vector3d::sqrt(sq);
        }
        r_max = cmax(r_max, sum);
    }
    for (j, wj) in form.w.iter().enumerate() {
        let mut sq = 0.0_f64;
        for q in 0..3 {
            // the coefficient of sigma^4 of sum_ik dg_ikj c'_ik
            let mut s4 = IZ;
            for ((gf, bf), cf) in form.g.iter().zip(&bar.g).zip(&c) {
                for ((gk, bk), ck) in gf.iter().zip(bf).zip(cf).take(n + 1) {
                    let (Some(&g), Some(&b), Some(ckq)) = (gk.get(j), bk.get(j), ck.get(q)) else {
                        continue;
                    };
                    let mut dg = g;
                    dg.set(0, dg.at(0) - b);
                    for i in 0..=TAY {
                        s4 = s4 + dg.at(i) * ckq.at(TAY - i);
                    }
                }
            }
            sq += s4.mag() * s4.mag();
        }
        let dx = 24.0 * crate::geom::vector3d::sqrt(sq);
        let dy = 24.0 * wj.at(TAY).mag();
        let dwlo = wj.at(0).lo;
        if !(dx.is_finite() && dy.is_finite() && dwlo > 0.0) {
            return false;
        }
        let (Some(xj), Some(yj), Some(lj)) = (rt.x.get_mut(j), rt.y.get_mut(j), rt.w_lo.get_mut(j))
        else {
            return false;
        };
        *xj = cmax(*xj, dx);
        *yj = cmax(*yj, dy);
        *lj = cmin(*lj, dwlo);
    }
    if !r_max.is_finite() {
        return false;
    }
    rt.r = cmax(rt.r, r_max);
    true
}

/// The arc's homogeneous rows `H[j][q]` and weights `W[j]` ([`arc_rows`]).
pub type ArcRows<S> = ([[Tay<S>; 3]; 3], [Tay<S>; 3]);

/// `sm_ArcRows` (cc08236): the ball's circle as rows: the rational quadratic arc from the
/// rail `rho1` to `rho2` about the ball's centre `m`, its control points `rho1, P1, rho2`
/// and weights `1, w, 1`, `w = cos(theta / 2)` (theta the angle between the radii
/// `b_i = rho_i - m`), `P1 = m + (mid - m) / w^2` where the rails' tangents meet; as
/// homogeneous rows `H_j = W_j (P_j - O)` about a fixed point `o`. The series are in s,
/// over a ring S (`f64`, or intervals). `None` where a radius is not positive or the arc
/// is a half turn or more (no single arc).
#[must_use]
pub fn arc_rows<S: TayScalar + GnScalar>(
    rho1: &[Tay<S>; 3],
    rho2: &[Tay<S>; 3],
    m: &[Tay<S>; 3],
    o: [f64; 3],
) -> Option<ArcRows<S>> {
    let mut b11 = Tay::<S>::zero();
    let mut b22 = Tay::<S>::zero();
    let mut b12 = Tay::<S>::zero();
    for ((&r1, &r2), &mq) in rho1.iter().zip(rho2).zip(m) {
        let b1 = r1 - mq;
        let b2 = r2 - mq;
        b11 = b11 + b1 * b1;
        b22 = b22 + b2 * b2;
        b12 = b12 + b1 * b2;
    }
    if !b11.at(0).is_positive() || !b22.at(0).is_positive() {
        return None;
    }
    // cos^2(theta / 2) = (1 + cos theta) / 2
    let half = (Tay::<S>::lift(1.0) + b12 / tay_sqrt(&(b11 * b22))) * Tay::<S>::lift(0.5);
    if !half.at(0).is_positive() {
        return None; // a half turn or more: no single arc
    }
    let w = tay_sqrt(&half);
    let inv = Tay::<S>::lift(1.0) / half;
    let ws = [Tay::<S>::lift(1.0), w, Tay::<S>::lift(1.0)];
    let mut h = [[Tay::<S>::zero(); 3]; 3];
    for (q, (((&r1, &r2), &mq), &oq)) in rho1.iter().zip(rho2).zip(m).zip(&o).enumerate() {
        let so = Tay::<S>::lift(oq);
        let p1 = mq + ((r1 + r2) * Tay::<S>::lift(0.5) - mq) * inv;
        let [h0, h1, h2] = &mut h;
        if let (Some(a), Some(b), Some(c)) = (h0.get_mut(q), h1.get_mut(q), h2.get_mut(q)) {
            *a = r1 - so;
            *b = w * (p1 - so);
            *c = r2 - so;
        }
    }
    Some((h, ws))
}

/// A piece's arc row terms ([`arc_terms_over`]): per row the largest `24 |H_j''''|` (`x`)
/// and `24 |W_j''''|` (`y`) and the least `W_j` (`w_lo`), and the largest `|S - O|` (`r`),
/// over the piece. A term not proved is infinite (`w_lo` minus infinity). Not upstream.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcTerms {
    /// `24 |H_j''''|`, per row.
    pub x: [f64; 3],
    /// `24 |W_j''''|`, per row.
    pub y: [f64; 3],
    /// The least `W_j`, per row.
    pub w_lo: [f64; 3],
    /// The largest `|S - O|`.
    pub r: f64,
}

impl ArcTerms {
    /// Nothing proved.
    pub const UNPROVED: Self = Self {
        x: [f64::INFINITY; 3],
        y: [f64::INFINITY; 3],
        w_lo: [f64::NEG_INFINITY; 3],
        r: f64::INFINITY,
    };

    /// Both terms' tighter bound, term by term: each holds over the piece, so the tighter
    /// one does (a NaN is not a bound).
    #[must_use]
    pub fn meet(self, o: Self) -> Self {
        let upper = |a: f64, b: f64| if a.is_nan() || b < a { b } else { a };
        let lower = |a: f64, b: f64| if a.is_nan() || b > a { b } else { a };
        let mut m = self;
        for (j, ((mx, my), mw)) in
            m.x.iter_mut()
                .zip(m.y.iter_mut())
                .zip(m.w_lo.iter_mut())
                .enumerate()
        {
            *mx = upper(*mx, o.x.get(j).copied().unwrap_or(f64::INFINITY));
            *my = upper(*my, o.y.get(j).copied().unwrap_or(f64::INFINITY));
            *mw = lower(*mw, o.w_lo.get(j).copied().unwrap_or(f64::NEG_INFINITY));
        }
        m.r = upper(m.r, o.r);
        m
    }

    /// Whether the terms prove the piece's rows: every row's terms finite and its least
    /// weight positive, and `R` finite ([`Self::fold`]'s test).
    #[must_use]
    pub fn proves(&self) -> bool {
        self.x.iter().chain(&self.y).all(|v| v.is_finite())
            && self.w_lo.iter().all(|&w| w > 0.0)
            && self.r.is_finite()
    }

    /// Folds the terms into `rt` (largest X, Y, R; least W), as `sm_ArcRowTermsOver` does;
    /// `false` where a row's term is not finite or its least weight not positive, or `R` is
    /// not finite.
    pub fn fold(&self, rt: &mut GnRowTerms) -> bool {
        for (j, ((&dx, &dy), &dwlo)) in self.x.iter().zip(&self.y).zip(&self.w_lo).enumerate() {
            if !(dx.is_finite() && dy.is_finite() && dwlo > 0.0) {
                return false;
            }
            let (Some(xj), Some(yj), Some(lj)) =
                (rt.x.get_mut(j), rt.y.get_mut(j), rt.w_lo.get_mut(j))
            else {
                return false;
            };
            *xj = cmax(*xj, dx);
            *yj = cmax(*yj, dy);
            *lj = cmin(*lj, dwlo);
        }
        if !self.r.is_finite() {
            return false;
        }
        rt.r = cmax(rt.r, self.r);
        true
    }
}

/// A piece's arc row terms both ways: `old` from interval series over the piece (the
/// fork's `sm_ArcRowTermsOver`), and `tight`, the tighter of those and the same terms from
/// Taylor models in s over the piece ([`super::taylor_model`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcTermsBoth {
    /// The interval series' terms (the fork's).
    pub old: ArcTerms,
    /// Term by term the tighter of `old` and the Taylor models' terms.
    pub tight: ArcTerms,
}

/// The range a series coefficient's enclosure takes over the piece: an interval's own, a
/// Taylor model's bounded over `[-1, 1]`.
trait OverPiece: TayScalar + GnScalar {
    fn over_piece(&self) -> Ivl;
}

impl OverPiece for Ivl {
    fn over_piece(&self) -> Ivl {
        *self
    }
}

impl OverPiece for Tm {
    fn over_piece(&self) -> Ivl {
        self.range()
    }
}

/// The arc row terms from the rails' and the centre's series over a piece (`rho`, `mc`)
/// and, for a varying ball (`dm`), the centre's s-derivative: the touch circle about
/// `m + ((rho1 - m) . m' / m' . m') m'`, its rows ([`arc_rows`]) and their terms. One
/// formula for both rings, so the interval terms are the fork's operation for operation.
fn arc_terms_of<S: OverPiece>(
    rho: &[[Tay<S>; 3]; 2],
    mut mc: [Tay<S>; 3],
    dm: Option<&[Tay<S>; 3]>,
    o: [f64; 3],
) -> ArcTerms {
    let [rho0, rho1] = rho;
    if let Some(dm) = dm {
        let mut dd = Tay::<S>::zero();
        let mut bd = Tay::<S>::zero();
        for ((&dq, &mq), &r0q) in dm.iter().zip(&mc).zip(rho0) {
            dd = dd + dq * dq;
            bd = bd + (r0q - mq) * dq;
        }
        if !dd.at(0).is_positive() {
            return ArcTerms::UNPROVED;
        }
        let kk = bd / dd;
        for (mq, &dq) in mc.iter_mut().zip(dm) {
            *mq = *mq + kk * dq;
        }
    }
    let Some((h, w)) = arc_rows(rho0, rho1, &mc, o) else {
        return ArcTerms::UNPROVED;
    };
    // |S - O| <= |m - O| + the ball's radius over the piece
    let mut d_mo = 0.0_f64;
    let mut d_rad = 0.0_f64;
    for ((mq, &oq), r0q) in mc.iter().zip(&o).zip(rho0) {
        let dd = (mq.at(0) - S::lift(oq)).over_piece().mag();
        d_mo += dd * dd;
        let b = (r0q.at(0) - mq.at(0)).over_piece().mag();
        d_rad += b * b;
    }
    let mut t = ArcTerms::UNPROVED;
    t.r = crate::geom::vector3d::sqrt(d_mo) + crate::geom::vector3d::sqrt(d_rad);
    for (((hj, wj), (tx, ty)), tw) in h
        .iter()
        .zip(&w)
        .zip(t.x.iter_mut().zip(t.y.iter_mut()))
        .zip(t.w_lo.iter_mut())
    {
        let mut sq = 0.0_f64;
        for hq in hj {
            let m = hq.at(TAY).over_piece().mag();
            sq += m * m;
        }
        *tx = 24.0 * crate::geom::vector3d::sqrt(sq);
        *ty = 24.0 * wj.at(TAY).over_piece().mag();
        *tw = wj.at(0).over_piece().lo;
    }
    t
}

/// [`centred`] as Taylor models in `t = (s - m) / delta` on `[-1, 1]`, `delta` at least
/// `|s - m|` over the piece (#14405): coefficient i is
/// `sum_{j < K - i} C(i + j, i) c_(i+j)(m) delta^j t^j + C(K, i) c_K(xi) delta^(K - i) t^(K - i)`,
/// the middle's coefficients and the piece's `top` kept as the polynomial's (a power past
/// the model's degree bounded into its remainder). The interval form is this one bounded
/// over `[-1, 1]` term by term.
#[must_use]
pub fn centred_tm(mid: &[Ivl], top: Ivl, kk: usize, delta: f64) -> Tay<Tm> {
    let mut r = Tay::<Tm>::zero();
    let dl = Ivl::from(delta);
    let mut coeffs = [IZ; MAX_S + 2];
    for i in 0..=TAY {
        let mut pow = Ivl::from(1.0);
        let mut j = 0;
        while i + j < kk {
            if j > 0 {
                pow = pow * dl;
            }
            let m = mid.get(i + j).copied().unwrap_or(IZ);
            if let Some(c) = coeffs.get_mut(j) {
                *c = Ivl::from(binom(i + j, i)) * m * pow;
            }
            j += 1;
        }
        pow = pow * dl;
        if let Some(c) = coeffs.get_mut(j) {
            *c = Ivl::from(binom(kk, i)) * top * pow;
        }
        r.set(i, Tm::from_poly(coeffs.get(..=j).unwrap_or(&[]), IZ));
    }
    r
}

/// The order of the rails' and the centre's series over a piece (`K = 4 + SM_GN_TAYLOR`).
const ARC_K: usize = 4 + TAYLOR;

/// A piece's series for the arc: the rails', the centre's, and (a varying ball) the centre's
/// s-derivative.
type ArcSeries<S> = ([[Tay<S>; 3]; 2], [Tay<S>; 3], Option<[Tay<S>; 3]>);

/// The rails' and the centre's coefficient series over a piece, centred at its middle by
/// `centre_of` ([`centred`] for intervals, [`centred_tm`] for Taylor models) from the
/// middle's coefficients and the piece's top one; and, `touch`, the centre's s-derivative.
fn piece_series<S: Copy>(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    touch: bool,
    zero: S,
    centre_of: impl Fn(&[Ivl], Ivl) -> Tay<S>,
) -> ArcSeries<S> {
    let z = Tay { c: [zero; TAY + 1] };
    let mut rho = [[z; 3]; 2];
    let mut mc = [z; 3];
    for q in 0..3 {
        let mut ac_mid = [IZ; ARC_K];
        for (rf, (ff, ffm)) in rho.iter_mut().zip(f.iter().zip(fm)) {
            for (i, a) in ac_mid.iter_mut().enumerate() {
                *a = vat(ffm, q, i, 0);
            }
            if let Some(x) = rf.get_mut(q) {
                *x = centre_of(&ac_mid, vat(ff, q, ARC_K, 0));
            }
        }
        for (i, a) in ac_mid.iter_mut().enumerate() {
            *a = vat(cm, q, i, 0);
        }
        if let Some(x) = mc.get_mut(q) {
            *x = centre_of(&ac_mid, vat(centre, q, ARC_K, 0));
        }
    }
    let dm = touch.then(|| {
        // m' from the centre's coefficients one order up: (i + 1) c_(i+1)
        let mut dm = [z; 3];
        for (q, dq) in dm.iter_mut().enumerate() {
            let mut ac_mid = [IZ; ARC_K];
            for (i, a) in ac_mid.iter_mut().enumerate() {
                *a = vat(cm, q, i + 1, 0) * Ivl::from((i + 1) as f64);
            }
            *dq = centre_of(
                &ac_mid,
                vat(centre, q, ARC_K + 1, 0) * Ivl::from((ARC_K + 1) as f64),
            );
        }
        dm
    });
    (rho, mc, dm)
}

/// The arc's row terms over one piece from interval series (the fork's
/// `sm_ArcRowTermsOver`): every coefficient taken over the whole piece. Arguments as
/// [`arc_terms_over`]'s.
fn arc_terms_interval(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    o: [f64; 3],
    touch: bool,
) -> ArcTerms {
    let (rho, mc, dm) = piece_series(f, centre, fm, cm, touch, IZ, |mid, top| {
        centred(mid, top, ARC_K, d)
    });
    arc_terms_of(&rho, mc, dm.as_ref(), o)
}

/// The arc's row terms over one piece from Taylor models in `s - m` over it
/// ([`centred_tm`]). Arguments as [`arc_terms_over`]'s.
fn arc_terms_taylor(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    o: [f64; 3],
    touch: bool,
) -> ArcTerms {
    let delta = d.mag();
    let zero = Tm::constant(IZ);
    let (rho, mc, dm) = piece_series(f, centre, fm, cm, touch, zero, |mid, top| {
        centred_tm(mid, top, ARC_K, delta)
    });
    arc_terms_of(&rho, mc, dm.as_ref(), o)
}

/// The arc's row terms over one piece, both ways ([`ArcTermsBoth`]). `f`, `centre`: the
/// rails' and the centre's series over the piece (to order `K = 4 + SM_GN_TAYLOR`); `fm`,
/// `cm`: at its middle (to K - 1); `d`: s less the middle over the piece. `touch`
/// (0d43b4c): the ball's radius varies, and the circle is the one it touches the envelope
/// along (`centre` and `cm` one order more, for m').
///
/// The interval terms (`old`) take every coefficient over the whole piece, so the rails
/// and the centre, which move together, enter the circle's radii, their lengths and the
/// rational rows as independent ranges: on a long piece the radii's squared lengths reach
/// 0 or the rows' fourth derivatives are orders of magnitude over their true values. The
/// Taylor models keep each coefficient a polynomial in `s - m` (plus a remainder), so the
/// common motion cancels; the tighter of the two is taken term by term (#14405).
#[must_use]
pub fn arc_terms_over(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    o: [f64; 3],
    touch: bool,
) -> ArcTermsBoth {
    let old = arc_terms_interval(f, centre, fm, cm, d, o, touch);
    let tm = arc_terms_taylor(f, centre, fm, cm, d, o, touch);
    ArcTermsBoth {
        old,
        tight: old.meet(tm),
    }
}

/// The arc's rows' least terms at a piece's middle, from the series there (`fm`, `vm`, a
/// point enclosure): per row the least `24 |H_j''''|` and `24 |W_j''''|` and the greatest
/// `W_j` at the middle; `None` where the arc is not proved there. Every piece holding the
/// middle bounds the rows there, so no level's terms are below these (#14405).
fn arc_least_at(ps: &PieceSeries, o: [f64; 3], touch: bool) -> Option<[[f64; 3]; 3]> {
    let (rho, mut mc, dm) = piece_series(&ps.f, &ps.v, &ps.fm, &ps.vm, touch, IZ, |mid, _| {
        centred(mid, IZ, ARC_K, IZ)
    });
    let [rho0, rho1] = &rho;
    if let Some(dm) = &dm {
        let mut dd = Tay::<Ivl>::zero();
        let mut bd = Tay::<Ivl>::zero();
        for ((&dq, &mq), &r0q) in dm.iter().zip(&mc).zip(rho0) {
            dd = dd + dq * dq;
            bd = bd + (r0q - mq) * dq;
        }
        if !dd.at(0).is_positive() {
            return None;
        }
        let kk = bd / dd;
        for (mq, &dq) in mc.iter_mut().zip(dm) {
            *mq = *mq + kk * dq;
        }
    }
    let (h, w) = arc_rows(rho0, rho1, &mc, o)?;
    let mut out = [[0.0_f64; 3]; 3];
    let [xl, yl, wh] = &mut out;
    for (j, (hj, wj)) in h.iter().zip(&w).enumerate() {
        let mut sq = 0.0_f64;
        for hq in hj {
            let m = hq.at(TAY).mig();
            sq += m * m;
        }
        if let (Some(x), Some(y), Some(ww)) = (xl.get_mut(j), yl.get_mut(j), wh.get_mut(j)) {
            *x = 24.0 * crate::geom::vector3d::sqrt(sq);
            *y = 24.0 * wj.at(TAY).mig();
            *ww = wj.at(0).hi;
        }
    }
    Some(out)
}

/// `sm_ArcRowTermsOver` (cc08236): the arc's row terms over one piece
/// ([`gn_row_terms_over`]'s, every row varying), folded into `rt`; `false` where they are
/// not proved. Not the fork's: the terms are the tighter of the fork's interval ones and
/// the Taylor models' ([`arc_terms_over`], #14405).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn arc_row_terms_over(
    f: &[VSer<Ivl>; 2],
    centre: &VSer<Ivl>,
    fm: &[VSer<Ivl>; 2],
    cm: &VSer<Ivl>,
    d: Ivl,
    o: [f64; 3],
    rt: &mut GnRowTerms,
    touch: bool,
) -> bool {
    arc_terms_over(f, centre, fm, cm, d, o, touch)
        .tight
        .fold(rt)
}

/// One piece of a level [`gn_certify_span`] proved: its parameter range, its
/// [`gn_fourth_over_series`] terms, and its series where the span's rows vary (the ball's
/// circle, or a varying thumbweight: their row terms are taken from them); and, where
/// taken, the tighter terms of the feet's Taylor models (#14405). Not upstream.
#[derive(Clone, Debug, PartialEq)]
pub struct CertifiedPiece {
    /// The piece's first and last parameter.
    pub s: [f64; 2],
    /// The piece's fourth s-derivative terms ([`Fourth`]) from the interval enclosure (the
    /// fork's): they steer the levels.
    pub fourth: Fourth,
    /// The piece's series from the interval enclosure, kept only where the span's rows vary.
    pub series: Option<Box<PieceSeries>>,
    /// The feet's Taylor models' terms met with the interval enclosure's, where taken.
    pub tight: Option<Box<TightPiece>>,
    /// What taking them needs, until they are taken (`None`: taken, or none to take).
    pub inputs: Option<Box<PieceInputs>>,
}

/// A piece's terms from the feet's Taylor models over it
/// ([`super::feet_tm::gn_enclose_tm`]) met with the interval enclosure's, term by term
/// (#14405): each holds over the piece, so the tighter one does.
#[derive(Clone, Debug, PartialEq)]
pub struct TightPiece {
    /// The fourth s-derivative terms, each no larger than the interval enclosure's.
    pub fourth: Fourth,
    /// The series, kept only where the span's rows vary: each coefficient's enclosure
    /// within the interval enclosure's.
    pub series: Option<PieceSeries>,
}

/// What the feet's Taylor models over a piece start from: the piece as the interval
/// enclosure takes it ([`SpanAt`]), its middle and the feet's point enclosure there.
#[derive(Clone, Debug, PartialEq)]
pub struct PieceInputs {
    /// The piece.
    pub at: SpanAt,
    /// Its middle.
    pub mid: f64,
    /// The feet's point enclosure at the middle.
    pub e4: Enclosure,
}

/// What a piece's feet Taylor models take from its span: the faces, the leader, the
/// offsets and sides, the jets graded and the series' order, the radius law, whether the
/// span's rows vary, and whether they are taken at all (#14405).
struct FeetCtx<'a, 'l> {
    k: &'a Kernel,
    faces_n: [&'a Nurbs; 2],
    leader_n: &'a NurbsCurve,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    order: usize,
    law: Option<&'a GnLaw<'l>>,
    rows_vary: bool,
    /// Whether the feet's Taylor models are taken at all (NURBS faces, not a blend's
    /// definition, and a thumbweight that does not vary).
    on: bool,
}

/// The lesser of two upper bounds (a NaN is no bound).
fn upper_meet(a: f64, b: f64) -> f64 {
    if a.is_nan() || b < a {
        b
    } else {
        a
    }
}

/// Two enclosures' [`Fourth`] terms of the same piece met: the lesser bound of each, the
/// least values `tm`'s (both are the middle's).
fn meet_fourth(old: &Fourth, tm: &Fourth) -> Fourth {
    let (m4o, uv4o, _, _, uvfo, _) = old;
    let (m4t, uv4t, m4_lo, uv4_lo, uvft, uv4_lo_fork) = tm;
    let mut m4 = *m4t;
    for (mf, of) in m4.iter_mut().zip(m4o) {
        for (m, &o) in mf.iter_mut().zip(of) {
            *m = upper_meet(o, *m);
        }
    }
    let mut uv4 = *uv4t;
    for (u, &o) in uv4.iter_mut().zip(uv4o) {
        *u = upper_meet(o, *u);
    }
    let mut uvf = *uvft;
    for (u, &o) in uvf.iter_mut().zip(uvfo) {
        *u = upper_meet(o, *u);
    }
    (m4, uv4, *m4_lo, *uv4_lo, uvf, *uv4_lo_fork)
}

/// Two enclosures of the same series met coefficient by coefficient into `a`; `false`
/// where one is empty (they do not both hold the same function).
fn meet_vser(a: &mut VSer<Ivl>, b: &VSer<Ivl>) -> bool {
    for (sa, sb) in a.a.iter_mut().zip(&b.a) {
        for (ra, rb) in sa.c.iter_mut().zip(&sb.c) {
            for (x, y) in ra.iter_mut().zip(rb) {
                let (lo, hi) = (cmax(x.lo, y.lo), cmin(x.hi, y.hi));
                let common = lo <= hi;
                if !common {
                    return false;
                }
                *x = Ivl::new(lo, hi);
            }
        }
    }
    true
}

/// Two enclosures' varying-row terms of the same piece met: the lesser `X`, `Y`, `R`, the
/// greater least `W`, and the thumbweights' enclosures' common part.
fn meet_rows(a: &GnRowTerms, b: &GnRowTerms) -> GnRowTerms {
    let mut m = a.clone();
    for (x, &y) in m.x.iter_mut().zip(&b.x) {
        *x = upper_meet(*x, y);
    }
    for (x, &y) in m.y.iter_mut().zip(&b.y) {
        *x = upper_meet(*x, y);
    }
    for (x, &y) in m.w_lo.iter_mut().zip(&b.w_lo) {
        if x.is_nan() || y > *x {
            *x = y;
        }
    }
    m.r = upper_meet(m.r, b.r);
    let ([alo, ahi], [blo, bhi]) = (a.w, b.w);
    let (lo, hi) = (cmax(alo, blo), cmin(ahi, bhi));
    if lo <= hi {
        m.w = [lo, hi];
    }
    m
}

/// Takes piece `p`'s feet Taylor models ([`super::feet_tm::gn_enclose_tm`]) where they are
/// still to take: its terms from them met with the interval enclosure's ([`TightPiece`]).
/// Where they are not proved, or a series' enclosure misses the interval one's (which
/// cannot be for the same feet), the piece keeps its interval terms; either way there is
/// nothing more to take.
fn take_tight(p: &mut CertifiedPiece, fc: &FeetCtx<'_, '_>) -> Result<()> {
    let Some(inp) = p.inputs.take() else {
        return Ok(());
    };
    let Some(e5) = super::feet_tm::gn_enclose_tm(
        fc.k,
        fc.faces_n,
        fc.leader_n,
        &inp.at,
        fc.r,
        fc.side,
        fc.n,
        fc.order,
        fc.law,
        inp.mid,
        &inp.e4.x,
    )?
    else {
        return Ok(());
    };
    let [s0, s1] = p.s;
    let (fourth_tm, series_tm) = fourth_of(&e5, &inp.e4, s1 - s0, fc.n, inp.mid);
    let series = if fc.rows_vary {
        let Some(mut t) = series_tm else {
            return Ok(());
        };
        if let Some(o) = p.series.as_deref() {
            if !(meet_vser(&mut t.f[0], &o.f[0])
                && meet_vser(&mut t.f[1], &o.f[1])
                && meet_vser(&mut t.v, &o.v))
            {
                return Ok(());
            }
        }
        Some(t)
    } else {
        None
    };
    p.tight = Some(Box::new(TightPiece {
        fourth: meet_fourth(&p.fourth, &fourth_tm),
        series,
    }));
    Ok(())
}

/// A span's finest level [`gn_certify_span_reusing`] proved: its pieces, in order, cover
/// the span. Not upstream: the fork certifies each half of a halved span afresh.
#[derive(Clone, Debug, PartialEq)]
pub struct CertifiedLevel {
    /// The level: the span in `2^level` pieces.
    pub level: u32,
    /// The pieces, in order of s.
    pub pieces: Vec<CertifiedPiece>,
}

/// The pieces of `level` that meet `[a, b]`, when they cover it without a gap (each one
/// starts where the one before ends, the first at or before `a`, the last at or after
/// `b`); `None` otherwise.
fn covering(level: &CertifiedLevel, a: f64, b: f64) -> Option<Vec<&CertifiedPiece>> {
    let meet: Vec<&CertifiedPiece> = level
        .pieces
        .iter()
        .filter(|p| p.s[1] > a && p.s[0] < b)
        .collect();
    let (first, last) = (meet.first()?, meet.last()?);
    let joined = meet.windows(2).all(|w| match w {
        [p, q] => p.s[1] == q.s[0],
        _ => false,
    });
    let covers = joined && first.s[0] <= a && last.s[1] >= b;
    covers.then_some(meet)
}

/// What [`gn_certify_span_reusing`] knows of a span for every level: its length, the jets
/// graded on their own, whether it is the ball's circle, whether its rows vary, whether
/// the ball's radius varies (the touch circle's), and the target.
struct LevelCtx {
    h: f64,
    l_jets: usize,
    arc: bool,
    rows_vary: bool,
    touch: bool,
    target: f64,
    halving: Halving,
}

/// Whether a proved level over the target goes on to a finer one only where it halved the
/// last level's bound ([`SpanReuse::to_target`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Halving {
    /// Only where it halved the bound.
    Steers,
    /// While over the target.
    Ignored,
}

/// One piece's row terms as a level holds them ([`absorb_piece`]): its varying rows' terms
/// and the arc's, from the series the level holds for the piece (the feet's Taylor models'
/// where taken, else the interval enclosure's). The arc's are the interval series' alone
/// (`tight` false) where those prove the piece, until the level needs the tighter ones
/// ([`tighten_level`]). A piece whose feet's Taylor models are still to take before it is
/// proved at all (`pending`) holds its interval rows' terms until they are taken.
struct PieceRows {
    vary: Option<GnRowTerms>,
    arc: Option<ArcTerms>,
    tight: bool,
    pending: bool,
}

/// A level's terms, folded in piece by piece ([`absorb_piece`]): the jets' and the uv
/// curves' largest fourth derivatives (per parameter and the fork's), their least values
/// at the pieces' middles, the varying rows' terms, and whether every piece so far is
/// proved. The certifying terms (`m4`, `uv4`, `uv4_fork`, `rows`, `proved`) are each
/// piece's as the level holds it; the interval enclosure's and the fork's interval row
/// terms (`m4_old`, `uv4_old`, `uv4_fork_old`, `rows_old`, `proved_old`, and the least
/// values) steer the levels (#14405). A piece holds its tighter terms (the arc's Taylor
/// models, the feet's) where the interval ones do not prove it, and the interval ones
/// otherwise until the level is tightened ([`Lazy`]). The floor terms (`m4_floor`,
/// `uv4_floor`) are below every piece's certifying terms, whichever it takes
/// ([`piece_floor`]).
struct LevelAcc {
    m4: [[f64; MAX_E + 1]; 2],
    uv4: [f64; 2],
    uv4_fork: [f64; 2],
    m4_old: [[f64; MAX_E + 1]; 2],
    uv4_old: [f64; 2],
    uv4_fork_old: [f64; 2],
    m4_lo: [[f64; MAX_E + 1]; 2],
    uv4_lo: [f64; 2],
    uv4_lo_fork: [f64; 2],
    m4_floor: [[f64; MAX_E + 1]; 2],
    uv4_floor: [f64; 2],
    proved: bool,
    rows: GnRowTerms,
    proved_old: bool,
    rows_old: GnRowTerms,
    pieces: Vec<PieceRows>,
    lazy: Lazy,
}

/// What a level's pieces have still to take (#14405): an arc's Taylor models for a piece
/// that holds its interval arc terms alone (`arc`, [`tighten_level`]), the feet's for a
/// piece the interval enclosure proves (`feet`, [`tighten_feet`]), and the feet's for a
/// piece it does not, before the level is proved at all (`pending`, [`resolve_pending`]).
#[derive(Clone, Copy, Default)]
struct Lazy {
    arc: bool,
    feet: bool,
    pending: bool,
}

/// No row terms folded in, for `ld` rows.
fn no_rows(ld: usize) -> GnRowTerms {
    GnRowTerms {
        x: vec![0.0; ld],
        y: vec![0.0; ld],
        w_lo: vec![f64::INFINITY; ld],
        r: 0.0,
        w: [f64::INFINITY, f64::NEG_INFINITY],
    }
}

impl LevelAcc {
    /// No piece folded in, for `ld` rows.
    fn new(ld: usize) -> Self {
        let rows = no_rows(ld);
        Self {
            m4: [[0.0_f64; MAX_E + 1]; 2],
            uv4: [0.0_f64; 2],
            uv4_fork: [0.0_f64; 2],
            m4_old: [[0.0_f64; MAX_E + 1]; 2],
            uv4_old: [0.0_f64; 2],
            uv4_fork_old: [0.0_f64; 2],
            m4_lo: [[0.0_f64; MAX_E + 1]; 2],
            uv4_lo: [0.0_f64; 2],
            uv4_lo_fork: [0.0_f64; 2],
            m4_floor: [[0.0_f64; MAX_E + 1]; 2],
            uv4_floor: [0.0_f64; 2],
            proved: true,
            proved_old: true,
            rows_old: rows.clone(),
            rows,
            pieces: Vec::new(),
            lazy: Lazy::default(),
        }
    }
}

/// Folds one piece's varying-row terms `p` (folded into fresh terms) into `rt`: largest
/// X, Y and R, least W, and the union of the thumbweights.
fn fold_rows(rt: &mut GnRowTerms, p: &GnRowTerms) {
    for (a, &b) in rt.x.iter_mut().zip(&p.x) {
        *a = cmax(*a, b);
    }
    for (a, &b) in rt.y.iter_mut().zip(&p.y) {
        *a = cmax(*a, b);
    }
    for (a, &b) in rt.w_lo.iter_mut().zip(&p.w_lo) {
        *a = cmin(*a, b);
    }
    rt.r = cmax(rt.r, p.r);
    let ([lo, hi], [plo, phi]) = (rt.w, p.w);
    rt.w = [cmin(lo, plo), cmax(hi, phi)];
}

/// Folds a piece's feet terms `f` into the largest `m4` (its first `l_jets + 1` jets),
/// `uv4` and `uv4_fork`; `false` where one is not finite (not proved).
fn fold_feet(
    m4: &mut [[f64; MAX_E + 1]; 2],
    uv4: &mut [f64; 2],
    uv4_fork: &mut [f64; 2],
    f: &Fourth,
    l_jets: usize,
) -> bool {
    let (pm, puv, _, _, puv_fork, _) = f;
    let mut finite = true;
    for (((m4f, pmf), uvf), &puvf) in m4.iter_mut().zip(pm).zip(uv4.iter_mut()).zip(puv) {
        for (m4fk, &pmfk) in m4f.iter_mut().zip(pmf).take(l_jets + 1) {
            finite &= pmfk < f64::INFINITY;
            *m4fk = cmax(*m4fk, pmfk);
        }
        finite &= puvf < f64::INFINITY;
        *uvf = cmax(*uvf, puvf);
    }
    for (u, &pu) in uv4_fork.iter_mut().zip(puv_fork) {
        *u = cmax(*u, pu);
    }
    finite
}

/// What a span's levels share: its stations, the varying thumbweight's fixed form and the
/// point O the rows are taken about, what it knows for every level, and what its pieces'
/// feet Taylor models take.
struct SpanCtx<'s, 'a, 'l> {
    a: &'s GnStation,
    b: &'s GnStation,
    bar: &'s GnSectionFormT<Ivl>,
    o: [f64; 3],
    level: &'s LevelCtx,
    feet: &'s FeetCtx<'a, 'l>,
}

/// The varying rows' terms of the piece `[s0, s1]` from its series `ps` (`None` where not
/// proved), about the span's first station.
fn vary_terms(
    k: &Kernel,
    v: &mut GnVarying,
    sc: &SpanCtx<'_, '_, '_>,
    s01: [f64; 2],
    ps: &PieceSeries,
    ld: usize,
) -> Option<GnRowTerms> {
    // the root's guess at the piece's middle: the stations' thumbweights, linearly
    let t = (ps.mid - sc.a.s) / sc.level.h;
    let d = Ivl::new(s01[0], s01[1]) - Ivl::from(ps.mid);
    let guess = (1.0 - t) * sc.a.w + t * sc.b.w;
    let mut terms = no_rows(ld);
    let proved = gn_row_terms_over(
        k, v, &ps.f, &ps.v, &ps.fm, &ps.vm, d, guess, sc.bar, sc.o, &mut terms,
    );
    proved.then_some(terms)
}

/// The rows a piece holds from its feet Taylor models' series `ps` (the piece's tighter
/// terms, met with those it held: `held`): the varying rows' and the arc's, both ways.
#[allow(clippy::too_many_arguments)]
fn tight_rows(
    k: &Kernel,
    vary: Option<&mut GnVarying>,
    sc: &SpanCtx<'_, '_, '_>,
    s01: [f64; 2],
    ps: &PieceSeries,
    ld: usize,
    held: Option<&PieceRows>,
) -> Option<PieceRows> {
    let mut pr = PieceRows {
        vary: None,
        arc: None,
        tight: true,
        pending: false,
    };
    if let Some(v) = vary {
        let t = vary_terms(k, v, sc, s01, ps, ld);
        let h = held.and_then(|h| h.vary.as_ref());
        pr.vary = match (t, h) {
            (Some(t), Some(h)) => Some(meet_rows(&t, h)),
            (Some(t), None) => Some(t),
            (None, Some(h)) => Some(h.clone()),
            (None, None) => return None,
        };
    }
    if sc.level.arc {
        let d = Ivl::new(s01[0], s01[1]) - Ivl::from(ps.mid);
        let t = arc_terms_over(&ps.f, &ps.v, &ps.fm, &ps.vm, d, sc.o, sc.level.touch).tight;
        let t = match held.and_then(|h| h.arc) {
            Some(h) => h.meet(t),
            None => t,
        };
        if !t.proves() {
            return None;
        }
        pr.arc = Some(t);
    }
    Some(pr)
}

/// The jets' and the uv curves' part of a level's bound (as [`close_level`] takes it) from
/// the terms `m4` and `uv`: the varying rows' and the arc's terms only raise the bound.
fn jets_uv_bound(ctx: &LevelCtx, m4: &[[f64; MAX_E + 1]; 2], uv: &[f64; 2]) -> f64 {
    let h = ctx.h;
    let k4 = h * h * h * h / 384.0;
    let mut bound = 0.0_f64;
    for (m4f, &uvf) in m4.iter().zip(uv) {
        let mut sum = 0.0_f64;
        for &m in m4f.iter().take(ctx.l_jets + 1) {
            sum += k4 * m;
        }
        bound = cmax(bound, cmax(if ctx.rows_vary { 0.0 } else { sum }, k4 * uvf));
    }
    bound
}

/// The floor of a piece's jets' and uv curves' fourth terms from its middle's point
/// enclosure `e4` (the piece of length `h`, `n` jets): [`fourth_of`]'s bounds without the
/// remainder over the piece, which is the only part an enclosure over the piece adds (the
/// interval one's and the Taylor models' alike, each with the same `e4`), and the faces'
/// least partials at the middle for their partials over the piece. Every piece's terms are
/// at least these, whichever enclosure it takes.
fn floor_terms(e4: &Enclosure, h: f64, n: usize) -> ([[f64; MAX_E + 1]; 2], [f64; 2]) {
    let mut m4 = [[0.0_f64; MAX_E + 1]; 2];
    let mut uv = [0.0_f64; 2];
    for (f, (uvf, dsl)) in uv.iter_mut().zip(&e4.ds_lo).enumerate() {
        let mut sum = 0.0_f64;
        for (c, &dsc) in ((2 * f)..(2 * f + 2)).zip(dsl) {
            let Some(x4) = e4.x.get(c) else {
                return ([[0.0_f64; MAX_E + 1]; 2], [0.0_f64; 2]);
            };
            sum += taylor_bound(|i| x4.at(i, 0).mag(), 0.0, h) * dsc;
        }
        *uvf = sum;
    }
    for (m4f, f4) in m4.iter_mut().zip(&e4.f) {
        for (kk, m4fk) in m4f.iter_mut().enumerate().take(n + 1) {
            let mut sq = 0.0_f64;
            for s4 in &f4.a {
                let m = taylor_bound(|i| s4.at(i, kk).mag(), 0.0, h);
                sq += m * m;
            }
            *m4fk = crate::geom::vector3d::sqrt(sq);
        }
    }
    (m4, uv)
}

/// Folds piece `p`'s floor terms into the level's (#14405): its terms as held where they
/// are final (its feet's Taylor models taken, or none to take), else [`floor_terms`] from
/// its middle's point enclosure.
fn piece_floor(acc: &mut LevelAcc, p: &CertifiedPiece, l_jets: usize) {
    let (m4, uv) = if let Some(inp) = p.inputs.as_deref() {
        let [s0, s1] = p.s;
        floor_terms(&inp.e4, s1 - s0, l_jets)
    } else {
        let (m4, uv, _, _, _, _) = p.tight.as_deref().map_or(&p.fourth, |t| &t.fourth);
        (*m4, *uv)
    };
    for (af, pf) in acc.m4_floor.iter_mut().zip(&m4) {
        for (a, &v) in af.iter_mut().zip(pf).take(l_jets + 1) {
            *a = cmax(*a, v);
        }
    }
    for (a, &v) in acc.uv4_floor.iter_mut().zip(&uv) {
        *a = cmax(*a, v);
    }
}

/// A lower bound on the level's certified bound whatever its pieces take (#14405): the
/// span's least values (`lb`, [`piece_least`]) and the level's floor terms'. Over the
/// target, the feet's Taylor models cannot bring the level to it, and are not taken.
fn gate(acc: &LevelAcc, ctx: &LevelCtx, lb: f64) -> f64 {
    let floor = jets_uv_bound(ctx, &acc.m4_floor, &acc.uv4_floor);
    cmax(lb, round_down(floor * (1.0 - 1.0e-12)))
}

/// A lower bound on every level's certified bound for the span, from piece `p`'s middle
/// (#14405): every piece holding the middle bounds the jets', the feet's and the arc rows'
/// fourth derivatives there from above, so each level's bound is at least the bound of
/// their least values there (the varying thumbweight's rows are not counted). It decides
/// only whether the feet's Taylor models are taken: over the target, no terms certify the
/// span, and taking them could not change its verdict.
fn piece_least(p: &CertifiedPiece, sc: &SpanCtx<'_, '_, '_>) -> f64 {
    let ctx = sc.level;
    let h = ctx.h;
    let k4 = h * h * h * h / 384.0;
    // the least values from the middle's point enclosure (the fourth terms' least parts)
    let from_e4 = p.inputs.as_deref().map(|inp| {
        let [s0, s1] = p.s;
        fourth_of(&inp.e4, &inp.e4, s1 - s0, ctx.l_jets, inp.mid)
    });
    let (fourth, series) = match (&from_e4, p.tight.as_deref()) {
        (Some((f, s)), _) => (f, s.as_ref()),
        (None, Some(t)) => (&t.fourth, t.series.as_ref()),
        (None, None) => (&p.fourth, p.series.as_deref()),
    };
    let (_, _, m4_lo, uv4_lo, _, _) = fourth;
    let mut lb = jets_uv_bound(ctx, m4_lo, uv4_lo);
    if ctx.arc {
        if let Some([xl, yl, wh]) = series.and_then(|ps| arc_least_at(ps, sc.o, ctx.touch)) {
            for ((&x, &y), &w) in xl.iter().zip(&yl).zip(&wh) {
                // den = w_lo - kh y is at most w_hi - kh y_lo, num at least kh x_lo
                let den = round_up(w - k4 * y);
                if den > 0.0 {
                    lb = cmax(lb, round_down(k4 * x / den));
                }
            }
        }
    }
    round_down(lb * (1.0 - 1.0e-12))
}

/// Folds piece `p` of the span into `acc`: its interval terms into the steering ones
/// (their least values at its middle only where `steer`: the middle lies in the span), and
/// the terms it holds into the certifying ones. It holds the interval terms where they
/// prove it (the arc's Taylor models at once where only those prove its rows, #14405), or
/// its feet's Taylor models' where already taken; otherwise it is pending (its feet's
/// Taylor models taken once the level's pieces are in, [`resolve_pending`]) where those can
/// still bring the level to the target ([`gate`]), and not proved where not. Its floor
/// terms go into the level's ([`piece_floor`]).
fn absorb_piece(
    k: &Kernel,
    acc: &mut LevelAcc,
    p: &mut CertifiedPiece,
    steer: bool,
    sc: &SpanCtx<'_, '_, '_>,
    vary: Option<&mut GnVarying>,
    lb: &mut f64,
) {
    let mut vary = vary;
    if steer && sc.feet.on {
        *lb = cmax(*lb, piece_least(p, sc));
    }
    let ctx = sc.level;
    let ld = acc.rows.x.len();
    // the interval enclosure's terms: they steer
    let old_feet = fold_feet(
        &mut acc.m4_old,
        &mut acc.uv4_old,
        &mut acc.uv4_fork_old,
        &p.fourth,
        ctx.l_jets,
    );
    let (_, _, pm_lo, puv_lo, _, puv_lo_fork) = &p.fourth;
    if steer {
        for (lf, plf) in acc.m4_lo.iter_mut().zip(pm_lo) {
            for (l, &pl) in lf.iter_mut().zip(plf) {
                *l = cmax(*l, pl);
            }
        }
        for (l, &pl) in acc.uv4_lo.iter_mut().zip(puv_lo) {
            *l = cmax(*l, pl);
        }
        for (l, &pl) in acc.uv4_lo_fork.iter_mut().zip(puv_lo_fork) {
            *l = cmax(*l, pl);
        }
    }
    let ps_old = p.series.as_deref().filter(|_| old_feet);
    let vary_old: Option<GnRowTerms> = match (vary.as_deref_mut(), ps_old) {
        (Some(v), Some(ps)) => vary_terms(k, v, sc, p.s, ps, ld),
        _ => None,
    };
    let arc_old = match ps_old {
        Some(ps) if ctx.arc => {
            let d = Ivl::new(p.s[0], p.s[1]) - Ivl::from(ps.mid);
            Some(arc_terms_interval(
                &ps.f, &ps.v, &ps.fm, &ps.vm, d, sc.o, ctx.touch,
            ))
        }
        _ => None,
    };
    let mut old_ok = old_feet;
    if vary.is_some() {
        match &vary_old {
            Some(t) => fold_rows(&mut acc.rows_old, t),
            None => old_ok = false,
        }
    }
    if ctx.arc {
        old_ok = old_ok && arc_old.is_some_and(|t| t.fold(&mut acc.rows_old));
    }
    if !old_ok {
        acc.proved_old = false;
    }
    // the terms it holds: the interval ones where they prove it, else the tighter ones
    let mut held: Option<PieceRows> = None;
    let prev = old_feet.then(|| PieceRows {
        vary: vary_old.clone(),
        arc: arc_old,
        tight: false,
        pending: false,
    });
    if p.tight.is_none() && old_feet {
        let mut pr = PieceRows {
            vary: None,
            arc: None,
            tight: false,
            pending: false,
        };
        let mut ok = vary.is_none() || vary_old.is_some();
        pr.vary = vary_old;
        if ok && ctx.arc {
            match (arc_old, ps_old) {
                (Some(t), _) if t.proves() => pr.arc = Some(t),
                (Some(t), Some(ps)) => {
                    // the arc's Taylor models where only they prove its rows
                    let d = Ivl::new(p.s[0], p.s[1]) - Ivl::from(ps.mid);
                    let tt = t.meet(arc_terms_taylor(
                        &ps.f, &ps.v, &ps.fm, &ps.vm, d, sc.o, ctx.touch,
                    ));
                    pr.tight = true;
                    ok = tt.proves();
                    pr.arc = Some(tt);
                }
                _ => ok = false,
            }
        }
        if ok {
            held = Some(pr);
        }
    }
    piece_floor(acc, p, ctx.l_jets);
    if held.is_none() && p.tight.is_some() {
        // its feet's Taylor models already taken (a halved span's piece): their rows
        held = held_tight(k, vary, sc, p, ld, prev.as_ref());
    }
    if held.is_none()
        && p.tight.is_none()
        && p.inputs.is_some()
        && gate(acc, ctx, *lb) <= ctx.target
    {
        // the interval terms do not prove it: its feet's Taylor models, once the level's
        // other pieces are in, where they can still bring it to the target
        let (vary, arc) = prev.map_or((None, None), |pr| (pr.vary, pr.arc));
        acc.pieces.push(PieceRows {
            vary,
            arc,
            tight: true,
            pending: true,
        });
        acc.lazy.pending = true;
        return;
    }
    let Some(pr) = held else {
        acc.proved = false;
        acc.proved_old = false;
        acc.pieces.push(PieceRows {
            vary: None,
            arc: None,
            tight: true,
            pending: false,
        });
        return;
    };
    let fourth = p.tight.as_deref().map_or(&p.fourth, |t| &t.fourth);
    acc.proved &= fold_feet(
        &mut acc.m4,
        &mut acc.uv4,
        &mut acc.uv4_fork,
        fourth,
        ctx.l_jets,
    );
    if let Some(t) = &pr.vary {
        fold_rows(&mut acc.rows, t);
    }
    if let Some(t) = &pr.arc {
        acc.proved = acc.proved && t.fold(&mut acc.rows);
        if !pr.tight {
            acc.lazy.arc = true;
        }
    }
    if p.tight.is_none() && p.inputs.is_some() {
        acc.lazy.feet = true;
    }
    if !acc.proved {
        acc.proved_old = false;
    }
    acc.pieces.push(pr);
    if acc.lazy.pending && gate(acc, ctx, *lb) > ctx.target {
        // over the target whatever the pending pieces take: not proved, as the interval
        // terms leave it
        acc.proved = false;
    }
}

/// The rows piece `p` holds from its feet's Taylor models where taken ([`tight_rows`], met
/// with `held`); with rows that do not vary, none to take. `None` where not proved.
fn held_tight(
    k: &Kernel,
    vary: Option<&mut GnVarying>,
    sc: &SpanCtx<'_, '_, '_>,
    p: &CertifiedPiece,
    ld: usize,
    held: Option<&PieceRows>,
) -> Option<PieceRows> {
    let t = p.tight.as_deref()?;
    match &t.series {
        Some(ps) => tight_rows(k, vary, sc, p.s, ps, ld, held),
        None if !sc.level.rows_vary => Some(PieceRows {
            vary: None,
            arc: None,
            tight: true,
            pending: false,
        }),
        None => None,
    }
}

/// Takes the feet's Taylor models of the level's pending pieces ([`absorb_piece`]) where
/// the level's floor ([`gate`]) is at most the target, their rows from them met with the
/// interval ones they held, and folds the level's certifying terms again. A pending piece
/// they do not prove leaves the level unproved, as does a floor over the target (as the
/// interval terms leave it). `pieces`: the level's, in the order they were folded in.
///
/// # Errors
///
/// A cancel.
fn resolve_pending(
    k: &Kernel,
    acc: &mut LevelAcc,
    pieces: &mut [CertifiedPiece],
    sc: &SpanCtx<'_, '_, '_>,
    vary: Option<&mut GnVarying>,
    lb: f64,
) -> Result<()> {
    acc.lazy.pending = false;
    if gate(acc, sc.level, lb) > sc.level.target {
        acc.proved = false;
        acc.proved_old = false;
        return Ok(());
    }
    let mut vary = vary;
    let ld = acc.rows.x.len();
    for (held, p) in acc.pieces.iter_mut().zip(pieces.iter_mut()) {
        if !held.pending {
            continue;
        }
        k.check_cancelled(OP_CERTIFY_SPAN)?;
        take_tight(p, sc.feet)?;
        let Some(pr) = held_tight(k, vary.as_deref_mut(), sc, p, ld, Some(held)) else {
            acc.proved = false;
            acc.proved_old = false;
            return Ok(());
        };
        *held = pr;
    }
    refold(acc, pieces, sc.level.l_jets);
    Ok(())
}

/// Folds the level's certifying terms again from what its pieces hold (`pieces`: the
/// level's, in the order they were folded in).
fn refold(acc: &mut LevelAcc, pieces: &[CertifiedPiece], l_jets: usize) {
    let mut m4 = [[0.0_f64; MAX_E + 1]; 2];
    let (mut uv4, mut uv4_fork) = ([0.0_f64; 2], [0.0_f64; 2]);
    let mut rows = no_rows(acc.rows.x.len());
    let mut proved = true;
    for (held, p) in acc.pieces.iter().zip(pieces) {
        let fourth = p.tight.as_deref().map_or(&p.fourth, |t| &t.fourth);
        proved &= fold_feet(&mut m4, &mut uv4, &mut uv4_fork, fourth, l_jets);
        if let Some(v) = &held.vary {
            fold_rows(&mut rows, v);
        }
        if let Some(arc) = &held.arc {
            proved = proved && arc.fold(&mut rows);
        }
    }
    acc.m4 = m4;
    acc.uv4 = uv4;
    acc.uv4_fork = uv4_fork;
    acc.rows = rows;
    if !proved {
        // not reached (the tighter terms prove what the held ones did); a level whose
        // terms do not hold certifies nothing
        acc.proved = false;
        acc.proved_old = false;
    }
}

/// Takes the arc's tighter terms ([`arc_terms_over`]'s, #14405) for every piece of the
/// level whose interval terms were held alone, and folds the level's certifying terms
/// again from its pieces': as though each piece had taken its tighter terms at once. A
/// piece proved stays proved (the tighter terms are never looser). `pieces`: the level's,
/// in the order they were folded in.
fn tighten_level(acc: &mut LevelAcc, pieces: &[CertifiedPiece], ctx: &LevelCtx, o: [f64; 3]) {
    for (held, p) in acc.pieces.iter_mut().zip(pieces) {
        if let Some(arc) = held.arc.as_mut() {
            if !held.tight {
                let ps = match p.tight.as_deref() {
                    Some(t) => t.series.as_ref(),
                    None => p.series.as_deref(),
                };
                if let Some(ps) = ps {
                    let d = Ivl::new(p.s[0], p.s[1]) - Ivl::from(ps.mid);
                    *arc = arc.meet(arc_terms_taylor(
                        &ps.f, &ps.v, &ps.fm, &ps.vm, d, o, ctx.touch,
                    ));
                }
                held.tight = true;
            }
        }
    }
    refold(acc, pieces, ctx.l_jets);
    acc.lazy.arc = false;
}

/// Takes the feet's Taylor models ([`take_tight`], #14405) for every piece of the level
/// that holds the interval enclosure's terms, its rows' terms from their series met with
/// those it held, and folds the level's certifying terms again: as though each piece had
/// taken them at once. A piece proved stays proved (its terms are never looser).
///
/// # Errors
///
/// A cancel.
fn tighten_feet(
    k: &Kernel,
    acc: &mut LevelAcc,
    pieces: &mut [CertifiedPiece],
    sc: &SpanCtx<'_, '_, '_>,
    vary: Option<&mut GnVarying>,
) -> Result<()> {
    let mut vary = vary;
    let ld = acc.rows.x.len();
    for (held, p) in acc.pieces.iter_mut().zip(pieces.iter_mut()) {
        if p.tight.is_some() || p.inputs.is_none() {
            continue;
        }
        k.check_cancelled(OP_CERTIFY_SPAN)?;
        take_tight(p, sc.feet)?;
        if let Some(ps) = p.tight.as_deref().and_then(|t| t.series.as_ref()) {
            if let Some(pr) = tight_rows(k, vary.as_deref_mut(), sc, p.s, ps, ld, Some(held)) {
                *held = pr;
            }
        }
    }
    refold(acc, pieces, sc.level.l_jets);
    acc.lazy.feet = false;
    Ok(())
}

/// What a level tells [`gn_certify_span_reusing`]: go on to the next level, stop there, or
/// skip this level (its bound by the fork's term is not finite).
enum Flow {
    Next,
    Stop,
    Skip,
}

/// Closes a level whose pieces are all proved by the certificate's terms (`acc.proved`):
/// its bound folded into `bound_out` (and the thumbweight over the span into `vary` where
/// it lowers that), and whether a finer level goes on ([`gn_certify_span`]); with the
/// level certified (its bound finite). `pieces`: the level's, as folded into `acc`.
///
/// The bound is first the one of the pieces' terms as held (the interval ones where they
/// prove a piece); where that is not at the target, the level is tightened, the arc's
/// rows first ([`tighten_level`]: their Taylor models), then the feet ([`tighten_feet`]:
/// the feet's Taylor models, and the rows from their series), each followed by the bound
/// again. A level the held terms certify the tighter ones certify (their bound is no
/// larger), so the level's verdict, and `bound_out` wherever it is over the target, are
/// those of every piece's tightest terms; the Taylor models are only taken where they can
/// change it.
///
/// The levels are steered as before #14405, by the interval enclosure's terms and the
/// fork's interval row terms (`acc.m4_old`, `acc.uv4_old`, `acc.uv4_fork_old`,
/// `acc.rows_old`): a level they do not prove goes on to the next one, the fork's term's
/// bound not finite skips it, and the halving that decides whether a finer level is worth
/// its cost compares their bounds. Only the stop at the target reads the certificate's
/// bound, which is never over theirs (each term is the tighter of the two, or theirs): so
/// a span the interval terms certify from a given level, these certify at the same level
/// or a coarser one, and a span takes no more levels than it did.
///
/// # Errors
///
/// A cancel while taking the feet's Taylor models.
#[allow(clippy::too_many_arguments)]
fn close_level(
    k: &Kernel,
    acc: &mut LevelAcc,
    pieces: &mut [CertifiedPiece],
    sc: &SpanCtx<'_, '_, '_>,
    bound_out: &mut f64,
    last: &mut f64,
    mut vary: Option<&mut GnVarying>,
    lb: f64,
) -> Result<(Flow, bool)> {
    let (ctx, bar, o) = (sc.level, sc.bar, sc.o);
    let (h, l_jets, arc, rows_vary) = (ctx.h, ctx.l_jets, ctx.arc, ctx.rows_vary);
    // the level's bound with the varying rows' terms `rows` and the uv curves' term `uv`
    // (#2690: taken with each parameter's own term, `uv4`, to certify, and with the
    // fork's, `uv4_fork`, to steer the levels as the fork does, so that no span takes more
    // levels or pieces than there)
    let level_bound = |m4: &[[f64; MAX_E + 1]; 2], rows: &GnRowTerms, uv: &[f64; 2]| -> f64 {
        let mut bound = 0.0_f64;
        let mut da = 0.0_f64;
        for (m4f, &uvf) in m4.iter().zip(uv) {
            let mut sum = 0.0_f64;
            for &m4fk in m4f.iter().take(l_jets + 1) {
                sum += h * h * h * h / 384.0 * m4fk;
            }
            da = cmax(da, sum);
            bound = cmax(
                bound,
                cmax(
                    if rows_vary { 0.0 } else { sum },
                    h * h * h * h / 384.0 * uvf,
                ),
            );
        }
        // the arc: no fixed section, every row's remainder (the varying rows' bound with
        // A = 0)
        if arc {
            da = 0.0;
        }
        if rows_vary {
            let kh = h * h * h * h / 384.0;
            for (j, wj) in bar.w.iter().enumerate() {
                let (Some(&lj), Some(&yj), Some(&xj)) =
                    (rows.w_lo.get(j), rows.y.get(j), rows.x.get(j))
                else {
                    break;
                };
                let den = lj - kh * yj;
                let num = wj.hi * da + kh * xj + kh * yj * rows.r;
                let positive = den > 0.0;
                bound = if positive {
                    cmax(bound, num / den)
                } else {
                    f64::INFINITY
                };
                if !positive {
                    break;
                }
            }
        }
        bound
    };
    // the certificate's bound (#14405): with the pieces' terms as held, and where that is
    // not at the target, with every piece's tighter terms (the bound and the level's
    // verdict then as though every piece had taken them at once)
    let mut certified = false;
    loop {
        if level_bound(&acc.m4, &acc.rows, &acc.uv4_fork).is_finite() {
            certified = true;
            // certified by each parameter's own term, never larger than the fork's
            let bound = round_up(level_bound(&acc.m4, &acc.rows, &acc.uv4) * (1.0 + 1.0e-12));
            if bound < *bound_out {
                // the thumbweight over the span, proved at the level whose bound is returned
                if let Some(v) = vary.as_deref_mut() {
                    let ([lo, hi], [rlo, rhi]) = (v.w, acc.rows.w);
                    v.w = [cmin(lo, rlo), cmax(hi, rhi)];
                }
            }
            *bound_out = cmin(*bound_out, bound);
            if *bound_out <= ctx.target {
                return Ok((Flow::Stop, true));
            }
        }
        // the tighter terms where the held ones are not at the target: the arc's first,
        // then the feet's (#14405)
        if acc.lazy.arc {
            tighten_level(acc, pieces, ctx, o);
        } else if acc.lazy.feet && gate(acc, ctx, lb) <= ctx.target {
            tighten_feet(k, acc, pieces, sc, vary.as_deref_mut())?;
        } else {
            break;
        }
        if !acc.proved {
            return Ok((Flow::Next, certified));
        }
    }
    // steered by the fork's interval terms: a level they do not prove goes on
    if !acc.proved_old {
        return Ok((Flow::Next, certified));
    }
    if !level_bound(&acc.m4_old, &acc.rows_old, &acc.uv4_fork_old).is_finite() {
        return Ok((Flow::Skip, certified));
    }
    let bound = round_up(level_bound(&acc.m4_old, &acc.rows_old, &acc.uv4_old) * (1.0 + 1.0e-12));
    // the bound no finer level goes under: over the target, no level certifies the span
    // (the jets' least terms only where the bound takes the jets' sum, not with
    // varying rows, whose own least terms are not ported: the fork's f916a8d). Steering,
    // the uv curves' least term is the fork's form, as the levels are the fork's; to the
    // target, it is each parameter's own (#2690), the form the certified bound takes: the
    // fork's is not under that bound, and stopped a re-certificate a level short of the
    // bound its construction proved on the span's halves (#15010)
    let k4 = h * h * h * h / 384.0;
    let uv_lo = match ctx.halving {
        Halving::Steers => &acc.uv4_lo_fork,
        Halving::Ignored => &acc.uv4_lo,
    };
    let mut lower = 0.0_f64;
    for (m4lf, &uvl) in acc.m4_lo.iter().zip(uv_lo) {
        let mut sum = 0.0_f64;
        for &m in m4lf.iter().take(l_jets + 1) {
            sum += k4 * m;
        }
        lower = cmax(lower, cmax(if rows_vary { 0.0 } else { sum }, k4 * uvl));
    }
    if round_down(lower * (1.0 - 1.0e-12)) > ctx.target {
        return Ok((Flow::Stop, certified));
    }
    // once proved, one finer level more only where the last one halved the bound (or, to
    // the target, while over it)
    if ctx.halving == Halving::Steers && bound > 0.5 * *last {
        return Ok((Flow::Stop, certified));
    }
    *last = bound;
    Ok((Flow::Next, certified))
}

/// `sm_GnCertifySpan`: the certified bound on the Hermite span between two stations, or
/// infinity where the span is not proved. The Hermite remainder is `h^4 / 384` times the
/// jets' largest fourth s-derivative over the span, bounded on its pieces
/// ([`gn_fourth_over`] on each): the span cut in `2^L` equal pieces, L raised (to 6 at
/// most) until every piece is proved and, over the target, while that halves the bound, so
/// that a long span's bound follows its derivatives, not the width of one enclosure over
/// it. The feet's uv curves are Hermite interpolants of the same stations, and the face at
/// a uv curve's point is off the face's true foot by at most `|S_u| |du| + |S_v| |dv|`,
/// each uv error `h^4 / 384 |x''''|` of its own parameter: that bound counts too (an
/// edge's curve against its face).
///
/// Every level's bound is at least the same sum with each term's least value at the
/// pieces' middles (every piece's enclosure holds its middle's value): once that is over
/// the target no finer level certifies the span, and the bound so far is returned for the
/// caller to halve the span.
///
/// Not the fork's (#2690): a level is certified by the uv curves' term taken per parameter
/// (`|S_u| |du| + |S_v| |dv|`), never larger than the fork's; the levels are steered (that
/// least value, and going on only where a level halved the bound) by the fork's term, so
/// a span takes at most the fork's levels and is certified at the same level or sooner.
///
/// # Errors
///
/// `InvalidInput` when `n` exceeds `SM_GN_MAXE`.
#[allow(clippy::too_many_arguments)]
pub fn gn_certify_span(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    target: f64,
    vary: Option<&mut GnVarying>,
    law: Option<&GnLaw<'_>>,
    defs: Option<&mut GnDefs<'_>>,
) -> Result<f64> {
    let mut reuse = SpanReuse::default();
    gn_certify_span_reusing(
        k, faces_n, leader_n, faces, center, a, b, r, side, n, target, vary, law, defs, &mut reuse,
    )
}

/// What [`gn_certify_span_reusing`] takes from a span's neighbours and gives back (not the
/// fork's, which certifies every span from level 0). The default takes nothing: the span's
/// certificate is then [`gn_certify_span`]'s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpanReuse<'a> {
    /// In: the finest proved level of the span this one is a piece of.
    pub inherit: Option<&'a CertifiedLevel>,
    /// In: how many halvings this span is of the one it inherits from: a piece of it cut into
    /// `2^split` (0 is taken as 1, a half).
    pub split: u32,
    /// In: the first level tried where nothing is inherited (the coarser ones are not).
    pub start: u32,
    /// Out: this span's finest proved level, for its own halves.
    pub kept: Option<CertifiedLevel>,
    /// Out: the first level this span proved (an inherited one as its level here).
    pub first: Option<u32>,
    /// In: go on to a finer level while the bound is over the target, not only where the
    /// last level halved it (a target proved before, which a re-certificate must reach: the
    /// halving is a guess at the cost of the next level, and on bounds that differ in
    /// their rounding it stops one run a level before the other). The least value of the
    /// certified bound (its uv curves' term in each parameter's own form, not the fork's
    /// that steers the levels) still stops them: past it no level certifies the span.
    pub to_target: bool,
}

/// [`gn_certify_span`] taking from and giving to its neighbours ([`SpanReuse`]), not the
/// fork's. `reuse.inherit`: the span is one of the `2^split` pieces of one the caller cut,
/// and that span's finest proved level ([`CertifiedLevel`]) has pieces that meet this span
/// and cover it. Their bounds hold over every point of them, so over this span; they are its
/// level `split` coarser than theirs was (the span is `2^split` times shorter; level 0 where
/// they are longer than the span), taken without an enclosure, and the
/// levels go on from the next one as [`gn_certify_span`]'s do. The pieces' row terms are
/// taken again about this span's first station. Otherwise the levels start at
/// `reuse.start` (the caller's guess from the span before; a level not tried bounds
/// nothing, so the bound returned holds as [`gn_certify_span`]'s). A level's end stations
/// that the level before solved (every other one, at the same parameter from the same
/// seed) are taken from it.
///
/// # Errors
///
/// As [`gn_certify_span`].
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn gn_certify_span_reusing(
    k: &Kernel,
    faces_n: [&Nurbs; 2],
    leader_n: &NurbsCurve,
    faces: [&Surface; 2],
    center: &BSplineCurve,
    a: &GnStation,
    b: &GnStation,
    r: [f64; 2],
    side: [f64; 2],
    n: usize,
    target: f64,
    mut vary: Option<&mut GnVarying>,
    law: Option<&GnLaw<'_>>,
    mut defs: Option<&mut GnDefs<'_>>,
    reuse: &mut SpanReuse<'_>,
) -> Result<f64> {
    crate::profile::profile_scope!(k, "blend::certificate::gn_certify_span");
    // the series' orders are checked once (the fork leaves them to its callers)
    Trunc::new(4 + TAYLOR, n)?;
    let mut bound_out = f64::INFINITY;
    let h = b.s - a.s;
    let mut last = f64::INFINITY;
    // the varying thumbweight's fixed one wb and point O: the first station's
    let mut bar = GnSectionFormT::<Ivl> {
        w: Vec::new(),
        g: [Vec::new(), Vec::new()],
    };
    let mut o = [0.0_f64; 3];
    // the ball's circle (n = 1, cc08236): its arc's rows all vary along s, about the first
    // station's centre
    let arc = n == 1;
    let rows_vary = vary.is_some() || arc;
    let l_jets = if arc { 0 } else { n }; // the jets graded on their own: the arc's rails only
    if let Some(v) = vary.as_deref() {
        if !(a.w.is_finite() && b.w.is_finite()) {
            return Err(SolidError::blend_failed(
                OP,
                "the varying jets' thumbweights are not finite",
            ));
        }
        bar = gn_form_normalized(v.n, Ivl::from(a.w));
        for (q, oq) in o.iter_mut().enumerate() {
            *oq = a.f[0].a.get(q).map_or(f64::NAN, |c| c.at(0, 0));
        }
    }
    if arc {
        bar.w = vec![Ivl::from(0.0); 3];
        for (q, oq) in o.iter_mut().enumerate() {
            *oq = a.v.a.get(q).map_or(f64::NAN, |c| c.at(0, 0));
        }
    }
    let ctx = LevelCtx {
        h,
        l_jets,
        arc,
        rows_vary,
        touch: law.is_some(),
        target,
        halving: if reuse.to_target {
            Halving::Ignored
        } else {
            Halving::Steers
        },
    };
    // the touch circle of a varying ball takes the centre's s-derivative: one order more
    let extra = usize::from(arc && law.is_some());
    // the feet's Taylor models (#14405) enclose NURBS faces only, not a blend by its
    // definition; and not where the thumbweight varies, whose rows' interval terms, not the
    // feet's, keep its levels over the target (the tilted pipe's round of
    // wr698_gn_round_is_its_definition took 2.7 times as long with them)
    let tm_faces = vary.is_none()
        && defs
            .as_deref()
            .is_none_or(|d| d.faces.iter().all(Option::is_none));
    let feet = FeetCtx {
        k,
        faces_n,
        leader_n,
        r,
        side,
        n: l_jets,
        order: 4 + TAYLOR + extra,
        law,
        rows_vary,
        on: tm_faces,
    };
    let sc = SpanCtx {
        a,
        b,
        bar: &bar,
        o,
        level: &ctx,
        feet: &feet,
    };
    // a lower bound on every level's certified bound, from the pieces' middles so far: the
    // feet's Taylor models are taken only while it is at most the target (#14405)
    let mut lb = 0.0_f64;
    // a cut span's pieces over this piece: its level `split` coarser than theirs
    let mut start = reuse.start.min(SPAN_MAX_LEVEL);
    let inherit = reuse.inherit;
    if let Some(pieces) = inherit.and_then(|l| covering(l, a.s, b.s)) {
        let split = reuse.split.max(1);
        start = inherit.map_or(1, |l| l.level.saturating_sub(split) + 1);
        let mut acc = LevelAcc::new(bar.w.len());
        let mut own_pieces: Vec<CertifiedPiece> = pieces.into_iter().cloned().collect();
        for p in &mut own_pieces {
            // a piece's least terms are at its middle: only a piece inside the span steers
            let steer = p.s[0] >= a.s && p.s[1] <= b.s;
            absorb_piece(k, &mut acc, p, steer, &sc, vary.as_deref_mut(), &mut lb);
            if !acc.proved {
                break;
            }
        }
        if acc.proved && acc.lazy.pending {
            resolve_pending(k, &mut acc, &mut own_pieces, &sc, vary.as_deref_mut(), lb)?;
        }
        if acc.proved {
            let mut own = CertifiedLevel {
                level: start - 1,
                pieces: own_pieces,
            };
            let (flow, certified) = close_level(
                k,
                &mut acc,
                &mut own.pieces,
                &sc,
                &mut bound_out,
                &mut last,
                vary.as_deref_mut(),
                lb,
            )?;
            if certified {
                reuse.first = reuse.first.or(Some(own.level));
                reuse.kept = Some(own);
            }
            if matches!(flow, Flow::Stop) {
                return Ok(bound_out);
            }
        }
    }
    // the last level whose end stations were all solved, and those stations
    let mut solved_ends: Option<(u32, Vec<GnStation>)> = None;
    for level in start..=SPAN_MAX_LEVEL {
        k.check_cancelled(OP_CERTIFY_SPAN)?;
        let pieces = 1_usize << level;
        let coarser = match &solved_ends {
            Some((l, e)) if l + 1 == level => Some(e),
            _ => None,
        };
        // the pieces' end stations
        let mut ends = Vec::new();
        ends.push(*a);
        let mut solved = true;
        let mut i = 1;
        while i < pieces && solved {
            k.check_cancelled(OP_CERTIFY_SPAN)?;
            // every other end is the coarser level's, at the same s from the same seed
            let known = coarser.filter(|_| i % 2 == 0).and_then(|e| e.get(i / 2));
            if let Some(st) = known {
                ends.push(*st);
                i += 1;
                continue;
            }
            let t = i as f64 / pieces as f64;
            let s = a.s + t * h;
            let mut seed = [0.0_f64; 4];
            for (c, sc) in seed.iter_mut().enumerate() {
                let ac = a.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
                let bc = b.x.get(c).map_or(f64::NAN, |x| x.at(0, 0));
                *sc = (1.0 - t) * ac + t * bc;
            }
            let sm = gn_try!(center.evaluate_point(s), bound_out);
            let point = Trunc::new(0, 0)?;
            let tol = 1.0e-9 * smax(1.0, sm.length());
            let end_r = gn_try!(law_series(law, s, 0, false), bound_out);
            match gn_solve_station_counted(
                k,
                faces,
                center,
                s,
                seed,
                r,
                side,
                point,
                tol,
                end_r.as_ref(),
            )
            .0
            {
                Ok(st) => ends.push(st),
                // a cancel (#2136) is not an unsolved piece: it ends the operation
                Err(e @ SolidError::Cancelled { .. }) => return Err(e),
                Err(_) => solved = false,
            }
            i += 1;
        }
        if !solved {
            continue;
        }
        ends.push(*b);
        solved_ends = Some((level, ends.clone()));
        let mut acc = LevelAcc::new(bar.w.len());
        let mut level_pieces = Vec::new();
        for pair in ends.windows(2) {
            if !acc.proved {
                break;
            }
            k.check_cancelled(OP_CERTIFY_SPAN)?;
            let [ea, eb] = pair else {
                return Err(SolidError::blend_failed(OP, "the span has no pair of ends"));
            };
            // the piece's enclosures ([`gn_fourth_over_series`]'s); the middle's also where
            // the piece's is not proved, for its feet's Taylor models (#14405)
            let pe = gn_try!(
                piece_enclosures(
                    k,
                    faces_n,
                    leader_n,
                    faces,
                    center,
                    ea,
                    eb,
                    r,
                    side,
                    l_jets,
                    law,
                    extra,
                    defs.as_deref_mut(),
                    tm_faces && gate(&acc, &ctx, lb) <= target,
                ),
                bound_out
            );
            let (fourth, series) = match (&pe.e5, &pe.mid) {
                (Some(e5), Some((mid, e4))) => fourth_of(e5, e4, eb.s - ea.s, l_jets, *mid),
                _ => (UNPROVED_FOURTH, None),
            };
            let inputs = match pe.mid {
                Some((mid, e4)) if tm_faces => Some(Box::new(PieceInputs {
                    at: span_at(faces_n, ea, eb),
                    mid,
                    e4,
                })),
                _ => None,
            };
            let mut piece = CertifiedPiece {
                s: [ea.s, eb.s],
                fourth,
                series: if rows_vary {
                    series.map(Box::new)
                } else {
                    None
                },
                tight: None,
                inputs,
            };
            absorb_piece(
                k,
                &mut acc,
                &mut piece,
                true,
                &sc,
                vary.as_deref_mut(),
                &mut lb,
            );
            level_pieces.push(piece);
        }
        if acc.proved && acc.lazy.pending {
            resolve_pending(k, &mut acc, &mut level_pieces, &sc, vary.as_deref_mut(), lb)?;
        }
        if !acc.proved {
            continue;
        }
        let mut own = CertifiedLevel {
            level,
            pieces: level_pieces,
        };
        let (flow, certified) = close_level(
            k,
            &mut acc,
            &mut own.pieces,
            &sc,
            &mut bound_out,
            &mut last,
            vary.as_deref_mut(),
            lb,
        )?;
        if certified {
            reuse.first = reuse.first.or(Some(own.level));
            reuse.kept = Some(own);
        }
        if matches!(flow, Flow::Stop) {
            break;
        }
    }
    Ok(bound_out)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::super::station::{as_b_spline, nurbs_partials, vec3};
    use super::*;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::extent2d::Extent2d;
    use crate::math::{Vec2, Vec3};
    use crate::nurbs::b_spline_curve::NurbCircleParam;
    use crate::surface::cone::Cone;
    use crate::surface::plane::Plane;

    fn plane(origin: Vec3, x_axis: Vec3, y_axis: Vec3) -> Surface {
        Surface::from(Plane::new(
            origin,
            x_axis,
            y_axis,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    fn nurbs(k: &Kernel, s: &Surface) -> Nurbs {
        Nurbs::of(&as_b_spline(k, s).unwrap().unwrap()).unwrap()
    }

    fn station(
        k: &Kernel,
        faces: [&Surface; 2],
        leader: &BSplineCurve,
        s: f64,
        seed: [f64; 4],
        r: [f64; 2],
        side: [f64; 2],
    ) -> GnStation {
        let tr = Trunc::new(0, 0).unwrap();
        super::super::station::gn_solve_station(k, faces, leader, s, seed, r, side, tr, 1e-12)
            .unwrap()
    }

    fn contains(i: Ivl, x: f64) -> bool {
        i.lo <= x && x <= i.hi
    }

    /// A cubic in u with an interior knot of multiplicity `m` at u = 1 (knots 0 x4, 1 xm,
    /// 2 x4), linear in v, its control points' z not a polynomial across the knot.
    fn cubic_with_knot(m: usize) -> Nurbs {
        let mut u = vec![0.0; 4];
        u.extend(std::iter::repeat_n(1.0, m));
        u.extend([2.0; 4]);
        let nu = u.len() - 4;
        let mut pw = Vec::new();
        let zs = [0.0, 1.0, -0.5, 2.0, 0.3, -1.2];
        for (i, z) in (0..nu).zip(zs.iter().cycle()) {
            for y in [0.0, 1.0] {
                pw.extend([i as f64, y, z + 0.25 * y, 1.0]);
            }
        }
        Nurbs {
            p: 3,
            q: 1,
            nu,
            nv: 2,
            u,
            v: vec![0.0, 0.0, 1.0, 1.0],
            pw,
        }
    }

    /// A box across a knot line: the piecewise enclosure holds every partial of the face on
    /// both sides (the third u-partial jumps at the knot), which one piece continued over the
    /// box does not; a knot of too low a continuity, and a box past the knots, are refused.
    #[test]
    fn a_box_past_its_span_keeps_the_unclipped_weight() {
        // the control weights bound the weight only inside the span: a box across the knot at
        // u = 1 is enclosed as `nurbs_partials_over_piece` encloses it
        let mut n = cubic_with_knot(1);
        // weights from 1 to 4.5: the weight continued past the span leaves their range
        for (i, w) in n.pw.chunks_mut(4).enumerate() {
            let k = 1.0 + 0.5 * i as f64;
            for c in w.iter_mut() {
                *c *= k;
            }
        }
        let a = nurbs_partials_within_span(&n, 3, 1, 0.9, 1.1, 0.2, 0.3, 3).unwrap();
        let b = nurbs_partials_over_piece(&n, 3, 1, 0.9, 1.1, 0.2, 0.3, 3).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_box_across_a_knot_line_is_enclosed_piece_by_piece() {
        let n = cubic_with_knot(1);
        let (ua, ub, va, vb) = (0.9, 1.1, 0.2, 0.3);
        let e = nurbs_partials_over_box(&n, ua, ub, va, vb, 3, FEET_BOX_CONTINUITY).unwrap();
        let left = nurbs_partials_over_piece(&n, 3, 1, ua, ub, va, vb, 3).unwrap();
        let mut continued_misses = false;
        for &u in &[0.9, 0.95, 1.0, 1.05, 1.1] {
            for &v in &[0.2, 0.25, 0.3] {
                // at the knot itself, either side's piece
                let spans = if u == 1.0 {
                    vec![3, 4]
                } else {
                    vec![span(&n.u, 3, n.nu, u)]
                };
                for ku in spans {
                    let x = nurbs_partials::<f64>(&n, ku, 1, u, v, 3);
                    for a in 0..=3 {
                        for b in 0..=(3 - a) {
                            let (p, q) = (x.at(a, b), e.at(a, b));
                            for (c, i) in [(p.x, q.x), (p.y, q.y), (p.z, q.z)] {
                                let slack = 1e-9 * (1.0 + c.abs());
                                assert!(
                                    i.lo - slack <= c && c <= i.hi + slack,
                                    "P[{a}][{b}] at ({u}, {v}): {c} not in {i:?}"
                                );
                            }
                        }
                    }
                    let c = x.at(3, 0).z;
                    continued_misses |= !contains(left.at(3, 0).z, c);
                }
            }
        }
        assert!(
            continued_misses,
            "the left piece continued is not the face past the knot"
        );
        // a point on the knot: both sides' partials (the third u-partial jumps there)
        let at = nurbs_partials_over_box(&n, 1.0, 1.0, 0.25, 0.25, 3, FEET_BOX_CONTINUITY).unwrap();
        let sides = [3, 4].map(|ku| nurbs_partials::<f64>(&n, ku, 1, 1.0, 0.25, 3).at(3, 0).z);
        assert!(sides[0] != sides[1], "{sides:?}");
        for c in sides {
            assert!(contains(at.at(3, 0).z, c), "{c} not in {:?}", at.at(3, 0).z);
        }
        // a double knot (C^1) crossed by a feet's box is refused; at C^1 asked, enclosed
        let d = cubic_with_knot(2);
        assert!(matches!(
            nurbs_partials_over_box(&d, ua, ub, va, vb, 3, FEET_BOX_CONTINUITY),
            Err(SolidError::Unsupported { .. })
        ));
        assert!(nurbs_partials_over_box(&d, ua, ub, va, vb, 3, 1).is_ok());
        // past the knots
        assert!(matches!(
            nurbs_partials_over_box(&n, 1.9, 2.1, va, vb, 2, FEET_BOX_CONTINUITY),
            Err(SolidError::Unsupported { .. })
        ));
    }

    /// Two planes at a square corner, the ball of radius r on the edge along y (the
    /// station test's closed form): the feet (tau r, L s, 0) and (0, L s, tau r) are linear
    /// in s and tau, so every fourth s-derivative is 0 and the span's bound is 0 to the
    /// enclosures' rounding. Checked against that closed form: the enclosed series hold
    /// the exact coefficients (`du1/deps = r`, `dv1/ds = L`, the box holds the feet over
    /// the span), every `m4`, `uv4` is below 1e-9, and the certified bound is in
    /// `[0, 1e-9]`.
    #[test]
    fn two_plane_span_bound_is_zero() {
        let k = Kernel::default();
        let f1 = plane(Vec3::ZERO, Vec3::X, Vec3::Y); // z = 0, (u, v) = (x, y)
        let f2 = plane(Vec3::ZERO, Vec3::Y, Vec3::Z); // x = 0, (u, v) = (y, z)
        let (r, len) = (2.0, 5.0);
        let leader =
            BSplineCurve::create_line_segment(3, Vec3::new(r, 0.0, r), Vec3::new(r, len, r))
                .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        assert_eq!((ln.p, ln.n, ln.u.len()), (1, 2, 4));
        let (n1, n2) = (nurbs(&k, &f1), nurbs(&k, &f2));
        let faces = [&f1, &f2];
        let (rr, side) = ([r, r], [1.0, 1.0]);
        let (sa, sb) = (0.2, 0.6);
        let a = station(&k, faces, &leader, sa, [r, len * sa, len * sa, r], rr, side);
        let b = station(&k, faces, &leader, sb, [r, len * sb, len * sb, r], rr, side);
        let n = 2;
        let e = gn_enclose(
            &k,
            [&n1, &n2],
            &ln,
            &a,
            &b,
            rr,
            side,
            n,
            4 + TAYLOR,
            None,
            None,
        )
        .unwrap()
        .expect("proved");
        assert!(contains(e.x[0].c[0][0], r) && contains(e.x[3].c[0][0], r));
        for q in [1, 2] {
            let x = e.x[q].c[0][0];
            assert!(x.lo <= len * sa && len * sb <= x.hi, "{q} {x:?}");
            assert!(contains(e.x[q].c[1][0], len), "{q}");
        }
        assert!(contains(e.x[0].c[0][1], r) && contains(e.x[3].c[0][1], r));
        // |S_u| and |S_v| of a unit plane are 1
        for dsf in e.ds {
            assert!(dsf.iter().all(|d| (d - 1.0).abs() < 1e-12), "{dsf:?}");
        }
        let (m4, uv4, ..) =
            gn_fourth_over(&k, [&n1, &n2], &ln, faces, &leader, &a, &b, rr, side, n).unwrap();
        for f in 0..2 {
            assert!(uv4[f] >= 0.0 && uv4[f] < 1e-9, "{uv4:?}");
            for kk in 0..=n {
                assert!(m4[f][kk] >= 0.0 && m4[f][kk] < 1e-9, "{m4:?}");
            }
        }
        let bound = gn_certify_span(
            &k,
            [&n1, &n2],
            &ln,
            faces,
            &leader,
            &a,
            &b,
            rr,
            side,
            n,
            1e-6,
            None,
            None,
            None,
        )
        .unwrap();
        assert!((0.0..1e-9).contains(&bound), "{bound}");
    }

    /// The (u, v) of the face's net nearest to `p`, on a grid (a seed for Newton).
    fn nearest_uv(n: &Nurbs, p: Vec3) -> (f64, f64) {
        let (u0, u1) = (n.u[0], n.u[n.u.len() - 1]);
        let (v0, v1) = (n.v[0], n.v[n.v.len() - 1]);
        let mut best = (f64::INFINITY, u0, v0);
        for i in 0..=400 {
            for j in 0..=40 {
                let u = u0 + (u1 - u0) * f64::from(i) / 400.0;
                let v = v0 + (v1 - v0) * f64::from(j) / 40.0;
                let ku = span(&n.u, n.p, n.nu, u);
                let kv = span(&n.v, n.q, n.nv, v);
                let d = (vec3(nurbs_partials(n, ku, kv, u, v, 0).at(0, 0)) - p).length();
                if d < best.0 {
                    best = (d, u, v);
                }
            }
        }
        (best.1, best.2)
    }

    /// A cylinder of radius R about z standing on the plane z = 0, the ball of radius r
    /// outside it: the centre line L(s) is the circle of radius R + r at height r (a
    /// rational quadratic arc), and the feet are `F1 = (L_xy (R + tau r) / (R + r), 0)` on
    /// the plane and `F2 = (L_xy R / (R + r), tau r)` on the cylinder. So the jets' fourth
    /// s-derivatives are, exactly, `|c10''''| = |L''''|`, `|c11''''| = r / (R + r) |L''''|`,
    /// `|c20''''| = R / (R + r) |L''''|` and 0 for the rest, and the plane's uv curve is
    /// `L_xy` (`uv4 >= |L_x''''| + |L_y''''|`, the plane's partials of length 1). Checked
    /// against that closed form, with
    /// L'''' from the leader's own evaluator: at the span's middle (one point, a tight
    /// enclosure) `4! c[4][k]` of every jet and of the plane's (u, v) holds the closed
    /// form's value; over the span every `m4`, `uv4` is finite and at least the closed
    /// form's maximum (sampled at 201 points); the certified bound is finite, below the
    /// target, at least `h^4 / 384` times the closed-form jets' sum, and level 0's (the
    /// pieces' maximum). The span's bounds are sound but loose (`m4[0][0]` about 36 times
    /// the closed form at h = 0.02: the series enclosed over an interval of s widen order
    /// by order, and the Taylor form's remainder carries that). A span five times as long
    /// fails Krawczyk's test in one enclosure and is proved by the pieces, its bound again
    /// finite, below the target and above the closed form's.
    /// A piece's enclosure holds every station sampled over the piece: the feet and their
    /// Taylor coefficients in s and eps, as solved in floating point at 201 points, to 1e-8 (the
    /// box tightened after Krawczyk's proof keeps the feet: every root in a box is in its
    /// K). A ball rolled between a plane and a cylinder (as made a NURBS), a piece of 0.02.
    #[test]
    fn an_enclosure_holds_every_sampled_station() {
        let k = Kernel::default();
        let (big_r, r) = (3.0, 1.0);
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let mut cyl = Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::X,
            Vec3::Y,
            big_r,
            0.0,
            360.0,
            10.0,
            false,
            false,
            false,
        );
        cyl.make_nurb(&k).unwrap();
        let fc = Surface::from(cyl);
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            big_r + r,
            10.0,
            80.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = big_r / (big_r + r);
            Vec3::new(l.x * q, l.y * q, r)
        };
        let (cu, cv) = nearest_uv(&nc, foot_c(0.41));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = foot_c(0.41) - Vec3::new(0.0, 0.0, r);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let n = 2;
        let order = 4 + TAYLOR;
        let solve = |s: f64, tr: Trunc| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            super::super::station::gn_solve_station(
                &k,
                faces,
                &leader,
                s,
                [l.x, l.y, u, v],
                rr,
                side,
                tr,
                1e-10,
            )
        };
        let point = Trunc::new(0, 0).unwrap();
        let (sa, sb) = (0.4, 0.42);
        let (a, b) = (solve(sa, point).unwrap(), solve(sb, point).unwrap());
        let e = gn_enclose(&k, [&np, &nc], &ln, &a, &b, rr, side, n, order, None, None)
            .unwrap()
            .expect("proved over the piece");
        // the floating solve's jets go as far as the leader's evaluation does (a rational
        // circle's derivatives are evaluated to a fixed order): every order it reaches
        let top = (0..=order)
            .rev()
            .find(|&o| solve(sa, Trunc::new(o, n - 1).unwrap()).is_ok())
            .unwrap();
        assert!(top >= 3, "{top}");
        let jets = Trunc::new(top, n - 1).unwrap();
        let mut judged = 0;
        for i in 0..=200 {
            let s = sa + (sb - sa) * f64::from(i) / 200.0;
            let st = solve(s, jets).unwrap();
            for (xs, xe) in st.x.iter().zip(&e.x) {
                for o in 0..=top {
                    for kk in 0..n {
                        let (x, iv) = (xs.c[o][kk], xe.c[o][kk]);
                        // the floating solve's own error (its residual within 1e-10)
                        let slack = 1e-8 * (1.0 + x.abs());
                        assert!(
                            iv.lo - slack <= x && x <= iv.hi + slack,
                            "s {s}: c[{o}][{kk}] {x:e} outside [{:e}, {:e}]",
                            iv.lo,
                            iv.hi
                        );
                        judged += 1;
                    }
                }
            }
        }
        assert_eq!(judged, 201 * 4 * (top + 1) * n);
    }

    #[test]
    fn cylinder_plane_span_bound_is_the_closed_form() {
        let k = Kernel::default();
        let (big_r, r) = (3.0, 1.0);
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let mut cyl = Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::X,
            Vec3::Y,
            big_r,
            0.0,
            360.0,
            10.0,
            false,
            false,
            false,
        );
        cyl.make_nurb(&k).unwrap();
        let fc = Surface::from(cyl);
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            big_r + r,
            10.0,
            80.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        // the sides: the plane's normal is +z, towards the ball; the cylinder's by its net
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = big_r / (big_r + r);
            Vec3::new(l.x * q, l.y * q, r)
        };
        let (cu, cv) = nearest_uv(&nc, foot_c(0.45));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = foot_c(0.45) - Vec3::new(0.0, 0.0, r);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let st = |s: f64| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            station(&k, faces, &leader, s, [l.x, l.y, u, v], rr, side)
        };
        let (sa, sb) = (0.4, 0.42);
        let (a, b) = (st(sa), st(sb));
        // the stations are the closed form's
        let fa = vec3(V3 {
            x: a.f[1].a[0].c[0][0],
            y: a.f[1].a[1].c[0][0],
            z: a.f[1].a[2].c[0][0],
        });
        assert!((fa - foot_c(sa)).length() < 1e-9);
        assert!((a.x[0].c[0][0] - lpt(sa).x).abs() < 1e-9);

        // L'''' over the span, sampled
        let (mut l4, mut l4x, mut l4y) = (0.0_f64, 0.0_f64, 0.0_f64);
        for i in 0..=200 {
            let s = sa + (sb - sa) * f64::from(i) / 200.0;
            let d = leader.evaluate(s, 4, true, false).unwrap();
            let d4 = d[4];
            assert!(d4.z.abs() < 1e-9);
            l4 = l4.max(Vec3::new(d4.x, d4.y, 0.0).length());
            l4x = l4x.max(d4.x.abs());
            l4y = l4y.max(d4.y.abs());
        }
        assert!(l4 > 1.0, "{l4}");
        let n = 2;
        // at the span's middle the jets' fourth derivatives are enclosed on one point: they
        // hold the closed form's, `(4! c[4][k])`
        let sm = 0.5 * (sa + sb);
        let ms = st(sm);
        let em = gn_enclose(
            &k,
            [&np, &nc],
            &ln,
            &ms,
            &ms,
            rr,
            side,
            n,
            4 + TAYLOR - 1,
            None,
            None,
        )
        .unwrap()
        .expect("proved at a point");
        let dm = leader.evaluate(sm, 4, true, false).unwrap()[4];
        let holds = |i: Ivl, x: f64| {
            let j = Ivl::from(24.0) * i;
            j.lo - 1e-9 <= x && x <= j.hi + 1e-9 && j.hi - j.lo < 1e-2
        };
        let (qp, qc) = (r / (big_r + r), big_r / (big_r + r));
        for (c, l) in [(0, dm.x), (1, dm.y), (2, 0.0)] {
            assert!(holds(em.f[0].a[c].c[4][0], l), "c10 {c}");
            assert!(holds(em.f[0].a[c].c[4][1], qp * l), "c11 {c}");
            assert!(holds(em.f[1].a[c].c[4][0], qc * l), "c20 {c}");
            assert!(holds(em.f[1].a[c].c[4][1], 0.0), "c21 {c}");
            assert!(holds(em.f[0].a[c].c[4][2], 0.0) && holds(em.f[1].a[c].c[4][2], 0.0));
        }
        // the plane's (u, v) is the foot's (x, y)
        assert!(holds(em.x[0].c[4][0], dm.x) && holds(em.x[1].c[4][0], dm.y));
        let (m4, uv4, ..) =
            gn_fourth_over(&k, [&np, &nc], &ln, faces, &leader, &a, &b, rr, side, n).unwrap();
        let want = [
            [l4, r / (big_r + r) * l4, 0.0],
            [big_r / (big_r + r) * l4, 0.0, 0.0],
        ];
        for f in 0..2 {
            for kk in 0..=n {
                let (got, w) = (m4[f][kk], want[f][kk]);
                assert!(got.is_finite() && w <= got, "{f} {kk} {got} {w}");
            }
        }
        let wuv = l4x + l4y;
        assert!(wuv <= uv4[0] && uv4[0].is_finite(), "{} {wuv}", uv4[0]);
        assert!(uv4[1].is_finite());

        let h = sb - sa;
        let target = 1e-3;
        let bound = gn_certify_span(
            &k,
            [&np, &nc],
            &ln,
            faces,
            &leader,
            &a,
            &b,
            rr,
            side,
            n,
            target,
            None,
            None,
            None,
        )
        .unwrap();
        let jets = h * h * h * h / 384.0 * (want[0][0] + want[0][1]);
        assert!(bound.is_finite() && bound < target, "{bound}");
        assert!(
            bound >= jets && bound >= h * h * h * h / 384.0 * wuv,
            "{bound} {jets}"
        );
        // below the target at level 0: the bound is that level's, the pieces' maximum
        let lvl0 = h * h * h * h / 384.0
            * (m4[0][0] + m4[0][1] + m4[0][2])
                .max(m4[1][0] + m4[1][1] + m4[1][2])
                .max(uv4[0])
                .max(uv4[1]);
        assert!((bound - lvl0).abs() <= 1e-9 * lvl0, "{bound} {lvl0}");

        // a span five times as long: one enclosure over it fails Krawczyk's test, the
        // pieces prove it, and the bound still holds the closed form's
        let (lb, hl) = (st(0.5), 0.1);
        let mut l4l = 0.0_f64;
        for i in 0..=500 {
            let d = leader
                .evaluate(sa + hl * f64::from(i) / 500.0, 4, true, false)
                .unwrap();
            l4l = l4l.max(Vec3::new(d[4].x, d[4].y, 0.0).length());
        }
        let one = gn_enclose(
            &k,
            [&np, &nc],
            &ln,
            &a,
            &lb,
            rr,
            side,
            n,
            4 + TAYLOR,
            None,
            None,
        )
        .unwrap();
        assert!(one.is_none());
        let long = gn_certify_span(
            &k,
            [&np, &nc],
            &ln,
            faces,
            &leader,
            &a,
            &lb,
            rr,
            side,
            n,
            target,
            None,
            None,
            None,
        )
        .unwrap();
        let jl = hl * hl * hl * hl / 384.0 * (1.0 + r / (big_r + r)) * l4l;
        assert!(
            long.is_finite() && long < target && long >= jl,
            "{long} {jl}"
        );
    }

    use crate::nurbs::b_spline_curve::{BSplineCurveForm, CanonicalData, KnotType};

    /// A fixed-seed generator in `[0, 1)`.
    fn rng(seed: u64) -> impl FnMut() -> f64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 11) as f64 / (1_u64 << 53) as f64
        }
    }

    /// The quadratic Bezier curve on `[0, 1]` with control points `c`.
    fn bezier2(c: [Vec3; 3]) -> BSplineCurve {
        BSplineCurve::create_canonical(&CanonicalData {
            dim: 3,
            degree: 2,
            control_points: &c,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[3, 3],
            knots: &[0.0, 1.0],
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        })
        .unwrap()
    }

    /// The faces, their nets, the leader and the radius law a piece's arc terms are taken
    /// on, and the feet's seed at s.
    struct ArcCase {
        faces: [Surface; 2],
        nets: [Nurbs; 2],
        leader: BSplineCurve,
        law: Option<BSplineCurve>,
        r: [f64; 2],
        side: [f64; 2],
        seed: Box<dyn Fn(f64) -> [f64; 4]>,
    }

    impl ArcCase {
        /// The wedge of the plane z = 0 and the plane through the x axis at `phi` from it
        /// (towards (0, cos phi, sin phi)), the ball between them of radius r(s), the
        /// quadratic Bezier law `r` over x = 10 s, s in `[0, 1]`: its centre
        /// `r(s) / sin(phi / 2)` along the bisector, the quadratic Bezier with the same
        /// control radii, is the leader. A constant `r` is no law.
        fn wedge(k: &Kernel, phi: f64, r: [f64; 3]) -> Self {
            let len = 10.0;
            let (cp, sp) = (phi.cos(), phi.sin());
            let f1 = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
            let f2 = plane(Vec3::ZERO, Vec3::X, Vec3::new(0.0, cp, sp));
            let (bc, bs) = ((0.5 * phi).cos(), (0.5 * phi).sin());
            let ctrl = |i: usize| {
                let rho = r[i] / bs;
                Vec3::new(len * i as f64 / 2.0, rho * bc, rho * bs)
            };
            let leader = bezier2([ctrl(0), ctrl(1), ctrl(2)]);
            let constant = r[0] == r[1] && r[1] == r[2];
            let law = (!constant).then(|| bezier2([0, 1, 2].map(|i| Vec3::new(r[i], 0.0, 0.0))));
            let lc = leader.clone();
            // the feet: the centre's projections, (x, rho cos(phi / 2)) on both planes
            let seed = Box::new(move |s: f64| {
                let p = lc.evaluate_point(s).unwrap();
                let v = p.y * cp + p.z * sp;
                [p.x, p.y, p.x, v]
            });
            Self {
                nets: [nurbs(k, &f1), nurbs(k, &f2)],
                faces: [f1, f2],
                leader,
                law,
                r: [r[0], r[0]],
                // the second plane's normal (0, -sin phi, cos phi) points away from the ball
                side: [1.0, -1.0],
                seed,
            }
        }

        /// The cylinder of radius 3 about z standing on the plane z = 0, the ball of radius
        /// 1 outside it, its centre line the circle of radius 4 at height 1 from 10 to 80
        /// degrees (a rational quadratic arc): the feet's motion is curved.
        fn cylinder(k: &Kernel) -> Self {
            let (big_r, r) = (3.0, 1.0);
            let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
            let mut cyl = Cone::new_cylinder_bounded(
                Vec3::new(0.0, 0.0, -2.0),
                Vec3::X,
                Vec3::Y,
                big_r,
                0.0,
                360.0,
                10.0,
                false,
                false,
                false,
            );
            cyl.make_nurb(k).unwrap();
            let fc = Surface::from(cyl);
            let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
            let leader = BSplineCurve::create_circle_segment(
                3,
                &frame,
                big_r + r,
                10.0,
                80.0,
                NurbCircleParam::Quadratic,
            )
            .unwrap();
            let nc = nurbs(k, &fc);
            let lc = leader.clone();
            let foot_c = move |s: f64| {
                let l = lc.evaluate_point(s).unwrap();
                let q = big_r / (big_r + r);
                Vec3::new(l.x * q, l.y * q, r)
            };
            let (cu, cv) = nearest_uv(&nc, foot_c(0.45));
            let pc = nurbs_partials(
                &nc,
                span(&nc.u, nc.p, nc.nu, cu),
                span(&nc.v, nc.q, nc.nv, cv),
                cu,
                cv,
                1,
            );
            let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
            let out = foot_c(0.45) - Vec3::new(0.0, 0.0, r);
            let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
            let lc = leader.clone();
            let ncs = nc.clone();
            let seed = Box::new(move |s: f64| {
                let l = lc.evaluate_point(s).unwrap();
                let (u, v) = nearest_uv(&ncs, foot_c(s));
                [l.x, l.y, u, v]
            });
            Self {
                nets: [nurbs(k, &fp), nc],
                faces: [fp, fc],
                leader,
                law: None,
                r: [r, r],
                side: [1.0, side_c],
                seed,
            }
        }

        /// The station at s, its series to `order` in s.
        fn station(&self, k: &Kernel, s: f64, order: usize) -> GnStation {
            let law = self.law.as_ref().map(|c| GnLaw::of(c).unwrap());
            let lr = law_series(law.as_ref(), s, order, false).unwrap();
            let [f0, f1] = &self.faces;
            gn_solve_station_counted(
                k,
                [f0, f1],
                &self.leader,
                s,
                (self.seed)(s),
                self.r,
                self.side,
                Trunc::new(order, 0).unwrap(),
                1e-12,
                lr.as_ref(),
            )
            .0
            .unwrap()
        }

        /// A piece's arc terms both ways and the point O they are taken about (the first
        /// station's centre); `None` where the feet over the piece are not enclosed.
        fn terms(&self, k: &Kernel, sa: f64, sb: f64) -> Option<(ArcTermsBoth, [f64; 3])> {
            let law = self.law.as_ref().map(|c| GnLaw::of(c).unwrap());
            let touch = law.is_some();
            let (a, b) = (self.station(k, sa, 0), self.station(k, sb, 0));
            let [f0, f1] = &self.faces;
            let [n0, n1] = &self.nets;
            let ln = NurbsCurve::of(&self.leader).unwrap();
            let (_, series) = gn_fourth_over_series(
                k,
                [n0, n1],
                &ln,
                [f0, f1],
                &self.leader,
                &a,
                &b,
                self.r,
                self.side,
                0,
                law.as_ref(),
                usize::from(touch),
                None,
            )
            .unwrap();
            let ps = series?;
            let o = [0, 1, 2].map(|q| a.v.a[q].c[0][0]);
            let d = Ivl::new(sa, sb) - Ivl::from(ps.mid);
            Some((arc_terms_over(&ps.f, &ps.v, &ps.fm, &ps.vm, d, o, touch), o))
        }

        /// The arc terms at one station, solved in floating point: its jets in point
        /// intervals, so the terms are the rows' values at s to their rounding.
        fn terms_at(&self, k: &Kernel, s: f64, o: [f64; 3]) -> ArcTerms {
            let touch = self.law.is_some();
            let st = self.station(k, s, 4 + usize::from(touch));
            let series = |v: &VSer<f64>, q: usize, up: usize| {
                let mut t = Tay::<Ivl>::zero();
                for i in 0..=TAY {
                    t.set(
                        i,
                        Ivl::from(
                            v.a[q].c[i + up][0] * (if up == 1 { (i + 1) as f64 } else { 1.0 }),
                        ),
                    );
                }
                t
            };
            let rho = [0, 1].map(|f| [0, 1, 2].map(|q| series(&st.f[f], q, 0)));
            let mc = [0, 1, 2].map(|q| series(&st.v, q, 0));
            let dm = touch.then(|| [0, 1, 2].map(|q| series(&st.v, q, 1)));
            arc_terms_of(&rho, mc, dm.as_ref(), o)
        }
    }

    /// The arc's row terms over a piece hold the rows sampled over it, on wide pieces
    /// (fixed seed): at 101 stations solved in floating point, `24 |H_j''''|` and
    /// `24 |W_j''''|` are at most the piece's terms, `W_j` at least its least weight and
    /// `|S - O|` within its `R`. Cases: wedges of two planes at random angles with random
    /// constant, linear and quadratic radius laws (a varying ball: the touch circle), and a
    /// ball rolled between a plane and a cylinder (curved rails); pieces up to 0.6 of the
    /// leader's parameter on the wedges (6 along the edge), up to 0.02 (1.4 degrees) on the
    /// cylinder, where the feet are enclosed no wider. The tighter terms are never looser
    /// than the fork's interval ones, term by term; on the wedges, where the rails and the
    /// centre move together, the interval terms do not prove the rows on most of these
    /// pieces (41 of 42), the Taylor models' do on every one.
    #[test]
    fn arc_terms_hold_their_sampled_rows_on_wide_pieces() {
        let k = Kernel::default();
        let mut next = rng(0x9e37_79b9_7f4a_7c15);
        let (mut judged, mut unenclosed) = (0, 0);
        let (mut old_unproved, mut wedges) = (0, 0);
        for case in 0..48 {
            let (c, wedge) = if case % 8 == 7 {
                (ArcCase::cylinder(&k), false)
            } else {
                let phi = (60.0 + 90.0 * next()).to_radians();
                let r0 = 1.0 + 2.0 * next();
                let r = match case % 3 {
                    0 => [r0; 3],
                    1 => {
                        let r1 = 1.0 + 2.0 * next();
                        [r0, 0.5 * (r0 + r1), r1]
                    }
                    _ => [r0, 1.0 + 2.0 * next(), 1.0 + 2.0 * next()],
                };
                (ArcCase::wedge(&k, phi, r), true)
            };
            // a cylinder's pieces as wide as its feet are enclosed over (their enclosure,
            // not the arc's terms, limits that face: 0.05 of the leader is not enclosed)
            let width = if wedge {
                0.05 + 0.55 * next()
            } else {
                0.005 + 0.015 * next()
            };
            let sa = next() * (1.0 - width);
            let sb = sa + width;
            let Some((both, o)) = c.terms(&k, sa, sb) else {
                // the feet over the piece not enclosed: no row terms to compare
                eprintln!("case {case} (wedge {wedge}): [{sa}, {sb}] not enclosed");
                unenclosed += 1;
                continue;
            };
            let (t, old) = (both.tight, both.old);
            // never looser (a NaN term of the interval series is no bound)
            let upper = |a: f64, b: f64| a <= b || b.is_nan();
            for j in 0..3 {
                assert!(
                    upper(t.x[j], old.x[j]) && upper(t.y[j], old.y[j]),
                    "{case}: {t:?} {old:?}"
                );
                assert!(
                    upper(old.w_lo[j], t.w_lo[j]) || old.w_lo[j].is_nan(),
                    "{case}: {t:?} {old:?}"
                );
            }
            assert!(upper(t.r, old.r), "{case}");
            let proved = t.fold(&mut GnRowTerms {
                x: vec![0.0; 3],
                y: vec![0.0; 3],
                w_lo: vec![f64::INFINITY; 3],
                r: 0.0,
                w: [f64::INFINITY, f64::NEG_INFINITY],
            });
            if wedge {
                wedges += 1;
                assert!(proved, "{case}: [{sa}, {sb}] {t:?}");
                let old_proved = old.x.iter().chain(&old.y).all(|v| v.is_finite())
                    && old.w_lo.iter().all(|&w| w > 0.0);
                if !old_proved {
                    old_unproved += 1;
                }
            }
            if !proved {
                continue;
            }
            for i in 0..=100 {
                let s = sa + (sb - sa) * f64::from(i) / 100.0;
                let at = c.terms_at(&k, s, o);
                // the floating solve's own error (its residual within 1e-12)
                let slack = |v: f64| 1e-8 * (1.0 + v.abs());
                for j in 0..3 {
                    assert!(
                        at.x[j] <= t.x[j] + slack(at.x[j]),
                        "{case} s {s}: x {at:?} {t:?}"
                    );
                    assert!(
                        at.y[j] <= t.y[j] + slack(at.y[j]),
                        "{case} s {s}: y {at:?} {t:?}"
                    );
                    assert!(
                        at.w_lo[j] >= t.w_lo[j] - slack(at.w_lo[j]),
                        "{case} s {s}: w {at:?} {t:?}"
                    );
                }
                assert!(at.r <= t.r + slack(at.r), "{case} s {s}: r {at:?} {t:?}");
                judged += 1;
            }
        }
        eprintln!("{judged} samples, {unenclosed} pieces not enclosed, {old_unproved} of {wedges} wedges unproved by the interval terms");
        assert_eq!(unenclosed, 0);
        assert_eq!(judged, 48 * 101);
        assert!(2 * old_unproved > wedges, "{old_unproved} of {wedges}");
    }

    /// Whether every coordinate of `p` lies in `e`'s, to a rounding slack.
    fn holds(e: V3<Ivl>, p: Vec3) -> bool {
        [(e.x, p.x), (e.y, p.y), (e.z, p.z)].iter().all(|&(i, c)| {
            let slack = 1e-12 * (1.0 + c.abs());
            i.lo - slack <= c && c <= i.hi + slack
        })
    }

    /// #14284: a plane continues past its net's ends in both directions, and its feet's box
    /// there holds the plane's own point `O + u X + v Y` and partials `X`, `Y`, 0; on the
    /// domain the box is `nurbs_partials_over_box`'s.
    #[test]
    fn a_plane_s_box_past_its_ends_is_the_plane() {
        let k = Kernel::default();
        let o = Vec3::new(1.0, 2.0, 3.0);
        let fp = plane(o, Vec3::X, Vec3::Y);
        let ends = EndContinuation::of(&fp);
        assert_eq!(
            ends,
            EndContinuation {
                u: true,
                v: true,
                apex: None
            }
        );
        let n = nurbs(&k, &fp);
        // on the domain: the same enclosure
        let inside =
            nurbs_partials_over_box_ending(&n, ends, 1.0, 2.0, -3.0, -1.0, 2, FEET_BOX_CONTINUITY)
                .unwrap();
        let plain =
            nurbs_partials_over_box(&n, 1.0, 2.0, -3.0, -1.0, 2, FEET_BOX_CONTINUITY).unwrap();
        assert_eq!(inside, plain);
        // past the u end and below the v start (the domain is [-10, 10] in both)
        let (ua, ub, va, vb) = (9.5, 12.0, -11.0, -9.8);
        let e = nurbs_partials_over_box_ending(&n, ends, ua, ub, va, vb, 2, FEET_BOX_CONTINUITY)
            .unwrap();
        for u in [ua, 10.0, 11.0, ub] {
            for v in [va, -10.0, vb] {
                let p = o + Vec3::X * u + Vec3::Y * v;
                assert!(
                    holds(e.at(0, 0), p),
                    "({u}, {v}): {p:?} not in {:?}",
                    e.at(0, 0)
                );
            }
        }
        assert!(holds(e.at(1, 0), Vec3::X) && holds(e.at(0, 1), Vec3::Y));
        for (a, b) in [(2, 0), (1, 1), (0, 2)] {
            assert!(
                holds(e.at(a, b), Vec3::ZERO),
                "P[{a}][{b}]: {:?}",
                e.at(a, b)
            );
        }
    }

    /// #14284: a cylinder continues past its net's ends only along its axis, where its feet's
    /// box holds the cylinder's own line `P(u, v_e) + (v - v_e) h Z` (its height `h` per unit
    /// of v); past a u end it is refused by name, and so is any end of a support whose kind
    /// is not a line (no continuation asked).
    #[test]
    fn a_cylinder_s_box_continues_along_its_axis_only() {
        let k = Kernel::default();
        let height = 10.0;
        let mut cyl = Cone::new_cylinder_bounded(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::X,
            Vec3::Y,
            3.0,
            0.0,
            360.0,
            height,
            false,
            false,
            false,
        );
        cyl.make_nurb(&k).unwrap();
        let fc = Surface::from(cyl);
        let ends = EndContinuation::of(&fc);
        assert_eq!(
            ends,
            EndContinuation {
                u: false,
                v: true,
                apex: None
            }
        );
        let n = nurbs(&k, &fc);
        assert_eq!(n.q, 1, "the cylinder's net is of degree 1 along its axis");
        let (u0, u1) = (knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize));
        let (v0, v1) = (knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize));
        let rate = height / (v1 - v0);
        for u in [u0 + 0.3 * (u1 - u0), u0 + 0.7 * (u1 - u0)] {
            let (va, vb) = (v1 - 0.5 * (v1 - v0), v1 + 0.2 * (v1 - v0));
            let e = nurbs_partials_over_box_ending(&n, ends, u, u, va, vb, 1, FEET_BOX_CONTINUITY)
                .unwrap();
            let edge = fc.evaluate_point(&k, Vec2::new(u, v1)).unwrap();
            for v in [va, v1, 0.5 * (v1 + vb), vb] {
                // on the domain the face itself, past it the cylinder's line through its edge
                let p = if v <= v1 {
                    fc.evaluate_point(&k, Vec2::new(u, v)).unwrap()
                } else {
                    edge + Vec3::Z * ((v - v1) * rate)
                };
                assert!(
                    holds(e.at(0, 0), p),
                    "({u}, {v}): {p:?} not in {:?}",
                    e.at(0, 0)
                );
            }
            assert!(holds(e.at(0, 1), Vec3::Z * rate), "{:?}", e.at(0, 1));
        }
        // past a u end: no continuation there
        let past_u = nurbs_partials_over_box_ending(
            &n,
            ends,
            u1 - 0.1,
            u1 + 0.1,
            v0,
            v1,
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            matches!(past_u, Err(SolidError::Unsupported { reason, .. }) if reason == FOOT_PAST_END),
            "{past_u:?}"
        );
        // a support whose kind continues nothing
        let none = nurbs_partials_over_box_ending(
            &n,
            EndContinuation::default(),
            u0,
            u0,
            v1,
            v1 + 0.1,
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            matches!(none, Err(SolidError::Unsupported { reason, .. }) if reason == FOOT_PAST_END),
            "{none:?}"
        );
    }

    /// #14284: of the failed evaluations inside the certificate, only a foot past its face's
    /// domain end (named, with its piece and box) is passed on by `gn_try!`; any other still
    /// proves nothing.
    #[test]
    fn only_a_foot_past_its_end_passes_gn_try() {
        fn through(e: SolidError) -> Result<Option<()>> {
            let () = gn_try!(Err::<(), SolidError>(e), None);
            Ok(Some(()))
        }
        let k = Kernel::default();
        let n = nurbs(&k, &plane(Vec3::ZERO, Vec3::X, Vec3::Y));
        let raw = SolidError::Unsupported {
            operation: OP,
            reason: FOOT_PAST_END,
        };
        let named = name_foot_past_end(raw, 1, Ivl::new(2.0, 3.0), [[11.0, 12.0], [0.0, 1.0]], &n);
        assert!(is_foot_past_domain_end(&named), "{named:?}");
        let text = named.to_string();
        assert!(
            text.contains("a foot past its face's domain end") && text.contains("support 1"),
            "{text}"
        );
        assert!(matches!(through(named), Err(e) if is_foot_past_domain_end(&e)));
        let other = SolidError::Unsupported {
            operation: OP,
            reason: "a box crosses a knot line where the face or curve is not continuous enough",
        };
        assert!(matches!(through(other), Ok(None)));
        assert!(matches!(through(SolidError::sm_err(OP)), Ok(None)));
    }

    /// #14284, the two planes of `two_plane_span_bound_is_zero` at a square corner, the ball
    /// of radius r on their edge along y, enclosed over a span whose feet run past the planes'
    /// domain end (y = 10, here L s for s in [0.8, 0.9] with L = 12): with both supports
    /// planes, the feet are proved on the planes continued and hold their closed form
    /// (`(r, L s)` and `(L s, r)`); with the second support a plain B-spline (the same net,
    /// not a definition's form, which continues nothing) the span is refused by
    /// `FootPastDomainEnd`, naming that support and the piece, and nothing is certified.
    #[test]
    fn a_foot_past_a_free_form_end_is_refused_and_a_plane_s_is_proved() {
        let k = Kernel::default();
        let f1 = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let f2 = plane(Vec3::ZERO, Vec3::Y, Vec3::Z);
        let free = Surface::from(as_b_spline(&k, &f2).unwrap().unwrap().into_owned());
        assert_eq!(EndContinuation::of(&free), EndContinuation::default());
        let (r, len) = (2.0, 12.0);
        let leader =
            BSplineCurve::create_line_segment(3, Vec3::new(r, 0.0, r), Vec3::new(r, len, r))
                .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (n1, n2) = (nurbs(&k, &f1), nurbs(&k, &f2));
        let (rr, side) = ([r, r], [1.0, 1.0]);
        let (sa, sb) = (0.8, 0.9);
        let a = station(
            &k,
            [&f1, &f2],
            &leader,
            sa,
            [r, len * sa, len * sa, r],
            rr,
            side,
        );
        let b = station(
            &k,
            [&f1, &f2],
            &leader,
            sb,
            [r, len * sb, len * sb, r],
            rr,
            side,
        );
        let at = SpanAt {
            s: Ivl::new(sa, sb),
            xa: core::array::from_fn(|c| a.x[c].at(0, 0)),
            xb: core::array::from_fn(|c| b.x[c].at(0, 0)),
            ku: [0; 2],
            kv: [0; 2],
            leader: None,
            piecewise: true,
        };
        let enclose =
            |ends| gn_enclose_over(&k, [&n1, &n2], &ln, &at, rr, side, 0, 2, None, None, ends);
        // two planes: proved past the end, the closed form held
        let e = enclose([EndContinuation::of(&f1), EndContinuation::of(&f2)])
            .unwrap()
            .expect("proved");
        for (q, exact) in [(0, r), (3, r)] {
            assert!(contains(e.x[q].c[0][0], exact), "{q}: {:?}", e.x[q].c[0][0]);
        }
        for q in [1, 2] {
            let x = e.x[q].c[0][0];
            assert!(
                x.lo <= len * sa && len * sb <= x.hi && x.hi > 10.0,
                "{q} {x:?}"
            );
            assert!(contains(e.x[q].c[1][0], len), "{q}");
        }
        // the second support free-form: refused by name, nothing proved
        let refused = enclose([EndContinuation::of(&f1), EndContinuation::of(&free)]);
        let Err(SolidError::Failed(f)) = &refused else {
            panic!("{refused:?}")
        };
        let crate::error::Reason::FootPastDomainEnd {
            support, s, domain, ..
        } = &f.reason
        else {
            panic!("{f:?}")
        };
        assert_eq!((*support, *s), (1, [sa, sb]));
        assert_eq!(domain[0], [-10.0, 10.0]);
        // and without the ends asked, as every caller but a definition's supports
        let plain = enclose([EndContinuation::default(); 2]);
        assert!(
            matches!(&plain, Err(e) if is_foot_past_domain_end(e)),
            "{plain:?}"
        );
    }

    /// A ball rolled outside a curved face standing on the plane through `o` (normal z):
    /// a cylinder of radius `big_r` about the vertical through `o`, or a sphere of radius
    /// `big_r` resting on the plane there. The ball's centre line is a circle about that
    /// vertical at height r (a rational quadratic arc from `deg[0]` to `deg[1]`), and the
    /// feet are in closed form: on the plane below the centre, on the curved face along
    /// the line from the axis (cylinder) or the sphere's centre to it. Returns the faces,
    /// their nets, the leader, the sides and the closed-form feet's points.
    #[allow(clippy::type_complexity)]
    fn rolled_ball(
        k: &Kernel,
        sphere: bool,
        o: Vec3,
        big_r: f64,
        r: f64,
        deg: [f64; 2],
    ) -> (
        [Surface; 2],
        [Nurbs; 2],
        BSplineCurve,
        [f64; 2],
        Box<dyn Fn(f64) -> [Vec3; 2]>,
    ) {
        use crate::surface::sphere::{Sphere, SphereOptions};
        let fp = plane(o, Vec3::X, Vec3::Y);
        let (fc, rho, centre) = if sphere {
            let c = o + Vec3::new(0.0, 0.0, big_r);
            let mut sp = Sphere::new(
                k,
                c,
                Vec3::X,
                Vec3::Y,
                Extent2d::from_bounds(deg[0] - 10.0, -85.0, deg[1] + 10.0, 20.0),
                big_r,
                SphereOptions::default(),
            )
            .unwrap();
            sp.make_nurb(k).unwrap();
            (Surface::from(sp), 2.0 * (big_r * r).sqrt(), c)
        } else {
            let mut cyl = Cone::new_cylinder_bounded(
                o + Vec3::new(0.0, 0.0, -2.0),
                Vec3::X,
                Vec3::Y,
                big_r,
                0.0,
                360.0,
                10.0,
                false,
                false,
                false,
            );
            cyl.make_nurb(k).unwrap();
            (Surface::from(cyl), big_r + r, o)
        };
        let frame = Axis2Placement::new(o + Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            rho,
            deg[0],
            deg[1],
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let lc = leader.clone();
        let feet = move |s: f64| {
            let l = lc.evaluate_point(s).unwrap();
            let on_plane = Vec3::new(l.x, l.y, o.z);
            let on_face = if sphere {
                let d = l - centre;
                centre + d * (big_r / d.length())
            } else {
                let d = Vec3::new(l.x - o.x, l.y - o.y, 0.0);
                Vec3::new(o.x, o.y, l.z) + d * (big_r / d.length())
            };
            [on_plane, on_face]
        };
        let nc = nurbs(k, &fc);
        let [_, fm] = feet(0.5);
        let (cu, cv) = nearest_uv(&nc, fm);
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = leader.evaluate_point(0.5).unwrap() - fm;
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        (
            [fp.clone(), fc.clone()],
            [nurbs(k, &fp), nc],
            leader,
            [1.0, side_c],
            Box::new(feet),
        )
    }

    /// The feet's Taylor models over a piece (`super::feet_tm::gn_enclose_tm`) hold every
    /// station sampled over it, on curved faces far from the origin (fixed seed): balls
    /// rolled between a plane and a rational cylinder or sphere (doubly curved), each
    /// moved up to 60 from the origin, on pieces of 0.0003 to 0.25 of the leader (an arc of
    /// up to 80 degrees), the arc's order (n = 0) and a G2 section's (n = 2). At 101 stations
    /// solved in floating point along each piece, the feet's parameters and their Taylor
    /// coefficients in s and eps (and for n = 0 the feet's points and the ball's centre)
    /// lie in the models' ranges. The models are proved on every piece; the interval
    /// enclosure fails on many of them, and where it holds, its top coefficients (order 6,
    /// the Hermite remainder's) are orders of magnitude wider.
    #[test]
    fn feet_taylor_models_hold_every_sampled_station_on_curved_faces() {
        let k = Kernel::default();
        let mut next = rng(0x5bd1_e995_a3c6_4f2b);
        let order = 4 + TAYLOR;
        let (mut judged, mut old_failed, mut cases) = (0, 0, 0);
        let mut ratios = Vec::new();
        for case in 0..24 {
            let sphere = case % 2 == 1;
            let n = if case % 4 < 2 { 0 } else { 2 };
            let o = Vec3::new(
                120.0 * next() - 60.0,
                120.0 * next() - 60.0,
                40.0 * next() - 20.0,
            );
            let big_r = 2.0 + 3.0 * next();
            let r = 0.4 + 1.1 * next();
            let d0 = 10.0 + 10.0 * next();
            let deg = [d0, d0 + 50.0 + 30.0 * next()];
            let (faces, nets, leader, side, feet) = rolled_ball(&k, sphere, o, big_r, r, deg);
            let [f0, f1] = &faces;
            let [n0, n1] = &nets;
            let ln = NurbsCurve::of(&leader).unwrap();
            assert_eq!(ln.u.len(), 6, "one quadratic arc: {:?}", ln.u);
            let rr = [r, r];
            // every third piece narrow, where the interval enclosure holds too
            let width = if case % 3 == 0 {
                0.0003 + 0.0012 * next()
            } else {
                0.02 + 0.23 * next()
            };
            let sa = next() * (1.0 - width);
            let sb = sa + width;
            let seed_at = |s: f64| {
                let [fp, fc] = feet(s);
                let (pu, pv) = nearest_uv(n0, fp);
                let (u, v) = nearest_uv(n1, fc);
                [pu, pv, u, v]
            };
            let solve = |s: f64, seed: [f64; 4], tr: Trunc| {
                super::super::station::gn_solve_station(
                    &k,
                    [f0, f1],
                    &leader,
                    s,
                    seed,
                    rr,
                    side,
                    tr,
                    1e-12,
                )
            };
            let point = Trunc::new(0, 0).unwrap();
            let x_of = |st: &GnStation| [0, 1, 2, 3].map(|c| st.x[c].c[0][0]);
            let a = solve(sa, seed_at(sa), point).unwrap();
            let b = solve(sb, seed_at(sb), point).unwrap();
            let m = 0.5 * (sa + sb);
            let smid = solve(m, seed_at(m), point).unwrap();
            let e4 = gn_enclose(
                &k,
                [n0, n1],
                &ln,
                &smid,
                &smid,
                rr,
                side,
                n,
                order - 1,
                None,
                None,
            )
            .unwrap()
            .expect("the middle's point enclosure");
            let at = span_at([n0, n1], &a, &b);
            let tm = super::super::feet_tm::gn_enclose_tm(
                &k,
                [n0, n1],
                &ln,
                &at,
                rr,
                side,
                n,
                order,
                None,
                m,
                &e4.x,
            )
            .unwrap()
            .unwrap_or_else(|| panic!("{case}: the models over [{sa}, {sb}] not proved"));
            cases += 1;
            // the fourth terms the piece takes: finite from the models, and where the
            // interval enclosure holds too, met with its terms, never above them
            let (ft, _) = fourth_of(&tm, &e4, sb - sa, n, m);
            let (tm4, tuv4, _, _, tuvf, _) = ft;
            let finite = |v: f64| v.is_finite() && v >= 0.0;
            assert!(
                tuv4.iter().chain(&tuvf).all(|&v| finite(v)),
                "{case}: {ft:?}"
            );
            assert!(tm4.iter().all(|f| f.iter().take(n + 1).all(|&v| finite(v))));
            match gn_enclose(&k, [n0, n1], &ln, &a, &b, rr, side, n, order, None, None).unwrap() {
                Some(old) => {
                    for c in 0..4 {
                        let (wo, wt) = (old.x[c].c[order][0].mag(), tm.x[c].c[order][0].mag());
                        if wt > 0.0 {
                            ratios.push(wo / wt);
                        }
                    }
                    let (fo, _) = fourth_of(&old, &e4, sb - sa, n, m);
                    let met = meet_fourth(&fo, &ft);
                    let (om4, ouv4, om4_lo, ouv4_lo, ouvf, ouvf_lo) = fo;
                    let (mm4, muv4, mm4_lo, muv4_lo, muvf, muvf_lo) = met;
                    let upper = om4.iter().flatten().zip(mm4.iter().flatten());
                    let upper = upper
                        .chain(ouv4.iter().zip(&muv4))
                        .chain(ouvf.iter().zip(&muvf));
                    for (&o, &t) in upper {
                        assert!(
                            t <= o || o.is_nan(),
                            "{case}: met {t:e} over the interval {o:e}"
                        );
                    }
                    // the least values are the middle's, both ways
                    assert_eq!((om4_lo, ouv4_lo, ouvf_lo), (mm4_lo, muv4_lo, muvf_lo));
                }
                None => old_failed += 1,
            }
            // the floating solve's jets go as far as the leader's evaluation does
            let kk_max = n.saturating_sub(1);
            let top = (0..=order)
                .rev()
                .find(|&o| solve(sa, x_of(&a), Trunc::new(o, kk_max).unwrap()).is_ok())
                .unwrap();
            assert!(top >= 3, "{top}");
            let jets = Trunc::new(top, kk_max).unwrap();
            let mut seed = x_of(&a);
            for i in 0..=100 {
                let s = sa + (sb - sa) * f64::from(i) / 100.0;
                let st = solve(s, seed, jets).unwrap();
                seed = x_of(&st);
                // the floating solve's own error (its residual within 1e-12)
                let slack = |x: f64| 1e-8 * (1.0 + x.abs());
                let holds = |iv: Ivl, x: f64| iv.lo - slack(x) <= x && x <= iv.hi + slack(x);
                for (q, (xs, xe)) in st.x.iter().zip(&tm.x).enumerate() {
                    for o in 0..=top {
                        for kk in 0..=kk_max {
                            let (x, iv) = (xs.c[o][kk], xe.c[o][kk]);
                            assert!(
                                holds(iv, x),
                                "{case} s {s}: x{q} c[{o}][{kk}] {x:e} outside {iv:?}"
                            );
                            judged += 1;
                        }
                    }
                }
                if n == 0 {
                    let pts = st.f.iter().chain([&st.v]).zip(tm.f.iter().chain([&tm.v]));
                    for (vs, ve) in pts {
                        for (cs, ce) in vs.a.iter().zip(&ve.a) {
                            for o in 0..=top {
                                let (x, iv) = (cs.c[o][0], ce.c[o][0]);
                                assert!(holds(iv, x), "{case} s {s}: point c[{o}] {x:e} {iv:?}");
                                judged += 1;
                            }
                        }
                    }
                }
            }
        }
        ratios.sort_by(f64::total_cmp);
        let median = ratios.get(ratios.len() / 2).copied().unwrap_or(f64::NAN);
        eprintln!(
            "{cases} pieces, {judged} coefficients judged; the interval enclosure failed on \
             {old_failed}, elsewhere its top coefficients {median:e} times the models' (median)"
        );
        assert_eq!(cases, 24);
        assert!(old_failed >= 8, "{old_failed}");
        assert!(ratios.len() >= 4 * 4, "{}", ratios.len());
        assert!(median > 100.0, "{median}");
    }

    /// A cone of `bot` radius at z = 0 to `top` at z = `height` about the z axis, a whole
    /// turn, its NURB made.
    fn upright_cone(k: &Kernel, bot: f64, top: f64, height: f64) -> Surface {
        let mut c = Cone::new_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            bot,
            top,
            0.0,
            360.0,
            height,
            false,
            false,
            false,
        );
        c.make_nurb(k).unwrap();
        Surface::from(c)
    }

    /// #14284's cone: a cone continues past its net's ends along its generator only, where
    /// its feet's box holds the cone's own point `r(h) (cos t X + sin t Y) + h Z` past the
    /// top (`r(h)` its radius at height `h`, `t` the edge point's angle), and `S_v` holds the
    /// generator; past a u end it is refused, and so is a continued part that reaches the
    /// apex (the cone's one point on its axis), and so is a box wholly past the apex, on the
    /// other nappe.
    #[test]
    fn a_cone_s_box_continues_along_its_generator() {
        let k = Kernel::default();
        let (bot, top, height) = (5.0, 3.0, 4.0);
        let fc = upright_cone(&k, bot, top, height);
        let ends = EndContinuation::of(&fc);
        assert!(!ends.u && ends.v && ends.apex.is_some(), "{ends:?}");
        let n = nurbs(&k, &fc);
        assert_eq!(n.q, 1, "the cone's net is of degree 1 along its generator");
        let (u0, u1) = (knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize));
        let (v0, v1) = (knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize));
        // the cone's own point: height along v (an affine map of the net's v, read at its ends)
        let at = |u: f64, v: f64| fc.evaluate_point(&k, Vec2::new(u, v)).unwrap();
        for u in [u0 + 0.3 * (u1 - u0), u0 + 0.7 * (u1 - u0)] {
            let (h0, h1) = (at(u, v0).z, at(u, v1).z);
            let rate = (h1 - h0) / (v1 - v0);
            let edge = at(u, v1);
            let t = edge.y.atan2(edge.x);
            let radius = |h: f64| bot + (top - bot) * h / height;
            let cone = |h: f64| Vec3::new(radius(h) * t.cos(), radius(h) * t.sin(), h);
            let (va, vb) = (v1 - 0.5 * (v1 - v0), v1 + 0.2 * (v1 - v0));
            let e = nurbs_partials_over_box_ending(&n, ends, u, u, va, vb, 1, FEET_BOX_CONTINUITY)
                .unwrap();
            for v in [va, v1, 0.5 * (v1 + vb), vb] {
                let p = cone(h1 + (v - v1) * rate);
                assert!(
                    holds(e.at(0, 0), p),
                    "({u}, {v}): {p:?} not in {:?}",
                    e.at(0, 0)
                );
            }
            let generator = cone(h1 + rate) - cone(h1);
            assert!(
                holds(e.at(0, 1), generator),
                "{:?} against {generator:?}",
                e.at(0, 1)
            );
        }
        // past a u end: no continuation there
        let past_u = nurbs_partials_over_box_ending(
            &n,
            ends,
            u1 - 0.1,
            u1 + 0.1,
            v0,
            v1,
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            matches!(past_u, Err(SolidError::Unsupported { reason, .. }) if reason == FOOT_PAST_END),
            "{past_u:?}"
        );
        // a box wholly past the top: short of the apex it is the cone continued; wholly past
        // the apex (heights 11 to 12, the apex at 10: on the other nappe, off the axis) it is
        // refused, since the way back to the top crosses the axis
        let mid_u = u0 + 0.3 * (u1 - u0);
        let (h0, h1) = (at(mid_u, v0).z, at(mid_u, v1).z);
        let v_at = |h: f64| v0 + (h - h0) * (v1 - v0) / (h1 - h0);
        let apex = height * bot / (bot - top);
        assert!((apex - 10.0).abs() < 1e-12);
        let short_of = nurbs_partials_over_box_ending(
            &n,
            ends,
            mid_u,
            mid_u,
            v_at(5.0),
            v_at(6.0),
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            short_of.is_ok(),
            "past the top, short of the apex: {short_of:?}"
        );
        let beyond = nurbs_partials_over_box_ending(
            &n,
            ends,
            mid_u,
            mid_u,
            v_at(11.0),
            v_at(12.0),
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            matches!(beyond, Err(SolidError::Unsupported { reason, .. }) if reason == FOOT_PAST_END),
            "wholly past the apex: {beyond:?}"
        );
        // a cone whose apex lies just past its top: a continuation reaching it is refused
        let pointed = upright_cone(&k, 5.0, 0.05, 4.0);
        let pn = nurbs(&k, &pointed);
        let pe = EndContinuation::of(&pointed);
        let (w0, w1) = (knot(&pn.v, pn.q as isize), knot(&pn.v, pn.nv as isize));
        let pu = knot(&pn.u, pn.p as isize)
            + 0.3 * (knot(&pn.u, pn.nu as isize) - knot(&pn.u, pn.p as isize));
        let short = nurbs_partials_over_box_ending(
            &pn,
            pe,
            pu,
            pu,
            w1 - 0.1 * (w1 - w0),
            w1 + 0.001 * (w1 - w0),
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(short.is_ok(), "short of the apex: {short:?}");
        let through = nurbs_partials_over_box_ending(
            &pn,
            pe,
            pu,
            pu,
            w1 - 0.1 * (w1 - w0),
            w1 + 0.1 * (w1 - w0),
            1,
            FEET_BOX_CONTINUITY,
        );
        assert!(
            matches!(through, Err(SolidError::Unsupported { reason, .. }) if reason == FOOT_PAST_END),
            "{through:?}"
        );
    }

    /// #14284's cone, a closed form: a ball of radius r between the plane z = 0 and an
    /// upright cone of semi-angle a (radius `bot` at z = 0) about the z axis, rolled round
    /// it. Its centre is the circle of radius `rho = bot + r (1 - sin a) / cos a` at height
    /// r; its feet are `(rho, theta, 0)` on the plane and `(rho - r cos a, theta, r (1 -
    /// sin a))` on the cone, at the height where the cone ends (its top), so the feet's box
    /// crosses the cone net's v end. With the cone continued (its generator), the feet are
    /// proved and hold the closed form; with the cone given as a plain B-spline (the same
    /// net, continuing nothing), the span is refused `FootPastDomainEnd`, nothing certified.
    /// (No procedural round of a cone ends where its cone ends: a cone standing on a plate
    /// rounds its root round a closed circle, so this is the end's case at the certificate.)
    #[test]
    fn a_ball_between_a_plane_and_a_cone_is_proved_at_the_cone_s_end() {
        let k = Kernel::default();
        let (bot, r) = (5.0, 2.0);
        let a = 30.0_f64.to_radians();
        let foot_h = r * (1.0 - a.sin());
        let top = bot - foot_h * a.tan();
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let fc = upright_cone(&k, bot, top, foot_h);
        let rho = bot + r * (1.0 - a.sin()) / a.cos();
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            rho,
            10.0,
            80.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = (rho - r * a.cos()) / rho;
            Vec3::new(l.x * q, l.y * q, foot_h)
        };
        // the sides: the plane's normal is +z, towards the ball; the cone's by its net
        let (cu, cv) = nearest_uv(&nc, foot_c(0.45));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = Vec3::new(foot_c(0.45).x, foot_c(0.45).y, 0.0);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let st = |s: f64| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            station(&k, faces, &leader, s, [l.x, l.y, u, v], rr, side)
        };
        // the span cut as `gn_certify_span` cuts it, `2^level` equal pieces, the level
        // raised to `SPAN_MAX_LEVEL` until every piece is proved. At level 0 (0.02 long)
        // Krawczyk's test fails (`gn_enclose` proves nothing) with the foot inside the cone
        // (a cone twice as tall) as at its end: the span's length, not the end. Past the end
        // the box is enclosed on the cone's own generator (`nurbs_partials_over_box_ending`,
        // which `partials_on` takes for a definition's box too), the end span continued.
        let (s0, s1) = (0.4, 0.42);
        let piece = |lo: f64, hi: f64| {
            let (sta, stb) = (st(lo), st(hi));
            SpanAt {
                s: Ivl::new(lo, hi),
                xa: core::array::from_fn(|c| sta.x[c].at(0, 0)),
                xb: core::array::from_fn(|c| stb.x[c].at(0, 0)),
                ku: [0; 2],
                kv: [0; 2],
                leader: None,
                piecewise: true,
            }
        };
        let enclose = |at: &SpanAt, ends| {
            gn_enclose_over(&k, [&np, &nc], &ln, at, rr, side, 0, 2, None, None, ends)
        };
        let continued = [EndContinuation::of(&fp), EndContinuation::of(&fc)];
        let free = Surface::from(as_b_spline(&k, &fc).unwrap().unwrap().into_owned());
        let plain = [EndContinuation::of(&fp), EndContinuation::of(&free)];
        let mut proved = None;
        for level in 0..=SPAN_MAX_LEVEL {
            let pieces = 1_u32 << level;
            let ends: Vec<f64> = (0..=pieces)
                .map(|i| s0 + f64::from(i) / f64::from(pieces) * (s1 - s0))
                .collect();
            let es: Option<Vec<_>> = ends
                .windows(2)
                .map(|w| {
                    let at = piece(w[0], w[1]);
                    enclose(&at, continued).unwrap().map(|e| (at, e))
                })
                .collect();
            if let Some(es) = es {
                proved = Some((level, es));
                break;
            }
        }
        let (level, es) = proved.expect("proved by SPAN_MAX_LEVEL");
        assert!(level >= 1, "the 0.02 span is proved at level 0");
        for (at, e) in &es {
            for s in [at.s.lo, at.s.hi] {
                let (l, c) = (lpt(s), foot_c(s));
                let fp_s = Vec3::new(l.x, l.y, 0.0);
                for (f, want) in [(0, fp_s), (1, c)] {
                    let x = &e.f[f];
                    let got = V3 {
                        x: x.a[0].c[0][0],
                        y: x.a[1].c[0][0],
                        z: x.a[2].c[0][0],
                    };
                    assert!(
                        holds(got, want),
                        "level {level}: foot {f} at s {s}: {want:?} not in {got:?}"
                    );
                }
            }
            // the same net continuing nothing: refused by name on every piece
            let refused = enclose(at, plain);
            let Err(SolidError::Failed(f)) = &refused else {
                panic!("{refused:?}")
            };
            let crate::error::Reason::FootPastDomainEnd { support, .. } = &f.reason else {
                panic!("{f:?}")
            };
            assert_eq!(*support, 1);
        }
    }

    /// The C1 quarter knot (#14611's pipe, tangency-labs/solidcraft#14284 follow-up), a
    /// closed form: the ball of #14284's cone test between z = 0 and an upright cone of
    /// semi-angle a, rolled from 60 to 120 degrees, its cone foot `(rho - r cos a, theta,
    /// r (1 - sin a))` crossing the cone net's quarter knot `u = 0.25` (multiplicity 2,
    /// degree 2: C0 by count, C1 only to rounding) at theta = 90 degrees. The cone is twice
    /// as tall as the foot's height, so no end is reached. For a pointwise use (order 1 in
    /// s) the feet are proved across the knot and hold the closed form; for a mean-value use
    /// of the definition (order 2) the box is refused, nothing proved.
    #[test]
    fn a_foot_across_a_circle_s_quarter_knot_is_proved_for_a_pointwise_use() {
        let k = Kernel::default();
        let (bot, r) = (5.0, 2.0);
        let a = 30.0_f64.to_radians();
        let foot_h = r * (1.0 - a.sin());
        let height = 2.0 * foot_h;
        let top = bot - height * a.tan();
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let fc = upright_cone(&k, bot, top, height);
        let rho = bot + r * (1.0 - a.sin()) / a.cos();
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            rho,
            60.0,
            120.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = (rho - r * a.cos()) / rho;
            Vec3::new(l.x * q, l.y * q, foot_h)
        };
        // the quarter knot, doubled, in the cone net's u
        let quarter = 0.25;
        assert_eq!(nc.u.iter().filter(|&&x| x == quarter).count(), 2);
        let (cu, cv) = nearest_uv(&nc, foot_c(0.3));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = Vec3::new(foot_c(0.3).x, foot_c(0.3).y, 0.0);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let st = |s: f64| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            station(&k, faces, &leader, s, [l.x, l.y, u, v], rr, side)
        };
        let piece = |lo: f64, hi: f64| {
            let (sta, stb) = (st(lo), st(hi));
            SpanAt {
                s: Ivl::new(lo, hi),
                xa: core::array::from_fn(|c| sta.x[c].at(0, 0)),
                xb: core::array::from_fn(|c| stb.x[c].at(0, 0)),
                ku: [0; 2],
                kv: [0; 2],
                leader: None,
                piecewise: true,
            }
        };
        let ends = [EndContinuation::of(&fp), EndContinuation::of(&fc)];
        let enclose = |at: &SpanAt, order: usize| {
            gn_enclose_over(
                &k,
                [&np, &nc],
                &ln,
                at,
                rr,
                side,
                0,
                order,
                None,
                None,
                ends,
            )
        };
        // a span whose middle piece's feet cross the knot, cut as `gn_certify_span` cuts it
        let (s0, s1) = (0.493, 0.513);
        let mut proved = None;
        for level in 0..=SPAN_MAX_LEVEL {
            let pieces = 1_u32 << level;
            let cuts: Vec<f64> = (0..=pieces)
                .map(|i| s0 + f64::from(i) / f64::from(pieces) * (s1 - s0))
                .collect();
            let es: Option<Vec<_>> = cuts
                .windows(2)
                .map(|w| {
                    let at = piece(w[0], w[1]);
                    enclose(&at, 1).unwrap().map(|e| (at, e))
                })
                .collect();
            if let Some(es) = es {
                proved = Some((level, es));
                break;
            }
        }
        let (level, es) = proved.expect("proved by SPAN_MAX_LEVEL");
        let mut crossing = 0;
        for (at, e) in &es {
            let u = e.x[2].at(0, 0);
            if u.lo < quarter && quarter < u.hi {
                crossing += 1;
                // the mean-value use: refused across the knot
                assert!(
                    enclose(at, 2).unwrap().is_none(),
                    "level {level}: order 2 proved"
                );
            }
            for s in [at.s.lo, at.s.hi] {
                let (l, c) = (lpt(s), foot_c(s));
                let fp_s = Vec3::new(l.x, l.y, 0.0);
                for (f, want) in [(0, fp_s), (1, c)] {
                    let x = &e.f[f];
                    let got = V3 {
                        x: x.a[0].c[0][0],
                        y: x.a[1].c[0][0],
                        z: x.a[2].c[0][0],
                    };
                    assert!(
                        holds(got, want),
                        "level {level}: foot {f} at s {s}: {want:?} not in {got:?}"
                    );
                }
            }
        }
        assert!(crossing >= 1, "level {level}: no piece across the knot");
        // its reason at order 2: the knot line is below C^2 by count
        let refused = nurbs_partials_over_box(&nc, 0.24, 0.26, 0.4, 0.6, 2, FEET_BOX_CONTINUITY);
        assert!(
            matches!(
                refused,
                Err(SolidError::Unsupported {
                    reason:
                        "a box crosses a knot line where the face or curve is not continuous enough",
                    ..
                })
            ),
            "{refused:?}"
        );
    }

    /// Krawczyk's `K` against its box: strictly inside, apart in one coordinate, straddling,
    /// and touching an end exactly (not inside: the strictness is pinned).
    #[test]
    fn k_against_its_box_is_inside_off_or_straddling() {
        let x = [Ivl::new(0.0, 1.0); 4];
        let k = |c: Ivl| {
            [
                Ivl::new(0.25, 0.75),
                Ivl::new(0.25, 0.75),
                c,
                Ivl::new(0.25, 0.75),
            ]
        };
        assert_eq!(classify_k(&k(Ivl::new(0.25, 0.75)), &x), KVerdict::Inside);
        assert_eq!(classify_k(&k(Ivl::new(1.5, 2.0)), &x), KVerdict::Off);
        assert_eq!(classify_k(&k(Ivl::new(-2.0, -0.5)), &x), KVerdict::Off);
        assert_eq!(classify_k(&k(Ivl::new(0.5, 1.5)), &x), KVerdict::Straddles);
        assert_eq!(classify_k(&k(Ivl::new(0.25, 1.0)), &x), KVerdict::Straddles);
        assert_eq!(classify_k(&k(Ivl::new(0.0, 0.75)), &x), KVerdict::Straddles);
        // touching from outside meets the box: not off
        assert_eq!(classify_k(&k(Ivl::new(1.0, 2.0)), &x), KVerdict::Straddles);
        assert_eq!(
            classify_k(&k(Ivl::new(f64::NAN, 0.5)), &x),
            KVerdict::Straddles
        );
    }

    /// The cell-by-cell decision (`enclose_by_cells`) on synthetic cells: a face folded at a
    /// simple knot (`C^0` by count) whose two pieces each give the decision a crafted
    /// outcome. Two roots on their own cells and apart are refused as two feet; a cell not
    /// decided over `H` is refused; roots that meet at the line are hulled; a root off its own
    /// cell, or an excluded cell, is dropped; nothing proved in the first pass is `None`.
    #[test]
    fn cells_across_a_knot_line_are_decided_or_refused_by_name() {
        let net = |u: Vec<f64>, rows: &[[f64; 3]]| {
            let mut pw = Vec::new();
            for row in rows {
                pw.extend([row[0], row[1], row[2], 1.0]);
            }
            Nurbs {
                p: 1,
                q: 1,
                nu: rows.len() / 2,
                nv: 2,
                u,
                v: vec![-3.0, -3.0, 3.0, 3.0],
                pw,
            }
        };
        let wall = net(
            vec![-3.0, -3.0, 3.0, 3.0],
            &[
                [-1.0, -3.0, -3.0],
                [-1.0, -3.0, 3.0],
                [-1.0, 3.0, -3.0],
                [-1.0, 3.0, 3.0],
            ],
        );
        let fold = net(
            vec![-1.0, -1.0, 0.0, 1.0, 1.0],
            &[
                [-1.0, -3.0, 0.0],
                [-1.0, 3.0, 0.0],
                [0.0, -3.0, 0.0],
                [0.0, 3.0, 0.0],
                [1.0, -3.0, -0.5],
                [1.0, 3.0, -0.5],
            ],
        );
        let left = FaceCell::Piece(1, 1);
        let bx = |u: [f64; 2]| {
            [
                Ivl::new(-0.01, 0.01),
                Ivl::new(0.99, 1.01),
                Ivl::new(u[0], u[1]),
                Ivl::new(-0.01, 0.01),
            ]
        };
        let x0 = bx([-0.01, 0.01]);
        let proved = |u: [f64; 2]| {
            let b = bx(u);
            let c = |i: usize| Ser::constant(b[i]);
            let p = V3 {
                x: Ivl::from(u[0]),
                y: IZ,
                z: IZ,
            };
            let e = Enclosure {
                f: [VSer::constant(p), VSer::constant(p)],
                x: [c(0), c(1), c(2), c(3)],
                ds: [[1.0; 2]; 2],
                ds_lo: [[1.0; 2]; 2],
                v: VSer::constant(p),
            };
            Inner::Proved(Box::new((e, b)))
        };
        // the first pass proves the left cell near the line; the second pass's outcome per
        // cell is the case's
        let run = |second: &dyn Fn(FaceCell) -> Inner| {
            enclose_by_cells(
                [&wall, &fold],
                [EndContinuation::default(); 2],
                Ivl::new(0.0, 1.0),
                &x0,
                [false, true],
                |cells, over| {
                    assert_eq!(cells[0], FaceCell::Hull);
                    Ok(match over {
                        None if cells[1] == left => proved([-0.005, -0.004]),
                        None => Inner::Unproved,
                        Some(h) => {
                            assert!(boxes_meet(&h, &x0));
                            second(cells[1])
                        }
                    })
                },
            )
        };
        let reason = |r: Result<Option<Enclosure>>| match r {
            Err(SolidError::Unsupported { reason, .. }) => reason,
            other => panic!("{:?}", other.map(|e| e.map(|e| e.x[2].at(0, 0)))),
        };
        // two feet apart, each on its own cell
        let two = run(&|c| {
            if c == left {
                proved([-0.005, -0.004])
            } else {
                proved([0.004, 0.005])
            }
        });
        assert!(reason(two).contains("two feet"));
        // a cell not decided
        let open = run(&|c| {
            if c == left {
                proved([-0.005, -0.004])
            } else {
                Inner::Unproved
            }
        });
        assert!(reason(open).contains("not decided"));
        // a cell whose K straddles H once: H is widened and every cell decided again
        let rounds = std::cell::Cell::new(0);
        let once = enclose_by_cells(
            [&wall, &fold],
            [EndContinuation::default(); 2],
            Ivl::new(0.0, 1.0),
            &x0,
            [false, true],
            |cells, over| {
                Ok(match over {
                    _ if cells[1] == left => proved([-0.005, -0.004]),
                    None => Inner::Unproved,
                    Some(h) => {
                        rounds.set(rounds.get() + 1);
                        if rounds.get() == 1 {
                            Inner::Straddles(h)
                        } else {
                            Inner::Excluded
                        }
                    }
                })
            },
        )
        .unwrap()
        .expect("the left foot after one widening");
        assert_eq!(rounds.get(), 2);
        assert_eq!(once.x[2].at(0, 0), Ivl::new(-0.005, -0.004));
        // straddling every round: refused once the rounds are spent
        let rounds = std::cell::Cell::new(0);
        let always = enclose_by_cells(
            [&wall, &fold],
            [EndContinuation::default(); 2],
            Ivl::new(0.0, 1.0),
            &x0,
            [false, true],
            |cells, over| {
                Ok(match over {
                    _ if cells[1] == left => proved([-0.005, -0.004]),
                    None => Inner::Unproved,
                    Some(h) => {
                        rounds.set(rounds.get() + 1);
                        Inner::Straddles(h)
                    }
                })
            },
        );
        assert!(reason(always).contains("not decided"));
        assert_eq!(rounds.get(), SECOND_PASS_ROUNDS);
        // one foot seen from both sides at the line: hulled
        let met = run(&|c| {
            if c == left {
                proved([-0.001, 0.0])
            } else {
                proved([0.0, 0.001])
            }
        })
        .unwrap()
        .expect("hulled");
        let u = met.x[2].at(0, 0);
        assert!(u.lo <= -0.001 && u.hi >= 0.001, "{u:?}");
        // the right cell's root off its own cell, or excluded: the left foot alone
        let off = |c: FaceCell| {
            if c == left {
                proved([-0.005, -0.004])
            } else {
                proved([-0.3, -0.2])
            }
        };
        let excluded = |c: FaceCell| {
            if c == left {
                proved([-0.005, -0.004])
            } else {
                Inner::Excluded
            }
        };
        for second in [&off as &dyn Fn(FaceCell) -> Inner, &excluded] {
            let one = run(second).unwrap().expect("the left foot");
            let u = one.x[2].at(0, 0);
            assert!(u.lo == -0.005 && u.hi == -0.004, "{u:?}");
        }
        // nothing proved in the first pass: not proved
        let none = enclose_by_cells(
            [&wall, &fold],
            [EndContinuation::default(); 2],
            Ivl::new(0.0, 1.0),
            &x0,
            [false, true],
            |_, _| Ok(Inner::Unproved),
        );
        assert!(none.unwrap().is_none());
        // a first-pass box past the fold's knots: H runs past them, refused
        let past = enclose_by_cells(
            [&wall, &fold],
            [EndContinuation::default(); 2],
            Ivl::new(0.0, 1.0),
            &x0,
            [false, true],
            |cells, over| {
                Ok(match over {
                    None if cells[1] == left => proved([-1.5, -0.004]),
                    _ => Inner::Unproved,
                })
            },
        );
        assert!(
            matches!(&past, Err(SolidError::Failed(f)) if matches!(f.reason, crate::error::Reason::FootPastDomainEnd { support: 1, .. })),
            "{:?}",
            past.map(|e| e.map(|e| e.x[2].at(0, 0)))
        );
        // a box across two lines of a face (three pieces in u): refused, cut the span
        let fold3 = net(
            vec![-1.0, -1.0, 0.0, 0.5, 1.0, 1.0],
            &[
                [-1.0, -3.0, 0.0],
                [-1.0, 3.0, 0.0],
                [0.0, -3.0, 0.0],
                [0.0, 3.0, 0.0],
                [0.5, -3.0, -0.25],
                [0.5, 3.0, -0.25],
                [1.0, -3.0, -1.0],
                [1.0, 3.0, -1.0],
            ],
        );
        let wide = enclose_by_cells(
            [&wall, &fold3],
            [EndContinuation::default(); 2],
            Ivl::new(0.0, 1.0),
            &x0,
            [false, true],
            |cells, over| {
                Ok(match over {
                    None if cells[1] == left => proved([-0.1, 0.6]),
                    _ => Inner::Unproved,
                })
            },
        );
        assert!(reason(wide).contains("spans a whole piece"));
        // the trigger is knot_cuts' refusal: they agree box by box
        for (lo, hi) in [
            (-0.1, 0.1),
            (-0.5, -0.1),
            (0.0, 0.5),
            (-1.0, 1.0),
            (0.0, 0.0),
        ] {
            assert_eq!(
                crosses_low_line(&fold.u, fold.p, lo, hi, FEET_BOX_CONTINUITY),
                knot_cuts(&fold.u, fold.p, fold.nu, lo, hi, FEET_BOX_CONTINUITY).is_err(),
                "[{lo}, {hi}]"
            );
        }
    }

    /// A span whose feet's box crosses two knot lines of a face (three pieces in u) is not
    /// enclosed whole: it is refused by name ("cut the span"), and one level down, as
    /// `gn_certify_span` cuts it, each half crosses one line and is proved cell by cell. The
    /// face is the plane z = 0 as a degree-1 net with simple knots at x = 0 and x = 0.01
    /// (`C^0` by count, flat), the other the wall y = -r; the ball's centre runs along
    /// `(x, 0, r)`, x from -0.004 to 0.014, its feet `(x, -r, r)` and `(x, 0, 0)`.
    #[test]
    fn a_box_across_two_knot_lines_is_proved_one_level_down() {
        let k = Kernel::default();
        let r = 1.0;
        let net = |u: Vec<f64>, rows: &[[f64; 3]]| {
            let mut pw = Vec::new();
            for row in rows {
                pw.extend([row[0], row[1], row[2], 1.0]);
            }
            Nurbs {
                p: 1,
                q: 1,
                nu: rows.len() / 2,
                nv: 2,
                u,
                v: vec![-3.0, -3.0, 3.0, 3.0],
                pw,
            }
        };
        // the wall y = -r: u its x, v its z; its normal P_u x P_v is -y, away from the ball
        let wall = net(
            vec![-3.0, -3.0, 3.0, 3.0],
            &[
                [-3.0, -r, -3.0],
                [-3.0, -r, 3.0],
                [3.0, -r, -3.0],
                [3.0, -r, 3.0],
            ],
        );
        // the floor: u its x, knots at 0 and 0.01; v its y; its normal +z, at the ball
        let floor = net(
            vec![-1.0, -1.0, 0.0, 0.01, 1.0, 1.0],
            &[
                [-1.0, -3.0, 0.0],
                [-1.0, 3.0, 0.0],
                [0.0, -3.0, 0.0],
                [0.0, 3.0, 0.0],
                [0.01, -3.0, 0.0],
                [0.01, 3.0, 0.0],
                [1.0, -3.0, 0.0],
                [1.0, 3.0, 0.0],
            ],
        );
        let (x0, x1) = (-0.004, 0.014);
        let ln = NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            pw: vec![x0, 0.0, r, 1.0, x1, 0.0, r, 1.0],
        };
        let x = |s: f64| x0 + (x1 - x0) * s;
        let (rr, side) = ([r, r], [-1.0, 1.0]);
        let ends = [EndContinuation::default(); 2];
        let piece = |lo: f64, hi: f64| SpanAt {
            s: Ivl::new(lo, hi),
            xa: [x(lo), r, x(lo), 0.0],
            xb: [x(hi), r, x(hi), 0.0],
            ku: [0; 2],
            kv: [0; 2],
            leader: None,
            piecewise: true,
        };
        let enclose = |at: &SpanAt| {
            gn_enclose_over(
                &k,
                [&wall, &floor],
                &ln,
                at,
                rr,
                side,
                0,
                1,
                None,
                None,
                ends,
            )
        };
        // the whole span: its first box crosses both lines
        let whole = enclose(&piece(0.0, 1.0));
        assert!(
            matches!(
                whole,
                Err(SolidError::Unsupported { reason, .. }) if reason.contains("spans a whole piece")
            ),
            "{:?}",
            whole.map(|e| e.map(|e| e.x[2].at(0, 0)))
        );
        // one level down: each half across one line, proved, the feet on the closed form
        for (lo, hi) in [(0.0, 0.5), (0.5, 1.0)] {
            let e = enclose(&piece(lo, hi))
                .unwrap()
                .expect("proved one level down");
            for s in [lo, hi] {
                let want = [Vec3::new(x(s), -r, r), Vec3::new(x(s), 0.0, 0.0)];
                for (f, w) in want.into_iter().enumerate() {
                    let xs = &e.f[f];
                    let got = V3 {
                        x: xs.a[0].c[0][0],
                        y: xs.a[1].c[0][0],
                        z: xs.a[2].c[0][0],
                    };
                    assert!(
                        holds(got, w),
                        "[{lo}, {hi}]: foot {f} at s {s}: {w:?} not in {got:?}"
                    );
                }
            }
        }
    }

    /// #15183: a root on the circle's quarter knot just outside the guesses' box. The ball and
    /// cone of the quarter-knot test over pieces [0.5, 0.5 + h] (h = 0.004 halved as the
    /// certificate's levels halve it), whose start puts the cone foot exactly on `u = 0.25`
    /// (the leader's middle, theta = 90 degrees); the guesses' cone u is moved so their box
    /// starts a few ulps past the knot (wr698's edge 46 missed it by 2e-10). The one solve's
    /// contraction forms a box across the line, which it could never prove; that box goes
    /// cell by cell, and the feet are proved, on the closed form.
    #[test]
    fn a_root_on_a_knot_line_past_the_guesses_is_proved_cell_by_cell() {
        let k = Kernel::default();
        let (bot, r) = (5.0, 2.0);
        let a = 30.0_f64.to_radians();
        let foot_h = r * (1.0 - a.sin());
        let height = 2.0 * foot_h;
        let top = bot - height * a.tan();
        let fp = plane(Vec3::ZERO, Vec3::X, Vec3::Y);
        let fc = upright_cone(&k, bot, top, height);
        let rho = bot + r * (1.0 - a.sin()) / a.cos();
        let frame = Axis2Placement::new(Vec3::new(0.0, 0.0, r), Vec3::X, Vec3::Y);
        let leader = BSplineCurve::create_circle_segment(
            3,
            &frame,
            rho,
            60.0,
            120.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let ln = NurbsCurve::of(&leader).unwrap();
        let (np, nc) = (nurbs(&k, &fp), nurbs(&k, &fc));
        let faces = [&fp, &fc];
        let lpt = |s: f64| leader.evaluate_point(s).unwrap();
        let foot_c = |s: f64| {
            let l = lpt(s);
            let q = (rho - r * a.cos()) / rho;
            Vec3::new(l.x * q, l.y * q, foot_h)
        };
        let (cu, cv) = nearest_uv(&nc, foot_c(0.3));
        let pc = nurbs_partials(
            &nc,
            span(&nc.u, nc.p, nc.nu, cu),
            span(&nc.v, nc.q, nc.nv, cv),
            cu,
            cv,
            1,
        );
        let nrm = vec3(pc.at(1, 0)).cross(vec3(pc.at(0, 1)));
        let out = Vec3::new(foot_c(0.3).x, foot_c(0.3).y, 0.0);
        let side_c = if nrm.dot(out) > 0.0 { 1.0 } else { -1.0 };
        let (rr, side) = ([r, r], [1.0, side_c]);
        let st = |s: f64| {
            let l = lpt(s);
            let (u, v) = nearest_uv(&nc, foot_c(s));
            station(&k, faces, &leader, s, [l.x, l.y, u, v], rr, side)
        };
        let ends = [EndContinuation::of(&fp), EndContinuation::of(&fc)];
        // the piece [0.5, 0.5 + h], its guesses' cone u lifted so their box starts a few ulps
        // past the knot (the real case's scale, wr698's edge 46: 2e-10)
        let piece = |h: f64| {
            let (sa, sb) = (0.5, 0.5 + h);
            let (sta, stb) = (st(sa), st(sb));
            let mut at = SpanAt {
                s: Ivl::new(sa, sb),
                xa: core::array::from_fn(|c| sta.x[c].at(0, 0)),
                xb: core::array::from_fn(|c| stb.x[c].at(0, 0)),
                ku: [0; 2],
                kv: [0; 2],
                leader: None,
                piecewise: true,
            };
            // the box's half width grows with |u| (its 1e-9 (1 + |m|)), so the lift is taken
            // twice, the second against the lifted box
            for _ in 0..2 {
                let (_, x0) = guess_box(&at);
                let lift = 0.25 - x0[2].lo + 8.0 * f64::EPSILON * 0.25;
                at.xa[2] += lift;
                at.xb[2] += lift;
            }
            at
        };
        let enclose = |at: &SpanAt| {
            gn_enclose_over(&k, [&np, &nc], &ln, at, rr, side, 0, 1, None, None, ends)
        };
        // cut as the certificate's levels cut a span: measured (15183-1p, the guesses lifted
        // by 1e-6 then), over the whole 0.004 the one solve's first box has q = 1.16, so it
        // never contracts and forms no box: the span's length, not the knot, which the levels
        // answer by halving
        let mut proved = None;
        for level in 0..=SPAN_MAX_LEVEL {
            let h = 0.004 / f64::from(1_u32 << level);
            let at = piece(h);
            let (_, x0) = guess_box(&at);
            assert!(
                x0[2].lo > 0.25 && x0[2].lo - 0.25 < 1.0e-14,
                "level {level}: the guesses' box {:?} is not just past the knot",
                x0[2]
            );
            assert_eq!(low_lines_in([&np, &nc], &x0), [false, false]);
            if let Some(e) = enclose(&at).unwrap() {
                proved = Some((level, at, e));
                break;
            }
        }
        let (level, at, e) = proved.expect("proved by SPAN_MAX_LEVEL");
        let u = e.x[2].at(0, 0);
        assert!(
            u.lo <= 0.25 && 0.25 <= u.hi,
            "the cone foot's u {u:?} misses the knot"
        );
        for s in [at.s.lo, at.s.hi] {
            let (l, c) = (lpt(s), foot_c(s));
            let fp_s = Vec3::new(l.x, l.y, 0.0);
            for (f, want) in [(0, fp_s), (1, c)] {
                let x = &e.f[f];
                let got = V3 {
                    x: x.a[0].c[0][0],
                    y: x.a[1].c[0][0],
                    z: x.a[2].c[0][0],
                };
                assert!(
                    holds(got, want),
                    "level {level}: foot {f} at s {s}: {want:?} not in {got:?}"
                );
            }
        }
    }

    /// #15183: the cell path's first box from a crossing is the hull of the one solve's first
    /// box and the crossing one, and each face's cells are read from the hull: here face 0
    /// crosses its line in the first box only (the crossing box is narrower there) and face 1
    /// in the crossing box only, so both are taken cell by cell.
    #[test]
    fn cells_after_a_crossing_are_read_from_the_hull() {
        let net = |u: Vec<f64>, rows: &[[f64; 3]]| {
            let mut pw = Vec::new();
            for row in rows {
                pw.extend([row[0], row[1], row[2], 1.0]);
            }
            Nurbs {
                p: 1,
                q: 1,
                nu: rows.len() / 2,
                nv: 2,
                u,
                v: vec![-3.0, -3.0, 3.0, 3.0],
                pw,
            }
        };
        let fold = || {
            net(
                vec![-1.0, -1.0, 0.0, 1.0, 1.0],
                &[
                    [-1.0, -3.0, 0.0],
                    [-1.0, 3.0, 0.0],
                    [0.0, -3.0, 0.0],
                    [0.0, 3.0, 0.0],
                    [1.0, -3.0, -0.5],
                    [1.0, 3.0, -0.5],
                ],
            )
        };
        let (f0, f1) = (fold(), fold());
        let flat = Ivl::new(-0.5, 0.5);
        let first = [Ivl::new(-0.01, 0.01), flat, Ivl::new(0.002, 0.004), flat];
        let crossing = [Ivl::new(0.001, 0.002), flat, Ivl::new(-0.01, 0.01), flat];
        assert_eq!(low_lines_in([&f0, &f1], &first), [true, false]);
        assert_eq!(low_lines_in([&f0, &f1], &crossing), [false, true]);
        let (x1, low) = from_the_crossing([&f0, &f1], &first, &crossing);
        assert_eq!(low, [true, true]);
        assert_eq!(x1[0], Ivl::new(-0.01, 0.01));
        assert_eq!(x1[2], Ivl::new(-0.01, 0.01));
    }

    /// The cell path's trigger is `knot_cuts`' refusal, box by box, across multiplicities: a
    /// cubic's simple knot (`C^2`, not low), double (`C^1`) and triple (`C^0`), and a rational
    /// circle's doubled quarter knot (`C^0` by count); boxes across, touching, beside and on
    /// the knot.
    #[test]
    fn the_trigger_is_knot_cuts_refusal() {
        let k = Kernel::default();
        let cone = nurbs(&k, &upright_cone(&k, 5.0, 3.0, 4.0));
        let mut nets: Vec<(Nurbs, f64)> = (1..=3).map(|m| (cubic_with_knot(m), 1.0)).collect();
        nets.push((cone, 0.25));
        for (n, at) in &nets {
            for (lo, hi) in [
                (at - 0.1, at + 0.1),
                (at - 0.1, *at),
                (*at, at + 0.1),
                (*at, *at),
                (at - 0.2, at - 0.1),
                (at + 0.1, at + 0.2),
            ] {
                assert_eq!(
                    crosses_low_line(&n.u, n.p, lo, hi, FEET_BOX_CONTINUITY),
                    knot_cuts(&n.u, n.p, n.nu, lo, hi, FEET_BOX_CONTINUITY).is_err(),
                    "degree {} knot {at} box [{lo}, {hi}]",
                    n.p
                );
            }
        }
    }

    /// A real `C^0` corner, not a circle: a ball of radius r rolled along y between the wall
    /// x = -r and a net folded at x = 0 (z = 0 for x <= 0, z = -m x past it: degree 1, a
    /// simple knot), its centre `(0, y, r)` above the fold. Its feet are `(-r, y, r)` and
    /// `(0, y, 0)`, on the fold, the box across it. For a pointwise use the feet are proved:
    /// the flat cell's root is the foot, the sloped cell's continued root `(-r m / sqrt(1 +
    /// m^2), ..)` lies off its own cell, so it is not a foot. For a use of order 2 the box is
    /// refused (`C^0` by count), nothing proved.
    #[test]
    fn a_foot_on_a_c0_fold_is_proved_for_a_pointwise_use_only() {
        let k = Kernel::default();
        let (r, m) = (1.0, 0.5);
        let net = |u: Vec<f64>, rows: &[[f64; 3]], v: [f64; 2]| {
            let mut pw = Vec::new();
            for row in rows {
                pw.extend([row[0], row[1], row[2], 1.0]);
            }
            Nurbs {
                p: 1,
                q: 1,
                nu: rows.len() / 2,
                nv: 2,
                u,
                v: vec![v[0], v[0], v[1], v[1]],
                pw,
            }
        };
        // the wall x = -r: u its y, v its z
        let wall = net(
            vec![-3.0, -3.0, 3.0, 3.0],
            &[
                [-r, -3.0, -3.0],
                [-r, -3.0, 3.0],
                [-r, 3.0, -3.0],
                [-r, 3.0, 3.0],
            ],
            [-3.0, 3.0],
        );
        // the fold: u its x (a knot at 0, simple), v its y
        let fold = net(
            vec![-1.0, -1.0, 0.0, 1.0, 1.0],
            &[
                [-1.0, -3.0, 0.0],
                [-1.0, 3.0, 0.0],
                [0.0, -3.0, 0.0],
                [0.0, 3.0, 0.0],
                [1.0, -3.0, -m],
                [1.0, 3.0, -m],
            ],
            [-3.0, 3.0],
        );
        // the leader: the centres' line, y = -2 + 4 s
        let ln = NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            pw: vec![0.0, -2.0, r, 1.0, 0.0, 2.0, r, 1.0],
        };
        let y = |s: f64| -2.0 + 4.0 * s;
        // both nets' normals P_u x P_v point at the ball: (1, 0, 0) and (0, 0, 1)
        let (rr, side) = ([r, r], [1.0, 1.0]);
        let (sa, sb) = (0.4, 0.6);
        let at = SpanAt {
            s: Ivl::new(sa, sb),
            xa: [y(sa), r, 0.0, y(sa)],
            xb: [y(sb), r, 0.0, y(sb)],
            ku: [0; 2],
            kv: [0; 2],
            leader: None,
            piecewise: true,
        };
        let ends = [EndContinuation::default(); 2];
        let enclose = |order: usize| {
            gn_enclose_over(
                &k,
                [&wall, &fold],
                &ln,
                &at,
                rr,
                side,
                0,
                order,
                None,
                None,
                ends,
            )
        };
        let e = enclose(1).unwrap().expect("proved at order 1");
        let u = e.x[2].at(0, 0);
        assert!(
            u.lo <= 0.0 && 0.0 <= u.hi,
            "the fold's u box {u:?} misses the knot"
        );
        for s in [sa, sb] {
            let want = [Vec3::new(-r, y(s), r), Vec3::new(0.0, y(s), 0.0)];
            for (f, w) in want.into_iter().enumerate() {
                let x = &e.f[f];
                let got = V3 {
                    x: x.a[0].c[0][0],
                    y: x.a[1].c[0][0],
                    z: x.a[2].c[0][0],
                };
                assert!(holds(got, w), "foot {f} at s {s}: {w:?} not in {got:?}");
            }
        }
        assert!(
            enclose(2).unwrap().is_none(),
            "order 2 proved across a C^0 line"
        );
        let refused = nurbs_partials_over_box(&fold, -0.1, 0.1, -1.0, 1.0, 2, FEET_BOX_CONTINUITY);
        assert!(
            matches!(refused, Err(SolidError::Unsupported { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_curve_without_a_span_is_a_typed_failure() {
        let cn = NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0; 4],
            pw: vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        };
        let error = curve_derivs_over(&cn, Ivl::new(0.0, 0.0), 1, None).unwrap_err();
        assert_eq!(
            error,
            SolidError::blend_failed(OP, "no piece covers the parameter")
        );
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
    }
}
