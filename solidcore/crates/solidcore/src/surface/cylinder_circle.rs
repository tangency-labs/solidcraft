//! The circles and rulings of a cylinder that a gap in its face closes over.

use std::f64::consts::TAU;

use crate::math::Vec3;

/// A cylinder: a point of its axis, its unit axis and its radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    /// A point of the axis.
    pub origin: Vec3,
    /// The unit axis.
    pub axis: Vec3,
    /// The radius.
    pub radius: f64,
}

impl Cylinder {
    /// How far `p` lies along the axis from [`Cylinder::origin`].
    #[must_use]
    pub fn height(&self, p: Vec3) -> f64 {
        (p - self.origin).dot(self.axis)
    }

    /// The point of the axis level with `p`.
    #[must_use]
    pub fn centre_at(&self, p: Vec3) -> Vec3 {
        self.origin + self.axis * self.height(p)
    }

    /// The circle frame at `p`: `x_axis` radial to `p`, `y_axis` the axis crossed with it.
    #[must_use]
    pub fn frame_at(&self, p: Vec3) -> (Vec3, Vec3) {
        let radial = p - self.centre_at(p);
        let x_axis = radial * (1.0 / radial.length());
        (x_axis, self.axis.cross(x_axis))
    }

    /// Whether `p` lies within `tol` of the cylinder's circle at `height`.
    #[must_use]
    pub fn on_circle(&self, p: Vec3, height: f64, tol: f64) -> bool {
        (self.height(p) - height).abs() <= tol
            && ((p - self.centre_at(p)).length() - self.radius).abs() <= tol
    }

    /// Whether the circle of centre `origin`, frame `x_axis` and `y_axis` and `radius` is this
    /// cylinder's circle at `height` within `tol` (a tilt `s` moves its rim `radius * s`).
    #[must_use]
    pub fn is_circle_at(
        &self,
        origin: Vec3,
        x_axis: Vec3,
        y_axis: Vec3,
        radius: f64,
        height: f64,
        tol: f64,
    ) -> bool {
        x_axis.cross(y_axis).cross(self.axis).length() <= tol / radius.max(1.0)
            && (origin - self.centre_at(origin)).length() <= tol
            && (self.height(origin) - height).abs() <= tol
            && (radius - self.radius).abs() <= tol
    }

    /// Whether `p` and `q` lie on one circle of the cylinder's: the same height within `tol`.
    #[must_use]
    pub fn same_circle(&self, p: Vec3, q: Vec3, tol: f64) -> bool {
        (self.height(p) - self.height(q)).abs() <= tol
    }

    /// Whether the line from `start` to `end` runs along the axis within `tol`, as a ruling does.
    #[must_use]
    pub fn is_ruling(&self, start: Vec3, end: Vec3, tol: f64) -> bool {
        (end - start).cross(self.axis).length() <= tol
    }

    /// The centre of the circle where the plane `normal · x = d`, square to the axis, cuts the
    /// cylinder, when `a` and `b` both lie on that circle within `tol`.
    #[must_use]
    pub fn plane_circle(&self, normal: Vec3, d: f64, a: Vec3, b: Vec3, tol: f64) -> Option<Vec3> {
        let across = normal.dot(self.axis);
        if normal.cross(self.axis).length() * self.radius > tol || across == 0.0 {
            return None;
        }
        let c = self.origin + self.axis * ((d - normal.dot(self.origin)) / across);
        let on = |p: Vec3| {
            (p.distance(c) - self.radius).abs() <= tol && (normal.dot(p) - d).abs() <= tol
        };
        (on(a) && on(b)).then_some(c)
    }
}

/// An arc about a centre: from its start along `x_axis`, through `sweep` radians towards
/// `y_axis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    /// From the centre towards the start, unit.
    pub x_axis: Vec3,
    /// A quarter turn on from `x_axis`, unit.
    pub y_axis: Vec3,
    /// The radius.
    pub radius: f64,
    /// The angle swept, in (0, 2π].
    pub sweep: f64,
}

/// The arc from `a` to `b` about `centre` in the plane normal to the unit `axis`, the way round
/// whose middle lies on the side `out` points to from the chord `a`–`b`. `None` for a radius of
/// `tol` or less, or when neither way's middle lies more than `tol` to that side. An end less
/// than `tol` along the circle from the start makes a full turn.
#[must_use]
pub fn arc_toward(a: Vec3, b: Vec3, centre: Vec3, axis: Vec3, out: Vec3, tol: f64) -> Option<Arc> {
    let radius = a.distance(centre);
    if radius.is_nan() || radius <= tol {
        return None;
    }
    let x_axis = (a - centre) * (1.0 / radius);
    let chord = (a + b) * 0.5;
    let side = out * (1.0 / out.length());
    let way = |n: Vec3| {
        let y_axis = n.cross(x_axis);
        let mut sweep = (b - centre).dot(y_axis).atan2((b - centre).dot(x_axis));
        // An arc length, not an angle, against the length tolerance.
        if sweep * radius <= tol {
            sweep += TAU;
        }
        let half = sweep / 2.0;
        let mid = centre + (x_axis * half.cos() + y_axis * half.sin()) * radius;
        (
            Arc {
                x_axis,
                y_axis,
                radius,
                sweep,
            },
            (mid - chord).dot(side),
        )
    };
    let (one, s1) = way(axis);
    let (other, s2) = way(axis * -1.0);
    (s1.max(s2) > tol).then_some(if s1 >= s2 { one } else { other })
}

/// The arc from `a` to `b` about `centre` whose middle lies on `mid`'s side ([`arc_toward`]).
#[must_use]
pub fn circle_arc_through(
    a: Vec3,
    mid: Vec3,
    b: Vec3,
    centre: Vec3,
    axis: Vec3,
    tol: f64,
) -> Option<Arc> {
    arc_toward(a, b, centre, axis, mid - (a + b) * 0.5, tol)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cylinder() -> Cylinder {
        Cylinder {
            origin: Vec3::new(0.0, 0.0, 0.0),
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 2.0,
        }
    }

    #[test]
    fn a_circle_about_the_axis_at_its_height_is_on_the_cylinder() {
        let (ex, ey) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert!(cylinder().is_circle_at(Vec3::new(0.0, 0.0, 5.0), ex, ey, 2.0, 5.0, 1e-9));
    }

    #[test]
    fn a_tilted_or_off_axis_or_other_radius_circle_is_not() {
        let c = cylinder();
        let (ex, ey) = (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        let at = Vec3::new(0.0, 0.0, 5.0);
        let s = 1.0 / 2f64.sqrt();
        // Its normal x × y is (s, 0, s): tilted 45 degrees.
        let (tx, ty) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(-s, 0.0, s));
        assert!(!c.is_circle_at(at, tx, ty, 2.0, 5.0, 1e-9), "tilted");
        assert!(
            !c.is_circle_at(Vec3::new(0.5, 0.0, 5.0), ex, ey, 2.0, 5.0, 1e-9),
            "off the axis"
        );
        assert!(!c.is_circle_at(at, ex, ey, 3.0, 5.0, 1e-9), "other radius");
        assert!(!c.is_circle_at(at, ex, ey, 2.0, 6.0, 1e-9), "other height");
    }

    #[test]
    fn the_frame_at_a_point_starts_on_its_radial_side() {
        let c = cylinder();
        let p = Vec3::new(0.0, 2.0, 5.0);
        let (x, y) = c.frame_at(p);
        assert!(x.dot(c.axis).abs() < 1e-12, "x square to the axis");
        assert!((x.length() - 1.0).abs() < 1e-12, "x unit");
        assert!((y - c.axis.cross(x)).length() < 1e-12, "y is axis × x");
        assert!((p - c.centre_at(p)).dot(x) > 0.0, "p on the +x side");
        assert!(
            (x - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-12,
            "x points at p"
        );
    }

    #[test]
    fn points_at_one_height_share_a_circle() {
        let c = cylinder();
        assert!(c.same_circle(Vec3::new(1.0, 0.0, 3.0), Vec3::new(0.0, 2.0, 3.0), 1e-9));
        assert!(!c.same_circle(Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 3.5), 0.1));
    }

    #[test]
    fn a_short_arc_on_a_large_circle_is_not_a_full_turn() {
        let r = 1.0e5;
        let (a, b) = (
            Vec3::new(r, 0.0, 0.0),
            Vec3::new(r * 0.02f64.cos(), r * 0.02f64.sin(), 0.0),
        );
        let arc = arc_toward(
            a,
            b,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            0.024,
        )
        .unwrap();
        assert!((arc.sweep - 0.02).abs() < 1e-12, "{}", arc.sweep);
    }

    #[test]
    fn a_short_arc_through_its_midpoint_is_an_arc() {
        let at = |deg: f64| {
            let t = deg.to_radians();
            Vec3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0)
        };
        let arc = circle_arc_through(
            at(0.0),
            at(1.0),
            at(2.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            1e-3,
        )
        .unwrap();
        assert!(
            (arc.sweep - 2f64.to_radians()).abs() < 1e-9,
            "{}",
            arc.sweep
        );
    }

    #[test]
    fn a_half_circle_through_its_top_point_goes_the_top_way() {
        let z = Vec3::new(0.0, 0.0, 1.0);
        let arc = circle_arc_through(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(-2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 0.0),
            z,
            1e-9,
        )
        .unwrap();
        assert!(
            (arc.sweep - std::f64::consts::PI).abs() < 1e-12,
            "{}",
            arc.sweep
        );
        let half = arc.sweep / 2.0;
        let mid = (arc.x_axis * half.cos() + arc.y_axis * half.sin()) * arc.radius;
        assert!((mid - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-9, "{mid:?}");
    }

    #[test]
    fn the_arc_goes_round_on_the_side_asked() {
        let (a, b) = (Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -10.0, 0.0));
        let z = Vec3::new(0.0, 0.0, 1.0);
        for (out, mid_x) in [(1.0, 10.0), (-1.0, -10.0)] {
            let arc = arc_toward(
                a,
                b,
                Vec3::new(0.0, 0.0, 0.0),
                z,
                Vec3::new(out, 0.0, 0.0),
                1e-9,
            )
            .unwrap();
            let half = arc.sweep / 2.0;
            let mid = (arc.x_axis * half.cos() + arc.y_axis * half.sin()) * arc.radius;
            assert!((mid.x - mid_x).abs() < 1e-9, "{mid:?}");
        }
        assert!(arc_toward(
            a,
            a,
            Vec3::new(0.0, 0.0, 0.0),
            z,
            Vec3::new(0.0, 0.0, 0.0),
            1e-9
        )
        .is_none());
    }

    #[test]
    fn a_plane_square_to_the_axis_cuts_the_circle_its_points_lie_on() {
        let c = Cylinder {
            origin: Vec3::new(0.0, 0.0, 0.0),
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 10.0,
        };
        let z = Vec3::new(0.0, 0.0, 1.0);
        let (a, b) = (Vec3::new(10.0, 0.0, 4.0), Vec3::new(0.0, 10.0, 4.0));
        assert_eq!(
            c.plane_circle(z, 4.0, a, b, 1e-9),
            Some(Vec3::new(0.0, 0.0, 4.0))
        );
        assert_eq!(c.plane_circle(z, 5.0, a, b, 1e-9), None);
        assert_eq!(
            c.plane_circle(Vec3::new(1.0, 0.0, 0.0), 4.0, a, b, 1e-9),
            None
        );
        assert!(c.on_circle(a, 4.0, 1e-9) && !c.on_circle(a, 3.0, 1e-9));
        let origin = Vec3::new(0.0, 0.0, 0.0);
        assert!(
            c.is_ruling(origin, z * -3.0, 1e-9)
                && !c.is_ruling(origin, Vec3::new(1.0, 0.0, 1.0), 1e-9)
        );
    }

    #[test]
    fn a_ruling_tilted_just_inside_tol_is_one_and_just_outside_is_not() {
        let c = cylinder();
        let start = Vec3::new(2.0, 0.0, 0.0);
        let tol = 0.01;
        let end = |dx: f64| start + Vec3::new(dx, 0.0, 10.0);
        assert!(c.is_ruling(start, end(0.009), tol), "inside tol");
        assert!(!c.is_ruling(start, end(0.011), tol), "outside tol");
    }

    #[test]
    fn a_circle_tilted_just_inside_tol_is_one_and_just_outside_is_not() {
        let c = cylinder();
        let at = Vec3::new(0.0, 0.0, 5.0);
        let x = Vec3::new(1.0, 0.0, 0.0);
        let tilted = |s: f64| Vec3::new(0.0, (1.0 - s * s).sqrt(), s);
        let tol = 0.02;
        assert!(
            c.is_circle_at(at, x, tilted(0.009), 2.0, 5.0, tol),
            "inside tol"
        );
        assert!(
            !c.is_circle_at(at, x, tilted(0.011), 2.0, 5.0, tol),
            "outside tol"
        );
    }
}
