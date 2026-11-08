//! Dense symmetric positive semi-definite solves for the constraint equations.
//!
//! The matrix `J M⁻¹ Jᵀ` is singular when joints repeat each other's constraints (every
//! revolute of a planar linkage built in 3D also holds the linkage in its plane), but its
//! right-hand side is then consistent. A Cholesky factorisation with diagonal pivoting stops at
//! the numerical rank and sets the multipliers of the dependent rows to zero, which solves such
//! a system exactly.

/// A square row-major matrix.
#[derive(Clone, Debug)]
pub(crate) struct Sym {
    n: usize,
    data: Vec<f64>,
}

impl Sym {
    pub(crate) fn zeros(n: usize) -> Sym {
        Sym { n, data: vec![0.0; n.saturating_mul(n)] }
    }
    pub(crate) fn get(&self, r: usize, c: usize) -> f64 {
        self.data.get(r * self.n + c).copied().unwrap_or(0.0)
    }
    pub(crate) fn set(&mut self, r: usize, c: usize, v: f64) {
        if let Some(x) = self.data.get_mut(r * self.n + c) {
            *x = v;
        }
    }
    fn swap(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        let n = self.n;
        for c in 0..n {
            self.data.swap(a * n + c, b * n + c);
        }
        for r in 0..n {
            self.data.swap(r * n + a, r * n + b);
        }
    }
}

/// Solve `A x = b` for a symmetric positive semi-definite `A`, dropping the directions whose
/// pivot falls below `rel_tol` times the largest diagonal. Returns the solution and the rank;
/// `None` when the input is not finite or the sizes disagree.
pub(crate) fn solve_psd(mut a: Sym, b: &[f64], rel_tol: f64) -> Option<(Vec<f64>, usize)> {
    let n = a.n;
    if b.len() != n || a.data.len() != n.checked_mul(n)? || !b.iter().chain(&a.data).all(|x| x.is_finite()) {
        return None;
    }
    let mut perm: Vec<usize> = (0..n).collect();
    let mut rhs = b.to_vec();
    let max_diag = (0..n).map(|i| a.get(i, i)).fold(0.0_f64, f64::max);
    let tol = (max_diag * rel_tol).max(f64::MIN_POSITIVE);
    let mut rank = 0;
    for k in 0..n {
        // The largest remaining diagonal becomes the pivot.
        let (best, best_val) = (k..n).map(|i| (i, a.get(i, i))).fold((k, f64::NEG_INFINITY), |acc, x| if x.1 > acc.1 { x } else { acc });
        if !(best_val > tol) {
            break;
        }
        a.swap(k, best);
        perm.swap(k, best);
        rhs.swap(k, best);
        let l = best_val.sqrt();
        a.set(k, k, l);
        // Both triangles are kept, so a later pivot swap moves whole rows and columns.
        for i in k + 1..n {
            let v = a.get(i, k) / l;
            a.set(i, k, v);
            a.set(k, i, v);
        }
        for j in k + 1..n {
            let ljk = a.get(j, k);
            if ljk == 0.0 {
                continue;
            }
            for i in j..n {
                let v = a.get(i, j) - a.get(i, k) * ljk;
                a.set(i, j, v);
                a.set(j, i, v);
            }
        }
        rank = k + 1;
    }
    // L y = rhs (first `rank` rows), then Lᵀ x = y; the dependent unknowns stay 0.
    let mut y = vec![0.0; n];
    for i in 0..rank {
        let s = (0..i).fold(rhs.get(i).copied().unwrap_or(0.0), |s, j| s - a.get(i, j) * y.get(j).copied().unwrap_or(0.0));
        if let Some(v) = y.get_mut(i) {
            *v = s / a.get(i, i);
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..rank).rev() {
        let s = (i + 1..rank).fold(y.get(i).copied().unwrap_or(0.0), |s, j| s - a.get(j, i) * x.get(j).copied().unwrap_or(0.0));
        if let Some(v) = x.get_mut(i) {
            *v = s / a.get(i, i);
        }
    }
    let mut out = vec![0.0; n];
    for (k, p) in perm.iter().enumerate() {
        if let (Some(o), Some(v)) = (out.get_mut(*p), x.get(k)) {
            *o = *v;
        }
    }
    out.iter().all(|v| v.is_finite()).then_some((out, rank))
}
