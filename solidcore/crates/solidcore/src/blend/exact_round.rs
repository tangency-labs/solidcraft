//! Constant rounds between planes in closed form: the geometry of a round on a straight edge
//! between two planes, of its ends, of the corner where three of them meet, and of the faces
//! and curves the round is made of, as plain data ([`crate::eval`]'s kinds). solidcore-ops' rounds
//! (its `blend::exact_round`) put these together into the body's record in front of the fillet
//! sessions, which build every round this does not cover.
//!
//! Not upstream (from yack learnings): a round of radius `r` on a straight edge
//! between two planes is a piece of the cylinder whose axis is where the planes' offsets by `r`
//! meet (into the material at a convex edge, out of it at a concave one); its cross-section is
//! the same circle all along the edge, so it touches each plane along a line, and every curve
//! and point of the result is known in closed form: nothing is traced, intersected or stitched.
//! A round ends square on a plane at right angles to its edge in a circular arc, or at the
//! corner where three rounds of one radius and convexity meet, on its section through the
//! centre of the ball touching the three planes; the corner is the ball's triangle between the
//! contacts, the patch [`ball_triangle_face`] makes. Each arc is a rational quadratic (Piegl &
//! Tiller, *The NURBS Book*, §7.3), the round's face the arc swept along the edge, and every
//! pcurve exact: a line on a plane, an iso-line of the round's net, and on a corner patch the
//! side's iso-line under the linear fractional change of parameter that takes one rational
//! quadratic of an arc to another (§6.4).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::blend::blend_corner::ball::ball_triangle_face;
use crate::eval::{Curve, Curve2, Frame, Surface};
use crate::surface::face_surface::record_face;
use crate::topology::FaceId;
use crate::{Brep, Kernel, Vec3};

/// A point or vector.
pub type V3 = [f64; 3];

/// An axis-aligned box: its least and greatest corners.
pub type Bounds = [V3; 2];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let n = norm(a);
    (n > 1e-300 && n.is_finite()).then(|| scale(a, 1.0 / n))
}

/// How far, in radians, a plane may be from square to an edge and still be the square end of
/// its round.
const SQUARE: f64 = 1e-12;

/// `x > t`, false when either is NaN.
fn exceeds(x: f64, t: f64) -> bool {
    matches!(x.partial_cmp(&t), Some(std::cmp::Ordering::Greater))
}

/// `x <= t`, false when either is NaN.
fn at_most(x: f64, t: f64) -> bool {
    matches!(
        x.partial_cmp(&t),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

/// A side of a corner patch's net: its control points, and its (u, v) at its parameter 0 and 1.
type Side2<'a> = (&'a [[f64; 4]], [f64; 2], [f64; 2]);

/// A plane's origin, its two axes and its unit natural normal, `u × v`.
#[derive(Clone, Copy)]
struct Plane {
    origin: V3,
    u: V3,
    v: V3,
    normal: V3,
}

impl Plane {
    fn of(s: &Surface) -> Option<Plane> {
        let (origin, u, v) = match s {
            Surface::Plane {
                origin,
                u_axis,
                v_axis,
            } => (*origin, *u_axis, *v_axis),
            Surface::PlaneThrough {
                origin,
                u_point,
                v_point,
            } => (*origin, sub(*u_point, *origin), sub(*v_point, *origin)),
            _ => return None,
        };
        let normal = unit(cross(u, v))?;
        Some(Plane {
            origin,
            u,
            v,
            normal,
        })
    }

    /// The (u, v) of `p`, projected onto the plane.
    fn uv(&self, p: V3) -> Option<[f64; 2]> {
        let d = sub(p, self.origin);
        let (uu, uv, vv) = (
            dot(self.u, self.u),
            dot(self.u, self.v),
            dot(self.v, self.v),
        );
        let (a, b) = (dot(self.u, d), dot(self.v, d));
        let det = uu * vv - uv * uv;
        if !exceeds(det.abs(), 1e-300) {
            return None;
        }
        Some([(a * vv - b * uv) / det, (b * uu - a * uv) / det])
    }
}

/// A plane face as a round meets it: its surface, whether its normal out of the material is
/// the surface's natural normal, and, for a face along the edge, whether its loop runs along
/// the edge (from the edge's start to its end).
#[derive(Clone, Copy)]
pub struct Side<'a> {
    /// The face's surface: a plane.
    pub plane: &'a Surface,
    /// The face's outward normal is the plane's natural normal.
    pub outward: bool,
    /// The face's loop runs along the edge.
    pub forward: bool,
}

/// The unit normal out of the material of a plane face, `None` for another surface.
#[must_use]
pub fn outward_normal(plane: &Surface, outward: bool) -> Option<V3> {
    let p = Plane::of(plane)?;
    Some(if outward {
        p.normal
    } else {
        scale(p.normal, -1.0)
    })
}

/// A round on a straight edge between two planes, on the section through the edge's start.
#[derive(Clone, Copy, Debug)]
pub struct Round {
    radius: f64,
    /// The edge's start, and its unit direction to its end.
    a: V3,
    d: V3,
    /// Each side's outward normal.
    n: [V3; 2],
    /// +1 at a convex edge (material taken away), -1 at a concave one (material added).
    s: f64,
    /// The axis's point and the contacts on the start's section, from `a`.
    centre: V3,
    contact: [V3; 2],
}

/// Where a round ends: its section's centre on the axis and its contact on each side.
#[derive(Clone, Copy, Debug)]
pub struct End {
    /// The section's centre.
    pub centre: V3,
    /// The contact on side 0's face and on side 1's.
    pub contacts: [V3; 2],
}

impl Round {
    /// The round of `radius` on the straight edge from `a` to `b` between the planes `sides`;
    /// `None` where the edge is not straight between them, the planes are tangent or fold back
    /// on each other, or the radius is not a size.
    #[must_use]
    pub fn new(a: V3, b: V3, sides: [Side<'_>; 2], radius: f64) -> Option<Round> {
        if !(radius > 0.0 && radius.is_finite()) {
            return None;
        }
        let d = unit(sub(b, a))?;
        let mut n = [[0.0; 3]; 2];
        let mut m = [[0.0; 3]; 2];
        for ((side, nk), mk) in sides.iter().zip(&mut n).zip(&mut m) {
            let plane = Plane::of(side.plane)?;
            *nk = outward_normal(side.plane, side.outward)?;
            // the edge lies in the plane
            if dot(plane.normal, d).abs() > 1e-9
                || dot(plane.normal, sub(a, plane.origin)).abs()
                    > 1e-9 * norm(sub(a, plane.origin)).max(1.0)
            {
                return None;
            }
            // the face lies to the left of its loop seen from its natural normal
            let along = if side.forward { d } else { scale(d, -1.0) };
            *mk = unit(cross(plane.normal, along))?;
        }
        let ([n0, n1], [m0, m1]) = (n, m);
        let bend = dot(m0, n1);
        if !exceeds(bend.abs(), 1e-6) || bend * dot(m1, n0) <= 0.0 {
            return None;
        }
        let s = if bend < 0.0 { 1.0 } else { -1.0 };
        let cos = dot(n0, n1);
        if !exceeds(1.0 + cos, 1e-6) {
            return None;
        }
        // the axis: n_k . x = -s r on the section through `a`, x = c (n0 + n1)
        let centre = scale(add(n0, n1), -s * radius / (1.0 + cos));
        let contact = [
            add(centre, scale(n0, s * radius)),
            add(centre, scale(n1, s * radius)),
        ];
        // each contact lies on its face's side of the edge
        if !(dot(contact[0], m0) > 1e-9 * radius && dot(contact[1], m1) > 1e-9 * radius) {
            return None;
        }
        Some(Round {
            radius,
            a,
            d,
            n,
            s,
            centre,
            contact,
        })
    }

    /// Whether the edge is convex (the round takes material away).
    #[must_use]
    pub fn convex(&self) -> bool {
        self.s > 0.0
    }

    /// The radius.
    #[must_use]
    pub fn radius(&self) -> f64 {
        self.radius
    }

    /// The round's end at its vertex `v` on the plane `end` square to the edge (its outward
    /// normal by `outward`), `None` where that plane is not square to it.
    #[must_use]
    pub fn open_end(&self, v: V3, end: &Surface, outward: bool) -> Option<End> {
        let nf = outward_normal(end, outward)?;
        if !at_most(1.0 - dot(nf, self.d).abs(), SQUARE) {
            return None;
        }
        Some(End {
            centre: add(v, self.centre),
            contacts: [add(v, self.contact[0]), add(v, self.contact[1])],
        })
    }

    /// The round's end at a corner's ball centred at `ball`, `None` where the ball's centre is
    /// not on the round's axis.
    #[must_use]
    pub fn corner_end(&self, ball: V3) -> Option<End> {
        let off = sub(ball, add(self.a, self.centre));
        let across = sub(off, scale(self.d, dot(off, self.d)));
        if norm(across) > 1e-9 * self.radius.max(1.0) {
            return None;
        }
        Some(End {
            centre: ball,
            contacts: [
                add(ball, scale(self.n[0], self.s * self.radius)),
                add(ball, scale(self.n[1], self.s * self.radius)),
            ],
        })
    }

    /// Whether the round runs forward along its edge from end `ends[0]` (at its start) to
    /// `ends[1]`: its face has a length.
    #[must_use]
    pub fn runs_between(&self, ends: [&End; 2]) -> bool {
        dot(sub(ends[1].centre, ends[0].centre), self.d) > 1e-6 * self.radius
    }

    /// A box holding the material the round takes away or adds, whatever its ends: the edge from
    /// `a` to `b` and the round's section at both.
    #[must_use]
    pub fn reach(&self, b: V3) -> Option<Bounds> {
        let mut points = vec![self.a, b];
        for at in [self.a, b] {
            points.push(add(at, self.centre));
            points.extend(self.contact.iter().map(|c| add(at, *c)));
        }
        bounds(&points)
    }
}

/// The centre of the ball of `radius` touching the three planes `faces` (each with its outward
/// normal's side) on the material's side at a convex corner, outside it at a concave one; `v`
/// the vertex where they meet. `None` where the planes nearly share a line.
#[must_use]
pub fn ball_centre(v: V3, faces: [(&Surface, bool); 3], convex: bool, radius: f64) -> Option<V3> {
    let [a, b, c] = [
        outward_normal(faces[0].0, faces[0].1)?,
        outward_normal(faces[1].0, faces[1].1)?,
        outward_normal(faces[2].0, faces[2].1)?,
    ];
    let det = dot(a, cross(b, c));
    if !exceeds(det.abs(), 1e-6) {
        return None;
    }
    // x with n_i . x = h: x = h (b x c + c x a + a x b) / det
    let h = if convex { -radius } else { radius };
    let sum = add(add(cross(b, c), cross(c, a)), cross(a, b));
    Some(add(v, scale(sum, h / det)))
}

/// Whether two points of a construction of `size` are one point.
#[must_use]
pub fn same_point(p: V3, q: V3, size: f64) -> bool {
    norm(sub(p, q)) <= 1e-9 * size.max(1.0)
}

/// The parameter of the point `p` on the line `line`.
#[must_use]
pub fn line_parameter(line: &Curve, p: V3) -> Option<f64> {
    let Curve::Line { start, end } = line else {
        return None;
    };
    let dir = sub(*end, *start);
    let l2 = dot(dir, dir);
    (l2 > 0.0).then(|| dot(sub(p, *start), dir) / l2)
}

/// Whether the interval `[lo, hi]` of a cut-back edge still has a length.
#[must_use]
pub fn interval_has_length(interval: [f64; 2]) -> bool {
    let [lo, hi] = interval;
    hi - lo > 1e-9 * lo.abs().max(hi.abs()).max(1.0)
}

/// The rational quadratic form of the arc about `centre` of `radius` from `p0` to `p1` (less
/// than a half turn), over [0, 1]: one span to a square turn, two past it, each span's middle
/// control point where the tangents at its ends cross, its weight the cosine of half its turn
/// (Piegl & Tiller, A7.1). Its knots and homogeneous control points.
fn arc_form(centre: V3, radius: f64, p0: V3, p1: V3) -> Option<(Vec<f64>, Vec<[f64; 4]>)> {
    let (a, b) = (sub(p0, centre), sub(p1, centre));
    let x = unit(a)?;
    let y = unit(sub(b, scale(x, dot(b, x))))?;
    let angle = dot(b, y).atan2(dot(b, x));
    if !(angle > 1e-6 && angle < std::f64::consts::PI - 1e-6) {
        return None;
    }
    let h = |p: V3, w: f64| [p[0] * w, p[1] * w, p[2] * w, w];
    if angle <= std::f64::consts::FRAC_PI_2 + 1e-12 {
        // one span: the tangents cross at c + (a + b) / (1 + cos), exact for a square turn
        let cos = dot(a, b) / (radius * radius);
        let w = (0.5 * (1.0 + cos)).sqrt();
        let pm = add(centre, scale(add(a, b), 1.0 / (1.0 + cos)));
        return Some((
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![h(p0, 1.0), h(pm, w), h(p1, 1.0)],
        ));
    }
    // two spans of half the turn each
    let half = angle / 2.0;
    let w = (half / 2.0).cos();
    let at = |t: f64, r: f64| add(centre, add(scale(x, r * t.cos()), scale(y, r * t.sin())));
    let tip = radius / w;
    Some((
        vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        vec![
            h(p0, 1.0),
            h(at(half / 2.0, tip), w),
            h(at(half, radius), 1.0),
            h(at(1.5 * half, tip), w),
            h(p1, 1.0),
        ],
    ))
}

/// The point at `t` in [0, 1] of an arc's rational quadratic form ([`arc_form`]): its span's
/// Bézier curve at the span's own parameter.
fn arc_point(knots: &[f64], h: &[[f64; 4]], t: f64) -> Option<V3> {
    let spans = h.len().checked_sub(1)? / 2;
    if spans == 0 || h.len() != 2 * spans + 1 || knots.len() != h.len() + 3 {
        return None;
    }
    let n = spans as f64;
    let i = ((t * n).floor().max(0.0) as usize).min(spans - 1);
    let local = t * n - i as f64;
    bezier(h.get(2 * i..2 * i + 3)?, local)
}

/// The arc about `centre` of `radius` from `p0` to `p1`, less than a half turn: its circle, and
/// its rational quadratic form over [0, 1] ([`arc_form`]).
#[must_use]
pub fn arc(centre: V3, radius: f64, p0: V3, p1: V3) -> Option<(Curve, Curve)> {
    let x = unit(sub(p0, centre))?;
    let to1 = sub(p1, centre);
    let y = unit(sub(to1, scale(x, dot(to1, x))))?;
    let (knots, homogeneous) = arc_form(centre, radius, p0, p1)?;
    Some((
        Curve::Circle {
            frame: Frame {
                origin: centre,
                x_axis: x,
                y_axis: y,
            },
            radius,
        },
        Curve::RationalSpline { knots, homogeneous },
    ))
}

/// A round's face: its cylinder and the net it is parameterised on (u along the section's arc,
/// v along the edge in its contact lines' parameter), its domain, its contact lines and the
/// pcurves of its loop.
#[derive(Clone, Debug)]
pub struct RoundFace {
    /// The cylinder.
    pub label: Surface,
    /// The net: the start's end arc, as its rational quadratic, swept along the edge.
    pub net: Surface,
    /// The net's box.
    pub domain: [[f64; 2]; 2],
    /// The side (0 or 1) at u = 0; the natural normal points away from the axis.
    pub u0_side: usize,
    /// Side 0's contact line and side 1's, and the interval of each between the round's ends.
    pub lines: [Curve; 2],
    /// The interval of both lines between the ends: [0, 1], or [1/4, 3/4] of a line run on half
    /// its length past an open end, as the fillet sessions run theirs.
    pub interval: [f64; 2],
    /// The pcurve of the end arc at the start and at the end, each arc running the way the
    /// face's loop does: the start's the way u does, from side `u0_side`'s contact to the
    /// other's, the end's back.
    pub arcs: [Curve2; 2],
    /// The pcurve of side 0's contact line and of side 1's, each from the start to the end.
    pub contacts: [Curve2; 2],
}

/// The face of `round` between its ends `ends` (at the edge's start and end); `open` says which
/// of them are open ends (on a plane square to the edge, [`Round::open_end`]).
#[must_use]
pub fn round_face(round: &Round, ends: [&End; 2], open: [bool; 2]) -> Option<RoundFace> {
    let [e0, e1] = ends;
    let [p0, p1] = e0.contacts;
    let c0 = e0.centre;
    let length = dot(sub(e1.centre, c0), round.d);
    // the contact lines' parameter: the edge between the ends, run on half its length past
    // each end when either is open
    let (t0, t1) = if open.contains(&true) {
        (0.25, 0.75)
    } else {
        (0.0, 1.0)
    };
    let per = length / (t1 - t0);
    let line = |a: V3, b: V3| Curve::Line {
        start: sub(a, scale(round.d, t0 * per)),
        end: add(b, scale(round.d, (1.0 - t1) * per)),
    };
    let lines = [
        line(e0.contacts[0], e1.contacts[0]),
        line(e0.contacts[1], e1.contacts[1]),
    ];
    // u runs from the side where the natural normal, du x dv, points away from the axis
    let toward1 = sub(p1, p0);
    let u0_side = usize::from(!exceeds(dot(cross(toward1, round.d), sub(p0, c0)), 0.0));
    let (q0, q2) = if u0_side == 0 { (p0, p1) } else { (p1, p0) };
    let (u_knots, section) = arc_form(c0, round.radius, q0, q2)?;
    // each control point of the section, and it moved along the edge to the end's section
    let row = |c: &[f64; 4]| -> Vec<[f64; 4]> {
        let [x, y, z, w] = *c;
        let top = scale(round.d, length * w);
        vec![*c, [x + top[0], y + top[1], z + top[2], w]]
    };
    let net = Surface::RationalSpline {
        u_knots,
        v_knots: vec![t0, t0, t1, t1],
        homogeneous: section.iter().map(row).collect(),
    };
    // the label as the net runs: x to u = 0, y the way u goes, z = x × y along v
    let x = unit(sub(q0, c0))?;
    let y = unit(sub(sub(q2, c0), scale(x, dot(sub(q2, c0), x))))?;
    let line2 = |knots: [f64; 2], a: [f64; 2], b: [f64; 2]| Curve2::Spline {
        knots: vec![knots[0], knots[0], knots[1], knots[1]],
        points: vec![a, b],
    };
    // side 0's contact line at u = ua, side 1's at u = ub
    let (ua, ub) = if u0_side == 0 { (0.0, 1.0) } else { (1.0, 0.0) };
    Some(RoundFace {
        // its origin on the axis at the lines' parameter 0
        label: Surface::Cylinder {
            frame: Frame {
                origin: sub(c0, scale(round.d, t0 * per)),
                x_axis: x,
                y_axis: y,
            },
            radius: round.radius,
        },
        net,
        domain: [[0.0, 1.0], [t0, t1]],
        u0_side,
        lines,
        interval: [t0, t1],
        arcs: [
            line2([0.0, 1.0], [0.0, t0], [1.0, t0]),
            line2([0.0, 1.0], [1.0, t1], [0.0, t1]),
        ],
        contacts: [
            line2([t0, t1], [ua, t0], [ua, t1]),
            line2([t0, t1], [ub, t0], [ub, t1]),
        ],
    })
}

/// A corner's face: the ball's triangle as a patch carrying its sphere, its net and its box.
#[derive(Clone, Debug)]
pub struct CornerFace {
    /// The ball patch's label (its sphere).
    pub label: Surface,
    /// The patch: one rational Bézier patch, `homogeneous[i][j]` with i along u.
    pub net: Surface,
    /// Its box.
    pub domain: [[f64; 2]; 2],
    /// Its normal points out of the material.
    centre: V3,
    convex: bool,
}

/// The corner where three rounds of `radius` meet: the ball centred at `ball`'s triangle
/// between the `contacts`, its normal out of the material (`convex`: the rounds take material
/// away). `None` where the triangle is not one patch.
///
/// # Errors
///
/// A cancellation, and the kernel's failures building the patch.
pub fn corner_face(
    k: &Kernel,
    ball: V3,
    radius: f64,
    contacts: [V3; 3],
    convex: bool,
) -> crate::Result<Option<CornerFace>> {
    let v3 = |a: V3| Vec3::new(a[0], a[1], a[2]);
    let dirs: Option<Vec<Vec3>> = contacts
        .iter()
        .map(|p| unit(sub(*p, ball)).map(v3))
        .collect();
    let Some([d0, d1, d2]) = dirs.and_then(|d| <[Vec3; 3]>::try_from(d).ok()) else {
        return Ok(None);
    };
    let Some(surface) = ball_triangle_face(k, v3(ball), radius, &[d0, d1, d2], !convex)? else {
        return Ok(None);
    };
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let face = record_face(k, &surface, &domain)?;
    let (Surface::BallPatch { .. }, Some(net @ Surface::RationalSpline { .. }), true) =
        (&face.surface, &face.nurbs, face.agrees)
    else {
        return Ok(None);
    };
    Ok(Some(CornerFace {
        label: face.surface.clone(),
        net: net.clone(),
        domain: face.domain,
        centre: ball,
        convex,
    }))
}

impl CornerFace {
    /// Whether the loop through the contacts `corners` (in its order) runs clockwise about the
    /// face's outward normal, so that it must be turned; `None` where they lie on one line.
    #[must_use]
    pub fn turns(&self, corners: [V3; 3]) -> Option<bool> {
        let [pa, pb, pc] = corners;
        let middle = scale(add(add(pa, pb), pc), 1.0 / 3.0);
        let out = scale(
            sub(middle, self.centre),
            if self.convex { 1.0 } else { -1.0 },
        );
        let turn = dot(cross(sub(pb, pa), sub(pc, pa)), out);
        (turn != 0.0 && turn.is_finite()).then_some(turn < 0.0)
    }

    /// The pcurve on the patch of `arc` (an end arc's rational quadratic, [`arc`]), lying
    /// along one of the patch's sides: that side's (u, v) line run in the arc's own parameter.
    /// Two rational parametrisations of one circular arc with the same ends differ by a linear
    /// fractional change of parameter fixing them, so the pcurve is a rational line, its
    /// weights from where the side passes the arc's middle. `None` where the arc is not along
    /// a side, or the pcurve does not lift onto it.
    #[must_use]
    pub fn pcurve(&self, arc: &Curve) -> Option<Curve2> {
        let Curve::RationalSpline { homogeneous: a, .. } = arc else {
            return None;
        };
        // one span: a side of the patch is one rational quadratic
        if a.len() != 3 {
            return None;
        }
        let Surface::RationalSpline {
            homogeneous: net, ..
        } = &self.net
        else {
            return None;
        };
        let (first, last) = (net.first()?, net.last()?);
        let [[u0, u1], [v0, v1]] = self.domain;
        let far: Vec<[f64; 4]> = net
            .iter()
            .map(|row| row.last().copied())
            .collect::<Option<_>>()?;
        // the sides: (control points along it, its (u, v) at the side's parameter 0 and 1)
        let sides: [Side2; 3] = [
            (first, [u0, v0], [u0, v1]),
            (last, [u1, v0], [u1, v1]),
            (&far, [u0, v1], [u1, v1]),
        ];
        let (start, end) = (bezier(a, 0.0)?, bezier(a, 1.0)?);
        let size = norm(sub(end, start)).max(1.0);
        for (side, uv0, uv1) in sides {
            let (s0, s1) = (bezier(side, 0.0)?, bezier(side, 1.0)?);
            let forward = if same_point(s0, start, size) && same_point(s1, end, size) {
                true
            } else if same_point(s0, end, size) && same_point(s1, start, size) {
                false
            } else {
                continue;
            };
            // the side's parameter at the arc's middle, by bisection along the chord
            let middle = bezier(a, 0.5)?;
            let chord = sub(s1, s0);
            let (mut lo, mut hi) = (0.0f64, 1.0f64);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if dot(sub(bezier(side, mid)?, middle), chord) < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let pm = 0.5 * (lo + hi);
            if !(pm > 1e-9 && pm < 1.0 - 1e-9) {
                return None;
            }
            let ratio = pm / (1.0 - pm);
            // forward: p(t) = l t / ((1 - t) + l t); backward: p(t) = l (1 - t) / (l (1 - t) + t)
            let at = |t: f64| {
                if forward {
                    ratio * t / ((1.0 - t) + ratio * t)
                } else {
                    ratio * (1.0 - t) / (ratio * (1.0 - t) + t)
                }
            };
            for t in [0.25, 0.75] {
                if !same_point(bezier(side, at(t))?, bezier(a, t)?, size) {
                    return None;
                }
            }
            let (uv_a, uv_b, wa, wb) = if forward {
                (uv0, uv1, 1.0, ratio)
            } else {
                (uv1, uv0, ratio, 1.0)
            };
            return Some(Curve2::RationalSpline {
                knots: vec![0.0, 0.0, 1.0, 1.0],
                homogeneous: vec![
                    [uv_a[0] * wa, uv_a[1] * wa, wa],
                    [uv_b[0] * wb, uv_b[1] * wb, wb],
                ],
            });
        }
        None
    }
}

/// A round face of a body built from [`round_face`], for [`keep_definitions`]: the face, the
/// two plane faces it rolls on, its radius and a point of its axis.
#[derive(Clone, Copy, Debug)]
pub struct KeptRound {
    /// The round's face.
    pub face: FaceId,
    /// The faces it rolls on: side 0's and side 1's.
    pub supports: [FaceId; 2],
    /// Its radius.
    pub radius: f64,
    /// A point of its axis.
    pub centre: V3,
}

/// A corner face of a body built from [`corner_face`], for [`keep_definitions`]: the face, its
/// ball's centre and radius.
#[derive(Clone, Copy, Debug)]
pub struct KeptCorner {
    /// The corner's face.
    pub face: FaceId,
    /// The ball's centre.
    pub centre: V3,
    /// The ball's radius.
    pub radius: f64,
}

/// Each round's and corner's face of `b` given the definition the fillet sessions give theirs:
/// a round, the exact round of the line class on its two planes (#2485); a corner, the ball's
/// corner on its sphere bounded by its loop's curves (#2357); each certified on its face's net,
/// as the sessions certify theirs.
///
/// # Errors
///
/// A face, edge or surface `b` does not have; the definitions' construction and certificates.
pub fn keep_definitions(
    k: &Kernel,
    b: &mut Brep,
    rounds: &[KeptRound],
    corners: &[KeptCorner],
) -> crate::Result<()> {
    use crate::blend::fillet_exact::ClassInputs;
    use crate::math::Point2;
    use crate::nurbs::knots::Side as KnotSide;
    use crate::surface::procedural_blend::corner::CornerDefinition;
    use crate::surface::procedural_blend::exact::{ExactClass, ExactRound};
    let v3 = |a: V3| Vec3::new(a[0], a[1], a[2]);
    let missing = || crate::SolidError::invalid_input(OP_KEEP, "a face without its surface");
    for round in rounds {
        let centre = v3(round.centre);
        let mut supports = Vec::new();
        let mut uv = [Point2::new(0.0, 0.0); 2];
        let mut offsets = [0.0; 2];
        for ((&f, uv), offset) in round.supports.iter().zip(&mut uv).zip(&mut offsets) {
            let face = b.face(f)?;
            let surface = face.surface.as_ref().ok_or_else(missing)?;
            *uv = face.uv_domain.evaluate(0.5, 0.5);
            let at = surface.evaluate_point(k, *uv)?;
            let normal = surface.evaluate_normal(k, *uv, KnotSide::Left, KnotSide::Left)?;
            *offset = (centre - at).dot(normal);
            supports.push(surface.copy(k)?);
        }
        let [s0, s1] = supports.as_slice() else {
            return Err(missing());
        };
        let inputs = ClassInputs {
            uv,
            offsets,
            centre,
            tol: b.tolerance(),
            ball_on_collapsed: false,
            radii_can_change: false,
        };
        let definition =
            ExactRound::create(k, ExactClass::Line, [s0, s1], round.radius, None, inputs)?;
        let surface = b
            .face_mut(round.face)?
            .surface
            .as_mut()
            .ok_or_else(missing)?;
        definition.recertify(k, surface)?;
        surface.set_exact_round(Some(definition));
    }
    for corner in corners {
        let mut boundary = Vec::new();
        for e in crate::topology::face::p4::edges(b, corner.face, None, false, None)? {
            let curve = b.edge(e)?.curve.as_ref().ok_or_else(missing)?;
            boundary.push(crate::blend::fillet_corner::p1::nurbs_curve(curve)?.clone());
        }
        let definition = CornerDefinition::ball(v3(corner.centre), corner.radius, boundary)?;
        let surface = b
            .face_mut(corner.face)?
            .surface
            .as_mut()
            .ok_or_else(missing)?;
        definition.recertify(k, surface)?;
        surface.set_corner_definition(definition);
    }
    Ok(())
}

const OP_KEEP: &str = "blend::exact_round::keep_definitions";

/// A rational Bézier curve's point at `p` in [0, 1], from its homogeneous control points.
fn bezier(h: &[[f64; 4]], p: f64) -> Option<V3> {
    let mut q: Vec<[f64; 4]> = h.to_vec();
    while q.len() > 1 {
        q = q
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some([
                    a[0] + p * (b[0] - a[0]),
                    a[1] + p * (b[1] - a[1]),
                    a[2] + p * (b[2] - a[2]),
                    a[3] + p * (b[3] - a[3]),
                ]),
                _ => None,
            })
            .collect();
    }
    let [x, y, z, w] = *q.first()?;
    (w > 0.0).then(|| [x / w, y / w, z / w])
}

/// The pcurve on `plane` of an edge on `curve` (a line, or a circle on its rational form
/// `nurbs`) over `interval`, exact: the plane's (u, v) are an affine image of space; with the
/// (u, v) of its control points, which hold it. `None` for another curve.
#[must_use]
pub fn plane_pcurve(
    plane: &Surface,
    curve: &Curve,
    nurbs: Option<&Curve>,
    interval: [f64; 2],
) -> Option<(Curve2, Vec<[f64; 2]>)> {
    let plane = Plane::of(plane)?;
    let [lo, hi] = interval;
    match (curve, nurbs) {
        (Curve::Line { start, end }, None) => {
            let dir = sub(*end, *start);
            let a = plane.uv(add(*start, scale(dir, lo)))?;
            let b = plane.uv(add(*start, scale(dir, hi)))?;
            Some((
                Curve2::Spline {
                    knots: vec![lo, lo, hi, hi],
                    points: vec![a, b],
                },
                vec![a, b],
            ))
        }
        (Curve::Circle { .. }, Some(Curve::RationalSpline { knots, homogeneous })) => {
            let mut h = Vec::new();
            let mut pts = Vec::new();
            for c in homogeneous {
                let w = c[3];
                if !exceeds(w, 0.0) {
                    return None;
                }
                let p = plane.uv([c[0] / w, c[1] / w, c[2] / w])?;
                h.push([p[0] * w, p[1] * w, w]);
                pts.push(p);
            }
            Some((
                Curve2::RationalSpline {
                    knots: knots.clone(),
                    homogeneous: h,
                },
                pts,
            ))
        }
        _ => None,
    }
}

/// `domain` grown round the (u, v) `points`.
#[must_use]
pub fn grown(domain: [[f64; 2]; 2], points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let [[mut u0, mut u1], [mut v0, mut v1]] = domain;
    for p in points {
        u0 = u0.min(p[0]);
        u1 = u1.max(p[0]);
        v0 = v0.min(p[1]);
        v1 = v1.max(p[1]);
    }
    [[u0, u1], [v0, v1]]
}

/// An edge of a plane face for [`plane_loops_hold`].
pub struct PlaneEdge<'a> {
    /// Its curve.
    pub curve: &'a Curve,
    /// The curve's rational form, where it has one.
    pub nurbs: Option<&'a Curve>,
    /// Its interval on the curve.
    pub interval: [f64; 2],
    /// Its vertices.
    pub vertices: [u32; 2],
    /// Their points.
    pub points: [V3; 2],
    /// Whether the rounds made it or cut it back.
    pub changed: bool,
    /// The face's loop it is on.
    pub r#loop: usize,
}

/// What a plane face's edge is in the face's (u, v).
enum Piece {
    /// A polyline along it: exact for a line, on the curve for an arc.
    Path(Vec<[f64; 2]>),
    /// A box round it (a curve this check does not follow).
    Box([f64; 2], [f64; 2]),
}

/// Whether the loops of a plane face (on `plane`, its box `domain`) are simple and nested: no
/// changed edge crosses or touches another of the face's edges but at a vertex they share, and
/// every loop but the outer lies inside the outer. `None` where an edge cannot be followed.
#[must_use]
pub fn plane_loops_hold(
    plane: &Surface,
    domain: [[f64; 2]; 2],
    edges: &[PlaneEdge<'_>],
    tolerance: f64,
) -> Option<bool> {
    let plane = Plane::of(plane)?;
    let mut pieces: Vec<Piece> = Vec::new();
    for e in edges {
        let piece = match (e.curve, e.nurbs) {
            (Curve::Line { start, end }, None) => {
                let dir = sub(*end, *start);
                let [lo, hi] = e.interval;
                Piece::Path(vec![
                    plane.uv(add(*start, scale(dir, lo)))?,
                    plane.uv(add(*start, scale(dir, hi)))?,
                ])
            }
            (Curve::Circle { .. }, Some(Curve::RationalSpline { knots, homogeneous }))
                if e.changed && e.interval == [0.0, 1.0] =>
            {
                let mut path = Vec::new();
                for i in 0..=32 {
                    path.push(plane.uv(arc_point(knots, homogeneous, f64::from(i) / 32.0)?)?);
                }
                Piece::Path(path)
            }
            _ => {
                let pts: Vec<[f64; 2]> = curve_points(e.curve, e.nurbs, e.interval)?
                    .iter()
                    .map(|p| plane.uv(*p))
                    .collect::<Option<_>>()?;
                let lo = pts
                    .iter()
                    .fold([f64::INFINITY; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
                let hi = pts.iter().fold([f64::NEG_INFINITY; 2], |m, p| {
                    [m[0].max(p[0]), m[1].max(p[1])]
                });
                Piece::Box(lo, hi)
            }
        };
        pieces.push(piece);
    }
    let [[u0, u1], [v0, v1]] = domain;
    let eps = 1e-9 * (u1 - u0).abs().max((v1 - v0).abs()).max(1.0) + 10.0 * tolerance;
    for (i, (ea, pa)) in edges.iter().zip(&pieces).enumerate() {
        if !ea.changed {
            continue;
        }
        let Piece::Path(a) = pa else {
            return None;
        };
        for (j, (eb, pb)) in edges.iter().zip(&pieces).enumerate() {
            if i == j {
                continue;
            }
            let shared: Vec<[f64; 2]> = ea
                .vertices
                .iter()
                .zip(&ea.points)
                .filter(|(v, _)| eb.vertices.contains(v))
                .map(|(_, p)| plane.uv(*p))
                .collect::<Option<_>>()?;
            let at_shared = |x: [f64; 2]| {
                shared
                    .iter()
                    .any(|s| (x[0] - s[0]).hypot(x[1] - s[1]) <= eps)
            };
            match pb {
                Piece::Path(b) => {
                    for sa in a.windows(2) {
                        for sb in b.windows(2) {
                            if let ([a0, a1], [b0, b1]) = (sa, sb) {
                                if let Some(x) = segments_meet(*a0, *a1, *b0, *b1, eps) {
                                    if !at_shared(x) {
                                        return Some(false);
                                    }
                                }
                            }
                        }
                    }
                }
                Piece::Box(lo, hi) => {
                    let inside = |p: &[f64; 2]| {
                        p[0] >= lo[0] - eps
                            && p[0] <= hi[0] + eps
                            && p[1] >= lo[1] - eps
                            && p[1] <= hi[1] + eps
                    };
                    let corners = [*lo, [hi[0], lo[1]], *hi, [lo[0], hi[1]], *lo];
                    let crosses = a.windows(2).any(|sa| {
                        corners.windows(2).any(|c| match (sa, c) {
                            ([a0, a1], [c0, c1]) => {
                                segments_meet(*a0, *a1, *c0, *c1, eps).is_some()
                            }
                            _ => true,
                        })
                    });
                    if a.iter().any(inside) || crosses {
                        return Some(false);
                    }
                }
            }
        }
    }
    // nesting: the loop of the largest area is the outer, every other one lies inside it
    let loops = edges.iter().map(|e| e.r#loop).max().map_or(0, |m| m + 1);
    if loops > 1 {
        let polys: Vec<Vec<[f64; 2]>> = (0..loops)
            .map(|li| {
                edges
                    .iter()
                    .zip(&pieces)
                    .filter(|(e, _)| e.r#loop == li)
                    .flat_map(|(_, p)| match p {
                        Piece::Path(x) => x.clone(),
                        Piece::Box(lo, hi) => vec![*lo, *hi],
                    })
                    .collect()
            })
            .collect();
        let area = |p: &Vec<[f64; 2]>| -> f64 {
            p.iter()
                .zip(p.iter().cycle().skip(1))
                .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                .sum::<f64>()
                .abs()
        };
        let (outer, ring) = polys
            .iter()
            .enumerate()
            .max_by(|a, b| area(a.1).total_cmp(&area(b.1)))?;
        for (li, p) in polys.iter().enumerate() {
            if li != outer && !p.iter().all(|x| inside_polygon(*x, ring)) {
                return Some(false);
            }
        }
    }
    Some(true)
}

/// Where segments `a0 a1` and `b0 b1` meet or come within `eps`, if they do.
fn segments_meet(
    a0: [f64; 2],
    a1: [f64; 2],
    b0: [f64; 2],
    b1: [f64; 2],
    eps: f64,
) -> Option<[f64; 2]> {
    let cr = |p: [f64; 2], q: [f64; 2]| p[0] * q[1] - p[1] * q[0];
    let r = [a1[0] - a0[0], a1[1] - a0[1]];
    let s = [b1[0] - b0[0], b1[1] - b0[1]];
    let (lr, ls) = (r[0].hypot(r[1]), s[0].hypot(s[1]));
    let denom = cr(r, s);
    if denom.abs() > 1e-12 * lr * ls {
        let w = [b0[0] - a0[0], b0[1] - a0[1]];
        let t = cr(w, s) / denom;
        let u = cr(w, r) / denom;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            return Some([a0[0] + t * r[0], a0[1] + t * r[1]]);
        }
    }
    // no crossing: they meet where an end of one is within eps of the other
    let nearest = |p: [f64; 2], o: [f64; 2], d: [f64; 2], l: f64| -> ([f64; 2], f64) {
        let t = if l > 0.0 {
            (((p[0] - o[0]) * d[0] + (p[1] - o[1]) * d[1]) / (l * l)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = [o[0] + t * d[0], o[1] + t * d[1]];
        (q, (p[0] - q[0]).hypot(p[1] - q[1]))
    };
    for (p, o, d, l) in [
        (a0, b0, s, ls),
        (a1, b0, s, ls),
        (b0, a0, r, lr),
        (b1, a0, r, lr),
    ] {
        let (q, gap) = nearest(p, o, d, l);
        if gap <= eps {
            return Some(q);
        }
    }
    None
}

/// Whether `p` is inside the closed polygon `ring` (crossing number).
fn inside_polygon(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The box round `points`.
#[must_use]
pub fn bounds(points: &[V3]) -> Option<Bounds> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for ((l, h), x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            if !x.is_finite() {
                return None;
            }
            *l = l.min(*x);
            *h = h.max(*x);
        }
    }
    lo.iter().all(|x| x.is_finite()).then_some([lo, hi])
}

/// Whether two boxes come within `margin` of each other.
#[must_use]
pub fn overlap(a: &Bounds, b: &Bounds, margin: f64) -> bool {
    let ([alo, ahi], [blo, bhi]) = (a, b);
    alo.iter()
        .zip(ahi)
        .zip(blo.iter().zip(bhi))
        .all(|((al, ah), (bl, bh))| !(*al > bh + margin || *bl > ah + margin))
}

/// The tolerance of the edges and vertices the rounds make on a body whose intersection
/// tolerance is `body`: four times it, as the fillet sessions give theirs (their stitch's), so
/// that what is built after a round meets its edges as it met the sessions'.
#[must_use]
pub fn made_tolerance(body: f64) -> f64 {
    4.0 * body
}

/// The margin two boxes of a round of `radius` on a body of `tolerance` are kept apart by.
#[must_use]
pub fn margin(radius: f64, tolerance: f64) -> f64 {
    1e-6 * radius.max(1.0) + 10.0 * tolerance
}

/// Points whose box holds an edge on `curve` over `interval`: a line's ends, a NURBS form's or
/// a spline's control points. `None` for another curve.
#[must_use]
pub fn curve_points(curve: &Curve, nurbs: Option<&Curve>, interval: [f64; 2]) -> Option<Vec<V3>> {
    match nurbs.unwrap_or(curve) {
        Curve::Line { start, end } => {
            let dir = sub(*end, *start);
            Some(
                interval
                    .iter()
                    .map(|t| add(*start, scale(dir, *t)))
                    .collect(),
            )
        }
        Curve::Spline { points, .. } | Curve::Polyline { points } => Some(points.clone()),
        Curve::RationalSpline { homogeneous, .. } => homogeneous
            .iter()
            .map(|h| (h[3] > 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]))
            .collect(),
        _ => None,
    }
}

/// Points whose box holds a face's surface: its NURBS form's control net (the whole surface),
/// or a spline surface's own. `None` for another surface (a plane's box is its edges').
#[must_use]
pub fn surface_points(surface: &Surface, nurbs: Option<&Surface>) -> Option<Vec<V3>> {
    match nurbs.unwrap_or(surface) {
        Surface::RationalSpline { homogeneous, .. } => homogeneous
            .iter()
            .flatten()
            .map(|h| (h[3] > 0.0).then(|| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]))
            .collect(),
        Surface::Spline { points, .. } => Some(points.iter().flatten().copied().collect()),
        _ => None,
    }
}

/// Whether `surface` is a plane.
#[must_use]
pub fn is_plane(surface: &Surface) -> bool {
    Plane::of(surface).is_some()
}

#[cfg(test)]
mod tests;
