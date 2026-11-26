//! The geometry of a pierce: where a curve of another sketch crosses the plane of this one.
//!
//! A Pierce constraint (#3599) puts a point of this sketch on a curve of another sketch, at
//! the point where that curve crosses the plane of this sketch. Each sketch is flat on its
//! own plane, so the crossing is a question about two planes: the curve of the other sketch
//! is a polyline in that sketch's own coordinates, and the plane of this sketch cuts it.
//!
//! The cut is the one the Project + Coincident workaround already makes in the document
//! crate (`section_of_polyline` in `crates/doc/src/project.rs`, reached through
//! `LinkSource::SketchCurve` with `LinkKind::Intersect`): every segment of the world
//! polyline against the plane. That function lives in the document crate, which is built
//! above this one, so the cut is repeated here over the same `Plane` and `Vec3` types.
//!
//! Conventions:
//!
//! - The crossing comes back in the coordinates of *this* sketch: measured from this
//!   sketch's plane origin along this sketch's own axes.
//! - A curve that crosses the plane more than once gives every crossing, in the order the
//!   curve meets the plane; [`pierce_point`] gives the first of them.
//! - A curve that never reaches the plane gives no crossing, and so does a curve that lies
//!   in it: such a curve meets the plane everywhere and nowhere, so there is no single
//!   point to pierce (two sketches sharing a plane is a mistake in the relation, not a
//!   degenerate pierce).
//! - A point of the curve close enough to the plane counts as lying on it, so a curve that
//!   touches the plane at its end pierces there.

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;

/// A point counts as lying on the plane when it is this close to it, as a fraction of how
/// far the curve reaches from the plane's origin: a curve far from the origin is not judged
/// by a tolerance meant for a curve near it.
const REL_TOL: f64 = 1e-9;

/// The points where the curve of another sketch crosses the plane of this one.
///
/// `plane` is the plane of this sketch, `curve_plane` the plane of the sketch that owns the
/// curve, and `curve_points` the points of that curve in the owning sketch's own
/// coordinates. The crossings come back in the coordinates of this sketch, in the order the
/// curve meets the plane.
///
/// A curve that never reaches the plane gives nothing, and so does a curve that lies in it:
/// there is no single point where such a curve meets the plane. A curve that crosses the
/// plane more than once gives every crossing.
pub fn pierce_points(plane: &Plane, curve_plane: &Plane, curve_points: &[Vec2]) -> Vec<Vec2> {
    let world: Vec<Vec3> = curve_points.iter().map(|p| curve_plane.to_world(*p)).collect();
    pierce_points_world(plane, &world)
}

/// The point where the curve of another sketch crosses the plane of this one.
///
/// The first crossing, in the order the curve meets the plane; see [`pierce_points`] for
/// every crossing, and for the curves that have none.
pub fn pierce_point(plane: &Plane, curve_plane: &Plane, curve_points: &[Vec2]) -> Option<Vec2> {
    pierce_points(plane, curve_plane, curve_points).into_iter().next()
}

/// The points where a curve already in world coordinates crosses the plane of this sketch.
///
/// The crossings come back in the coordinates of this sketch, in the order the curve meets
/// the plane.
pub fn pierce_points_world(plane: &Plane, curve: &[Vec3]) -> Vec<Vec2> {
    let tol = tolerance(plane, curve);
    let mut out: Vec<Vec2> = Vec::new();
    for w in curve.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let (ha, hb) = (plane.height(*a), plane.height(*b));
        let (on_a, on_b) = (ha.abs() <= tol, hb.abs() <= tol);
        // A segment lying in the plane meets it all along: no single point to give.
        let p = if on_a && on_b {
            continue;
        } else if on_a {
            *a
        } else if on_b {
            *b
        } else if (ha > 0.0) == (hb > 0.0) {
            // Both ends on the same side: the segment never meets the plane.
            continue;
        } else {
            a.lerp(*b, ha / (ha - hb))
        };
        let local = plane.to_local(p);
        // A vertex on the plane is the crossing of both of its segments: give it once.
        if !out.iter().any(|q| q.dist(local) <= tol) {
            out.push(local);
        }
    }
    out
}

/// The point where a curve already in world coordinates crosses the plane of this sketch.
///
/// The first crossing, in the order the curve meets the plane; see [`pierce_points_world`]
/// for every crossing, and for the curves that have none.
pub fn pierce_point_world(plane: &Plane, curve: &[Vec3]) -> Option<Vec2> {
    pierce_points_world(plane, curve).into_iter().next()
}

/// How close to the plane a point counts as lying on it.
fn tolerance(plane: &Plane, curve: &[Vec3]) -> f64 {
    let reach = curve.iter().fold(0.0_f64, |m, p| m.max((*p - plane.origin).len()));
    reach.max(1.0) * REL_TOL
}

#[cfg(test)]
mod tests {
    use super::{pierce_point, pierce_point_world, pierce_points, pierce_points_world};
    use solidcore_record::{Plane, Vec2, Vec3};
    use solidcraft_geom::prelude::*;

    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    fn near(a: Vec2, b: Vec2) -> bool {
        a.dist(b) < 1e-9
    }

    /// A line of a sketch on the XZ plane, given in that sketch's own coordinates.
    fn xz_line(a: (f64, f64), b: (f64, f64)) -> Vec<Vec2> {
        vec![v(a.0, a.1), v(b.0, b.1)]
    }

    #[test]
    fn a_line_of_another_sketch_pierces_this_plane() {
        // A sketch on the XZ plane, a line at x = 5 running from (5, -5) to (5, 5): it
        // crosses the XY plane (z = 0) at the world point (5, 0, 0), which is (5, 0) here.
        let p = pierce_point(&Plane::XY, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, 5.0)));
        assert!(p.is_some_and(|q| near(q, v(5.0, 0.0))), "{p:?}");
    }

    #[test]
    fn a_curve_that_already_touches_the_plane_pierces_there() {
        // The same line, but ending on the plane: the crossing is that end point.
        let p = pierce_point(&Plane::XY, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, 0.0)));
        assert!(p.is_some_and(|q| near(q, v(5.0, 0.0))), "{p:?}");
    }

    #[test]
    fn a_curve_touching_the_plane_at_a_vertex_pierces_once() {
        // The vertex on the plane is the crossing of both of its segments, and it is one
        // point, not two.
        let pts = pierce_points(&Plane::XY, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, 5.0)));
        assert_eq!(pts.len(), 1, "{pts:?}");
        assert!(near(pts[0], v(5.0, 0.0)), "{pts:?}");
        let p = pierce_point(&Plane::XY, &Plane::XZ, &[v(5.0, -5.0), v(5.0, 0.0), v(5.0, 5.0)]);
        assert!(p.is_some_and(|q| near(q, v(5.0, 0.0))), "{p:?}");
    }

    #[test]
    fn a_curve_that_never_reaches_the_plane_does_not_pierce() {
        // The same line, but on the other side of the plane: it never crosses it.
        assert!(pierce_point(&Plane::XY, &Plane::XZ, &xz_line((5.0, 1.0), (5.0, 5.0))).is_none());
        // A sketch on a parallel plane, ten units above: it never reaches this one.
        let above = Plane { origin: Vec3::new(0.0, 0.0, 10.0), x: Vec3::X, y: Vec3::Y };
        assert!(pierce_point(&Plane::XY, &above, &xz_line((5.0, -5.0), (5.0, 5.0))).is_none());
    }

    #[test]
    fn a_curve_in_the_same_plane_has_no_single_crossing() {
        // The curve lies in the plane of this sketch: it meets it everywhere and nowhere,
        // so there is no point to pierce.
        assert!(pierce_point(&Plane::XY, &Plane::XY, &[v(0.0, 0.0), v(5.0, 5.0)]).is_none());
        assert!(pierce_points(&Plane::XY, &Plane::XY, &[v(0.0, 0.0), v(5.0, 5.0)]).is_empty());
    }

    #[test]
    fn a_curve_that_crosses_twice_gives_both_crossings() {
        // A U on the XZ plane, dipping through the XY plane and coming back: two
        // crossings, the first one is the pierce.
        let u = [v(0.0, 1.0), v(0.0, -1.0), v(2.0, -1.0), v(2.0, 1.0)];
        let pts = pierce_points(&Plane::XY, &Plane::XZ, &u);
        assert_eq!(pts.len(), 2, "{pts:?}");
        assert!(near(pts[0], v(0.0, 0.0)), "{pts:?}");
        assert!(near(pts[1], v(2.0, 0.0)), "{pts:?}");
        let p = pierce_point(&Plane::XY, &Plane::XZ, &u);
        assert!(p.is_some_and(|q| near(q, v(0.0, 0.0))), "{p:?}");
    }

    #[test]
    fn a_point_within_tolerance_of_the_plane_counts_as_on_it() {
        // An end point a hundredth of a nanometre above the plane is on it: the curve
        // pierces there.
        let p = pierce_point(&Plane::XY, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, -1e-10)));
        assert!(p.is_some_and(|q| near(q, v(5.0, 0.0))), "{p:?}");
        // A thousandth of a unit off the plane is not on it: the curve never reaches it.
        assert!(pierce_point(&Plane::XY, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, -1e-3))).is_none());
    }

    #[test]
    fn the_crossing_is_in_this_sketches_coordinates() {
        // The same world crossing, measured from another plane origin: (5, 0, 0) is
        // (3, -3) in a plane whose origin is (2, 3, 0).
        let shifted = Plane { origin: Vec3::new(2.0, 3.0, 0.0), x: Vec3::X, y: Vec3::Y };
        let p = pierce_point(&shifted, &Plane::XZ, &xz_line((5.0, -5.0), (5.0, 5.0)));
        assert!(p.is_some_and(|q| near(q, v(3.0, -3.0))), "{p:?}");
    }

    #[test]
    fn a_curve_with_no_length_does_not_pierce() {
        assert!(pierce_point(&Plane::XY, &Plane::XZ, &[]).is_none());
        assert!(pierce_point(&Plane::XY, &Plane::XZ, &[v(5.0, 0.0)]).is_none());
    }

    #[test]
    fn a_world_curve_pierces_the_plane() {
        // The same geometry, given straight in world coordinates.
        let world = [Vec3::new(5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, -5.0)];
        let p = pierce_point_world(&Plane::XY, &world);
        assert!(p.is_some_and(|q| near(q, v(5.0, 0.0))), "{p:?}");
        assert_eq!(pierce_points_world(&Plane::XY, &world).len(), 1);
    }
}
