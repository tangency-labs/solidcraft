//! A certified bound of the Hausdorff distance between two curves that both run along one
//! circle: a circular arc's rational NURB and a B-spline computed along it, whatever their
//! parameterisations.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`RefCircle`]              | none     |
//! | [`hausdorff_about_circle`] | none     |
//!
//! [`crate::nurbs::crv_deviation::certified_hausdorff`] compares two curves at equal
//! parameters, which an arc (parameterised by its rational quadratic NURB) and a curve
//! computed along it (a section curve, parameterised by its fit) do not share. Here each
//! curve is compared with a reference circle instead, and the two with each other through it.
//!
//! **Frame.** The circle's frame `M` (rows: its X axis, its Y axis and their computed cross
//! product) maps a point `p` to `M (p - c)`. In that space the circle is exactly
//! `x² + y² = r², z = 0`. `M` is orthonormal up to `g`, the largest deviation of `M Mᵀ` from
//! the identity (each entry computed, plus `γ_4` of its terms' sum): `‖M Mᵀ - I‖₂ ≤ 3 g`, so
//! every singular value of `M` lies in `[√(1 - 3g), √(1 + 3g)]` and, for `3 g ≤ 1/2`,
//! distances grow or shrink between the spaces by at most `s = 1 + 3 g`. The bound is
//! computed in `M`'s space and multiplied by `s`.
//!
//! **One curve.** Over its interval, the curve is cut into Bézier pieces (the interval's ends
//! inserted as knots, [`Curve::refine`], [`Curve::decompose_bez`]) whose homogeneous control
//! points `(X, Y, Z, W)` are mapped into the frame. Its points `p(t) = (x, y, z)` then satisfy:
//!
//! - `F(t) = x² + y² - r² = N(t) / W(t)²` with `N = X² + Y² - r² W²`, a polynomial of twice
//!   the degree whose Bernstein coefficients are the products' (`N_k = Σ_(i+j=k)
//!   C(p,i) C(p,j) / C(2p,k) (X_i X_j + Y_i Y_j - r² W_i W_j)`), and likewise `W² = Σ V_k B_k`
//!   with every `V_k > 0` (positive weights). `F = Σ N_k B_k / Σ V_k B_k` is a weighted mean
//!   of the `N_k / V_k`, so on a piece `|F| ≤ max |N_k| / V_k` and `F ≥ min N_k / V_k` (exact
//!   for an arc of a concentric circle, whose `N` is a multiple of `W²`). `N` and `V` are
//!   halved by de Casteljau's algorithm while a piece's bound is well above the largest `|F|`
//!   seen at a piece's end (at most [`MAX_LEVEL`] times, [`MAX_ITEMS`] pieces per curve): the
//!   bound is that of every piece left, whatever the halving does, and the halving only
//!   tightens it, the halves' coefficients being means of the whole's. Then
//!   `ρ = √(x² + y²) ≥ ρ_min = √(r² + min F)` (`min F` of either sign) and
//!   `|ρ - r| = |F| / (ρ + r) ≤ D_ρ = max|F| / (ρ_min + r)`.
//! - `|z| ≤ D_z`, the largest `|Z_i / W_i|` (a rational Bézier piece of positive weights lies
//!   in the convex hull of its points).
//! - Its ends' directions `u0`, `u1` (the planar parts of its first and last homogeneous
//!   points, of the same directions as the points themselves since `W > 0`), ordered so that
//!   `u0 × u1 > 0`. With `m = û0 + û1`, every control point has `m · p > 0` (so the whole
//!   curve does: `ρ > 0` and the polar angle `φ(t)` about `m` is continuous in `(-π/2, π/2)`)
//!   and `m × u1 ≥ 0`, `u0 × m ≥ 0` (the ends lie at `φ1 ≥ 0 ≥ φ0`). Each is checked with its
//!   rounding.
//! - `ℓ`, the largest signed distance of a control point past the half plane of `u1`
//!   (`û1 × p`) or of `u0` (`p × û0`), or 0: how far the curve runs past its ends' angles.
//!
//! Let `A'` be the arc of the circle from `φ0` to `φ1`. Each point of `A'` is, by the
//! intermediate value theorem, `r (cos φ(t), sin φ(t), 0)` for some `t`, within
//! `√((ρ - r)² + z²) ≤ √(D_ρ² + D_z²)` of `p(t)`. Each `p(t)` with `φ(t)` in `[φ0, φ1]` is
//! as close to that point of `A'`; one past `φ1` by `δ ∈ (0, π/2]` (`φ < π/2` and `φ1 ≥ 0`)
//! has `ℓ ≥ ρ sin δ` and, as `1 - cos δ ≤ sin² δ`, is within
//! `√((D_ρ + ℓ² / ρ_min)² + ℓ² + D_z²)` of `A'`'s end `r û1` (likewise past `φ0`). That is
//! `H`, the curve's Hausdorff distance from `A'`.
//!
//! **Two curves.** `H(a, b) ≤ H(a, A') + H(A', B') + H(B', b)`. With `b`'s ends also on `a`'s
//! side of its line (`m_a · u > 0`), all four ends' angles lie in one chart
//! `(-π/2, π/2)`, and two arcs `[α0, α1]`, `[β0, β1]` of one circle in it are within
//! `max(r |û_a0 - û_b0|, r |û_a1 - û_b1|)` of each other: a point of one outside the other
//! lies between an end of each, nearer than the two ends' chord (less than `π` apart).
//!
//! **Rounding** (docs/definitions-and-certificates.md §4.4). Knot insertion carries
//! `γ_(12 (I + 1))` of twice the largest homogeneous coordinate (as in
//! [`crate::nurbs::crv_deviation`]), and moving a point into the frame `γ_5` of its terms'
//! sum; with `W̃` a lower bound on the exact smallest weight (the computed one less `e`, rounded
//! down; refused unless `e` is under half the computed one), a homogeneous error `e` moves
//! the curve by at most
//! `√3 e (1 + P) / W̃` (`P` its largest coordinate), counted as `2 e (1 + P) / W̃`. Each
//! `N_k` (`V_k`) carries `γ_(2p + 12)` of `S`, the largest sum of its (their) terms'
//! magnitudes over the piece (at most `p + 8` roundings: the binomial ratio, the products,
//! the sums), and a halving level at most `n` more roundings of a degree-`n` polynomial's
//! coefficients, each a mean of two (`γ_(n + 12 + n L)` of `S` after `L` levels,
//! `γ_j + γ_k + γ_j γ_k ≤ γ_(j+k)`). Every other step rounds by a few `γ`,
//! counted on its result.
//!
//! **Underflow.** The rounding model above holds in the normal range. A rational curve is the
//! same curve with all its homogeneous coordinates scaled by one factor, so each curve is
//! first scaled, exactly, by the power of two that brings its largest weight into `[1, 2)`
//! (no bound when that scaling is not exact); with the refusal of a smallest weight within
//! twice `e` of 0 every weight is then above `5·10^-15`, and with a radius of at least
//! [`R_MIN`] the largest `F` coefficient's term-magnitude sum `S` is past `10^-209`, far
//! above the at most `2^-1075` an operation in the subnormal range loses, which the `γ` count's spare
//! roundings (`2p + 12` counted, at most `p + 8` taken) cover. A planar cross or dot product
//! one of whose products underflows has no error bound (no bound for the curves);
//! `D_z`, `ℓ` and `D_ρ`'s sum are raised to [`FLOOR`] when positive and below it, so that
//! their squares are normal. The frame map's bound carries an absolute `16 · 2^-1074` for
//! the at most eleven operations of a coordinate that may round in the subnormal range
//! (each losing at most `2^-1075` there).
//!
//! Deviations: none (no upstream counterpart). 3D curves of positive weights (or
//! non-rational), degree 1 to [`MAX_DEGREE`], knots clamped; arcs of less than a half turn
//! about the reference circle. The halving is a work list, not a recursion, of at most
//! [`MAX_LEVEL`] levels and [`MAX_ITEMS`] pieces.

use crate::error::query::QueryFailure;
use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::knots::MAX_DEGREE;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};

const OP: &str = "nurbs::circle_deviation::hausdorff_about_circle";

/// The most times a piece's `N` and `W` are halved (module docs).
pub const MAX_LEVEL: usize = 20;

/// The most pieces one curve's `F` bound looks at, halved ones included (module docs).
pub const MAX_ITEMS: usize = 1 << 13;

/// The smallest radius a bound is given for (module docs, underflow).
pub const R_MIN: f64 = 1e-90;

/// What `D_z`, `ℓ` and `D_ρ`'s sum are raised to when positive and below it, so that their
/// squares are normal (module docs, underflow).
pub const FLOOR: f64 = 1e-150;

/// The reference circle: center `center`, radius `radius`, in the plane of `x_axis` and
/// `y_axis` (unit and orthogonal up to rounding; the module docs count how far).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefCircle {
    /// The center.
    pub center: Vec3,
    /// The unit X axis of the circle's plane.
    pub x_axis: Vec3,
    /// The unit Y axis of the circle's plane.
    pub y_axis: Vec3,
    /// The radius.
    pub radius: f64,
}

/// A bound `d` such that every point of `a` over `ia` is within `d` of a point of `b` over
/// `ib` and every point of `b` over `ib` within `d` of a point of `a` over `ia` (their
/// Hausdorff distance), every rounding of computing it included, found through the circle
/// `circle` both run along (module docs). The curves' parameterisations need not be related.
///
/// # Errors
///
/// `InvalidInput` for a 2D curve, a weight not positive, a number not finite, knots not
/// clamped, decreasing or repeated past the degree, a degree out of range, an interval reversed, of no length or not inside its
/// curve's domain, or a circle of a radius not positive; [`QueryFailure::BoundOverflows`] when
/// the bound overflows; those of the refinement and the extraction. `Ok(None)` (no bound, an
/// answer rather than a failure) when a curve does not run along the circle closely enough
/// for one: a point on its axis, a half turn or more, or ends not on one side; or when the
/// numbers leave the range the rounding is counted in (a radius below [`R_MIN`], a weight
/// scaling that is not exact).
pub fn hausdorff_about_circle(
    a: &Curve,
    ia: [f64; 2],
    b: &Curve,
    ib: [f64; 2],
    circle: &RefCircle,
) -> Result<Option<f64>> {
    let r = circle.radius;
    if !(r.is_finite() && r > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a circle of a radius not positive",
        ));
    }
    if r < R_MIN {
        return Ok(None);
    }
    let frame = Frame::of(circle)?;
    let Some(na) = Near::of(a, ia, &frame, r)? else {
        return Ok(None);
    };
    let Some(nb) = Near::of(b, ib, &frame, r)? else {
        return Ok(None);
    };
    // `b`'s ends on `a`'s side, so that all four ends' angles share `a`'s chart
    if !(na.on_side(nb.u0) && na.on_side(nb.u1)) {
        return Ok(None);
    }
    let chord = |p: [f64; 2], q: [f64; 2]| -> Option<f64> {
        let (p, q) = (unit(p)?, unit(q)?);
        let (dx, dy) = (p[0] - q[0], p[1] - q[1]);
        // each unit vector's components carry `γ_4`, the difference and norm `γ_4` more
        Some(r * ((dx * dx + dy * dy).sqrt() + 4.0 * gamma(4)) * (1.0 + gamma(4)))
    };
    let (Some(c0), Some(c1)) = (chord(na.u0, nb.u0), chord(na.u1, nb.u1)) else {
        return Ok(None);
    };
    let arcs = c0.max(c1);
    let s = frame.stretch;
    let moved = s * (na.real_moved + nb.real_moved) + na.frame_moved + nb.frame_moved;
    let bound = s * (na.h + nb.h + arcs + moved) * (1.0 + gamma(6));
    if !bound.is_finite() {
        return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
    }
    Ok(Some(bound))
}

/// `v` divided by its length, each component within `γ_4` of the exact unit vector's; `None`
/// for a vector too short (or too long) to divide by its length (no bound).
fn unit(v: [f64; 2]) -> Option<[f64; 2]> {
    let [x, y] = v;
    let n = (x * x + y * y).sqrt();
    if !(n.is_finite() && n > f64::MIN_POSITIVE) {
        return None;
    }
    Some([x / n, y / n])
}

/// The circle's frame: its rows, center and stretch `s` (module docs).
struct Frame {
    rows: [Vec3; 3],
    center: Vec3,
    stretch: f64,
}

impl Frame {
    fn of(c: &RefCircle) -> Result<Self> {
        let (e1, e2) = (c.x_axis, c.y_axis);
        let n = Vec3::new(
            e1.y * e2.z - e1.z * e2.y,
            e1.z * e2.x - e1.x * e2.z,
            e1.x * e2.y - e1.y * e2.x,
        );
        let rows = [e1, e2, n];
        let fin = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
        if !(rows.iter().all(|&v| fin(v)) && fin(c.center)) {
            return Err(SolidError::invalid_input(OP, "numbers not finite"));
        }
        let mut g = 0.0_f64;
        for (i, p) in rows.iter().enumerate() {
            for (j, q) in rows.iter().enumerate() {
                let dot = p.x * q.x + p.y * q.y + p.z * q.z;
                let sum = (p.x * q.x).abs() + (p.y * q.y).abs() + (p.z * q.z).abs();
                let id = if i == j { 1.0 } else { 0.0 };
                g = g.max(((dot - id).abs() + gamma(4) * sum) * (1.0 + gamma(2)));
            }
        }
        let three_g = 3.0 * g * (1.0 + gamma(1));
        if three_g.is_nan() || three_g > 0.5 {
            return Err(SolidError::invalid_input(
                OP,
                "a circle whose axes are not unit and orthogonal",
            ));
        }
        Ok(Self {
            rows,
            center: c.center,
            stretch: (1.0 + three_g) * (1.0 + gamma(1)),
        })
    }

    /// The homogeneous point `(x, y, z, w)` in the frame, `M (x - w c)`, and the error bound
    /// of each of its coordinates (`γ_5` of its terms' sum, plus `16 · 2^-1074` for steps
    /// that round in the subnormal range, module docs).
    fn map(&self, p: [f64; 4]) -> ([f64; 4], f64) {
        let [x, y, z, w] = p;
        let d = [
            x - w * self.center.x,
            y - w * self.center.y,
            z - w * self.center.z,
        ];
        let m = [
            x.abs() + (w * self.center.x).abs(),
            y.abs() + (w * self.center.y).abs(),
            z.abs() + (w * self.center.z).abs(),
        ];
        let mut out = [0.0; 4];
        let mut err = 0.0_f64;
        for (o, e) in out.iter_mut().zip(&self.rows) {
            let [d0, d1, d2] = d;
            let [m0, m1, m2] = m;
            *o = e.x * d0 + e.y * d1 + e.z * d2;
            let sum = e.x.abs() * m0 + e.y.abs() * m1 + e.z.abs() * m2;
            err = err.max(gamma(5) * sum * (1.0 + gamma(4)) + 16.0 * f64::from_bits(1));
        }
        if let Some(o) = out.get_mut(3) {
            *o = w;
        }
        (out, err)
    }
}

/// What one curve's comparison with the circle gives (module docs).
struct Near {
    /// Its ends' planar directions, `u0 × u1 > 0`.
    u0: [f64; 2],
    u1: [f64; 2],
    /// `m = û0 + û1`.
    m: [f64; 2],
    /// Its Hausdorff distance from the arc `A'` (in the frame).
    h: f64,
    /// How far rounding moved its pieces before they were mapped (real space) and after.
    real_moved: f64,
    frame_moved: f64,
}

/// Whether `x y`, rounded, may have lost more than its relative rounding: both factors
/// nonzero and the product below the normal range.
fn underflows(x: f64, y: f64) -> bool {
    x != 0.0 && y != 0.0 && (x * y).abs() < f64::MIN_POSITIVE
}

/// The error bound of `a ± b`, the rounded products `x0 y0` and `x1 y1`: `γ_2` of the
/// magnitudes' sum (a sum or difference in the subnormal range is exact), or infinite when a
/// product underflowed (no bound).
fn two_terms([x0, y0, x1, y1]: [f64; 4], a: f64, b: f64) -> f64 {
    if underflows(x0, y0) || underflows(x1, y1) {
        f64::INFINITY
    } else {
        gamma(2) * (a.abs() + b.abs())
    }
}

/// `p × q` (planar) and its error bound.
fn cross(p: [f64; 2], q: [f64; 2]) -> (f64, f64) {
    let (a, b) = (p[0] * q[1], p[1] * q[0]);
    (a - b, two_terms([p[0], q[1], p[1], q[0]], a, b))
}

/// `p · q` (planar) and its error bound.
fn dot(p: [f64; 2], q: [f64; 2]) -> (f64, f64) {
    let (a, b) = (p[0] * q[0], p[1] * q[1]);
    (a + b, two_terms([p[0], q[0], p[1], q[1]], a, b))
}

impl Near {
    /// Whether the direction `u` is on `m`'s side, rounding included.
    fn on_side(&self, u: [f64; 2]) -> bool {
        let (d, e) = dot(u, self.m);
        d > e
    }

    fn of(c: &Curve, [lo, hi]: [f64; 2], frame: &Frame, r: f64) -> Result<Option<Self>> {
        let Some((c, pieces, inserted)) = checked_pieces(c, [lo, hi])? else {
            return Ok(None);
        };
        let c = &c;
        // homogeneous coordinates, a non-rational point weighing 1
        let homog = |p: &CPoint| {
            let w = if p.w == NOW { 1.0 } else { p.w };
            [p.x, p.y, p.z, w]
        };
        // the refinement's error (module docs)
        let size = c.cpts().iter().fold(0.0_f64, |m, p| {
            let [x, y, z, w] = homog(p);
            m.max(x.abs()).max(y.abs()).max(z.abs()).max(w.abs())
        });
        let largest = c.cpts().iter().fold(0.0_f64, |m, p| {
            let [x, y, z, w] = homog(p);
            m.max((x / w).abs()).max((y / w).abs()).max((z / w).abs())
        }) * (1.0 + gamma(1));
        let e_ref = gamma(inserted.saturating_add(1).saturating_mul(12)) * 2.0 * size;

        let mut mapped: Vec<Vec<[f64; 4]>> = Vec::new();
        let mut e_frame = 0.0_f64;
        let mut w_min = f64::INFINITY;
        for piece in &pieces {
            let mut pts = Vec::new();
            for p in piece.cpts() {
                let h = homog(p);
                let [_, _, _, w] = h;
                if !(w > 0.0 && w.is_finite()) {
                    return Err(SolidError::invalid_input(OP, "a weight not positive"));
                }
                w_min = w_min.min(w);
                let (q, e) = frame.map(h);
                if !(q.iter().all(|x| x.is_finite()) && e.is_finite()) {
                    return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
                }
                e_frame = e_frame.max(e);
                pts.push(q);
            }
            mapped.push(pts);
        }
        if !(w_min > 0.0 && w_min.is_finite()) {
            return Err(SolidError::invalid_input(OP, "a weight not positive"));
        }
        // the computed weights are within `e_ref` of the exact ones, so the exact
        // smallest is at least `w_min - e_ref`, taken rounded down
        if e_ref.is_nan() || e_ref >= 0.5 * w_min {
            return Ok(None);
        }
        let w_low = (w_min - e_ref) * (1.0 - gamma(1));
        // multiplied by the frame's stretch where it is summed (`hausdorff_about_circle`)
        let real_moved = 2.0 * e_ref * (1.0 + largest) / w_low * (1.0 + gamma(4));
        let frame_moved = 2.0 * e_frame / w_min * (1.0 + gamma(2));

        // the ends' directions, ordered
        let first = mapped.first().and_then(|p| p.first()).copied();
        let last = mapped.last().and_then(|p| p.last()).copied();
        let (Some([x0, y0, _, _]), Some([x1, y1, _, _])) = (first, last) else {
            return Err(SolidError::query(OP, QueryFailure::Defect { why: "a curve with an interval of some length has a Bézier piece there, of at least two control points" }));
        };
        let (mut u0, mut u1) = ([x0, y0], [x1, y1]);
        let (turn, e) = cross(u0, u1);
        if !turn.is_finite() {
            return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
        }
        // an infinite `e`: a product underflowed, or the sum overflowed; no bound
        if turn.abs() <= e {
            return Ok(None);
        }
        if turn < 0.0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        let (Some(h0), Some(h1)) = (unit(u0), unit(u1)) else {
            return Ok(None);
        };
        let m = [h0[0] + h1[0], h0[1] + h1[1]];
        let (c1, e1) = cross(m, u1);
        let (c0, e0) = cross(u0, m);
        if !(c1 > e1 && c0 > e0) {
            return Ok(None);
        }
        // the half plane, the height and the overrun past the ends' angles
        let (n0, n1) = (
            (u0[0] * u0[0] + u0[1] * u0[1]).sqrt(),
            (u1[0] * u1[0] + u1[1] * u1[1]).sqrt(),
        );
        let (n0, n1) = (n0 * (1.0 - gamma(3)), n1 * (1.0 - gamma(3)));
        let mut d_z = 0.0_f64;
        let mut over = 0.0_f64;
        for &[x, y, z, w] in mapped.iter().flatten() {
            let (d, e) = dot([x, y], m);
            if d <= e {
                return Ok(None);
            }
            let q = (z / w).abs() * (1.0 + gamma(1));
            d_z = d_z.max(if z != 0.0 && q < FLOOR { FLOOR } else { q });
            let (a, ea) = cross(u1, [x, y]);
            let (b, eb) = cross([x, y], u0);
            if !(ea.is_finite() && eb.is_finite()) {
                return Ok(None);
            }
            let past = ((a + ea) / n1).max((b + eb) / n0);
            if past > 0.0 {
                let o = past / w * (1.0 + gamma(3));
                over = over.max(if o < FLOOR { FLOOR } else { o });
            }
        }
        let (f_abs, f_low) = f_bounds(&mapped, r)?;
        let r2 = r * r;
        // `r² + min F`, rounded down
        let rho2 = if f_low < 0.0 {
            (r2 * (1.0 - gamma(1)) + f_low) * (1.0 - gamma(1))
        } else {
            (r2 + f_low) * (1.0 - gamma(2))
        };
        if rho2.is_nan() || rho2 <= 0.0 {
            return Ok(None);
        }
        let rho_min = rho2.sqrt() * (1.0 - gamma(1));
        let d_rho = f_abs / (rho_min + r) * (1.0 + gamma(2));
        let radial = ((d_rho + over * over / rho_min) * (1.0 + gamma(3))).max(FLOOR);
        let h = (radial * radial + over * over + d_z * d_z).sqrt() * (1.0 + gamma(5));
        if !h.is_finite() {
            return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
        }
        Ok(Some(Self {
            u0,
            u1,
            m,
            h,
            real_moved,
            frame_moved,
        }))
    }
}

/// After the checks: `c` scaled so its largest weight is in `[1, 2)` (module docs,
/// underflow), its Bézier pieces over `[lo, hi]`, and how many knots the refinement and the
/// extraction inserted in all (module docs): the whole decomposition's joined knot vector
/// (each inner knot `p` times) less `c`'s. `None` when the scaling is not exact.
fn checked_pieces(c: &Curve, [lo, hi]: [f64; 2]) -> Result<Option<(Curve, Vec<Curve>, usize)>> {
    let p = c.degree();
    if !((1..=MAX_DEGREE).contains(&p) && c.knots().len() == c.cpts().len() + p + 1) {
        return Err(SolidError::invalid_input(
            OP,
            "a degree of 0 or past the most, or knots that do not fit the points",
        ));
    }
    let finite_w = |w: f64| w == NOW || (w.is_finite() && w > 0.0);
    if !c
        .cpts()
        .iter()
        .all(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && finite_w(q.w))
        || !c.knots().iter().all(|k| k.is_finite())
    {
        return Err(SolidError::invalid_input(
            OP,
            "numbers not finite or a weight not positive",
        ));
    }
    if c.cpts().iter().any(|q| q.z == NOZ) {
        return Err(SolidError::invalid_input(OP, "a 2D curve"));
    }
    let knots = c.knots();
    let flat =
        |run: Option<&[f64]>| run.is_some_and(|r| r.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(p + 1);
    if !flat(knots.get(..=p)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(SolidError::invalid_input(OP, "knots not clamped"));
    }
    // knots that decrease, end knots of multiplicity other than `p + 1`, or an interior
    // knot of multiplicity past `p` (the curve may then break there, and the pieces would
    // not join into it)
    if c.knot_vector().and_then(|kv| kv.validate(p)).is_err() {
        return Err(SolidError::invalid_input(
            OP,
            "knots that decrease, or a knot repeated more than the degree allows",
        ));
    }
    let (Some(&k0), Some(&k1)) = (knots.first(), knots.last()) else {
        return Err(SolidError::invalid_input(OP, "a curve without knots"));
    };
    if !(lo.is_finite() && hi.is_finite() && k0 <= lo && lo < hi && hi <= k1) {
        return Err(SolidError::invalid_input(
            OP,
            "an interval reversed, of no length or not inside its curve's domain",
        ));
    }
    let Some(c) = weights_near_one(c)? else {
        return Ok(None);
    };
    let missing: Vec<f64> = [lo, hi]
        .into_iter()
        .filter(|&u| u > k0 && u < k1 && !knots.contains(&u))
        .collect();
    let refined = if missing.is_empty() {
        c.clone()
    } else {
        c.refine(&missing)?
    };
    let all = refined.decompose_bez()?;
    let joined = all
        .len()
        .saturating_sub(1)
        .saturating_mul(p)
        .saturating_add(2 * (p + 1));
    let inserted = joined.saturating_sub(knots.len());
    let pieces = all
        .into_iter()
        .filter(|x| {
            x.knots().first().is_some_and(|&k| lo <= k)
                && x.knots().last().is_some_and(|&k| k <= hi)
        })
        .collect();
    Ok(Some((c, pieces, inserted)))
}

/// `c` with every homogeneous coordinate multiplied by the power of two that brings its
/// largest weight into `[1, 2)`: the same curve (module docs, underflow); a non-rational curve
/// unchanged. `None` when the scaling is not exact (a coordinate leaving the normal range).
/// The weights are finite and positive (checked).
fn weights_near_one(c: &Curve) -> Result<Option<Curve>> {
    let w_max = c
        .cpts()
        .iter()
        .filter(|q| q.w != NOW)
        .fold(0.0_f64, |m, q| m.max(q.w));
    if w_max == 0.0 {
        return Ok(Some(c.clone()));
    }
    let mut s = 1.0_f64;
    while w_max * s >= 2.0 {
        s *= 0.5;
    }
    while w_max * s < 1.0 {
        s *= 2.0;
    }
    if s == 1.0 {
        return Ok(Some(c.clone()));
    }
    let exact = |v: f64| {
        let t = v * s;
        t.is_finite() && t / s == v
    };
    let mut pw = Vec::new();
    for q in c.cpts() {
        if q.w == NOW || !(exact(q.x) && exact(q.y) && exact(q.z) && exact(q.w)) {
            return Ok(None);
        }
        pw.push(CPoint::new(q.x * s, q.y * s, q.z * s, q.w * s));
    }
    Ok(Some(Curve::new(pw, c.degree(), c.knots().to_vec())?))
}

/// `C(n, k)` for `k = 0..=n` (exact: every value is an integer below `2^53` for
/// `n <= 2 MAX_DEGREE`).
fn binomials(n: usize) -> Vec<f64> {
    let mut row = vec![1.0_f64];
    for _ in 0..n {
        let mut next = Vec::new();
        next.push(1.0);
        next.extend(row.windows(2).map(|w| w.iter().sum::<f64>()));
        next.push(1.0);
        row = next;
    }
    row
}

/// One piece of `N` and `V = W²` being halved: their Bernstein coefficients and its level.
struct Item {
    n: Vec<f64>,
    v: Vec<f64>,
    level: usize,
}

/// The halves of a Bernstein polynomial's coefficients (de Casteljau at 1/2).
fn halve(c: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut row = c.to_vec();
    while let Some(&first) = row.first() {
        left.push(first);
        if let Some(&last) = row.last() {
            right.push(last);
        }
        row = row
            .windows(2)
            .map(|w| w.iter().sum::<f64>() * 0.5)
            .collect();
    }
    right.reverse();
    (left, right)
}

/// Bounds `(max |F|, min F)` of `F = x² + y² - r²` over the mapped
/// pieces, every rounding included (module docs).
fn f_bounds(pieces: &[Vec<[f64; 4]>], r: f64) -> Result<(f64, f64)> {
    let r2 = r * r;
    // each piece's coefficients, the largest magnitude of `N`'s and of `V`'s terms
    let mut stack: Vec<(Item, f64, f64)> = Vec::new();
    let mut seen = 0.0_f64;
    for pts in pieces.iter().rev() {
        let p = pts.len().saturating_sub(1);
        let deg = 2 * p;
        let (bp, b2p) = (binomials(p), binomials(deg));
        let mut n = vec![0.0_f64; deg + 1];
        let mut v = vec![0.0_f64; deg + 1];
        let mut mag = vec![0.0_f64; deg + 1];
        for (i, (&[xi, yi, _, wi], &ci)) in pts.iter().zip(&bp).enumerate() {
            for (j, (&[xj, yj, _, wj], &cj)) in pts.iter().zip(&bp).enumerate() {
                let k = i + j;
                let Some(&ck) = b2p.get(k) else { continue };
                let beta = ci * cj / ck;
                let ww = wi * wj;
                let t = xi * xj + yi * yj - r2 * ww;
                let a = (xi * xj).abs() + (yi * yj).abs() + r2 * ww;
                if let (Some(nk), Some(vk), Some(mk)) = (n.get_mut(k), v.get_mut(k), mag.get_mut(k))
                {
                    *nk += beta * t;
                    *vk += beta * ww;
                    *mk += beta * a;
                }
            }
        }
        if !n.iter().chain(&v).chain(&mag).all(|x| x.is_finite()) {
            return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
        }
        let k0 = 2 * p + 12;
        let s_n = mag.iter().fold(0.0_f64, |m, &x| m.max(x)) * (1.0 + gamma(k0));
        let s_v = v.iter().fold(0.0_f64, |m, &x| m.max(x)) * (1.0 + gamma(k0));
        for (nv, vv) in [(n.first(), v.first()), (n.last(), v.last())] {
            if let (Some(&nv), Some(&vv)) = (nv, vv) {
                seen = seen.max((nv / vv).abs());
            }
        }
        stack.push((Item { n, v, level: 0 }, s_n, s_v));
    }
    let (mut f_abs, mut f_low) = (0.0_f64, f64::INFINITY);
    let mut taken = 0usize;
    while let Some((item, s_n, s_v)) = stack.pop() {
        taken += 1;
        let deg = item.n.len().saturating_sub(1);
        let k = (deg + 12).saturating_add(deg.saturating_mul(item.level));
        let (e_n, e_v) = (gamma(k) * s_n, gamma(k) * s_v);
        // `F = Σ N_k B_k / Σ V_k B_k` with every `V_k > 0`: a weighted mean of the `N_k / V_k`
        let mut upper = 0.0_f64;
        let mut lower = f64::INFINITY;
        for (&nk, &vk) in item.n.iter().zip(&item.v) {
            let den = vk - e_v;
            if den.is_nan() || den <= 0.0 {
                return Err(SolidError::invalid_input(OP, "a weight not positive"));
            }
            upper = upper.max((nk.abs() + e_n) / den * (1.0 + gamma(3)));
            let low = nk - e_n;
            lower = lower.min(if low < 0.0 {
                low / den * (1.0 + gamma(3))
            } else {
                low / (vk + e_v) * (1.0 - gamma(3))
            });
        }
        let n_abs = item.n.iter().fold(0.0_f64, |m, &x| m.max(x.abs()));
        let split = item.level < MAX_LEVEL
            && taken + stack.len() + 2 <= MAX_ITEMS
            && upper > 1.0625 * seen
            && n_abs > 4.0 * e_n;
        if split {
            let (nl, nr) = halve(&item.n);
            let (vl, vr) = halve(&item.v);
            if let (Some(&nv), Some(&vv)) = (nl.last(), vl.last()) {
                seen = seen.max((nv / vv).abs());
            }
            let level = item.level + 1;
            stack.push((
                Item {
                    n: nr,
                    v: vr,
                    level,
                },
                s_n,
                s_v,
            ));
            stack.push((
                Item {
                    n: nl,
                    v: vl,
                    level,
                },
                s_n,
                s_v,
            ));
            continue;
        }
        f_abs = f_abs.max(upper);
        f_low = f_low.min(lower);
    }
    if !(f_abs.is_finite() && f_low.is_finite()) {
        return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
    }
    Ok((f_abs, f_low))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::det;
    use crate::nurbs::knots::Side;
    use std::f64::consts::FRAC_1_SQRT_2;

    /// The quarter circle of radius `r` about `(cx, cy, z)` from angle `a0` (a rational
    /// quadratic Bézier, weights 1, √½, 1).
    fn quarter(r: f64, cx: f64, cy: f64, z: f64, a0: f64) -> Curve {
        let (c, s) = (det::cos(a0), det::sin(a0));
        let at = |x: f64, y: f64| (cx + r * (c * x - s * y), cy + r * (s * x + c * y));
        let (p0, p1, p2) = (at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0));
        let w = FRAC_1_SQRT_2;
        let pw = vec![
            CPoint::new(p0.0, p0.1, z, 1.0),
            CPoint::new(p1.0 * w, p1.1 * w, z * w, w),
            CPoint::new(p2.0, p2.1, z, 1.0),
        ];
        Curve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
    }

    fn circle(r: f64, cx: f64, cy: f64, z: f64) -> RefCircle {
        RefCircle {
            center: Vec3::new(cx, cy, z),
            x_axis: Vec3::new(1.0, 0.0, 0.0),
            y_axis: Vec3::new(0.0, 1.0, 0.0),
            radius: r,
        }
    }

    /// A non-rational cubic B-spline through `n + 1` points of the quarter circle of radius
    /// `r` about the origin, raised by `dz` (interpolating each span's ends and tangents:
    /// the Hermite form of each span, which keeps it within `r (1 - cos(h/2))`-scale of the
    /// circle).
    fn cubic_along(r: f64, n: u32, dz: f64) -> Curve {
        let h = std::f64::consts::FRAC_PI_2 / f64::from(n);
        let k = 4.0 / 3.0 * det::tan(h / 4.0);
        let mut pw = Vec::new();
        let mut knots = vec![0.0; 4];
        for i in 0..n {
            let (a, b) = (h * f64::from(i), h * f64::from(i + 1));
            let p = |t: f64| (r * det::cos(t), r * det::sin(t));
            let d = |t: f64| (-r * det::sin(t), r * det::cos(t));
            let (pa, pb, da, db) = (p(a), p(b), d(a), d(b));
            if i == 0 {
                pw.push(CPoint::new(pa.0, pa.1, dz, NOW));
            }
            pw.push(CPoint::new(pa.0 + k * da.0, pa.1 + k * da.1, dz, NOW));
            pw.push(CPoint::new(pb.0 - k * db.0, pb.1 - k * db.1, dz, NOW));
            pw.push(CPoint::new(pb.0, pb.1, dz, NOW));
            if i + 1 < n {
                knots.extend([b / h, b / h, b / h]);
            }
        }
        knots.extend([f64::from(n); 4]);
        Curve::new(pw, 3, knots).unwrap()
    }

    /// The refinement's rounding allowance of two quarters about `(4, 248, 7)` of radius
    /// 1.15 (module docs): per curve `2 e (1 + P) / W̃` with `e = 2 γ_12` of the largest
    /// homogeneous coordinate (below 250), `P` below 250 and `W̃ = √½`; then 1% and 1e-11
    /// for the remaining few-`γ` steps on numbers near 1.
    fn allowance() -> f64 {
        let e = 2.0 * gamma(12) * 250.0;
        2.0 * (2.0 * e * 251.0 / FRAC_1_SQRT_2) * 1.01 + 1e-11
    }

    #[test]
    fn an_arc_is_no_distance_from_itself() {
        let a = quarter(1.15, 4.0, 248.0, 7.0, 0.5);
        let c = circle(1.15, 4.0, 248.0, 7.0);
        let d = hausdorff_about_circle(&a, [0.0, 1.0], &a, [0.0, 1.0], &c)
            .unwrap()
            .unwrap();
        assert!(d <= allowance(), "{d} vs {}", allowance());
    }

    /// Two quarter arcs of radii `r` and `r + δ` about one center, the second also raised
    /// by `ε`: every point of each is `√(δ² + ε²)` from the other (closed form), and the bound
    /// is no less and only rounding more.
    #[test]
    fn concentric_arcs_are_bounded_by_their_closed_form_distance() {
        for (dr, dz) in [(1e-5, 0.0), (0.0, 1e-5), (3e-6, 4e-6), (1e-3, 0.0)] {
            let a = quarter(1.15, 4.0, 248.0, 7.0, 0.0);
            let b = quarter(1.15 + dr, 4.0, 248.0, 7.0 + dz, 0.0);
            let want = (dr * dr + dz * dz).sqrt();
            let c = circle(1.15, 4.0, 248.0, 7.0);
            let d = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c)
                .unwrap()
                .unwrap();
            let allow = allowance();
            assert!(
                d >= want && d <= want * (1.0 + 1e-9) + allow,
                "{d} vs {want}"
            );
            let d = hausdorff_about_circle(&b, [0.0, 1.0], &a, [0.0, 1.0], &c)
                .unwrap()
                .unwrap();
            assert!(
                d >= want && d <= want * (1.0 + 1e-9) + allow,
                "{d} vs {want}"
            );
        }
    }

    /// One arc and the same arc rotated by `θ` about its center: the Hausdorff distance is the
    /// chord `2 r sin(θ / 2)` between their ends (closed form); either orientation.
    #[test]
    fn arcs_of_one_circle_differing_in_extent_are_bounded_by_their_end_chord() {
        let r = 1.15;
        let c = circle(r, 0.0, 0.0, 0.0);
        for theta in [1e-5, 1e-3, 0.1] {
            let a = quarter(r, 0.0, 0.0, 0.0, 0.0);
            let b = quarter(r, 0.0, 0.0, 0.0, theta);
            let want = 2.0 * r * det::sin(theta / 2.0);
            let d = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c)
                .unwrap()
                .unwrap();
            assert!(
                d >= want && d <= want * (1.0 + 1e-6) + 1e-11,
                "{d} vs {want}"
            );
            let rev = b.reversed().unwrap();
            let d = hausdorff_about_circle(&a, [0.0, 1.0], &rev, [0.0, 1.0], &c)
                .unwrap()
                .unwrap();
            assert!(
                d >= want && d <= want * (1.0 + 1e-6) + 1e-11,
                "{d} vs {want}"
            );
        }
    }

    /// An arc against a polynomial cubic along it, of another parameterisation: the bound
    /// holds the curve's sampled distance from the circle (a lower bound of the Hausdorff
    /// distance), and is within twice the cubic's sampled radial error plus its rise.
    #[test]
    fn an_arc_and_a_cubic_along_it_are_bounded() {
        let r = 1.15;
        let c = circle(r, 0.0, 0.0, 0.0);
        let a = quarter(r, 0.0, 0.0, 0.0, 0.0);
        for (n, dz) in [(1, 0.0), (4, 0.0), (8, 2e-6)] {
            let b = cubic_along(r, n, dz);
            let hi = f64::from(n);
            let mut sampled = 0.0_f64;
            for i in 0..=4000 {
                let p = b.eval(hi * f64::from(i) / 4000.0, Side::Left).unwrap();
                let rho = (p.x * p.x + p.y * p.y).sqrt();
                sampled = sampled.max((det::powi(rho - r, 2) + p.z * p.z).sqrt());
            }
            let d = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, hi], &c)
                .unwrap()
                .unwrap();
            assert!(d >= sampled, "{d} < {sampled}");
            assert!(d <= 2.0 * sampled + 1e-10, "{d} vs {sampled}");
        }
    }

    #[test]
    fn a_half_turn_or_a_curve_off_the_circle_has_no_bound() {
        let r = 1.0;
        let c = circle(r, 0.0, 0.0, 0.0);
        let a = quarter(r, 0.0, 0.0, 0.0, 0.0);
        // the opposite quarter: its ends are not on the first's side
        let b = quarter(r, 0.0, 0.0, 0.0, std::f64::consts::PI);
        assert!(hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c)
            .unwrap()
            .is_none());
        // a sub-interval of no length, and one outside the domain
        assert!(hausdorff_about_circle(&a, [0.5, 0.5], &a, [0.0, 1.0], &c).is_err());
        assert!(hausdorff_about_circle(&a, [0.0, 1.5], &a, [0.0, 1.0], &c).is_err());
    }

    /// A degree-1 curve whose interior knot is repeated past its degree breaks there: its two
    /// segments do not join, so pieces bounded one by one bound no connected curve; refused.
    #[test]
    fn an_interior_knot_past_the_degree_is_refused() {
        let c = circle(1.0, 0.0, 0.0, 0.0);
        let a = quarter(1.0, 0.0, 0.0, 0.0, 0.0);
        let pw = vec![
            CPoint::new(1.0, 0.0, 0.0, NOW),
            CPoint::new(0.9, 0.1, 0.0, NOW),
            CPoint::new(0.1, 0.9, 0.0, NOW),
            CPoint::new(0.0, 1.0, 0.0, NOW),
        ];
        let b = Curve::new(pw, 1, vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0]).unwrap();
        let got = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c);
        assert!(
            matches!(got, Err(SolidError::InvalidInput { .. })),
            "{got:?}"
        );
    }

    /// A circle of a radius below [`R_MIN`] has no bound (an answer, not a failure), even
    /// for an arc against itself.
    #[test]
    fn a_radius_below_the_range_has_no_bound() {
        let r = R_MIN / 10.0;
        let c = circle(r, 0.0, 0.0, 0.0);
        let a = quarter(r, 0.0, 0.0, 0.0, 0.0);
        let got = hausdorff_about_circle(&a, [0.0, 1.0], &a, [0.0, 1.0], &c).unwrap();
        assert_eq!(got, None);
    }

    /// The arc raised by `10^-200` against the arc in the circle's plane: their Hausdorff
    /// distance is `10^-200` (closed form), whose square underflows. The bound is still
    /// given, at least that distance and at most the rounding scale of a unit arc.
    #[test]
    fn an_offset_whose_square_underflows_is_still_bounded() {
        let c = circle(1.0, 0.0, 0.0, 0.0);
        let a = quarter(1.0, 0.0, 0.0, 1e-200, 0.0);
        let b = quarter(1.0, 0.0, 0.0, 0.0, 0.0);
        let d = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c)
            .unwrap()
            .unwrap();
        assert!((1e-200..1e-11).contains(&d), "{d}");
    }

    /// Two quadratic Béziers with every weight `w`: ends `(1, ∓1/4)` and middle control points
    /// `(1, 0)` and `(33/32, 0)`, so `b`'s midpoint `(1 + 1/64, 0)` is `1/64` from `a` (the
    /// segment `x = 1`; closed form), and the bound is at least that. Scaling every weight
    /// by one factor leaves the curves, and the bound, as they are: at `w = 2^-536` the
    /// products of weights would round in the subnormal range unscaled.
    #[test]
    fn a_bound_does_not_depend_on_the_weights_scale() {
        let c = circle(1.0, 0.0, 0.0, 0.0);
        let bez = |mid: f64, w: f64| {
            let pw = [(1.0, -0.25), (mid, 0.0), (1.0, 0.25)]
                .iter()
                .map(|&(x, y)| CPoint::new(x * w, y * w, 0.0, w))
                .collect();
            Curve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap()
        };
        let tiny = det::powi(2.0_f64, -536);
        let mut got = Vec::new();
        for w in [1.0, tiny, 4.0] {
            let (a, b) = (bez(1.0, w), bez(33.0 / 32.0, w));
            let d = hausdorff_about_circle(&a, [0.0, 1.0], &b, [0.0, 1.0], &c)
                .unwrap()
                .unwrap();
            assert!(d >= 1.0 / 64.0, "w = {w}: {d}");
            got.push(d);
        }
        assert!(got.windows(2).all(|p| p[0] == p[1]), "{got:?}");
    }

    /// Sub-intervals: the arc over `[0, 1/2]` against itself over `[0, 1/2]` and over
    /// `[0, 1]`; the second differs by the second half's extent, the chord from the midpoint
    /// to the end (closed form `2 r sin(π/8)`, the quarter's half being an eighth turn).
    #[test]
    fn sub_intervals_are_bounded() {
        let r = 1.15;
        let c = circle(r, 0.0, 0.0, 0.0);
        let a = quarter(r, 0.0, 0.0, 0.0, 0.0);
        let d = hausdorff_about_circle(&a, [0.0, 0.5], &a, [0.0, 0.5], &c)
            .unwrap()
            .unwrap();
        assert!(d < 1e-11, "{d}");
        let want = 2.0 * r * det::sin(std::f64::consts::PI / 8.0);
        let d = hausdorff_about_circle(&a, [0.0, 0.5], &a, [0.0, 1.0], &c)
            .unwrap()
            .unwrap();
        assert!(d >= want && d <= want * (1.0 + 1e-9), "{d} vs {want}");
    }
}
