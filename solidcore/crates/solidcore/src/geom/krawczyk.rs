//! One Krawczyk step for a system of `N` equations in `N` unknowns, in rounded intervals, and
//! the Newton root it is centred on (not upstream; moved from
//! `offset::offset_executive::reach::net`, #15009, so that the blend code's copies can share
//! it, tangency-labs/solidcraft#15140).
//!
//! Krawczyk's operator (definitions-and-certificates.md §4.3): a box `Y` about a point `ỹ`
//! with `K(Y) = ỹ − C F(ỹ) + (I − C F′(Y)) (Y − ỹ)` strictly inside it holds a root of `F`,
//! for any matrix `C` [Krawczyk 1969; Moore 1977], where `F′(Y)` encloses `F`'s partials over
//! `Y` (or, for an `F` continuous and smooth piecewise across lines inside `Y`, their hull:
//! the mean-value form holds along every segment [Neumaier 1990, §2.1]). [`krawczyk`] takes
//! `C` (an approximate inverse of `F′` at `ỹ`, [`newton`]'s or a caller's), `F` at a point
//! box and `F′` over a box as closures, starts from a box of twice Newton's step about `ỹ`
//! and widens it (epsilon-inflation) for at most [`KRAWCZYK_STEPS`] steps. The small
//! inverses take their matrices by columns, as the callers' `F′` closures give them.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`krawczyk`], [`newton`], [`KRAWCZYK_STEPS`], [`NEWTON_STEPS`] | (not upstream: #15009's, moved) |
//! | [`inverse3`], [`inverse_of_columns2`], [`inverse_of_columns3`], [`inverse_of_columns4`] | (not upstream: #15009's, moved) |
//! | [`contraction`], [`k_operator`] | `sm_GnEncloseOver`'s `C`, `q` and `K` (the fork's order), extracted (#15140) |
//! | [`KNumber`], [`start_minus_y`], [`plus_c_times`] | `K`'s two sums, in the same order, for the feet's Taylor-model tube (not upstream: #14405's, #15140) |
//!
//! Deviations: [`krawczyk`]'s centre, first box, widening and step count are #15009's as they
//! were in `reach/net.rs`; its operator is the crate's one ([`contraction`], [`k_operator`],
//! the fork's `sm_GnEncloseOver` order, #15140), so its boxes differ from #15009's by
//! roundings only (each operation rounds outward; the order of the terms changed, not the
//! maths). [`contraction`], [`start_minus_y`] and [`plus_c_times`] take any [`KNumber`]:
//! [`Ivl`] for the certificate, `net.rs`, the ball and [`krawczyk`], a Taylor model
//! ([`TmN`]) for the feet's tube (`feet_tm.rs`, Rump's form about its polynomial), each in
//! the operations and order it had in its own copy, so every caller's bits are unchanged.

use crate::blend::fillet_section::certificate::inverse4;
use crate::blend::fillet_section::interval::{cmax, round_up, Ivl};
use crate::blend::fillet_section::tay::Ring;
use crate::blend::fillet_section::taylor_model::TmN;
use crate::error::Result;
use crate::geom::vector3d::Vec3;

/// The point interval 0 (a missing entry's value, as `IZ` in the certificate).
const ZERO: Ivl = Ivl::new(0.0, 0.0);

/// A number Krawczyk's operator is formed in: an interval, or a Taylor model in one
/// variable over intervals (`feet_tm.rs`'s tube), every operation rounding outward.
pub(crate) trait KNumber: Ring {
    /// `self` times the number `y`, as each caller's copy formed it: [`Ivl`]'s product
    /// `y · self`, [`TmN::scale`].
    fn scaled(self, y: f64) -> Self;
    /// The range of values `self` encloses: an [`Ivl`] itself, a [`TmN`]'s
    /// [`TmN::range`] over `[-1, 1]`.
    fn hull(self) -> Ivl;
}

impl KNumber for Ivl {
    fn scaled(self, y: f64) -> Self {
        Ivl::from(y) * self
    }
    fn hull(self) -> Ivl {
        self
    }
}

impl<const M: usize> KNumber for TmN<M> {
    fn scaled(self, y: f64) -> Self {
        self.scale(Ivl::from(y))
    }
    fn hull(self) -> Ivl {
        self.range()
    }
}

/// The most Krawczyk steps one root gets ([`krawczyk`]).
pub(crate) const KRAWCZYK_STEPS: usize = 8;

/// The most Newton steps for the root a Krawczyk box is centred on.
pub(crate) const NEWTON_STEPS: usize = 40;

/// The inverse of the 3 × 3 matrix of columns `c`, as rows; `None` where it is singular in
/// floating point.
pub(crate) fn inverse3(c: [Vec3; 3]) -> Option<[[f64; 3]; 3]> {
    let [a, b, d] = c;
    let det = a.dot(b.cross(d));
    if !(det.is_finite() && det != 0.0) {
        return None;
    }
    let rows = [b.cross(d), d.cross(a), a.cross(b)].map(|r| [r.x / det, r.y / det, r.z / det]);
    rows.iter().flatten().all(|x| x.is_finite()).then_some(rows)
}

/// Krawczyk's test about `mid` with the approximate inverse `c` (rows), `F` enclosed at a
/// point box by `value` and `F′` over a box by `jacobian` (columns): the box proved to hold
/// a root (module docs), or `None` where none is within [`KRAWCZYK_STEPS`] steps.
///
/// # Errors
///
/// Those of `value` and `jacobian` (a cancel).
pub(crate) fn krawczyk<const N: usize>(
    mid: [f64; N],
    c: [[f64; N]; N],
    value: &dyn Fn([Ivl; N]) -> Result<Option<[Ivl; N]>>,
    jacobian: &dyn Fn([Ivl; N]) -> Result<Option<[[Ivl; N]; N]>>,
) -> Result<Option<[Ivl; N]>> {
    let Some(f) = value(mid.map(Ivl::from))? else {
        return Ok(None);
    };
    let times = |row: &[f64; N], x: &[Ivl; N]| {
        row.iter()
            .zip(x)
            .fold(Ivl::from(0.0), |s, (&r, &v)| s + Ivl::from(r) * v)
    };
    let cf: [Ivl; N] = c.map(|row| times(&row, &f));
    // the first box: twice Newton's step from the middle, and a little more
    let mut radius = cf.map(|x| 2.0 * x.mag());
    for (r, m) in radius.iter_mut().zip(&mid) {
        *r = round_up(*r + 1e-12 * (1.0 + m.abs()));
    }
    for _ in 0..KRAWCZYK_STEPS {
        let mut y = mid.map(Ivl::from);
        for ((yi, &m), &r) in y.iter_mut().zip(&mid).zip(&radius) {
            if !r.is_finite() {
                return Ok(None);
            }
            *yi = Ivl::new(m - r, m + r);
        }
        let Some(j) = jacobian(y)? else {
            return Ok(None);
        };
        // the crate's one operator (#15140), in its order: `F′` by rows from the columns given
        let rows: [[Ivl; N]; N] = core::array::from_fn(|r| {
            core::array::from_fn(|col| j.get(col).and_then(|cl| cl.get(r)).copied().unwrap_or(ZERO))
        });
        let (cm, _) = contraction(&c, &rows);
        let k = k_operator(&mid, &c, &f, &cm, &y);
        let inside = k.iter().zip(&y).all(|(a, b)| a.lo > b.lo && a.hi < b.hi);
        if inside {
            let mut out = k;
            for (o, b) in out.iter_mut().zip(&y) {
                *o = Ivl::new(o.lo.max(b.lo), o.hi.min(b.hi));
            }
            return Ok(Some(out));
        }
        for ((r, ki), &m) in radius.iter_mut().zip(&k).zip(&mid) {
            let reach = (ki.lo - m).abs().max((ki.hi - m).abs());
            *r = round_up(1.5 * reach.max(*r) + 1e-12 * (1.0 + m.abs()));
        }
    }
    Ok(None)
}

/// The contraction of Krawczyk's operator for the preconditioner `y` (rows) and `F′` over a
/// box `jac` (rows: `jac[k][j]` the `k`th equation's partial in the `j`th unknown):
/// `C = I − Y F′`, each `C_ij` formed from `δ_ij` by subtracting `y_ik F′_kj` in `k`'s
/// order, and `q = max_i Σ_j |C_ij|`, each row's sum rounded up as it grows. The fork's
/// order (`sm_GnEncloseOver`), shared with [`k_operator`] (tangency-labs/solidcraft#15140).
/// Over Taylor models, `q` is taken over their ranges ([`KNumber::hull`]).
pub(crate) fn contraction<T: KNumber, const N: usize>(
    y: &[[f64; N]; N],
    jac: &[[T; N]; N],
) -> ([[T; N]; N], f64) {
    let mut cm = [[T::lift(0.0); N]; N];
    let mut q = 0.0_f64;
    for (i, (yi, ci)) in y.iter().zip(cm.iter_mut()).enumerate() {
        let mut row = 0.0_f64;
        for (j, cij) in ci.iter_mut().enumerate() {
            let mut c = T::lift(if i == j { 1.0 } else { 0.0 });
            for (&yik, jk) in yi.iter().zip(jac) {
                // never read: `j < N` and every row of `jac` holds N entries
                let jkj = jk.get(j).copied().unwrap_or_else(|| T::lift(0.0));
                c = c - jkj.scaled(yik);
            }
            *cij = c;
            row = round_up(row + c.hull().mag());
        }
        q = cmax(q, row);
    }
    (cm, q)
}

/// `start_i − Σ_j y_ij g_j`, each term subtracted from `start_i` in `j`'s order: the first
/// sum of Krawczyk's operator ([`k_operator`] from `mid`; the feet's tube from 0, `−Y G`).
pub(crate) fn start_minus_y<T: KNumber, const N: usize>(
    start: [T; N],
    y: &[[f64; N]; N],
    g: &[T; N],
) -> [T; N] {
    let mut out = start;
    for (o, yi) in out.iter_mut().zip(y) {
        for (&yij, &gj) in yi.iter().zip(g) {
            *o = *o - gj.scaled(yij);
        }
    }
    out
}

/// `start_i + Σ_j c_ij delta_j`, each term added to `start_i` in `j`'s order: the second
/// sum of Krawczyk's operator ([`k_operator`] with `delta = x − mid`; the feet's tube with
/// its radii `[−r_j, r_j]`, and its coefficients' iteration and defect).
pub(crate) fn plus_c_times<T: Ring, const N: usize>(
    start: [T; N],
    c: &[[T; N]; N],
    delta: &[T; N],
) -> [T; N] {
    let mut out = start;
    for (o, ci) in out.iter_mut().zip(c) {
        for (&cij, &dj) in ci.iter().zip(delta) {
            *o = *o + cij * dj;
        }
    }
    out
}

/// Krawczyk's operator over the box `x` about `mid`: `K_i = mid_i − Σ_j y_ij g_j + Σ_j
/// c_ij (x_j − mid_j)`, from `mid_i`, each term subtracted then added in `j`'s order (the
/// fork's `sm_GnEncloseOver`), `g` the system at the point `mid`, `c` the [`contraction`] for
/// `y`. A box `x` with `K` strictly inside it holds a root of the system (module docs); each
/// caller keeps its own centre, first box, preconditioner and widening, and a caller whose
/// proof needs `q < 1` tests it itself (tangency-labs/solidcraft#15140). It is
/// [`plus_c_times`] after [`start_minus_y`] from `mid`, with `delta = x − mid`.
pub(crate) fn k_operator<const N: usize>(
    mid: &[f64; N],
    y: &[[f64; N]; N],
    g: &[Ivl; N],
    c: &[[Ivl; N]; N],
    x: &[Ivl; N],
) -> [Ivl; N] {
    let mut delta = *x;
    for (dj, &mj) in delta.iter_mut().zip(mid) {
        *dj = *dj - Ivl::from(mj);
    }
    plus_c_times(start_minus_y(mid.map(Ivl::from), y, g), c, &delta)
}

/// `F` at a point and its columns there, as numbers ([`newton`]).
pub(crate) type Linearised<const N: usize> = Option<([f64; N], [[f64; N]; N])>;

/// A square matrix's inverse (rows) from its columns, `None` where it is singular.
pub(crate) type Inverse<const N: usize> = dyn Fn(&[[f64; N]; N]) -> Option<[[f64; N]; N]>;

/// Newton's root of `F` from `start` (`at` gives `F` and its columns there): the root, or
/// `None` where a step is not defined or the steps do not settle.
pub(crate) fn newton<const N: usize>(
    start: [f64; N],
    at: &dyn Fn([f64; N]) -> Result<Linearised<N>>,
    inverse: &Inverse<N>,
) -> Result<Option<[f64; N]>> {
    let mut y = start;
    let mut last = f64::INFINITY;
    for _ in 0..NEWTON_STEPS {
        let Some((f, j)) = at(y)? else {
            return Ok(None);
        };
        let Some(c) = inverse(&j) else {
            return Ok(None);
        };
        let mut size = 0.0_f64;
        let mut next = y;
        for (yi, row) in next.iter_mut().zip(&c) {
            let step: f64 = row.iter().zip(&f).map(|(r, x)| r * x).sum();
            if !step.is_finite() {
                return Ok(None);
            }
            size = size.max(step.abs());
            *yi -= step;
        }
        // settled: the step no longer shrinks, at the rounding of `F` wherever the part
        // lies (no stop relative to `y`, which a part far from the origin never meets)
        if size == 0.0 || size >= last {
            return Ok(Some(y));
        }
        y = next;
        last = size;
    }
    // a start only: Krawczyk's box decides whether a root is near
    Ok(y.iter().all(|x| x.is_finite()).then_some(y))
}

/// The inverse (rows) of a 3 × 3 matrix given by columns.
pub(crate) fn inverse_of_columns3(j: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    inverse3(j.map(|[x, y, z]| Vec3::new(x, y, z)))
}

/// The inverse (rows) of a 4 × 4 matrix given by columns.
pub(crate) fn inverse_of_columns4(j: &[[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut rows = [[0.0_f64; 4]; 4];
    for (jj, col) in j.iter().enumerate() {
        for (row, &x) in rows.iter_mut().zip(col) {
            if let Some(r) = row.get_mut(jj) {
                *r = x;
            }
        }
    }
    let inv = inverse4(&rows).ok()?;
    inv.iter().flatten().all(|x| x.is_finite()).then_some(inv)
}

/// The inverse (rows) of a 2 × 2 matrix given by columns.
pub(crate) fn inverse_of_columns2(j: &[[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let [[a, c], [b, d]] = *j;
    let det = a * d - b * c;
    if !(det.is_finite() && det != 0.0) {
        return None;
    }
    let inv = [[d / det, -b / det], [-c / det, a / det]];
    inv.iter().flatten().all(|x| x.is_finite()).then_some(inv)
}

#[cfg(test)]
mod tests {
    //! Closed forms of the one operator (#15140): `x² − 2` has its root √2 bracketed;
    //! over `[2, 3]` its operator lies wholly below the box, so the box holds no root; a
    //! singular Jacobian has no inverse, and Newton refuses it.
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test

    use super::*;

    /// `x² − 2` at a point box.
    fn value(x: [Ivl; 1]) -> [Ivl; 1] {
        [x[0] * x[0] - Ivl::from(2.0)]
    }

    /// Its derivative `2x` over a box, as columns.
    fn jacobian(x: [Ivl; 1]) -> [[Ivl; 1]; 1] {
        [[Ivl::from(2.0) * x[0]]]
    }

    #[test]
    fn the_root_of_x_squared_minus_two_is_proved_in_a_box_about_root_two() {
        let mid = core::f64::consts::SQRT_2;
        let c = [[1.0 / (2.0 * mid)]];
        let f = |x| -> Result<Option<[Ivl; 1]>> { Ok(Some(value(x))) };
        let j = |x| -> Result<Option<[[Ivl; 1]; 1]>> { Ok(Some(jacobian(x))) };
        let b = krawczyk([mid], c, &f, &j).unwrap().unwrap()[0];
        // the box holds √2 (its square brackets 2) and is no wider than the first box
        assert!(b.lo * b.lo < 2.0 && 2.0 < b.hi * b.hi, "{b:?}");
        assert!(b.hi - b.lo < 1e-10, "{b:?}");
    }

    #[test]
    fn a_box_whose_operator_lies_outside_it_holds_no_root() {
        // over X = [2, 3] about 2.5, with y = 1 / (2 · 2.5): C = 1 − y [4, 6] = [−0.2, 0.2],
        // q = 0.2, and K = 2.5 − y · 4.25 + C · [−0.5, 0.5] = [1.55, 1.75], below X
        let (mid, y) = ([2.5], [[0.2]]);
        let x = [Ivl::new(2.0, 3.0)];
        let (c, q) = contraction(&y, &jacobian(x));
        // the closed forms, to the operations' outward rounding
        let near = |a: f64, b: f64| (a - b).abs() < 1e-12;
        assert!(near(c[0][0].lo, -0.2) && near(c[0][0].hi, 0.2), "{c:?}");
        assert!(near(q, 0.2), "{q}");
        let g = value([Ivl::from(2.5)]);
        let k = k_operator(&mid, &y, &g, &c, &x)[0];
        assert!(near(k.lo, 1.55) && near(k.hi, 1.75), "{k:?}");
        // K misses X: by Krawczyk's theorem every root in X lies in K ∩ X, which is empty
        assert!(k.hi < x[0].lo);
    }

    #[test]
    fn a_singular_jacobian_has_no_inverse_and_newton_refuses_it() {
        assert_eq!(inverse_of_columns2(&[[1.0, 2.0], [2.0, 4.0]]), None);
        assert_eq!(
            inverse_of_columns3(&[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]]),
            None
        );
        // F(x, y) = (x + y − 1, 2x + 2y − 2): its Jacobian's columns are dependent everywhere
        let at = |p: [f64; 2]| -> Result<Linearised<2>> {
            Ok(Some((
                [p[0] + p[1] - 1.0, 2.0 * p[0] + 2.0 * p[1] - 2.0],
                [[1.0, 2.0], [1.0, 2.0]],
            )))
        };
        assert_eq!(newton([0.0, 0.0], &at, &inverse_of_columns2).unwrap(), None);
    }
}
