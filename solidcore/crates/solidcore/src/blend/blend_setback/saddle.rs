//! The certificates of an order-2 setback fill at a corner of any convexity mix (owner,
//! 2026-11-13: the G2 mixed corner is a Setback whose saddle carries a certificate; not the
//! fork's, which sets back only corners of one convexity).
//!
//! Upstream: none (the fork's `SmBlendSetback` certifies only its G1 fill, convex).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`certified_side`] | none |
//! | [`clear_of_kept_face`] | none |
//!
//! - **The normal** ([`certified_side`]): a patch's normal `N = S_u x S_v` against a fixed
//!   direction d, the outward normal of its round (or of the corner, for an interior part).
//!   With `S = X / w` (X the homogeneous net's first three components),
//!   `w^4 S_u x S_v = w (w X_u x X_v + w_v X x X_u + w_u X_v x X)`, so `N . d` has the sign
//!   of `E = d . (w X_u x X_v + w_v X x X_u + w_u X_v x X)` (weights positive), a polynomial
//!   in Bernstein form. E certified at least `1e-9` of its largest coefficient over the whole
//!   patch (or at most its negative), by its coefficients on pieces halved each way up to
//!   [`GRAPH_DEPTH`] times, makes N regular everywhere and one-signed against d: the patch
//!   does not fold, and its normal keeps to one side of the material. Never sampled.
//! - **The kept faces** ([`clear_of_kept_face`]): a face where a convex and a concave round
//!   meet has no side the fill keeps to (the concave round rises off it where the convex one
//!   dips under it: Várady and Rockwood 1997, §5 of "Geometric construction for setback
//!   vertex blending"); what the fill must not do is reach the part of the face that is kept,
//!   past the rounds' rails and the spring curve. Each piece of each patch is certified to
//!   stay off it one of three ways, by its coefficients: (a) it lies beside a spring curve's
//!   half and its projected cross derivative points into the hole along it (`(S_u x S_v) . n`
//!   one-signed, the hole's side); (b) its projection lies on the vertex's side of the rails
//!   that bound the kept part (both rails where the face's angle at the vertex is reflex, its
//!   kept part then past either; else one of them, as the kept part lies past both), within
//!   the corner's tolerance; (c) it is the interior part's corner at the face's base point and
//!   lies on the hole's side of the spring curve's tangent there, within the tolerance. A
//!   piece certified none of these ways is halved each way up to [`GRAPH_DEPTH`] times; one
//!   left at that depth refuses the face.
//!
//! Deviations: the recursion is an explicit stack bounded by [`GRAPH_DEPTH`] halvings, each
//! piece polling the kernel's cancel token.

use super::certify::{t_axpy, t_du, t_dv, t_make, t_mul_fast, t_nonneg, t_scale, t_split};
use super::curved::comps;
use super::graph::GRAPH_DEPTH;
use super::{el_ref, first, Face, Net, Tensor};
use crate::error::Result;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};

const OP: &str = "blend::blend_setback::saddle";

/// `d . (a x b)` of two polynomial vectors.
fn cross_dot(d: Vec3, a: [&Tensor; 3], b: [&Tensor; 3]) -> Result<Tensor> {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    let minus = |p: &Tensor, q: &Tensor, r: &Tensor, s: &Tensor| -> Result<Tensor> {
        t_axpy(&t_mul_fast(p, q)?, -1.0, &t_mul_fast(r, s)?)
    };
    let x = minus(ay, bz, az, by)?;
    let y = minus(az, bx, ax, bz)?;
    let z = minus(ax, by, ay, bx)?;
    let s = t_axpy(&t_make(0, 0), d.x, &x)?;
    let s = t_axpy(&s, d.y, &y)?;
    t_axpy(&s, d.z, &z)
}

/// `E`, of the sign of `(S_u x S_v) . d` over the net (module docs).
fn normal_along(net: &Net, d: Vec3) -> Result<Tensor> {
    let [x, y, z, w] = comps(net)?;
    let (xu, yu, zu, wu) = (t_du(&x)?, t_du(&y)?, t_du(&z)?, t_du(&w)?);
    let (xv, yv, zv, wv) = (t_dv(&x)?, t_dv(&y)?, t_dv(&z)?, t_dv(&w)?);
    let uv = cross_dot(d, [&xu, &yu, &zu], [&xv, &yv, &zv])?;
    let xu_side = cross_dot(d, [&x, &y, &z], [&xu, &yu, &zu])?;
    let xv_side = cross_dot(d, [&xv, &yv, &zv], [&x, &y, &z])?;
    let s = t_mul_fast(&w, &uv)?;
    let s = t_axpy(&s, 1.0, &t_mul_fast(&wv, &xu_side)?)?;
    t_axpy(&s, 1.0, &t_mul_fast(&wu, &xv_side)?)
}

/// `t - c`, c a constant.
fn less(t: &Tensor, c: f64) -> Result<Tensor> {
    let mut one = t_make(0, 0);
    one.c.fill(1.0);
    t_axpy(t, -c, &one)
}

/// The sign (1 or -1) the net's normal is certified to have against `d` over its whole
/// square, bounded away from zero by `1e-9` of `E`'s largest coefficient (module docs); 0
/// where neither sign is certified to [`GRAPH_DEPTH`] halvings.
///
/// # Errors
/// [`crate::error::SolidError`] of the Bernstein arithmetic (a malformed net).
pub(super) fn certified_side(net: &Net, d: Vec3) -> Result<f64> {
    let e = normal_along(net, d)?;
    let margin = 1.0e-9 * t_scale(&e);
    if !(margin > 0.0) {
        return Ok(0.0);
    }
    if t_nonneg(&less(&e, margin)?, 0.0, GRAPH_DEPTH)? == 1 {
        return Ok(1.0);
    }
    let mut neg = e;
    for x in &mut neg.c {
        *x = -*x;
    }
    if t_nonneg(&less(&neg, margin)?, 0.0, GRAPH_DEPTH)? == 1 {
        return Ok(-1.0);
    }
    Ok(0.0)
}

/// The sign of `x`: 1, -1, or 0 (zero or not a number).
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// The homogeneous form `sign ((X - P w) x t) . n + tol w` of the net: the side of the line
/// through P along t in the plane of normal n, `sign` turning it positive on the side asked,
/// widened by `tol`.
fn line_side(net: &Net, p: Point3, t: Vec3, n: Vec3, sign: f64, tol: f64) -> Tensor {
    let mut f = t_make(net.p, net.q);
    for (fi, h) in f.c.iter_mut().zip(&net.pts) {
        let w = h[3];
        let rel = Vec3::new(h[0] - p.x * w, h[1] - p.y * w, h[2] - p.z * w);
        *fi = sign * rel.cross(t).dot(n) + tol * w;
    }
    f
}

/// Whether every coefficient is at least 0 (`strict`: above 0).
fn all_at_least(t: &Tensor, strict: bool) -> bool {
    t.c.iter().all(|&x| if strict { x > 0.0 } else { x >= 0.0 })
}

/// What a piece of a patch is certified by (module docs).
struct Piece {
    /// The vertex's side of the rail of the round after the face, of the one before, and the
    /// hole's side of the spring curve's tangent at the base point (each widened by `tol`).
    lines: [Tensor; 3],
    /// The projected cross derivative toward the hole, beside a spring half (strict).
    cross: Option<Tensor>,
    /// Whether the piece reaches the spring half's side of its patch.
    at_spring: bool,
    /// Whether the piece holds the interior part's corner at the base point.
    at_base: bool,
    /// Halvings left.
    depth: usize,
}

/// Whether the order-2 fill `nets` (its `2n` patches in the fill's order: the setback
/// patches, then the interior parts) is certified off the kept part of the plane `face`,
/// the one at cycle position `m` (setback patch m's side `v = 0`, patch `m - 1`'s side
/// `v = 1`, interior part m's corner `(1, 1)` on it), at the corner's vertex `vertex`
/// (module docs). `false` where a piece is not certified at [`GRAPH_DEPTH`] halvings, or the
/// face's rails or spring curve are degenerate there.
///
/// # Errors
/// [`crate::error::SolidError`] of the evaluations and the Bernstein arithmetic (a
/// malformed net or position), and the kernel's cancellation.
pub(super) fn clear_of_kept_face(
    k: &Kernel,
    nets: &[Net],
    m: usize,
    face: &Face,
    vertex: Point3,
    tol: f64,
) -> Result<bool> {
    let n = nets.len().checked_div(2).ok_or_else(super::sm_err)?;
    if n < 3 || nets.len() != 2 * n || m >= n {
        return Ok(false);
    }
    // the round before face m in the cycle (n >= 3)
    let prev = (m + n - 1).checked_rem(n).ok_or_else(super::sm_err)?;
    let after = el_ref(nets, m)?;
    let before = el_ref(nets, prev)?;
    let nrm = face.normal;
    let side = |p: Point3, t: Vec3, x: Point3| (x - p).cross(t).dot(nrm);
    // the rails: lines along the spring curve's ends (tangent to them there)
    let (pa, ta, _) = first(after, 0.0, 0.0)?;
    let (pb, tb, _) = first(before, 0.0, 1.0)?;
    let (sa, sb) = (sign(side(pa, ta, vertex)), sign(side(pb, tb, vertex)));
    // the base point and the spring curve's tangent there
    let (pc, tc, _) = first(after, 1.0, 0.0)?;
    let sc = sign(side(pc, tc, vertex));
    // the hole's side across each spring half, from its middle toward the vertex
    let (qa, ua, _) = first(after, 0.5, 0.0)?;
    let (qb, ub, _) = first(before, 0.5, 1.0)?;
    let ha = sign(ua.cross(vertex - qa).dot(nrm));
    // patch m - 1's side v = 1: the hole lies toward -v
    let hb = -sign(ub.cross(vertex - qb).dot(nrm));
    if sa == 0.0 || sb == 0.0 || sc == 0.0 || ha == 0.0 || hb == 0.0 {
        return Ok(false);
    }
    // the face's angle at the vertex is reflex where the other rail's end lies on the
    // vertex's side of this one: its kept part then lies past either rail, else past both
    let reflex = sa * side(pa, ta, pb) > 0.0;

    for (i, net) in nets.iter().enumerate() {
        let spring = if i == m {
            Some((ha, true))
        } else if i == prev {
            Some((hb, false))
        } else {
            None
        };
        let cross = match spring {
            Some((h, _)) => {
                let mut e = normal_along(net, nrm)?;
                for x in &mut e.c {
                    *x *= h;
                }
                Some(e)
            }
            None => None,
        };
        let mut stack = vec![Piece {
            lines: [
                line_side(net, pa, ta, nrm, sa, tol),
                line_side(net, pb, tb, nrm, sb, tol),
                line_side(net, pc, tc, nrm, sc, tol),
            ],
            cross,
            at_spring: spring.is_some(),
            at_base: i == n + m,
            depth: GRAPH_DEPTH,
        }];
        // the spring half's side: v = 0 of patch m, v = 1 of patch m - 1
        let low_v = spring.is_some_and(|(_, low)| low);
        while let Some(piece) = stack.pop() {
            k.check_cancelled(OP)?;
            let [la, lb, lc] = &piece.lines;
            let rails = if reflex {
                all_at_least(la, false) && all_at_least(lb, false)
            } else {
                all_at_least(la, false) || all_at_least(lb, false)
            };
            let beside =
                piece.at_spring && piece.cross.as_ref().is_some_and(|e| all_at_least(e, true));
            let tangent = piece.at_base && all_at_least(lc, false);
            if rails || beside || tangent {
                continue;
            }
            if piece.depth == 0 {
                return Ok(false);
            }
            // halved each way: u first, then v
            let mut halves_u: [Vec<Tensor>; 2] = [Vec::new(), Vec::new()];
            let mut all: Vec<&Tensor> = piece.lines.iter().collect();
            if let Some(e) = piece.cross.as_ref() {
                all.push(e);
            }
            for t in &all {
                let (lo, hi) = t_split(t, 0)?;
                el_mut_vec(&mut halves_u, 0)?.push(lo);
                el_mut_vec(&mut halves_u, 1)?.push(hi);
            }
            for (hu, tensors) in halves_u.iter().enumerate() {
                let mut halves_v: [Vec<Tensor>; 2] = [Vec::new(), Vec::new()];
                for t in tensors {
                    let (lo, hi) = t_split(t, 1)?;
                    el_mut_vec(&mut halves_v, 0)?.push(lo);
                    el_mut_vec(&mut halves_v, 1)?.push(hi);
                }
                for (hv, parts) in halves_v.into_iter().enumerate() {
                    let mut parts = parts.into_iter();
                    let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next())
                    else {
                        return Err(super::sm_err());
                    };
                    let cross = parts.next();
                    stack.push(Piece {
                        lines: [a, b, c],
                        cross,
                        at_spring: piece.at_spring && (hv == 0) == low_v,
                        at_base: piece.at_base && hu == 1 && hv == 1,
                        depth: piece.depth - 1,
                    });
                }
            }
        }
    }
    Ok(true)
}

/// The vector at `i` of a pair, mutably.
fn el_mut_vec(v: &mut [Vec<Tensor>; 2], i: usize) -> Result<&mut Vec<Tensor>> {
    v.get_mut(i).ok_or_else(super::sm_err)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::super::{distances, fill_hole_to, net_of, Edge, FillOrders, Round};
    use super::*;
    use crate::nurbs::b_spline_curve::KnotType;
    use crate::nurbs::b_spline_surface::{
        BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData, SurfParamType,
    };
    use crate::surface::surface::Surface;

    /// The order-2 section between two planes (thumbweight 0) along `t`: contact `c1(t)` on
    /// the first face, `c2(t)` on the second, the edge's point `e(t)`, over `[s, s + len]`.
    fn round(
        c1: impl Fn(f64) -> Point3,
        c2: impl Fn(f64) -> Point3,
        e: impl Fn(f64) -> Point3,
        s: f64,
    ) -> Surface {
        let mut pts = Vec::new();
        let mut w = Vec::new();
        for t in [s, s + 10.0] {
            let (r1, r2, ee) = (c1(t), c2(t), e(t));
            pts.extend([r1, (r1 + ee) * 0.5, ee, ee, (ee + r2) * 0.5, r2]);
            w.extend([1.0, 0.4, 0.1, 0.1, 0.4, 1.0]);
        }
        Surface::from(
            BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: 1,
                v_degree: 5,
                control_points: &pts,
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &[2, 2],
                v_multiplicities: &[6, 6],
                u_knots: &[s, s + 10.0],
                v_knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&w),
                uv_domain: None,
            })
            .unwrap(),
        )
    }

    /// A step's corner at the origin, rounded 2 with the order-2 section and filled to G2 at
    /// the rule's setbacks: the material `y > 0` and (`z < 0` or `x < 0`); the x edge convex
    /// (faces y = 0, z = 0), the y edge concave (z = 0, x = 0), the z edge convex (x = 0,
    /// y = 0). The front face y = 0 has a reflex angle at the vertex.
    fn step_corner() -> (Vec<Face>, Vec<Net>, Vec<usize>) {
        let k = Kernel::default();
        let r = 2.0;
        let o = Point3::new(0.0, 0.0, 0.0);
        let faces = vec![
            Face::plane(o, Vec3::new(0.0, -1.0, 0.0)),
            Face::plane(o, Vec3::new(0.0, 0.0, 1.0)),
            Face::plane(o, Vec3::new(1.0, 0.0, 0.0)),
        ];
        let edges = [
            (Vec3::new(1.0, 0.0, 0.0), [0, 1]),
            (Vec3::new(0.0, 1.0, 0.0), [1, 2]),
            (Vec3::new(0.0, 0.0, 1.0), [2, 0]),
        ]
        .map(|(away, faces)| Edge {
            away,
            radius: r,
            faces,
        });
        let sb = distances(&faces, &edges).unwrap().setbacks;
        let p = Point3::new;
        let rounds = [
            round(
                |t| p(t, 0.0, -r),
                |t| p(t, r, 0.0),
                |t| p(t, 0.0, 0.0),
                sb[0],
            ),
            round(
                |t| p(r, t, 0.0),
                |t| p(0.0, t, r),
                |t| p(0.0, t, 0.0),
                sb[1],
            ),
            round(
                |t| p(0.0, r, t),
                |t| p(-r, 0.0, t),
                |t| p(0.0, 0.0, t),
                sb[2],
            ),
        ];
        let input: Vec<Round<'_>> = rounds
            .iter()
            .zip(&sb)
            .map(|(surface, &end)| Round {
                surface,
                along: SurfParamType::U,
                end,
                corner_above: false,
            })
            .collect();
        let orders = FillOrders {
            rounds: 2,
            faces: 2,
        };
        let fill = fill_hole_to(&k, &faces, &input, orders, 1e-9).unwrap();
        let nets = fill.patches.iter().map(|p| net_of(p).unwrap()).collect();
        (faces, nets, fill.face)
    }

    /// The step's fill is certified off the kept parts of its two mixed faces (z = 0 and
    /// x = 0); with an interior part carried over the kept part of z = 0, it is not.
    #[test]
    fn kept_face_accepts_the_step_and_rejects_a_fill_reaching_the_face() {
        let k = Kernel::default();
        let (faces, nets, cycle) = step_corner();
        let o = Point3::new(0.0, 0.0, 0.0);
        let mixed: Vec<usize> = (0..3)
            .filter(|&m| faces[cycle[m]].normal.y == 0.0)
            .collect();
        assert_eq!(mixed.len(), 2);
        for &m in &mixed {
            assert!(clear_of_kept_face(&k, &nets, m, &faces[cycle[m]], o, 1e-9).unwrap());
        }
        // interior part m carried 4 along x and y, over the kept part of face z = 0 (past
        // both rails, x = 2 and y = 2)
        let m = (0..3).find(|&m| faces[cycle[m]].normal.z == 1.0).unwrap();
        let mut pulled = nets.clone();
        for h in &mut pulled[3 + m].pts {
            h[0] += 4.0 * h[3];
            h[1] += 4.0 * h[3];
        }
        assert!(!clear_of_kept_face(&k, &pulled, m, &faces[cycle[m]], o, 1e-9).unwrap());
    }

    /// The step's patches are certified to face one way against their rounds' outward
    /// normals; a patch is not certified against a direction in its own tangent plane.
    #[test]
    fn normal_side_of_the_step_fill() {
        let (faces, nets, cycle) = step_corner();
        let n = 3;
        let mut sum = Vec3::ZERO;
        for f in &faces {
            sum += f.normal;
        }
        let corner = sum.unitize().unwrap();
        let mut sides = Vec::new();
        for (i, net) in nets.iter().enumerate() {
            let d = if i < n {
                (faces[cycle[i]].normal + faces[cycle[(i + 1) % n]].normal)
                    .unitize()
                    .unwrap()
            } else {
                corner
            };
            sides.push(certified_side(net, d).unwrap());
        }
        assert!(
            sides.iter().all(|&s| s != 0.0 && s == sides[0]),
            "{sides:?}"
        );
        // along its round's end section the setback patch's normal is square to the round's
        // edge: against the edge's direction it is not certified to either side
        let edge = faces[cycle[0]]
            .normal
            .cross(faces[cycle[1]].normal)
            .unitize()
            .unwrap();
        assert_eq!(certified_side(&nets[0], edge).unwrap(), 0.0);
    }
}
