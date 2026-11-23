//! Bivariate Bernstein tools and the convexity certificate of a G1 fill (the fork's
//! `sm_SbT*`, `sm_SbFundamentals`, `sm_SbAffinePatch`, `sm_SbAffineFundamentals`,
//! `sm_SbSurface`, `sm_SbCertifiedConvex`; tangency-labs/smlib 590a7f3, 08d4b02, f1d81f4;
//! smlib#104).
//!
//! Deviations: `sm_SbTNonneg` recurses to its `depth` argument (at most
//! [`super::graph::GRAPH_DEPTH`] from its callers), the fork's own bound.
//! `std::min_element` is the first least coefficient by `<`, as the fork's.
//! `sm_SbTMulFast`'s binomials are read from a table ([`Binomials`]) built by the same
//! function, a factor shared by many products is scaled by them once ([`t_scaled`]), and
//! `sm_SbTDet4`'s minors of rows that recur are taken once ([`det4_minors`]): the same
//! operations on the same values in the same order, so the same bits, without evaluating
//! two binomials per coefficient of every factor and product.

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

/// The binomial coefficients `C(n, i)` for `i` in `0 ..= n`, each as [`binomial`] gives it,
/// kept per `n` once asked: the products' scalings read them per coefficient.
#[derive(Debug, Default)]
pub(super) struct Binomials {
    rows: Vec<Vec<f64>>,
    /// The [`binomial`] evaluations made (the work the table saves is counted, never timed).
    evaluations: usize,
}

impl Binomials {
    /// The row `C(n, 0 ..= n)`.
    fn row(&mut self, n: usize) -> Result<&[f64]> {
        while self.rows.len() <= n {
            let m = self.rows.len();
            let row = (0..=m)
                .map(|i| binomial(m, i))
                .collect::<Result<Vec<f64>>>()?;
            self.evaluations = self.evaluations.saturating_add(row.len());
            self.rows.push(row);
        }
        self.rows.get(n).map(Vec::as_slice).ok_or_else(sm_err)
    }
}

/// A polynomial's coefficients scaled by their binomials, `c[i, j] C(p, i) C(q, j)`, as
/// [`t_mul_fast`] scales its factors: a factor used in many products is scaled once.
pub(super) struct ScaledTensor {
    p: usize,
    q: usize,
    c: Vec<f64>,
}

/// `a`'s coefficients scaled by their binomials (`sm_SbTMulFast`'s first step).
pub(super) fn t_scaled(a: &Tensor, binomials: &mut Binomials) -> Result<ScaledTensor> {
    let width = a.q + 1;
    if a.c.len() != (a.p + 1) * width {
        return Err(sm_err());
    }
    let bq = binomials.row(a.q)?.to_vec();
    let bp = binomials.row(a.p)?;
    let mut c = Vec::new();
    for (i, &bpi) in bp.iter().enumerate() {
        for (&x, &bqj) in row(&a.c, i, width)?.iter().zip(&bq) {
            c.push(x * bpi * bqj);
        }
    }
    Ok(ScaledTensor { p: a.p, q: a.q, c })
}

/// Row `i` of coefficients `c` stored `width` to a row.
fn row(c: &[f64], i: usize, width: usize) -> Result<&[f64]> {
    let start = i * width;
    c.get(start..start + width).ok_or_else(sm_err)
}

/// The product of two scaled polynomials, convolved and unscaled (`sm_SbTMulFast`'s
/// second and third steps): the same operations, in the same order, as [`t_mul_fast`].
pub(super) fn t_mul_scaled(
    a: &ScaledTensor,
    b: &ScaledTensor,
    binomials: &mut Binomials,
) -> Result<Tensor> {
    let mut c = t_make(a.p + b.p, a.q + b.q);
    let (aw, bw, cw) = (a.q + 1, b.q + 1, c.q + 1);
    for i in 0..=a.p {
        for (j, &av) in row(&a.c, i, aw)?.iter().enumerate() {
            if av == 0.0 {
                continue;
            }
            for k in 0..=b.p {
                let start = (i + k) * cw + j;
                let out = c.c.get_mut(start..start + bw).ok_or_else(sm_err)?;
                for (cv, &bv) in out.iter_mut().zip(row(&b.c, k, bw)?) {
                    *cv += av * bv;
                }
            }
        }
    }
    let bq = binomials.row(c.q)?.to_vec();
    let bp = binomials.row(c.p)?;
    for (i, &bpi) in bp.iter().enumerate() {
        let start = i * cw;
        let out = c.c.get_mut(start..start + cw).ok_or_else(sm_err)?;
        for (x, &bqj) in out.iter_mut().zip(&bq) {
            let f = bpi * bqj;
            *x /= f;
        }
    }
    Ok(c)
}

/// `sm_SbTMulFast`: `a b`, as `sm_SbTMul` but the binomials taken once: the coefficients
/// scaled by them, convolved, and unscaled.
pub(super) fn t_mul_fast(a: &Tensor, b: &Tensor) -> Result<Tensor> {
    t_mul_with(a, b, &mut Binomials::default())
}

/// [`t_mul_fast`] with its binomials from `binomials`.
fn t_mul_with(a: &Tensor, b: &Tensor, binomials: &mut Binomials) -> Result<Tensor> {
    let sa = t_scaled(a, binomials)?;
    let sb = t_scaled(b, binomials)?;
    t_mul_scaled(&sa, &sb, binomials)
}

/// `sm_SbTDet3`: the determinant of a 3 x 3 matrix of polynomials.
pub(super) fn t_det3(m: [[&Tensor; 3]; 3]) -> Result<Tensor> {
    t_det3_with(m, &mut Binomials::default())
}

/// [`t_det3`] with its binomials from `binomials`.
fn t_det3_with(m: [[&Tensor; 3]; 3], binomials: &mut Binomials) -> Result<Tensor> {
    let mut s = t_make(0, 0);
    for j in 0..3 {
        let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
        let [r0, r1, r2] = m;
        let minor = t_axpy(
            &t_mul_with(el(&r1, j1)?, el(&r2, j2)?, binomials)?,
            -1.0,
            &t_mul_with(el(&r1, j2)?, el(&r2, j1)?, binomials)?,
        )?;
        s = t_axpy(&s, 1.0, &t_mul_with(el(&r0, j)?, &minor, binomials)?)?;
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

/// `sm_SbTDet4`'s minors: the 3 x 3 minors of the first three rows `rows` of a 4 x 4 matrix
/// of polynomials, column `c` struck out in minor `c`.
fn det4_minors(rows: [&[Tensor; 4]; 3], binomials: &mut Binomials) -> Result<[Tensor; 4]> {
    let mut minors: [Tensor; 4] = Default::default();
    for (c, minor) in minors.iter_mut().enumerate() {
        *minor = t_det3_with(minor_without(rows, c)?, binomials)?;
    }
    Ok(minors)
}

/// `sm_SbTDet4`: the determinant along its last row `last`, from the minors of its other
/// rows ([`det4_minors`]): a determinant whose first three rows recur takes their minors
/// once.
fn t_det4_along(
    last: &[Tensor; 4],
    minors: &[Tensor; 4],
    binomials: &mut Binomials,
) -> Result<Tensor> {
    let mut s = t_make(0, 0);
    for (c, minor) in minors.iter().enumerate() {
        let sign = if (3 + c) % 2 == 0 { 1.0 } else { -1.0 };
        s = t_axpy(&s, sign, &t_mul_with(el_ref(last, c)?, minor, binomials)?)?;
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
    let mut binomials = Binomials::default();
    // the minors of H, Hu, Hv, the same for every last row: taken once
    let minors = det4_minors([&rows.h, &rows.hu, &rows.hv], &mut binomials)?;
    let mut out: [Tensor; 4] = Default::default();
    for (slot, row) in out.iter_mut().zip(last) {
        // along the last row: det = sum_c (-1)^(3 + c) last[c] minor(3, c)
        *slot = t_det4_along(row, &minors, &mut binomials)?;
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
    affine_fundamentals_with(p, e, &mut Binomials::default())
}

/// [`affine_fundamentals`] with its binomials from `binomials`. Every product is
/// [`t_mul_fast`]'s, in its order, but a factor in many products is scaled by its binomials
/// once (the cofactors, each unknown's share and its partials), the minors of `H, Hu, Hv`
/// are taken once for `o`, `l`, `m` and `n`, and the binomials are read from the table: the
/// same operations on the same values, so the same bits.
fn affine_fundamentals_with(
    p: &AffinePatch,
    e: Vec3,
    binomials: &mut Binomials,
) -> Result<AffineFundamentals> {
    let rows = rows_of(&p.base, e)?;
    let base_minors = det4_minors([&rows.h, &rows.hu, &rows.hv], binomials)?;
    let o = t_det4_along(&rows.one, &base_minors, binomials)?;
    // each unknown's share and its first partials, the same for l, m and n: scaled once
    let mut z_scaled = Vec::new();
    for zt in &p.z {
        let (zu, zv) = (t_du(zt)?, t_dv(zt)?);
        let scaled = [
            t_scaled(zt, binomials)?,
            t_scaled(&zu, binomials)?,
            t_scaled(&zv, binomials)?,
        ];
        z_scaled.push((zu, zv, scaled));
    }
    let mut q0: [Tensor; 3] = Default::default();
    let mut qj: [Vec<Tensor>; 3] = Default::default();
    for k in 0..3 {
        let hx = el_ref(&rows.hx, k)?;
        let prows = [&rows.h, &rows.hu, &rows.hv, hx];
        *el_mut(&mut q0, k)? = t_det4_along(hx, &base_minors, binomials)?;
        // the cofactors: row r replaced by E (row 3's is o)
        let mut cof: [Tensor; 4] = Default::default();
        for r in 0..3 {
            let mut pr = prows;
            *el_mut(&mut pr, r)? = &rows.one;
            let [r0, r1, r2, r3] = pr;
            let minors = det4_minors([r0, r1, r2], binomials)?;
            *el_mut(&mut cof, r)? = t_det4_along(r3, &minors, binomials)?;
        }
        cof[3] = o.clone();
        // each cofactor is a factor of every unknown's products: scaled once
        let mut cof_scaled = Vec::new();
        for c in &cof {
            cof_scaled.push(t_scaled(c, binomials)?);
        }
        let [c0, c1, c2, c3] = cof_scaled.as_slice() else {
            return Err(sm_err());
        };
        let out = el_mut(&mut qj, k)?;
        out.clear();
        for (zu, zv, [st, su, sv]) in &z_scaled {
            let zx = match k {
                0 => t_du(zu)?,
                1 => t_dv(zu)?,
                _ => t_dv(zv)?,
            };
            let mut s = t_mul_scaled(st, c0, binomials)?;
            s = t_axpy(&s, 1.0, &t_mul_scaled(su, c1, binomials)?)?;
            s = t_axpy(&s, 1.0, &t_mul_scaled(sv, c2, binomials)?)?;
            s = t_axpy(
                &s,
                1.0,
                &t_mul_scaled(&t_scaled(&zx, binomials)?, c3, binomials)?,
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        /// The binomials the reference products evaluate.
        static EVALUATIONS: Cell<usize> = const { Cell::new(0) };
    }

    fn counted_binomial(n: usize, k: usize) -> f64 {
        EVALUATIONS.with(|e| e.set(e.get() + 1));
        binomial(n, k).unwrap()
    }

    /// `sm_SbTMulFast` as it was ported before the table: each coefficient's binomials
    /// evaluated where it is scaled and unscaled.
    fn reference_mul(a: &Tensor, b: &Tensor) -> Tensor {
        let mut c = t_make(a.p + b.p, a.q + b.q);
        let mut sa = vec![0.0; a.c.len()];
        let mut sb = vec![0.0; b.c.len()];
        for i in 0..=a.p {
            for j in 0..=a.q {
                sa[i * (a.q + 1) + j] =
                    a.c[i * (a.q + 1) + j] * counted_binomial(a.p, i) * counted_binomial(a.q, j);
            }
        }
        for i in 0..=b.p {
            for j in 0..=b.q {
                sb[i * (b.q + 1) + j] =
                    b.c[i * (b.q + 1) + j] * counted_binomial(b.p, i) * counted_binomial(b.q, j);
            }
        }
        for i in 0..=a.p {
            for j in 0..=a.q {
                let av = sa[i * (a.q + 1) + j];
                if av == 0.0 {
                    continue;
                }
                for k in 0..=b.p {
                    let row = (i + k) * (c.q + 1) + j;
                    for l in 0..=b.q {
                        c.c[row + l] += av * sb[k * (b.q + 1) + l];
                    }
                }
            }
        }
        for i in 0..=c.p {
            for j in 0..=c.q {
                let f = counted_binomial(c.p, i) * counted_binomial(c.q, j);
                c.c[i * (c.q + 1) + j] /= f;
            }
        }
        c
    }

    fn reference_det3(m: [[&Tensor; 3]; 3]) -> Tensor {
        let mut s = t_make(0, 0);
        for j in 0..3 {
            let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
            let [r0, r1, r2] = m;
            let minor = t_axpy(
                &reference_mul(r1[j1], r2[j2]),
                -1.0,
                &reference_mul(r1[j2], r2[j1]),
            )
            .unwrap();
            s = t_axpy(&s, 1.0, &reference_mul(r0[j], &minor)).unwrap();
        }
        s
    }

    fn reference_det4(m: [&[Tensor; 4]; 4]) -> Tensor {
        let mut s = t_make(0, 0);
        let [r0, r1, r2, r3] = m;
        for (c, last) in r3.iter().enumerate() {
            let pm = minor_without([r0, r1, r2], c).unwrap();
            let sign = if (3 + c) % 2 == 0 { 1.0 } else { -1.0 };
            s = t_axpy(&s, sign, &reference_mul(last, &reference_det3(pm))).unwrap();
        }
        s
    }

    /// `sm_SbAffineFundamentals` as it was ported before the table: every product of every
    /// determinant and of every unknown's share taken afresh.
    fn reference_affine_fundamentals(p: &AffinePatch, e: Vec3) -> AffineFundamentals {
        let rows = rows_of(&p.base, e).unwrap();
        let o = reference_det4([&rows.h, &rows.hu, &rows.hv, &rows.one]);
        let mut q0: [Tensor; 3] = Default::default();
        let mut qj: [Vec<Tensor>; 3] = Default::default();
        for k in 0..3 {
            let prows = [&rows.h, &rows.hu, &rows.hv, &rows.hx[k]];
            q0[k] = reference_det4(prows);
            let mut cof: [Tensor; 4] = Default::default();
            for r in 0..3 {
                let mut pr = prows;
                pr[r] = &rows.one;
                cof[r] = reference_det4(pr);
            }
            cof[3] = o.clone();
            for zt in &p.z {
                let zu = t_du(zt).unwrap();
                let zv = t_dv(zt).unwrap();
                let zx = match k {
                    0 => t_du(&zu).unwrap(),
                    1 => t_dv(&zu).unwrap(),
                    _ => t_dv(&zv).unwrap(),
                };
                let mut s = reference_mul(zt, &cof[0]);
                s = t_axpy(&s, 1.0, &reference_mul(&zu, &cof[1])).unwrap();
                s = t_axpy(&s, 1.0, &reference_mul(&zv, &cof[2])).unwrap();
                s = t_axpy(&s, 1.0, &reference_mul(&zx, &cof[3])).unwrap();
                qj[k].push(s);
            }
        }
        AffineFundamentals { q0, qj, o }
    }

    /// A fixed-seed generator of coefficients in [-1, 1), with exact zeros (both signs)
    /// sprinkled in: the products skip a zero scaled coefficient.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let x = ((self.0 >> 11) as f64) / ((1u64 << 53) as f64) * 2.0 - 1.0;
            match self.0 % 11 {
                0 => 0.0,
                1 => -0.0,
                _ => x,
            }
        }

        fn tensor(&mut self, p: usize, q: usize) -> Tensor {
            let mut t = t_make(p, q);
            for c in &mut t.c {
                *c = self.next();
            }
            t
        }
    }

    fn same_bits(a: &Tensor, b: &Tensor) {
        assert_eq!((a.p, a.q), (b.p, b.q));
        for (x, y) in a.c.iter().zip(&b.c) {
            assert_eq!(x.to_bits(), y.to_bits(), "{x} vs {y}");
        }
    }

    #[test]
    fn products_from_the_table_are_the_per_coefficient_products_bit_for_bit() {
        let mut rng = Lcg(1557);
        let mut binomials = Binomials::default();
        for (p, q, r, s) in [
            (0, 0, 0, 0),
            (0, 3, 2, 0),
            (1, 1, 9, 9),
            (3, 9, 12, 27),
            (9, 9, 9, 9),
            (2, 5, 0, 7),
        ] {
            let (a, b) = (rng.tensor(p, q), rng.tensor(r, s));
            let want = reference_mul(&a, &b);
            same_bits(&t_mul_fast(&a, &b).unwrap(), &want);
            same_bits(&t_mul_with(&a, &b, &mut binomials).unwrap(), &want);
            let (sa, sb) = (
                t_scaled(&a, &mut binomials).unwrap(),
                t_scaled(&b, &mut binomials).unwrap(),
            );
            same_bits(&t_mul_scaled(&sa, &sb, &mut binomials).unwrap(), &want);
        }
    }

    /// A rational net of degree (3, 7) about a point away from the origin, its weights in
    /// [0.5, 1.5], moved along e by five unknowns (four dense shares, one a single
    /// coefficient), as the setback fill's patches are.
    fn rational_patch(rng: &mut Lcg) -> AffinePatch {
        let (p, q) = (3, 7);
        let mut pts = Vec::new();
        for i in 0..=p {
            for j in 0..=q {
                let w = 1.0 + 0.5 * rng.next();
                let (x, y, z) = (
                    f64::from(i as u32) + 0.1 * rng.next(),
                    f64::from(j as u32) + 0.1 * rng.next(),
                    0.3 * rng.next(),
                );
                pts.push([(x + 20.0) * w, (y - 7.0) * w, (z + 3.0) * w, w]);
            }
        }
        let mut z: Vec<Tensor> = (0..4).map(|_| rng.tensor(p, q)).collect();
        let mut single = t_make(p, q);
        single.c[9] = 1.0;
        z.push(single);
        AffinePatch {
            base: Net { p, q, pts },
            index: (0..z.len()).collect(),
            z,
        }
    }

    #[test]
    fn affine_fundamentals_are_the_forks_bit_for_bit_with_each_binomial_evaluated_once() {
        let mut rng = Lcg(14460);
        let patch = rational_patch(&mut rng);
        let e = Vec3::new(0.3, -0.2, 1.0);
        EVALUATIONS.with(|c| c.set(0));
        let want = reference_affine_fundamentals(&patch, e);
        let forks = EVALUATIONS.with(Cell::get);
        let mut binomials = Binomials::default();
        let got = affine_fundamentals_with(&patch, e, &mut binomials).unwrap();
        same_bits(&got.o, &want.o);
        for k in 0..3 {
            same_bits(&got.q0[k], &want.q0[k]);
            assert_eq!(got.qj[k].len(), want.qj[k].len());
            for (a, b) in got.qj[k].iter().zip(&want.qj[k]) {
                same_bits(a, b);
            }
        }
        // The work counted, never timed: one evaluation per binomial up to the products'
        // largest degree, where the fork's evaluates two per coefficient of every factor and
        // of every product.
        let largest = [&got.o, &got.q0[0], &got.q0[1], &got.q0[2]]
            .iter()
            .map(|t| t.p.max(t.q))
            .max()
            .unwrap();
        assert!(
            binomials.evaluations <= (largest + 1) * (largest + 2) / 2,
            "{} evaluations",
            binomials.evaluations
        );
        assert!(
            forks > 500 * binomials.evaluations,
            "{forks} against {}",
            binomials.evaluations
        );
        // and the curvature determinants of the net itself
        let net = &patch.base;
        let rows = rows_of(net, e).unwrap();
        let [huu, huv, hvv] = &rows.hx;
        let got = fundamentals(net, e).unwrap();
        for (k, last) in [huu, huv, hvv, &rows.one].into_iter().enumerate() {
            same_bits(
                &got[k],
                &reference_det4([&rows.h, &rows.hu, &rows.hv, last]),
            );
        }
    }
}
