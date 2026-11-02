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

/// `sm_SbDualQP`: the strictly convex quadratic programme `min 1/2 y' G y + g' y` subject to
/// `C y >= b`, by the dual active-set method (from the unconstrained minimum, adding the
/// most violated constraint each step and dropping those whose multipliers would turn
/// negative): `G` `n x n`, `C` `m x n`, row-major.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // the fork's one function
pub(super) fn dual_qp(
    gm: &[f64],
    g: &[f64],
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
) -> Result<QpOutcome> {
    // G = L L', J = L^-T (J' G J = I), J[r n + c] its column c
    let mut l = vec![0.0; n * n];
    for j in 0..n {
        let mut s = at(gm, j * n + j)?;
        for k in 0..j {
            s -= at(&l, j * n + k)? * at(&l, j * n + k)?;
        }
        if !(s > 0.0) {
            return Ok(QpOutcome::NotDefinite);
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
    let mut active: Vec<usize> = Vec::new();
    let mut u: Vec<f64> = Vec::new();
    let mut rm = vec![0.0; n * n];
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
        let mut p = m;
        let mut worst = -tol;
        for i in 0..m {
            let s = slack(&y, i)?;
            if s < worst {
                worst = s;
                p = i;
            }
        }
        if p == m {
            return Ok(QpOutcome::Solved(y));
        }
        let mut up = 0.0;
        loop {
            let q = active.len();
            for (c, dc) in d.iter_mut().enumerate() {
                let mut s = 0.0;
                for k in 0..n {
                    s += at(&jm, k * n + c)? * at(cm, p * n + k)?;
                }
                *dc = s;
            }
            // the step in y, z = J2 d2, and in the multipliers, r = R^-1 d1
            let mut dz = 0.0;
            for (k, zk) in z.iter_mut().enumerate() {
                let mut s = 0.0;
                for c in q..n {
                    s += at(&jm, k * n + c)? * at(&d, c)?;
                }
                *zk = s;
                dz = smax(dz, s.abs());
            }
            for i in (0..q).rev() {
                let mut s = at(&d, i)?;
                for j in i + 1..q {
                    s -= at(&rm, i * n + j)? * at(&r, j)?;
                }
                *el_mut(&mut r, i)? = s / at(&rm, i * n + i)?;
            }
            let mut t1 = f64::INFINITY;
            let mut drop = q;
            for k in 0..q {
                let (rk, uk) = (at(&r, k)?, at(&u, k)?);
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
                -slack(&y, p)? / zn
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
                        let (a, bb) = (at(&jm, k * n + j - 1)?, at(&jm, k * n + j)?);
                        *el_mut(&mut jm, k * n + j - 1)? = cs * a + sn * bb;
                        *el_mut(&mut jm, k * n + j)? = -sn * a + cs * bb;
                    }
                    j -= 1;
                }
                if !(at(&d, q)?.abs() > 1.0e-14) {
                    return Ok(QpOutcome::Infeasible { proven: false });
                }
                for i in 0..=q {
                    *el_mut(&mut rm, i * n + q)? = at(&d, i)?;
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
                    *el_mut(&mut rm, i * n + j)? = at(&rm, i * n + j + 1)?;
                }
                *el_mut(&mut rm, i * n + q - 1)? = 0.0;
            }
            for j in drop..q.saturating_sub(1) {
                let (mut a, mut bb) = (at(&rm, j * n + j)?, at(&rm, (j + 1) * n + j)?);
                let (cs, sn) = givens(&mut a, &mut bb);
                *el_mut(&mut rm, j * n + j)? = a;
                *el_mut(&mut rm, (j + 1) * n + j)? = 0.0;
                for k in j + 1..q.saturating_sub(1) {
                    let (x1, x2) = (at(&rm, j * n + k)?, at(&rm, (j + 1) * n + k)?);
                    *el_mut(&mut rm, j * n + k)? = cs * x1 + sn * x2;
                    *el_mut(&mut rm, (j + 1) * n + k)? = -sn * x1 + cs * x2;
                }
                for k in 0..n {
                    let (x1, x2) = (at(&jm, k * n + j)?, at(&jm, k * n + j + 1)?);
                    *el_mut(&mut jm, k * n + j)? = cs * x1 + sn * x2;
                    *el_mut(&mut jm, k * n + j + 1)? = -sn * x1 + cs * x2;
                }
            }
        }
    }
    Ok(QpOutcome::Infeasible { proven: false })
}

/// `sm_SbDualQPRows`: [`dual_qp`] over many rows, few of which bind: solved on a working set
/// of rows, those violated at its solution added (the most violated first, up to a block at
/// a time) until none is. Its answer is the full programme's: a solution meets every row; a
/// set of rows with no solution is one the full set has none of either (`proven` as
/// [`dual_qp`]'s).
pub(super) fn dual_qp_rows(
    gm: &[f64],
    g: &[f64],
    cm: &[f64],
    b: &[f64],
    n: usize,
    m: usize,
    tol: f64,
) -> Result<QpOutcome> {
    const BLOCK: usize = 2000;
    let mut taken = vec![false; m];
    let mut set: Vec<usize> = Vec::new();
    // y at the unconstrained minimum: the empty set's solution
    let mut y = match dual_qp(gm, g, &[], &[], n, 0, tol)? {
        QpOutcome::Solved(y) => y,
        // SER: the empty programme's status (SM_ERR_INVALID_INPUT; with no rows it is
        // never infeasible)
        other => return Ok(other),
    };
    for _ in 0..200 {
        let mut violated: Vec<(f64, usize)> = Vec::new();
        for (i, &inside) in taken.iter().enumerate() {
            if inside {
                continue;
            }
            let mut s = -at(b, i)?;
            for (k, &yk) in y.iter().enumerate() {
                s += at(cm, i * n + k)? * yk;
            }
            if s < -tol {
                violated.push((s, i));
            }
        }
        if violated.is_empty() {
            return Ok(QpOutcome::Solved(y));
        }
        let take = BLOCK.min(violated.len());
        // the pairs' order (slack, then row) is total on distinct rows
        violated.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i) in violated.iter().take(take) {
            *el_mut(&mut taken, i)? = true;
            set.push(i);
        }
        let mut sc = vec![0.0; set.len() * n];
        let mut sb = vec![0.0; set.len()];
        for (t, &row) in set.iter().enumerate() {
            for k in 0..n {
                *el_mut(&mut sc, t * n + k)? = at(cm, row * n + k)?;
            }
            *el_mut(&mut sb, t)? = at(b, row)?;
        }
        match dual_qp(gm, g, &sc, &sb, n, set.len(), tol)? {
            QpOutcome::Solved(next) => y = next,
            other => return Ok(other),
        }
    }
    Ok(QpOutcome::Infeasible { proven: false })
}
