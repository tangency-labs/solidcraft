//! Ordered Cholesky and Schur-complement solves for bounded spline fitting.
//!
//! Migrated from SolidCraft's original fill solver. Both Cholesky variants and
//! the ordered-map Schur reduction retain their existing arithmetic order.
//! Deviations: malformed public dimensions are refused before allocation or
//! reduction; allocation failure uses the standard kernel failure path.
use crate::error::nurbs::{LinearSolveFailure, NurbsFailure};
use crate::{Result, SolidError};

/// Maximum scalar unknowns: three coordinates for 6000 free control points.
pub const MAX_COORDINATES: usize = 18_000;
/// Maximum shared scalar unknowns, retaining the existing dense Schur bound.
pub const MAX_SHARED_UNKNOWNS: usize = 4000;
const OP: &str = "geom::ordered_solve";
fn error(reason: LinearSolveFailure) -> SolidError {
    SolidError::nurbs(OP, NurbsFailure::LinearSolve(reason))
}
fn refuse<T>(reason: LinearSolveFailure) -> Result<T> {
    Err(error(reason))
}
fn dimension(n: usize) -> Result<()> {
    if n > MAX_COORDINATES {
        return refuse(LinearSolveFailure::TooLarge);
    }
    Ok(())
}
fn filled<T: Clone>(n: usize, value: T) -> Result<Vec<T>> {
    if n > MAX_COORDINATES * (MAX_COORDINATES + 1) {
        return refuse(LinearSolveFailure::TooLarge);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| error(LinearSolveFailure::Allocation))?;
    out.resize(n, value);
    Ok(out)
}

/// A symmetric banded matrix: the upper band, row `r` holding columns `r ..= r + bw`.
#[derive(Clone)]
pub struct Band {
    n: usize,
    bw: usize,
    a: Vec<f64>,
}

impl Band {
    /// Allocate a bounded symmetric matrix with the supplied upper half-bandwidth.
    pub fn new(n: usize, bw: usize) -> Result<Band> {
        dimension(n)?;
        if bw > MAX_COORDINATES {
            return refuse(LinearSolveFailure::InvalidShape);
        }
        let width = bw
            .checked_add(1)
            .ok_or_else(|| error(LinearSolveFailure::InvalidShape))?;
        let count = n
            .checked_mul(width)
            .ok_or_else(|| error(LinearSolveFailure::TooLarge))?;
        Ok(Band {
            n,
            bw,
            a: filled(count, 0.0)?,
        })
    }
    fn index(&self, r: usize, s: usize) -> Option<usize> {
        let (r, s) = if r <= s { (r, s) } else { (s, r) };
        if s >= self.n || s - r > self.bw {
            return None;
        }
        r.checked_mul(self.bw + 1)?.checked_add(s - r)
    }
    fn get(&self, r: usize, s: usize) -> f64 {
        self.index(r, s)
            .and_then(|i| self.a.get(i))
            .copied()
            .unwrap_or(0.0)
    }
    /// Add `v` at `(r, s)` with `r ≤ s` (the lower half is the same by symmetry: callers adding
    /// a full outer product reach each stored entry once through `r ≤ s`).
    pub fn add(&mut self, r: usize, s: usize, v: f64) {
        if r <= s {
            if let Some(x) = self.index(r, s).and_then(|i| self.a.get_mut(i)) {
                *x += v;
            }
        }
    }
    /// Solve `A·x = b` by banded Cholesky factorisation `A = Uᵀ·U`; an error when `A` is not
    /// positive definite.
    pub fn solve(self, b: Vec<f64>) -> Result<Vec<f64>> {
        self.factor_rows()?.substitute(b)
    }

    /// Factor the band in place by rows. Each update visits contiguous entries
    /// and subtracts the preceding rows in their original order.
    pub fn factor_rows(mut self) -> Result<Band> {
        let (n, bw) = (self.n, self.bw);
        let w = bw + 1;
        let mut row = filled(w, 0.0)?;
        for r in 0..n {
            let Some(here) = self.a.get_mut(r * w..r * w + w) else {
                return refuse(LinearSolveFailure::InvalidShape);
            };
            let d = here.first().copied().unwrap_or(0.0);
            if d.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
                return refuse(LinearSolveFailure::FitSingular);
            }
            let d = d.sqrt();
            for (k, x) in here.iter_mut().enumerate() {
                *x = if k == 0 { d } else { *x / d };
            }
            row.copy_from_slice(here);
            let end = (r + bw + 1).min(n);
            for s in r + 1..end {
                let us = row.get(s - r).copied().unwrap_or(0.0);
                if us == 0.0 {
                    continue;
                }
                let len = end - s;
                let (Some(dst), Some(src)) = (
                    self.a.get_mut(s * w..s * w + len),
                    row.get(s - r..s - r + len),
                ) else {
                    return refuse(LinearSolveFailure::InvalidShape);
                };
                for (x, y) in dst.iter_mut().zip(src) {
                    *x -= us * y;
                }
            }
        }
        Ok(self)
    }

    /// Solve `Uᵀ·U·x = b` with a factor from [`Band::factor_rows`].
    pub fn substitute(&self, mut b: Vec<f64>) -> Result<Vec<f64>> {
        let (n, bw) = (self.n, self.bw);
        if b.len() != n {
            return refuse(LinearSolveFailure::InvalidShape);
        }
        // Uᵀ·y = b, then U·x = y.
        for r in 0..n {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for k in r.saturating_sub(bw)..r {
                v -= self.get(k, r) * b.get(k).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        for r in (0..n).rev() {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for s in r + 1..(r + bw + 1).min(n) {
                v -= self.get(r, s) * b.get(s).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        if b.iter().any(|x| !x.is_finite()) {
            return refuse(LinearSolveFailure::FitNonFinite);
        }
        Ok(b)
    }
}

/// The normal equations' matrix: banded where the band is narrow against the size, else dense.
pub struct Normal(Storage);

enum Storage {
    Band(Band),
    Dense(Vec<f64>, usize),
    Blocks(BlockNormal),
}

impl Normal {
    /// Choose banded or dense storage using the existing bandwidth threshold.
    pub fn new(n: usize, bw: usize) -> Result<Normal> {
        dimension(n)?;
        if bw > MAX_COORDINATES {
            return refuse(LinearSolveFailure::InvalidShape);
        }
        Ok(Normal(if 4 * bw < n {
            Storage::Band(Band::new(n, bw)?)
        } else {
            Storage::Dense(filled(n * n, 0.0)?, n)
        }))
    }

    /// Coordinate owners, with shared coordinates represented by `None`.
    pub fn blocks(owner: Vec<Option<usize>>) -> Result<Normal> {
        BlockNormal::new(owner).map(|m| Normal(Storage::Blocks(m)))
    }

    /// Add `v` at `(r, s)` (callers add both halves of a symmetric matrix).
    pub fn add(&mut self, r: usize, s: usize, v: f64) {
        match &mut self.0 {
            Storage::Band(b) => b.add(r, s, v),
            Storage::Dense(a, n) => {
                if let Some(x) = (r < *n && s < *n)
                    .then(|| r * *n + s)
                    .and_then(|i| a.get_mut(i))
                {
                    *x += v;
                }
            }
            Storage::Blocks(m) => m.add(r, s, v),
        }
    }

    /// Solve using the retained representation and deterministic reduction order.
    pub fn solve(self, b: Vec<f64>) -> Result<Vec<f64>> {
        match self.0 {
            Storage::Band(m) => m.factor_rows()?.substitute(b),
            Storage::Dense(a, n) => cholesky(a, b, n),
            Storage::Blocks(m) => m.solve(&b),
        }
    }
}

/// The normal equations of patches about a centre, solved by blocks: each patch's interior
/// unknowns (coordinates of control points only that patch holds, coupled to no other patch's)
/// form a banded block, eliminated exactly onto the shared unknowns (the spokes', the centre's,
/// and any a row couples across patches) by their Schur complement, which is solved dense; the
/// interiors then follow by back substitution. Entries are kept sparse until the blocks are
/// known (callers add both halves of the symmetric matrix), in ordered maps: the eliminations
/// sum them in their order, so a hashed map's order (another each run) would make the fill
/// differ in its last bits from one run to the next, and a design read back another surface.
struct BlockNormal {
    /// Each coordinate's patch when only one patch holds its point, else `None`.
    owner: Vec<Option<usize>>,
    entries: std::collections::BTreeMap<(usize, usize), f64>,
    invalid_shape: bool,
}

/// One patch's factored interior block `A_kk`, with `g = A_kk⁻¹ b_k` and, for each shared
/// column `j` coupled to the patch, `z_j = A_kk⁻¹ A_k,j`.
type SolvedPart = (Band, Vec<f64>, Vec<(usize, Vec<f64>)>);

impl BlockNormal {
    fn new(owner: Vec<Option<usize>>) -> Result<BlockNormal> {
        dimension(owner.len())?;
        if owner.iter().flatten().any(|p| *p >= MAX_COORDINATES) {
            return refuse(LinearSolveFailure::InvalidShape);
        }
        Ok(BlockNormal {
            owner,
            entries: std::collections::BTreeMap::new(),
            invalid_shape: false,
        })
    }

    fn add(&mut self, r: usize, s: usize, v: f64) {
        if r >= self.owner.len() || s >= self.owner.len() {
            self.invalid_shape = true;
            return;
        }
        if v != 0.0 {
            *self.entries.entry((r, s)).or_insert(0.0) += v;
        }
    }

    fn solve(mut self, b: &[f64]) -> Result<Vec<f64>> {
        let n = self.owner.len();
        if self.invalid_shape
            || b.len() != n
            || self.entries.keys().any(|(r, c)| *r >= n || *c >= n)
        {
            return refuse(LinearSolveFailure::InvalidShape);
        }
        // A coordinate coupled to another patch's interior is shared too.
        let mut coupled = filled(n, false)?;
        for (r, c) in self.entries.keys() {
            if matches!((self.owner.get(*r).copied().flatten(), self.owner.get(*c).copied().flatten()), (Some(a), Some(b)) if a != b)
            {
                if let Some(mark) = coupled.get_mut(*r) {
                    *mark = true;
                }
                if let Some(mark) = coupled.get_mut(*c) {
                    *mark = true;
                }
            }
        }
        for (owner, coupled) in self.owner.iter_mut().zip(coupled) {
            if coupled {
                *owner = None;
            }
        }
        let shared: Vec<usize> = (0..n)
            .filter(|x| self.owner.get(*x).copied().flatten().is_none())
            .collect();
        if shared.len() > MAX_SHARED_UNKNOWNS {
            return refuse(LinearSolveFailure::SharedLimit {
                given: shared.len(),
            });
        }
        let mut place = filled(n, usize::MAX)?;
        for (i, x) in shared.iter().enumerate() {
            if let Some(slot) = place.get_mut(*x) {
                *slot = i;
            }
        }
        let patches = self
            .owner
            .iter()
            .flatten()
            .copied()
            .max()
            .map_or(0, |m| m + 1);
        let mut interior: Vec<Vec<usize>> = filled(patches, Vec::new())?;
        for (x, o) in self.owner.iter().enumerate() {
            if let Some(list) = o.and_then(|p| interior.get_mut(p)) {
                if let Some(slot) = place.get_mut(x) {
                    *slot = list.len();
                }
                list.push(x);
            }
        }
        let m = shared.len();
        // The shared block, and per patch its block, its coupling to the shared unknowns.
        let mut s_mat = filled(m * m, 0.0)?;
        let mut blocks: Vec<std::collections::BTreeMap<(usize, usize), f64>> =
            filled(patches, std::collections::BTreeMap::new())?;
        let mut couple: Vec<std::collections::BTreeMap<(usize, usize), f64>> =
            filled(patches, std::collections::BTreeMap::new())?;
        for ((r, c), v) in std::mem::take(&mut self.entries) {
            let (or, oc) = (
                self.owner.get(r).copied().flatten(),
                self.owner.get(c).copied().flatten(),
            );
            let (pr, pc) = (
                place.get(r).copied().unwrap_or(0),
                place.get(c).copied().unwrap_or(0),
            );
            match (or, oc) {
                (None, None) => {
                    if let Some(x) = s_mat.get_mut(pr * m + pc) {
                        *x += v;
                    }
                }
                (Some(p), Some(_)) => {
                    if let Some(blk) = blocks.get_mut(p) {
                        *blk.entry((pr, pc)).or_insert(0.0) += v;
                    }
                }
                // Interior row, shared column (the transposed entry is its mirror).
                (Some(p), None) => {
                    if let Some(cp) = couple.get_mut(p) {
                        *cp.entry((pr, pc)).or_insert(0.0) += v;
                    }
                }
                (None, Some(_)) => {}
            }
        }
        let mut rhs: Vec<f64> = shared
            .iter()
            .map(|x| b.get(*x).copied().unwrap_or(0.0))
            .collect();
        let mut solved_parts: Vec<SolvedPart> = Vec::new();
        for p in 0..patches {
            let ids = interior.get(p).map_or(&[][..], Vec::as_slice);
            let k = ids.len();
            let blk = blocks.get(p).cloned().unwrap_or_default();
            let bw = blk.keys().map(|(r, c)| r.abs_diff(*c)).max().unwrap_or(0);
            let mut band = Band::new(k.max(1), bw)?;
            for (&(r, c), &v) in &blk {
                band.add(r, c, v);
            }
            if k == 0 {
                solved_parts.push((band, Vec::new(), Vec::new()));
                continue;
            }
            let band = band.factor_rows()?;
            // g = A_kk⁻¹ b_k; per shared column j coupled to this patch, z_j = A_kk⁻¹ A_k,j.
            let bk: Vec<f64> = ids
                .iter()
                .map(|x| b.get(*x).copied().unwrap_or(0.0))
                .collect();
            let g = band.substitute(bk)?;
            let mut columns: std::collections::BTreeMap<usize, Vec<f64>> =
                std::collections::BTreeMap::new();
            for (&(r, j), &v) in couple.get(p).into_iter().flatten() {
                if let std::collections::btree_map::Entry::Vacant(entry) = columns.entry(j) {
                    entry.insert(filled(k, 0.0)?);
                }
                if let Some(cell) = columns.get_mut(&j).and_then(|col| col.get_mut(r)) {
                    *cell += v;
                }
            }
            let mut z: Vec<(usize, Vec<f64>)> = Vec::new();
            for (j, col) in columns {
                z.push((j, band.substitute(col)?));
            }
            // S −= A_Bk·Z, rhs −= A_Bk·g, A_Bk the transpose of the coupling.
            let coupling = couple.get(p).cloned().unwrap_or_default();
            let mut by_column: std::collections::BTreeMap<usize, Vec<(usize, f64)>> =
                std::collections::BTreeMap::new();
            for (&(r, j), &v) in &coupling {
                by_column.entry(j).or_default().push((r, v));
            }
            for (i, entries) in &by_column {
                let dot = |w: &[f64]| {
                    entries
                        .iter()
                        .map(|(r, v)| v * w.get(*r).copied().unwrap_or(0.0))
                        .sum::<f64>()
                };
                if let Some(x) = rhs.get_mut(*i) {
                    *x -= dot(&g);
                }
                for (j, zj) in &z {
                    if let Some(x) = s_mat.get_mut(i * m + j) {
                        *x -= dot(zj);
                    }
                }
            }
            solved_parts.push((band, g, z));
        }
        let xs = if m > 0 {
            cholesky(s_mat, rhs, m)?
        } else {
            Vec::new()
        };
        let mut x = filled(n, 0.0)?;
        for (i, id) in shared.iter().enumerate() {
            if let Some(slot) = x.get_mut(*id) {
                *slot = xs.get(i).copied().unwrap_or(0.0);
            }
        }
        for (p, (_, g, z)) in solved_parts.iter().enumerate() {
            let ids = interior.get(p).map_or(&[][..], Vec::as_slice);
            for (r, id) in ids.iter().enumerate() {
                let mut v = g.get(r).copied().unwrap_or(0.0);
                for (j, zj) in z {
                    v -= zj.get(r).copied().unwrap_or(0.0) * xs.get(*j).copied().unwrap_or(0.0);
                }
                if let Some(slot) = x.get_mut(*id) {
                    *slot = v;
                }
            }
        }
        if x.iter().any(|v| !v.is_finite()) {
            return refuse(LinearSolveFailure::BlocksNonFinite);
        }
        Ok(x)
    }
}

/// Solve the symmetric positive definite `a·x = b` (dense, `n × n`).
fn cholesky(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Result<Vec<f64>> {
    dimension(n)?;
    if a.len() != n * n || b.len() != n {
        return refuse(LinearSolveFailure::InvalidShape);
    }
    let at = |a: &[f64], r: usize, c: usize| a.get(r * n + c).copied().unwrap_or(0.0);
    for c in 0..n {
        let mut d = at(&a, c, c);
        for k in 0..c {
            d -= at(&a, c, k).powi(2);
        }
        if d.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || !d.is_finite() {
            return refuse(LinearSolveFailure::PatchSingular);
        }
        let d = d.sqrt();
        if let Some(x) = a.get_mut(c * n + c) {
            *x = d;
        }
        for r in c + 1..n {
            let mut v = at(&a, r, c);
            for k in 0..c {
                v -= at(&a, r, k) * at(&a, c, k);
            }
            if let Some(x) = a.get_mut(r * n + c) {
                *x = v / d;
            }
        }
    }
    for r in 0..n {
        let mut v = b.get(r).copied().unwrap_or(0.0);
        for k in 0..r {
            v -= at(&a, r, k) * b.get(k).copied().unwrap_or(0.0);
        }
        if let Some(x) = b.get_mut(r) {
            *x = v / at(&a, r, r);
        }
    }
    for r in (0..n).rev() {
        let mut v = b.get(r).copied().unwrap_or(0.0);
        for k in r + 1..n {
            v -= at(&a, k, r) * b.get(k).copied().unwrap_or(0.0);
        }
        if let Some(x) = b.get_mut(r) {
            *x = v / at(&a, r, r);
        }
    }
    if b.iter().any(|x| !x.is_finite()) {
        return refuse(LinearSolveFailure::PatchNonFinite);
    }
    Ok(b)
}
