//! The knot lines of a drop surface across which its parameterisation is only
//! position-continuous, which a dropped UV curve turns a corner on (not upstream).
//!
//! Upstream: none. `SmSurface::DropCurve` (`source/SMLib/src/SmSurfaceDropLib.cpp`) does
//! not look at the surface's knots.
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`KnotLines`] | none     |
//!
//! **Why.** Across a knot of multiplicity at least the degree a B-spline surface is only
//! `C0`: `S_u` (or `S_v`) may jump there, as it does at the sections of a loft and where a
//! surface was extended past its end. The UV curve of a smooth 3D curve solves
//! `S_u u' + S_v v' = C'` on the tangent plane, so where `S_u` jumps `u'` jumps with it:
//! the UV curve has a corner on the line, although its image on the surface is smooth.
//! `DropCurve` builds the UV curve from cubic pieces that share their end tangents. A
//! piece with the line inside it, or with an end a rounding error past it, takes one end
//! tangent from each side of the line and has to turn the corner inside itself: a notch a
//! fraction of the tolerance deep, with a radius of curvature far below the curve's.
//!
//! **The lines.** For a B-spline surface, every interior knot of multiplicity at least
//! the degree in its direction that lies inside the drop domain by more than the
//! parameter zero. No other kind is given lines: an analytic surface is evaluated from
//! its definition, which has no such line.
//!
//! **On a line.** A parameter within the kernel's parameter zero ([`EFF_ZERO_PARAM`]) of
//! a knot, relative to the surface's knot range in that direction, is the knot
//! ([`KnotLines::settle`]): only at the knot itself do the surface's evaluators answer
//! from the side asked for, and a piece shorter than that has no second derivative left
//! in double precision. The point moves by at most that zero times `|S_u|` (or `|S_v|`),
//! which the drop's measured deviation includes.
//!
//! **Smooth lines only.** A point is put on a line only where the surface is smooth
//! across it there: its unit normals on the two sides agree within one degree, the angle
//! within which `SmSurface::CalculateContinuities` reads a knot line as `G1`. Across
//! such a line only the parameterisation changes, and the corner above is the whole of
//! it. Across a crease the drop itself is not continuous (the points of a wedge all drop
//! onto the crease), the UV curve runs along the line for a stretch, and nothing here is
//! derived for it: a drop point on a crease is left where it is, as upstream's.
//!
//! **Where a derivative jumps.** A point is also left where it is where the first
//! derivatives of the two sides agree (to [`JUMP`]): the surface is `C1` there (a
//! rational circle's double knots), the UV curve turns no corner, and the pieces share
//! their tangent as upstream's.
//!
//! Deviations: not upstream. No recursion.

use crate::error::{ignored, Result};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO_PARAM, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::knots::Side;
use crate::surface::surface::p1::evaluate_1st_derivatives;
use crate::surface::surface::p2::SurfaceKind;
use crate::surface::surface::Surface;

/// The cosine of one degree: the surface is smooth across a knot line where its normals
/// on the two sides agree within that angle (`SmSurface::CalculateContinuities`' limit
/// for a `G1` knot line).
const SMOOTH_COSINE: f64 = 0.999_847_695_156_391_3;

/// A first derivative jumps across a knot line where its two sides differ by more than
/// this fraction of the longer one ([`EFF_ZERO_SQRT`]). Below it the UV curve's corner is
/// under a millionth of a radian, which no piece can turn into a small radius, and the
/// two sides are one: a rational circle's double knots, across which the surface is `C1`.
const JUMP: f64 = EFF_ZERO_SQRT;

/// Whether the two sides `a` and `b` of a first derivative at a knot differ by more than
/// [`JUMP`] of the longer. The drop asks it of the surface's partials across a knot line
/// ([`KnotLines::settle`]) and of the dropped curve's derivative at a knot of its own (`p3`'s
/// span walk).
#[must_use]
pub(crate) fn derivative_jumps(a: Vec3, b: Vec3) -> bool {
    (a - b).length() > JUMP * a.length().max(b.length())
}

/// One knot line: the direction whose parameter is constant along it, and that knot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnotLine {
    /// [`SurfParamType::U`] for a line `u = knot`, [`SurfParamType::V`] for `v = knot`.
    pub direction: SurfParamType,
    /// The knot.
    pub knot: f64,
}

impl KnotLine {
    /// How far `uv` is past the line in its direction: negative before the knot, zero on
    /// it.
    #[must_use]
    pub fn offset(&self, uv: Point2) -> f64 {
        if self.direction == SurfParamType::U {
            uv.x - self.knot
        } else {
            uv.y - self.knot
        }
    }
}

/// The `C0` knot lines of a drop surface inside its drop domain (module docs), with the
/// parameter zero of each direction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KnotLines {
    /// The `u = knot` lines, ascending.
    u: Vec<f64>,
    /// The `v = knot` lines, ascending.
    v: Vec<f64>,
    /// The parameter zero in u and in v.
    zero: [f64; 2],
}

impl KnotLines {
    /// No lines: the drop of a surface without `C0` knot lines.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            u: Vec::new(),
            v: Vec::new(),
            zero: [0.0; 2],
        }
    }

    /// The lines of `surface` inside `uv_domain` (module docs).
    ///
    /// # Errors
    ///
    /// Those of the surface's knots (a B-spline surface without its NURB).
    pub fn of(surface: &Surface, uv_domain: &Extent2d) -> Result<Self> {
        let SurfaceKind::BSpline(b) = surface.kind() else {
            return Ok(Self::none());
        };
        let mut lines = Self::none();
        for (direction, inside) in [
            (SurfParamType::U, uv_domain.u_interval()),
            (SurfParamType::V, uv_domain.v_interval()),
        ] {
            let (knots, multiplicities) = b.knots(direction, None)?;
            let degree = b.degree(direction);
            let (Some(&first), Some(&last)) = (knots.first(), knots.last()) else {
                continue;
            };
            let zero = EFF_ZERO_PARAM * (last - first);
            let found = knots
                .iter()
                .zip(&multiplicities)
                .filter(|&(&knot, &multiplicity)| {
                    multiplicity >= degree
                        && knot > first
                        && knot < last
                        && knot > inside.min() + zero
                        && knot < inside.max() - zero
                })
                .map(|(&knot, _)| knot)
                .collect();
            if direction == SurfParamType::U {
                lines.u = found;
                lines.zero[0] = zero;
            } else {
                lines.v = found;
                lines.zero[1] = zero;
            }
        }
        Ok(lines)
    }

    /// `uv` put on each line within the parameter zero of it across which `surface` is
    /// smooth there and a first derivative jumps (module docs), and whether it is then on
    /// a `u = knot` line and on a `v = knot` line. Across a crease, where the two sides'
    /// derivatives agree, or where the surface cannot be evaluated, `uv` keeps its
    /// parameter and is on no line.
    ///
    /// # Errors
    ///
    /// [`crate::error::SolidError::Unported`] and cancellation from the normals.
    pub fn settle(&self, k: &Kernel, surface: &Surface, uv: Point2) -> Result<(Point2, [bool; 2])> {
        let near = self.on_lines(uv);
        let [mut on_u_line, mut on_v_line] = self.lines_through(near);
        if !(on_u_line || on_v_line) {
            return Ok((uv, [false; 2]));
        }
        // the unit normal and the first derivatives from the sides given
        let side = |u_side, v_side| -> Result<Option<(Vec3, Vec3, Vec3)>> {
            let normal = ignored(surface.evaluate_normal(k, near, u_side, v_side))?;
            let derivatives = ignored(evaluate_1st_derivatives(k, surface, near, u_side, v_side))?;
            Ok(normal
                .zip(derivatives)
                .map(|(normal, (_, du, dv))| (normal, du, dv)))
        };
        let ahead = side(Side::Left, Side::Left)?;
        // smooth across the line, and a first derivative that jumps there
        let turns_a_corner = |behind: Option<(Vec3, Vec3, Vec3)>| {
            ahead.zip(behind).is_some_and(|(a, b)| {
                a.0.dot(b.0) >= SMOOTH_COSINE
                    && (derivative_jumps(a.1, b.1) || derivative_jumps(a.2, b.2))
            })
        };
        on_u_line = on_u_line && turns_a_corner(side(Side::Right, Side::Left)?);
        on_v_line = on_v_line && turns_a_corner(side(Side::Left, Side::Right)?);
        let settled = Point2::new(
            if on_u_line { near.x } else { uv.x },
            if on_v_line { near.y } else { uv.y },
        );
        Ok((settled, [on_u_line, on_v_line]))
    }

    /// Whether `uv` is within the parameter zero of `line`.
    #[must_use]
    pub fn is_within_zero(&self, line: KnotLine, uv: Point2) -> bool {
        let zero = if line.direction == SurfParamType::U {
            self.zero[0]
        } else {
            self.zero[1]
        };
        line.offset(uv).abs() <= zero
    }

    /// `uv` with each parameter within the parameter zero of a line put on that line.
    #[must_use]
    fn on_lines(&self, uv: Point2) -> Point2 {
        let put = |x: f64, knots: &[f64], zero: f64| {
            knots
                .iter()
                .copied()
                .find(|&knot| (x - knot).abs() <= zero)
                .unwrap_or(x)
        };
        Point2::new(
            put(uv.x, &self.u, self.zero[0]),
            put(uv.y, &self.v, self.zero[1]),
        )
    }

    /// Whether `uv` is exactly on a `u = knot` line and on a `v = knot` line.
    fn lines_through(&self, uv: Point2) -> [bool; 2] {
        [self.u.contains(&uv.x), self.v.contains(&uv.y)]
    }

    /// The line with `a` and `b` strictly on its two sides that the straight step from
    /// `a` to `b` reaches first; `None` when no line separates them (a point on a line is
    /// on neither side of it).
    #[must_use]
    pub fn crossed(&self, a: Point2, b: Point2) -> Option<KnotLine> {
        let mut first: Option<(f64, KnotLine)> = None;
        for (direction, knots, from, to) in [
            (SurfParamType::U, &self.u, a.x, b.x),
            (SurfParamType::V, &self.v, a.y, b.y),
        ] {
            for &knot in knots {
                let separates = (from < knot && knot < to) || (to < knot && knot < from);
                if !separates {
                    continue;
                }
                let fraction = (knot - from) / (to - from);
                if first.is_none_or(|(least, _)| fraction < least) {
                    first = Some((fraction, KnotLine { direction, knot }));
                }
            }
        }
        first.map(|(_, line)| line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::vector2d::Vec2;
    use crate::math::det;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::math::{CPoint, NOW};
    use crate::surface::plane::Plane;

    /// A patch over `[0, 4] x [0, 2]`, cubic in u with the knot 1 three times (a `C0`
    /// line) and the knot 2 once (smooth), quadratic in v with the knot 1 twice (a `C0`
    /// line). With `jump`, `Su` doubles across `u = 1` and `Sv` across `v = 1`; without,
    /// both are the same on the two sides. Its net is the plane `z = 0` folded about the
    /// control points' row on `u = 1` (`x = 3`): each side rises `fold_degrees` from the
    /// plane, so the normals on the two sides of that line are twice that apart.
    fn patch(fold_degrees: f64, jump: bool) -> Surface {
        let rise = det::tan(fold_degrees.to_radians());
        let step = if jump { 2.0 } else { 1.0 };
        let net: Vec<Vec<CPoint>> = (0..8_i32)
            .map(|i| {
                let x = f64::from(i.min(3)) + step * f64::from((i - 3).max(0));
                (0..5_i32)
                    .map(|j| {
                        let y = f64::from(j.min(2)) + step * f64::from((j - 2).max(0));
                        CPoint::new(x, y, rise * (x - 3.0).abs(), NOW)
                    })
                    .collect()
            })
            .collect();
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 4.0, 4.0, 4.0, 4.0];
        let v = [0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0];
        Surface::from(BSplineSurface::from_data(7, 4, 3, 2, 11, 7, &net, &u, &v).unwrap())
    }

    const U_LINE: KnotLine = KnotLine {
        direction: SurfParamType::U,
        knot: 1.0,
    };
    const V_LINE: KnotLine = KnotLine {
        direction: SurfParamType::V,
        knot: 1.0,
    };

    /// The lines are the interior knots of multiplicity at least the degree, inside the
    /// drop domain; a parameter within the parameter zero of one (1.2e-8 of the knot
    /// range) is put on it, and one past that is not.
    #[test]
    fn the_lines_are_the_c0_knots_inside_the_domain() {
        let k = Kernel::default();
        let whole = Extent2d::from_bounds(0.0, 0.0, 4.0, 2.0);
        let flat = patch(0.0, true);
        let lines = KnotLines::of(&flat, &whole).unwrap();
        assert_eq!(lines.u, vec![1.0]);
        assert_eq!(lines.v, vec![1.0]);

        let near = Point2::new(1.0 + 4.0e-8, 1.0 - 2.0e-8);
        let (settled, on) = lines.settle(&k, &flat, near).unwrap();
        assert_eq!((settled.x, settled.y, on), (1.0, 1.0, [true, true]));
        let off = Point2::new(1.0 + 6.0e-8, 2.0 - 1.0e-9);
        assert_eq!(lines.settle(&k, &flat, off).unwrap(), (off, [false; 2]));
        let on_u = Point2::new(1.0, 0.3);
        assert_eq!(
            lines.settle(&k, &flat, on_u).unwrap(),
            (on_u, [true, false])
        );
        assert!(lines.is_within_zero(U_LINE, near));
        assert!(!lines.is_within_zero(U_LINE, off));

        // a domain that ends on the u line has only the v line; no other kind has lines
        let part = Extent2d::from_bounds(1.0, 0.0, 4.0, 2.0);
        let lines = KnotLines::of(&flat, &part).unwrap();
        assert_eq!((lines.u.len(), lines.v.len()), (0, 1));
        let plane = Surface::from(Plane::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec2::new(1.0, 1.0),
            &whole,
        ));
        assert_eq!(KnotLines::of(&plane, &whole).unwrap(), KnotLines::none());
    }

    /// A point is put on a line only where the surface is smooth across it and a first
    /// derivative jumps. The patch folded about `u = 1` by just under half a degree a
    /// side (its normals there just under one degree apart) is smooth, and a point near
    /// that line is put on it. Folded by 2 degrees a side it is creased: the point keeps
    /// its parameter and is on no line, also when it is exactly on the knot. The `v = 1`
    /// line of the same patch stays flat and keeps its points. Where the derivatives of
    /// the two sides agree the surface is `C1` across its knot lines, and a point is on
    /// none of them.
    #[test]
    fn a_point_is_put_on_a_line_only_where_the_surface_turns_the_uv_curve() {
        let k = Kernel::default();
        let whole = Extent2d::from_bounds(0.0, 0.0, 4.0, 2.0);
        let near = Point2::new(1.0 - 3.0e-8, 1.0 + 1.0e-8);
        let exact = Point2::new(1.0, 0.25);
        for (fold_degrees, smooth) in [(0.0, true), (0.499, true), (2.0, false)] {
            let surface = patch(fold_degrees, true);
            let lines = KnotLines::of(&surface, &whole).unwrap();
            let (settled, on) = lines.settle(&k, &surface, near).unwrap();
            let want_u = if smooth { 1.0 } else { near.x };
            assert_eq!(
                (settled.x, settled.y, on),
                (want_u, 1.0, [smooth, true]),
                "folded {fold_degrees}"
            );
            assert_eq!(
                lines.settle(&k, &surface, exact).unwrap(),
                (exact, [smooth, false]),
                "folded {fold_degrees}"
            );
        }
        let c1 = patch(0.0, false);
        let lines = KnotLines::of(&c1, &whole).unwrap();
        assert_eq!((lines.u.len(), lines.v.len()), (1, 1));
        assert_eq!(lines.settle(&k, &c1, near).unwrap(), (near, [false; 2]));
        assert_eq!(lines.settle(&k, &c1, exact).unwrap(), (exact, [false; 2]));
    }

    /// A step crosses the line its two ends are strictly on the two sides of, the first
    /// one it reaches; an end on a line is on neither side of it.
    #[test]
    fn a_step_crosses_the_first_line_between_its_ends() {
        let whole = Extent2d::from_bounds(0.0, 0.0, 4.0, 2.0);
        let lines = KnotLines::of(&patch(0.0, true), &whole).unwrap();
        let step = |a: [f64; 2], b: [f64; 2]| {
            lines.crossed(Point2::new(a[0], a[1]), Point2::new(b[0], b[1]))
        };
        assert_eq!(step([0.5, 0.2], [3.0, 0.4]), Some(U_LINE));
        assert_eq!(step([3.0, 0.4], [0.5, 0.2]), Some(U_LINE));
        // both lines between the ends: u = 1 at a fifth of the way, v = 1 at half
        assert_eq!(step([0.5, 0.5], [3.0, 1.5]), Some(U_LINE));
        assert_eq!(step([0.9, 0.2], [1.4, 1.8]), Some(U_LINE));
        assert_eq!(step([0.2, 0.9], [1.8, 1.4]), Some(V_LINE));
        // the smooth knot u = 2 is no line, and an end on a line does not cross it
        assert_eq!(step([1.5, 0.2], [3.0, 0.4]), None);
        assert_eq!(step([1.0, 0.2], [3.0, 0.4]), None);
        assert_eq!(step([0.5, 0.2], [1.0, 0.4]), None);
        assert_eq!(U_LINE.offset(Point2::new(0.5, 7.0)), -0.5);
        assert_eq!(V_LINE.offset(Point2::new(7.0, 1.25)), 0.25);
    }
}
