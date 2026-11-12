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
//!
//! Deviations: [`krawczyk`]'s centre, first box, widening and step count are #15009's as they
//! were in `reach/net.rs`; its operator is the crate's one ([`contraction`], [`k_operator`],
//! the fork's `sm_GnEncloseOver` order, #15140), so its boxes differ from #15009's by
//! roundings only (each operation rounds outward; the order of the terms changed, not the
//! maths).

use crate::blend::fillet_section::certificate::inverse4;
use crate::blend::fillet_section::interval::{cmax, round_up, Ivl};
use crate::error::Result;
use crate::geom::vector3d::Vec3;

/// The point interval 0 (a missing entry's value, as `IZ` in the certificate).
const ZERO: Ivl = Ivl::new(0.0, 0.0);

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
pub(crate) fn contraction<const N: usize>(
    y: &[[f64; N]; N],
    jac: &[[Ivl; N]; N],
) -> ([[Ivl; N]; N], f64) {
    let mut cm = [[Ivl::from(0.0); N]; N];
    let mut q = 0.0_f64;
    for (i, (yi, ci)) in y.iter().zip(cm.iter_mut()).enumerate() {
        let mut row = 0.0_f64;
        for (j, cij) in ci.iter_mut().enumerate() {
            let mut c = Ivl::from(if i == j { 1.0 } else { 0.0 });
            for (&yik, jk) in yi.iter().zip(jac) {
                c = c - Ivl::from(yik) * jk.get(j).copied().unwrap_or(ZERO);
            }
            *cij = c;
            row = round_up(row + c.mag());
        }
        q = cmax(q, row);
    }
    (cm, q)
}

/// Krawczyk's operator over the box `x` about `mid`: `K_i = mid_i − Σ_j y_ij g_j + Σ_j
/// c_ij (x_j − mid_j)`, from `mid_i`, each term subtracted then added in `j`'s order (the
/// fork's `sm_GnEncloseOver`), `g` the system at the point `mid`, `c` the [`contraction`] for
/// `y`. A box `x` with `K` strictly inside it holds a root of the system (module docs); each
/// caller keeps its own centre, first box, preconditioner and widening, and a caller whose
/// proof needs `q < 1` tests it itself (tangency-labs/solidcraft#15140).
pub(crate) fn k_operator<const N: usize>(
    mid: &[f64; N],
    y: &[[f64; N]; N],
    g: &[Ivl; N],
    c: &[[Ivl; N]; N],
    x: &[Ivl; N],
) -> [Ivl; N] {
    let mut kk = [Ivl::from(0.0); N];
    for (i, ((yi, ci), ki)) in y.iter().zip(c).zip(kk.iter_mut()).enumerate() {
        let mut k = Ivl::from(mid.get(i).copied().unwrap_or(f64::NAN));
        for (&yij, gj) in yi.iter().zip(g) {
            k = k - Ivl::from(yij) * *gj;
        }
        for (j, &cij) in ci.iter().enumerate() {
            let xj = x.get(j).copied().unwrap_or(ZERO);
            k = k + cij * (xj - Ivl::from(mid.get(j).copied().unwrap_or(f64::NAN)));
        }
        *ki = k;
    }
    kk
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
