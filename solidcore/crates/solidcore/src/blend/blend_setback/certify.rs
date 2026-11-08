//! Bivariate Bernstein tools and the convexity certificate of a G1 fill (the fork's
//! `sm_SbT*`, `sm_SbFundamentals`, `sm_SbAffinePatch`, `sm_SbAffineFundamentals`,
//! `sm_SbSurface`, `sm_SbCertifiedConvex`; tangency-labs/smlib 590a7f3, 08d4b02, f1d81f4;
//! smlib#104).
//!
//! Deviations: `sm_SbTNonneg` recurses to its `depth` argument (at most
//! [`super::graph::GRAPH_DEPTH`] from its callers), the fork's own bound.
//! `std::min_element` is the first least coefficient by `<`, as the fork's.

use super::{
    binomial, el, el_mut, el_ref, euclid, sm_err, smax, tensor_elevate, Face, Net, Tensor,
};
use crate::error::Result;
use crate::math::Vec3;
use crate::nurbs::b_spline_curve::KnotType;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};

/// `sm_SbTMake`: the zero polynomial of degree `(p, q)`.
pub(super) fn t_make(p: usize, q: usize) -> Tensor {
    Tensor {
        p,
        q,
        c: vec![0.0; (p + 1) * (q + 1)],
    }
}

/// `sm_SbTAxpy`: `a + s b`, both raised to the larger degrees.
pub(super) fn t_axpy(a: &Tensor, s: f64, b: &Tensor) -> Result<Tensor> {
    let p = smax_usize(a.p, b.p);
    let q = smax_usize(a.q, b.q);
    let mut sa = if a.p == p && a.q == q {
        a.clone()
    } else {
        tensor_elevate(a, p, q)?
    };
    let sb = if b.p == p && b.q == q {
        b.clone()
    } else {
        tensor_elevate(b, p, q)?
    };
    for (ai, &bi) in sa.c.iter_mut().zip(&sb.c) {
        *ai += s * bi;
    }
    Ok(sa)
}

/// `smos_Max` on sizes.
fn smax_usize(a: usize, b: usize) -> usize {
    if a > b {
        a
    } else {
        b
    }
}

/// `sm_SbTDu`.
pub(super) fn t_du(a: &Tensor) -> Result<Tensor> {
    if a.p == 0 {
        return Ok(t_make(0, a.q));
    }
    let mut s = t_make(a.p - 1, a.q);
    for i in 0..a.p {
        for j in 0..=a.q {
            *el_mut(&mut s.c, i * (a.q + 1) + j)? =
                a.p as f64 * (el(&a.c, (i + 1) * (a.q + 1) + j)? - el(&a.c, i * (a.q + 1) + j)?);
        }
    }
    Ok(s)
}

/// `sm_SbTDv`.
pub(super) fn t_dv(a: &Tensor) -> Result<Tensor> {
    if a.q == 0 {
        return Ok(t_make(a.p, 0));
    }
    let mut s = t_make(a.p, a.q - 1);
    for i in 0..=a.p {
        for j in 0..a.q {
            *el_mut(&mut s.c, i * a.q + j)? =
                a.q as f64 * (el(&a.c, i * (a.q + 1) + j + 1)? - el(&a.c, i * (a.q + 1) + j)?);
        }
    }
    Ok(s)
}

/// `sm_SbTSplit`: the halves at `u = 1/2` (`dir` 0) or `v = 1/2` (`dir` 1), by de
/// Casteljau.
pub(super) fn t_split(a: &Tensor, dir: u8) -> Result<(Tensor, Tensor)> {
    let mut lo = a.clone();
    let mut hi = a.clone();
    let (n, other) = if dir == 0 { (a.p, a.q) } else { (a.q, a.p) };
    let mut w = vec![0.0; n + 1];
    for k in 0..=other {
        let idx = |i: usize| {
            if dir == 0 {
                i * (a.q + 1) + k
            } else {
                k * (a.q + 1) + i
            }
        };
        for (i, wi) in w.iter_mut().enumerate() {
            *wi = el(&a.c, idx(i))?;
        }
        for r in 0..=n {
            *el_mut(&mut lo.c, idx(r))? = el(&w, 0)?;
            *el_mut(&mut hi.c, idx(n - r))? = el(&w, n - r)?;
            for i in 0..n - r {
                let next = el(&w, i + 1)?;
                let wi = el_mut(&mut w, i)?;
                *wi = 0.5 * (*wi + next);
            }
        }
    }
    Ok((lo, hi))
}

/// `sm_SbTPieces`: the pieces of `a` after `levels` halvings each way, in a fixed order.
pub(super) fn t_pieces(a: &Tensor, levels: usize) -> Result<Vec<Tensor>> {
    let mut pieces = vec![a.clone()];
    for _ in 0..levels {
        let mut next = Vec::new();
        for piece in &pieces {
            let (pa, pb) = t_split(piece, 0)?;
            let (a0, a1) = t_split(&pa, 1)?;
            let (b0, b1) = t_split(&pb, 1)?;
            next.push(a0);
            next.push(a1);
            next.push(b0);
            next.push(b1);
        }
        pieces = next;
    }
    Ok(pieces)
}

/// `sm_SbTMulFast`: `a b`, as `sm_SbTMul` but the binomials taken once: the coefficients
/// scaled by them, convolved, and unscaled.
pub(super) fn t_mul_fast(a: &Tensor, b: &Tensor) -> Result<Tensor> {
    let mut c = t_make(a.p + b.p, a.q + b.q);
    let mut sa = vec![0.0; a.c.len()];
    let mut sb = vec![0.0; b.c.len()];
    for i in 0..=a.p {
        for j in 0..=a.q {
            *el_mut(&mut sa, i * (a.q + 1) + j)? =
                el(&a.c, i * (a.q + 1) + j)? * binomial(a.p, i)? * binomial(a.q, j)?;
        }
    }
    for i in 0..=b.p {
        for j in 0..=b.q {
            *el_mut(&mut sb, i * (b.q + 1) + j)? =
                el(&b.c, i * (b.q + 1) + j)? * binomial(b.p, i)? * binomial(b.q, j)?;
        }
    }
    for i in 0..=a.p {
        for j in 0..=a.q {
            let av = el(&sa, i * (a.q + 1) + j)?;
            if av == 0.0 {
                continue;
            }
            for k in 0..=b.p {
                let row = (i + k) * (c.q + 1) + j;
                for l in 0..=b.q {
                    *el_mut(&mut c.c, row + l)? += av * el(&sb, k * (b.q + 1) + l)?;
                }
            }
        }
    }
    for i in 0..=c.p {
        for j in 0..=c.q {
            let f = binomial(c.p, i)? * binomial(c.q, j)?;
            *el_mut(&mut c.c, i * (c.q + 1) + j)? /= f;
        }
    }
    Ok(c)
}

/// `sm_SbTDet3`: the determinant of a 3 x 3 matrix of polynomials.
pub(super) fn t_det3(m: [[&Tensor; 3]; 3]) -> Result<Tensor> {
    let mut s = t_make(0, 0);
    for j in 0..3 {
        let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
        let [r0, r1, r2] = m;
        let minor = t_axpy(
            &t_mul_fast(el(&r1, j1)?, el(&r2, j2)?)?,
            -1.0,
            &t_mul_fast(el(&r1, j2)?, el(&r2, j1)?)?,
        )?;
        s = t_axpy(&s, 1.0, &t_mul_fast(el(&r0, j)?, &minor)?)?;
    }
    Ok(s)
}

/// `sm_SbTNonneg`: whether `a >= -eps` on the whole square, certified: 1 (every coefficient
/// of each piece `>= -eps`, halving each way up to `depth` times), 0 (a corner of a piece, a
/// value of `a`, below `-eps`), -1 (undecided at that depth).
pub(super) fn t_nonneg(a: &Tensor, eps: f64, depth: usize) -> Result<i32> {
    let corners = [
        el(&a.c, 0)?,
        el(&a.c, a.q)?,
        el(&a.c, a.p * (a.q + 1))?,
        *a.c.last().ok_or_else(sm_err)?,
    ];
    for corner in corners {
        if corner < -eps {
            return Ok(0);
        }
    }
    // std::min_element: the first least by <
    let mut least = el(&a.c, 0)?;
    for &x in a.c.iter().skip(1) {
        if x < least {
            least = x;
        }
    }
    if least >= -eps {
        return Ok(1);
    }
    if depth == 0 {
        return Ok(-1);
    }
    let (pa, pb) = t_split(a, 0)?;
    let (a0, a1) = t_split(&pa, 1)?;
    let (b0, b1) = t_split(&pb, 1)?;
    let mut result = 1;
    for piece in [&a0, &a1, &b0, &b1] {
        let r = t_nonneg(piece, eps, depth - 1)?;
        if r == 0 {
            return Ok(0);
        }
        result = if r < result { r } else { result };
    }
    Ok(result)
}

/// `sm_SbTScale`: the largest coefficient's size.
pub(super) fn t_scale(a: &Tensor) -> f64 {
    let mut d = 0.0;
    for &x in &a.c {
        d = smax(d, x.abs());
    }
    d
}

/// The four components of a net as polynomials, and their first and second derivatives:
/// `h`, `hu`, `hv`, `huu`, `huv`, `hvv`, and `(e, 0)` as constants.
struct Rows {
    h: [Tensor; 4],
    hu: [Tensor; 4],
    hv: [Tensor; 4],
    hx: [[Tensor; 4]; 3],
    one: [Tensor; 4],
}

fn rows_of(net: &Net, e: Vec3) -> Result<Rows> {
    let mk = |c: usize| -> Result<Tensor> {
        let mut t = t_make(net.p, net.q);
        for (i, ti) in t.c.iter_mut().enumerate() {
            *ti = el(el_ref(&net.pts, i)?, c)?;
        }
        Ok(t)
    };
    let h = [mk(0)?, mk(1)?, mk(2)?, mk(3)?];
    let hu = [t_du(&h[0])?, t_du(&h[1])?, t_du(&h[2])?, t_du(&h[3])?];
    let hv = [t_dv(&h[0])?, t_dv(&h[1])?, t_dv(&h[2])?, t_dv(&h[3])?];
    let huu = [t_du(&hu[0])?, t_du(&hu[1])?, t_du(&hu[2])?, t_du(&hu[3])?];
    let huv = [t_dv(&hu[0])?, t_dv(&hu[1])?, t_dv(&hu[2])?, t_dv(&hu[3])?];
    let hvv = [t_dv(&hv[0])?, t_dv(&hv[1])?, t_dv(&hv[2])?, t_dv(&hv[3])?];
    let constant = |x: f64| Tensor {
        p: 0,
        q: 0,
        c: vec![x],
    };
    let one = [constant(e.x), constant(e.y), constant(e.z), constant(0.0)];
    Ok(Rows {
        h,
        hu,
        hv,
        hx: [huu, huv, hvv],
        one,
    })
}

/// The 3 x 3 minor of rows `rows` without column `c`.
pub(super) fn minor_without<'a>(
    rows: [&'a [Tensor; 4]; 3],
    c: usize,
) -> Result<[[&'a Tensor; 3]; 3]> {
    let mut m: [[Option<&Tensor>; 3]; 3] = [[None; 3]; 3];
    for (r, row) in rows.iter().enumerate() {
        let mut cc = 0;
        for (col, t) in row.iter().enumerate() {
            if col != c {
                *el_mut(el_mut(&mut m, r)?, cc)? = Some(t);
                cc += 1;
            }
        }
    }
    let get =
        |r: usize, c: usize| -> Result<&'a Tensor> { el(el_ref(&m, r)?, c)?.ok_or_else(sm_err) };
    Ok([
        [get(0, 0)?, get(0, 1)?, get(0, 2)?],
        [get(1, 0)?, get(1, 1)?, get(1, 2)?],
        [get(2, 0)?, get(2, 1)?, get(2, 2)?],
    ])
}

/// `sm_SbTDet4`: the determinant of a 4 x 4 matrix of polynomials (rows `m[r]`), along its
/// last row.
fn t_det4(m: [&[Tensor; 4]; 4]) -> Result<Tensor> {
    let mut s = t_make(0, 0);
    let [r0, r1, r2, r3] = m;
    for c in 0..4 {
        let pm = minor_without([r0, r1, r2], c)?;
        let sign = if (3 + c) % 2 == 0 { 1.0 } else { -1.0 };
        s = t_axpy(&s, sign, &t_mul_fast(el_ref(r3, c)?, &t_det3(pm)?)?)?;
    }
    Ok(s)
}

/// `sm_SbFundamentals`: a homogeneous net's `(x w, y w, z w, w)` curvature determinants:
/// `l = det(H, Hu, Hv, Huu)`, `m = det(H, Hu, Hv, Huv)`, `n = det(H, Hu, Hv, Hvv)` (each
/// `-w^4 (Pu x Pv) . P..` for `P = H / w`) and `o = det(H, Hu, Hv, (e, 0))`
/// (`-w^3 (Pu x Pv) . e`).
pub(super) fn fundamentals(net: &Net, e: Vec3) -> Result<[Tensor; 4]> {
    let rows = rows_of(net, e)?;
    let [huu, huv, hvv] = &rows.hx;
    let last = [huu, huv, hvv, &rows.one];
    let mut out: [Tensor; 4] = Default::default();
    for (k, slot) in out.iter_mut().enumerate() {
        // along the last row: det = sum_c (-1)^(3 + c) last[c] minor(3, c)
        let mut s = t_make(0, 0);
        for c in 0..4 {
            let pm = minor_without([&rows.h, &rows.hu, &rows.hv], c)?;
            let minor = t_det3(pm)?;
            let sign = if (3 + c) % 2 == 0 { 1.0 } else { -1.0 };
            s = t_axpy(&s, sign, &t_mul_fast(el_ref(el(&last, k)?, c)?, &minor)?)?;
        }
        *slot = s;
    }
    Ok(out)
}

/// `sm_SbAffinePatch`: a patch whose control points move only along one direction e,
/// linearly in the unknowns: its net is `base + sum_j y_j Z_j (e, 0)`, `Z_j` (`z`, of the
/// net's degrees) for the unknowns `index`. Its projection along e (and its weights) never
/// moves.
#[derive(Debug, Clone, Default)]
pub(super) struct AffinePatch {
    /// `m_sBase`.
    pub base: Net,
    /// `m_vIndex`.
    pub index: Vec<usize>,
    /// `m_vZ`.
    pub z: Vec<Tensor>,
}

/// `sm_SbAffineNet`: the patch's net at the unknowns `y`.
pub(super) fn affine_net(p: &AffinePatch, y: &[f64], e: Vec3) -> Result<Net> {
    let mut net = p.base.clone();
    for (k, &ix) in p.index.iter().enumerate() {
        let yk = el(y, ix)?;
        if yk == 0.0 {
            continue;
        }
        let zk = el_ref(&p.z, k)?;
        for (i, pt) in net.pts.iter_mut().enumerate() {
            let z = yk * el(&zk.c, i)?;
            pt[0] += z * e.x;
            pt[1] += z * e.y;
            pt[2] += z * e.z;
        }
    }
    Ok(net)
}

/// `sm_SbSurface`: a net's patch as a rational (or polynomial, all weights 1) Bezier surface
/// over `[0, 1]^2`.
pub(super) fn net_surface(net: &Net) -> Result<BSplineSurface> {
    let mut pts = Vec::new();
    let mut ws = Vec::new();
    let mut rational = false;
    for h in &net.pts {
        if !(h[3] > 0.0) {
            return Err(sm_err());
        }
        pts.push(euclid(h));
        ws.push(h[3]);
        if h[3] != 1.0 {
            rational = true;
        }
    }
    let um = [net.p + 1, net.p + 1];
    let vm = [net.q + 1, net.q + 1];
    let knots = [0.0, 1.0];
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: net.p,
        v_degree: net.q,
        control_points: &pts,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &um,
        v_multiplicities: &vm,
        u_knots: &knots,
        v_knots: &knots,
        knot_type: KnotType::Unspecified,
        weights: if rational { Some(ws.as_slice()) } else { None },
        uv_domain: None,
    })
}

/// What [`affine_fundamentals`] works out.
pub(super) struct AffineFundamentals {
    /// `rQ0[k]`: `l`, `m`, `n` (k = 0, 1, 2) at `y = 0`.
    pub q0: [Tensor; 3],
    /// `rQj[k][t]`: the share of unknown `index[t]` in each.
    pub qj: [Vec<Tensor>; 3],
    /// `rO`: `o` (fixed).
    pub o: Tensor,
}

/// `sm_SbAffineFundamentals`: an affine patch's curvature determinants `l`, `m`, `n` as
/// affine functions of the unknowns, and `o` (fixed). With `H = H0 + Z (e, 0)` and
/// `E = (e, 0)`, `det(H, Hu, Hv, X) = det(H0, H0u, H0v, X0) + sum_r Z_r det(.., E in row r,
/// ..)` (two rows along E give 0): affine in Z.
pub(super) fn affine_fundamentals(p: &AffinePatch, e: Vec3) -> Result<AffineFundamentals> {
    let rows = rows_of(&p.base, e)?;
    let o = t_det4([&rows.h, &rows.hu, &rows.hv, &rows.one])?;
    let mut q0: [Tensor; 3] = Default::default();
    let mut qj: [Vec<Tensor>; 3] = Default::default();
    for k in 0..3 {
        let hx = el_ref(&rows.hx, k)?;
        let prows = [&rows.h, &rows.hu, &rows.hv, hx];
        *el_mut(&mut q0, k)? = t_det4(prows)?;
        // the cofactors: row r replaced by E (row 3's is o)
        let mut cof: [Tensor; 4] = Default::default();
        for r in 0..3 {
            let mut pr = prows;
            *el_mut(&mut pr, r)? = &rows.one;
            *el_mut(&mut cof, r)? = t_det4(pr)?;
        }
        cof[3] = o.clone();
        let out = el_mut(&mut qj, k)?;
        out.clear();
        for zt in &p.z {
            let zu = t_du(zt)?;
            let zv = t_dv(zt)?;
            let zx = match k {
                0 => t_du(&zu)?,
                1 => t_dv(&zu)?,
                _ => t_dv(&zv)?,
            };
            let mut s = t_mul_fast(zt, &cof[0])?;
            s = t_axpy(&s, 1.0, &t_mul_fast(&zu, &cof[1])?)?;
            s = t_axpy(&s, 1.0, &t_mul_fast(&zv, &cof[2])?)?;
            s = t_axpy(&s, 1.0, &t_mul_fast(&zx, &cof[3])?)?;
            out.push(s);
        }
    }
    Ok(AffineFundamentals { q0, qj, o })
}

/// `sm_SbSidesOnPlanes` (smlib#125 3e10318): the sides of the setback patches (nets
/// `0 .. n - 1`) G1 to a plane by construction: patch `m`'s side `v = 0` on face `m`,
/// `v = 1` on face `m + 1` (`cycle_face`: the cycle's faces), each marked where its first
/// two rows of control points (the side and its cross derivative) lie on that face's
/// plane, to rounding at their scale. Two flags per net, `false` for every other net and
/// side.
pub(super) fn sides_on_planes(
    nets: &[Net],
    n: usize,
    faces: &[Face],
    cycle_face: &[usize],
) -> Vec<bool> {
    let mut on = vec![false; 2 * nets.len()];
    let mut m = 0;
    while m < n && m < nets.len() && n <= cycle_face.len() {
        for e in 0..2 {
            let Some(&f) = cycle_face.get((m + e) % n) else {
                continue;
            };
            let Some(face) = faces.get(f) else {
                continue;
            };
            let Ok(normal) = face.normal.unitize() else {
                continue;
            };
            let Some(net) = nets.get(m) else {
                continue;
            };
            if net.q < 1 {
                continue;
            }
            let mut is_on = true;
            'rows: for i in 0..=net.p {
                for jj in 0..2 {
                    let j = if e == 0 { jj } else { net.q - jj };
                    let Some(h) = net.pts.get(i * (net.q + 1) + j) else {
                        is_on = false;
                        break 'rows;
                    };
                    if !(h[3].abs() > 0.0) {
                        is_on = false;
                        break 'rows;
                    }
                    let x = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
                    let d = x - face.point;
                    let scale = 1.0 + x.length() + face.point.length();
                    is_on = d.dot(normal).abs() <= 1.0e-12 * scale;
                    if !is_on {
                        break 'rows;
                    }
                }
            }
            if let Some(slot) = on.get_mut(2 * m + e) {
                *slot = is_on;
            }
        }
        m += 1;
    }
    on
}

/// `sm_SbCertifiedConvex`: the fill's shape certified from its nets: on every patch
/// `o = det(H, Hu, Hv, (e, 0))` of one sign s and `s l`, `s n`, `l n - m^2 >= 0` (each to
/// `1e-12` of its coefficients' scale), from Bernstein coefficients halving up to `depth`
/// times. `Err(patch)` (`rlPatch`): the first patch not certified.
///
/// `plane_sides[2 p + e]` (`e = 0`: side `v = 0`, `e = 1`: side `v = 1`; smlib#125
/// 3e10318) marks a side of patch `p` that is G1 to a plane by the patch's construction
/// ([`sides_on_planes`]). Along such a side the normal is the plane's, so `N_u = 0`, and
/// `L = -S_u . N_u` and `M = -S_v . N_u` vanish identically: their Bernstein rows on that
/// side are set to exactly 0 rather than read from rounding. Every other side keeps its
/// computed rows.
pub(super) fn certified_convex(
    nets: &[Net],
    plane_sides: &[bool],
    e: Vec3,
    depth: usize,
) -> Result<core::result::Result<(), usize>> {
    for (p, net) in nets.iter().enumerate() {
        let [mut fl, mut fm, fn_, fo] = fundamentals(net, e)?;
        for side in 0..2 {
            if !plane_sides.get(2 * p + side).copied().unwrap_or(false) {
                continue;
            }
            // a side G1 to a plane: N_u = 0 there, so L = -S_u . N_u = 0 and M = -S_v . N_u = 0
            for t in [&mut fl, &mut fm] {
                let q = t.q;
                for i in 0..=t.p {
                    if let Some(c) = t.c.get_mut(i * (q + 1) + if side == 0 { 0 } else { q }) {
                        *c = 0.0;
                    }
                }
            }
        }
        let d_o = t_scale(&fo);
        let mut neg_o = fo.clone();
        for x in &mut neg_o.c {
            *x = -*x;
        }
        let s = if t_nonneg(&fo, 1.0e-12 * d_o, depth)? == 1 {
            1.0
        } else if t_nonneg(&neg_o, 1.0e-12 * d_o, depth)? == 1 {
            -1.0
        } else {
            0.0
        };
        if s == 0.0 {
            return Ok(Err(p));
        }
        let mut sl = fl.clone();
        let mut sn = fn_.clone();
        for x in &mut sl.c {
            *x *= s;
        }
        for x in &mut sn.c {
            *x *= s;
        }
        let disc = t_axpy(&t_mul_fast(&fl, &fn_)?, -1.0, &t_mul_fast(&fm, &fm)?)?;
        for check in [&sl, &sn, &disc] {
            if t_nonneg(check, 1.0e-12 * t_scale(check), depth)? != 1 {
                return Ok(Err(p));
            }
        }
    }
    Ok(Ok(()))
}
