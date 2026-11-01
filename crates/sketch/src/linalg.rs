//! Small dense linear algebra for the solver: row-major matrices, Gaussian elimination with
//! partial pivoting, and reduced row echelon form for rank and null-space analysis.

/// Row-major dense matrix.
#[derive(Clone, Debug)]
pub struct Mat {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Mat {
    pub fn zeros(rows: usize, cols: usize) -> Mat {
        Mat { rows, cols, data: vec![0.0; rows.saturating_mul(cols)] }
    }
    pub fn get(&self, r: usize, c: usize) -> f64 {
        self.data.get(r * self.cols + c).copied().unwrap_or(0.0)
    }
    pub fn set(&mut self, r: usize, c: usize, v: f64) {
        if let Some(x) = self.data.get_mut(r * self.cols + c) {
            *x = v;
        }
    }
    pub fn add(&mut self, r: usize, c: usize, v: f64) {
        if let Some(x) = self.data.get_mut(r * self.cols + c) {
            *x += v;
        }
    }
    fn swap_rows(&mut self, a: usize, b: usize) {
        if a == b {
            return;
        }
        for c in 0..self.cols {
            self.data.swap(a * self.cols + c, b * self.cols + c);
        }
    }
}

/// Solve `A x = b` (A square). Returns `None` when singular.
pub fn solve(mut a: Mat, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = a.rows;
    if a.cols != n || b.len() != n || n.checked_mul(n) != Some(a.data.len()) {
        return None;
    }
    for k in 0..n {
        let mut piv = k;
        let mut best = a.get(k, k).abs();
        for r in k + 1..n {
            let v = a.get(r, k).abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if !(best > 1e-300) || !best.is_finite() {
            return None;
        }
        a.swap_rows(k, piv);
        b.swap(k, piv);
        // Row by row on slices, so the update vectorises: the same operations in the same
        // order as element by element (the solution is bit for bit the same).
        let (above, below) = a.data.split_at_mut_checked((k + 1) * n)?;
        let pivot = above.get(k * n + k..)?;
        let d = *pivot.first()?;
        let (b_above, b_below) = b.split_at_mut_checked(k + 1)?;
        let bk = *b_above.last()?;
        for (row, br) in below.chunks_exact_mut(n).zip(b_below.iter_mut()) {
            let row = row.get_mut(k..)?;
            let f = *row.first()? / d;
            if f == 0.0 {
                continue;
            }
            for (x, v) in row.iter_mut().zip(pivot) {
                *x += -f * v;
            }
            *br -= f * bk;
        }
    }
    let mut x = vec![0.0; n];
    for k in (0..n).rev() {
        let mut s = *b.get(k)?;
        for c in k + 1..n {
            s -= a.get(k, c) * x.get(c)?;
        }
        *x.get_mut(k)? = s / a.get(k, k);
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// Reduce to row echelon form in place; returns the pivot column of each pivot row.
pub fn rref(a: &mut Mat, tol: f64) -> Vec<usize> {
    let mut pivots = Vec::new();
    let mut row = 0;
    for col in 0..a.cols {
        if row >= a.rows {
            break;
        }
        let mut piv = row;
        let mut best = a.get(row, col).abs();
        for r in row + 1..a.rows {
            let v = a.get(r, col).abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if !(best > tol) {
            continue;
        }
        a.swap_rows(row, piv);
        let d = a.get(row, col);
        for c in 0..a.cols {
            let v = a.get(row, c) / d;
            a.set(row, c, v);
        }
        for r in 0..a.rows {
            if r == row {
                continue;
            }
            let f = a.get(r, col);
            if f == 0.0 {
                continue;
            }
            for c in 0..a.cols {
                let v = a.get(row, c);
                a.add(r, c, -f * v);
            }
        }
        pivots.push(col);
        row += 1;
    }
    pivots
}

/// For each column of `j`: is that variable fixed by the constraints (zero in every null-space
/// vector)? Also returns the rank.
pub fn determined_vars(j: &Mat, tol: f64) -> (Vec<bool>, usize) {
    let mut a = j.clone();
    let pivots = rref(&mut a, tol);
    let rank = pivots.len();
    let free: Vec<usize> = (0..a.cols).filter(|c| !pivots.contains(c)).collect();
    let mut det = vec![true; a.cols];
    for f in &free {
        if let Some(d) = det.get_mut(*f) {
            *d = false;
        }
    }
    // Null vector for free column f: x_f = 1, x_pivot(r) = -a[r][f].
    for (r, pc) in pivots.iter().enumerate() {
        if free.iter().any(|f| a.get(r, *f).abs() > tol)
            && let Some(d) = det.get_mut(*pc)
        {
            *d = false;
        }
    }
    (det, rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_and_detects_singular() {
        let mut a = Mat::zeros(2, 2);
        a.set(0, 0, 2.0);
        a.set(0, 1, 1.0);
        a.set(1, 0, 1.0);
        a.set(1, 1, 3.0);
        let x = solve(a, vec![3.0, 5.0]).unwrap();
        assert!((x[0] - 0.8).abs() < 1e-12 && (x[1] - 1.4).abs() < 1e-12);
        let s = Mat::zeros(2, 2);
        assert!(solve(s, vec![1.0, 1.0]).is_none());
    }

    /// The elimination element by element, as `solve` did before it worked on row slices.
    fn solve_by_elements(mut a: Mat, mut b: Vec<f64>) -> Option<Vec<f64>> {
        let n = a.rows;
        for k in 0..n {
            let mut piv = k;
            let mut best = a.get(k, k).abs();
            for r in k + 1..n {
                if a.get(r, k).abs() > best {
                    best = a.get(r, k).abs();
                    piv = r;
                }
            }
            if !(best > 1e-300) || !best.is_finite() {
                return None;
            }
            a.swap_rows(k, piv);
            b.swap(k, piv);
            let d = a.get(k, k);
            for r in k + 1..n {
                let f = a.get(r, k) / d;
                if f == 0.0 {
                    continue;
                }
                for c in k..n {
                    let v = a.get(k, c);
                    a.add(r, c, -f * v);
                }
                let bk = b[k];
                b[r] -= f * bk;
            }
        }
        let mut x = vec![0.0; n];
        for k in (0..n).rev() {
            let mut s = b[k];
            for (c, xc) in x.iter().enumerate().skip(k + 1) {
                s -= a.get(k, c) * xc;
            }
            x[k] = s / a.get(k, k);
        }
        x.iter().all(|v| v.is_finite()).then_some(x)
    }

    #[test]
    fn row_slices_solve_bit_for_bit_as_elements_did() {
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        for n in [1, 2, 3, 7, 16, 41, 120] {
            for sparse in [false, true] {
                let mut a = Mat::zeros(n, n);
                for r in 0..n {
                    for c in 0..n {
                        let v = next();
                        // Sparse rows (as a sketch's Jacobian has) skip most updates: f == 0.
                        if !sparse || r == c || v.abs() < 0.1 {
                            a.set(r, c, if r == c { v + 4.0 } else { v });
                        }
                    }
                }
                let b: Vec<f64> = (0..n).map(|_| next()).collect();
                let new = solve(a.clone(), b.clone()).unwrap();
                let old = solve_by_elements(a, b).unwrap();
                assert!(new.iter().zip(&old).all(|(x, y)| x.to_bits() == y.to_bits()), "n {n}, sparse {sparse}");
            }
        }
        // Wrong shapes are refused, not read past.
        assert!(solve(Mat { rows: 2, cols: 2, data: vec![1.0; 3] }, vec![1.0, 1.0]).is_none());
        assert!(solve(Mat::zeros(2, 3), vec![1.0, 1.0]).is_none());
    }

    #[test]
    fn rank_and_determined() {
        // x0 + x1 = .., x2 = .. ; x0 and x1 not individually determined.
        let mut j = Mat::zeros(2, 3);
        j.set(0, 0, 1.0);
        j.set(0, 1, 1.0);
        j.set(1, 2, 1.0);
        let (det, rank) = determined_vars(&j, 1e-9);
        assert_eq!(rank, 2);
        assert_eq!(det, vec![false, false, true]);
    }
}
