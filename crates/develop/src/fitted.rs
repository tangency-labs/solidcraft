//! A free-form panel's flattening, fitted in its own surface's spline space.
//!
//! The flat map F(u, v) is a planar spline over the panel's surface's own knots, refined where it
//! must be. The surface's parameter lines therefore carry to the flat exactly, and so does
//! anything drawn in its parameters. The flat answers to the neutral surface's metric, its first
//! fundamental form I - 2hII + h^2 III, with h the depth of the neutral surface below the sheet.
//!
//! A developable neutral surface is developed exactly. The flat's frame turns at rates its metric
//! fixes (F_u and F_v must agree across F_uv = F_vu), so the flat follows from integrals along one
//! row of parameters and up and down every column from it, taken by Gauss collocation of order
//! ten, and F interpolates those points one direction at a time. Interpolation is all the error
//! left, and it falls with the span size to the power of F's degree. On any other surface the
//! integrals keep the stretch of the lines they were taken along, and F is solved for instead:
//! least stretch over its space, by damped Gauss-Newton on the metric with a band Cholesky
//! solve. What stretch is left is the flat's cost, which is reported and never forced.

use solidcraft_geom::bspline::{NurbsSurface, bernstein};
use solidcraft_geom::spline_space::{Cond, Interpolant, Space, embed};
use solidcraft_geom::{Seg2, Vec2, Vec3};

/// The stretch a fitted flat may leave, as a strain: refinement stops below it.
pub(crate) const STRETCH_TOL: f64 = 1e-7;

/// The most samples one level of a fit is measured at: a work limit.
const MAX_SAMPLES: usize = 1 << 20;

/// The most multiply-adds one Gauss-Newton factorization may take: a work limit (what a dense
/// solve of 1,600 unknowns took).
const MAX_BAND_WORK: f64 = 2.7e9;

/// How many times the fit's space is refined at most.
const MAX_REFINEMENTS: usize = 7;

/// Damped Gauss-Newton steps per fit.
const MAX_STEPS: usize = 40;

/// A developed flat whose stretch falls by less than this factor in a refinement is not
/// converging, so its surface is not developable (interpolation of degree d falls by about 2^d).
const STALL: f64 = 0.5;

/// How far a finer level may still move a solved flat for the flat to count as resolved, and ten
/// times how far a Gauss-Newton step may move it for the solve to count as converged: the 1e-6 a
/// sheet's flats are held to.
const RESOLVED: f64 = 1e-6;

/// The highest derivative order [`partials_from`] takes.
const MAX_ORDER: usize = 3;

/// A surface's partial derivatives at a point: element `[k][l]` is its k-th derivative in u of
/// its l-th in v.
type Partials = [[Vec3; MAX_ORDER + 1]; MAX_ORDER + 1];

/// The basis functions of `sp` alive at `t` with their derivatives up to `order`: for each order,
/// the index of the first and the values.
fn basis_rows(sp: &Space, t: f64, order: usize) -> Option<Vec<(usize, Vec<f64>)>> {
    (0..=order).map(|d| sp.basis(t, d).ok()).collect()
}

/// A spline surface's partial derivatives up to total `order` (at most [`MAX_ORDER`]) at a
/// point, from the basis functions alive there in each direction ([`basis_rows`]).
fn partials_from(s: &NurbsSurface, bu: &[(usize, Vec<f64>)], bv: &[(usize, Vec<f64>)], order: usize) -> Option<Partials> {
    let order = order.min(MAX_ORDER);
    let [nu, _] = s.count();
    // The homogeneous sum for each pair of derivative orders.
    let mut a = [[Vec3::ZERO; MAX_ORDER + 1]; MAX_ORDER + 1];
    let mut w = [[0.0; MAX_ORDER + 1]; MAX_ORDER + 1];
    for k in 0..=order {
        for l in 0..=order - k {
            let ((iu, wu), (iv, wv)) = (bu.get(k)?, bv.get(l)?);
            for (j, bj) in wv.iter().enumerate() {
                for (i, bi) in wu.iter().enumerate() {
                    let c = s.homogeneous().get((iv + j) * nu + iu + i)?;
                    let x = bi * bj;
                    a[k][l] += c.xyz() * x;
                    w[k][l] += c.w * x;
                }
            }
        }
    }
    if !(w[0][0].abs() > f64::MIN_POSITIVE) {
        return None;
    }
    let binomial = |n: usize, k: usize| (0..k).fold(1.0, |b, i| b * (n - i) as f64 / (i + 1) as f64);
    // The quotient rule (Piegl and Tiller, algorithm A4.4).
    let mut out = [[Vec3::ZERO; MAX_ORDER + 1]; MAX_ORDER + 1];
    for k in 0..=order {
        for l in 0..=order - k {
            let mut x = a[k][l];
            for j in 1..=l {
                x -= out[k][l - j] * (binomial(l, j) * w[0][j]);
            }
            for i in 1..=k {
                x -= out[k - i][l] * (binomial(k, i) * w[i][0]);
                for j in 1..=l {
                    x -= out[k - i][l - j] * (binomial(k, i) * binomial(l, j) * w[i][j]);
                }
            }
            out[k][l] = x * (1.0 / w[0][0]);
        }
    }
    Some(out)
}

/// The neutral surface's unit normal toward the material, depth below the sheet, and first
/// fundamental form (E, F, G) at a point whose derivatives are `d`. The material lies on the
/// side `Su x Sv` points to; the neutral surface lies K x T from the inside of the bend there,
/// which is the material side where the surface curves toward it.
fn neutral(d: &[Vec3; 6], thickness: f64, k_factor: f64) -> Option<(Vec3, f64, [f64; 3])> {
    let [_, su, sv, suu, suv, svv] = *d;
    let n = su.cross(sv).normalized()?;
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, m, nn) = (suu.dot(n), suv.dot(n), svv.dot(n));
    let det = e * g - f * f;
    if !(det > 0.0) {
        return None;
    }
    let mean = (e * nn - 2.0 * f * m + g * l) / (2.0 * det);
    let h = if mean > 0.0 { (1.0 - k_factor) * thickness } else { k_factor * thickness };
    // III = II I^-1 II.
    let (i00, i01, i11) = (g / det, -f / det, e / det);
    let (a00, a01, a10, a11) = (l * i00 + m * i01, l * i01 + m * i11, m * i00 + nn * i01, m * i01 + nn * i11);
    let (t00, t01, t11) = (a00 * l + a01 * m, a00 * m + a01 * nn, a10 * m + a11 * nn);
    Some((n, h, [e - 2.0 * h * l + h * h * t00, f - 2.0 * h * m + h * h * t01, g - 2.0 * h * nn + h * h * t11]))
}

/// What the development needs at one point: the rates at which the flat's frame turns along u
/// and along v, the neutral surface's derivatives N_u and N_v as the frame holds them (N_u along
/// its x axis), and the neutral depth there.
struct Frame {
    turn: [f64; 2],
    a: Vec2,
    b: Vec2,
    depth: f64,
}

/// The development's data at `(u, v)` of the spline surface `s`. In the frame, F_u = R(t) a and
/// F_v = R(t) b, with |a|^2, a.b and |b|^2 the neutral metric; F_uv = F_vu then fixes the turn t's
/// rates, t_v J a - t_u J b = b_u - a_v with J the quarter turn. They need the metric's first
/// derivatives, so the neutral surface's second, so the surface's third. `bu` and `bv` are the
/// basis functions alive at the point ([`basis_rows`] to third order).
fn frame(s: &NurbsSurface, bu: &[(usize, Vec<f64>)], bv: &[(usize, Vec<f64>)], thickness: f64, k_factor: f64) -> Option<Frame> {
    let d = partials_from(s, bu, bv, 3)?;
    let at = |k: usize, l: usize| d.get(k).and_then(|r| r.get(l)).copied();
    let (su, sv, suu, suv, svv) = (at(1, 0)?, at(0, 1)?, at(2, 0)?, at(1, 1)?, at(0, 2)?);
    let (suuu, suuv, suvv) = (at(3, 0)?, at(2, 1)?, at(1, 2)?);
    let (_, h, _) = neutral(&[at(0, 0)?, su, sv, suu, suv, svv], thickness, k_factor)?;
    // The unit normal n = m / r with m = Su x Sv, and its derivatives.
    let m = su.cross(sv);
    let r = m.len();
    if !(r > 0.0) {
        return None;
    }
    let n = m * (1.0 / r);
    let mu = suu.cross(sv) + su.cross(suv);
    let mv = suv.cross(sv) + su.cross(svv);
    let muu = suuu.cross(sv) + suu.cross(suv) * 2.0 + su.cross(suuv);
    let muv = suuv.cross(sv) + suu.cross(svv) + su.cross(suvv);
    let (ru, rv) = (n.dot(mu), n.dot(mv));
    let (ruu, ruv) = ((mu.dot(mu) + m.dot(muu) - ru * ru) / r, (mu.dot(mv) + m.dot(muv) - ru * rv) / r);
    let nu = (mu - n * ru) * (1.0 / r);
    let nv = (mv - n * rv) * (1.0 / r);
    let nuu = (muu - nu * (2.0 * ru) - n * ruu) * (1.0 / r);
    let nuv = (muv - nu * rv - nv * ru - n * ruv) * (1.0 / r);
    // The neutral surface S + h n: its first derivatives and the second ones the rates need.
    let (xu, xv, xuu, xuv) = (su + nu * h, sv + nv * h, suu + nuu * h, suv + nuv * h);
    let (e, f, g) = (xu.dot(xu), xu.dot(xv), xv.dot(xv));
    let (eu, ev, fu, gu) = (2.0 * xu.dot(xuu), 2.0 * xu.dot(xuv), xuu.dot(xv) + xu.dot(xuv), 2.0 * xv.dot(xuv));
    let det = e * g - f * f;
    if !(e > 0.0 && det > 0.0) {
        return None;
    }
    // a = (ax, 0) and b = (bx, by); a's derivative along v and b's along u.
    let ax = e.sqrt();
    let (bx, by) = (f / ax, (det / e).sqrt());
    let ax_v = ev / (2.0 * ax);
    let bx_u = fu / ax - f * eu / (2.0 * e * ax);
    let by_u = ((eu * g + e * gu - 2.0 * f * fu) * e - det * eu) / (2.0 * e * e * by);
    let turn_u = (bx_u - ax_v) / by;
    let turn_v = (by_u + bx * turn_u) / ax;
    Some(Frame { turn: [turn_u, turn_v], a: Vec2::new(ax, 0.0), b: Vec2::new(bx, by), depth: h })
}

/// Gauss-Legendre collocation with five stages, of order ten: its nodes and weights on [0, 1],
/// and its stage matrix (row i: the weights that integrate from 0 to node i).
struct Collocation {
    c: [f64; 5],
    w: [f64; 5],
    a: [[f64; 5]; 5],
}

impl Collocation {
    fn new() -> Collocation {
        let (x1, x2) = ((5.0 + 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0, (5.0 - 2.0 * (10.0f64 / 7.0).sqrt()).sqrt() / 3.0);
        let (w1, w2) = ((322.0 - 13.0 * 70.0f64.sqrt()) / 900.0, (322.0 + 13.0 * 70.0f64.sqrt()) / 900.0);
        let c = [(1.0 - x1) / 2.0, (1.0 - x2) / 2.0, 0.5, (1.0 + x2) / 2.0, (1.0 + x1) / 2.0];
        let w = [w1 / 2.0, w2 / 2.0, 64.0 / 225.0, w2 / 2.0, w1 / 2.0];
        // Each Lagrange polynomial of the nodes integrated from 0 to each node, by the rule itself
        // on [0, c_i] (exact: the polynomials have degree four).
        let lagrange = |j: usize, t: f64| (0..5).filter(|&k| k != j).fold(1.0, |p, k| p * (t - c[k]) / (c[j] - c[k]));
        let a = std::array::from_fn(|i| std::array::from_fn(|j| c[i] * (0..5).map(|m| w[m] * lagrange(j, c[i] * c[m])).sum::<f64>()));
        Collocation { c, w, a }
    }
}

/// The rates along one parameter line at a parameter: the frame's turn rate, the derivative it
/// turns, and the neutral depth.
type Rate<'a> = dyn Fn(f64) -> Option<(f64, Vec2, f64)> + 'a;

/// One parameter line's development, from `from` (parameter, turn, point) on through `targets`
/// (in order away from it), its steps cut at `breaks`, where the rates are not smooth: the turn
/// and point at each target.
fn carry(rate: &Rate, from: (f64, f64, Vec2), targets: &[f64], breaks: &[f64], gl: &Collocation) -> Option<Vec<(f64, Vec2)>> {
    let (mut t, mut turn, mut at) = from;
    let mut out = Vec::with_capacity(targets.len());
    for &target in targets {
        let mut cuts: Vec<f64> = breaks.iter().copied().filter(|&b| (b - t) * (target - b) > 0.0).collect();
        cuts.sort_by(|x, y| (x - t).abs().total_cmp(&(y - t).abs()));
        cuts.push(target);
        for c in cuts {
            (turn, at) = step(rate, t, c, turn, at, gl, 4)?;
            t = c;
        }
        out.push((turn, at));
    }
    Some(out)
}

/// One collocation step from `t0` to `t1`, split where the neutral surface changes side (its
/// depth, and with it the rates, jump there), at most `splits` times over.
fn step(rate: &Rate, t0: f64, t1: f64, turn: f64, at: Vec2, gl: &Collocation, splits: usize) -> Option<(f64, Vec2)> {
    let h = t1 - t0;
    if h == 0.0 {
        return Some((turn, at));
    }
    let stages: Vec<(f64, Vec2, f64)> = gl.c.iter().map(|c| rate(t0 + c * h)).collect::<Option<_>>()?;
    if splits > 0
        && let Some(k) = (0..4).find(|&k| stages.get(k).map(|s| s.2) != stages.get(k + 1).map(|s| s.2))
    {
        // Bisected between the two nodes either side of the change.
        let side = stages.get(k)?.2;
        let (mut lo, mut hi) = (t0 + gl.c[k] * h, t0 + gl.c[k + 1] * h);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if mid == lo || mid == hi {
                break;
            }
            match rate(mid) {
                Some((_, _, d)) if d == side => lo = mid,
                _ => hi = mid,
            }
        }
        let x = 0.5 * (lo + hi);
        let (turn, at) = step(rate, t0, x, turn, at, gl, splits - 1)?;
        return step(rate, x, t1, turn, at, gl, splits - 1);
    }
    let mut moved = Vec2::ZERO;
    for (i, &(_, d, _)) in stages.iter().enumerate() {
        let turned = turn + h * stages.iter().zip(&gl.a[i]).map(|(s, a)| a * s.0).sum::<f64>();
        moved += rotated(d, turned) * gl.w[i];
    }
    let turn = turn + h * stages.iter().zip(&gl.w).map(|(s, w)| w * s.0).sum::<f64>();
    Some((turn, at + moved * h))
}

/// `v` turned by `angle`.
fn rotated(v: Vec2, angle: f64) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// The spline surface `s`'s neutral surface developed at the Greville points of `spaces`, along
/// their middle row and then up and down every column, and interpolated there one direction at a
/// time: F's control points, and how far F's derivatives miss the development's at those points
/// along u and along v, as strains. The flat starts at the parameters' first corner, its u
/// direction along +x there.
fn developed(s: &NurbsSurface, s_spaces: &[Space; 2], spaces: &[Space; 2], thickness: f64, k_factor: f64) -> Option<(Vec<Vec2>, [f64; 2])> {
    let gl = Collocation::new();
    let greville = |sp: &Space| -> Option<Vec<f64>> {
        let d = sp.degree();
        (0..sp.count()).map(|i| sp.knots().get(i + 1..=i + d).map(|k| k.iter().sum::<f64>() / d as f64)).collect()
    };
    let breaks = |sp: &Space| {
        let mut b = sp.knots().to_vec();
        b.dedup();
        b
    };
    let (gu, gv) = (greville(&spaces[0])?, greville(&spaces[1])?);
    let (bu, bv) = (breaks(&s_spaces[0]), breaks(&s_spaces[1]));
    let (nu, nv) = (gu.len(), gv.len());
    let mid = nv / 2;
    let base = *gv.get(mid)?;
    // Along a line one direction's basis functions stay as they are.
    let at_base = basis_rows(&s_spaces[1], base, 3)?;
    let along_u = |u: f64| frame(s, &basis_rows(&s_spaces[0], u, 3)?, &at_base, thickness, k_factor).map(|f| (f.turn[0], f.a, f.depth));
    let (&u0, rest) = gu.split_first()?;
    let mut row = vec![(0.0, Vec2::ZERO)];
    row.extend(carry(&along_u, (u0, 0.0, Vec2::ZERO), rest, &bu, &gl)?);
    let mut grid = vec![(0.0, Vec2::ZERO); nu * nv];
    let down: Vec<f64> = gv.get(..mid)?.iter().rev().copied().collect();
    for (i, (&u, &(turn, at))) in gu.iter().zip(&row).enumerate() {
        let at_u = basis_rows(&s_spaces[0], u, 3)?;
        let along_v = |v: f64| frame(s, &at_u, &basis_rows(&s_spaces[1], v, 3)?, thickness, k_factor).map(|f| (f.turn[1], f.b, f.depth));
        *grid.get_mut(mid * nu + i)? = (turn, at);
        for (k, p) in carry(&along_v, (base, turn, at), gv.get(mid + 1..)?, &bv, &gl)?.into_iter().enumerate() {
            *grid.get_mut((mid + 1 + k) * nu + i)? = p;
        }
        for (k, p) in carry(&along_v, (base, turn, at), &down, &bv, &gl)?.into_iter().enumerate() {
            *grid.get_mut((mid - 1 - k) * nu + i)? = p;
        }
    }
    let &(turn0, at0) = grid.first()?;
    let pts: Vec<Vec3> = grid
        .iter()
        .map(|&(_, p)| {
            let q = rotated(p - at0, -turn0);
            Vec3::new(q.x, q.y, 0.0)
        })
        .collect();
    let along = |sp: &Space, g: &[f64]| Interpolant::new(sp.clone(), g.iter().map(|&t| Cond { t, der: 0 }).collect()).ok();
    let (fit_u, fit_v) = (along(&spaces[0], &gu)?, along(&spaces[1], &gv)?);
    let rows: Vec<Vec<Vec3>> = (0..nv).map(|j| fit_u.fit(pts.get(j * nu..(j + 1) * nu)?).ok()).collect::<Option<_>>()?;
    let mut ctrl = vec![Vec2::ZERO; nu * nv];
    for i in 0..nu {
        let col: Vec<Vec3> = rows.iter().map(|r| r.get(i).copied()).collect::<Option<_>>()?;
        for (j, c) in fit_v.fit(&col).ok()?.into_iter().enumerate() {
            *ctrl.get_mut(j * nu + i)? = Vec2::new(c.x, c.y);
        }
    }
    // The development's derivatives at the points are N_u and N_v turned through the frame's turn
    // there.
    let metric = neutral_metrics(s, s_spaces, &gu, &gv, thickness, k_factor)?;
    let mut short = [0.0f64; 2];
    for ((&(turn, _), &[e, f, g]), (fu, fv)) in grid.iter().zip(&metric).zip(Grid::new(spaces, &gu, &gv)?.derivatives(&ctrl)) {
        let ax = e.sqrt();
        let (a, b) = (Vec2::new(ax, 0.0), Vec2::new(f / ax, ((e * g - f * f) / e).sqrt()));
        short[0] = short[0].max((fu - rotated(a, turn - turn0)).len() / ax);
        short[1] = short[1].max((fv - rotated(b, turn - turn0)).len() / g.sqrt());
    }
    Some((ctrl, short))
}

/// A planar spline F(u, v) over a tensor space: the flat a free-form panel's parameters map to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FlatMap {
    spaces: [Space; 2],
    /// Control points, u fastest.
    ctrl: Vec<Vec2>,
    /// The largest strain left between F's metric and the neutral surface's.
    pub(crate) stretch: f64,
}

impl FlatMap {
    /// F and its first derivatives at `(u, v)`.
    fn eval(&self, u: f64, v: f64) -> Option<[Vec2; 3]> {
        let nu = self.spaces[0].count();
        let ((iu, b0u), (_, b1u)) = (self.spaces[0].basis(u, 0).ok()?, self.spaces[0].basis(u, 1).ok()?);
        let ((iv, b0v), (_, b1v)) = (self.spaces[1].basis(v, 0).ok()?, self.spaces[1].basis(v, 1).ok()?);
        let mut out = [Vec2::ZERO; 3];
        for j in 0..b0v.len() {
            for i in 0..b0u.len() {
                let c = *self.ctrl.get((iv + j) * nu + iu + i)?;
                let (bu, bv, du, dv) = (*b0u.get(i)?, *b0v.get(j)?, *b1u.get(i)?, *b1v.get(j)?);
                out[0] += c * (bu * bv);
                out[1] += c * (du * bv);
                out[2] += c * (bu * dv);
            }
        }
        Some(out)
    }

    pub(crate) fn point(&self, p: Vec2) -> Vec2 {
        self.eval(p.x, p.y).map_or(p, |e| e[0])
    }

    pub(crate) fn dir(&self, p: Vec2, v: Vec2) -> Option<Vec2> {
        let [_, fu, fv] = self.eval(p.x, p.y)?;
        (fu * v.x + fv * v.y).normalized()
    }

    /// A straight segment of the parameters flattened: F along a line is a polynomial of the
    /// spaces' degrees added (those of the directions it moves in) on each piece between knot
    /// lines, so it comes back exactly as Bezier pieces. Higher than a segment can hold, it is
    /// followed by chords.
    pub(crate) fn line(&self, a: Vec2, b: Vec2, tol: f64, out: &mut Vec<Seg2>) {
        let mut cuts = vec![0.0, 1.0];
        for (axis, (x0, x1)) in [(a.x, b.x), (a.y, b.y)].into_iter().enumerate() {
            if (x1 - x0).abs() > 0.0 {
                for &k in self.spaces[axis].knots() {
                    let t = (k - x0) / (x1 - x0);
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-14);
        let degree = (if a.x != b.x { self.spaces[0].degree() } else { 0 }) + (if a.y != b.y { self.spaces[1].degree() } else { 0 });
        let at = |t: f64| self.point(a.lerp(b, t));
        for w in cuts.windows(2) {
            let [t0, t1] = [w[0], w[1]];
            match bezier_through(degree, |s| at(t0 + (t1 - t0) * s)) {
                Some(seg) => out.push(seg),
                None => {
                    let n = ((a.dist(b) * (t1 - t0) / tol).sqrt().ceil() as usize).clamp(2, 4096);
                    let pts: Vec<Vec2> = (0..=n).map(|k| at(t0 + (t1 - t0) * k as f64 / n as f64)).collect();
                    out.extend(pts.windows(2).filter_map(|p| match p {
                        [p, q] => Some(Seg2::Line { a: *p, b: *q }),
                        _ => None,
                    }));
                }
            }
        }
    }
}

/// The Bezier of `degree` through the polynomial curve `f` on [0, 1], as the segment kind that
/// holds it: found from `degree + 1` points, which a polynomial of that degree is fixed by.
fn bezier_through(degree: usize, f: impl Fn(f64) -> Vec2) -> Option<Seg2> {
    if degree == 0 || degree > solidcraft_geom::MAX_BEZIER_DEGREE {
        return None;
    }
    let n = degree + 1;
    let ts: Vec<f64> = (0..n).map(|k| k as f64 / degree as f64).collect();
    let mut m: Vec<f64> = ts.iter().flat_map(|&t| (0..n).map(move |i| bernstein(degree, i, t))).collect();
    let mut rhs: Vec<Vec2> = ts.iter().map(|&t| f(t)).collect();
    solve_dense(&mut m, &mut rhs, n)?;
    Some(match (degree, rhs.as_slice()) {
        (1, [p, q]) => Seg2::Line { a: *p, b: *q },
        (2, [p, c, q]) => Seg2::Conic { a: *p, apex: *c, b: *q, w: 1.0 },
        (3, [p0, p1, p2, p3]) => Seg2::Cubic { p0: *p0, p1: *p1, p2: *p2, p3: *p3 },
        _ => {
            let mut p = [Vec2::ZERO; solidcraft_geom::MAX_BEZIER_CTRL];
            for (slot, c) in p.iter_mut().zip(&rhs) {
                *slot = *c;
            }
            Seg2::Bezier { n: u8::try_from(degree).ok()?, p }
        }
    })
}

/// Solves the dense `n x n` system `m x = rhs` in place (Gaussian elimination with partial
/// pivoting), the solution left in `rhs`.
fn solve_dense<T: Copy + std::ops::Sub<Output = T> + std::ops::Mul<f64, Output = T>>(m: &mut [f64], rhs: &mut [T], n: usize) -> Option<()> {
    if m.len() != n * n || rhs.len() != n {
        return None;
    }
    for c in 0..n {
        let pivot = (c..n).max_by(|&a, &b| m[a * n + c].abs().total_cmp(&m[b * n + c].abs()))?;
        if !(m[pivot * n + c].abs() > 1e-300) {
            return None;
        }
        if pivot != c {
            for k in 0..n {
                m.swap(c * n + k, pivot * n + k);
            }
            rhs.swap(c, pivot);
        }
        for r in c + 1..n {
            let f = m[r * n + c] / m[c * n + c];
            if f != 0.0 {
                for k in c..n {
                    m[r * n + k] -= f * m[c * n + k];
                }
                rhs[r] = rhs[r] - rhs[c] * f;
            }
        }
    }
    for c in (0..n).rev() {
        let mut x = rhs[c];
        for k in c + 1..n {
            x = x - rhs[k] * m[c * n + k];
        }
        rhs[c] = x * (1.0 / m[c * n + c]);
    }
    Some(())
}

/// F's basis functions and their first derivatives along the lines of a grid of parameters, so
/// that evaluating a map on the grid costs no basis evaluation.
struct Grid {
    /// F's control points to a row.
    nu: usize,
    u: Vec<(usize, Vec<f64>, Vec<f64>)>,
    v: Vec<(usize, Vec<f64>, Vec<f64>)>,
}

impl Grid {
    fn new(spaces: &[Space; 2], us: &[f64], vs: &[f64]) -> Option<Grid> {
        let lines = |sp: &Space, ts: &[f64]| -> Option<Vec<(usize, Vec<f64>, Vec<f64>)>> {
            ts.iter()
                .map(|&t| {
                    let ((i, b0), (_, b1)) = (sp.basis(t, 0).ok()?, sp.basis(t, 1).ok()?);
                    Some((i, b0, b1))
                })
                .collect()
        };
        Some(Grid { nu: spaces[0].count(), u: lines(&spaces[0], us)?, v: lines(&spaces[1], vs)? })
    }

    /// F's first derivatives (F_u, F_v) at every grid point, u fastest, for control points
    /// `ctrl`: the v basis applied down every column first, then the u basis along the row, so
    /// each point costs a few products rather than one per control point it sees.
    fn derivatives(&self, ctrl: &[Vec2]) -> Vec<(Vec2, Vec2)> {
        let mut out = Vec::with_capacity(self.u.len() * self.v.len());
        let (mut r0, mut r1) = (vec![Vec2::ZERO; self.nu], vec![Vec2::ZERO; self.nu]);
        for (iv, v0, v1) in &self.v {
            for (k, (x0, x1)) in r0.iter_mut().zip(r1.iter_mut()).enumerate() {
                (*x0, *x1) = v0.iter().zip(v1).enumerate().fold((Vec2::ZERO, Vec2::ZERO), |(s0, s1), (b, (w0, w1))| {
                    let c = ctrl.get((iv + b) * self.nu + k).copied().unwrap_or_default();
                    (s0 + c * *w0, s1 + c * *w1)
                });
            }
            for (iu, u0, u1) in &self.u {
                let fu = u1.iter().enumerate().fold(Vec2::ZERO, |s, (a, w)| s + r0.get(iu + a).copied().unwrap_or_default() * *w);
                let fv = u0.iter().enumerate().fold(Vec2::ZERO, |s, (a, w)| s + r1.get(iu + a).copied().unwrap_or_default() * *w);
                out.push((fu, fv));
            }
        }
        out
    }
}

/// Where a fit is measured: a grid of points inside every knot span of F's spaces, with the
/// neutral metric at each, u fastest.
struct Samples {
    grid: Grid,
    metric: Vec<[f64; 3]>,
}

impl Samples {
    fn new(s: &NurbsSurface, s_spaces: &[Space; 2], spaces: &[Space; 2], thickness: f64, k_factor: f64) -> Option<Samples> {
        let along = |sp: &Space| -> Vec<f64> {
            let mut breaks: Vec<f64> = sp.knots().to_vec();
            breaks.dedup();
            let per = sp.degree() + 2;
            breaks.windows(2).flat_map(|w| (0..per).map(move |k| w[0] + (w[1] - w[0]) * (k as f64 + 0.5) / per as f64)).collect()
        };
        let (us, vs) = (along(&spaces[0]), along(&spaces[1]));
        Some(Samples { grid: Grid::new(spaces, &us, &vs)?, metric: neutral_metrics(s, s_spaces, &us, &vs, thickness, k_factor)? })
    }

    fn derivatives(&self, ctrl: &[Vec2]) -> Vec<(Vec2, Vec2)> {
        self.grid.derivatives(ctrl)
    }
}

/// The neutral metric (E, F, G) at every point of the grid `us` x `vs`, u fastest.
fn neutral_metrics(s: &NurbsSurface, s_spaces: &[Space; 2], us: &[f64], vs: &[f64], thickness: f64, k_factor: f64) -> Option<Vec<[f64; 3]>> {
    let lines = |sp: &Space, ts: &[f64]| -> Option<Vec<Vec<(usize, Vec<f64>)>>> { ts.iter().map(|&t| basis_rows(sp, t, 2)).collect() };
    let (su, sv) = (lines(&s_spaces[0], us)?, lines(&s_spaces[1], vs)?);
    let mut metric = Vec::with_capacity(us.len() * vs.len());
    for bv in &sv {
        for bu in &su {
            let d = partials_from(s, bu, bv, 2)?;
            let (_, _, m) = neutral(&[d[0][0], d[1][0], d[0][1], d[2][0], d[1][1], d[0][2]], thickness, k_factor)?;
            metric.push(m);
        }
    }
    Some(metric)
}

/// Fits the flat map of the spline surface `s`'s neutral surface, for sheet `thickness` and
/// `k_factor`.
pub(crate) fn fit(s: &NurbsSurface, thickness: f64, k_factor: f64) -> Result<FlatMap, &'static str> {
    let [p, q] = s.degree();
    let [ku, kv] = s.knots().clone();
    let s_spaces = [
        Space::new(p, ku).map_err(|_| "a free-form panel's surface needs clamped knots")?,
        Space::new(q, kv).map_err(|_| "a free-form panel's surface needs clamped knots")?,
    ];
    // F's own space: over the surface's parameters and breaks, so that its parameter lines and
    // anything drawn in them stay the surface's. Along a straight direction (degree 1, the
    // rulings of a ruled surface) a development is exactly linear, so it stays as it is and is
    // never refined. Along a curved one F takes the highest degree that keeps the flat of a
    // straight line of the parameters one exact Bezier per span (of the two degrees added): four
    // when both directions curve, eight beside a straight one. Interpolation error falls with the
    // span size to the power of the degree, so a high degree meets the tolerance with far fewer
    // samples than a cubic. Across the surface's knots F is only as smooth as the neutral surface:
    // an offset is one order less smooth than its surface, so a knot of multiplicity m in degree
    // p leaves it C^(p - m - 1), and F repeats the knot to allow exactly that.
    let curved = [p > 1, q > 1];
    let own = |sp: &Space, here: bool, other: bool| -> Option<Space> {
        if !here {
            return Some(sp.clone());
        }
        let d = if other { solidcraft_geom::MAX_BEZIER_DEGREE / 2 } else { solidcraft_geom::MAX_BEZIER_DEGREE - 1 };
        let [a, b] = sp.domain();
        let inner: Vec<f64> = sp.knots().iter().copied().filter(|&k| k > a && k < b).collect();
        let mut knots = vec![a; d + 1];
        for run in inner.chunk_by(|x, y| x == y) {
            let smooth = sp.degree() as isize - run.len() as isize - 1;
            knots.extend(std::iter::repeat_n(*run.first()?, (d as isize - smooth).clamp(1, d as isize) as usize));
        }
        knots.extend(std::iter::repeat_n(b, d + 1));
        Space::new(d, knots).ok()
    };
    let (Some(fu), Some(fv)) = (own(&s_spaces[0], curved[0], curved[1]), own(&s_spaces[1], curved[1], curved[0])) else {
        return Err("a free-form panel's surface has knots its flat cannot be fitted over");
    };
    let mut spaces = [fu, fv];
    // The developed flat's stretch a level coarser; whether the flat is being solved for (the
    // surface has shown it is not developable); and the solution a level coarser, carried into
    // this level's space.
    let mut developed_before = f64::INFINITY;
    let mut solving = false;
    let mut solved_before: Option<Vec<Vec2>> = None;
    let mut level = 0;
    loop {
        let pts = Samples::new(s, &s_spaces, &spaces, thickness, k_factor).ok_or("a free-form panel's surface has a point with no normal")?;
        let (developed, short) = developed(s, &s_spaces, &spaces, thickness, k_factor).ok_or("a free-form panel's surface could not be developed")?;
        let stretch = strain(&pts, &developed);
        // Developing, a refinement adds spans only along a direction whose derivatives F misses by
        // a tenth as much as the other's or more: a direction F already follows needs none.
        let top = short[0].max(short[1]);
        let along = [curved[0] && short[0] >= top / 10.0, curved[1] && short[1] >= top / 10.0];
        let affordable = |n: &[Space; 2]| sample_count(n) <= MAX_SAMPLES;
        let developing = if level < MAX_REFINEMENTS { finer(&spaces, along).or_else(|| finer(&spaces, curved)).filter(affordable) } else { None };
        // On a developable surface a developed flat's stretch falls by a power of the span size
        // with each refinement; on any other it keeps the stretch of the lines it was carried
        // along, and from then on the flat is solved for, from the better of the developed flat
        // and the solution a level coarser.
        solving |= stretch > STRETCH_TOL && (developing.is_none() || stretch > STALL * developed_before);
        developed_before = stretch;
        let before = solved_before.take();
        let map = if solving {
            let start = match &before {
                Some(c) if strain(&pts, c) < stretch => c.clone(),
                _ => developed,
            };
            let ctrl = gauss_newton(&spaces, start, &pts);
            let stretch = strain(&pts, &ctrl);
            FlatMap { spaces: spaces.clone(), ctrl, stretch }
        } else {
            FlatMap { spaces: spaces.clone(), ctrl: developed, stretch }
        };
        // Done when the flat is within the tolerance of the surface, or, solved for, when a level
        // finer moved no control point, and so no point of the flat, further than RESOLVED.
        let moved = |c: &Vec<Vec2>| c.iter().zip(&map.ctrl).map(|(a, b)| a.dist(*b)).fold(0.0, f64::max);
        let resolved = map.stretch <= STRETCH_TOL || before.as_ref().is_some_and(|c| moved(c) <= RESOLVED);
        let next = if !solving {
            developing
        } else if level < MAX_REFINEMENTS {
            finer(&spaces, curved).filter(|n| affordable(n) && band_work(n) <= MAX_BAND_WORK)
        } else {
            None
        };
        let Some(next) = next.filter(|_| !resolved) else { return Ok(map) };
        if solving {
            solved_before = Some(lift(&spaces, &map.ctrl, &next).ok_or("a free-form panel's flat could not be refined")?);
        }
        spaces = next;
        level += 1;
    }
}

/// The spaces with a knot added in the middle of every span of their curved directions; none
/// when neither curves. The surface's own parameters stay F's, so nothing drawn in them moves.
fn finer(spaces: &[Space; 2], curved: [bool; 2]) -> Option<[Space; 2]> {
    if !(curved[0] || curved[1]) {
        return None;
    }
    let refine = |sp: &Space, curved: bool| -> Option<Space> {
        if !curved {
            return Some(sp.clone());
        }
        let mut b: Vec<f64> = sp.knots().to_vec();
        b.dedup();
        let mids: Vec<f64> = b.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
        sp.refined(&mids).ok()
    };
    Some([refine(&spaces[0], curved[0])?, refine(&spaces[1], curved[1])?])
}

/// How many samples `samples` takes over `spaces`.
fn sample_count(spaces: &[Space; 2]) -> usize {
    spaces
        .iter()
        .map(|sp| {
            let mut b: Vec<f64> = sp.knots().to_vec();
            b.dedup();
            b.len().saturating_sub(1) * (sp.degree() + 2)
        })
        .product()
}

/// The Gauss-Newton system's unknowns over `spaces` (two per control point) and its half
/// bandwidth, the unknowns numbered along the shorter direction first: two control points share
/// samples only within a degree of each other both ways.
fn band_shape(spaces: &[Space; 2]) -> (usize, usize) {
    let (nu, nv) = (spaces[0].count(), spaces[1].count());
    let (du, dv) = (spaces[0].degree(), spaces[1].degree());
    let reach = if nu <= nv { dv * nu + du } else { du * nv + dv };
    (2 * nu * nv, 2 * reach + 1)
}

/// The multiply-adds of one factorization of the Gauss-Newton system over `spaces`.
fn band_work(spaces: &[Space; 2]) -> f64 {
    let (n, w) = band_shape(spaces);
    n as f64 * (w * w) as f64
}

/// Control points of the same map in a finer tensor space (exact: knot insertion row by row,
/// then column by column).
fn lift(from: &[Space; 2], ctrl: &[Vec2], to: &[Space; 2]) -> Option<Vec<Vec2>> {
    let (nu, nv) = (from[0].count(), from[1].count());
    let mu = to[0].count();
    let as3 = |p: Vec2| Vec3::new(p.x, p.y, 0.0);
    let mut rows = Vec::with_capacity(nv);
    for j in 0..nv {
        let row: Vec<Vec3> = (0..nu).map(|i| ctrl.get(j * nu + i).copied().map(as3)).collect::<Option<_>>()?;
        rows.push(embed(&from[0], &row, &to[0]).ok()?);
    }
    let mut out = vec![Vec2::ZERO; mu * to[1].count()];
    for i in 0..mu {
        let col: Vec<Vec3> = rows.iter().map(|r| r.get(i).copied()).collect::<Option<_>>()?;
        let lifted = embed(&from[1], &col, &to[1]).ok()?;
        for (j, c) in lifted.into_iter().enumerate() {
            *out.get_mut(j * mu + i)? = Vec2::new(c.x, c.y);
        }
    }
    Some(out)
}

/// A symmetric matrix of half bandwidth `w`, its lower band kept row by row: entry (i, j), for j
/// in i - w ..= i, at (i + 1) w + j (those of columns below zero unused).
#[derive(Clone)]
struct Band {
    n: usize,
    w: usize,
    a: Vec<f64>,
}

impl Band {
    fn new(n: usize, w: usize) -> Band {
        Band { n, w, a: vec![0.0; n * (w + 1)] }
    }

    /// Adds `x` to entry (i, j), j <= i, when it is in the band.
    fn add(&mut self, i: usize, j: usize, x: f64) {
        if j <= i
            && i - j <= self.w
            && let Some(e) = self.a.get_mut((i + 1) * self.w + j)
        {
            *e += x;
        }
    }

    /// Levenberg-Marquardt damping: every diagonal entry d made d + lambda (1 + d).
    fn damp(&mut self, lambda: f64) {
        for i in 0..self.n {
            if let Some(d) = self.a.get_mut((i + 1) * self.w + i) {
                *d += lambda * (1.0 + *d);
            }
        }
    }

    /// Cholesky factorization in place, L L^T with L in the band (nothing outside it fills in);
    /// none when the matrix is not positive definite.
    fn cholesky(&mut self) -> Option<()> {
        let w = self.w;
        for i in 0..self.n {
            let (lo, ri) = (i.saturating_sub(w), (i + 1) * w);
            for j in lo..=i {
                let rj = (j + 1) * w;
                let dot: f64 = self.a.get(ri + lo..ri + j)?.iter().zip(self.a.get(rj + lo..rj + j)?).map(|(x, y)| x * y).sum();
                let s = self.a.get(ri + j)? - dot;
                let l = if j == i {
                    if !(s > 0.0) {
                        return None;
                    }
                    s.sqrt()
                } else {
                    s / self.a.get(rj + j)?
                };
                *self.a.get_mut(ri + j)? = l;
            }
        }
        Some(())
    }

    /// Solves L L^T x = b in place, after `cholesky`.
    fn solve(&self, b: &mut [f64]) -> Option<()> {
        let w = self.w;
        for i in 0..self.n {
            let (lo, ri) = (i.saturating_sub(w), (i + 1) * w);
            let dot: f64 = self.a.get(ri + lo..ri + i)?.iter().zip(b.get(lo..i)?).map(|(l, x)| l * x).sum();
            *b.get_mut(i)? = (b.get(i)? - dot) / self.a.get(ri + i)?;
        }
        for i in (0..self.n).rev() {
            let (lo, ri) = (i.saturating_sub(w), (i + 1) * w);
            let x = b.get(i)? / self.a.get(ri + i)?;
            *b.get_mut(i)? = x;
            for (bk, l) in b.get_mut(lo..i)?.iter_mut().zip(self.a.get(ri + lo..ri + i)?) {
                *bk -= l * x;
            }
        }
        Some(())
    }
}

/// Damped Gauss-Newton on the metric: F's first fundamental form against the neutral surface's
/// at every sample, each residual scaled to a strain. The normal equations are a band
/// ([`band_shape`]): a factorization costs n w^2 against n^3 dense. Nested dissection only pulls
/// ahead past some thirty to forty control points a side (its separators are a degree wide), and
/// further once its indexing costs are counted.
fn gauss_newton(spaces: &[Space; 2], mut ctrl: Vec<Vec2>, pts: &Samples) -> Vec<Vec2> {
    let (unknowns, w) = band_shape(spaces);
    if band_work(spaces) > MAX_BAND_WORK || unknowns != 2 * ctrl.len() {
        return ctrl;
    }
    let (nu, nv) = (spaces[0].count(), spaces[1].count());
    let slot = |k: usize| 2 * if nu <= nv { k } else { (k % nu) * nv + k / nu };
    let mut lambda = 1e-6;
    let mut cost = metric_cost(pts, &ctrl);
    for _ in 0..MAX_STEPS {
        let mut jtj = Band::new(unknowns, w);
        let mut jtr = vec![0.0; unknowns];
        let mut at = pts.metric.iter().zip(pts.derivatives(&ctrl));
        let mut seen: Vec<(usize, f64, f64)> = Vec::new();
        for (iv, v0, v1) in &pts.grid.v {
            for (iu, u0, u1) in &pts.grid.u {
                let Some((&[e, f, g], (fu, fv))) = at.next() else { return ctrl };
                // The control points the sample sees (their first unknowns), with their weights in
                // F_u and in F_v.
                seen.clear();
                seen.extend(v0.iter().zip(v1).enumerate().flat_map(|(b, (b0, b1))| {
                    u0.iter().zip(u1).enumerate().map(move |(a, (a0, a1))| (slot((iv + b) * nu + iu + a), a1 * b0, a0 * b1))
                }));
                let (se, sf, sg) = (1.0 / e.max(1e-300), 1.0 / (e * g).sqrt().max(1e-300), 1.0 / g.max(1e-300));
                let (r0, r1, r2) = ((fu.dot(fu) - e) * se, (fu.dot(fv) - f) * sf, (fv.dot(fv) - g) * sg);
                // Every residual's gradient is F_u and F_v scaled by a control point's two weights
                // (wu, wv), so J^T r takes wu p + wv q at each, and J^T J's 2 x 2 block for two of
                // them is wu wu' A + wv wv' B + wu wv' C + wv wu' C^T.
                let (p, q) = (fu * (2.0 * se * r0) + fv * (sf * r1), fu * (sf * r1) + fv * (2.0 * sg * r2));
                let outer = |x: Vec2, y: Vec2| [x.x * y.x, x.x * y.y, x.y * y.x, x.y * y.y];
                let (uu, vv, vu) = (outer(fu, fu), outer(fv, fv), outer(fv, fu));
                let a: [f64; 4] = std::array::from_fn(|i| 4.0 * se * se * uu[i] + sf * sf * vv[i]);
                let b: [f64; 4] = std::array::from_fn(|i| sf * sf * uu[i] + 4.0 * sg * sg * vv[i]);
                let c = vu.map(|x| sf * sf * x);
                for &(sa, wua, wva) in &seen {
                    for (k, x) in [(sa, wua * p.x + wva * q.x), (sa + 1, wua * p.y + wva * q.y)] {
                        if let Some(y) = jtr.get_mut(k) {
                            *y += x;
                        }
                    }
                    // The lower triangle only.
                    for &(sb, wub, wvb) in seen.iter().filter(|s| s.0 <= sa) {
                        let (k1, k2, k3, k4) = (wua * wub, wva * wvb, wua * wvb, wva * wub);
                        for (r, s) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                            jtj.add(sa + r, sb + s, k1 * a[2 * r + s] + k2 * b[2 * r + s] + k3 * c[2 * r + s] + k4 * c[2 * s + r]);
                        }
                    }
                }
            }
        }
        // Levenberg-Marquardt: damp, try, and keep the step only if the metric fits better. The
        // damping also pins the free rigid motion of the flat.
        let mut improved = false;
        for _ in 0..8 {
            let mut m = jtj.clone();
            m.damp(lambda);
            let mut step: Vec<f64> = jtr.iter().map(|x| -x).collect();
            if m.cholesky().and_then(|()| m.solve(&mut step)).is_none() {
                lambda *= 10.0;
                continue;
            }
            let trial: Vec<Vec2> = ctrl
                .iter()
                .enumerate()
                .map(|(k, c)| *c + Vec2::new(step.get(slot(k)).copied().unwrap_or(0.0), step.get(slot(k) + 1).copied().unwrap_or(0.0)))
                .collect();
            let tc = metric_cost(pts, &trial);
            if tc < cost {
                // Converged once a step moves no control point a tenth of RESOLVED.
                let far = ctrl.iter().zip(&trial).map(|(a, b)| a.dist(*b)).fold(0.0, f64::max);
                improved = cost - tc > 1e-30 && far > RESOLVED / 10.0;
                ctrl = trial;
                cost = tc;
                lambda = (lambda * 0.3).max(1e-12);
                break;
            }
            lambda *= 10.0;
        }
        if !improved || cost < 1e-28 {
            break;
        }
    }
    ctrl
}

/// The sum of the squared strain residuals of F's metric against the samples'.
fn metric_cost(pts: &Samples, ctrl: &[Vec2]) -> f64 {
    pts.metric
        .iter()
        .zip(pts.derivatives(ctrl))
        .map(|([e, f, g], (fu, fv))| ((fu.dot(fu) - e) / e).powi(2) + ((fu.dot(fv) - f) / (e * g).sqrt()).powi(2) + ((fv.dot(fv) - g) / g).powi(2))
        .sum()
}

/// The largest principal strain of F against the neutral surface over the samples: the square
/// roots of the eigenvalues of (neutral metric)^-1 (F's metric), less one.
fn strain(pts: &Samples, ctrl: &[Vec2]) -> f64 {
    pts.metric
        .iter()
        .zip(pts.derivatives(ctrl))
        .map(|(&[e, f, g], (fu, fv))| {
            let (a, b, c) = (fu.dot(fu), fu.dot(fv), fv.dot(fv));
            let det = e * g - f * f;
            // (I_n)^-1 I_F, a 2 x 2 matrix: its trace and determinant give its eigenvalues.
            let (m00, m01, m10, m11) = ((g * a - f * b) / det, (g * b - f * c) / det, (e * b - f * a) / det, (e * c - f * b) / det);
            let (tr, dt) = (m00 + m11, m00 * m11 - m01 * m10);
            let disc = (tr * tr / 4.0 - dt).max(0.0).sqrt();
            [tr / 2.0 + disc, tr / 2.0 - disc].iter().map(|l| (l.max(0.0).sqrt() - 1.0).abs()).fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}
