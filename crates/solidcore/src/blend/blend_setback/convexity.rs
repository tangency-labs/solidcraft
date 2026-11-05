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
use super::qp::QpRows;
use super::{el, el_mut, el_ref, sm_err, smax, tensor_elevate, Tensor};
use crate::error::Result;
use crate::math::{Vec3, PI};
use core::cell::Cell;

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

/// A patch's affine curvature pieces, before expanding them into sector rows.
struct SectorPatch {
    start: usize,
    end: usize,
    coefficients: usize,
    weights: Vec<[f64; 3]>,
    side: f64,
    scale: f64,
    inside: f64,
    pc0: Vec<Vec<Tensor>>,
    pcf: Vec<Vec<Tensor>>,
}

/// Sector rows generated from the affine curvature pieces only when needed.
///
/// Slots follow patch, piece, coefficient, direction, pair order, including slots
/// whose row has no free height. Removing those slots gives exactly the stored
/// programme's row order, so an equal-slack tie selects the same constraint.
pub(super) struct SectorRows {
    free: usize,
    slots: usize,
    patches: Vec<SectorPatch>,
    rows_built: Cell<usize>,
}

impl SectorRows {
    /// An empty programme with `free` unknown heights.
    pub(super) fn new(free: usize) -> Self {
        Self {
            free,
            slots: 0,
            patches: Vec::new(),
            rows_built: Cell::new(0),
        }
    }

    /// Keep one patch's affine pieces and direction weights, not its expanded rows.
    ///
    /// `inside` is the caller's already evaluated inward margin: the planar fill
    /// uses `1e-12 * scale`, the curved fill `1e-12 * strict.factor * scale`.
    /// `theta` includes its closing direction (first angle plus pi).
    pub(super) fn add_patch(
        &mut self,
        theta: &[f64],
        side: f64,
        scale: f64,
        inside: f64,
        pc0: Vec<Vec<Tensor>>,
        pcf: Vec<Vec<Tensor>>,
    ) -> Result<()> {
        let free_components = self.free.checked_mul(3).ok_or_else(sm_err)?;
        if pc0.len() != 3 || pcf.len() != free_components || theta.len() < 2 {
            return Err(sm_err());
        }
        let first = el_ref(&pc0, 0)?;
        let pieces = first.len();
        let coefficients = first.first().map_or(0, |t| t.c.len());
        for component in pc0.iter().chain(&pcf) {
            if component.len() != pieces || component.iter().any(|t| t.c.len() != coefficients) {
                return Err(sm_err());
            }
        }
        let mut weights = Vec::new();
        for angles in theta.windows(2) {
            let ta = el(angles, 0)?;
            let (c0, s0) = (ta.cos(), ta.sin());
            for pair in 0..2 {
                let (c1, s1) = if pair == 0 {
                    (c0, s0)
                } else {
                    let tb = el(angles, 1)?;
                    (tb.cos(), tb.sin())
                };
                weights.push([c0 * c1, c0 * s1 + s0 * c1, s0 * s1]);
            }
        }
        let count = pieces
            .checked_mul(coefficients)
            .and_then(|n| n.checked_mul(weights.len()))
            .ok_or_else(sm_err)?;
        let end = self.slots.checked_add(count).ok_or_else(sm_err)?;
        self.patches.push(SectorPatch {
            start: self.slots,
            end,
            coefficients,
            weights,
            side,
            scale,
            inside,
            pc0,
            pcf,
        });
        self.slots = end;
        Ok(())
    }
}

impl QpRows for SectorRows {
    fn slot_count(&self) -> usize {
        self.slots
    }

    fn rows_built(&self) -> usize {
        self.rows_built.get()
    }

    fn violated(
        &self,
        y: &[f64],
        tol: f64,
        taken: &[bool],
        native: bool,
    ) -> Result<Vec<(f64, usize)>> {
        if y.len() != self.free || taken.len() != self.slots {
            return Err(sm_err());
        }
        // Bound both evaluation orders, including normalization and the final
        // dot product. The generous issue-specified relative bound is used only
        // when it dominates a gamma bound for this programme's operation count.
        // Each affine component uses n products and n sums; each exact row uses
        // three weighted terms per unknown, normalization, and n products/sums
        // for its slack. 16*n+64 exceeds either path length. Four gamma factors
        // allow both evaluation errors and rounding of the absolute accumulation.
        // Positive dmax preserves the sign; the native correction is nonnegative.
        // The raw/y size guards prevent intermediate overflow in the alternative
        // normalized evaluation order.
        // An absolute floor covers gradual underflow; exceptional arithmetic
        // always goes through the original normalized-row calculation below.
        let operations = 16.0 * self.free as f64 + 64.0;
        let roundoff = operations * f64::EPSILON;
        let y_size = y.iter().fold(0.0, |sum, v| sum + v.abs());
        let cheap = tol >= 0.0
            && roundoff < 0.5
            && 4.0 * roundoff / (1.0 - roundoff) <= 1.0e-10
            && y_size.is_finite()
            && y_size < f64::MAX / 16.0;
        let mut out = Vec::new();
        let mut scratch = Vec::new();
        for patch in &self.patches {
            let mut slot = patch.start;
            for piece in 0..el_ref(&patch.pc0, 0)?.len() {
                for i in 0..patch.coefficients {
                    let mut value = [0.0; 3];
                    let mut magnitude = [0.0; 3];
                    let mut raw_scale = 0.0;
                    let mut regular = cheap && patch.side.abs() == 1.0;
                    if cheap {
                        for k in 0..3 {
                            let c = el(&el_ref(el_ref(&patch.pc0, k)?, piece)?.c, i)?;
                            regular &= c.is_finite() && c.abs() <= f64::MAX / 16.0;
                            let mut v = c;
                            let mut a = c.abs();
                            for (f, &yf) in y.iter().enumerate() {
                                let component = k
                                    .checked_mul(self.free)
                                    .and_then(|v| v.checked_add(f))
                                    .ok_or_else(sm_err)?;
                                let c = el(&el_ref(el_ref(&patch.pcf, component)?, piece)?.c, i)?;
                                regular &= c.is_finite() && c.abs() <= f64::MAX / 16.0;
                                raw_scale = smax(raw_scale, c.abs());
                                let term = yf * c;
                                v += term;
                                a += term.abs();
                            }
                            *el_mut(&mut value, k)? = v;
                            *el_mut(&mut magnitude, k)? = a;
                        }
                    }
                    for w in &patch.weights {
                        let current = slot;
                        slot = slot.checked_add(1).ok_or_else(sm_err)?;
                        if *taken.get(current).ok_or_else(sm_err)? {
                            continue;
                        }
                        let mut v = 0.0;
                        let mut a = patch.inside.abs();
                        if cheap {
                            for (k, &wk) in w.iter().enumerate() {
                                let weight = patch.side * wk;
                                v += weight * el(&value, k)?;
                                a += weight.abs() * el(&magnitude, k)?;
                            }
                            v -= patch.inside;
                        }
                        // Division by dmax can itself underflow. In unnormalized
                        // units its absolute error also scales with dmax, which
                        // is at most six times the largest affine coefficient.
                        let underflow = operations
                            * f64::MIN_POSITIVE
                            * (1.0 + raw_scale)
                            * (1.0 + y_size)
                            * 8.0;
                        let bound = 1.0e-10 * a + underflow;
                        if regular
                            && w.iter().all(|v| v.is_finite() && v.abs() <= 2.0)
                            && v.is_finite()
                            && a.is_finite()
                            && v > bound
                        {
                            continue;
                        }
                        let Some(b) = self.row(current, &mut scratch)? else {
                            continue;
                        };
                        let mut slack = -b;
                        let mut size = b.abs();
                        for (&c, &yf) in scratch.iter().zip(y) {
                            let term = c * yf;
                            slack += term;
                            if native {
                                size += term.abs();
                            }
                        }
                        if native {
                            slack += (y.len() + 1) as f64 * f64::EPSILON * size;
                        }
                        if slack < -tol {
                            out.push((slack, current));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>> {
        let patch = self
            .patches
            .iter()
            .find(|patch| slot >= patch.start && slot < patch.end)
            .ok_or_else(sm_err)?;
        self.rows_built.set(self.rows_built.get().saturating_add(1));
        let local = slot.checked_sub(patch.start).ok_or_else(sm_err)?;
        let directions = patch.weights.len();
        let coefficient = local.checked_div(directions).ok_or_else(sm_err)?;
        let piece = coefficient
            .checked_div(patch.coefficients)
            .ok_or_else(sm_err)?;
        let i = coefficient
            .checked_rem(patch.coefficients)
            .ok_or_else(sm_err)?;
        let w = el_ref(
            &patch.weights,
            local.checked_rem(directions).ok_or_else(sm_err)?,
        )?;
        let mut cst = 0.0;
        let mut dmax = 0.0;
        for (k, &wk) in w.iter().enumerate() {
            cst += patch.side * wk * el(&el_ref(el_ref(&patch.pc0, k)?, piece)?.c, i)?;
        }
        scratch.resize(self.free, 0.0);
        for (f, rf) in scratch.iter_mut().enumerate() {
            let mut s = 0.0;
            for (k, &wk) in w.iter().enumerate() {
                let component = k
                    .checked_mul(self.free)
                    .and_then(|v| v.checked_add(f))
                    .ok_or_else(sm_err)?;
                s += patch.side * wk * el(&el_ref(el_ref(&patch.pcf, component)?, piece)?.c, i)?;
            }
            *rf = s;
            dmax = smax(dmax, s.abs());
        }
        if !(dmax > 1.0e-11 * patch.scale) {
            return Ok(None);
        }
        for rf in scratch {
            *rf /= dmax;
        }
        Ok(Some((-cst + patch.inside) / dmax))
    }
}

/// Prepare the planar fill's affine pieces for lazy sector-row generation.
pub(super) fn sector_rows(
    patches: &[AffinePatch],
    curvatures: &[PatchCurvatures],
    kernel: &[f64],
    yp: &[f64],
    zn: &[Vec<f64>],
    free: usize,
) -> Result<SectorRows> {
    let mut rows = SectorRows::new(free);
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
        let mut pc0: Vec<Vec<Tensor>> = vec![Vec::new(); 3];
        let components = free.checked_mul(3).ok_or_else(sm_err)?;
        let mut pcf: Vec<Vec<Tensor>> = vec![Vec::new(); components];
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
                let component = k
                    .checked_mul(free)
                    .and_then(|v| v.checked_add(f))
                    .ok_or_else(sm_err)?;
                *el_mut(&mut pcf, component)? = t_pieces(sff, GRAPH_LEVELS)?;
            }
        }
        rows.add_patch(
            &theta,
            el_ref(curvatures, p)?.side,
            scale,
            1.0e-12 * scale,
            pc0,
            pcf,
        )?;
    }
    Ok(rows)
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

    #[test]
    fn streamed_planar_rows_match_dense_bits() {
        let patches = [bump_patch(), bump_patch()];
        let curvatures = patch_curvatures(&patches, Vec3::new(0.0, 0.0, 1.0), 6)
            .unwrap()
            .unwrap();
        // Duplicate and non-fan kernel angles retain their exact sorted positions.
        for kernel in [[-1.0, -1.0], [0.0, PI / 16.0], [0.123, 2.345]] {
            for yp in [-0.7, 0.0, 1.125] {
                let basis = [vec![0.3], vec![-2.125]];
                let dense =
                    sector_conditions(&patches, &curvatures, &kernel, &[yp], &basis, 2).unwrap();
                let streamed =
                    sector_rows(&patches, &curvatures, &kernel, &[yp], &basis, 2).unwrap();
                assert_eq!(streamed.rows_built(), 0);
                let mut scratch = vec![99.0; 9];
                let mut row = 0;
                for slot in 0..streamed.slot_count() {
                    let Some(b) = streamed.row(slot, &mut scratch).unwrap() else {
                        continue;
                    };
                    assert_eq!(scratch.len(), 2);
                    assert_eq!(b.to_bits(), dense.b[row].to_bits(), "slot {slot}");
                    for (f, &c) in scratch.iter().enumerate() {
                        assert_eq!(c.to_bits(), dense.c[row * 2 + f].to_bits(), "slot {slot}");
                    }
                    row += 1;
                }
                assert_eq!(row, dense.b.len());
                assert_eq!(streamed.rows_built(), streamed.slot_count());
            }
        }
    }

    fn slot_patch() -> (Vec<Vec<Tensor>>, Vec<Vec<Tensor>>) {
        let mut pc0 = vec![Vec::new(); 3];
        let mut pcf = vec![Vec::new(); 3];
        for k in 0..3 {
            for piece in 0..2 {
                let mut constant = t_make(0, 1);
                let mut free = t_make(0, 1);
                for i in 0..2 {
                    constant.c[i] = if k == 1 {
                        0.0
                    } else {
                        -((1 + 2 * piece + i) as f64)
                    };
                    free.c[i] = if k == 1 { 0.0 } else { 1.0 };
                }
                pc0[k].push(constant);
                pcf[k].push(free);
            }
        }
        (pc0, pcf)
    }

    #[test]
    fn skipped_slots_keep_patch_piece_coefficient_and_pair_order() {
        let mut rows = SectorRows::new(1);
        for _ in 0..2 {
            let (pc0, pcf) = slot_patch();
            rows.add_patch(&[0.0, PI / 2.0, PI], 1.0, 1.0, 1e-12, pc0, pcf)
                .unwrap();
        }
        assert_eq!(rows.slot_count(), 32);
        let mut scratch = Vec::new();
        for slot in 0..rows.slot_count() {
            let b = rows.row(slot, &mut scratch).unwrap();
            assert_eq!(b.is_some(), slot % 2 == 0, "slot {slot}");
            if let Some(b) = b {
                assert_eq!(scratch, [1.0]);
                assert_eq!(b, (1 + (slot % 16) / 4) as f64 + 1e-12);
            }
        }
        assert!(rows.row(rows.slot_count(), &mut scratch).is_err());
    }

    #[test]
    fn row_threshold_and_curved_margin_keep_their_arithmetic() {
        let scale = 1.7;
        let factor = 13.25;
        let inside = 1e-12 * factor * scale;
        let threshold: f64 = 1e-11 * scale;
        for magnitude in [threshold, f64::from_bits(threshold.to_bits() + 1), 0.125] {
            let mut pc0 = Vec::new();
            let mut pcf = Vec::new();
            for k in 0..3 {
                pc0.push(vec![Tensor {
                    p: 0,
                    q: 0,
                    c: vec![if k == 0 { -0.37 } else { 0.0 }],
                }]);
                pcf.push(vec![Tensor {
                    p: 0,
                    q: 0,
                    c: vec![if k == 0 { magnitude } else { 0.0 }],
                }]);
            }
            let mut rows = SectorRows::new(1);
            rows.add_patch(&[0.0, PI], -1.0, scale, inside, pc0, pcf)
                .unwrap();
            let mut scratch = Vec::new();
            let b = rows.row(0, &mut scratch).unwrap();
            if magnitude == threshold {
                assert_eq!(b, None);
            } else {
                assert_eq!(scratch, [-1.0]);
                assert_eq!(
                    b.unwrap().to_bits(),
                    ((-0.37 + inside) / magnitude).to_bits()
                );
            }
        }
    }

    #[test]
    fn incompatible_piece_arrays_are_rejected() {
        let mut rows = SectorRows::new(1);
        let (pc0, mut pcf) = slot_patch();
        pcf[1].pop();
        assert!(rows
            .add_patch(&[0.0, PI], 1.0, 1.0, 1e-12, pc0, pcf)
            .is_err());
        assert_eq!(rows.slot_count(), 0);
    }

    struct ExactRows<'a>(&'a SectorRows);

    impl QpRows for ExactRows<'_> {
        fn slot_count(&self) -> usize {
            self.0.slot_count()
        }
        fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>> {
            self.0.row(slot, scratch)
        }
    }

    #[test]
    fn cheap_scan_preserves_exact_violations_and_native_rounding() {
        for scale in [1.0, 1e-300, 1e280, f64::INFINITY, f64::NAN] {
            for side in [-1.0, 1.0] {
                let (mut pc0, mut pcf) = slot_patch();
                for t in pc0.iter_mut().chain(&mut pcf).flatten() {
                    for c in &mut t.c {
                        *c *= scale;
                    }
                }
                let mut rows = SectorRows::new(1);
                rows.add_patch(
                    &[0.0, 0.0, 0.123, PI / 2.0, PI],
                    side,
                    scale,
                    1e-12 * 13.25 * scale,
                    pc0,
                    pcf,
                )
                .unwrap();
                for y in [
                    -1e300,
                    -5.0,
                    0.0,
                    1.0,
                    4.0,
                    5.0,
                    1e300,
                    f64::INFINITY,
                    f64::NAN,
                ] {
                    for native in [false, true] {
                        let taken: Vec<_> = (0..rows.slot_count()).map(|i| i % 7 == 0).collect();
                        let expected = ExactRows(&rows)
                            .violated(&[y], 1e-12, &taken, native)
                            .unwrap();
                        let actual = rows.violated(&[y], 1e-12, &taken, native).unwrap();
                        let bits = |v: Vec<(f64, usize)>| {
                            v.into_iter()
                                .map(|(slack, slot)| (slack.to_bits(), slot))
                                .collect::<Vec<_>>()
                        };
                        assert_eq!(
                            bits(actual),
                            bits(expected),
                            "scale={scale} side={side} y={y} native={native}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn cheap_scan_skips_satisfied_rows_but_keeps_margin_boundary() {
        let (pc0, pcf) = slot_patch();
        let mut rows = SectorRows::new(1);
        rows.add_patch(&[0.0, 0.0, PI / 2.0, PI], 1.0, 1.0, 1e-12, pc0, pcf)
            .unwrap();
        let taken = vec![false; rows.slot_count()];
        assert!(rows
            .violated(&[5.0], 0.0, &taken, false)
            .unwrap()
            .is_empty());
        assert!(rows.rows_built() < rows.slot_count());
        for y in [1.0, 1.0 + 1e-12, 4.0, 4.0 + 1e-12] {
            assert_eq!(
                rows.violated(&[y], 0.0, &taken, false).unwrap(),
                ExactRows(&rows).violated(&[y], 0.0, &taken, false).unwrap()
            );
        }
    }

    #[test]
    fn constructor_rejects_overflow_and_handles_empty_shapes() {
        let (pc0, pcf) = slot_patch();
        assert!(SectorRows::new(usize::MAX)
            .add_patch(&[0.0, PI], 1.0, 1.0, 0.0, pc0, pcf)
            .is_err());
        let mut rows = SectorRows::new(0);
        rows.add_patch(&[0.0, PI], 1.0, 0.0, 0.0, vec![vec![]; 3], vec![])
            .unwrap();
        assert_eq!(rows.slot_count(), 0);
        assert!(rows.row(0, &mut Vec::new()).is_err());
        assert!(rows.violated(&[], 0.0, &[], false).unwrap().is_empty());
        assert!(rows
            .add_patch(&[], 1.0, 0.0, 0.0, vec![vec![]; 3], vec![])
            .is_err());
    }

    #[test]
    fn normalization_underflow_requires_exact_fallback() {
        let scalar = |c| {
            vec![Tensor {
                p: 0,
                q: 0,
                c: vec![c],
            }]
        };
        let mut rows = SectorRows::new(2);
        rows.add_patch(
            &[0.0, 0.0],
            1.0,
            1.0,
            0.0,
            vec![scalar(-0.5), scalar(0.0), scalar(0.0)],
            vec![
                scalar(1e-100),
                scalar(1e300),
                scalar(0.0),
                scalar(0.0),
                scalar(0.0),
                scalar(0.0),
            ],
        )
        .unwrap();
        // The unnormalized value is positive, but the first normalized row
        // coefficient rounds to zero. The exact programme therefore violates it.
        let y = [1e100, 0.0];
        let taken = vec![false; rows.slot_count()];
        let expected = ExactRows(&rows).violated(&y, 0.0, &taken, false).unwrap();
        assert_eq!(expected.len(), 2);
        assert_eq!(rows.violated(&y, 0.0, &taken, false).unwrap(), expected);
    }

    #[test]
    fn full_direction_fan_avoids_materializing_satisfied_rows() {
        let (pc0, pcf) = slot_patch();
        let theta: Vec<_> = (0..=GRAPH_DIRECTIONS)
            .map(|i| PI * i as f64 / GRAPH_DIRECTIONS as f64)
            .collect();
        let mut rows = SectorRows::new(1);
        rows.add_patch(&theta, 1.0, 1.0, 1e-12, pc0, pcf).unwrap();
        let taken = vec![false; rows.slot_count()];
        assert_eq!(rows.slot_count(), 128);
        assert!(rows.violated(&[5.0], 0.0, &taken, true).unwrap().is_empty());
        assert_eq!(rows.rows_built(), 0);
        assert!(ExactRows(&rows)
            .violated(&[5.0], 0.0, &taken, true)
            .unwrap()
            .is_empty());
        assert_eq!(rows.rows_built(), rows.slot_count());
    }

    #[test]
    fn long_signed_affine_scan_matches_exact_near_inward_boundary() {
        const FREE: usize = 154;
        let scalar = |c| {
            vec![Tensor {
                p: 0,
                q: 0,
                c: vec![c],
            }]
        };
        let y: Vec<_> = (0..FREE)
            .map(|f| if f % 3 == 0 { -0.75 } else { 1.125 })
            .collect();
        let mut skipped = 0;
        let mut built = 0;
        for side in [-1.0, 1.0] {
            for delta in [-1.0, -1e-8, -1e-12, 0.0, 1e-12, 1e-8, 1.0] {
                let mut pc0 = Vec::new();
                let mut pcf = Vec::new();
                for k in 0..3 {
                    let mut sum = 0.0;
                    for (f, &yf) in y.iter().enumerate() {
                        let sign = if (f + k) % 2 == 0 { 1.0 } else { -1.0 };
                        let c = sign * ((f % 7 + 1) as f64 / 13.0) * (k + 1) as f64;
                        sum += yf * c;
                        pcf.push(scalar(c));
                    }
                    pc0.push(scalar(-sum + delta));
                }
                let mut rows = SectorRows::new(FREE);
                rows.add_patch(
                    &[0.0, 0.0, 0.123, PI / 2.0, PI],
                    side,
                    1.0,
                    1e-12 * 13.25,
                    pc0,
                    pcf,
                )
                .unwrap();
                let taken = vec![false; rows.slot_count()];
                for native in [false, true] {
                    let before = rows.rows_built();
                    let actual = rows.violated(&y, 0.0, &taken, native).unwrap();
                    let exact_builds = rows.rows_built() - before;
                    built += exact_builds;
                    skipped += rows.slot_count() - exact_builds;
                    let expected = ExactRows(&rows).violated(&y, 0.0, &taken, native).unwrap();
                    assert_eq!(
                        actual, expected,
                        "side={side} delta={delta} native={native}"
                    );
                }
            }
        }
        assert!(built > 0, "near-boundary rows require exact arithmetic");
        assert!(skipped > 0, "well-inside rows still use the cheap bound");
    }

    #[test]
    fn native_row_order_fixture_matches_integrated_streamed_solver() {
        use super::super::qp::{dual_qp_source, QpOutcome, QpStats};

        // Same inputs as harness/cpp/tests/blend_setback_streamed_row_order.cpp:
        // two patches, one coefficient, four direction pairs, alternating skips.
        let mut rows = SectorRows::new(1);
        for _ in 0..2 {
            let mut pc0 = Vec::new();
            let mut pcf = Vec::new();
            for k in 0..3 {
                let mut constant = t_make(0, 0);
                let mut free = t_make(0, 0);
                constant.c[0] = if k == 1 { 0.0 } else { -1.0 };
                free.c[0] = if k == 1 { 0.0 } else { 1.0 };
                pc0.push(vec![constant]);
                pcf.push(vec![free]);
            }
            rows.add_patch(&[0.0, PI / 2.0, PI], 1.0, 1.0, 1e-12, pc0, pcf)
                .unwrap();
        }
        assert_eq!(rows.slot_count(), 8);
        let mut stats = QpStats::default();
        let result = dual_qp_source(&[1.0], &[0.0], &rows, 1, 1e-13, false, &mut stats).unwrap();
        assert_eq!(result, QpOutcome::Solved(vec![1.0 + 1e-12]));
        assert_eq!(stats.peak_rows_held, 4);
        assert_eq!(stats.steps, 3);
    }
}
