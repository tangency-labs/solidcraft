//! Where a round meets the end of an earlier round of the other kind at a shared split end: a
//! round adding material along the foot of a wall that stands on a plate, its end flush with the
//! plate's slanted side, and a round taking material away along the plate's top edge up to the
//! wall's end (either one first).
//!
//! The earlier round left a vertex where the later round's chain ends: the plate's top `T`, the
//! face `X` beside the chain and the earlier round `g`, tangent to `T` along a straight contact.
//! The round's end runs on `X` from there to the vertex where it meets its other face `Y` and the
//! end face `E` that holds the line `L` up the wall's end.

use solidcraft_geom::Vec3;
use truck_modeling as mt;

use crate::blend::plane_normal;
use crate::body::from_p3;
use crate::curveblend::project;

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
