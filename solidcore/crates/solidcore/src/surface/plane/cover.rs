//! How far a bounded plane reaches over a box: whether its section with a face in the box is
//! the infinite plane's (every corner's foot on the plane inside its parameter rectangle).
//!
//! Not upstream: SolidCraft's composition (`plane_covers`), moved into the kernel so the
//! modelling layer computes no geometry (docs/layers.md §1). A query with a certified value
//! (owner rule 13564): it returns the margin and its enclosure, and the caller compares it
//! with its tolerance. Unlike SolidCraft's, it runs on the plane's own placement and scales,
//! not on three evaluated points and a 2x2 solve.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::geom::extent3d::Extent3d;
use crate::geom::vector3d::{Point3, Vec3};
use crate::nurbs::math::gamma;
use crate::surface::plane::Plane;
use crate::surface::side::Bracket;

impl Plane {
    /// The least margin, in model units along the plane, by which the feet on the plane of
    /// the eight corners of `bx` lie inside the plane's parameter rectangle (its STEP domain):
    /// for each corner `q` and each side of the rectangle, `X.(q - O) - u0 su`,
    /// `u1 su - X.(q - O)`, and the same in `v`, the plane being `O + u su X + v sv Y` with
    /// unit, square `X`, `Y`. Negative where a foot lies outside. The plane reaches over the
    /// box within `tol` when the margin is at least `-tol`.
    ///
    /// The enclosure is the margin's rounding and the axes' defect:
    /// - each term is the difference of a three-term dot product of a rounded difference and
    ///   a rounded product, at most 7 rounded operations, so it is within `gamma_8` of the sum
    ///   of its terms' magnitudes (`|X|.(|q| + |O|) + |u0 su|`);
    /// - the stored axes are unit and square only to rounding: with `e` the largest of
    ///   `|X.X - 1|`, `|Y.Y - 1|`, `|X.Y|` (plus their own rounding), the foot's coordinates
    ///   `G^-1 (X.r, Y.r)` (`G` the axes' Gram matrix) differ from `(X.r, Y.r)` by at most
    ///   `2e / (1 - 2e)` of their largest, and a parameter's length along `X` from the model
    ///   length by `e` of it: `3e / (1 - 2e)` of the terms' largest magnitude is added. Axes
    ///   with `e >= 1/4` bound nothing (an infinite enclosure).
    #[must_use]
    pub fn box_margin(&self, bx: &Extent3d) -> Bracket {
        let (o, x, y) = (
            self.position().origin(),
            self.position().x_axis(),
            self.position().y_axis(),
        );
        let s = self.uv_scale();
        let d = self.step_uv_domain();
        let (lo, hi) = (d.min(), d.max());
        let (m, mm) = (bx.min(), bx.max());
        let abs = |v: Vec3| Vec3::new(v.x.abs(), v.y.abs(), v.z.abs());
        let defect = (x.dot(x) - 1.0)
            .abs()
            .max((y.dot(y) - 1.0).abs())
            .max(x.dot(y).abs())
            + gamma(4) * (abs(x).dot(abs(x)) + abs(y).dot(abs(y)));
        let skew = if defect < 0.25 {
            3.0 * defect / (1.0 - 2.0 * defect) * (1.0 + gamma(4))
        } else {
            f64::INFINITY
        };
        let (mut margin, mut bound) = (f64::INFINITY, 0.0_f64);
        for c in 0..8 {
            let q = Point3::new(
                if c & 1 == 0 { m.x } else { mm.x },
                if c & 2 == 0 { m.y } else { mm.y },
                if c & 4 == 0 { m.z } else { mm.z },
            );
            let r = q - o;
            let size = abs(q) + abs(o);
            for (axis, scale, a, b) in [(x, s.x, lo.x, hi.x), (y, s.y, lo.y, hi.y)] {
                let e = axis.dot(r);
                let mag = abs(axis).dot(size);
                for (term, end) in [(e - a * scale, a * scale), (b * scale - e, b * scale)] {
                    if term < margin {
                        margin = term;
                    }
                    bound = bound.max(gamma(8) * (mag + end.abs()) + skew * (mag + end.abs()));
                }
            }
        }
        Bracket::about(margin, bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::extent2d::Extent2d;
    use crate::geom::vector2d::Vec2;

    fn bx(lo: [f64; 3], hi: [f64; 3]) -> Extent3d {
        Extent3d::from_points(
            Point3::new(lo[0], lo[1], lo[2]),
            Point3::new(hi[0], hi[1], hi[2]),
        )
    }

    /// The plane z = 1 over x in [-2, 3], y in [-1, 4] (scales 2 and 0.5 on the parameters).
    fn plane() -> Plane {
        Plane::new(
            Point3::new(0.0, 0.0, 1.0),
            Vec3::X,
            Vec3::Y,
            Vec2::new(2.0, 0.5),
            &Extent2d::from_bounds(-1.0, -2.0, 1.5, 8.0),
        )
    }

    #[test]
    fn the_margin_is_the_nearest_rectangle_side() {
        // a box inside: nearest is x = 3 against the corner at 2.5
        let m = plane().box_margin(&bx([-1.0, 0.0, -5.0], [2.5, 3.0, 7.0]));
        assert!((m.mid() - 0.5).abs() <= m.upper - m.lower + 1e-15);
        assert!(m.lower <= 0.5 && 0.5 <= m.upper);
        assert!(m.upper - m.lower < 1e-13);
        // a box out over y = -1 by 0.25
        let m = plane().box_margin(&bx([-1.0, -1.25, 0.0], [2.0, 3.0, 1.0]));
        assert!(m.lower <= -0.25 && -0.25 <= m.upper);
    }

    #[test]
    fn a_tilted_plane_measures_along_itself() {
        // the plane through the origin with X = (1, 1, 0)/sqrt2, Y = z, over [0, 1]^2 (unit
        // scales): the foot of (a, b, h) is ((a + b) / sqrt2, h)
        let r2 = 2f64.sqrt();
        let p = Plane::new(
            Point3::ZERO,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Z,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
        );
        let m = p.box_margin(&bx([0.1, 0.1, 0.2], [0.3, 0.3, 0.9]));
        let want = (0.2 / r2).min(1.0 - 0.6 / r2).min(0.2).min(1.0 - 0.9);
        assert!(m.lower <= want && want <= m.upper, "{m:?} {want}");
        // the rounding and the stored axes' defect (a few ulps)
        assert!(m.upper - m.lower < 1e-13, "{m:?}");
        // lower the box's top: the u sides come nearest
        let m = p.box_margin(&bx([0.05, 0.05, 0.4], [0.3, 0.3, 0.6]));
        let want = 0.1 / r2;
        assert!(m.lower <= want && want <= m.upper, "{m:?} {want}");
    }

    #[test]
    fn far_coordinates_widen_the_bound_not_the_value() {
        let p = Plane::new(
            Point3::new(1e6, 1e6, 0.0),
            Vec3::X,
            Vec3::Y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        );
        let m = p.box_margin(&bx(
            [1e6 - 1.0, 1e6 - 1.0, 0.0],
            [1e6 + 1.0, 1e6 + 1.0, 0.0],
        ));
        assert!(m.lower <= 9.0 && 9.0 <= m.upper);
        assert!(
            m.upper - m.lower > 1e-12 && m.upper - m.lower < 1e-7,
            "{m:?}"
        );
    }

    /// Axes only nearly square (REVIEW2's finding 6 case): the kernel keeps Y = (1e-13, 1, 0),
    /// orthogonalising only past its zero tolerance. The foot of (0, 1e6, 0) is at
    /// u = -1e-13 1e6 = -1e-7 in the axes' own coordinates, so the u-low margin is -1e-7; the
    /// enclosure holds it (the axes' defect taken in), where one of the dot products' rounding
    /// alone (about 1e-8) would not.
    #[test]
    fn nearly_square_axes_widen_the_enclosure_to_the_true_foot() {
        let p = Plane::new(
            Point3::ZERO,
            Vec3::X,
            Vec3::new(1e-13, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(0.0, -1e7, 10.0, 1e7),
        );
        let y = p.position().y_axis();
        assert!(y.x != 0.0, "{y:?}");
        let m = p.box_margin(&bx([0.0, 1e6, 0.0], [0.0, 1e6, 0.0]));
        assert!(m.lower <= -1e-7 && -1e-7 <= m.upper, "{m:?}");
    }
}
