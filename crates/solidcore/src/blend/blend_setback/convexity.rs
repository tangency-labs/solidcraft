//! The convexity programme of a fill whose patches move along one direction e, affine in
//! their unknowns (the fork's `sm_SbGraphG1`'s sides and sector conditions, moved out of
//! [`super::graph`] unchanged so that a fill of any order calls them).
//!
//! For a patch `P + H e` with its projection P fixed, the curvature determinants `l`, `m`,
//! `n` are affine in the heights H and the side `o` of its normal is fixed
//! ([`super::certify::affine_fundamentals`]); nothing here depends on the patches' degrees or
//! on the fill's order. The body stays convex where `s [[l, m], [m, n]]` is positive
//! semidefinite on every patch: [`sector_conditions`] are linear sufficient conditions for it
//! on the Bernstein coefficient matrices of each patch's pieces (sector conditions over
//! directions on a half turn, and a patch's own singular direction where it has one), as
//! `C t >= b` in the free unknowns t. Rows with no free unknown in them (a patch's rows fixed
//! by its data: a G2 patch's rows along a round or a face, where `K = 0`) are left to the
//! certificate ([`super::certify::certified_convex`]).

use super::certify::{affine_fundamentals, t_make, t_nonneg, t_pieces, t_scale, AffinePatch};
use super::{el, el_mut, el_ref, smax, tensor_elevate, Tensor};
use crate::error::Result;
use crate::math::{Vec3, PI};

/// `SM_SB_GRAPH_LEVELS`: the sector conditions' halvings.
pub(super) const GRAPH_LEVELS: usize = 1;
/// `SM_SB_GRAPH_DIRECTIONS`: their directions over a half turn.
pub(super) const GRAPH_DIRECTIONS: usize = 16;

/// A patch's curvature determinants as affine functions of the unknowns, and its side.
pub(super) struct PatchCurvatures {
    /// `l`, `m`, `n` at the unknowns 0.
    pub q0: [Tensor; 3],
    /// Each unknown's share in `l`, `m`, `n` (in the patch's `index` order).
    pub qj: [Vec<Tensor>; 3],
    /// The sign of `o` over the whole patch (1 or -1), certified.
    pub side: f64,
}

/// Each patch's curvature determinants, affine in its unknowns, and the side its normal
/// faces: `o` of one sign, certified to `1e-12` of its scale halving up to `depth` times.
/// `Ok(Err(p))`: patch p's `o` is not certified of one sign (the fill folds).
pub(super) fn patch_curvatures(
    patches: &[AffinePatch],
    e: Vec3,
    depth: usize,
) -> Result<core::result::Result<Vec<PatchCurvatures>, usize>> {
    let mut out = Vec::with_capacity(patches.len());
    for (p, patch) in patches.iter().enumerate() {
        let af = affine_fundamentals(patch, e)?;
        let mut neg_o = af.o.clone();
        for x in &mut neg_o.c {
            *x = -*x;
        }
        let d_o = 1.0e-12 * t_scale(&af.o);
        let s = if t_nonneg(&af.o, d_o, depth)? == 1 {
            1.0
        } else if t_nonneg(&neg_o, d_o, depth)? == 1 {
            -1.0
        } else {
            0.0
        };
        if s == 0.0 {
            return Ok(Err(p));
        }
        out.push(PatchCurvatures {
            q0: af.q0,
            qj: af.qj,
            side: s,
        });
    }
    Ok(Ok(out))
}

/// The sector conditions `C t >= b` (`c` row-major, `free` columns), held `1e-12` of each
/// patch's scale inside.
pub(super) struct SectorConditions {
    /// `C`, row-major.
    pub c: Vec<f64>,
    /// `b`.
    pub b: Vec<f64>,
}

/// The sector conditions on every patch's pieces' coefficient matrices, over the free
/// unknowns t (the unknowns `y = y_particular + sum_f t_f basis[f]`): `kernel[p]` a direction
/// (in `[0, pi)`) where patch p's second fundamental form is forced singular, or negative.
pub(super) fn sector_conditions(
    patches: &[AffinePatch],
    curvatures: &[PatchCurvatures],
    kernel: &[f64],
    yp: &[f64],
    zn: &[Vec<f64>],
    free: usize,
) -> Result<SectorConditions> {
    let mut cm: Vec<f64> = Vec::new();
    let mut bv: Vec<f64> = Vec::new();
    for (p, patch) in patches.iter().enumerate() {
        let mut theta: Vec<f64> = (0..GRAPH_DIRECTIONS)
            .map(|k| PI * k as f64 / GRAPH_DIRECTIONS as f64)
            .collect();
        let kp = el(kernel, p)?;
        if kp >= 0.0 {
            theta.push(kp);
        }
        theta.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
        theta.push(el(&theta, 0)? + PI);
        let q0p = &el_ref(curvatures, p)?.q0;
        let qjp = &el_ref(curvatures, p)?.qj;
        let (mut big_p, mut big_q) = (0, 0);
        for t in q0p {
            big_p = if big_p > t.p { big_p } else { t.p };
            big_q = if big_q > t.q { big_q } else { t.q };
        }
        // l, m, n over t: at t = 0 (y = yp) and per free direction, at common degrees, in
        // pieces
        let mut pc0: Vec<Vec<Tensor>> = vec![Vec::new(); 3];
        let mut pcf: Vec<Vec<Tensor>> = vec![Vec::new(); 3 * free];
        let mut scale = 0.0;
        for k in 0..3 {
            let q0k = el_ref(q0p, k)?;
            let mut s0 = tensor_elevate(q0k, big_p, big_q)?;
            let mut sf = vec![t_make(big_p, big_q); free];
            let qjk = el_ref(qjp, k)?;
            for (t, &u) in patch.index.iter().enumerate() {
                let sj = tensor_elevate(el_ref(qjk, t)?, big_p, big_q)?;
                scale = smax(scale, t_scale(&sj));
                let ypu = el(yp, u)?;
                for (s0i, &sji) in s0.c.iter_mut().zip(&sj.c) {
                    *s0i += ypu * sji;
                }
                for (f, sff) in sf.iter_mut().enumerate() {
                    let z = el(el_ref(zn, f)?, u)?;
                    if z == 0.0 {
                        continue;
                    }
                    for (sfi, &sji) in sff.c.iter_mut().zip(&sj.c) {
                        *sfi += z * sji;
                    }
                }
            }
            scale = smax(scale, t_scale(q0k));
            *el_mut(&mut pc0, k)? = t_pieces(&s0, GRAPH_LEVELS)?;
            for (f, sff) in sf.iter().enumerate() {
                *el_mut(&mut pcf, k * free + f)? = t_pieces(sff, GRAPH_LEVELS)?;
            }
        }
        let sp = el_ref(curvatures, p)?.side;
        let mut row_t = vec![0.0; free];
        let pc00 = el_ref(&pc0, 0)?;
        for pc in 0..pc00.len() {
            for i in 0..el_ref(pc00, pc)?.c.len() {
                for a in 0..theta.len() - 1 {
                    for pair in 0..2 {
                        let ta = el(&theta, a)?;
                        let (c0, s0) = (ta.cos(), ta.sin());
                        let (c1, s1) = if pair == 0 {
                            (c0, s0)
                        } else {
                            let tb = el(&theta, a + 1)?;
                            (tb.cos(), tb.sin())
                        };
                        let w = [c0 * c1, c0 * s1 + s0 * c1, s0 * s1];
                        let mut cst = 0.0;
                        let mut dmax = 0.0;
                        for (k, &wk) in w.iter().enumerate() {
                            cst += sp * wk * el(&el_ref(el_ref(&pc0, k)?, pc)?.c, i)?;
                        }
                        for (f, rf) in row_t.iter_mut().enumerate() {
                            let mut s = 0.0;
                            for (k, &wk) in w.iter().enumerate() {
                                s += sp * wk * el(&el_ref(el_ref(&pcf, k * free + f)?, pc)?.c, i)?;
                            }
                            *rf = s;
                            dmax = smax(dmax, s.abs());
                        }
                        if !(dmax > 1.0e-11 * scale) {
                            continue; // no free height in it: the certificate decides it
                        }
                        for &rf in &row_t {
                            cm.push(rf / dmax);
                        }
                        bv.push((-cst + 1.0e-12 * scale) / dmax); // held a little inside
                    }
                }
            }
        }
    }
    Ok(SectorConditions { c: cm, b: bv })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::blend_setback::{Net, Tensor};

    /// A planar degree (2, 2) net over the unit square, moved along z by one unknown times
    /// the bump `u^2 + v^2` (Bernstein coefficients `b_i + b_j`, `b = (0, 0, 1)`).
    fn bump_patch() -> AffinePatch {
        let mut pts = Vec::new();
        for i in 0..3 {
            for j in 0..3 {
                pts.push([f64::from(i) / 2.0, f64::from(j) / 2.0, 0.0, 1.0]);
            }
        }
        let b = [0.0, 0.0, 1.0];
        let mut z = Vec::new();
        for bi in b {
            for bj in b {
                z.push(bi + bj);
            }
        }
        AffinePatch {
            base: Net { p: 2, q: 2, pts },
            index: vec![0],
            z: vec![Tensor { p: 2, q: 2, c: z }],
        }
    }

    fn satisfied(sc: &SectorConditions, t: f64) -> bool {
        sc.c.iter().zip(&sc.b).all(|(&c, &b)| c * t >= b)
    }

    #[test]
    fn a_bump_is_convex_on_one_side_only() {
        let patches = [bump_patch()];
        let e = Vec3::new(0.0, 0.0, 1.0);
        let curvatures = patch_curvatures(&patches, e, 6).unwrap().unwrap();
        assert_eq!(curvatures.len(), 1);
        assert_eq!(curvatures[0].side.abs(), 1.0);
        let sc =
            sector_conditions(&patches, &curvatures, &[-1.0], &[0.0], &[vec![1.0]], 1).unwrap();
        assert!(!sc.b.is_empty());
        assert_eq!(sc.c.len(), sc.b.len());
        // z = t (u^2 + v^2) bends one way for t > 0 and the other for t < 0: exactly one of
        // them meets every sector condition
        assert_ne!(satisfied(&sc, 1.0), satisfied(&sc, -1.0));
        // and the flat patch (t = 0) sits on the boundary, held a little inside: not met
        assert!(!satisfied(&sc, 0.0));
    }
}
