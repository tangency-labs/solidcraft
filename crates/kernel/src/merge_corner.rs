//! Where a concave round and a convex round meet at a shared end: the merge corner.
//!
//! A cove `A` along the foot of a wall `W` standing on a plate top `T`, and a round `B` along the
//! plate's top edge with its side `S`, both ending at the vertex `V` where `W` and `S` meet `T`
//! (the wall's end `E` flush with the plate's side, so the line `L = W ∩ S` is square to `T`).
//! Each round alone ends in the other's plane; together the two rounds merge into one S-shaped
//! band: in every section normal to the bisector `b` of `T`'s corner the profile runs along `A`,
//! then along the internal tangent of the two rounds' sections, then along `B`.
//!
//! With `t = tan(γ/2)`, `γ` the turn of the tangent from `T`, the ends of the tangent run along
//! exact rational cubics `Γ_A` and `Γ_B` (weight `1 + t²`), and the band between them is an exact
//! rational patch of degree (4, 1), regular everywhere: no collapsed row at the corner `P*` where
//! the two rounds' contact lines on `T` cross.
//!
//! `mixed_end` recognises such an end on the body an earlier round left: the later round's chain
//! ends at the earlier round `g`, tangent to `T` along a straight contact, whose end runs on the
//! face beside the chain to the vertex where it meets its other face and the end face holding `L`.

use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::blend::{plane_normal, unsupported};
use crate::body::from_p3;
use crate::curveblend::project;
use crate::{KernelError, Result, guard};

// ---------------------------------------------------------------------------------------------
// Geometry: the band between the two rounds, in closed form.
// ---------------------------------------------------------------------------------------------

/// Smallest and largest radius taken (the fillet's own limits).
const RADII: (f64, f64) = (1e-6, 1e6);
/// Largest coordinate taken.
const REACH: f64 = 1e9;
/// Largest cosine between directions that should be square (normals of the faces along `L`
/// against `T`'s, the edges against their faces' normals).
const SQUARE: f64 = 1e-8;
/// Narrowest corner of `T` at the vertex: 5°.
const MIN_ANGLE: f64 = 5.0 * std::f64::consts::PI / 180.0;

/// The faces at the shared end, as the B-rep has them: normals point out of the solid.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Faces {
    /// The vertex where `T`, `W` and `S` meet.
    pub(crate) v: Vec3,
    /// `T`'s normal: up, from the plate into the air (the wall's side).
    pub(crate) top: Vec3,
    /// `W`'s normal: off the wall, over `T`.
    pub(crate) wall: Vec3,
    /// `S`'s normal: out of the plate's side.
    pub(crate) side: Vec3,
    /// `E`'s normal: out of the wall's end.
    pub(crate) end: Vec3,
    /// The edge `T`/`W` leaving `v` (`A`'s edge).
    pub(crate) along_wall: Vec3,
    /// The edge `T`/`S` leaving `v` (`B`'s edge).
    pub(crate) along_side: Vec3,
}

/// The merge corner at one shared end: `A` of radius `ra` on `W`, `B` of radius `rb` on `S`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Zone {
    pub(crate) v: Vec3,
    /// Up from `T`.
    pub(crate) n: Vec3,
    /// Along `W`'s trace on `T`, and along `S`'s.
    pub(crate) w: Vec3,
    pub(crate) s: Vec3,
    /// In `T`, square to `w` and `s`, both into `T`'s corner.
    pub(crate) m_w: Vec3,
    pub(crate) m_s: Vec3,
    /// The bisector of `T`'s corner, and across it from `w`'s side to `s`'s.
    pub(crate) b: Vec3,
    pub(crate) lambda: Vec3,
    /// `T`'s angle at the vertex.
    pub(crate) theta: f64,
    pub(crate) ra: f64,
    pub(crate) rb: f64,
    /// How far along `b` the band reaches: the section through `P*`.
    pub(crate) rho_star: f64,
    /// Where the contact lines on `T` cross, and where the band meets `L` on `W` and on `S`.
    pub(crate) p_star: Vec3,
    pub(crate) q_w: Vec3,
    pub(crate) q_s: Vec3,
    /// Whether the patch's own normal (`∂X/∂t × ∂X/∂τ`) points out of the solid.
    pub(crate) faces_out: bool,
}

/// The band as a face: its surface with the normal out of the solid, and its boundary from `P*`,
/// counterclockwise about that normal (and in the surface's parameters).
#[derive(Clone, Debug)]
pub(crate) struct Trimmed {
    pub(crate) surface: mt::NurbsSurface<mt::Vector4>,
    pub(crate) edges: [mt::NurbsCurve<mt::Vector4>; 3],
    /// Where each edge starts: `P*` first.
    pub(crate) corners: [Vec3; 3],
}

fn finite_point(p: Vec3) -> bool {
    p.is_finite() && p.x.abs() <= REACH && p.y.abs() <= REACH && p.z.abs() <= REACH
}

fn unit(v: Vec3, what: &str) -> Result<Vec3> {
    v.normalized().ok_or_else(|| KernelError::Invalid(format!("merge corner: {what} has no direction")))
}

/// `v` less its part along the unit `d`, as a unit vector.
fn square_to(v: Vec3, d: Vec3, what: &str) -> Result<Vec3> {
    unit(v - d * v.dot(d), what)
}

/// The merge corner of rounds `ra` (on `W`) and `rb` (on `S`) at the shared end `faces`.
pub(crate) fn zone(faces: &Faces, ra: f64, rb: f64) -> Result<Zone> {
    let Faces { v, top, wall, side, end, along_wall, along_side } = *faces;
    if ![ra, rb].iter().all(|r| r.is_finite() && *r >= RADII.0 && *r <= RADII.1) {
        return Err(KernelError::Invalid("merge corner: radius out of range".into()));
    }
    if !finite_point(v) || ![top, wall, side, end, along_wall, along_side].iter().all(|d| d.is_finite()) {
        return Err(KernelError::Invalid("merge corner: point or direction not finite".into()));
    }
    let n = unit(top, "the top's normal")?;
    let (wall, side, end) = (unit(wall, "the wall's normal")?, unit(side, "the side's normal")?, unit(end, "the end's normal")?);
    let (along_wall, along_side) = (unit(along_wall, "the wall's edge")?, unit(along_side, "the side's edge")?);
    // W, S and E stand square on T, and each edge lies in both its faces.
    let square = [wall.dot(n), side.dot(n), end.dot(n), along_wall.dot(n), along_wall.dot(wall), along_side.dot(n), along_side.dot(side)];
    if square.iter().any(|c| !(c.abs() <= SQUARE)) {
        return Err(unsupported("a round meeting another at an end whose faces are not square to the top"));
    }
    let m_w = square_to(wall, n, "the wall's normal")?;
    let m_s = -square_to(side, n, "the side's normal")?;
    let w = square_to(square_to(along_wall, n, "the wall's edge")?, m_w, "the wall's edge")?;
    let s = square_to(square_to(along_side, n, "the side's edge")?, m_s, "the side's edge")?;
    // Each edge runs into the other face's material side: T's corner at the vertex is convex.
    if !(w.dot(m_s) > SQUARE && s.dot(m_w) > SQUARE) {
        return Err(unsupported("a round meeting another at a reflex or flat corner of the top"));
    }
    let theta = w.dot(s).clamp(-1.0, 1.0).acos();
    if !(MIN_ANGLE..std::f64::consts::PI).contains(&theta) {
        return Err(unsupported("rounds meeting at an end where the top's corner is narrower than 5°"));
    }
    let b = unit(w + s, "the corner's bisector")?;
    let lambda = unit(s - w, "the corner's section")?;
    let r = ra + rb;
    let (sin, cot) = (theta.sin(), theta.cos() / theta.sin());
    let rho_star = r / (2.0 * (theta / 2.0).sin());
    let p_star = v + w * (rb / sin + ra * cot) + m_w * ra;
    // The band lies along the wall, not past its end.
    if !((p_star - v).dot(end) < -SQUARE * r) {
        return Err(unsupported("rounds meeting at an end that reach past the wall's end"));
    }
    let zone = Zone {
        v,
        n,
        w,
        s,
        m_w,
        m_s,
        b,
        lambda,
        theta,
        ra,
        rb,
        rho_star,
        p_star,
        q_w: v + n * ra,
        q_s: v - n * rb,
        faces_out: w.cross(n).dot(m_w) > 0.0,
    };
    let numbers = [theta, rho_star];
    if !(numbers.iter().all(|x| x.is_finite()) && [p_star, zone.q_w, zone.q_s].iter().all(|p| p.is_finite())) {
        return Err(KernelError::Invalid("merge corner: not finite".into()));
    }
    Ok(zone)
}

/// The Bernstein coefficients of the polynomial with power coefficients `power` (degree
/// `power.len() - 1`).
fn bernstein(power: &[mt::Vector4]) -> Vec<mt::Vector4> {
    let p = power.len().saturating_sub(1);
    let choose = |n: usize, k: usize| (0..k).fold(1.0, |acc, i| acc * n.saturating_sub(i) as f64 / (i + 1) as f64);
    (0..power.len())
        .map(|i| {
            power.iter().take(i + 1).enumerate().fold(mt::Vector4::new(0.0, 0.0, 0.0, 0.0), |acc, (k, a)| acc + *a * (choose(i, k) / choose(p, k)))
        })
        .collect()
}

/// A homogeneous control value: `p` is already the point times its weight `w`.
fn h(p: Vec3, w: f64) -> mt::Vector4 {
    mt::Vector4::new(p.x, p.y, p.z, w)
}

fn bezier(power: &[mt::Vector4]) -> Result<mt::NurbsCurve<mt::Vector4>> {
    let ctrl = bernstein(power);
    guard("merge corner curve", || {
        let degree = ctrl.len().saturating_sub(1);
        let curve =
            mt::BSplineCurve::try_new(mt::KnotVec::bezier_knot(degree), ctrl).map_err(|e| KernelError::Failed(format!("merge corner curve: {e}")))?;
        Ok(mt::NurbsCurve::new(curve))
    })
}

impl Zone {
    /// `R = rA + rB`.
    pub(crate) fn r(&self) -> f64 {
        self.ra + self.rb
    }

    fn c(&self) -> f64 {
        (self.theta / 2.0).cos()
    }

    /// `ρ*/c`: how far along `w` the section through `P*` cuts `W`'s trace.
    fn k(&self) -> f64 {
        self.rho_star / self.c()
    }

    /// The straight run each round needs back from the vertex: along `W`'s trace and along `S`'s.
    pub(crate) fn needed_run(&self) -> (f64, f64) {
        let (sin, cot) = (self.theta.sin(), self.theta.cos() / self.theta.sin());
        (self.rb / sin + self.ra * cot, self.ra / sin + self.rb * cot)
    }

    /// Where the section at `t` cuts `W`'s trace (`ρ = ρ*(1 − t)`).
    fn o(&self, t: f64) -> Vec3 {
        self.v + self.w * (self.k() * (1.0 - t))
    }

    /// The band's edge on `A` at `t` (`P*` at 0, `q_W` at 1).
    pub(crate) fn gamma_a(&self, t: f64) -> Vec3 {
        let q = 1.0 + t * t;
        self.o(t) + self.lambda * (self.ra * (1.0 - t).powi(2) / (q * self.c())) + self.n * (2.0 * self.ra * t * t / q)
    }

    /// The band's edge on `B` at `t` (`P*` at 0, `q_S` at 1).
    pub(crate) fn gamma_b(&self, t: f64) -> Vec3 {
        let q = 1.0 + t * t;
        let across = self.r() * (1.0 - t) - self.rb * (1.0 - t).powi(2) / q;
        self.o(t) + self.lambda * (across / self.c()) - self.n * (2.0 * self.rb * t * t / q)
    }

    /// The ruling's direction at `t`: `Γ_B − Γ_A = τ_B(t)·d̃(t)`.
    pub(crate) fn ruling(&self, t: f64) -> Vec3 {
        self.lambda * ((1.0 - t * t) / self.c()) - self.n * (2.0 * t)
    }

    /// Where the ruling at `t` meets `B`: the trim `τ_B(t) = R t/(1 + t²)`.
    pub(crate) fn tau_b(&self, t: f64) -> f64 {
        self.r() * t / (1.0 + t * t)
    }

    /// The patch at `(t, τ)`: `Γ_A(t) + τ·d̃(t)`.
    pub(crate) fn point(&self, t: f64, tau: f64) -> Vec3 {
        self.gamma_a(t) + self.ruling(t) * tau
    }

    /// `Γ_A`'s numerator and weight `1 + t²` in powers of `t`.
    fn power_a(&self) -> [mt::Vector4; 4] {
        let (k, c, ra) = (self.k(), self.c(), self.ra);
        let start = self.v + self.w * k + self.lambda * (ra / c);
        [h(start, 1.0), h(-self.w * k - self.lambda * (2.0 * ra / c), 0.0), h(start + self.n * (2.0 * ra), 1.0), h(-self.w * k, 0.0)]
    }

    /// `Γ_B`'s numerator and weight in powers of `t`.
    fn power_b(&self) -> [mt::Vector4; 4] {
        let (k, c, ra, rb) = (self.k(), self.c(), self.ra, self.rb);
        let start = self.v + self.w * k + self.lambda * (ra / c);
        [
            h(start, 1.0),
            h(-self.w * k + self.lambda * ((rb - ra) / c), 0.0),
            h(start - self.n * (2.0 * rb), 1.0),
            h(-self.w * k - self.lambda * (self.r() / c), 0.0),
        ]
    }

    /// `Γ_A` from `P*` (t = 0) to `q_W` (t = 1): an exact rational cubic.
    pub(crate) fn curve_a(&self) -> Result<mt::NurbsCurve<mt::Vector4>> {
        bezier(&self.power_a())
    }

    /// `Γ_B` from `P*` (t = 0) to `q_S` (t = 1).
    pub(crate) fn curve_b(&self) -> Result<mt::NurbsCurve<mt::Vector4>> {
        bezier(&self.power_b())
    }

    /// The patch `X(t, τ)` over `t ∈ [0, 1]`, `τ ∈ [0, R/2]`, of degree (4, 1): the band is the part
    /// `τ ≤ τ_B(t)`. Its numerator is `N_A(t) + τ·(1 + t²)·d̃(t)` over the weight `1 + t²`.
    pub(crate) fn patch(&self) -> Result<mt::NurbsSurface<mt::Vector4>> {
        let [a0, a1, a2, a3] = self.power_a();
        let along = self.lambda / self.c();
        // (1 + t²)·d̃(t) = (1 − t⁴)·λ/c − (2t + 2t³)·n.
        let d = [h(along, 0.0), h(self.n * -2.0, 0.0), h(Vec3::ZERO, 0.0), h(self.n * -2.0, 0.0), h(-along, 0.0)];
        let near = [a0, a1, a2, a3, h(Vec3::ZERO, 0.0)];
        let top = self.r() / 2.0;
        let far: Vec<mt::Vector4> = near.iter().zip(&d).map(|(a, d)| *a + *d * top).collect();
        let ctrl: Vec<Vec<mt::Vector4>> = bernstein(&near).into_iter().zip(bernstein(&far)).map(|(a, b)| vec![a, b]).collect();
        guard("merge corner patch", || {
            let knots = (mt::KnotVec::bezier_knot(4), mt::KnotVec::from(vec![0.0, 0.0, top, top]));
            let surface = mt::BSplineSurface::try_new(knots, ctrl).map_err(|e| KernelError::Failed(format!("merge corner patch: {e}")))?;
            Ok(mt::NurbsSurface::new(surface))
        })
    }

    /// The band as a face: `Γ_A` from `P*` to `q_W`, `L` down to `q_S`, `Γ_B` back to `P*` when the
    /// patch faces out; the other way round on the patch with its parameters swapped otherwise.
    pub(crate) fn trimmed(&self) -> Result<Trimmed> {
        let (a, b, patch) = (self.curve_a()?, self.curve_b()?, self.patch()?);
        let line = |from: Vec3, to: Vec3| {
            guard("merge corner line", || {
                let curve = mt::BSplineCurve::try_new(mt::KnotVec::bezier_knot(1), vec![h(from, 1.0), h(to, 1.0)])
                    .map_err(|e| KernelError::Failed(format!("merge corner line: {e}")))?;
                Ok(mt::NurbsCurve::new(curve))
            })
        };
        let (along_l, back_l) = (line(self.q_w, self.q_s)?, line(self.q_s, self.q_w)?);
        guard("merge corner face", || {
            let back = |c: &mt::NurbsCurve<mt::Vector4>| {
                let mut c = c.clone();
                c.invert();
                c
            };
            if self.faces_out {
                Ok(Trimmed { edges: [a.clone(), along_l, back(&b)], surface: patch, corners: [self.p_star, self.q_w, self.q_s] })
            } else {
                let mut surface = patch;
                surface.swap_axes();
                Ok(Trimmed { edges: [b.clone(), back_l, back(&a)], surface, corners: [self.p_star, self.q_s, self.q_w] })
            }
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Recognition
// ---------------------------------------------------------------------------------------------

/// A chain's end at an earlier round's end that the round meets there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MixedEnd {
    /// Whether the earlier round adds material (it rises off `T` to the outside, a wall's foot)
    /// rather than taking it away.
    pub(crate) earlier_adds: bool,
}

/// The earlier round at vertex `v` of a chain between planes `top` and `side`, when `round` (the
/// face the chain runs into there) is a round tangent to `top` that ends flush with the end of a
/// wall: its end runs on `side` from `v` to a vertex of four faces (the round, `side`, the
/// round's other face `Y` and the end face `E`, all but the round planar), where the edges of `E`
/// with `side` and with `Y` lie on one line `L` square to `top` through the corner the planes
/// `top`, `side` and `Y` make, and the round is a cylinder between `top` and `Y`. `faces`: the
/// shell's faces; `tolerance`: the blend's fit tolerance.
pub(crate) fn mixed_end(faces: &[mt::Face], v: &mt::Vertex, top: &mt::Face, side: &mt::Face, round: &mt::Face, tolerance: f64) -> Option<MixedEnd> {
    use mt::{BoundedCurve, ParametricCurve};
    let n_t = plane_normal(top)?;
    let n_x = plane_normal(side)?;
    if plane_normal(round).is_some() {
        return None;
    }
    let p = from_p3(v.point());
    let at = |e: &mt::Edge, x: &mt::Vertex| e.front() == x || e.back() == x;
    let shared = |a: &mt::Face, b: &mt::Face, x: &mt::Vertex| a.edge_iter().find(|e| at(e, x) && b.edge_iter().any(|y| y.id() == e.id()));
    // The round's contact with the top: straight, the round tangent to the top along it.
    let contact = shared(top, round, v)?;
    if !crate::curveblend::straight(&contact, tolerance) {
        return None;
    }
    let mid = crate::blend::edge_mid(&contact)?;
    if project(round, mid, None)?.1.dot(n_t) < 1.0 - 1e-4 {
        return None;
    }
    // The round's end on `side`, to its far vertex `u`.
    let cap = shared(side, round, v)?;
    let u = if cap.front() == v { cap.back().clone() } else { cap.front().clone() };
    let q = from_p3(u.point());
    let rise = (q - p).dot(n_t);
    if rise.abs() <= tolerance {
        return None;
    }
    // The four faces at `u`: the round, `side`, `Y` (the round's other face) and `E`.
    let around: Vec<&mt::Face> = faces.iter().filter(|f| f.vertex_iter().any(|x| x == u)).take(5).collect();
    if around.len() != 4 || !around.iter().any(|f| f.id() == round.id()) || !around.iter().any(|f| f.id() == side.id()) {
        return None;
    }
    let others: Vec<&mt::Face> = around.iter().copied().filter(|f| f.id() != round.id() && f.id() != side.id()).collect();
    let [a, b] = others[..] else { return None };
    let (y, e) = if shared(a, round, &u).is_some() { (a, b) } else { (b, a) };
    if shared(e, round, &u).is_some() {
        return None;
    }
    let n_y = plane_normal(y)?;
    plane_normal(e)?;
    // `L`: the edges of `E` with `side` and with `Y`, on one line square to the top.
    let away = |edge: &mt::Edge| -> Option<Vec3> {
        if !crate::curveblend::straight(edge, tolerance) {
            return None;
        }
        let far = if edge.front() == &u { edge.back() } else { edge.front() };
        (from_p3(far.point()) - q).normalized()
    };
    let (l1, l2) = (away(&shared(e, side, &u)?)?, away(&shared(e, y, &u)?)?);
    if l1.dot(l2) > -1.0 + 1e-6 || l1.dot(n_t).abs() < 1.0 - 1e-6 {
        return None;
    }
    // The corner of the planes `top`, `side` and `Y` lies on `L`.
    let den = n_t.dot(n_x.cross(n_y));
    if den.abs() < 1e-6 {
        return None;
    }
    let corner = (n_x.cross(n_y) * n_t.dot(p) + n_y.cross(n_t) * n_x.dot(p) + n_t.cross(n_x) * n_y.dot(q)) * (1.0 / den);
    let off_line = (corner - q) - l1 * (corner - q).dot(l1);
    if off_line.len() > tolerance * 10.0 {
        return None;
    }
    // A cylinder between `top` and `Y`: its axis along their line, its centre `r` from both
    // contacts (behind the round's outward normal when it takes material away, in front when it
    // adds), and the round's end on it.
    let axis = n_t.cross(n_y).normalized()?;
    let flat = |x: Vec3| x - axis * x.dot(axis);
    let n_u = project(round, q, None)?.1;
    let (chord, turn) = (flat(p - q), flat(n_t - n_u));
    let r = chord.len() / turn.len().max(1e-12);
    if !(r.is_finite() && r > tolerance) {
        return None;
    }
    let s = if rise > 0.0 { -1.0 } else { 1.0 };
    let centre = p - n_t * (s * r);
    if flat(centre - (q - n_u * (s * r))).len() > tolerance * 10.0 {
        return None;
    }
    let c = cap.curve();
    let (t0, t1) = c.range_tuple();
    let on_cylinder = (0..=16).all(|i| {
        let x = from_p3(c.subs(t0 + (t1 - t0) * f64::from(i) / 16.0));
        (flat(x - centre).len() - r).abs() <= tolerance * 10.0
    });
    on_cylinder.then_some(MixedEnd { earlier_adds: rise > 0.0 })
}

#[cfg(test)]
#[path = "merge_corner_tests.rs"]
mod tests;
