//! The planes a partial wall's wrap carries on onto (#5358, Task 3): a plane face tangent to the
//! wall across one of its lines along the axis continues the wall as one developable strip. In
//! wall coordinates (arc length round the wall from the sketch's radial, height along the axis)
//! the plane past a seam at `x` is the strip from `x` on, laid flat: a point (s, z) on it is
//! `foot + dir · side · (s - x) + axis · z`, a rigid placement, so profile curves stay exact there.
use crate::offset::Surf;
use solidcore_record::Vec3;
use solidcore_record::brep::{Brep, Curve, Loop};
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::prelude::*;

use super::{angle, face_axial_bounds};

/// A wall about `axis` through `o` of radius `r`, seen from the sketch: angles from `radial`
/// towards `tangent`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Axes {
    pub o: Vec3,
    pub axis: Vec3,
    pub radial: Vec3,
    pub tangent: Vec3,
    pub r: f64,
}

impl Axes {
    /// The direction from the axis at angle `theta`, and the direction round the wall there.
    fn at(&self, theta: f64) -> (Vec3, Vec3) {
        let (s, c) = theta.sin_cos();
        (self.radial * c + self.tangent * s, self.tangent * c - self.radial * s)
    }
}

/// A plane face tangent to the wall along one of the wall's lines.
#[derive(Clone, Copy, Debug)]
pub(super) struct Flat {
    /// The plane face, and the record's edge it shares with the wall (the seam).
    pub face: usize,
    pub seam: usize,
    /// The seam's wall coordinate; `side` is +1 when the plane carries on past the wall's last
    /// angle, -1 before its first.
    pub x: f64,
    pub side: f64,
    /// How far the plane runs on from the seam, square to the axis.
    pub width: f64,
    /// The seam's point at height 0 (level with `o`), the direction along the plane away from
    /// the wall, and the plane's normal out of the material (the wall's radial at the seam).
    pub foot: Vec3,
    pub dir: Vec3,
    pub normal: Vec3,
}

/// The faces whose loops use record edge `edge`.
fn faces_of_edge(rec: &Brep, edge: usize) -> Vec<usize> {
    rec.faces
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            f.loops.iter().any(|l| match l {
                Loop::Edges { edges } => edges.iter().any(|u| u.edge as usize == edge),
                Loop::Vertex { .. } => false,
            })
        })
        .map(|(i, _)| i)
        .collect()
}

/// A straight edge's two ends, measured on its curve: a line, or a spline whose control points
/// all lie within `tol` of the line through its first and last (a chamfer's or a boolean's
/// straight edge; the curve lies in their hull).
fn line_ends(rec: &Brep, edge: usize, tol: f64) -> Option<(Vec3, Vec3)> {
    let e = rec.edges.get(edge)?;
    match &e.curve {
        Curve::Line { .. } => {}
        Curve::Spline { points, .. } => {
            let (a, b) = (Vec3::from(*points.first()?), Vec3::from(*points.last()?));
            let d = (b - a).normalized()?;
            if points.iter().any(|p| {
                let w = Vec3::from(*p) - a;
                (w - d * w.dot(d)).len() > tol
            }) {
                return None;
            }
        }
        _ => return None,
    }
    let c = CurveEval::new(e.parameters()).ok()?;
    let [t0, t1] = e.interval;
    Some((c.point(t0)?, c.point(t1)?))
}

/// The planes tangent to the outside wall face `face` (a rectangle in angle and height, angles
/// `span` from `wall.radial`, heights `heights` along the axis from `wall.o`) across its two lines
/// along the axis, each a rectangle of four lines (two along the axis, two square to it) between
/// the same heights, with its material on the wall's side. Any other neighbour is left out: the
/// wrap stops at its seam.
pub(super) fn tangent_flats(
    rec: &Brep,
    surfaces: &[Option<Surf>],
    face: usize,
    wall: &Axes,
    span: (f64, f64),
    heights: (f64, f64),
    tol: f64,
) -> Vec<Flat> {
    let mut out = Vec::new();
    let Some(f) = rec.faces.get(face) else { return out };
    let [Loop::Edges { edges }] = f.loops.as_slice() else { return out };
    if rec.faces.len() > 50_000 {
        return out;
    }
    let along = |p: Vec3, q: Vec3| (q - p).normalized().is_some_and(|d| d.dot(wall.axis).abs() > 1.0 - 1e-9);
    let height = |p: Vec3| (p - wall.o).dot(wall.axis);
    for u in edges {
        let seam = u.edge as usize;
        let Some((p, q)) = line_ends(rec, seam, tol) else { continue };
        if !along(p, q) {
            continue;
        }
        // Which of the wall's ends the line is at.
        let a = angle(p, wall.o, wall.axis, wall.radial, wall.tangent);
        let near =
            |end: f64| ((a - end + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs() * wall.r <= tol * 10.0;
        let (side, end) = if near(span.1) {
            (1.0, span.1)
        } else if near(span.0) {
            (-1.0, span.0)
        } else {
            continue;
        };
        let (normal, round) = wall.at(end);
        let dir = round * side;
        let others: Vec<usize> = faces_of_edge(rec, seam).into_iter().filter(|g| *g != face).collect();
        let [g] = others.as_slice() else { continue };
        // Tangent, its material on the wall's side.
        let Some(Some(Surf::Plane { n, .. })) = surfaces.get(*g) else { continue };
        if n.dot(normal) < 1.0 - 1e-9 {
            continue;
        }
        // A rectangle: one loop of four lines, two along the axis and two square to it.
        let Some(pf) = rec.faces.get(*g) else { continue };
        let [Loop::Edges { edges: sides }] = pf.loops.as_slice() else { continue };
        if sides.len() != 4 {
            continue;
        }
        let ends: Option<Vec<(Vec3, Vec3)>> = sides.iter().map(|s| line_ends(rec, s.edge as usize, tol)).collect();
        let Some(ends) = ends else { continue };
        let lengthwise = ends.iter().filter(|(a, b)| along(*a, *b)).count();
        let across = ends.iter().filter(|(a, b)| (*b - *a).normalized().is_some_and(|d| d.dot(wall.axis).abs() < 1e-9)).count();
        if lengthwise != 2 || across != 2 {
            continue;
        }
        let foot = wall.o + normal * wall.r;
        let reach: Vec<f64> = ends.iter().flat_map(|(a, b)| [*a, *b]).map(|v| (v - foot).dot(dir)).collect();
        let width = reach.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if reach.iter().any(|d| *d < -tol * 10.0) || !width.is_finite() || width <= tol * 10.0 {
            continue;
        }
        // Between the wall's heights.
        let same = |a: f64, b: f64| (a - b).abs() <= tol * 10.0;
        let (h0, h1) = (height(p).min(height(q)), height(p).max(height(q)));
        if !same(h0, heights.0) || !same(h1, heights.1) {
            continue;
        }
        match face_axial_bounds(rec, *g, wall.o, wall.axis) {
            Ok(Some((lo, hi))) if same(lo, heights.0) && same(hi, heights.1) => {}
            _ => continue,
        }
        out.push(Flat { face: *g, seam, x: end * wall.r, side, width, foot, dir, normal });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offset::surf_of;

    #[test]
    fn the_planes_tangent_to_a_wall_carry_its_wrap_on() {
        let body = super::super::tests::round_ended(15.);
        let rec = body.record();
        let surfaces: Vec<Option<Surf>> = (0..rec.faces.len()).map(|f| surf_of(rec, f)).collect();
        let wall = surfaces.iter().position(|s| matches!(s, Some(Surf::Cylinder { .. }))).unwrap();
        // Seen from a sketch tangent at x = -10, its x running towards -y.
        let axes = Axes { o: Vec3::ZERO, axis: Vec3::Z, radial: -Vec3::X, tangent: -Vec3::Y, r: 10. };
        let h = std::f64::consts::FRAC_PI_2;
        let flats = tangent_flats(rec, &surfaces, wall, &axes, (-h, h), (0., 20.), 1e-7);
        assert_eq!(flats.len(), 2, "{flats:?}");
        for f in &flats {
            assert!((f.x.abs() - 10. * h).abs() < 1e-9 && (f.x.signum() - f.side).abs() < 1e-12, "{f:?}");
            assert!((f.width - 15.).abs() < 1e-9 && (f.dir - Vec3::X).len() < 1e-9, "{f:?}");
            // Past the wall's last angle (towards -y) the plane is y = -10.
            let y = -10. * f.side;
            assert!((f.foot - Vec3::new(0., y, 0.)).len() < 1e-9 && (f.normal - Vec3::new(0., y / 10., 0.)).len() < 1e-9, "{f:?}");
            assert!(matches!(surfaces[f.face], Some(Surf::Plane { .. })));
        }
        // The far end's plane is not tangent, and a wall without its planes has none.
        let tube = crate::boolean(
            &crate::cylinder(Vec3::ZERO, Vec3::Z, 10., 20.).unwrap(),
            &crate::box_solid(Vec3::new(0., -20., -1.), Vec3::new(20., 20., 21.)).unwrap(),
            crate::BoolOp::Cut,
        )
        .unwrap()
        .unwrap();
        let rec = tube.record();
        let surfaces: Vec<Option<Surf>> = (0..rec.faces.len()).map(|f| surf_of(rec, f)).collect();
        let wall = surfaces.iter().position(|s| matches!(s, Some(Surf::Cylinder { .. }))).unwrap();
        assert!(tangent_flats(rec, &surfaces, wall, &axes, (-h, h), (0., 20.), 1e-7).is_empty());
    }
}
