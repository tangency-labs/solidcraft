//! Sparse Cholesky factorisation for the solver's damped normal equations.
//!
//! LM solves `(JᵀJ + λW) s = -Jᵀr` every trial step. Each constraint reads a handful of
//! unknowns, so `JᵀJ` has a few entries per row; a dense elimination of a component of a
//! thousand unknowns does a third of a billion multiply-adds a step, almost all on zeros
//! (minutes per solve on a sketch of a few hundred curves). Here the unknowns are ordered to
//! keep the factor sparse (minimum degree), its structure is worked out once per LM run
//! ([`Pattern`]) and every trial step factors and solves on that structure only.
//!
//! The matrix is symmetric positive definite (`λ > 0`, `W > 0`), so no pivoting is needed. A
//! pivot that loses nearly all of its diagonal to cancellation (a system on the edge of
//! singular) gets no answer here: the caller then solves it densely, as before.

/// The structure of a symmetric matrix's Cholesky factor in a fill-reducing order.
#[derive(Clone, Debug)]
pub(crate) struct Pattern {
    /// `order[k]`: the unknown eliminated `k`-th.
    order: Vec<usize>,
    /// Per eliminated column `k`: the later columns (in elimination order, ascending) where the
    /// factor has an entry below the diagonal.
    below: Vec<Vec<usize>>,
    /// Per column `k`: the earlier columns `j` whose `below` holds `k`, and where.
    left: Vec<Vec<(usize, usize)>>,
    /// Per column `k`: where its entries below the diagonal start in the factor's flat values.
    at: Vec<usize>,
    /// The factorisation's buffers, kept between the solves of one LM run.
    scratch: std::cell::RefCell<Scratch>,
}

#[derive(Clone, Debug, Default)]
struct Scratch {
    diag: Vec<f64>,
    vals: Vec<f64>,
    work: Vec<f64>,
}

/// Pivots smaller than this share of the diagonal they started from are cancellation, not a
/// value: the factorisation gives up and the dense elimination solves the system.
const PIVOT_FLOOR: f64 = 1e-15;

impl Pattern {
    /// The factor's structure for a matrix whose off-diagonal entries are at most where two
    /// unknowns share a group in `groups` (the unknowns one constraint reads). Unknowns at or
    /// past `n` are ignored. Greedy minimum degree, ties to the lower index.
    pub(crate) fn new(n: usize, groups: &[Vec<usize>]) -> Pattern {
        let mut adj: Vec<std::collections::BTreeSet<usize>> = vec![std::collections::BTreeSet::new(); n];
        for g in groups {
            for (k, &a) in g.iter().enumerate() {
                for &b in g.iter().skip(k + 1) {
                    if a != b && a < n && b < n {
                        if let Some(s) = adj.get_mut(a) {
                            s.insert(b);
                        }
                        if let Some(s) = adj.get_mut(b) {
                            s.insert(a);
                        }
                    }
                }
            }
        }
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        let mut neighbours: Vec<Vec<usize>> = Vec::with_capacity(n);
        // Unknowns by degree, for the minimum: (degree, unknown), stale entries skipped.
        let mut by_degree: std::collections::BTreeSet<(usize, usize)> = adj.iter().enumerate().map(|(v, s)| (s.len(), v)).collect();
        while let Some((d, v)) = by_degree.pop_first() {
            if done.get(v).copied().unwrap_or(true) || adj.get(v).map(std::collections::BTreeSet::len) != Some(d) {
                continue;
            }
            if let Some(f) = done.get_mut(v) {
                *f = true;
            }
            let nb: Vec<usize> = adj.get_mut(v).map(std::mem::take).unwrap_or_default().into_iter().collect();
            // Eliminating v joins its neighbours to one another.
            for &a in &nb {
                let Some(s) = adj.get_mut(a) else { continue };
                let before = s.len();
                s.remove(&v);
                for &b in &nb {
                    if b != a {
                        s.insert(b);
                    }
                }
                if s.len() != before {
                    by_degree.insert((s.len(), a));
                }
            }
            order.push(v);
            neighbours.push(nb);
        }
        let mut position = vec![0; n];
        for (k, &v) in order.iter().enumerate() {
            if let Some(p) = position.get_mut(v) {
                *p = k;
            }
        }
        let below: Vec<Vec<usize>> = neighbours
            .into_iter()
            .map(|nb| {
                let mut b: Vec<usize> = nb.iter().filter_map(|v| position.get(*v).copied()).collect();
                b.sort_unstable();
                b
            })
            .collect();
        let mut left = vec![Vec::new(); n];
        for (j, rows) in below.iter().enumerate() {
            for (q, &k) in rows.iter().enumerate() {
                if let Some(l) = left.get_mut(k) {
                    l.push((j, q));
                }
            }
        }
        let mut at = Vec::with_capacity(n);
        let mut fill = 0;
        for rows in &below {
            at.push(fill);
            fill += rows.len();
        }
        Pattern { order, below, left, at, scratch: std::cell::RefCell::new(Scratch::default()) }
    }

    /// The entries of the factor below its diagonal.
    #[cfg(test)]
    pub(crate) fn fill(&self) -> usize {
        self.below.iter().map(Vec::len).sum()
    }

    /// Solve `(A + diag(shift)) x = b` for the symmetric `A` given by `entry(i, j)` (read only
    /// where the pattern has the factor's entries, which covers `A`'s). `None` when a pivot is
    /// not clearly positive (see [`PIVOT_FLOOR`]), when `stop` answers true (asked before each
    /// column), or when the answer is not finite.
    #[cfg(test)]
    pub(crate) fn solve(&self, entry: &dyn Fn(usize, usize) -> f64, shift: &[f64], b: &[f64], stop: &dyn Fn() -> bool) -> Option<Vec<f64>> {
        let order = &self.order;
        let row = |k: usize, q: Option<usize>| {
            let vk = order.get(k).copied().unwrap_or(0);
            let vi = match q {
                Some(q) => self.below.get(k).and_then(|r| r.get(q)).and_then(|i| order.get(*i)).copied().unwrap_or(0),
                None => vk,
            };
            entry(vi, vk)
        };
        self.factor_solve(row, shift, b, stop)
    }

    /// Where the factorisation reads `normal`'s entries ([`Pattern::solve_bound`]).
    pub(crate) fn bind(&self, normal: &Normal) -> Slots {
        let vk = |k: usize| self.order.get(k).copied().unwrap_or(0);
        Slots {
            diag: (0..self.order.len()).map(|k| normal.slot(vk(k), vk(k))).collect(),
            below: self.below.iter().enumerate().map(|(k, rows)| rows.iter().map(|&i| normal.slot(vk(i), vk(k))).collect()).collect(),
        }
    }

    /// [`Pattern::solve`] of the matrix in `normal`, read at `slots` ([`Pattern::bind`]).
    pub(crate) fn solve_bound(&self, normal: &Normal, slots: &Slots, shift: &[f64], b: &[f64], stop: &dyn Fn() -> bool) -> Option<Vec<f64>> {
        let at = |s: Option<&Option<usize>>| s.copied().flatten().and_then(|s| normal.vals.get(s)).copied().unwrap_or(0.0);
        let row = |k: usize, q: Option<usize>| match q {
            Some(q) => at(slots.below.get(k).and_then(|r| r.get(q))),
            None => at(slots.diag.get(k)),
        };
        self.factor_solve(row, shift, b, stop)
    }

    /// The factorisation and solve, reading the matrix by eliminated column `k`: its diagonal
    /// (`entry(k, None)`) and the entry of its `q`-th row below it (`entry(k, Some(q))`).
    fn factor_solve(&self, entry: impl Fn(usize, Option<usize>) -> f64, shift: &[f64], b: &[f64], stop: &dyn Fn() -> bool) -> Option<Vec<f64>> {
        let n = self.order.len();
        if shift.len() != n || b.len() != n {
            return None;
        }
        let mut fresh = Scratch::default();
        let mut kept = self.scratch.try_borrow_mut().ok();
        let scratch = kept.as_deref_mut().unwrap_or(&mut fresh);
        let fill = self.below.iter().map(Vec::len).sum();
        scratch.diag.clear();
        scratch.diag.resize(n, 0.0);
        scratch.vals.clear();
        scratch.vals.resize(fill, 0.0);
        scratch.work.clear();
        scratch.work.resize(n, 0.0);
        let Scratch { diag, vals, work } = scratch;
        for k in 0..n {
            if stop() {
                return None;
            }
            let vk = *self.order.get(k)?;
            let rows = self.below.get(k)?;
            let start = entry(k, None) + shift.get(vk)?;
            let mut d = start;
            for (q, &i) in rows.iter().enumerate() {
                *work.get_mut(i)? = entry(k, Some(q));
            }
            for &(j, q) in self.left.get(k)? {
                let from = *self.at.get(j)?;
                let cols = self.below.get(j)?;
                let lj = vals.get(from..from + cols.len())?;
                let ljk = *lj.get(q)?;
                d -= ljk * ljk;
                for (&i, &lij) in cols.get(q + 1..)?.iter().zip(lj.get(q + 1..)?) {
                    *work.get_mut(i)? -= lij * ljk;
                }
            }
            if !(d > PIVOT_FLOOR * start.abs()) || !d.is_finite() {
                return None;
            }
            let root = d.sqrt();
            *diag.get_mut(k)? = root;
            let from = *self.at.get(k)?;
            for (&i, v) in rows.iter().zip(vals.get_mut(from..from + rows.len())?) {
                *v = *work.get(i)? / root;
            }
        }
        // L y = P b, then Lᵀ z = y, x = Pᵀ z.
        let mut y: Vec<f64> = self.order.iter().map(|&v| b.get(v).copied().unwrap_or(0.0)).collect();
        for k in 0..n {
            let yk = *y.get(k)? / *diag.get(k)?;
            *y.get_mut(k)? = yk;
            let from = *self.at.get(k)?;
            let rows = self.below.get(k)?;
            for (&i, &l) in rows.iter().zip(vals.get(from..from + rows.len())?) {
                *y.get_mut(i)? -= l * yk;
            }
        }
        for k in (0..n).rev() {
            let mut s = *y.get(k)?;
            let from = *self.at.get(k)?;
            let rows = self.below.get(k)?;
            for (&i, &l) in rows.iter().zip(vals.get(from..from + rows.len())?) {
                s -= l * *y.get(i)?;
            }
            *y.get_mut(k)? = s / *diag.get(k)?;
        }
        let mut x = vec![0.0; n];
        for (k, &v) in self.order.iter().enumerate() {
            *x.get_mut(v)? = *y.get(k)?;
        }
        x.iter().all(|v| v.is_finite()).then_some(x)
    }
}

/// The damped normal equations' matrix `JᵀJ` of a large component, stored only where it can
/// have entries: per unknown, the unknowns it shares a constraint with (and itself), flat, an
/// entry found by its slot. Filled row by row of `J` in the same order as the dense matrix, so
/// every entry is the same sum.
#[derive(Clone, Debug)]
pub(crate) struct Normal {
    /// Per unknown `i`: its entries are slots `start[i]..start[i + 1]`.
    start: Vec<usize>,
    /// Per slot: the column, ascending within an unknown's slots.
    cols: Vec<usize>,
    pub(crate) vals: Vec<f64>,
}

impl Normal {
    /// The structure for unknowns `0..n` and `groups` as [`Pattern::new`] takes them.
    pub(crate) fn new(n: usize, groups: &[Vec<usize>]) -> Normal {
        let mut per: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        for g in groups {
            for &a in g {
                for &b in g {
                    if a < n
                        && b < n
                        && a != b
                        && let Some(c) = per.get_mut(a)
                    {
                        c.push(b);
                    }
                }
            }
        }
        let mut start = Vec::with_capacity(n + 1);
        let mut cols = Vec::new();
        for mut c in per {
            c.sort_unstable();
            c.dedup();
            start.push(cols.len());
            cols.extend(c);
        }
        start.push(cols.len());
        let vals = vec![0.0; cols.len()];
        Normal { start, cols, vals }
    }

    /// The slot of entry `(i, k)`, if the structure has one.
    pub(crate) fn slot(&self, i: usize, k: usize) -> Option<usize> {
        let (from, to) = (*self.start.get(i)?, *self.start.get(i + 1)?);
        self.cols.get(from..to)?.binary_search(&k).ok().map(|at| from + at)
    }

    /// All entries zero again, for the next iteration.
    pub(crate) fn clear(&mut self) {
        self.vals.fill(0.0);
    }

    #[cfg(test)]
    pub(crate) fn get(&self, i: usize, k: usize) -> f64 {
        self.slot(i, k).and_then(|s| self.vals.get(s)).copied().unwrap_or(0.0)
    }

    /// The same matrix, dense.
    pub(crate) fn dense(&self) -> crate::linalg::Mat {
        let n = self.start.len().saturating_sub(1);
        let mut m = crate::linalg::Mat::zeros(n, n);
        for i in 0..n {
            for k in self.start.get(i).copied().unwrap_or(0)..self.start.get(i + 1).copied().unwrap_or(0) {
                if let (Some(&c), Some(&v)) = (self.cols.get(k), self.vals.get(k)) {
                    m.set(i, c, v);
                }
            }
        }
        m
    }
}

/// Where a [`Pattern`]'s factorisation reads each entry in a [`Normal`]: per eliminated column,
/// the slot of its diagonal and of each entry below it (`None` where the factor fills in).
#[derive(Clone, Debug)]
pub(crate) struct Slots {
    diag: Vec<Option<usize>>,
    below: Vec<Vec<Option<usize>>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linalg::{Mat, solve};

    /// A chain of unknowns, each tied to the next: tridiagonal, no fill.
    #[test]
    fn chain_has_no_fill_and_matches_dense() {
        let n = 50;
        let groups: Vec<Vec<usize>> = (0..n - 1).map(|i| vec![i, i + 1]).collect();
        let p = Pattern::new(n, &groups);
        assert_eq!(p.fill(), n - 1);
        let mut a = Mat::zeros(n, n);
        for i in 0..n {
            a.set(i, i, 2.0);
            if i + 1 < n {
                a.set(i, i + 1, -1.0);
                a.set(i + 1, i, -1.0);
            }
        }
        let b: Vec<f64> = (0..n).map(|i| (i as f64).sin()).collect();
        let shift = vec![1e-3; n];
        let x = p.solve(&|i, j| a.get(i, j), &shift, &b, &|| false).unwrap();
        let mut m = a.clone();
        for i in 0..n {
            m.add(i, i, 1e-3);
        }
        let dense = solve(m, b).unwrap();
        for (s, d) in x.iter().zip(&dense) {
            assert!((s - d).abs() < 1e-10 * d.abs().max(1.0), "{s} {d}");
        }
    }

    /// Random groups of three unknowns (a constraint between points): the same answer as the
    /// dense elimination, to rounding.
    #[test]
    fn random_groups_match_dense() {
        let mut seed = 7_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let n = 120;
        let mut groups = Vec::new();
        let mut j = Mat::zeros(200, n);
        for row in 0..200 {
            let g: Vec<usize> = (0..3).map(|_| (next() * n as f64) as usize % n).collect();
            for &c in &g {
                j.set(row, c, next() - 0.5);
            }
            groups.push(g);
        }
        let mut a = Mat::zeros(n, n);
        for row in 0..j.rows {
            for c in 0..n {
                for d in 0..n {
                    a.add(c, d, j.get(row, c) * j.get(row, d));
                }
            }
        }
        let p = Pattern::new(n, &groups);
        let shift: Vec<f64> = (0..n).map(|i| 1e-2 * (1.0 + (i % 3) as f64)).collect();
        let b: Vec<f64> = (0..n).map(|_| next() - 0.5).collect();
        let x = p.solve(&|r, c| a.get(r, c), &shift, &b, &|| false).unwrap();
        let mut m = a.clone();
        for (i, s) in shift.iter().enumerate() {
            m.add(i, i, *s);
        }
        let dense = solve(m, b).unwrap();
        for (s, d) in x.iter().zip(&dense) {
            assert!((s - d).abs() < 1e-9 * d.abs().max(1.0), "{s} {d}");
        }
    }

    /// The matrix kept in a [`Normal`] and read at its bound slots solves to the same bits as
    /// read entry by entry.
    #[test]
    fn bound_slots_solve_as_entries() {
        let mut seed = 11_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let n = 90;
        let groups: Vec<Vec<usize>> = (0..150).map(|_| (0..3).map(|_| (next() * n as f64) as usize % n).collect()).collect();
        let p = Pattern::new(n, &groups);
        let mut normal = Normal::new(n, &groups);
        for g in &groups {
            let d: Vec<f64> = g.iter().map(|_| next() - 0.5).collect();
            for (a, da) in g.iter().zip(&d) {
                for (b, db) in g.iter().zip(&d) {
                    if let Some(s) = normal.slot(*a, *b) {
                        normal.vals[s] += da * db;
                    }
                }
            }
        }
        let shift = vec![1e-3; n];
        let b: Vec<f64> = (0..n).map(|_| next() - 0.5).collect();
        let slots = p.bind(&normal);
        let bound = p.solve_bound(&normal, &slots, &shift, &b, &|| false).unwrap();
        let entries = p.solve(&|i, k| normal.get(i, k), &shift, &b, &|| false).unwrap();
        assert_eq!(bound, entries);
        let dense = normal.dense();
        for i in 0..n {
            for k in 0..n {
                assert_eq!(dense.get(i, k), normal.get(i, k));
            }
        }
    }

    /// A singular matrix with no shift has a zero pivot: no answer, for the dense fallback.
    #[test]
    fn singular_gives_none() {
        let a = Mat::zeros(3, 3);
        let p = Pattern::new(3, &[vec![0, 1], vec![1, 2]]);
        assert!(p.solve(&|r, c| a.get(r, c), &[0.0; 3], &[1.0, 2.0, 3.0], &|| false).is_none());
    }

    /// The solve stops when asked to.
    #[test]
    fn stops_when_asked() {
        let p = Pattern::new(2, &[vec![0, 1]]);
        assert!(p.solve(&|r, c| if r == c { 1.0 } else { 0.0 }, &[0.0; 2], &[1.0, 1.0], &|| true).is_none());
    }
}
