//! The strictly convex quadratic programmes of a G1 fill's heights (the fork's `sm_SbDualQP`,
//! `sm_SbDualQPRows`, tangency-labs/smlib 08d4b02, f1d81f4; smlib#104): the dual active-set
//! method (Goldfarb and Idnani).
//!
//! Deviations: the status and the solution are one return value ([`QpOutcome`]); the fork's
//! `*pbProven` is its field. `std::partial_sort` of the violated rows is a full sort of the
//! pairs `(slack, row)`, a total order on distinct rows, so the rows taken are the same.

use super::{el, el_mut, sm_err, smax, smin};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use core::cell::Cell;

/// Deterministic work done by a constraint-source solve.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QpStats {
    /// Exact row materialization attempts, including slots found to have no row.
    pub rows_built: usize,
    /// Maximum number of selected rows held by the working set.
    pub peak_rows_held: usize,
    /// Dual active-set iterations, including the final feasibility scan.
    pub steps: usize,
}

/// What a programme came to.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum QpOutcome {
    /// `SM_SUCCESS`: every constraint holds within the tolerance, at `y`.
    Solved(Vec<f64>),
    /// `SM_ERR`: no `y` meets the constraints, or no progress within the step limit;
    /// `proven` (`*pbProven`): shown infeasible from the rows themselves.
    Infeasible {
        /// `*pbProven`.
        proven: bool,
    },
    /// `SM_ERR_INVALID_INPUT`: `G` is not positive definite.
    NotDefinite,
}

/// A row-major element `m[i n + j]`, `SM_ERR` past its end.
fn at(m: &[f64], i: usize) -> Result<f64> {
    el(m, i)
}

/// The dual method's state (`sm_SbDualQP`'s locals): `J` (`J' G J = I`, turned as
/// constraints join), the solution, the active rows and their multipliers, and `R`. Kept
/// between calls, it continues the method on more rows ([`dual_qp_rows`] beside a curved
/// face, #2343).
pub(super) struct DualState {
    jm: Vec<f64>,
    y: Vec<f64>,
    active: Vec<usize>,
    u: Vec<f64>,
    rm: Vec<f64>,
}

/// `sm_SbDualQP`'s start: `G = L L'`, `J = L^-T` and the unconstrained minimum; `None` where
/// `G` is not positive definite.
fn dual_start(gm: &[f64], g: &[f64], n: usize) -> Result<Option<DualState>> {
    // G = L L', J = L^-T (J' G J = I), J[r n + c] its column c
    let mut l = vec![0.0; n * n];
    for j in 0..n {
        let mut s = at(gm, j * n + j)?;
        for k in 0..j {
            s -= at(&l, j * n + k)? * at(&l, j * n + k)?;
        }
        if !(s > 0.0) {
            return Ok(None);
        }
        *el_mut(&mut l, j * n + j)? = sqrt(s);
        for i in j + 1..n {
            let mut t = at(gm, i * n + j)?;
            for k in 0..j {
                t -= at(&l, i * n + k)? * at(&l, j * n + k)?;
            }
            *el_mut(&mut l, i * n + j)? = t / at(&l, j * n + j)?;
        }
    }
    let mut jm = vec![0.0; n * n];
    for c in 0..n {
        for i in (0..n).rev() {
            let mut s = if i == c { 1.0 } else { 0.0 };
            for k in i + 1..n {
                s -= at(&l, k * n + i)? * at(&jm, k * n + c)?;
            }
            *el_mut(&mut jm, i * n + c)? = s / at(&l, i * n + i)?;
        }
    }
    // the unconstrained minimum, y = -J J' g
    let mut y = vec![0.0; n];
    {
        let mut t = vec![0.0; n];
        for (c, tc) in t.iter_mut().enumerate() {
            for r in 0..n {
                *tc += at(&jm, r * n + c)? * at(g, r)?;
            }
        }
        for (r, yr) in y.iter_mut().enumerate() {
            for (c, &tc) in t.iter().enumerate() {
                *yr -= at(&jm, r * n + c)? * tc;
            }
        }
    }
    Ok(Some(DualState {
        jm,
        y,
        active: Vec::new(),
        u: Vec::new(),
        rm: vec![0.0; n * n],
    }))
}

/// `sm_SbDualQP`'s steps from `st`, over the rows `C y >= b` (`C` `m x n`, row-major); `rounding`
/// takes a row as violated only past its own rounding bound ([`violation`]).
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // the fork's one function
fn dual_run(
    st: &mut DualState,
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
    rounding: bool,
    steps: &mut usize,
) -> Result<QpOutcome> {
    let DualState {
        jm,
        y,
        active,
        u,
        rm,
    } = st;
    let mut d = vec![0.0; n];
    let mut z = vec![0.0; n];
    let mut r = vec![0.0; n];
    let slack = |y: &[f64], i: usize| -> Result<f64> {
        let mut s = -at(b, i)?;
        for (k, &yk) in y.iter().enumerate() {
            s += at(cm, i * n + k)? * yk;
        }
        Ok(s)
    };
    let givens = |a: &mut f64, b: &mut f64| -> (f64, f64) {
        let h = a.hypot(*b);
        if h == 0.0 {
            return (1.0, 0.0);
        }
        let (cs, sn) = (*a / h, *b / h);
        *a = h;
        *b = 0.0;
        (cs, sn)
    };
    let max_steps = 50 * (m + n) + 1000;
    for _ in 0..max_steps {
        *steps = steps.saturating_add(1);
        let mut p = m;
        let mut worst = -tol;
        for i in 0..m {
            let s = if rounding {
                violation(cm, b, y, i, n)?
            } else {
                slack(y, i)?
            };
            if s < worst {
                worst = s;
                p = i;
            }
        }
        if p == m {
            return Ok(QpOutcome::Solved(y.clone()));
        }
        let mut up = 0.0;
        loop {
            let q = active.len();
            for (c, dc) in d.iter_mut().enumerate() {
                let mut s = 0.0;
                for k in 0..n {
                    s += at(jm, k * n + c)? * at(cm, p * n + k)?;
                }
                *dc = s;
            }
            // the step in y, z = J2 d2, and in the multipliers, r = R^-1 d1
            let mut dz = 0.0;
            for (k, zk) in z.iter_mut().enumerate() {
                let mut s = 0.0;
                for c in q..n {
                    s += at(jm, k * n + c)? * at(&d, c)?;
                }
                *zk = s;
                dz = smax(dz, s.abs());
            }
            for i in (0..q).rev() {
                let mut s = at(&d, i)?;
                for j in i + 1..q {
                    s -= at(rm, i * n + j)? * at(&r, j)?;
                }
                *el_mut(&mut r, i)? = s / at(rm, i * n + i)?;
            }
            let mut t1 = f64::INFINITY;
            let mut drop = q;
            for k in 0..q {
                let (rk, uk) = (at(&r, k)?, at(u, k)?);
                if rk > 0.0 && uk / rk < t1 {
                    t1 = uk / rk;
                    drop = k;
                }
            }
            let mut zn = 0.0;
            for (k, &zk) in z.iter().enumerate() {
                zn += zk * at(cm, p * n + k)?;
            }
            let t2 = if dz > 1.0e-14 && zn > 0.0 {
                -slack(y, p)? / zn
            } else {
                f64::INFINITY
            };
            let t = smin(t1, t2);
            if t == f64::INFINITY {
                // no step: n_p = N_A r (z = 0) with every r_k <= 0. For any feasible y,
                // n_p . y = sum r_k n_k . y <= sum r_k b_k: proven infeasible when b_p
                // exceeds that (checked here on the rows themselves, not trusted from the
                // factors)
                if !(dz <= 1.0e-14) {
                    return Ok(QpOutcome::Infeasible { proven: false });
                }
                let (mut resid, mut scale, mut bound) = (0.0, 1.0, 0.0);
                for k in 0..n {
                    let mut s = at(cm, p * n + k)?;
                    for (a, &act) in active.iter().enumerate() {
                        s -= at(&r, a)? * at(cm, act * n + k)?;
                    }
                    resid = smax(resid, s.abs());
                }
                for (a, &act) in active.iter().enumerate() {
                    bound += at(&r, a)? * at(b, act)?;
                    scale = smax(scale, at(&r, a)?.abs());
                }
                let bp = at(b, p)?;
                let proven = resid <= 1.0e-9 * scale && bp - bound > 1.0e-9 * (scale + bp.abs());
                return Ok(QpOutcome::Infeasible { proven });
            }
            if t2 != f64::INFINITY {
                for (yk, &zk) in y.iter_mut().zip(&z) {
                    *yk += t * zk;
                }
            }
            for (uk, &rk) in u.iter_mut().zip(&r) {
                *uk -= t * rk;
            }
            up += t;
            if t2 != f64::INFINITY && t == t2 {
                // p joins: d[q + 1 ..] rotated into d[q], J's columns alike
                let mut j = n.checked_sub(1).ok_or_else(sm_err)?;
                while j > q {
                    let (mut dj1, mut dj) = (at(&d, j - 1)?, at(&d, j)?);
                    let (cs, sn) = givens(&mut dj1, &mut dj);
                    *el_mut(&mut d, j - 1)? = dj1;
                    *el_mut(&mut d, j)? = dj;
                    for k in 0..n {
                        let (a, bb) = (at(jm, k * n + j - 1)?, at(jm, k * n + j)?);
                        *el_mut(jm, k * n + j - 1)? = cs * a + sn * bb;
                        *el_mut(jm, k * n + j)? = -sn * a + cs * bb;
                    }
                    j -= 1;
                }
                if !(at(&d, q)?.abs() > 1.0e-14) {
                    return Ok(QpOutcome::Infeasible { proven: false });
                }
                for i in 0..=q {
                    *el_mut(rm, i * n + q)? = at(&d, i)?;
                }
                active.push(p);
                u.push(up);
                break;
            }
            // drop leaves: its column out of R, R made triangular again by rotations of its
            // rows (J's columns alike)
            if drop >= active.len() {
                return Err(sm_err());
            }
            active.remove(drop);
            u.remove(drop);
            for i in 0..q {
                for j in drop..q.saturating_sub(1) {
                    *el_mut(rm, i * n + j)? = at(rm, i * n + j + 1)?;
                }
                *el_mut(rm, i * n + q - 1)? = 0.0;
            }
            for j in drop..q.saturating_sub(1) {
                let (mut a, mut bb) = (at(rm, j * n + j)?, at(rm, (j + 1) * n + j)?);
                let (cs, sn) = givens(&mut a, &mut bb);
                *el_mut(rm, j * n + j)? = a;
                *el_mut(rm, (j + 1) * n + j)? = 0.0;
                for k in j + 1..q.saturating_sub(1) {
                    let (x1, x2) = (at(rm, j * n + k)?, at(rm, (j + 1) * n + k)?);
                    *el_mut(rm, j * n + k)? = cs * x1 + sn * x2;
                    *el_mut(rm, (j + 1) * n + k)? = -sn * x1 + cs * x2;
                }
                for k in 0..n {
                    let (x1, x2) = (at(jm, k * n + j)?, at(jm, k * n + j + 1)?);
                    *el_mut(jm, k * n + j)? = cs * x1 + sn * x2;
                    *el_mut(jm, k * n + j + 1)? = -sn * x1 + cs * x2;
                }
            }
        }
    }
    Ok(QpOutcome::Infeasible { proven: false })
}

/// `sm_SbDualQP`: the strictly convex quadratic programme `min 1/2 y' G y + g' y` subject to
/// `C y >= b`, by the dual active-set method (from the unconstrained minimum, adding the
/// most violated constraint each step and dropping those whose multipliers would turn
/// negative): `G` `n x n`, `C` `m x n`, row-major.
#[cfg(test)]
pub(super) fn dual_qp(
    gm: &[f64],
    g: &[f64],
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
    rounding: bool,
) -> Result<QpOutcome> {
    let Some(mut st) = dual_start(gm, g, n)? else {
        return Ok(QpOutcome::NotDefinite);
    };
    dual_run(&mut st, cm, b, n, m, tol, rounding, &mut 0)
}

/// Row `i`'s slack `C_i y - b_i` past its own rounding bound: the slack plus
/// `(n + 1) eps (sum_k |C_ik y_k| + |b_i|)`, the bound of its dot product's rounding. Negative
/// only where the row is violated by more than its rounding can account for.
fn violation(cm: &[f64], b: &[f64], y: &[f64], i: usize, n: usize) -> Result<f64> {
    let bi = at(b, i)?;
    let mut s = -bi;
    let mut size = bi.abs();
    for (k, &yk) in y.iter().enumerate() {
        let t = at(cm, i * n + k)? * yk;
        s += t;
        size += t.abs();
    }
    Ok(s + (n + 1) as f64 * f64::EPSILON * size)
}

/// Constraints addressed by stable slots, including slots that contain no row.
///
/// Row generation must be deterministic: a slot's coefficients and right-hand side
/// cannot change between a violation scan and its addition to the working set.
pub(super) trait QpRows {
    /// The number of stable slots, including skipped ones.
    fn slot_count(&self) -> usize;

    /// Cumulative exact materialization attempts, for deterministic work accounting.
    /// Providers without work instrumentation may leave this at zero.
    fn rows_built(&self) -> usize {
        0
    }

    /// Write a slot's coefficients into reusable scratch and return its right-hand side.
    /// `None` denotes a skipped slot; its scratch contents are unspecified. A present
    /// row has exactly as many coefficients as the programme has unknowns.
    fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>>;

    /// Find unselected violated rows, retaining their stable slot identities.
    /// Providers may omit exact construction only when a bound proves a row satisfied.
    /// `native` adds the curved fill's dot-product rounding bound to the slack.
    fn violated(
        &self,
        y: &[f64],
        tol: f64,
        taken: &[bool],
        native: bool,
    ) -> Result<Vec<(f64, usize)>> {
        let mut out = Vec::new();
        let mut row = Vec::new();
        for slot in 0..self.slot_count() {
            if *taken.get(slot).ok_or_else(sm_err)? {
                continue;
            }
            let Some(b) = self.row(slot, &mut row)? else {
                continue;
            };
            if row.len() != y.len() {
                return Err(sm_err());
            }
            let mut slack = -b;
            let mut size = b.abs();
            for (&c, &yk) in row.iter().zip(y) {
                let t = c * yk;
                slack += t;
                if native {
                    size += t.abs();
                }
            }
            if native {
                slack += (y.len() + 1) as f64 * f64::EPSILON * size;
            }
            if slack < -tol {
                out.push((slack, slot));
            }
        }
        Ok(out)
    }
}

/// Dense constraints for margin probes and stored-versus-generated comparisons.
struct DenseRows<'a> {
    c: &'a [f64],
    b: &'a [f64],
    n: usize,
    m: usize,
    builds: Cell<usize>,
}

impl QpRows for DenseRows<'_> {
    fn slot_count(&self) -> usize {
        self.m
    }

    fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>> {
        self.builds.set(self.builds.get().saturating_add(1));
        let start = slot.checked_mul(self.n).ok_or_else(sm_err)?;
        let end = start.checked_add(self.n).ok_or_else(sm_err)?;
        let row = self.c.get(start..end).ok_or_else(sm_err)?;
        scratch.clear();
        scratch.extend_from_slice(row);
        Ok(Some(at(self.b, slot)?))
    }

    fn rows_built(&self) -> usize {
        self.builds.get()
    }
}

/// `sm_SbDualQPRows`: a dual QP over many rows, few of which bind: solved on a working set
/// of rows, those violated at its solution added (the most violated first, up to a block at
/// a time) until none is. Its answer is the full programme's: a solution meets every row; a
/// set of rows with no solution is one the full set has none of either (`proven` as
/// the dense dual solver's).
///
/// `native` (the curved fill's, #2343; not the fork): a row counts as violated only past its
/// own rounding bound ([`violation`]), and each round continues the dual method from the
/// last round's state on the rows added ([`DualState`]) instead of solving the working set
/// afresh. The method adds rows to a dual-feasible state as it goes, so the state solving
/// one set starts the next; the programme being strictly convex, its solution is the same.
#[cfg(test)]
pub(super) fn dual_qp_rows(
    gm: &[f64],
    g: &[f64],
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
    native: bool,
) -> Result<QpOutcome> {
    dual_qp_rows_with_stats(gm, g, cm, b, n, m, tol, native, &mut QpStats::default())
}

/// Solve a dense row source while retaining its working-set work counters.
pub(super) fn dual_qp_rows_with_stats(
    gm: &[f64],
    g: &[f64],
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
    native: bool,
    stats: &mut QpStats,
) -> Result<QpOutcome> {
    dual_qp_source(
        gm,
        g,
        &DenseRows {
            c: cm,
            b,
            n,
            m,
            builds: Cell::new(0),
        },
        n,
        tol,
        native,
        stats,
    )
}

/// Solve a programme while holding only selected constraint rows. Stable source slots
/// preserve the dense programme's violation ordering even when some slots are skipped.
/// The curved fill retains its rounding-aware scan and continued dual state.
pub(super) fn dual_qp_source(
    gm: &[f64],
    g: &[f64],
    rows: &impl QpRows,
    n: usize,
    tol: f64,
    native: bool,
    stats: &mut QpStats,
) -> Result<QpOutcome> {
    let before = rows.rows_built();
    let result = dual_qp_source_inner(gm, g, rows, n, tol, native, stats);
    stats.rows_built = stats
        .rows_built
        .saturating_add(rows.rows_built().saturating_sub(before));
    result
}

fn dual_qp_source_inner(
    gm: &[f64],
    g: &[f64],
    rows: &impl QpRows,
    n: usize,
    tol: f64,
    native: bool,
    stats: &mut QpStats,
) -> Result<QpOutcome> {
    const BLOCK: usize = 2000;
    let mut taken = vec![false; rows.slot_count()];
    let mut set: Vec<usize> = Vec::new();
    // y at the unconstrained minimum: the empty set's solution
    let Some(mut state) = dual_start(gm, g, n)? else {
        // SER: the empty programme's status (SM_ERR_INVALID_INPUT; with no rows it is
        // never infeasible)
        return Ok(QpOutcome::NotDefinite);
    };
    // The unconstrained programme completes one feasibility scan with no rows.
    stats.steps = stats.steps.saturating_add(1);
    let mut y = state.y.clone();
    let mut sc: Vec<f64> = Vec::new();
    let mut sb: Vec<f64> = Vec::new();
    let mut row = Vec::new();
    for _ in 0..200 {
        let mut violated = rows.violated(&y, tol, &taken, native)?;
        if violated.is_empty() {
            return Ok(QpOutcome::Solved(y));
        }
        let take = BLOCK.min(violated.len());
        // the pairs' order (slack, then row) is total on distinct rows
        violated.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i) in violated.iter().take(take) {
            *el_mut(&mut taken, i)? = true;
            set.push(i);
            let b = rows.row(i, &mut row)?.ok_or_else(sm_err)?;
            if row.len() != n {
                return Err(sm_err());
            }
            sc.extend_from_slice(&row);
            sb.push(b);
        }
        stats.peak_rows_held = stats.peak_rows_held.max(set.len());
        let outcome = if native {
            dual_run(
                &mut state,
                &sc,
                &sb,
                n,
                set.len(),
                tol,
                true,
                &mut stats.steps,
            )?
        } else {
            let Some(mut fresh) = dual_start(gm, g, n)? else {
                return Ok(QpOutcome::NotDefinite);
            };
            dual_run(
                &mut fresh,
                &sc,
                &sb,
                n,
                set.len(),
                tol,
                false,
                &mut stats.steps,
            )?
        };
        match outcome {
            QpOutcome::Solved(next) => y = next,
            other => return Ok(other),
        }
    }
    Ok(QpOutcome::Infeasible { proven: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Every other slot is absent, so source indices differ from dense indices.
    struct SparseRows {
        dense: Vec<(Vec<f64>, f64)>,
    }

    impl QpRows for SparseRows {
        fn slot_count(&self) -> usize {
            2 * self.dense.len()
        }

        fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>> {
            if slot & 1 == 1 {
                return Ok(None);
            }
            let (c, b) = &self.dense[slot / 2];
            scratch.clear();
            scratch.extend_from_slice(c);
            Ok(Some(*b))
        }
    }

    fn compare(gm: &[f64], g: &[f64], rows: &SparseRows, native: bool) -> QpOutcome {
        let c: Vec<f64> = rows.dense.iter().flat_map(|(c, _)| c.clone()).collect();
        let b: Vec<f64> = rows.dense.iter().map(|(_, b)| *b).collect();
        let expected = dual_qp(gm, g, &c, &b, g.len(), b.len(), 1.0e-13, native).unwrap();
        let streamed = dual_qp_source(
            gm,
            g,
            rows,
            g.len(),
            1.0e-13,
            native,
            &mut QpStats::default(),
        )
        .unwrap();
        assert_eq!(streamed, expected);
        assert_eq!(
            streamed,
            dual_qp_rows(gm, g, &c, &b, g.len(), b.len(), 1.0e-13, native).unwrap()
        );
        streamed
    }

    #[test]
    fn generated_rows_preserve_skips_ties_and_selection() {
        let rows = SparseRows {
            dense: vec![(vec![1.0], 1.0); 4],
        };
        let mut taken = vec![false; rows.slot_count()];
        assert_eq!(
            rows.violated(&[0.0], 1.0e-13, &taken, false).unwrap(),
            vec![(-1.0, 0), (-1.0, 2), (-1.0, 4), (-1.0, 6)]
        );
        taken[0] = true;
        assert_eq!(
            rows.violated(&[0.0], 1.0e-13, &taken, false).unwrap(),
            vec![(-1.0, 2), (-1.0, 4), (-1.0, 6)]
        );
        for native in [false, true] {
            assert_eq!(
                compare(&[1.0], &[0.0], &rows, native),
                QpOutcome::Solved(vec![1.0])
            );
        }
    }

    #[test]
    fn generated_rows_preserve_infeasibility_and_nondefinite_status() {
        let rows = SparseRows {
            dense: vec![(vec![1.0], 1.0), (vec![-1.0], 0.0)],
        };
        for native in [false, true] {
            assert_eq!(
                compare(&[1.0], &[0.0], &rows, native),
                QpOutcome::Infeasible { proven: true }
            );
            assert_eq!(
                compare(&[0.0], &[0.0], &rows, native),
                QpOutcome::NotDefinite
            );
        }
    }

    #[test]
    fn native_scan_uses_the_existing_dot_product_rounding_bound() {
        let b = f64::from_bits(1.0_f64.to_bits() + 1);
        let rows = SparseRows {
            dense: vec![(vec![1.0], b)],
        };
        let taken = vec![false; rows.slot_count()];
        let raw = rows.violated(&[1.0], 0.0, &taken, false).unwrap();
        assert_eq!(raw, vec![(1.0 - b, 0)]);
        assert!(rows.violated(&[1.0], 0.0, &taken, true).unwrap().is_empty());
        let y = [0.5];
        assert_eq!(
            rows.violated(&y, 0.0, &taken, true).unwrap(),
            vec![(violation(&[1.0], &[b], &y, 0, 1).unwrap(), 0)]
        );
    }

    /// Records the selected stable slots at each scan. Rows of the first family all
    /// constrain x >= 1; once they bind, the second family requires y - x >= 0.
    struct BlockRows {
        scans: RefCell<Vec<Vec<usize>>>,
    }

    impl QpRows for BlockRows {
        fn slot_count(&self) -> usize {
            8002
        }

        fn row(&self, slot: usize, scratch: &mut Vec<f64>) -> Result<Option<f64>> {
            if slot & 1 == 1 {
                return Ok(None);
            }
            scratch.clear();
            if slot < 4002 {
                scratch.extend_from_slice(&[1.0, 0.0]);
                Ok(Some(1.0))
            } else {
                scratch.extend_from_slice(&[-1.0, 1.0]);
                Ok(Some(0.0))
            }
        }

        fn violated(
            &self,
            y: &[f64],
            tol: f64,
            taken: &[bool],
            native: bool,
        ) -> Result<Vec<(f64, usize)>> {
            self.scans.borrow_mut().push(
                taken
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &v)| v.then_some(i))
                    .collect(),
            );
            // Independent dense scan retains the source slot identity.
            let mut out = Vec::new();
            for (slot, &selected) in taken.iter().enumerate() {
                if selected || slot & 1 == 1 {
                    continue;
                }
                let (c, b) = if slot < 4002 {
                    ([1.0, 0.0], 1.0)
                } else {
                    ([-1.0, 1.0], 0.0)
                };
                let s = if native {
                    violation(&c, &[b], y, 0, 2)?
                } else {
                    -b + c[0] * y[0] + c[1] * y[1]
                };
                if s < -tol {
                    out.push((s, slot));
                }
            }
            Ok(out)
        }
    }

    #[test]
    fn generated_working_set_preserves_two_thousand_row_blocks_and_warm_starts() {
        for native in [false, true] {
            let rows = BlockRows {
                scans: RefCell::new(Vec::new()),
            };
            let mut stats = QpStats::default();
            let result = dual_qp_source(
                &[1.0, 0.0, 0.0, 1.0],
                &[0.0, 0.0],
                &rows,
                2,
                1.0e-13,
                native,
                &mut stats,
            )
            .unwrap();
            let reference = dual_qp(
                &[1.0, 0.0, 0.0, 1.0],
                &[0.0, 0.0],
                &[1.0, 0.0, -1.0, 1.0],
                &[1.0, 0.0],
                2,
                2,
                1.0e-13,
                native,
            )
            .unwrap();
            assert_eq!(result, reference);
            let QpOutcome::Solved(y) = result else {
                panic!("feasible two-row programme");
            };
            assert!(y.iter().all(|&v| (v - 1.0).abs() <= 4.0 * f64::EPSILON));
            assert_eq!(stats.peak_rows_held, 4000);
            assert!(stats.steps > 0);
            let scans = rows.scans.borrow();
            assert_eq!(scans.len(), 3);
            assert!(scans[0].is_empty());
            assert_eq!(scans[1], (0..4000).step_by(2).collect::<Vec<_>>());
            assert_eq!(scans[2].len(), 4000);
            assert!(!scans[2].contains(&4000));
            assert_eq!(
                &scans[2][2000..],
                &(4002..8002).step_by(2).collect::<Vec<_>>()
            );
        }
    }
}
