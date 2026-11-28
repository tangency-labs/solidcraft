//! (not upstream) How well an intersection curve of two surfaces is known across itself:
//! an enclosed lower bound of the angle between the two surfaces along it, from the
//! surfaces' definitions, rounding included, never samples.
//!
//! Upstream: none. An intersection curve carries its zone tolerance wherever it runs.
//! Where the two surfaces cross at the angle `theta`, a surface moved by the tolerance
//! moves the curve by tolerance / sin `theta` within the other surface: a curve of two
//! nearly tangent walls is known only that far (docs/booleans.md, "Seams and poles"; the
//! surface analogue of [`Curve::crossing_reach`](crate::curve::curve::Curve)).
//!
//! | Rust                               | Kind                                          |
//! |------------------------------------|-----------------------------------------------|
//! | [`Surface::normal_cone_in_ball`]   | plane, cylinder, cone, sphere, ring torus     |
//! | [`intersection_reach`]             | a curve with a hull (`curve/proximity.rs`)    |
//! | [`crossing_line_in_ball`]          | the two surfaces' crossing over a ball        |
//!
//! **A normal cone.** For a ball about `m` of radius `rho`, a cone (an axis and a half
//! angle) that holds the normal line of the surface at every point of it inside the ball,
//! in closed form:
//! - plane: its normal.
//! - sphere about `c`: the normal at `p` runs along `p - c`, and every point of the ball
//!   is seen from `c` within `asin(rho / |m - c|)` of `m - c`.
//! - cylinder and cone about the axis `(o, a)`: with `q(p)` the part of `p - o` across
//!   `a` (a projection, so `|q(p) - q(m)| <= rho`), the radial direction of `p` lies
//!   within `phi = asin(rho / |q(m)|)` of that of `m`. The normal is the radial direction
//!   less the slope times `a`, normalised: two of them at radial directions `phi` apart
//!   make the angle `acos((cos phi + t^2) / (1 + t^2)) <= phi`. A ball that reaches the
//!   apex's height has none (the slope's sign changes there).
//! - torus about `(c, a)`, major radius `R` above the minor one: the normal at `p` runs
//!   along `p - C(p)`, `C(p) = c + R u(p)` the point of the major circle in `p`'s half
//!   plane, `u` the unit radial direction. `|u(p) - u(m)| <= phi` as above, so
//!   `|(p - C(p)) - (m - C(m))| <= rho + R phi`, seen from the origin within the `asin` of
//!   that over `|m - C(m)|`.
//!
//! A ball that reaches the axis (the centre), or whose cone would be a quarter turn or
//! more, has none. Rounding: every length read at `m` is taken to
//! `e = gamma_16 (|m| + |o| + |R|)`, added to the ball and taken off the distance it is
//! divided by, and the angle grows by `gamma_8` relative and absolute.
//!
//! **The angle.** Over a ball, the two surfaces cross at no less than the angle between
//! the two cones' axes (as lines) less both half angles.
//!
//! **The reach.** The curve is cut into pieces, each in a ball (its chord's middle, half
//! the chord and the piece's hull radius, [`Curve::piece_ball`](crate::curve::curve::Curve)).
//! With the ball widened by `rho`, the angle over it bounds the crossing wherever the
//! exact curve can lie within `rho` of the piece. The reach tolerance / sin `theta`, with
//! `theta` the least angle over all pieces, is proved when it is at most `rho`; `rho`
//! starts at four tolerances and is widened fourfold, [`REACH_WINDOWS`] times at most. A
//! piece is halved while its cones are wide ([`FINE`]) and it is larger than `rho`, at
//! most [`MAX_PIECES`] pieces in all. Where a piece no larger than `rho` proves no angle
//! (the surfaces tangent there, or a kind without a closed form), nothing is proved.
//!
//! Deviations: not a port. Surfaces of other kinds (B-spline, extrusion, revolution,
//! offset, blend, a torus without a hole) have no cone here, and nothing is proved for
//! their curves. No recursion: the pieces wait on a stack bounded by [`MAX_PIECES`].

use crate::curve::curve::Curve;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::nurbs::math::gamma;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;

const OP: &str = "surface::normal_cone::intersection_reach";

/// How many radii, each four times the last, [`intersection_reach`] tries.
pub const REACH_WINDOWS: usize = 8;

/// The most pieces [`intersection_reach`] reads for one radius.
pub const MAX_PIECES: usize = 4096;

/// The two cones' half angles together (radians) below which a piece is not halved
/// further: the angle read is then within this of the least angle over the piece.
pub const FINE: f64 = 0.02;

/// How many times a surface in its STEP parameterisation is unwrapped.
const STEP_DEPTH: usize = 8;

/// A cone that holds the normal line of a surface at every point of it inside a ball.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NormalCone {
    /// The cone's unit axis (either sense: the normals are lines).
    pub(crate) axis: Vec3,
    /// Its half angle in radians, rounding included; below a quarter turn.
    pub(crate) half_angle: f64,
}

/// The angle within which every point of a ball of radius `rho` is seen from a point at
/// the distance `len` (read to `e`) of its centre, rounding included; `None` where the
/// ball reaches that point or a number is not finite.
fn seen_within(rho: f64, len: f64, e: f64) -> Option<f64> {
    let (num, den) = (rho + 2.0 * e, len - e);
    if !(num.is_finite() && den.is_finite() && num >= 0.0 && den > 0.0 && num < den) {
        return None;
    }
    let half = (num / den).asin() * (1.0 + gamma(8)) + gamma(8);
    (half.is_finite() && half < 0.5 * std::f64::consts::PI).then_some(half)
}

/// The cone about `axis` (not yet unit) of the half angle `half_angle`; `None` for an
/// axis of no length.
fn cone_about(axis: Vec3, half_angle: f64) -> Option<NormalCone> {
    let len = axis.length();
    (len.is_finite() && len > 0.0).then(|| NormalCone {
        axis: axis / len,
        half_angle,
    })
}

impl Surface {
    /// A cone holding the normal line of this surface at every point of it within
    /// `radius` of `centre` (module docs): a plane, cylinder, cone, sphere or ring torus,
    /// in closed form with its rounding. `None` for another kind, a ball that reaches the
    /// surface's axis, centre or apex height, or a cone of a quarter turn or more.
    #[must_use]
    pub(crate) fn normal_cone_in_ball(&self, centre: Vec3, radius: f64) -> Option<NormalCone> {
        if !(radius.is_finite() && radius >= 0.0 && centre.length().is_finite()) {
            return None;
        }
        let mut surface = self;
        for _ in 0..STEP_DEPTH {
            match surface.kind() {
                SurfaceKind::Step(s) => surface = s.surface()?,
                _ => break,
            }
        }
        match surface.kind() {
            SurfaceKind::Plane(p) => cone_about(p.position().z_axis(), gamma(8)),
            SurfaceKind::Sphere(s) => {
                let c = s.base.position.origin();
                let v = centre - c;
                let e = gamma(16) * (centre.length() + c.length());
                cone_about(v, seen_within(radius, v.length(), e)?)
            }
            SurfaceKind::Cone(c) => {
                let (o, a) = (c.rev.position.origin(), c.rev.position.z_axis());
                let (r0, slope) = (c.base_radius(), c.tan_semi_angle());
                let v = centre - o;
                let height = v.dot(a);
                let q = v - a * height;
                let e = gamma(16) * (centre.length() + o.length() + r0.abs());
                let phi = seen_within(radius, q.length(), e)?;
                // one nappe over the whole ball: the radius keeps its sign
                let r = r0 + slope * height;
                if r.is_nan() || r.abs() <= slope.abs() * (radius + 2.0 * e) + e {
                    return None;
                }
                let radial = q / q.length();
                cone_about(radial - a * (slope * r.signum()), phi)
            }
            SurfaceKind::Torus(t) => {
                let (o, a) = (t.base.position.origin(), t.base.position.z_axis());
                let (major, minor) = (t.major_radius.abs(), t.minor_radius.abs());
                if major.is_nan() || minor.is_nan() || major <= minor {
                    return None;
                }
                let v = centre - o;
                let q = v - a * v.dot(a);
                let e = gamma(16) * (centre.length() + o.length() + major);
                let phi = seen_within(radius, q.length(), e)?;
                let from_circle = v - q * (major / q.length());
                let spread = radius + major * phi * (1.0 + gamma(4));
                cone_about(from_circle, seen_within(spread, from_circle.length(), e)?)
            }
            _ => None,
        }
    }
}

/// The angle between `a` and `b` as lines, by `atan2` of the cross and the dot products.
fn line_angle(a: Vec3, b: Vec3) -> f64 {
    let between = a.cross(b).length().atan2(a.dot(b));
    between.min(std::f64::consts::PI - between)
}

/// How two surfaces cross over a ball: their normal cones there, proved apart.
#[derive(Clone, Copy, Debug)]
struct Crossing {
    /// A lower bound of the angle between the two surfaces at every point of both in the
    /// ball (radians, above zero).
    theta: f64,
    /// The two cones' half angles together.
    wide: f64,
    /// The two cones' unit axes.
    axes: [Vec3; 2],
}

/// How `s1` and `s2` cross at every point of both within `ball` of `centre`: the angle
/// between the two cones' axes (as lines) less both half angles, rounding included (module
/// docs, "The angle"); `None` where either has no cone there or the angle is not proved
/// above zero.
fn crossing_over_ball(s1: &Surface, s2: &Surface, centre: Vec3, ball: f64) -> Option<Crossing> {
    let (c1, c2) = s1
        .normal_cone_in_ball(centre, ball)
        .zip(s2.normal_cone_in_ball(centre, ball))?;
    let wide = c1.half_angle + c2.half_angle;
    let theta = line_angle(c1.axis, c2.axis) * (1.0 - gamma(8)) - wide;
    (theta > 0.0).then_some(Crossing {
        theta,
        wide,
        axes: [c1.axis, c2.axis],
    })
}

/// Where the intersection of two surfaces runs over a ball ([`crossing_line_in_ball`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct CrossingLine {
    /// A unit direction: the cross product of the two cones' axes.
    pub(crate) line: Vec3,
    /// An upper bound, below one, of the sine of the angle between `line` and the tangent
    /// line of the intersection at every point of it in the ball, rounding included.
    pub(crate) spread: f64,
}

/// Not upstream: where `s1` and `s2` are proved to cross, never tangent, at every point of
/// both within `radius` of `centre`, the line their intersection runs along there. The
/// intersection is then regular in the ball: no branch point, touch point or tangent curve,
/// one smooth curve through each of its points (docs/ssi.md §4.4), whose tangent is
/// `n1 x n2`, `n1` and `n2` the normals there.
///
/// The bound: with `a1`, `a2` the cones' axes, `L` their unit cross product and `n_i` within
/// the half angle `alpha_i` of `a_i`, the height of `n_i` over the plane of the axes is
/// `|n_i . L| <= sin alpha_i`. Write `n_i = p_i + h_i L`, `p_i` in that plane: then
/// `n1 x n2 = p1 x p2 + h2 p1 x L + h1 L x p2`, whose first term runs along `L` and whose
/// others, across `L`, are no longer than `|h1| + |h2| <= alpha1 + alpha2`. The normals'
/// angle as lines lies between `theta` and a quarter turn, so `|n1 x n2| >= sin theta`, and
/// the sine of the angle between `n1 x n2` and `L` is at most `(alpha1 + alpha2) / sin
/// theta`. `L` itself is read to `gamma_8 / sin theta` of the exact cross product's line.
///
/// `None` where a surface is of a kind without a cone here, the ball reaches its axis,
/// centre or apex height, or the crossing is not proved with a bound below one (the surfaces
/// tangent or nearly so in the ball).
#[must_use]
pub(crate) fn crossing_line_in_ball(
    s1: &Surface,
    s2: &Surface,
    centre: Vec3,
    radius: f64,
) -> Option<CrossingLine> {
    let crossing = crossing_over_ball(s1, s2, centre, radius)?;
    let [a1, a2] = crossing.axes;
    let line = a1.cross(a2);
    let len = line.length();
    let sine = crossing.theta.sin() * (1.0 - gamma(4));
    if !(len.is_finite() && len > 0.0 && sine > 0.0) {
        return None;
    }
    let spread = (crossing.wide + gamma(8)) / sine * (1.0 + gamma(4));
    (spread.is_finite() && spread < 1.0).then(|| CrossingLine {
        line: line / len,
        spread,
    })
}

/// A lower bound of the sine of the angle between `s1` and `s2` wherever both have a
/// point within `rho` of `curve` over `interval` (module docs); `None` where a piece no
/// larger than `rho` proves no angle, or the pieces run out.
fn least_crossing_sine(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    surfaces: [&Surface; 2],
    rho: f64,
) -> Result<Option<f64>> {
    let Some(cuts) = curve.hull_breaks(interval.min(), interval.max())? else {
        return Ok(None);
    };
    let mut stack: Vec<(f64, f64)> = cuts
        .windows(2)
        .filter_map(|w| match *w {
            [x, y] if x < y => Some((x, y)),
            _ => None,
        })
        .collect();
    if stack.is_empty() {
        return Ok(None);
    }
    let [s1, s2] = surfaces;
    let mut least = f64::INFINITY;
    let mut tried = 0_usize;
    while let Some((x, y)) = stack.pop() {
        k.check_cancelled(OP)?;
        tried += 1;
        if tried > MAX_PIECES {
            return Ok(None);
        }
        let Some((centre, piece)) = curve.piece_ball(x, y)? else {
            return Ok(None);
        };
        let ball = (piece + rho) * (1.0 + gamma(2));
        let theta = crossing_over_ball(s1, s2, centre, ball).map(|c| (c.theta, c.wide));
        let mid = 0.5 * (x + y);
        let halves = piece > rho && x < mid && mid < y;
        match theta {
            Some((theta, wide)) if wide <= FINE || !halves => {
                least = least.min(theta.sin() * (1.0 - gamma(4)));
            }
            _ if halves => {
                stack.push((mid, y));
                stack.push((x, mid));
            }
            _ => return Ok(None),
        }
    }
    Ok((least.is_finite() && least > 0.0).then_some(least))
}

/// How far across itself the intersection curve `curve` of `s1` and `s2` is known over
/// `interval`, each surface known to `tolerance`: tolerance / sin `theta`, `theta` an
/// enclosed lower bound of the angle between the two surfaces wherever the exact curve
/// can lie (module docs). `None` where no reach is proved: the surfaces are tangent or
/// nearly so somewhere along the curve, or of a kind without a closed form, or the curve
/// has no hull.
///
/// # Errors
///
/// The kernel's cancellation.
pub(crate) fn intersection_reach(
    k: &Kernel,
    curve: &Curve,
    interval: &Extent1d,
    s1: &Surface,
    s2: &Surface,
    tolerance: f64,
) -> Result<Option<f64>> {
    let (a, b) = (interval.min(), interval.max());
    if !(tolerance > 0.0 && tolerance.is_finite() && a.is_finite() && b.is_finite() && a < b) {
        return Ok(None);
    }
    let mut rho = 4.0 * tolerance;
    for _ in 0..REACH_WINDOWS {
        let Some(sine) = least_crossing_sine(k, curve, interval, [s1, s2], rho)? else {
            return Ok(None);
        };
        let reach = tolerance / sine * (1.0 + gamma(4));
        if reach <= rho {
            return Ok(Some(reach));
        }
        rho *= 4.0;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::ellipse::{Ellipse, EllipseOptions};
    use crate::math::Point3;
    use crate::primitive::api_primitives as api;
    use crate::topology::Brep;

    /// The surfaces of a B-rep's faces.
    fn surfaces(b: &Brep) -> Vec<Surface> {
        crate::topology::brep::p1::faces(b, None)
            .unwrap()
            .into_iter()
            .filter_map(|f| crate::topology::face::p2::surface(b, f).unwrap().cloned())
            .collect()
    }

    fn of_kind(b: &Brep, pick: impl Fn(&SurfaceKind) -> bool) -> Surface {
        surfaces(b)
            .into_iter()
            .find(|s| pick(s.kind()))
            .expect("the body has a face of the kind")
    }

    /// A circle about `centre` in the plane across `normal`.
    fn circle(k: &Kernel, centre: Point3, x: Vec3, y: Vec3, radius: f64) -> Curve {
        let domain = Extent1d::from_min_max(0.0, 360.0);
        let opts = EllipseOptions::default();
        Curve::from(Ellipse::new(k, centre, x, y, domain, radius, radius, &opts).unwrap())
    }

    /// The exact normal lines of a unit sphere, a cylinder of radius 2 about z, a cone
    /// and a torus at points inside a ball lie in the ball's cone, and a ball reaching
    /// the axis or the centre has none.
    #[test]
    fn a_normal_cone_holds_the_normals_of_every_point_in_its_ball() {
        type Point = dyn Fn(f64, f64) -> (Vec3, Vec3);
        let mut k = Kernel::default();
        let sphere = of_kind(
            &api::create_sphere(&mut k, Point3::ZERO, 1.0).unwrap(),
            |s| matches!(s, SurfaceKind::Sphere(_)),
        );
        let cylinder = of_kind(
            &api::create_cylinder(&mut k, Point3::ZERO, 2.0, 5.0).unwrap(),
            |s| matches!(s, SurfaceKind::Cone(_)),
        );
        let cone = of_kind(
            &api::create_cone(&mut k, Point3::ZERO, 2.0, 1.0, 3.0).unwrap(),
            |s| matches!(s, SurfaceKind::Cone(_)),
        );
        let torus = of_kind(
            &api::create_torus(&mut k, Point3::ZERO, 3.0, 1.0).unwrap(),
            |s| matches!(s, SurfaceKind::Torus(_)),
        );
        // points on each surface, with their exact normals
        let radius = 0.05;
        let on_sphere = |u: f64, v: f64| {
            let n = Vec3::new(v.cos() * u.cos(), v.cos() * u.sin(), v.sin());
            (n, n)
        };
        let on_cylinder = |u: f64, v: f64| {
            let n = Vec3::new(u.cos(), u.sin(), 0.0);
            (n * 2.0 + Vec3::Z * (2.5 + v), n)
        };
        // radius 2 at z = 0 down to 1 at z = 3: slope -1/3
        let on_cone = |u: f64, v: f64| {
            let h = 1.5 + v;
            let r = 2.0 - h / 3.0;
            let radial = Vec3::new(u.cos(), u.sin(), 0.0);
            (radial * r + Vec3::Z * h, radial + Vec3::Z / 3.0)
        };
        let on_torus = |u: f64, v: f64| {
            let radial = Vec3::new(u.cos(), u.sin(), 0.0);
            let n = radial * v.cos() + Vec3::Z * v.sin();
            (radial * 3.0 + n, n)
        };
        let cases: [(&Surface, &Point); 4] = [
            (&sphere, &on_sphere),
            (&cylinder, &on_cylinder),
            (&cone, &on_cone),
            (&torus, &on_torus),
        ];
        for (surface, point) in cases {
            let (m, _) = point(0.7, 0.3);
            let held = surface.normal_cone_in_ball(m, radius).unwrap();
            let mut widest = 0.0_f64;
            for i in -20..=20 {
                for j in -20..=20 {
                    let (p, n) = point(0.7 + 0.004 * f64::from(i), 0.3 + 0.004 * f64::from(j));
                    if (p - m).length() > radius {
                        continue;
                    }
                    let off = line_angle(held.axis, n);
                    assert!(off <= held.half_angle, "{off} > {}", held.half_angle);
                    widest = widest.max(off);
                }
            }
            // and the cone is no idle bound: a normal uses a good part of it
            assert!(widest > 0.2 * held.half_angle, "{widest} {held:?}");
            assert!(surface.normal_cone_in_ball(Point3::ZERO, 0.1).is_none());
        }
        // a ball across the cone's apex height (z = 6) has none
        assert!(cone
            .normal_cone_in_ball(Point3::new(0.1, 0.0, 5.99), 0.05)
            .is_none());
    }

    /// A plane through a unit sphere at the height `h` crosses it at `acos h`: the
    /// circle's reach is the tolerance over `sqrt(1 - h^2)`, enclosed from above and
    /// within a few per cent. A plane 1e-9 inside the sphere's top is tangent to it as
    /// far as any ball shows, and has no reach.
    #[test]
    fn a_circle_s_reach_on_a_sphere_is_the_tolerance_over_the_crossing_s_sine() {
        let mut k = Kernel::default();
        let sphere = of_kind(
            &api::create_sphere(&mut k, Point3::ZERO, 1.0).unwrap(),
            |s| matches!(s, SurfaceKind::Sphere(_)),
        );
        let block = api::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let plane = surfaces(&block)
            .into_iter()
            .find(|s| match s.kind() {
                SurfaceKind::Plane(p) => p.position().z_axis().z.abs() > 0.5,
                _ => false,
            })
            .expect("the block has a face across z");
        let tol = 1e-5;
        for h in [0.0_f64, 0.5, 0.9, 0.999] {
            let r = (1.0 - h * h).sqrt();
            let c = circle(&k, Point3::new(0.0, 0.0, h), Vec3::X, Vec3::Y, r);
            let ivl = c.natural_interval();
            let reach = intersection_reach(&k, &c, &ivl, &plane, &sphere, tol)
                .unwrap()
                .expect("a crossing has a reach");
            let exact = tol / r;
            assert!(reach >= exact, "h {h}: {reach} < {exact}");
            assert!(reach <= 1.6 * exact, "h {h}: {reach} against {exact}");
            if h == 0.0 {
                assert!(reach <= 1.001 * exact, "{reach}");
            }
        }
        let h: f64 = 1.0 - 1e-9;
        let r = (1.0 - h * h).sqrt();
        let c = circle(&k, Point3::new(0.0, 0.0, h), Vec3::X, Vec3::Y, r);
        let ivl = c.natural_interval();
        assert!(intersection_reach(&k, &c, &ivl, &plane, &sphere, tol)
            .unwrap()
            .is_none());
        // no reach without a tolerance
        let c = circle(&k, Point3::ZERO, Vec3::X, Vec3::Y, 1.0);
        let ivl = c.natural_interval();
        assert!(intersection_reach(&k, &c, &ivl, &plane, &sphere, 0.0)
            .unwrap()
            .is_none());
    }
}
