//! Not upstream (#20643): a proved bound on the distance from a cone's (or cylinder's) net to
//! the cone as it evaluates its point, at the same parameters, over the whole net.
//!
//! A cone evaluates its point by its STEP formula
//!
//! `S = O + r (cos x° X + sin x° Y) + v Z`, `r = R0 + t v` (`t` the tangent of its semi-angle),
//!
//! at the STEP parameters `(x, v)` it converts from the net's: the angle `x` from the sweep
//! parameter `σ` through its polar circle (`ConvertToPolarParameter`: an end of the circle's
//! NURB snaps to an end of its angular domain, other parameters are evaluated on the circle's
//! NURB and inverted by `atan2`, wrapped into that domain, snapped to its start at the seam
//! and moved by a turn at the ends of the angle table), the height `v` an affine map (or `±`)
//! of the linear parameter `λ`, and both clamped into the cone's domain while out-of-bounds
//! evaluation is off. The net's point is `N = Σ_j B_j(λ) Nw_j(σ) / Σ_j B_j(λ) W_j(σ)` from its
//! stored homogeneous control points (rows `j` along `σ`).
//!
//! With `F = [X Y Z]` the stored frame, `g ≥ ‖FᵀF − I‖` and `E` the formula above evaluated
//! exactly at the evaluation's `(x, v)` (and the stored `R0`, `t`):
//!
//! `|S − N| ≤ |S − E| + √(1+g) (T3 + (|t| + 1) |v − h_N| + |R(v)| min(Δα, 2)) + g |N − O|`,
//!
//! from `N − E = F (Fᵀ(N − O) − (R cos x°, R sin x°, v)) + (I − FFᵀ)(N − O)`:
//!
//! - `|S − E|` the rounding of the formula (each operation's, the radius's and the libm
//!   `cos`/`sin` of the rounded `x π / 180`, within 2 ulps);
//! - `h_N = Σ B_j zz_j / W` the net's height (`zz_j = Z·(Nw_j − O W_j)`): `|v − h_N|` is at
//!   most the map's rounding, the clamp's reach and `|G| / W`, `G = Σ_j B_j (h(λ) W_j − zz_j)`
//!   a polynomial in `(σ, λ)` formed before it is bounded (its near cancellation in its
//!   coefficients);
//! - `T3 = |q − R(h_N) â|`, `q` the net's point in the frame's `(X, Y)` and `â` the unit
//!   direction of the polar circle's exact point `a = ((P − C)·Xc, f (P − C)·Yc)` (`f` the
//!   stored `rx / ry`), whose angle `θ` the inversion computes: its component across `â` is
//!   `Σ B_j cross(a, b_j) / (|a| W)` and along it `Σ B_j (dot(a, b_j) − |a| c_j) / (|a| W)`
//!   (`b_j` the row's `(X, Y)` coordinates, `c_j = R0 W_j + t zz_j`), each bounded by the
//!   largest row's (a mediant: the `B_j` are non-negative and sum to one over the net), the
//!   along part through `dot² − |a|² c²` (a polynomial, so its cancellation is kept);
//! - `Δα` the angle between `â` and the evaluation's `x`: the rounding of the polar point's
//!   evaluation and of the inversion (`A2`, `A3`), the rounding of the wraps and turns
//!   (`A4`), and where the conversion may output a constant instead (a snap, the seam, a
//!   clamp), the farthest that constant is from `θ` over the piece. The atan2 output, made
//!   positive, is followed through each branch of the wrap (kept within `EFF_ZERO_DEG` of the
//!   domain, else wrapped by a turn: into it, down from its end, or left outside), the seam
//!   snap and the two table turns (each with its own end, angle and sign, as the circle is
//!   inside out or not), so only constants the conversion can reach from `θ` are counted.
//!   The seam snap (`Ellipse::step_inversion` on a full turn) needs the rounded polar point
//!   within `τ = zone_tol / 100` of the end point, which bounds its angle off `â` by
//!   `asin(D / rx) ≤ (π/3) D / rx` (`asin x ≤ (π/3) x` on `[0, 1/2]`), `D = L (τ + δP + δE) +
//!   |d|` (`seam_reach`, `L` the operator norm of the map to the circle's frame): a full turn's
//!   net is up to about `(π/3) τ R / rx` (1.05e-7 for a round cylinder at the default zone
//!   tolerance; 9e-8 measured 1e-7 before the end) off the surface it evaluates, a real tear
//!   the bound counts.
//!
//! Every quantity is enclosed in outward-rounded interval arithmetic from the stored data;
//! the polynomials are Taylor expansions about each piece's middle
//! ([`super::polynomial_pieces`]). The sweep direction is cut at the net's and the circle's
//! knots and at the snap windows, and bisected where the angle's hull is not proved or a
//! constant the conversion may output is far from `θ`; a piece left unproved at the caps
//! takes the loose bound `|N − O| + √(1+g)(|r| + |v|) + |S − E|`.
//!
//! | Rust                 | Upstream |
//! |----------------------|----------|
//! | [`cone_net_residual`] | none     |
//!
//! Deviations: none (new code). The bisection is bounded by [`MAX_DEPTH`] and
//! [`MAX_PIECES`]. The libm `atan2`, `atan`, `cos` and `sin` are taken within 2 ulps
//! (ARGUED, as the evaluation's own accuracy), the polar circle's evaluation within
//! `3 γ(8p + 8) max |P_i|` per coordinate (ARGUED: a homogeneous de Boor sum of non-negative
//! basis values, then one division).

use super::polynomial_pieces::{
    add_scaled, curve_taylor, inverse_factorial, magnitude, mul, range, stored_nurbs, zero, Poly,
};
use super::OP;
use crate::blend::fillet_section::certificate::NurbsCurve;
use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_section::station::{ders_basis, knot, span, Nurbs};
use crate::curve::ellipse::Ellipse;
use crate::geom::tol::{context_zone_tol_3d, SmallTopoType};
use crate::kernel::Kernel;
use crate::math::{Vec3, EFF_ZERO, EFF_ZERO_DEG};
use crate::surface::cone::Cone;
use crate::Result;

/// The deepest bisection of the sweep direction.
const MAX_DEPTH: u32 = 60;

/// The most pieces the sweep direction is cut into.
const MAX_PIECES: usize = 4096;

/// The unit roundoff.
const U: f64 = f64::EPSILON / 2.0;

/// `γ_n = n u / (1 − n u)`, rounded up.
fn gamma(n: f64) -> f64 {
    let nu = Ivl::from(n) * Ivl::from(U);
    (nu / (Ivl::from(1.0) - nu)).hi
}

/// `π`, enclosed (`PI` is the double just below it).
fn pi() -> Ivl {
    Ivl::new(std::f64::consts::PI, std::f64::consts::PI + 4.5e-16)
}

/// The three coordinates of `v`, each held exactly.
fn ivl3(v: Vec3) -> [Ivl; 3] {
    [Ivl::from(v.x), Ivl::from(v.y), Ivl::from(v.z)]
}

/// `a · b`, enclosed.
fn dot3(a: &[Ivl; 3], b: &[Ivl; 3]) -> Ivl {
    a.iter().zip(b).fold(zero(), |acc, (&x, &y)| acc + x * y)
}

/// The circular distance of `d` degrees to a whole number of turns, in `[0, 180]`.
fn circular(d: f64) -> f64 {
    let r = d.rem_euclid(360.0);
    r.min(360.0 - r)
}

/// The rounding allowance of the degree arithmetic below on values of size `s`.
fn slack(s: f64) -> f64 {
    16.0 * U * (s.abs() + 720.0)
}

/// The farthest the angle `c` is, modulo 360, from an angle in `[lo, hi]` (degrees), rounded
/// up.
fn farthest(c: f64, [lo, hi]: [f64; 2]) -> f64 {
    let s = slack(c.abs() + lo.abs() + hi.abs());
    let short = hi - lo < 360.0 - s;
    if !short {
        return 180.0;
    }
    // (the antipode of `c` in the hull: 180 apart)
    let antipode = c + 180.0;
    let k = ((lo - antipode) / 360.0).ceil();
    if antipode + 360.0 * k <= hi + s {
        return 180.0;
    }
    (circular(c - lo).max(circular(c - hi)) + s).min(180.0)
}

/// The nearest the angle `c` is, modulo 360, to an angle in `[lo, hi]` (degrees), rounded
/// down.
fn nearest(c: f64, [lo, hi]: [f64; 2]) -> f64 {
    let s = slack(c.abs() + lo.abs() + hi.abs());
    if hi - lo >= 360.0 - s {
        return 0.0;
    }
    let r = (c - lo).rem_euclid(360.0);
    let d = if r <= hi - lo {
        0.0
    } else {
        (r - (hi - lo)).min(360.0 - r)
    };
    (d - s).max(0.0)
}

/// The angles in `[lo, hi]` modulo 360 that lie in `[jlo, jhi]`, conservatively: one hull
/// per turn, appended to `out` (all of `[jlo, jhi]` for a range of a turn or more).
fn representatives([lo, hi]: [f64; 2], [jlo, jhi]: [f64; 2], out: &mut Vec<[f64; 2]>) {
    let ordered = jlo <= jhi;
    if !ordered {
        return;
    }
    let s = slack(lo.abs() + hi.abs() + jlo.abs() + jhi.abs());
    // NaN takes the hull
    let short = hi - lo < 360.0 - s;
    if !short {
        out.push([jlo, jhi]);
        return;
    }
    let k0 = ((jlo - hi) / 360.0).floor() - 1.0;
    let k1 = ((jhi - lo) / 360.0).ceil() + 1.0;
    // NaN takes the hull
    let few = k1 - k0 <= 8.0;
    if !few {
        out.push([jlo, jhi]);
        return;
    }
    let mut k = k0;
    while k <= k1 {
        let (a, b) = (lo + 360.0 * k - s, hi + 360.0 * k + s);
        let (a, b) = (a.max(jlo), b.min(jhi));
        if a <= b {
            out.push([a, b]);
        }
        k += 1.0;
    }
}

/// How the cone maps its linear parameter to its height.
#[derive(Clone, Copy)]
enum HeightMap {
    /// `h = s0 + (λ − n0) / l · ls` (fp constants), rounded at each operation.
    Affine { s0: f64, n0: f64, l: f64, ls: f64 },
    /// `h = ±λ`, exact.
    Sign(f64),
}

impl HeightMap {
    /// The heights over `λ` in `lam`, enclosed.
    fn at(self, lam: Ivl) -> Ivl {
        match self {
            Self::Affine { s0, n0, l, ls } => {
                Ivl::from(s0) + (lam - Ivl::from(n0)) / Ivl::from(l) * Ivl::from(ls)
            }
            Self::Sign(s) => lam * Ivl::from(s),
        }
    }

    /// `dh / dλ`, enclosed.
    fn slope(self) -> Ivl {
        match self {
            Self::Affine { l, ls, .. } => Ivl::from(ls) / Ivl::from(l),
            Self::Sign(s) => Ivl::from(s),
        }
    }

    /// The evaluation's rounding of the heights `h` (`γ4 (|s0| + |h − s0|)`).
    fn rounding(self, h: Ivl) -> f64 {
        match self {
            Self::Affine { s0, .. } => {
                let off = (h - Ivl::from(s0)).mag();
                (Ivl::from(gamma(4.0)) * (Ivl::from(s0.abs()) + Ivl::from(off))).hi
            }
            Self::Sign(_) => 0.0,
        }
    }
}

/// One row of the net along the sweep direction, at a piece: its Taylor polynomials and the
/// bounds the row gives.
struct Row {
    /// The row's weight `W_j`.
    w: Poly,
    /// `zz_j = Z · (Nw_j − O W_j)`.
    zz: Poly,
    /// A lower bound of `W_j` over the piece.
    w_lo: f64,
    /// `(|cross(a, b_j)| + e_j) / (|a| W_j)` over the piece, rounded up.
    t3: f64,
    /// `|Nw_j − O W_j| / W_j` over the piece, rounded up.
    to_origin: f64,
}

/// What one piece of the sweep direction gives.
enum Piece {
    /// A bound over the piece.
    Bound(f64),
    /// Cut it in two.
    Split,
    /// No proof over it: the loose bound.
    Loose,
}

/// The data of the bound, taken once from the cone and the net.
struct Setup<'a> {
    o: [Ivl; 3],
    x: [Ivl; 3],
    y: [Ivl; 3],
    z: [Ivl; 3],
    r0: f64,
    t: f64,
    map: HeightMap,
    /// The cone's angle and height domains, where it clamps into them.
    clamp: Option<([f64; 2], [f64; 2])>,
    /// `√(1 + g)` and `g`.
    frame: (f64, f64),
    /// The net's rows along the sweep direction (one per linear index).
    rows: Vec<NurbsCurve>,
    /// The linear direction's knots, degree and count.
    lam: (&'a [f64], usize, usize),
    /// The polar circle's NURB, centre, axes and `f`.
    polar: NurbsCurve,
    c: [Ivl; 3],
    xc: [Ivl; 3],
    yc: [Ivl; 3],
    f: f64,
    /// The circle NURB's natural interval.
    nat: [f64; 2],
    /// The circle's angular domain `[a0, a1]`, its length, and the snap angles of the
    /// interval's start and end.
    angular: [f64; 2],
    length: f64,
    snaps: [f64; 2],
    /// The farthest (degrees) the polar point's exact direction is from the domain end's
    /// where the inversion snaps to the seam ([`seam_reach`]), when proved.
    seam_reach: Option<f64>,
    /// The ends of the table's knots and angles, and its scaled zero.
    knot_ends: [f64; 2],
    /// The table's turns: near the knot, an angle within `EFF_ZERO_DEG` of the table's angle
    /// is moved by the shift (`ConvertToPolarParameter`'s `move_to_min` / `move_to_max`).
    turns: [(f64, f64, f64); 2],
    scaled_zero: f64,
    /// The rounding of the wraps and turns (degrees), and the largest angle magnitude.
    a4: f64,
    big: f64,
    /// The loose bound.
    loose: f64,
}

/// The `j`-th basis of the span `k` (degree `p`) as a polynomial about `m`.
fn basis_polys(knots: &[f64], k: usize, p: usize, m: f64) -> Vec<Poly> {
    let ders = ders_basis(knots, k, p, Ivl::from(m), p);
    (0..=p)
        .map(|i| {
            (0..=p)
                .map(|d| {
                    ders.get(d)
                        .and_then(|r| r.get(i))
                        .copied()
                        .unwrap_or_else(zero)
                        * inverse_factorial(d)
                })
                .collect()
        })
        .collect()
}

/// `Σ_k e_k c_k` of three polynomials.
fn combine(c: &[Poly], e: &[Ivl; 3]) -> Poly {
    let mut out = Vec::new();
    for (ck, &ek) in c.iter().zip(e) {
        add_scaled(&mut out, ck, ek);
    }
    out
}

/// `Σ|c_j| ρ^j`, every power counted.
fn size(c: &[Ivl], rho: f64) -> f64 {
    magnitude(c, rho, 0)
}

impl Setup<'_> {
    /// The rounding of the STEP formula at angles of size at most `big` degrees and heights
    /// of size at most `v`: `|S − E|`, rounded up.
    fn evaluation(&self, big: f64, v: f64) -> f64 {
        let g8 = Ivl::from(gamma(8.0));
        let g3 = Ivl::from(gamma(3.0));
        let (r0, t, v) = (
            Ivl::from(self.r0.abs()),
            Ivl::from(self.t.abs()),
            Ivl::from(v),
        );
        let radius = (r0 + t * v) * (Ivl::from(1.0) + g3);
        let trig = Ivl::from(big) * pi() / Ivl::from(180.0) * Ivl::from(4.04 * U)
            + Ivl::from(4.0 * U)
            + Ivl::from(1.0e-300);
        let mut sum = zero();
        for k in 0..3 {
            let get = |a: &[Ivl; 3]| Ivl::from(a.get(k).copied().unwrap_or_else(zero).mag());
            let (ok, xy, zk) = (get(&self.o), get(&self.x) + get(&self.y), get(&self.z));
            sum = sum
                + g8 * (ok + radius * xy + v * zk)
                + g3 * (r0 + t * v) * xy
                + radius * trig * xy * (Ivl::from(1.0) + g8);
        }
        if sum.is_finite() {
            sum.hi
        } else {
            f64::INFINITY
        }
    }

    /// The angle hull, angle error and the circle's `|a|` at a piece: `None` where the piece
    /// must be cut (the hull or the evaluation's accuracy not proved), else `(θ hull in
    /// degrees, |a|_lo, |a|_hi, a's polynomials, the inversion's error in degrees)`.
    #[allow(clippy::type_complexity)]
    fn angle(&self, kp: usize, m: f64, rho: f64) -> Option<([f64; 2], f64, f64, [Poly; 2], f64)> {
        let cp = &self.polar;
        let tp = curve_taylor(cp, kp, m);
        let [px, py, pz, wp] = &tp;
        let rel: Vec<Poly> = [px, py, pz]
            .iter()
            .zip(&self.c)
            .map(|(pk, &ck)| {
                let mut d = (*pk).clone();
                add_scaled(&mut d, wp, Ivl::from(0.0) - ck);
                d
            })
            .collect();
        let ax = combine(&rel, &self.xc);
        let mut ay = Vec::new();
        add_scaled(&mut ay, &combine(&rel, &self.yc), Ivl::from(self.f));
        let w = range(wp, rho);
        if !(w.lo > 0.0 && w.is_finite()) {
            return None;
        }
        // the hull of the direction about the middle's
        let reference = [range(&ax, 0.0).mid(), range(&ay, 0.0).mid()];
        let [rx, ry] = reference;
        if !(rx.is_finite() && ry.is_finite()) || (rx == 0.0 && ry == 0.0) {
            return None;
        }
        let mut cr = Vec::new();
        add_scaled(&mut cr, &ay, Ivl::from(rx));
        add_scaled(&mut cr, &ax, Ivl::from(-ry));
        let mut dt = Vec::new();
        add_scaled(&mut dt, &ax, Ivl::from(rx));
        add_scaled(&mut dt, &ay, Ivl::from(ry));
        let (cr, dt) = (range(&cr, rho), range(&dt, rho));
        if !(dt.lo > 0.0 && cr.is_finite() && dt.is_finite()) {
            return None;
        }
        let q = cr / dt;
        if !q.is_finite() {
            return None;
        }
        // (libm atan within 2 ulps; atan2 likewise)
        let widen = |x: f64| 4.5e-16 * x.abs() + 1.0e-300;
        let (plo, phi) = (q.lo.atan(), q.hi.atan());
        let phi_ivl = Ivl::new(plo - widen(plo), phi + widen(phi));
        let base = ry.atan2(rx);
        let theta = (Ivl::new(base - 9.0e-16, base + 9.0e-16) + phi_ivl) * Ivl::from(180.0) / pi();
        let norm_ref = (Ivl::from(rx) * Ivl::from(rx) + Ivl::from(ry) * Ivl::from(ry)).sqrt();
        let a_lo = (dt / norm_ref).lo;
        let a_hi = (range(&mul(&ax, &ax), rho) + range(&mul(&ay, &ay), rho))
            .sqrt()
            .hi;
        if !(a_lo > 0.0 && theta.is_finite() && a_hi.is_finite()) {
            return None;
        }
        // A2: the evaluated point's error, from the span's control points
        let p = cp.p;
        let gp = Ivl::from(3.0 * gamma(8.0 * p as f64 + 8.0));
        let mut xk = [0.0_f64; 3];
        for i in 0..=p {
            let base = 4 * (kp + i).checked_sub(p)?;
            let w = Ivl::from(*cp.pw.get(base + 3)?);
            for (k, slot) in xk.iter_mut().enumerate() {
                let c = Ivl::from(*cp.pw.get(base + k)?) / w;
                *slot = slot.max(c.mag());
            }
        }
        let u = Ivl::from(U);
        let g3 = Ivl::from(gamma(3.0));
        let (mut e_dx, mut e_dy, mut dy_size) = (zero(), zero(), zero());
        for (k, &x) in xk.iter().enumerate() {
            let eps = gp * Ivl::from(x);
            let ck = Ivl::from(self.c.get(k)?.mag());
            let size = Ivl::from(x) + eps + ck;
            let e_vec = eps + u * size;
            let (xa, ya) = (
                Ivl::from(self.xc.get(k)?.mag()),
                Ivl::from(self.yc.get(k)?.mag()),
            );
            e_dx = e_dx + e_vec * xa + g3 * size * xa;
            e_dy = e_dy + e_vec * ya + g3 * size * ya;
            dy_size = dy_size + size * ya;
        }
        let f = Ivl::from(self.f.abs());
        let e_y = f * e_dy + u * f * (dy_size + e_dy);
        let delta = (e_dx + e_y) * Ivl::from(1.01);
        // |a| Euclidean: the homogeneous one over the weight
        let a_euclid = Ivl::from(a_lo) / Ivl::from(w.hi);
        if !(delta.is_finite() && delta.hi <= 0.5 * a_euclid.lo) {
            return None;
        }
        // asin(Δ/|a|) ≤ (π/3) Δ/|a| radians (Δ ≤ |a|/2), in degrees 60 Δ/|a|; then A3 for
        // atan2, the turn and degrees
        let a2 = (Ivl::from(60.0) * delta / a_euclid * Ivl::from(1.03)).hi;
        let inversion = a2 + 3.0e-13;
        Some(([theta.lo, theta.hi], a_lo, a_hi, [ax, ay], inversion))
    }

    /// Whether `[s0, s1]` meets the window `|σ − c| ≤ w`.
    fn meets([s0, s1]: [f64; 2], c: f64, w: f64) -> bool {
        s0 <= c + w && s1 >= c - w
    }

    /// The half-width of the end-snap window about `c` (inflated past rounding).
    fn snap_window(c: f64) -> f64 {
        EFF_ZERO + 1.0e-15 * (1.0 + c.abs())
    }

    /// The half-width of the table's turn window about `c`.
    fn turn_window(&self, c: f64) -> f64 {
        self.scaled_zero.abs() * (1.0 + 8.0 * U) + 8.0 * U * c.abs() + 1.0e-300
    }

    /// The window edges of the sweep direction: the pieces are cut there.
    fn window_cuts(&self) -> Vec<f64> {
        let mut out = Vec::new();
        // the end-snap windows cut at twice their half-width: far past the rounding of
        // `c ± w` (an ulp of `c`, under `1e-15 (1 + |c|) ≤ w`), so the piece next to a window
        // does not meet it (the window, `1e-12` wide, is under the bisection's floor, and a
        // neighbour meeting it would take the snap's constant over its whole width)
        for c in self.nat {
            let w = 2.0 * Self::snap_window(c);
            out.extend([c - w, c + w]);
        }
        for c in self.knot_ends {
            let w = self.turn_window(c) * (1.0 + 1.0e-6) + 1.0e-300;
            out.extend([c - w, c + w]);
        }
        out
    }

    /// The constants the conversion may output over the piece instead of the inverted angle,
    /// and the largest angle magnitude it may give: `(farthest constant from θ, angles' size,
    /// whether cutting the piece can narrow the farthest)`.
    fn events(&self, piece: [f64; 2], theta: [f64; 2], inversion: f64) -> (f64, f64, bool) {
        let z = EFF_ZERO_DEG * (1.0 + 1.0e-9) + self.a4;
        let [a0, a1] = self.angular;
        let e = self.a4;
        let err = inversion + e;
        let psi = [theta[0] - err, theta[1] + err];
        // the inverted angles: hulls of values congruent to an angle within `err` of `θ`
        // modulo 360 (`atan2` made positive: `[0, 360]` up to its rounding; kept where
        // within `EFF_ZERO_DEG` of the domain, else wrapped into `[a0, a0 + 360]` or a turn
        // below)
        let mut xs: Vec<[f64; 2]> = Vec::new();
        // (`Extent1d::contains_periodic_value`: an angle within `EFF_ZERO_DEG` of the domain
        // is output as it is; any other is wrapped into `[a0, a0 + 360)`, then output there
        // when within `EFF_ZERO_DEG` of the domain, or a turn down from near `a0 + 360`, into
        // `[a0 − z, a1 + z]`, or else as wrapped, outside the domain)
        let mut vs = Vec::new();
        representatives(psi, [-e, 360.0 + e], &mut vs);
        let outside = [
            a1 + EFF_ZERO_DEG - 2.0 * e,
            a0 + 360.0 - EFF_ZERO_DEG + 2.0 * e,
        ];
        for [lo, hi] in vs {
            let (ilo, ihi) = (lo.max(a0 - z), hi.min(a1 + z));
            if ilo <= ihi {
                xs.push([ilo, ihi]);
            }
            let below = [lo, hi.min(a0 - EFF_ZERO_DEG + e)];
            let above = [lo.max(a1 + EFF_ZERO_DEG - e), hi];
            for part in [below, above] {
                if part[0] <= part[1] {
                    representatives(part, [a0 - z, a1 + z], &mut xs);
                    representatives(part, outside, &mut xs);
                }
            }
        }
        // the constants the conversion outputs exactly: the end snaps
        let mut constants: Vec<f64> = Vec::new();
        let [start, end] = self.nat;
        if Self::meets(piece, start, Self::snap_window(start)) {
            constants.push(self.snaps[0]);
        }
        if Self::meets(piece, end, Self::snap_window(end)) {
            constants.push(self.snaps[1]);
        }
        // the seam: the snap needs the evaluated point within the approximation tolerance of
        // the domain end's, so its output `a0` is at most the turn's gap `a0 − a1` and the
        // end's reach from `â`
        let mut seam = (0.0_f64, false);
        let mut seam_out = None;
        if (self.length - 360.0).abs() <= z && nearest(a1, psi) <= z {
            seam_out = Some(a0);
            let (f, n) = (farthest(a0, theta), nearest(a0, theta));
            seam = match self.seam_reach {
                Some(r) => (
                    f.min(circular(a0 - a1) + slack(a0.abs() + a1.abs()) + r),
                    false,
                ),
                None => (f, f > 2.0 * n + inversion + e),
            };
        }
        // the turns at the table's ends (the angle modulo 360 kept): an inverted angle within
        // `EFF_ZERO_DEG` of a table end moved a turn, an exact output moved likewise
        let mut exact: Vec<f64> = constants.iter().copied().chain(seam_out).collect();
        for (kt, a, shift) in self.turns {
            if !Self::meets(piece, kt, self.turn_window(kt)) {
                continue;
            }
            let mut turned = Vec::new();
            for &[lo, hi] in &xs {
                if lo <= a + z && hi >= a - z {
                    turned.push([lo.max(a - z) + shift - e, hi.min(a + z) + shift + e]);
                }
            }
            let mut shifted = Vec::new();
            for &c in &exact {
                if (c - a).abs() <= z {
                    shifted.push(c + shift);
                }
            }
            xs.extend(turned);
            exact.extend(shifted);
        }
        let mut big = 0.0_f64;
        for &[lo, hi] in &xs {
            big = big.max(lo.abs()).max(hi.abs());
        }
        for &c in &exact {
            big = big.max(c.abs());
        }
        if !big.is_finite() {
            big = self.big;
        }
        // the clamp into the cone's domain: an inverted angle below `cmin` (above `cmax`)
        // output as `cmin` (`cmax`), at most its farthest from the angles it replaces plus
        // their error from `θ`; an exact output clamped, at most its farthest from `θ`
        let mut clamps: Vec<f64> = Vec::new();
        if let Some(([cmin, cmax], _)) = self.clamp {
            for &[lo, hi] in &xs {
                if lo < cmin {
                    clamps.push(
                        farthest(cmin, theta).min(farthest(cmin, [lo, hi.min(cmin)]) + err + e),
                    );
                }
                if hi > cmax {
                    clamps.push(
                        farthest(cmax, theta).min(farthest(cmax, [lo.max(cmax), hi]) + err + e),
                    );
                }
            }
            for &c in &exact {
                if c < cmin {
                    constants.push(cmin);
                }
                if c > cmax {
                    constants.push(cmax);
                }
            }
            big = big.min(cmin.abs().max(cmax.abs()));
        }
        let (mut far, mut narrows) = seam;
        for f in clamps {
            far = far.max(f);
        }
        for &c in &constants {
            let (f, n) = (farthest(c, theta), nearest(c, theta));
            far = far.max(f);
            // cutting the piece can bring this constant's farthest down to its nearest
            narrows |= f > 2.0 * n + inversion + e;
        }
        (far, big, narrows)
    }

    /// The bound over the sweep piece `[s0, s1]` (inside one span of the net's rows and of
    /// the polar circle) and the whole linear direction.
    fn piece(&self, [s0, s1]: [f64; 2], can_split: bool) -> Piece {
        let m = 0.5 * (s0 + s1);
        let rho = (Ivl::from(s1) - Ivl::from(m))
            .hi
            .max((Ivl::from(m) - Ivl::from(s0)).hi);
        let [n0, n1] = self.nat;
        if !(s0 >= n0 && s1 <= n1 && rho.is_finite()) {
            return Piece::Loose;
        }
        let cp = &self.polar;
        let kp = span(&cp.u, cp.p, cp.n, m);
        if !(knot(&cp.u, kp as isize) <= s0 && s1 <= knot(&cp.u, kp as isize + 1)) {
            return Piece::Loose;
        }
        let Some((theta, a_lo, a_hi, [ax, ay], inversion)) = self.angle(kp, m, rho) else {
            return if can_split {
                Piece::Split
            } else {
                Piece::Loose
            };
        };
        let (far, big, narrows) = self.events([s0, s1], theta, inversion);
        let base = inversion + self.a4;
        if far > 2.0 * base && narrows && can_split && s1 - s0 > 4.0 * EFF_ZERO * (1.0 + m.abs()) {
            return Piece::Split;
        }
        let alpha_deg = base.max(far + self.a4);
        let chord = (Ivl::from(alpha_deg) * pi() / Ivl::from(180.0)).hi.min(2.0);
        // the rows
        let Some(first) = self.rows.first() else {
            return Piece::Loose;
        };
        let ks = span(&first.u, first.p, first.n, m);
        if !(knot(&first.u, ks as isize) <= s0 && s1 <= knot(&first.u, ks as isize + 1)) {
            return Piece::Loose;
        }
        let aa = {
            let mut s = mul(&ax, &ax);
            add_scaled(&mut s, &mul(&ay, &ay), Ivl::from(1.0));
            s
        };
        let mut rows = Vec::new();
        for row in &self.rows {
            let Some(r) = self.row(row, ks, m, rho, &ax, &ay, &aa, a_lo, a_hi) else {
                return Piece::Loose;
            };
            rows.push(r);
        }
        // the linear direction's spans
        let (lk, q, nl) = self.lam;
        let (e0, e1) = (knot(lk, q as isize), knot(lk, nl as isize));
        let mut worst = 0.0_f64;
        let mut k = q;
        while k < nl {
            let (l0, l1) = (knot(lk, k as isize), knot(lk, k as isize + 1));
            k += 1;
            if !(l0 < l1 && l0 >= e0 && l1 <= e1) {
                continue;
            }
            match self.linear_span(&rows, k - 1, [l0, l1], rho, chord, big) {
                Some(b) => worst = worst.max(b),
                None => return Piece::Loose,
            }
        }
        if worst.is_finite() {
            Piece::Bound(worst)
        } else {
            Piece::Loose
        }
    }

    /// One row's polynomials and bounds at a piece.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        row: &NurbsCurve,
        ks: usize,
        m: f64,
        rho: f64,
        ax: &[Ivl],
        ay: &[Ivl],
        aa: &[Ivl],
        a_lo: f64,
        a_hi: f64,
    ) -> Option<Row> {
        let tr = curve_taylor(row, ks, m);
        let [nx, ny, nz, w] = &tr;
        let d: Vec<Poly> = [nx, ny, nz]
            .iter()
            .zip(&self.o)
            .map(|(nk, &ok)| {
                let mut d = (*nk).clone();
                add_scaled(&mut d, w, Ivl::from(0.0) - ok);
                d
            })
            .collect();
        let w_lo = range(w, rho).lo;
        let positive = w_lo > 0.0;
        if !positive {
            return None;
        }
        let (bx, by, zz) = (
            combine(&d, &self.x),
            combine(&d, &self.y),
            combine(&d, &self.z),
        );
        // across: cross(a, b_j)
        let mut cross = mul(ax, &by);
        add_scaled(&mut cross, &mul(ay, &bx), Ivl::from(-1.0));
        let perp = range(&cross, rho).mag();
        // along: dot(a, b_j) − |a| c_j, through dot² − |a|² c²
        let mut dotp = mul(ax, &bx);
        add_scaled(&mut dotp, &mul(ay, &by), Ivl::from(1.0));
        let mut c = Vec::new();
        add_scaled(&mut c, w, Ivl::from(self.r0));
        add_scaled(&mut c, &zz, Ivl::from(self.t));
        let mut diff = mul(&dotp, &dotp);
        add_scaled(&mut diff, &mul(aa, &mul(&c, &c)), Ivl::from(-1.0));
        let big_m = size(&diff, rho);
        let (dr, crr) = (range(&dotp, rho), range(&c, rho));
        let mut e = (Ivl::from(dr.mag()) + Ivl::from(a_hi) * Ivl::from(crr.mag())).hi;
        let same = (dr.lo >= 0.0 && crr.lo >= 0.0) || (dr.hi <= 0.0 && crr.hi <= 0.0);
        if same && big_m.is_finite() {
            e = e.min(Ivl::from(big_m).sqrt().hi);
            let den = Ivl::from(dr.mig()) + Ivl::from(a_lo) * Ivl::from(crr.mig());
            if den.lo > 0.0 {
                e = e.min((Ivl::from(big_m) / den).hi);
            }
        }
        let t3 = ((Ivl::from(perp) + Ivl::from(e)) / (Ivl::from(a_lo) * Ivl::from(w_lo))).hi;
        let reach = d
            .iter()
            .fold(zero(), |acc, dk| {
                let r = Ivl::from(range(dk, rho).mag());
                acc + r * r
            })
            .sqrt();
        let to_origin = (reach / Ivl::from(w_lo)).hi;
        (t3.is_finite() && to_origin.is_finite()).then_some(Row {
            w: w.clone(),
            zz,
            w_lo,
            t3,
            to_origin,
        })
    }

    /// The bound over a sweep piece (`rows`, `rho`, the chord of its angle error and the
    /// angles' size) and the linear span `k` over `[l0, l1]`.
    fn linear_span(
        &self,
        rows: &[Row],
        k: usize,
        [l0, l1]: [f64; 2],
        rho: f64,
        chord: f64,
        big: f64,
    ) -> Option<f64> {
        let (lk, q, _) = self.lam;
        let lm = 0.5 * (l0 + l1);
        let rl = (Ivl::from(l1) - Ivl::from(lm))
            .hi
            .max((Ivl::from(lm) - Ivl::from(l0)).hi);
        let basis = basis_polys(lk, k, q, lm);
        let h_mid = self.map.at(Ivl::from(lm));
        let h_poly: Poly = vec![h_mid, self.map.slope()];
        // G = Σ_j B_j (h W_j − zz_j), by powers of μ
        let mut g: Vec<Poly> = vec![Vec::new(); q + 2];
        let (mut w_lo, mut t3, mut to_origin) = (f64::INFINITY, 0.0_f64, 0.0_f64);
        for (i, b) in basis.iter().enumerate() {
            let j = (k + i).checked_sub(q)?;
            let row = rows.get(j)?;
            w_lo = w_lo.min(row.w_lo);
            t3 = t3.max(row.t3);
            to_origin = to_origin.max(row.to_origin);
            let bh = mul(b, &h_poly);
            for (pw, slot) in g.iter_mut().enumerate() {
                if let Some(&c) = bh.get(pw) {
                    add_scaled(slot, &row.w, c);
                }
                if let Some(&c) = b.get(pw) {
                    add_scaled(slot, &row.zz, Ivl::from(0.0) - c);
                }
            }
        }
        let mut g_size = zero();
        let mut power = Ivl::from(1.0);
        for slot in &g {
            g_size = g_size + Ivl::from(size(slot, rho)) * power;
            power = power * Ivl::from(rl);
        }
        // the heights: the map's, its rounding, the clamp's reach
        let h = self.map.at(Ivl::new(l0, l1));
        let e_y = self.map.rounding(h);
        let mut v = h + Ivl::new(-e_y, e_y);
        let mut reach = 0.0_f64;
        if let Some((_, [b0, b1])) = self.clamp {
            reach = (b0 - h.lo).max(h.hi - b1).max(0.0);
            v = Ivl::new(v.lo.max(b0).min(b1), v.hi.min(b1).max(b0));
        }
        let dv = Ivl::from(e_y) + Ivl::from(reach) + g_size / Ivl::from(w_lo);
        let radius = (Ivl::from(self.r0) + Ivl::from(self.t) * v).mag();
        let (s1g, g) = self.frame;
        let total = Ivl::from(self.evaluation(big, v.mag()))
            + Ivl::from(s1g)
                * (Ivl::from(t3)
                    + (Ivl::from(self.t.abs()) + Ivl::from(1.0)) * dv
                    + Ivl::from(radius) * Ivl::from(chord))
            + Ivl::from(g) * Ivl::from(to_origin);
        total.is_finite().then_some(total.hi)
    }
}

/// Not upstream (#20643): an upper bound on `|S(σ, λ) − N(σ, λ)|` over the whole parameter
/// box of `net`, `S` the cone `cone` as it evaluates its point at the net's parameters and `N`
/// the net's exact point (see the module docs). `None` where it is not enclosed: the cone's
/// polar conversion not set up, a weight of the net's not proved positive, or a bound that is
/// not finite.
///
/// # Errors
///
/// The kernel's cancelled status (polled once per piece of the sweep).
#[cfg(test)]
pub(super) fn cone_net_residual(k: &Kernel, cone: &Cone, net: &Nurbs) -> Result<Option<f64>> {
    cone_net_residual_within(k, cone, net, None)
}

/// [`cone_net_residual`] over the net's parameters whose sweep coordinate is in `window`'s
/// (`[[u0, u1], [v0, v1]]`, the net's `(u, v)`; the sweep direction's range taken, the
/// linear direction's whole range kept): the distance from the net to the cone at every
/// parameter of the net in that box. It is taken over the pieces of the whole net's sweep
/// partition (its knots, the polar knots and the snap windows' cuts) that meet that range, so
/// a piece is never cut inside a snap window. A piece reaching past that range is halved
/// toward it (halves outside it dropped), each half's bound capped by the piece's own, so
/// the bound is at most the whole net's. The whole net where `window` is `None`, or where its sweep range
/// does not meet the net's (a bound over the whole net holds over any part of it).
pub(super) fn cone_net_residual_within(
    k: &Kernel,
    cone: &Cone,
    net: &Nurbs,
    window: Option<[[f64; 2]; 2]>,
) -> Result<Option<f64>> {
    let mut cancelled = false;
    let bound = enclose(k, cone, net, window, &mut cancelled);
    if cancelled {
        k.check_cancelled(OP)?;
    }
    Ok(bound)
}

/// [`cone_net_residual`]'s enclosure; `cancelled` set (and `None`) where the kernel was
/// cancelled during the sweep.
fn enclose(
    k: &Kernel,
    cone: &Cone,
    net: &Nurbs,
    window: Option<[[f64; 2]; 2]>,
    cancelled: &mut bool,
) -> Option<f64> {
    let rev = &cone.rev;
    let circle = rev.polar_curve()?;
    let table = &rev.polar;
    if !table.has_curve || table.natural_knots.is_empty() || table.angles.is_empty() {
        return None;
    }
    if circle.radius_y == 0.0 {
        return None;
    }
    let position = &rev.position;
    let (o, x, y, z) = (
        ivl3(position.origin()),
        ivl3(position.x_axis()),
        ivl3(position.y_axis()),
        ivl3(position.z_axis()),
    );
    // g ≥ ‖FᵀF − I‖ (Frobenius)
    let axes = [x, y, z];
    let mut g2 = zero();
    for (i, a) in axes.iter().enumerate() {
        for (j, b) in axes.iter().enumerate() {
            let e = dot3(a, b) - Ivl::from(if i == j { 1.0 } else { 0.0 });
            g2 = g2 + e * e;
        }
    }
    let g = g2.sqrt().hi;
    let s1g = (Ivl::from(1.0) + Ivl::from(g)).sqrt().hi;
    // the height map
    let inside_out = cone.inside_out;
    let step_ivl = rev.anal_uv_domain.v_interval();
    let nurb_domain = rev.base.natural_uv_domain();
    let swap = rev.swap_uv;
    let nurb_ivl = if swap {
        nurb_domain.u_interval()
    } else {
        nurb_domain.v_interval()
    };
    let linear = rev.is_bounded().ok()?
        || crate::surface::cone::p1::unbounded_maps_linearly(inside_out, &step_ivl, &nurb_ivl);
    let map = if linear {
        if inside_out {
            HeightMap::Affine {
                s0: step_ivl.min(),
                n0: nurb_ivl.max(),
                l: -nurb_ivl.length(),
                ls: step_ivl.length(),
            }
        } else {
            HeightMap::Affine {
                s0: step_ivl.min(),
                n0: nurb_ivl.min(),
                l: nurb_ivl.length(),
                ls: step_ivl.length(),
            }
        }
    } else {
        HeightMap::Sign(if inside_out { -1.0 } else { 1.0 })
    };
    let clamp = (!rev.is_out_of_bounds_enabled()).then(|| {
        let u = rev.anal_uv_domain.u_interval();
        ([u.min(), u.max()], [step_ivl.min(), step_ivl.max()])
    });
    // the rows along the sweep direction
    let (sk, sp, sn, lk, lq, ln) = if swap {
        (&net.v, net.q, net.nv, &net.u, net.p, net.nu)
    } else {
        (&net.u, net.p, net.nv, &net.v, net.q, net.nv)
    };
    let sn = if swap { sn } else { net.nu };
    let mut rows = Vec::new();
    for j in 0..ln {
        let mut pw = Vec::new();
        for i in 0..sn {
            let at = if swap { j * net.nv + i } else { i * net.nv + j };
            pw.extend_from_slice(net.pw.get(4 * at..4 * at + 4)?);
        }
        rows.push(NurbsCurve {
            p: sp,
            n: sn,
            u: sk.clone(),
            pw,
        });
    }
    let polar = stored_nurbs(&circle.base)?;
    let seam_reach = seam_reach(k, circle, &polar);
    let nat = circle.base.natural_interval();
    let anal = circle.anal_domain;
    let (a0, a1) = (anal.min(), anal.max());
    let snaps = if circle.inside_out {
        [a1, a0]
    } else {
        [a0, a1]
    };
    let knots = &table.natural_knots;
    let angles = &table.angles;
    let (k0, k1) = (*knots.first()?, *knots.last()?);
    let (g0, g1) = (*angles.first()?, *angles.last()?);
    let scaled_zero = (k1 - k0) / 360.0 * EFF_ZERO_DEG;
    let mut big = a0.abs().max(a1.abs()).max(g0.abs()).max(g1.abs());
    if let Some(([cmin, cmax], _)) = clamp {
        big = big.max(cmin.abs()).max(cmax.abs());
    }
    let big = big + 721.0;
    let a4 = (Ivl::from(4.0 * U)
        * Ivl::from(big)
        * (Ivl::from(4.0) + Ivl::from(big) / Ivl::from(360.0)))
    .hi + 1.0e-12;
    // (#20643 review: a parameter that is NaN or infinite where it is taken as a number gives
    // no bound; an unbounded height or angle range is not taken as one, only checked for NaN)
    let finite = [
        a0,
        a1,
        g0,
        g1,
        k0,
        k1,
        nat.min(),
        nat.max(),
        circle.radius_x / circle.radius_y,
    ]
    .iter()
    .chain([cone.base_radius, cone.tan_semi_angle()].iter())
    .all(|v| v.is_finite());
    let affine = match map {
        HeightMap::Affine { s0, n0, l, ls } => [s0, n0, l, ls].iter().all(|v| v.is_finite()),
        HeightMap::Sign(_) => true,
    };
    let clamp_ok =
        clamp.is_none_or(|([u0, u1], [v0, v1])| ![u0, u1, v0, v1].iter().any(|v| v.is_nan()));
    if !(finite && affine && clamp_ok && !step_ivl.min().is_nan() && !step_ivl.max().is_nan()) {
        return None;
    }
    let position_c = circle.position;
    let mut setup = Setup {
        o,
        x,
        y,
        z,
        r0: cone.base_radius,
        t: cone.tan_semi_angle(),
        map,
        clamp,
        frame: (s1g, g),
        rows,
        lam: (lk, lq, ln),
        polar,
        c: ivl3(position_c.origin()),
        xc: ivl3(position_c.x_axis()),
        yc: ivl3(position_c.y_axis()),
        f: circle.radius_x / circle.radius_y,
        nat: [nat.min(), nat.max()],
        angular: [a0, a1],
        length: anal.length(),
        snaps,
        seam_reach,
        knot_ends: [k0, k1],
        turns: if circle.inside_out {
            [(k1, g0, -360.0), (k0, g1, 360.0)]
        } else {
            [(k0, g1, -360.0), (k1, g0, 360.0)]
        },
        scaled_zero,
        a4,
        big,
        loose: f64::INFINITY,
    };
    setup.loose = loose_bound(&setup, net)?;
    // the sweep pieces: cut at the knots and windows
    let (d0, d1) = (knot(sk, sp as isize), knot(sk, sn as isize));
    if !(d0 < d1 && d0.is_finite() && d1.is_finite()) {
        return None;
    }
    let mut cuts: Vec<f64> = vec![d0, d1];
    cuts.extend(sk.iter().copied());
    cuts.extend(setup.polar.u.iter().copied());
    cuts.extend(setup.window_cuts());
    let mut cuts: Vec<f64> = cuts
        .into_iter()
        .filter(|c| c.is_finite() && *c >= d0 && *c <= d1)
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    // (#20643: only the whole net's pieces that meet the sweep range of the caller's box, so
    // each piece is one the whole net's partition has, never one cut inside a snap window)
    let sweep = window.map(|[u, v]| if swap { v } else { u });
    if let Some([w0, w1]) = sweep {
        let first = cuts.iter().rposition(|c| *c <= w0).unwrap_or(0);
        let last = cuts
            .iter()
            .position(|c| *c >= w1)
            .unwrap_or_else(|| cuts.len().saturating_sub(1));
        if let Some(kept) = cuts.get(first..=last).filter(|kept| kept.len() >= 2) {
            cuts = kept.to_vec();
        }
    }
    // (each piece with `cap`, the least bound of the pieces it was cut from because they
    // reach past the window: the distance over the piece's part in the window is under both)
    let mut stack: Vec<([f64; 2], u32, f64)> = cuts
        .windows(2)
        .filter_map(|w| match w {
            [a, b] if a < b => Some(([*a, *b], 0, f64::INFINITY)),
            _ => None,
        })
        .collect();
    // (#20643 review: a partition past the budget is not evaluated piece by piece; the loose
    // bound holds over the whole net)
    if stack.len() >= MAX_PIECES {
        return setup.loose.is_finite().then_some(setup.loose);
    }
    // a piece meets the window's sweep range (closed), or there is none
    let meets = |[s0, s1]: [f64; 2]| sweep.is_none_or(|[w0, w1]| s0 <= w1 && w0 <= s1);
    // a piece reaches past it
    let reaches_past = |[s0, s1]: [f64; 2]| sweep.is_some_and(|[w0, w1]| s0 < w0 || w1 < s1);
    let mut worst = 0.0_f64;
    let mut pieces = stack.len();
    while let Some(([s0, s1], depth, cap)) = stack.pop() {
        if k.check_cancelled(OP).is_err() {
            *cancelled = true;
            return None;
        }
        let m = 0.5 * (s0 + s1);
        let can_split = depth < MAX_DEPTH && pieces + 1 < MAX_PIECES && s0 < m && m < s1;
        let (b, cut_cap) = match setup.piece([s0, s1], can_split) {
            Piece::Bound(b) if can_split && reaches_past([s0, s1]) => (None, cap.min(b)),
            Piece::Bound(b) => (Some(b), cap),
            Piece::Loose => (Some(setup.loose), cap),
            Piece::Split => (None, cap),
        };
        if let Some(b) = b {
            worst = worst.max(b.min(cap));
        } else {
            pieces += 1;
            for half in [[s0, m], [m, s1]] {
                if meets(half) {
                    stack.push((half, depth + 1, cut_cap));
                }
            }
        }
    }
    worst.is_finite().then_some(worst)
}

/// The farthest, in degrees (rounded up), the direction `â` of the polar circle's exact point
/// can be from the direction of its domain's end `a1` where `Ellipse::step_inversion` snaps
/// the point to the seam: the snap needs `|Ẽ − P̃| ≤ τ` (`τ` the approximation tolerance
/// `zone_tol / 100` of the kernel's zone tolerance, as `ConvertToPolarParameter` passes none),
/// `Ẽ` the rounded end point and `P̃` the rounded polar point. With `δE`, `δP` their rounding,
/// `M` the map `P ↦ a` (rows `Xc` and `f Yc`; `‖M‖ ≤ L`, `L²` the larger eigenvalue of
/// `M Mᵀ = [[Xc·Xc, f Xc·Yc], [f Xc·Yc, f² Yc·Yc]]`) and `a(E) = rx (cos a1, sin a1) + d`
/// (`d` the circle frame's skew), `|a(P) − rx (cos a1, sin a1)| ≤ D = L (τ + δP + δE) + |d|`,
/// so the angle is at most `asin(D / rx) ≤ (π/3) D / rx` (`asin x / x` grows on `(0, 1]` and is
/// `π/3` at `1/2`), in degrees `60 D / rx`. `None` (no reach proved: the
/// farthest of `a0` from `θ` is taken instead) for a radius not positive or `D > rx / 2`.
fn seam_reach(k: &Kernel, circle: &Ellipse, polar: &NurbsCurve) -> Option<f64> {
    let (rx, ry) = (circle.radius_x, circle.radius_y);
    if !(rx > 0.0 && ry > 0.0 && rx.is_finite() && ry.is_finite()) {
        return None;
    }
    let z_tol = context_zone_tol_3d(k, None, SmallTopoType::NormalSize);
    let tau = Ivl::from((z_tol / 100.0).abs()) * Ivl::from(1.0 + 1.0e-12);
    // δP: each coordinate within 3 γ(8p + 8) of the largest control point's (as `angle`)
    let gp = Ivl::from(3.0 * gamma(8.0 * polar.p as f64 + 8.0));
    let mut xk = [0.0_f64; 3];
    for cpt in polar.pw.chunks_exact(4) {
        let &[px, py, pz, w] = cpt else {
            return None;
        };
        for (slot, c) in xk.iter_mut().zip([px, py, pz]) {
            *slot = slot.max((Ivl::from(c) / Ivl::from(w)).mag());
        }
    }
    let mut dp = zero();
    for x in xk {
        dp = dp + gp * Ivl::from(x);
    }
    // δE: `C + rx cos(a1 π/180) Xc + ry sin(a1 π/180) Yc`, the argument within 3 roundings
    // and libm's `cos`/`sin` within 2 ulps, then the products and sums
    let pos = &circle.position;
    let (c, xc, yc) = (ivl3(pos.origin()), ivl3(pos.x_axis()), ivl3(pos.y_axis()));
    let a1 = circle.anal_domain.max();
    let g4 = Ivl::from(gamma(4.0));
    let one = Ivl::from(1.0);
    let trig = Ivl::from(a1.abs()) * pi() / Ivl::from(180.0) * Ivl::from(3.01 * U)
        + Ivl::from(4.0 * U)
        + Ivl::from(1.0e-300);
    let (rxi, ryi) = (Ivl::from(rx), Ivl::from(ry));
    let mut de = zero();
    for ((ck, xa), ya) in c.iter().zip(&xc).zip(&yc) {
        let t = rxi * Ivl::from(xa.mag()) + ryi * Ivl::from(ya.mag());
        de = de + g4 * (Ivl::from(ck.mag()) + t) + t * trig * (one + g4);
    }
    // the map's norm and the frame's skew `d`
    let f = Ivl::from(rx / ry);
    let (xx, xy, yy) = (dot3(&xc, &xc), dot3(&xc, &yc), dot3(&yc, &yc));
    // ‖M‖² = (tr + √((xx − f² yy)² + 4 f² xy²)) / 2, the larger eigenvalue of M Mᵀ
    let (tr, diff) = (xx + f * f * yy, xx - f * f * yy);
    let disc = diff * diff + Ivl::from(4.0) * f * f * xy * xy;
    let l = ((tr + disc.sqrt()) * Ivl::from(0.5)).sqrt();
    let skew = rxi * Ivl::from((xx - one).mag())
        + ryi * Ivl::from(xy.mag())
        + Ivl::from((f * rxi).mag()) * Ivl::from(xy.mag())
        + Ivl::from((f * ryi * yy - rxi).mag());
    let d = l * (tau + dp + de) + skew;
    if !(d.is_finite() && d.hi <= 0.5 * rx) {
        return None;
    }
    let reach = (Ivl::from(60.0) * d / rxi * Ivl::from(1.0 + 1.0e-9)).hi;
    reach.is_finite().then_some(reach)
}

/// The loose bound `|N − O| + √(1+g)(|r| + |v|) + |S − E|` over the whole net: `|N − O|` at
/// most the farthest control point's (positive weights, a convex combination), `|S − O|` at
/// most the frame's norm times `|(r cos, r sin, v)|`. `None` for a weight not positive.
fn loose_bound(setup: &Setup<'_>, net: &Nurbs) -> Option<f64> {
    let mut reach = 0.0_f64;
    for c in net.pw.chunks_exact(4) {
        let [px, py, pz, w] = [*c.first()?, *c.get(1)?, *c.get(2)?, *c.get(3)?];
        // (a NaN or infinite coordinate or weight gives no bound: `f64::max` drops a NaN)
        let finite = [px, py, pz, w].iter().all(|v| v.is_finite());
        if !(finite && w > 0.0) {
            return None;
        }
        let w = Ivl::from(w);
        let p = [Ivl::from(px) / w, Ivl::from(py) / w, Ivl::from(pz) / w];
        let d = p
            .iter()
            .zip(&setup.o)
            .fold(zero(), |acc, (&a, &b)| acc + (a - b) * (a - b))
            .sqrt()
            .hi;
        if !d.is_finite() {
            return None;
        }
        reach = reach.max(d);
    }
    let (lk, q, nl) = setup.lam;
    let h = setup
        .map
        .at(Ivl::new(knot(lk, q as isize), knot(lk, nl as isize)));
    let e_y = setup.map.rounding(h);
    let mut v = h + Ivl::new(-e_y, e_y);
    if let Some((_, [b0, b1])) = setup.clamp {
        v = Ivl::new(v.lo.max(b0).min(b1), v.hi.min(b1).max(b0));
    }
    let radius = (Ivl::from(setup.r0) + Ivl::from(setup.t) * v).mag();
    let total = Ivl::from(reach)
        + Ivl::from(setup.frame.0) * (Ivl::from(radius) + Ivl::from(v.mag()))
        + Ivl::from(setup.evaluation(setup.big, v.mag()));
    total.is_finite().then_some(total.hi)
}

#[cfg(test)]
mod tests {
    use super::super::{net_to_surface_bound, NetSurface};
    use crate::kernel::Kernel;
    use crate::math::Point2;
    use crate::math::{Point3, Vec3};
    use crate::surface::cone::Cone;
    use crate::surface::surface::Surface;

    /// Bounded cones and cylinders about the z axis through the origin, of every
    /// parameterisation the conversion branches on: `(name, cone, least radius, greatest radius)`.
    fn cones(k: &Kernel) -> Vec<(String, Surface, f64, f64)> {
        let mut out = Vec::new();
        for (bot, top) in [(2.0, 2.0), (2.0, 1.0), (0.5, 3.0), (12.5, 12.5)] {
            for (a0, a1) in [(0.0, 360.0), (0.0, 270.0), (30.0, 100.0), (-90.0, 180.0)] {
                for swap in [false, true] {
                    for inside_out in [false, true] {
                        let mut c = Cone::new_bounded(
                            Point3::ZERO,
                            Vec3::X,
                            Vec3::Y,
                            bot,
                            top,
                            a0,
                            a1,
                            3.0,
                            swap,
                            inside_out,
                            true,
                        );
                        c.make_nurb(k).unwrap();
                        let name =
                            format!("r {bot}..{top} angles {a0}..{a1} swap {swap} io {inside_out}");
                        out.push((
                            name,
                            Surface::from(c),
                            f64::min(bot, top),
                            f64::max(bot, top),
                        ));
                    }
                }
            }
        }
        out
    }

    /// The cone's net, and its net with every control point's `x, y` scaled by `1 + eps`
    /// (about the axis: the lifted net's point is the net's point so scaled, at the same
    /// parameters, its weights unchanged).
    fn nets(k: &Kernel, s: &Surface, eps: f64) -> (super::Nurbs, super::Nurbs) {
        let net = NetSurface::of(k, s, &[]).unwrap().unwrap().net;
        let mut lifted = net.clone();
        for c in lifted.pw.chunks_exact_mut(4) {
            c[0] *= 1.0 + eps;
            c[1] *= 1.0 + eps;
        }
        (net, lifted)
    }

    #[test]
    fn a_cone_net_lifted_off_its_cone_is_counted_in_the_bound() {
        // The lifted net's point is M N (M scales x, y by 1 + eps), N within `b0` (the
        // unlifted bound) of the cone's point S: |S − M N| ≥ |M N − N| − |N − S| ≥
        // eps r − b0, r the least radius over the net (the cone's least, its radius affine
        // in the height and the net's radius within b0 of the cone's).
        let k = Kernel::default();
        for eps in [1.0e-9, 1.0e-6, 1.0e-3] {
            for (name, s, r, _) in cones(&k) {
                let (net, lifted) = nets(&k, &s, eps);
                let b0 = net_to_surface_bound(&k, &s, &net).unwrap().unwrap();
                let b = net_to_surface_bound(&k, &s, &lifted)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{name} eps {eps:e}: no bound"));
                let lift = eps * (r - b0) - b0;
                assert!(
                    b >= lift,
                    "{name} eps {eps:e}: bound {b:e} below the lift {lift:e}"
                );
                // and it is no looser than needed: the lift at the farthest radius, plus
                // the unlifted bound and the rounding (closed form: |M N − S| ≤ eps |N_xy|
                // + |N − S|)
                let reach = eps * (3.0 * 6.0 + 2.0 * r) + b0;
                assert!(b <= 1.0e3 * reach, "{name} eps {eps:e}: bound {b:e} loose");
            }
        }
    }

    #[test]
    fn the_bound_holds_inside_the_end_snap_windows() {
        // Not a proof (the bound is one): the distance measured at parameters inside the
        // windows where the conversion snaps to its domain's ends is under the bound.
        let k = Kernel::default();
        let eps = 1.0e-7;
        for (name, s, _, _) in cones(&k) {
            let (net, lifted) = nets(&k, &s, eps);
            let b = net_to_surface_bound(&k, &s, &lifted).unwrap().unwrap();
            let crate::surface::surface::SurfaceKind::Cone(c) = s.kind() else {
                unreachable!()
            };
            let (u, v) = (net.u.clone(), net.v.clone());
            let (u0, u1) = (u[net.p], u[net.nu]);
            let (v0, v1) = (v[net.q], v[net.nv]);
            for (su, sv) in [(true, false), (false, true)] {
                for t in [0.0, 1.0e-13, 5.0e-13, 9.0e-13, 2.0e-12] {
                    for end in [false, true] {
                        let along = |a: f64, b: f64| if end { b - t } else { a + t };
                        let uv = Point2::new(
                            if su { along(u0, u1) } else { 0.5 * (u0 + u1) },
                            if sv { along(v0, v1) } else { 0.5 * (v0 + v1) },
                        );
                        let p = s.evaluate_point(&k, uv).unwrap();
                        let n = c.rev.base.evaluate_point(uv).unwrap();
                        let m = Vec3::new(n.x * (1.0 + eps), n.y * (1.0 + eps), n.z);
                        let d = (p - m).length();
                        assert!(d <= b, "{name} at {uv:?}: {d:e} over {b:e}");
                    }
                }
            }
        }
    }

    /// The derived allowance a full turn adds: the seam snap of `Ellipse::step_inversion`
    /// moves a point whose rounded polar point is within `τ = 1e-7` (the default zone
    /// tolerance over 100) of the domain's end onto the seam, an angle of at most
    /// `asin(D / rx) ≤ (π/3) D / rx` off its own (`D ≤ rx / 2`), `D = L τ` plus the
    /// roundings (`L = 1`, the map's operator norm for an orthonormal frame and a round
    /// circle; the roundings under `1e-12 (1 + r)`), seen at the radius `r`
    /// (`|S − N| ≤ r Δα`), with 1% over for the `√(1 + g)` frame factor and the bound's own
    /// margins.
    fn seam_allowance(r: f64, rx: f64) -> f64 {
        std::f64::consts::FRAC_PI_3 * (1.0e-7 + 1.0e-12 * (1.0 + r)) * r / rx * 1.01
    }

    #[test]
    fn an_exact_cone_nets_distance_to_its_cone_is_enclosed_tightly() {
        let k = Kernel::default();
        for (name, s, _, r_max) in cones(&k) {
            let (net, _) = nets(&k, &s, 0.0);
            let b = net_to_surface_bound(&k, &s, &net)
                .unwrap()
                .unwrap_or_else(|| panic!("{name}: no bound"));
            let crate::surface::surface::SurfaceKind::Cone(c) = s.kind() else {
                unreachable!()
            };
            let circle = c.rev.polar_curve().unwrap();
            // the stored net is the cone up to its control points' rounding: a few ulps of
            // the size (3 + radius) of its points; the end snaps output the domain's end for
            // `σ` within `w ≈ 1e-12` of the circle's end, a real tear of up to `r ŝ w` (`ŝ`
            // the angle's speed in radians per `σ`: on a rational quadratic arc under 180° at
            // most 1.25 times its arc's mean, the end's `2 sin(φ/2) / φ` under 1; taken as
            // twice the circle's mean), counted over the window's piece (`2 w` wide, cut at
            // twice the window) and the bisection's floor (`4 EFF_ZERO (1 + |σ|)`): `r ŝ 8 w`,
            // `w` taken as `1.01e-12 (1 + |σ|)`; a full turn adds the seam snap's reach
            let nat = circle.base.natural_interval();
            let speed = 2.0 * (circle.anal_domain.length() / nat.length()).to_radians();
            let ends = nat.min().abs().max(nat.max().abs());
            let snap = r_max * speed * 8.0 * 1.01e-12 * (1.0 + ends);
            let seam = if circle.anal_domain.length() >= 360.0 - 1.0e-4 {
                seam_allowance(r_max, circle.radius_x)
            } else {
                0.0
            };
            assert!(
                b <= 1.0e-12 * (3.0 + r_max * 6.0) + snap + seam,
                "{name}: bound {b:e}"
            );
        }
    }

    /// The seam snap is real: on a full-turn cylinder of radius 12.5 the surface evaluated
    /// `5e-8` before its `u` domain's end is snapped onto the seam (its rounded polar point
    /// within `τ = 1e-7` of the end), more than `1e-8` from the exact net's point there
    /// (measured about `4.5e-8`), so no bound of a full-turn net under `1e-8` can hold (the
    /// `1e-9` the trims' tests asked before); the bound covers it.
    #[test]
    fn a_full_turn_cones_seam_snap_is_inside_the_bound() {
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            12.5,
            12.5,
            0.0,
            360.0,
            3.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c.clone());
        let net = NetSurface::of(&k, &s, &[]).unwrap().unwrap().net;
        let b = net_to_surface_bound(&k, &s, &net).unwrap().unwrap();
        let d = c.rev.base.natural_uv_domain();
        let v = 0.5 * (d.v_interval().min() + d.v_interval().max());
        let uv = Point2::new(d.u_interval().max() - 5.0e-8, v);
        let p = s.evaluate_point(&k, uv).unwrap();
        let n = c.rev.base.evaluate_point(uv).unwrap();
        let gap = (p - n).length();
        assert!(gap > 1.0e-8, "the snap tears {gap:e}");
        assert!(gap <= b, "{gap:e} over the bound {b:e}");
    }

    #[test]
    fn a_window_edge_at_a_knot_takes_no_more_than_the_whole_net() {
        // #20643: a window is taken as the whole net's pieces that meet it, so a window edge
        // just off a knot or a polar end (inside a snap window) is no piece of its own: the
        // windowed bound is a maximum over a subset of the whole net's pieces, at most the
        // whole net's (a window cut inside a snap window took the snap's constant: 6 for a
        // whole-net bound of 1.5e-11 in heal.stitch file_extrusion2).
        let k = Kernel::default();
        let mut checked = 0;
        for (name, s, _, _) in cones(&k) {
            let crate::surface::surface::SurfaceKind::Cone(c) = s.kind() else {
                unreachable!()
            };
            let net = NetSurface::of(&k, &s, &[]).unwrap().unwrap().net;
            let whole = super::cone_net_residual(&k, c, &net).unwrap().unwrap();
            let (u0, u1) = (net.u[net.p], net.u[net.nu]);
            let (v0, v1) = (net.v[net.q], net.v[net.nv]);
            let mut edges: Vec<f64> = net.u.iter().chain(net.v.iter()).copied().collect();
            edges.sort_by(f64::total_cmp);
            edges.dedup();
            for &e in &edges {
                for t in [0.0, 5.0e-13, 3.6e-12] {
                    for at in [e - t, e + t] {
                        for w in [
                            [[at, u1], [v0, v1]],
                            [[u0, at], [v0, v1]],
                            [[u0, u1], [at, v1]],
                            [[u0, u1], [v0, at]],
                        ] {
                            let b = super::cone_net_residual_within(&k, c, &net, Some(w))
                                .unwrap()
                                .unwrap_or_else(|| panic!("{name} {w:?}: no bound"));
                            assert!(b <= whole, "{name} {w:?}: {b:e} over the whole {whole:e}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 1000, "{checked}");
    }

    /// A net with a NaN control point gives no bound (`f64::max` would drop it from the
    /// loose bound's reach).
    #[test]
    fn a_net_cut_past_the_piece_budget_takes_the_loose_bound() {
        // (#20643 review) a cylinder's net as a polyline of 5000 sides along its circle: its
        // knots cut the sweep into more pieces than `MAX_PIECES`, which are not evaluated one
        // by one; the loose bound (past the radius) is taken instead of the chord's sag
        // (about 2.5e-6 here) the pieces would prove
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            12.5,
            12.5,
            0.0,
            360.0,
            3.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c.clone());
        let d = c.rev.base.natural_uv_domain();
        let (u0, u1) = (d.u_interval().min(), d.u_interval().max());
        let (v0, v1) = (d.v_interval().min(), d.v_interval().max());
        let n = 5000;
        let us: Vec<f64> = (0..n)
            .map(|i| u0 + (u1 - u0) * i as f64 / (n - 1) as f64)
            .collect();
        let mut knots = vec![u0];
        knots.extend(&us);
        knots.push(u1);
        let mut pw = Vec::new();
        for &u in &us {
            for v in [v0, v1] {
                let p = c.rev.base.evaluate_point(Point2::new(u, v)).unwrap();
                pw.extend_from_slice(&[p.x, p.y, p.z, 1.0]);
            }
        }
        let net = super::Nurbs {
            p: 1,
            q: 1,
            nu: n,
            nv: 2,
            u: knots,
            v: vec![v0, v0, v1, v1],
            pw,
        };
        assert!(n > super::MAX_PIECES);
        let b = net_to_surface_bound(&k, &s, &net).unwrap().unwrap();
        assert!(b > 12.5, "{b:e}");
    }

    #[test]
    fn a_nan_net_gives_no_bound() {
        let k = Kernel::default();
        for (name, s, _, _) in cones(&k) {
            let (mut net, _) = nets(&k, &s, 0.0);
            if let Some(c) = net.pw.get_mut(0) {
                *c = f64::NAN;
            }
            let b = net_to_surface_bound(&k, &s, &net).unwrap();
            assert!(b.is_none(), "{name}: {b:?}");
        }
    }

    /// The kept-trim cover's bound (`trim_distance_bound`) on a full-turn cylinder counts the
    /// cone's distance off its net (#20643 review): the trim `W` is an iso line `e` before the
    /// `u` end, the curve `C` the net's image of it (a straight line along the axis), and
    /// the surface's image of `W` is the formula point, off the net by up to the residual.
    /// The bound holds the formula point's distance from `C` and the residual. (Within the
    /// seam snap, about 1e-7 before the end, no bound is given.)
    #[test]
    fn a_cylinders_kept_trim_bound_counts_its_net_residual() {
        use super::super::polynomial_pieces::trim_distance_bound;
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            12.5,
            12.5,
            0.0,
            360.0,
            3.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c.clone());
        let residual =
            net_to_surface_bound(&k, &s, &NetSurface::of(&k, &s, &[]).unwrap().unwrap().net)
                .unwrap()
                .unwrap();
        let d = c.rev.base.natural_uv_domain();
        let (v0, v1) = (d.v_interval().min(), d.v_interval().max());
        let line = |a: Point3, b: Point3| {
            let n = NurbCurve::new(
                vec![
                    CPoint::new(a.x, a.y, a.z, NOW),
                    CPoint::new(b.x, b.y, b.z, NOW),
                ],
                1,
                vec![0.0, 0.0, 1.0, 1.0],
            )
            .unwrap();
            BSplineCurve::from_nurb(n, 3, false)
        };
        for e in [1e-2, 1e-3, 3e-4, 1e-4, 1e-5, 1e-6, 3e-7, 1.5e-7] {
            let u = d.u_interval().max() - e;
            let n0 = c.rev.base.evaluate_point(Point2::new(u, v0)).unwrap();
            let n1 = c.rev.base.evaluate_point(Point2::new(u, v1)).unwrap();
            let curve = line(n0, n1);
            let uv = line(Point3::new(u, v0, 0.0), Point3::new(u, v1, 0.0));
            let p = s
                .evaluate_point(&k, Point2::new(u, 0.5 * (v0 + v1)))
                .unwrap();
            let axis = (n1 - n0) * (1.0 / (n1 - n0).length());
            let off = p - n0;
            let across = (off - axis * off.dot(axis)).length();
            let g = trim_distance_bound(&k, &curve, [0.0, 1.0], &uv, [0.0, 1.0], &s, 1.0e-14, 0.0)
                .unwrap()
                .unwrap();
            assert!(g >= across, "{e:e}: {g:e} under {across:e}");
            // (the trim's box's residual: the seam's tear counted only near the seam)
            let net = NetSurface::of(&k, &s, &[]).unwrap().unwrap();
            let local = super::super::cone_net_offset(&k, &s, &net, &uv)
                .unwrap()
                .unwrap();
            assert!(g >= local, "{e:e}: {g:e} under the residual {local:e}");
            assert!(local <= residual, "{e:e}: {local:e} over {residual:e}");
            if e >= 1e-4 {
                assert!(
                    local * 100.0 < residual,
                    "{e:e}: {local:e} not local against {residual:e}"
                );
            }
        }
    }

    /// An offset of a cone evaluates the cone's formula point plus the distance along its
    /// net's normal (#20643 review): at a nonzero distance it is off the net's offset by the
    /// base's tear, which the bisections' residual counts (it was `0`).
    #[test]
    fn an_offset_cones_seam_snap_is_inside_its_residual() {
        use super::super::bisection_residual;
        use crate::surface::offset_surface::OffsetSurface;
        let k = Kernel::default();
        let mut c = Cone::new_bounded(
            Point3::ZERO,
            Vec3::X,
            Vec3::Y,
            12.5,
            12.5,
            0.0,
            360.0,
            3.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let base = Surface::from(c.clone());
        let d = c.rev.base.natural_uv_domain();
        let v = 0.5 * (d.v_interval().min() + d.v_interval().max());
        let uv = Point2::new(d.u_interval().max() - 5.0e-8, v);
        let tear = (base.evaluate_point(&k, uv).unwrap() - c.rev.base.evaluate_point(uv).unwrap())
            .length();
        assert!(tear > 1.0e-8, "the snap tears {tear:e}");
        let s = Surface::from(OffsetSurface::new(&k, 1.5, base.clone(), true, 0).unwrap());
        // (the offset's point is the base's plus the distance along the net's normal)
        let step =
            (s.evaluate_point(&k, uv).unwrap() - base.evaluate_point(&k, uv).unwrap()).length();
        assert!((step - 1.5).abs() < 1.0e-12, "the offset steps {step}");
        let net = NetSurface::of_exact(&k, &s, &[]).unwrap().unwrap();
        assert!(net.offset.is_some());
        let r = bisection_residual(&k, &s, &net, None).unwrap().unwrap();
        assert!(tear <= r, "{tear:e} over the residual {r:e}");
    }
}
