//! The setback chamfer's station certified over a span of its edge (not upstream): the feet
//! of [`super::setback_station`] enclosed for every s of a span by Krawczyk's operator over
//! Taylor models in s, the crossing condition decided on those enclosures, the exclusion that
//! makes the root the first crossing from the edge, and the chord's coupled system. Nothing
//! calls these yet; the setback tracer and certificate will.
//!
//! **The tube.** Over the span `[m − δ, m + δ]`, `s = m + δ t`, `t ∈ [−1, 1]`, a foot is the
//! root x of `F(x; s) = [(|S(x) − C(s)|² − d²)/2, (S(x) − C(s)) · C′(s)]`. Its tube is the
//! Rump-form Krawczyk proof of [`super::feet_tm::gn_enclose_tm`] in these unknowns: `p0(t)`
//! a polynomial close to the root (any polynomial is sound; here the cubic through point
//! roots at four Chebyshev nodes), `Y` the inverse of the Jacobian at the middle, and a box R
//! about `p0(t)` whose operator `D + C R` (`D` the range of `−Y F(p0(t), t)`, `C` that of
//! `I − Y F′(p0(t) + R, t)`) lies strictly inside R with `q = |C| < 1`: for every t, F has
//! exactly one root in `p0(t) + R`, and it lies in `p0(t) + D + C R`. The pieces are the
//! crate's one Krawczyk operator's ([`crate::geom::krawczyk`]'s `contraction`,
//! `start_minus_y`, `plus_c_times`) over `feet_tm`'s models (`Ft`), with the faces' partials
//! and the edge's derivatives from [`super::feet_tm::nurbs_partials_tm`] and
//! [`super::feet_tm::curve_derivs_tm`]. Those continue one polynomial piece, so a proof
//! stands only where the whole box the root is unique in lies in the face's piece and the
//! span in the edge's: elsewhere nothing is proved. Unlike `gn_enclose_tm`, no interval
//! enclosure of the root at the middle is checked against the tube: which root the tube holds
//! is the exclusion's to prove ([`first_crossing`]), not the tube's. The same tube serves the
//! edge's projection on a support ([`enclose_projection`]) and the chord's five unknowns
//! ([`enclose_chord`]).
//!
//! **The crossing condition** ([`crossing`]) is `(S − C) · C″ − |C′|²` enclosed over the tube:
//! below 0 for every s, the stations' planes do not cross before the tube's root; at or above 0
//! for every s, they cross at every station of the span. It is a statement about the tube's
//! root: it is about the foot where [`first_crossing`] holds for that tube.
//!
//! **The first crossing** ([`first_crossing`]). "From the edge" is read as from the edge's
//! projection P(s) on the support, the nearest point near the edge, enclosed by its own tube
//! (the edge lies on the support only within the model's tolerance, so the edge point itself
//! need not be a point of the support). In the support's (u, v), with `a` the parameter along
//! the edge (`S_a · C′` the larger at P) and `b` the one across, a box K runs in b from a
//! strip N holding P to past the foot's tube, on the side the foot lies; the side is proved to
//! be the face's (`S_b · into` of one sign over N). The section of the station's plane is the
//! zero set of `F_2 = (S − C) · C′`. Over K and the span, proved by interval enclosure on
//! cells of K and the span (each cell a box in u, v and s):
//! - **the section is one arc across K:** `∂F_2/∂a` has one sign on every cell, and `F_2` has
//!   opposite signs on K's two a-sides, so for each b exactly one a is on the section, a
//!   continuous function of b (one-sided partials of one sign keep it so across a crease);
//! - **every cell is one of:** holding no point of the section (`0 ∉ F_2`); inside the ball
//!   (`max |S − C|² < d²`, d's least value for an interval d); or, outside N, monotone (the
//!   foot system's `det J` of the one sign it has at the foot's tube). Along the section's
//!   tangent `τ = (−∂F_2/∂v, ∂F_2/∂u)`, `d/dλ (|S − C|²/2) = −det J`, so the distance is
//!   strictly monotone in monotone cells, and `τ`'s b component is `±∂F_2/∂a`, of one sign:
//!   one orientation along the arc.
//!
//! Then along the arc from N (inside the ball) the first point at distance d lies in a
//! monotone cell where the distance grows; past it the distance can only be in monotone cells
//! while it is at least d, where it keeps growing, so it never comes back to d. The foot's
//! tube lies in K on the section, so its root is that first point. This proves more than "no
//! nearer root" (monotone or inside, from N to the foot): it is sufficient, not necessary, and
//! `NotProved` is not "does not fit". The cells are split from a 4 × 8 grid in (a, b), the
//! longest side relative to K's (s included) first, within a budget of [`CELL_BUDGET`] cells.
//!
//! Deviations: not the fork's (no counterpart). Every loop has a fixed count (8 tube steps,
//! 200 steps of the radii's iteration as `gn_enclose_tm`, `NEWTON_STEPS` per point root, the
//! cells' budget), no recursion: the cells are a work list.

use super::certificate::{curve_derivs_over, nurbs_partials_over_box, NurbsCurve};
use super::feet_tm::{curve_derivs_tm, nurbs_partials_tm, Ft, FEET_TM_N};
use super::interval::{cmax, round_down, round_up, Ivl};
use super::series::{v_sub, V3};
use super::station::{knot, nurbs_partials, solve4, span, Nurbs, Partials};
use crate::error::{Result, SolidError};
use crate::geom::krawczyk::{
    contraction, inverse_of_columns2, inverse_of_columns5, newton, plus_c_times, start_minus_y,
    Linearised,
};
use crate::kernel::Kernel;

const OP: &str = "blend::fillet_section::setback_enclose";

const IZ: Ivl = Ivl::new(0.0, 0.0);

/// The most steps of the tube's Krawczyk test (as `gn_enclose_tm`'s).
const TUBE_STEPS: usize = 8;

/// The most cells the first-crossing proof classifies over one span.
pub const CELL_BUDGET: usize = 16384;

/// A system's Jacobian (rows) over a tube, or `None` where an evaluation proves nothing.
type Jacobian<const N: usize> = Result<Option<[[Ft; N]; N]>>;

/// A two-unknown system's value and columns at a point.
type Point2System = ([f64; 2], [[f64; 2]; 2]);

/// A two-unknown system's value and Jacobian (rows) over models.
type Model2System = ([Ft; 2], [[Ft; 2]; 2]);

/// A system's value and Jacobian over a tube.
type System<const N: usize> = Result<Option<([Ft; N], [[Ft; N]; N])>>;

/// What an enclosure decided about a condition over a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proof {
    /// Proved to hold at every station of the span.
    Holds,
    /// Proved to fail at every station of the span.
    Fails,
    /// Neither proved: the enclosure decides nothing (a shorter span may).
    NotProved,
}

/// A span of the edge: `s = m + δ t`, `t ∈ [−1, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    /// Its middle m.
    pub m: f64,
    /// Its half length δ, at least 0.
    pub delta: f64,
}

impl Span {
    /// `[m − δ, m + δ]`, rounded outward: it holds every `m + δ t`.
    fn interval(self) -> Ivl {
        Ivl::new(
            round_down(self.m - self.delta),
            round_up(self.m + self.delta),
        )
    }

    fn valid(self) -> bool {
        self.m.is_finite() && self.delta.is_finite() && self.delta >= 0.0
    }
}

/// A point of a support enclosed over a span (a foot, or the edge's projection): its (u, v)
/// and its point as Taylor models in t, the root's box, and the box it is the only root in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FootTube {
    /// The (u, v) for every t: `p0(t)` with the operator's remainder.
    pub uv: [Ft; 2],
    /// The point `S(u(t), v(t))`.
    pub point: V3<Ft>,
    /// The (u, v) box holding the root for every t.
    pub uv_box: [Ivl; 2],
    /// The hull over t of the boxes `p0(t) + R`: for each t the root is the only one in its
    /// own box `p0(t) + R` (not in this hull).
    pub unique_box: [Ivl; 2],
    /// The face's polynomial piece `(ku, kv)` the proof stands on.
    pub piece: [usize; 2],
    /// The span the tube was proved over.
    pub span: Span,
    /// The setback it was proved at (a foot's), or `None` (the edge's projection).
    pub setback: Option<Ivl>,
}

/// The chord's feet and setback enclosed over a span.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChordTube {
    /// The equal setback d for every t.
    pub d: Ft,
    /// The feet on supports 1 and 2.
    pub feet: [FootTube; 2],
}

// ---- small algebra ----------------------------------------------------------------------

fn dot(a: V3<Ft>, b: V3<Ft>) -> Ft {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn sub(a: V3<Ft>, b: V3<Ft>) -> V3<Ft> {
    V3 {
        x: a.x - b.x,
        y: a.y - b.y,
        z: a.z - b.z,
    }
}

fn dot_i(a: V3<Ivl>, b: V3<Ivl>) -> Ivl {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn f64_v(v: V3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `x.hi − x.lo` (not rounded: a cell's size only, never a bound).
fn width(x: Ivl) -> f64 {
    x.hi - x.lo
}

/// A model's range over the t interval `t` (inside `[−1, 1]`): `Σ c_j t^j + rem` in
/// intervals.
fn model_over(m: &Ft, t: Ivl) -> Ivl {
    let mut r = IZ;
    let mut pow = Ivl::from(1.0);
    for (j, &cj) in m.c.iter().enumerate() {
        if j > 0 {
            pow = pow * t;
        }
        r = r + cj * pow;
    }
    r + m.rem
}

fn lift(x: f64) -> Ft {
    Ft::constant(Ivl::from(x))
}

/// Whether `[lo, hi]` lies in the closed knot span `[u_k, u_{k+1}]`.
fn inside_span(u: &[f64], k: usize, lo: f64, hi: f64) -> bool {
    let (a, b) = (knot(u, k as isize), knot(u, k as isize + 1));
    a <= lo && hi <= b
}

/// A failed evaluation proves nothing (`Ok(None)`); a cancel ends the operation.
fn proved<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e @ SolidError::Cancelled { .. }) => Err(e),
        Err(_) => Ok(None),
    }
}

// ---- point evaluations (the polynomial p0 only: they prove nothing) ---------------------

/// The edge's `C, C′, C″` at s, as numbers.
fn edge_at(edge: &NurbsCurve, s: f64) -> Result<Option<[[f64; 3]; 3]>> {
    let Some(d) = proved(curve_derivs_over(edge, Ivl::from(s), 2, None))? else {
        return Ok(None);
    };
    let mid = |v: Option<&V3<Ivl>>| v.map(|v| [v.x.mid(), v.y.mid(), v.z.mid()]);
    let (Some(c0), Some(c1), Some(c2)) = (mid(d.first()), mid(d.get(1)), mid(d.get(2))) else {
        return Ok(None);
    };
    let all = [c0, c1, c2];
    Ok(all.iter().flatten().all(|x| x.is_finite()).then_some(all))
}

/// The face's partials at `x`, on the piece it lies in.
fn face_at(face: &Nurbs, x: [f64; 2], lk: usize) -> Partials<f64> {
    let ku = span(&face.u, face.p, face.nu, x[0]);
    let kv = span(&face.v, face.q, face.nv, x[1]);
    nurbs_partials::<f64>(face, ku, kv, x[0], x[1], lk)
}

/// The foot system at `x` for the edge `c` at setback d: its value and columns.
fn foot_point(face: &Nurbs, c: &[[f64; 3]; 3], d: f64, x: [f64; 2]) -> Point2System {
    let p = face_at(face, x, 1);
    let (s, su, sv) = (f64_v(p.at(0, 0)), f64_v(p.at(1, 0)), f64_v(p.at(0, 1)));
    let g = sub3(s, c[0]);
    let f = [0.5 * (dot3(g, g) - d * d), dot3(g, c[1])];
    let columns = [[dot3(g, su), dot3(su, c[1])], [dot3(g, sv), dot3(sv, c[1])]];
    (f, columns)
}

/// The projection system at `x` (`(S − C) · S_u = (S − C) · S_v = 0`): value and columns.
fn projection_point(face: &Nurbs, c: &[[f64; 3]; 3], x: [f64; 2]) -> Point2System {
    let p = face_at(face, x, 2);
    let (s, su, sv) = (f64_v(p.at(0, 0)), f64_v(p.at(1, 0)), f64_v(p.at(0, 1)));
    let (suu, suv, svv) = (f64_v(p.at(2, 0)), f64_v(p.at(1, 1)), f64_v(p.at(0, 2)));
    let g = sub3(s, c[0]);
    let f = [dot3(g, su), dot3(g, sv)];
    let a = dot3(su, su) + dot3(g, suu);
    let b = dot3(su, sv) + dot3(g, suv);
    let e = dot3(sv, sv) + dot3(g, svv);
    (f, [[a, b], [b, e]])
}

/// The four Chebyshev nodes in t.
fn nodes() -> [f64; FEET_TM_N] {
    core::array::from_fn(|i| ((2 * i + 1) as f64 * crate::math::PI / (2 * FEET_TM_N) as f64).cos())
}

/// `p0(t)`: the cubic in t through the roots `root(s_i, start)` at the Chebyshev nodes, each
/// continued from the last; constant where δ is 0. `None` where a root is not found.
fn chebyshev<const N: usize>(
    at: Span,
    start: [f64; N],
    root: &dyn Fn(f64, [f64; N]) -> Result<Option<[f64; N]>>,
) -> Result<Option<[Ft; N]>> {
    if at.delta == 0.0 {
        let Some(x) = root(at.m, start)? else {
            return Ok(None);
        };
        return Ok(Some(x.map(lift)));
    }
    let ts = nodes();
    let mut values = [[0.0_f64; FEET_TM_N]; N];
    let mut last = start;
    for (i, &t) in ts.iter().enumerate() {
        let Some(x) = root(at.m + at.delta * t, last)? else {
            return Ok(None);
        };
        for (vj, &xj) in values.iter_mut().zip(&x) {
            if let Some(v) = vj.get_mut(i) {
                *v = xj;
            }
        }
        last = x;
    }
    // the monomial coefficients: V c = x, V[i][j] = t_i^j
    let vander: [[f64; FEET_TM_N]; FEET_TM_N] = core::array::from_fn(|i| {
        core::array::from_fn(|j| ts.get(i).map_or(f64::NAN, |t| t.powi(j as i32)))
    });
    let mut out = [lift(0.0); N];
    for (o, v) in out.iter_mut().zip(&values) {
        let Some(c) = proved(solve4(&vander, v))? else {
            return Ok(None);
        };
        if !c.iter().all(|x| x.is_finite()) {
            return Ok(None);
        }
        *o = Ft::from_points(&c);
    }
    Ok(Some(out))
}

// ---- models -------------------------------------------------------------------------------

/// The edge's derivatives over the span as models, with its piece; `None` where the span is
/// not inside one piece of the edge or an evaluation proves nothing.
fn edge_models(edge: &NurbsCurve, at: Span) -> Result<Option<[V3<Ft>; 3]>> {
    let piece = span(&edge.u, edge.p, edge.n, at.m);
    let s = at.interval();
    if !inside_span(&edge.u, piece, s.lo, s.hi) {
        return Ok(None);
    }
    let Some(d) = proved(curve_derivs_tm(edge, at.m, at.delta, 2, piece))? else {
        return Ok(None);
    };
    let (Some(&c0), Some(&c1), Some(&c2)) = (d.first(), d.get(1), d.get(2)) else {
        return Ok(None);
    };
    Ok(Some([c0, c1, c2]))
}

/// The face's partials to order `lk` at the models `(u, v)` on the piece `(ku, kv)`.
fn face_models(
    face: &Nurbs,
    piece: [usize; 2],
    uv: [Ft; 2],
    lk: usize,
) -> Result<Option<Partials<Ft>>> {
    let [ku, kv] = piece;
    let [u, v] = uv;
    proved(nurbs_partials_tm(face, ku, kv, u, v, lk))
}

/// The foot system's value and Jacobian (rows) at the partials `p` and the edge `c`, the
/// setback `dd` a model.
fn foot_system(p: &Partials<Ft>, c: &[V3<Ft>; 3], dd: Ft) -> ([Ft; 2], [[Ft; 2]; 2]) {
    let [c0, c1, _] = *c;
    let g = sub(p.at(0, 0), c0);
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let f = [(dot(g, g) - dd * dd).scale(Ivl::from(0.5)), dot(g, c1)];
    let j = [[dot(g, su), dot(g, sv)], [dot(su, c1), dot(sv, c1)]];
    (f, j)
}

/// The projection system's value and Jacobian (rows).
fn projection_system(p: &Partials<Ft>, c: &[V3<Ft>; 3]) -> ([Ft; 2], [[Ft; 2]; 2]) {
    let g = sub(p.at(0, 0), c[0]);
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let f = [dot(g, su), dot(g, sv)];
    let a = dot(su, su) + dot(g, p.at(2, 0));
    let b = dot(su, sv) + dot(g, p.at(1, 1));
    let e = dot(sv, sv) + dot(g, p.at(0, 2));
    (f, [[a, b], [b, e]])
}

/// The Rump-form tube of `N` unknowns about the polynomial `p0` with the preconditioner `y`
/// (module docs): `g0` the system at `p0` (models in t), `jac` its Jacobian over a tube.
/// The root's models (`p0` with the operator's remainder) and the radii R of the box it is
/// the only root in, or `None` where nothing is proved.
fn tube<const N: usize>(
    p0: [Ft; N],
    y: &[[f64; N]; N],
    g0: &[Ft; N],
    jac: &mut dyn FnMut(&[Ft; N]) -> Jacobian<N>,
) -> Result<Option<([Ft; N], [f64; N])>> {
    let dr = start_minus_y([Ft::constant(IZ); N], y, g0).map(|d| d.range());
    let next_radii = |cr: &[[Ivl; N]; N], q: f64| -> Option<[f64; N]> {
        let contracts = q < 1.0;
        if !contracts {
            return None;
        }
        // K's radii are at most d + |C| w: w = (I - |C|)^-1 d holds them; a half more
        let d = dr.map(Ivl::mag);
        let mut w = d;
        for _ in 0..200 {
            let mut wn = [0.0_f64; N];
            for ((wni, &di), ci) in wn.iter_mut().zip(&d).zip(cr) {
                *wni = di;
                for (cij, &wj) in ci.iter().zip(&w) {
                    *wni += cij.mag() * wj;
                }
            }
            w = wn;
        }
        let mut rr = [0.0_f64; N];
        for ((ri, &wi), pc) in rr.iter_mut().zip(&w).zip(&p0) {
            let m0 = pc.c.first().map_or(f64::NAN, |c| c.mid());
            *ri = 1.5 * wi + 1.0e-13 * (1.0 + m0.abs());
        }
        rr.iter().all(|x| x.is_finite()).then_some(rr)
    };
    let Some(j0) = jac(&p0)? else {
        return Ok(None);
    };
    let (c0, q0) = contraction(y, &j0);
    let Some(mut rr) = next_radii(&c0.map(|row| row.map(|c| c.range())), q0) else {
        return Ok(None);
    };
    for _ in 0..TUBE_STEPS {
        let mut box_t = p0;
        for (tc, &ri) in box_t.iter_mut().zip(&rr) {
            *tc = tc.with_rem(Ivl::new(-ri, ri));
        }
        let Some(j) = jac(&box_t)? else {
            return Ok(None);
        };
        let (cm, q) = contraction(y, &j);
        let contracts = q < 1.0;
        if !contracts {
            return Ok(None);
        }
        let cr = cm.map(|row| row.map(|c| c.range()));
        let kk = plus_c_times(dr, &cr, &rr.map(|r| Ivl::new(-r, r)));
        let inside = kk.iter().zip(&rr).all(|(v, &ri)| v.lo > -ri && v.hi < ri);
        if inside {
            let mut x = p0;
            for ((xc, &ki), &ri) in x.iter_mut().zip(&kk).zip(&rr) {
                let lo = cmax(ki.lo, -ri);
                let hi = if ki.hi < ri { ki.hi } else { ri };
                *xc = xc.with_rem(Ivl::new(lo, hi));
            }
            return Ok(Some((x, rr)));
        }
        let Some(next) = next_radii(&cr, q) else {
            return Ok(None);
        };
        for (ri, &ni) in rr.iter_mut().zip(&next) {
            *ri = cmax(*ri, ni);
        }
    }
    Ok(None)
}

/// The tube's result as a [`FootTube`], refused (`None`) unless the box the root is unique in
/// lies in the face's piece.
fn foot_tube(
    face: &Nurbs,
    piece: [usize; 2],
    x: [Ft; 2],
    rr: [f64; 2],
    p0: [Ft; 2],
    proved_at: (Span, Option<Ivl>),
) -> Result<Option<FootTube>> {
    let unique_box: [Ivl; 2] = core::array::from_fn(|i| {
        let p = p0.get(i).map_or_else(Ivl::whole, Ft::range);
        let r = rr.get(i).copied().unwrap_or(f64::INFINITY);
        p + Ivl::new(-r, r)
    });
    let [bu, bv] = unique_box;
    if !(inside_span(&face.u, piece[0], bu.lo, bu.hi)
        && inside_span(&face.v, piece[1], bv.lo, bv.hi))
    {
        return Ok(None);
    }
    let Some(p) = face_models(face, piece, x, 0)? else {
        return Ok(None);
    };
    Ok(Some(FootTube {
        uv: x,
        point: p.at(0, 0),
        uv_box: x.map(|m| m.range()),
        unique_box,
        piece,
        span: proved_at.0,
        setback: proved_at.1,
    }))
}

/// A two-unknown tube on `face` from the point roots `root` (from `x_m` at the middle), the
/// system `system` over models: its preconditioner from `columns` at the middle's root.
fn tube2(
    face: &Nurbs,
    at: Span,
    x_m: [f64; 2],
    root: &dyn Fn(f64, [f64; 2]) -> Result<Option<[f64; 2]>>,
    columns: &dyn Fn([f64; 2]) -> Result<Option<[[f64; 2]; 2]>>,
    system: &dyn Fn(&Partials<Ft>) -> Model2System,
    lk: usize,
    setback: Option<Ivl>,
) -> Result<Option<FootTube>> {
    let Some(mid) = root(at.m, x_m)? else {
        return Ok(None);
    };
    let Some(cols) = columns(mid)? else {
        return Ok(None);
    };
    let Some(y) = inverse_of_columns2(&cols) else {
        return Ok(None);
    };
    let Some(p0) = chebyshev(at, mid, root)? else {
        return Ok(None);
    };
    let piece = [
        span(&face.u, face.p, face.nu, mid[0]),
        span(&face.v, face.q, face.nv, mid[1]),
    ];
    let Some(pz) = face_models(face, piece, p0, lk)? else {
        return Ok(None);
    };
    let (g0, _) = system(&pz);
    let mut jac = |x: &[Ft; 2]| -> Jacobian<2> {
        Ok(face_models(face, piece, *x, lk)?.map(|p| system(&p).1))
    };
    let Some((x, rr)) = tube(p0, &y, &g0, &mut jac)? else {
        return Ok(None);
    };
    foot_tube(face, piece, x, rr, p0, (at, setback))
}

/// The foot at setback `d` on `face` over the span `at`, enclosed (module docs), its point
/// roots continued from `x_m` (near the foot at the middle, as
/// [`super::setback_station::setback_foot`] finds it). `None` where nothing is proved: a
/// point root is not found, the tube does not contract, or it or the span leaves the pieces
/// its models continue.
///
/// # Errors
///
/// A cancel.
pub fn enclose_foot(
    k: &Kernel,
    face: &Nurbs,
    edge: &NurbsCurve,
    d: f64,
    at: Span,
    x_m: [f64; 2],
) -> Result<Option<FootTube>> {
    k.check_cancelled(OP)?;
    if !(d.is_finite() && d > 0.0 && at.valid()) {
        return Ok(None);
    }
    let Some(cm) = edge_models(edge, at)? else {
        return Ok(None);
    };
    let root = |s: f64, start: [f64; 2]| -> Result<Option<[f64; 2]>> {
        let Some(c) = edge_at(edge, s)? else {
            return Ok(None);
        };
        newton(
            start,
            &|x| Ok(Some(foot_point(face, &c, d, x))),
            &inverse_of_columns2,
        )
    };
    let columns = |x: [f64; 2]| -> Result<Option<[[f64; 2]; 2]>> {
        let Some(c) = edge_at(edge, at.m)? else {
            return Ok(None);
        };
        Ok(Some(foot_point(face, &c, d, x).1))
    };
    let dd = lift(d);
    let system = |p: &Partials<Ft>| foot_system(p, &cm, dd);
    tube2(
        face,
        at,
        x_m,
        &root,
        &columns,
        &system,
        1,
        Some(Ivl::from(d)),
    )
}

/// The edge's projection P(s) on `face` over the span `at` (`(S − C) · S_u = (S − C) · S_v =
/// 0`), enclosed as a foot is, from `x_m` near it at the middle (the edge's (u, v) on the
/// support). `None` where nothing is proved.
///
/// # Errors
///
/// A cancel.
pub fn enclose_projection(
    k: &Kernel,
    face: &Nurbs,
    edge: &NurbsCurve,
    at: Span,
    x_m: [f64; 2],
) -> Result<Option<FootTube>> {
    k.check_cancelled(OP)?;
    if !at.valid() {
        return Ok(None);
    }
    let Some(cm) = edge_models(edge, at)? else {
        return Ok(None);
    };
    let root = |s: f64, start: [f64; 2]| -> Result<Option<[f64; 2]>> {
        let Some(c) = edge_at(edge, s)? else {
            return Ok(None);
        };
        newton(
            start,
            &|x| Ok(Some(projection_point(face, &c, x))),
            &inverse_of_columns2,
        )
    };
    let columns = |x: [f64; 2]| -> Result<Option<[[f64; 2]; 2]>> {
        let Some(c) = edge_at(edge, at.m)? else {
            return Ok(None);
        };
        Ok(Some(projection_point(face, &c, x).1))
    };
    let system = |p: &Partials<Ft>| projection_system(p, &cm);
    tube2(face, at, x_m, &root, &columns, &system, 2, None)
}

/// Whether the stations' planes stay clear of the tube's root over the span (module docs):
/// the range of `(S − C) · C″ − |C′|²` over the tube. It is about the foot where
/// [`first_crossing`] holds for the same tube.
///
/// # Errors
///
/// A cancel.
pub fn crossing(k: &Kernel, edge: &NurbsCurve, foot: &FootTube, at: Span) -> Result<Proof> {
    k.check_cancelled(OP)?;
    if !(at.valid() && foot.span == at) {
        return Ok(Proof::NotProved);
    }
    let Some([c0, c1, c2]) = edge_models(edge, at)? else {
        return Ok(Proof::NotProved);
    };
    let g = sub(foot.point, c0);
    let e = (dot(g, c2) - dot(c1, c1)).range();
    Ok(if e.hi < 0.0 {
        Proof::Holds
    } else if e.lo >= 0.0 {
        Proof::Fails
    } else {
        Proof::NotProved
    })
}

// ---- the first crossing ------------------------------------------------------------------

/// A cell of the first-crossing proof: `[u, v, s]` intervals.
type Cell = [Ivl; 3];

/// What the first-crossing proof reads over a cell.
struct CellValues {
    /// `F_2 = (S − C) · C′`.
    f2: Ivl,
    /// `∂F_2/∂a`.
    f2a: Ivl,
    /// `|S − C|²`.
    dist2: Ivl,
    /// The foot system's `det J`.
    det: Ivl,
    /// `S_b`.
    sb: V3<Ivl>,
}

/// The cell's values; `None` where the face or the edge cannot be enclosed there (past its
/// knots).
fn cell_values(
    face: &Nurbs,
    edge: &NurbsCurve,
    cell: &Cell,
    a_is_u: bool,
) -> Result<Option<CellValues>> {
    let [u, v, s] = *cell;
    let Some(cd) = proved(curve_derivs_over(edge, s, 1, None))? else {
        return Ok(None);
    };
    let (Some(&c0), Some(&c1)) = (cd.first(), cd.get(1)) else {
        return Ok(None);
    };
    let Some(p) = proved(nurbs_partials_over_box(face, u.lo, u.hi, v.lo, v.hi, 1, 0))? else {
        return Ok(None);
    };
    let g = v_sub(p.at(0, 0), c0);
    let (su, sv) = (p.at(1, 0), p.at(0, 1));
    let (sa, sb) = if a_is_u { (su, sv) } else { (sv, su) };
    let (gu, gv, uc, vc) = (dot_i(g, su), dot_i(g, sv), dot_i(su, c1), dot_i(sv, c1));
    Ok(Some(CellValues {
        f2: dot_i(g, c1),
        f2a: dot_i(sa, c1),
        dist2: dot_i(g, g),
        det: gu * vc - gv * uc,
        sb,
    }))
}

/// Whether `x` is of the strict sign `sign`.
fn of_sign(x: Ivl, sign: f64) -> bool {
    if sign > 0.0 {
        x.lo > 0.0
    } else {
        x.hi < 0.0
    }
}

/// The strict sign of `x`, or `None` where it holds 0 (or is not a number).
fn sign_of(x: Ivl) -> Option<f64> {
    if x.lo > 0.0 {
        Some(1.0)
    } else if x.hi < 0.0 {
        Some(-1.0)
    } else {
        None
    }
}

/// A cell split in two along its longest side relative to `scale` (the whole box's widths),
/// or `None` where every side is below `1e-9` of its scale.
fn halves(cell: &Cell, scale: &[f64; 3]) -> Option<[Cell; 2]> {
    let mut best = (0.0_f64, 0usize);
    for (i, (c, &sc)) in cell.iter().zip(scale).enumerate() {
        let rel = width(*c) / sc;
        if rel > best.0 {
            best = (rel, i);
        }
    }
    let (rel, i) = best;
    let splittable = rel >= 1e-9;
    if !splittable {
        return None;
    }
    let side = *cell.get(i)?;
    let m = 0.5 * (side.lo + side.hi);
    let (mut a, mut b) = (*cell, *cell);
    *a.get_mut(i)? = Ivl::new(side.lo, m);
    *b.get_mut(i)? = Ivl::new(m, side.hi);
    Some([a, b])
}

/// Proves `holds` on every cell of `start`'s grid, splitting a cell that fails, within the
/// budget `spent` counts against. `false` where a cell fails at the smallest size or the
/// budget is spent.
fn every_cell(
    k: &Kernel,
    start: Vec<Cell>,
    scale: &[f64; 3],
    spent: &mut usize,
    holds: &mut dyn FnMut(&Cell) -> Result<bool>,
) -> Result<bool> {
    let mut work = start;
    while let Some(cell) = work.pop() {
        *spent += 1;
        if *spent > CELL_BUDGET {
            return Ok(false);
        }
        if *spent % 64 == 0 {
            k.check_cancelled(OP)?;
        }
        if holds(&cell)? {
            continue;
        }
        let Some([a, b]) = halves(&cell, scale) else {
            return Ok(false);
        };
        work.push(a);
        work.push(b);
    }
    Ok(true)
}

/// The first-crossing proof (module docs) for the foot `foot` at the setback `d` (an
/// interval: the chord's d over its span) on `face`, from the edge's projection `edge_proj`
/// on the same face, the face on the side of `into` (a vector from the edge into it).
/// [`Proof::Holds`] where it is proved for every s of the span; [`Proof::NotProved`]
/// otherwise (never `Fails`: no proof of a nearer root is attempted).
///
/// # Errors
///
/// A cancel.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn first_crossing(
    k: &Kernel,
    face: &Nurbs,
    edge: &NurbsCurve,
    d: Ivl,
    at: Span,
    foot: &FootTube,
    edge_proj: &FootTube,
    into: [f64; 3],
) -> Result<Proof> {
    k.check_cancelled(OP)?;
    let not = Ok(Proof::NotProved);
    let matches = foot.span == at
        && edge_proj.span == at
        && foot.setback == Some(d)
        && edge_proj.setback.is_none();
    if !(matches
        && at.valid()
        && d.lo > 0.0
        && d.hi.is_finite()
        && into.iter().all(|x| x.is_finite()))
    {
        return not;
    }
    let span_s = at.interval();
    // d's least square, rounded down
    let d2 = (Ivl::from(d.lo) * Ivl::from(d.lo)).lo;
    // the projection is near the edge: inside the ball for every s
    let Some([e0, _, _]) = edge_models(edge, at)? else {
        return not;
    };
    let gp = sub(edge_proj.point, e0);
    let near = dot(gp, gp).range().hi < d2;
    if !near {
        return not;
    }
    // the axes: a along the edge (the larger |S_a . C'| at the projection's middle)
    let pm = [edge_proj.uv_box[0].mid(), edge_proj.uv_box[1].mid()];
    let fm = [foot.uv_box[0].mid(), foot.uv_box[1].mid()];
    let Some([_, c1m, _]) = edge_at(edge, at.m)? else {
        return not;
    };
    let p = face_at(face, pm, 1);
    let (uc, vc) = (dot3(f64_v(p.at(1, 0)), c1m), dot3(f64_v(p.at(0, 1)), c1m));
    let a_is_u = uc.abs() >= vc.abs();
    // b's lengths in a's units, for K's margin in a
    let (nu, nv) = (
        dot3(f64_v(p.at(1, 0)), f64_v(p.at(1, 0))).sqrt(),
        dot3(f64_v(p.at(0, 1)), f64_v(p.at(0, 1))).sqrt(),
    );
    let b_in_a = if a_is_u { nv / nu } else { nu / nv };
    if !(b_in_a.is_finite() && b_in_a > 0.0) {
        return not;
    }
    let (ia, ib) = if a_is_u { (0, 1) } else { (1, 0) };
    let pick = |b: &[Ivl; 2], i: usize| b.get(i).copied().unwrap_or_else(Ivl::whole);
    let (pa, pb) = (pick(&edge_proj.uv_box, ia), pick(&edge_proj.uv_box, ib));
    let (fa, fb) = (pick(&foot.unique_box, ia), pick(&foot.unique_box, ib));
    // the side: b runs from the projection toward the foot
    let (pbm, fbm) = (
        pm.get(ib).copied().unwrap_or(f64::NAN),
        fm.get(ib).copied().unwrap_or(f64::NAN),
    );
    let Some(sb_dir) = sign_of(Ivl::from(fbm - pbm)) else {
        return not;
    };
    let length = (fbm - pbm).abs();
    // the strip N: the projection's b range, widened a little away from the foot
    let eta = 0.02 * length + width(pb);
    let (n_lo, n_hi, b_lo, b_hi) = if sb_dir > 0.0 {
        (pb.lo - eta, pb.hi, pb.lo - eta, fb.hi)
    } else {
        (pb.lo, pb.hi + eta, fb.lo, pb.hi + eta)
    };
    // the foot past N, on the far side
    let beyond = if sb_dir > 0.0 {
        fb.lo > n_hi
    } else {
        fb.hi < n_lo
    };
    if !(beyond && b_lo < b_hi) {
        return not;
    }
    // the strip at a cell's s: the projection's b there, widened away from the foot by eta
    let proj_b = edge_proj.uv.get(ib).copied();
    let strip_at = |sc: Ivl| -> Option<Ivl> {
        // the cell's t, rounded outward, within the models' [-1, 1]
        let t = if at.delta > 0.0 {
            (sc - Ivl::from(at.m)) / Ivl::from(at.delta)
        } else {
            Ivl::from(0.0)
        };
        let t = Ivl::new(t.lo.max(-1.0), t.hi.min(1.0));
        let ordered = t.lo <= t.hi;
        if !ordered {
            return None;
        }
        let b = model_over(&proj_b?, t);
        let strip = if sb_dir > 0.0 {
            Ivl::new(b.lo - eta, b.hi)
        } else {
            Ivl::new(b.lo, b.hi + eta)
        };
        // the hull strip holds every s's strip; within it, the per-cell one is narrower
        Some(Ivl::new(strip.lo.max(n_lo), strip.hi.min(n_hi)))
    };
    // within the face's domain in b (the projection is in it)
    let (b_first, b_last) = if a_is_u {
        (
            knot(&face.v, face.q as isize),
            knot(&face.v, face.nv as isize),
        )
    } else {
        (
            knot(&face.u, face.p as isize),
            knot(&face.u, face.nu as isize),
        )
    };
    let (n_lo, n_hi, b_lo, b_hi) = (
        n_lo.max(b_first),
        n_hi.min(b_last),
        b_lo.max(b_first),
        b_hi.min(b_last),
    );
    let n = Ivl::new(n_lo, n_hi);
    let cell_of = |a: Ivl, b: Ivl, s: Ivl| -> Cell {
        if a_is_u {
            [a, b, s]
        } else {
            [b, a, s]
        }
    };
    // the signs: F_2's along a and det J's, at the foot's box
    let Some(at_foot) = cell_values(face, edge, &cell_of(fa, fb, span_s), a_is_u)? else {
        return not;
    };
    let (Some(sa), Some(sj)) = (sign_of(at_foot.f2a), sign_of(at_foot.det)) else {
        return not;
    };
    let into_i = V3 {
        x: Ivl::from(into[0]),
        y: Ivl::from(into[1]),
        z: Ivl::from(into[2]),
    };
    let within = |x: Ivl, y: Ivl| y.lo <= x.lo && x.hi <= y.hi;
    let mut spent = 0usize;
    // K's a range: both boxes and a margin; wider tries where a side does not hold
    for widen in [0.25, 1.0, 4.0] {
        // (the boxes already hull the span's motion; the margin is the section's wander)
        let w = widen * length * b_in_a;
        let ka = Ivl::new(pa.lo.min(fa.lo) - w, pa.hi.max(fa.hi) + w);
        let kb = Ivl::new(b_lo, b_hi);
        if !(within(fa, ka) && within(fb, kb) && within(pa, ka) && within(pb, n)) {
            return not;
        }
        let whole = cell_of(ka, kb, span_s);
        let scale = whole.map(width);
        if !scale.iter().all(|x| x.is_finite() && *x >= 0.0) {
            return not;
        }
        let scale = scale.map(|x| if x > 0.0 { x } else { 1.0 });
        // the face's side over N: S_b . into of the sign b runs toward the foot
        let mut side = |cell: &Cell| -> Result<bool> {
            Ok(cell_values(face, edge, cell, a_is_u)?
                .is_some_and(|v| of_sign(dot_i(v.sb, into_i), sb_dir)))
        };
        if !every_cell(
            k,
            vec![cell_of(ka, n, span_s)],
            &scale,
            &mut spent,
            &mut side,
        )? {
            // (a wider K does not change the strip: nothing more to try)
            return not;
        }
        // the a-sides: F_2 of sign -sa at K's low a and sa at its high a, for every b and s
        let mut sides_hold = true;
        for (a_side, want) in [(ka.lo, -sa), (ka.hi, sa)] {
            let mut edge_side = |cell: &Cell| -> Result<bool> {
                Ok(cell_values(face, edge, cell, a_is_u)?.is_some_and(|v| of_sign(v.f2, want)))
            };
            let start = vec![cell_of(Ivl::from(a_side), kb, span_s)];
            if !every_cell(k, start, &scale, &mut spent, &mut edge_side)? {
                sides_hold = false;
                break;
            }
        }
        if !sides_hold {
            continue;
        }
        // the cells
        let mut start = Vec::new();
        for i in 0..4 {
            for j in 0..8 {
                // the grid's ends exactly K's, so the cells cover it
                let part = |x: Ivl, i: i32, n: i32| {
                    let at = |j: i32| {
                        if j == 0 {
                            x.lo
                        } else if j == n {
                            x.hi
                        } else {
                            x.lo + width(x) * f64::from(j) / f64::from(n)
                        }
                    };
                    Ivl::new(at(i), at(i + 1))
                };
                start.push(cell_of(part(ka, i, 4), part(kb, j, 8), span_s));
            }
        }
        let mut classified = |cell: &Cell| -> Result<bool> {
            let b = cell.get(ib).copied().unwrap_or_else(Ivl::whole);
            let sc = cell.get(2).copied().unwrap_or_else(Ivl::whole);
            let Some(strip) = strip_at(sc) else {
                return Ok(false);
            };
            let in_n = b.lo < strip.hi && b.hi > strip.lo;
            // wholly past the strip, away from the foot: before the arc's start, only the
            // graph matters there
            let before = if sb_dir > 0.0 {
                b.hi <= strip.lo
            } else {
                b.lo >= strip.hi
            };
            Ok(cell_values(face, edge, cell, a_is_u)?.is_some_and(|v| {
                let graph = of_sign(v.f2a, sa);
                let no_section = v.f2.lo > 0.0 || v.f2.hi < 0.0;
                let inside = v.dist2.hi < d2;
                let monotone = !in_n && of_sign(v.det, sj);
                graph && (before || no_section || inside || monotone)
            }))
        };
        let all = every_cell(k, start, &scale, &mut spent, &mut classified)?;
        return Ok(if all { Proof::Holds } else { Proof::NotProved });
    }
    not
}

/// The chord `chord` over the span `at` (module docs, five unknowns: both feet's (u, v) and
/// the equal setback d), its point roots continued from `x_m` (the feet's (u, v)) and `d_m`
/// at the middle (as [`super::setback_station::chord_setback`] finds them). `None` where
/// nothing is proved.
///
/// # Errors
///
/// A cancel.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn enclose_chord(
    k: &Kernel,
    faces: [&Nurbs; 2],
    edge: &NurbsCurve,
    chord: f64,
    at: Span,
    x_m: [[f64; 2]; 2],
    d_m: f64,
) -> Result<Option<ChordTube>> {
    k.check_cancelled(OP)?;
    if !(chord.is_finite() && chord > 0.0 && d_m.is_finite() && d_m > 0.0 && at.valid()) {
        return Ok(None);
    }
    let Some(cm) = edge_models(edge, at)? else {
        return Ok(None);
    };
    let [f1, f2] = faces;
    // the point system: value and columns
    let point = |c: &[[f64; 3]; 3], x: [f64; 5]| -> Linearised<5> {
        let [u1, v1, u2, v2, d] = x;
        let p1 = face_at(f1, [u1, v1], 1);
        let p2 = face_at(f2, [u2, v2], 1);
        let (s1, s1u, s1v) = (f64_v(p1.at(0, 0)), f64_v(p1.at(1, 0)), f64_v(p1.at(0, 1)));
        let (s2, s2u, s2v) = (f64_v(p2.at(0, 0)), f64_v(p2.at(1, 0)), f64_v(p2.at(0, 1)));
        let (g1, g2, e) = (sub3(s1, c[0]), sub3(s2, c[0]), sub3(s1, s2));
        let f = [
            0.5 * (dot3(g1, g1) - d * d),
            dot3(g1, c[1]),
            0.5 * (dot3(g2, g2) - d * d),
            dot3(g2, c[1]),
            dot3(e, e) - chord * chord,
        ];
        let rows = [
            [dot3(g1, s1u), dot3(g1, s1v), 0.0, 0.0, -d],
            [dot3(s1u, c[1]), dot3(s1v, c[1]), 0.0, 0.0, 0.0],
            [0.0, 0.0, dot3(g2, s2u), dot3(g2, s2v), -d],
            [0.0, 0.0, dot3(s2u, c[1]), dot3(s2v, c[1]), 0.0],
            [
                2.0 * dot3(e, s1u),
                2.0 * dot3(e, s1v),
                -2.0 * dot3(e, s2u),
                -2.0 * dot3(e, s2v),
                0.0,
            ],
        ];
        let columns: [[f64; 5]; 5] = core::array::from_fn(|j| {
            core::array::from_fn(|i| rows.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0))
        });
        Some((f, columns))
    };
    let root = |s: f64, start: [f64; 5]| -> Result<Option<[f64; 5]>> {
        let Some(c) = edge_at(edge, s)? else {
            return Ok(None);
        };
        newton(start, &|x| Ok(point(&c, x)), &inverse_of_columns5)
    };
    let start = [x_m[0][0], x_m[0][1], x_m[1][0], x_m[1][1], d_m];
    let Some(mid) = root(at.m, start)? else {
        return Ok(None);
    };
    let Some(cmid) = edge_at(edge, at.m)? else {
        return Ok(None);
    };
    let Some((_, cols)) = point(&cmid, mid) else {
        return Ok(None);
    };
    let Some(y) = inverse_of_columns5(&cols) else {
        return Ok(None);
    };
    let Some(p0) = chebyshev(at, mid, &root)? else {
        return Ok(None);
    };
    let [m1u, m1v, m2u, m2v, _] = mid;
    let piece = [
        [span(&f1.u, f1.p, f1.nu, m1u), span(&f1.v, f1.q, f1.nv, m1v)],
        [span(&f2.u, f2.p, f2.nu, m2u), span(&f2.v, f2.q, f2.nv, m2v)],
    ];
    let system = |x: &[Ft; 5]| -> System<5> {
        let [u1, v1, u2, v2, dd] = *x;
        let (Some(p1), Some(p2)) = (
            face_models(f1, piece[0], [u1, v1], 1)?,
            face_models(f2, piece[1], [u2, v2], 1)?,
        ) else {
            return Ok(None);
        };
        let (a, ja) = foot_system(&p1, &cm, dd);
        let (b, jb) = foot_system(&p2, &cm, dd);
        let e = sub(p1.at(0, 0), p2.at(0, 0));
        let two = Ivl::from(2.0);
        let z = lift(0.0);
        let minus_d = z - dd;
        let [[a00, a01], [a10, a11]] = ja;
        let [[b00, b01], [b10, b11]] = jb;
        let f = [
            a[0],
            a[1],
            b[0],
            b[1],
            dot(e, e) - lift(chord) * lift(chord),
        ];
        let j = [
            [a00, a01, z, z, minus_d],
            [a10, a11, z, z, z],
            [z, z, b00, b01, minus_d],
            [z, z, b10, b11, z],
            [
                dot(e, p1.at(1, 0)).scale(two),
                dot(e, p1.at(0, 1)).scale(two),
                (z - dot(e, p2.at(1, 0))).scale(two),
                (z - dot(e, p2.at(0, 1))).scale(two),
                z,
            ],
        ];
        Ok(Some((f, j)))
    };
    let Some((g0, _)) = system(&p0)? else {
        return Ok(None);
    };
    let mut jac = |x: &[Ft; 5]| -> Jacobian<5> { Ok(system(x)?.map(|s| s.1)) };
    let Some((x, rr)) = tube(p0, &y, &g0, &mut jac)? else {
        return Ok(None);
    };
    let [u1, v1, u2, v2, d] = x;
    let [r1, r2, r3, r4, _] = rr;
    let [q1, q2, q3, q4, _] = p0;
    let proved_at = (at, Some(d.range()));
    let (Some(t1), Some(t2)) = (
        foot_tube(f1, piece[0], [u1, v1], [r1, r2], [q1, q2], proved_at)?,
        foot_tube(f2, piece[1], [u2, v2], [r3, r4], [q3, q4], proved_at)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(ChordTube { d, feet: [t1, t2] }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[allow(clippy::type_complexity, clippy::needless_range_loop)]
mod tests {
    //! The enclosures against closed forms: the hemisphere rim (ball 1, flat 2), a hole of
    //! radius 5 chamfered 6 (on the plate and the wall: built), a boss of radius 5 chamfered
    //! inward 6 on its top (the stations cross: refused) and 4 (built), and a line on a
    //! cylinder at the cylinder's radius, whose far root is not the first crossing.
    use super::*;
    use crate::blend::fillet_section::station::as_b_spline;
    use crate::geom::axis2_placement::Axis2Placement;
    use crate::geom::extent2d::Extent2d;
    use crate::math::{Point3, Vec3};
    use crate::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::surface::cone::Cone;
    use crate::surface::sphere::{Sphere, SphereOptions};
    use crate::surface::surface::Surface;

    const R: f64 = 5.0;

    fn plane() -> Nurbs {
        let b = BSplineSurface::create_bilinear_surface(
            Point3::new(-20.0, -20.0, 0.0),
            Point3::new(20.0, -20.0, 0.0),
            Point3::new(-20.0, 20.0, 0.0),
            Point3::new(20.0, 20.0, 0.0),
        )
        .unwrap();
        Nurbs::of(&b).unwrap()
    }

    fn nurbs_of(k: &Kernel, s: &Surface) -> Nurbs {
        Nurbs::of(&as_b_spline(k, s).unwrap().unwrap()).unwrap()
    }

    fn ball(k: &Kernel) -> Nurbs {
        let mut sp = Sphere::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, -30.0, 90.0, 40.0),
            R,
            SphereOptions::default(),
        )
        .unwrap();
        sp.make_nurb(k).unwrap();
        nurbs_of(k, &Surface::from(sp))
    }

    /// The cylinder of radius 5 about z, from `z0` up `height`, its angle from `from` round
    /// (its knot lines every 90 degrees from there).
    fn cylinder(k: &Kernel, z0: f64, height: f64, from: f64) -> Nurbs {
        let mut c = Cone::new_cylinder_bounded(
            Point3::new(0.0, 0.0, z0),
            Vec3::X,
            Vec3::Y,
            R,
            from,
            from + 360.0,
            height,
            false,
            false,
            false,
        );
        c.make_nurb(k).unwrap();
        nurbs_of(k, &Surface::from(c))
    }

    /// The rim circle of radius 5 at z = 0, from 0 to 90 degrees.
    fn rim(to: f64) -> NurbsCurve {
        let frame = Axis2Placement::new(Point3::ZERO, Vec3::X, Vec3::Y);
        let c =
            BSplineCurve::create_circle_segment(3, &frame, R, 0.0, to, NurbCircleParam::Quadratic)
                .unwrap();
        NurbsCurve::of(&c).unwrap()
    }

    /// A span in the middle of the edge's first piece, `frac` of it long.
    fn span_of(edge: &NurbsCurve, frac: f64) -> Span {
        let (a, b) = (
            knot(&edge.u, edge.p as isize),
            knot(&edge.u, edge.p as isize + 1),
        );
        Span {
            m: 0.5 * (a + b),
            delta: 0.5 * frac * (b - a),
        }
    }

    /// The (u, v) of `p` on `face` (a test's inversion: a grid, then Gauss-Newton).
    fn uv_of(face: &Nurbs, p: [f64; 3]) -> [f64; 2] {
        let (u0, u1) = (
            knot(&face.u, face.p as isize),
            knot(&face.u, face.nu as isize),
        );
        let (v0, v1) = (
            knot(&face.v, face.q as isize),
            knot(&face.v, face.nv as isize),
        );
        let mut best = (f64::INFINITY, [u0, v0]);
        for i in 0..=60 {
            for j in 0..=60 {
                let x = [
                    u0 + (u1 - u0) * f64::from(i) / 60.0,
                    v0 + (v1 - v0) * f64::from(j) / 60.0,
                ];
                let q = f64_v(face_at(face, x, 0).at(0, 0));
                let e = dot3(sub3(q, p), sub3(q, p));
                if e < best.0 {
                    best = (e, x);
                }
            }
        }
        let mut x = best.1;
        for _ in 0..60 {
            let pf = face_at(face, x, 1);
            let (q, su, sv) = (f64_v(pf.at(0, 0)), f64_v(pf.at(1, 0)), f64_v(pf.at(0, 1)));
            let r = sub3(p, q);
            let g = [[dot3(su, su), dot3(su, sv)], [dot3(su, sv), dot3(sv, sv)]];
            let inv = inverse_of_columns2(&g).unwrap();
            let b = [dot3(su, r), dot3(sv, r)];
            x = [
                x[0] + inv[0][0] * b[0] + inv[0][1] * b[1],
                x[1] + inv[1][0] * b[0] + inv[1][1] * b[1],
            ];
        }
        let q = f64_v(face_at(face, x, 0).at(0, 0));
        assert!(
            dot3(sub3(q, p), sub3(q, p)).sqrt() < 1e-10,
            "{p:?} not on the face"
        );
        x
    }

    fn holds(b: V3<Ivl>, p: [f64; 3]) -> bool {
        [b.x, b.y, b.z]
            .iter()
            .zip(p)
            .all(|(i, x)| i.lo <= x && x <= i.hi)
    }

    fn widest(b: V3<Ivl>) -> f64 {
        [b.x, b.y, b.z]
            .iter()
            .map(|i| width(*i))
            .fold(0.0, f64::max)
    }

    /// The point of a model vector at t.
    fn at_t(v: V3<Ft>, t: f64) -> V3<Ivl> {
        V3 {
            x: v.x.at(t),
            y: v.y.at(t),
            z: v.z.at(t),
        }
    }

    /// The edge's point at s, and its unit radial direction.
    fn radial(edge: &NurbsCurve, s: f64) -> ([f64; 3], [f64; 3]) {
        let c = edge_at(edge, s).unwrap().unwrap()[0];
        let l = dot3(c, c).sqrt();
        (c, [c[0] / l, c[1] / l, 0.0])
    }

    /// Everything a foot gets: the tube, the edge's projection, the crossing and the first
    /// crossing.
    struct Station {
        foot: FootTube,
        crossing: Proof,
        first: Proof,
    }

    fn station(
        k: &Kernel,
        face: &Nurbs,
        edge: &NurbsCurve,
        d: f64,
        at: Span,
        foot_guess: [f64; 3],
        into: [f64; 3],
    ) -> Station {
        let (c, _) = radial(edge, at.m);
        let foot = enclose_foot(k, face, edge, d, at, uv_of(face, foot_guess))
            .unwrap()
            .unwrap();
        let proj = enclose_projection(k, face, edge, at, uv_of(face, c))
            .unwrap()
            .unwrap();
        let crossing = crossing(k, edge, &foot, at).unwrap();
        let first = first_crossing(k, face, edge, Ivl::from(d), at, &foot, &proj, into).unwrap();
        Station {
            foot,
            crossing,
            first,
        }
    }

    /// The hemisphere of radius 5 at 1 on the ball and 2 on the flat: the tubes hold the
    /// closed-form feet (radius 4.9 at height 3 sqrt(11) / 10; radius 3) at every t, tightly;
    /// the crossing and the first crossing hold on both.
    #[test]
    fn a_hemisphere_rim_s_tubes_hold_its_closed_form_feet() {
        let k = Kernel::default();
        let edge = rim(90.0);
        let (fl, bl) = (plane(), ball(&k));
        let at = span_of(&edge, 0.002);
        let h = 3.0 * 11.0_f64.sqrt() / 10.0;
        let (_, r_m) = radial(&edge, at.m);
        let cases: [(&Nurbs, f64, [f64; 3], fn([f64; 3], f64) -> [f64; 3]); 2] = [
            (&fl, 2.0, [-r_m[0], -r_m[1], 0.0], |r, _| {
                [3.0 * r[0], 3.0 * r[1], 0.0]
            }),
            (&bl, 1.0, [0.0, 0.0, 1.0], |r, h| {
                [4.9 * r[0], 4.9 * r[1], h]
            }),
        ];
        for (face, d, into, closed) in cases {
            let guess = closed(r_m, h);
            let st = station(&k, face, &edge, d, at, guess, into);
            for t in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let (_, r) = radial(&edge, at.m + at.delta * t);
                let want = closed(r, h);
                let b = at_t(st.foot.point, t);
                assert!(holds(b, want), "d {d} t {t}: {b:?} {want:?}");
                assert!(widest(b) < 1e-9, "d {d} t {t}: {b:?}");
            }
            assert_eq!(st.crossing, Proof::Holds, "d {d}");
            assert_eq!(st.first, Proof::Holds, "d {d}");
            // a setback 1e-6 longer: its tube no longer holds the foot at d
            let longer = enclose_foot(&k, face, &edge, d + 1e-6, at, uv_of(face, guess))
                .unwrap()
                .unwrap();
            let (_, r) = radial(&edge, at.m);
            assert!(!holds(at_t(longer.point, 0.0), closed(r, h)), "d {d}");
        }
    }

    /// A span across a knot of the edge proves nothing: the models continue one piece.
    #[test]
    fn a_span_across_the_edge_s_knot_proves_nothing() {
        let k = Kernel::default();
        let edge = rim(180.0);
        let joint = knot(&edge.u, edge.p as isize + 1);
        assert!(
            joint < knot(&edge.u, edge.n as isize),
            "a circle of 180 degrees has two pieces"
        );
        let at = Span {
            m: joint,
            delta: 0.01,
        };
        let fl = plane();
        let (_, r) = radial(&edge, at.m);
        let x = uv_of(&fl, [3.0 * r[0], 3.0 * r[1], 0.0]);
        assert!(enclose_foot(&k, &fl, &edge, 2.0, at, x).unwrap().is_none());
        let inside = Span {
            m: joint - 0.02,
            delta: 0.01,
        };
        assert!(enclose_foot(&k, &fl, &edge, 2.0, inside, x)
            .unwrap()
            .is_some());
    }

    /// A hole of radius 5 in a plate z = 0, chamfered 6: on the plate the foot is at radius
    /// 11, on the hole's wall 6 below the rim. Neither crosses (the feet are on the convex
    /// side of the rim, or along its axis), and both are first crossings: it is built.
    #[test]
    fn a_chamfer_of_6_around_a_hole_of_radius_5_is_not_refused() {
        let k = Kernel::default();
        let edge = rim(90.0);
        let at = span_of(&edge, 0.2);
        let (_, r) = radial(&edge, at.m);
        let plate = station(
            &k,
            &plane(),
            &edge,
            6.0,
            at,
            [11.0 * r[0], 11.0 * r[1], 0.0],
            r,
        );
        assert_eq!((plate.crossing, plate.first), (Proof::Holds, Proof::Holds));
        let wall = cylinder(&k, -20.0, 25.0, 0.0);
        let on_wall = station(
            &k,
            &wall,
            &edge,
            6.0,
            at,
            [R * r[0], R * r[1], -6.0],
            [0.0, 0.0, -1.0],
        );
        assert_eq!(
            (on_wall.crossing, on_wall.first),
            (Proof::Holds, Proof::Holds)
        );
        for t in [-1.0, 0.0, 1.0] {
            let (_, rt) = radial(&edge, at.m + at.delta * t);
            assert!(holds(
                at_t(plate.foot.point, t),
                [11.0 * rt[0], 11.0 * rt[1], 0.0]
            ));
            assert!(holds(
                at_t(on_wall.foot.point, t),
                [R * rt[0], R * rt[1], -6.0]
            ));
        }
    }

    /// A boss of radius 5 chamfered inward on its top: at 6 the foot is past the axis, where
    /// the stations' planes cross ((S − C) · C″ / |C′|² = 6 · 5 / 25 = 1.2): refused. At 4 it
    /// is 0.8: built.
    #[test]
    fn a_chamfer_past_the_axis_of_a_boss_crosses() {
        let k = Kernel::default();
        let edge = rim(90.0);
        let at = span_of(&edge, 0.2);
        let (_, r) = radial(&edge, at.m);
        let inward = [-r[0], -r[1], 0.0];
        let past = station(&k, &plane(), &edge, 6.0, at, [-r[0], -r[1], 0.0], inward);
        assert_eq!(past.first, Proof::Holds);
        assert_eq!(past.crossing, Proof::Fails);
        let short = station(&k, &plane(), &edge, 4.0, at, [r[0], r[1], 0.0], inward);
        assert_eq!((short.crossing, short.first), (Proof::Holds, Proof::Holds));
    }

    /// A line on a cylinder of radius 5 along its axis, setback 5: the section is the circle,
    /// at distance `10 sin(θ/2)` from the edge, d at 60 and 300 degrees. The one at 60 is
    /// the first crossing; the one at 300 is not (the distance passes 10 at 180 and comes
    /// back), and its proof must not hold.
    #[test]
    fn only_the_first_root_on_a_cylinder_is_the_first_crossing() {
        let k = Kernel::default();
        let cyl = cylinder(&k, -10.0, 20.0, -45.0);
        let line = BSplineCurve::create_line_segment(
            3,
            Point3::new(R, 0.0, -2.0),
            Point3::new(R, 0.0, 2.0),
        )
        .unwrap();
        let edge = NurbsCurve::of(&line).unwrap();
        let at = span_of(&edge, 0.1);
        let c = edge_at(&edge, at.m).unwrap().unwrap()[0];
        let proj = enclose_projection(&k, &cyl, &edge, at, uv_of(&cyl, c))
            .unwrap()
            .unwrap();
        let into = [0.0, 1.0, 0.0];
        let backward = [0.0, -1.0, 0.0];
        for (deg, want) in [(60.0_f64, Proof::Holds), (300.0, Proof::NotProved)] {
            let a = deg.to_radians();
            let guess = [R * a.cos(), R * a.sin(), c[2]];
            let foot = enclose_foot(&k, &cyl, &edge, R, at, uv_of(&cyl, guess))
                .unwrap()
                .unwrap();
            assert!(
                holds(at_t(foot.point, 0.0), guess),
                "{deg}: {:?}",
                foot.point
            );
            let got =
                first_crossing(&k, &cyl, &edge, Ivl::from(R), at, &foot, &proj, into).unwrap();
            assert_eq!(got, want, "{deg}");
            // the face on the other side of the edge: not this root's
            let other =
                first_crossing(&k, &cyl, &edge, Ivl::from(R), at, &foot, &proj, backward).unwrap();
            assert_eq!(other, Proof::NotProved, "{deg} backward");
            // a tube of another span, or another setback, proves nothing here
            let shifted = Span {
                m: at.m + 0.1 * at.delta,
                ..at
            };
            assert_eq!(
                first_crossing(&k, &cyl, &edge, Ivl::from(R), shifted, &foot, &proj, into).unwrap(),
                Proof::NotProved
            );
            assert_eq!(
                first_crossing(&k, &cyl, &edge, Ivl::from(R + 0.1), at, &foot, &proj, into)
                    .unwrap(),
                Proof::NotProved
            );
            assert_eq!(
                crossing(&k, &edge, &foot, shifted).unwrap(),
                Proof::NotProved
            );
        }
    }

    /// The chord on the hemisphere rim: its tube holds the equal setback d with
    /// `c² = 2 d² − d³ / 5`, and both feet's closed forms.
    #[test]
    fn a_chord_s_tube_on_the_hemisphere_rim() {
        let k = Kernel::default();
        let edge = rim(90.0);
        let (fl, bl) = (plane(), ball(&k));
        let at = span_of(&edge, 0.002);
        let (_, r) = radial(&edge, at.m);
        let d = 1.5_f64;
        let c = (2.0 * d * d - d * d * d / R).sqrt();
        let beta = (0.5 * d / R).asin();
        let ball_foot = |r: [f64; 3]| {
            let rad = R - d * beta.sin();
            [rad * r[0], rad * r[1], d * beta.cos()]
        };
        let x_m = [
            uv_of(&fl, [(R - d) * r[0], (R - d) * r[1], 0.0]),
            uv_of(&bl, ball_foot(r)),
        ];
        let tube = enclose_chord(&k, [&fl, &bl], &edge, c, at, x_m, d)
            .unwrap()
            .unwrap();
        for t in [-1.0, 0.0, 1.0] {
            let di = tube.d.at(t);
            assert!(di.lo <= d && d <= di.hi && width(di) < 1e-9, "{t}: {di:?}");
            let (_, rt) = radial(&edge, at.m + at.delta * t);
            assert!(holds(
                at_t(tube.feet[0].point, t),
                [(R - d) * rt[0], (R - d) * rt[1], 0.0]
            ));
            assert!(holds(at_t(tube.feet[1].point, t), ball_foot(rt)));
        }
        // the first crossing of each foot, its d the tube's
        let dr = tube.d.range();
        for (face, foot, into) in [
            (&fl, &tube.feet[0], [-r[0], -r[1], 0.0]),
            (&bl, &tube.feet[1], [0.0, 0.0, 1.0]),
        ] {
            let cm = edge_at(&edge, at.m).unwrap().unwrap()[0];
            let proj = enclose_projection(&k, face, &edge, at, uv_of(face, cm))
                .unwrap()
                .unwrap();
            assert_eq!(
                first_crossing(&k, face, &edge, dr, at, foot, &proj, into).unwrap(),
                Proof::Holds
            );
        }
    }

    /// The 5 x 5 preconditioner inverts.
    #[test]
    fn the_five_by_five_inverse_inverts() {
        let cols: [[f64; 5]; 5] = core::array::from_fn(|j| {
            core::array::from_fn(|i| {
                if i == j {
                    2.0
                } else {
                    0.1 * (i + 2 * j) as f64
                }
            })
        });
        let inv = inverse_of_columns5(&cols).unwrap();
        for i in 0..5 {
            for j in 0..5 {
                let s: f64 = (0..5).map(|m| inv[i][m] * cols[j][m]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((s - want).abs() < 1e-13, "{i} {j}: {s}");
            }
        }
        assert!(inverse_of_columns5(&[[0.0; 5]; 5]).is_none());
        // the anti-diagonal: every column needs a row swap
        let anti: [[f64; 5]; 5] = core::array::from_fn(|j| {
            core::array::from_fn(|i| if i + j == 4 { 1.0 + j as f64 } else { 0.0 })
        });
        let inv = inverse_of_columns5(&anti).unwrap();
        for i in 0..5 {
            for j in 0..5 {
                let s: f64 = (0..5).map(|m| inv[i][m] * anti[j][m]).sum();
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((s - want).abs() < 1e-14, "{i} {j}: {s}");
            }
        }
    }
}
