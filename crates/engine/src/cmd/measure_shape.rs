//! What a picked item is, exactly (#1392): circles, arcs and cylinders (round items with an
//! axis), planes, straight lines and points, from the B-rep's exact curves and surfaces or the
//! sketch's own geometry. Round items report their diameter and radius, and a pair with a
//! round item measures from its centre or axis, with the closest and farthest distances too.

use serde_json::{Value, json};
use solidcraft_geom::{UnitVec3, Vec3};

use std::f64::consts::{PI, TAU};

/// The exact shape of a picked item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    /// A circle, an arc or a cylinder. `center` lies on the axis: a circle's or arc's centre,
    /// or for a cylinder the point of the axis level with the middle of the face. `lo..hi` is
    /// the extent along `axis` from `center` (0..0 for a planar circle). `angle` is the span
    /// around the axis (2π all the way round).
    Round {
        center: Vec3,
        axis: UnitVec3,
        radius: f64,
        angle: f64,
        planar: bool,
        lo: f64,
        hi: f64,
    },
    Plane {
        point: Vec3,
        normal: UnitVec3,
    },
    Line {
        a: Vec3,
        b: Vec3,
    },
    Point(Vec3),
    Other,
}

/// Within this of a whole turn counts as all the way round (a tessellated face's coverage).
const FULL_TURN: f64 = TAU - 1e-3;

impl Shape {
    pub(crate) fn full(&self) -> bool {
        matches!(self, Shape::Round { angle, .. } if *angle >= FULL_TURN)
    }
}

/// Diameter, radius and length (circumference or arc length, with the arc's angle) of a round
/// item, added to its description. Ø first, then R, then the length.
pub(crate) fn add_round(v: &mut Value, shape: &Shape) {
    let Shape::Round { center, axis, radius, angle, planar, .. } = *shape else { return };
    v["diameter"] = json!(2.0 * radius);
    v["radius"] = json!(radius);
    if angle >= FULL_TURN {
        v["circumference"] = json!(TAU * radius);
    } else {
        v["angle_deg"] = json!(angle.to_degrees());
        v["arc_length"] = json!(radius * angle);
    }
    v["center"] = json!(center);
    v["axis"] = json!(axis);
    v["round"] = json!(if !planar {
        "cylinder"
    } else if angle >= FULL_TURN {
        "circle"
    } else {
        "arc"
    });
}

/// The radius of a toroidal or spherical face, added to its description: a fillet round a
/// circular edge is a torus whose tube radius is the fillet's (`major_radius` is the ring
/// through the middle of the rolling ball), a ball or a spherical corner patch a sphere. These
/// faces have no axis to measure from, so they stay out of [`Shape`].
pub(crate) fn add_curved(v: &mut Value, surface: Option<solidcraft_kernel::FaceSurface>) {
    use solidcraft_kernel::FaceSurface;
    let (radius, kind) = match surface {
        Some(FaceSurface::Torus { minor, major, .. }) if minor.is_finite() && minor > 0.0 => {
            if major.is_finite() {
                v["major_radius"] = json!(major.abs());
            }
            (minor, "torus")
        }
        Some(FaceSurface::Sphere { radius, .. }) if radius.is_finite() && radius > 0.0 => (radius, "sphere"),
        _ => return,
    };
    v["diameter"] = json!(2.0 * radius);
    v["radius"] = json!(radius);
    v["surface"] = json!(kind);
}

/// Cylinder `axis` through `origin` of radius `radius` over the points of its face: the
/// round shape, centred level with the middle of the face, with its extent and coverage.
pub(crate) fn cylinder(origin: Vec3, axis: UnitVec3, radius: f64, pts: &[Vec3]) -> Option<Shape> {
    if pts.is_empty() || !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let ts: Vec<f64> = pts.iter().map(|p| (*p - origin).dot(axis)).collect();
    let (tmin, tmax) = ts.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(*t), b.max(*t)));
    let mid = (tmin + tmax) * 0.5;
    let center = origin + axis * mid;
    Some(Shape::Round { center, axis, radius, angle: coverage(center, axis, pts), planar: false, lo: tmin - mid, hi: tmax - mid })
}

/// The angle around the axis the points cover: a whole turn less the widest gap between them.
pub(crate) fn coverage(center: Vec3, axis: UnitVec3, pts: &[Vec3]) -> f64 {
    let x = axis.any_perp();
    let y = axis.cross(x);
    let mut angles: Vec<f64> = pts
        .iter()
        .filter_map(|p| {
            let d = *p - center;
            let r = d - axis * d.dot(axis);
            (r.len() > 1e-12).then(|| r.dot(y).atan2(r.dot(x)))
        })
        .collect();
    if angles.len() < 2 {
        return 0.0;
    }
    angles.sort_by(f64::total_cmp);
    let wrap = angles.first().copied().unwrap_or(0.0) + TAU - angles.last().copied().unwrap_or(0.0);
    let widest = angles.windows(2).map(|w| w[1] - w[0]).fold(wrap, f64::max);
    // Tessellation leaves gaps of a few degrees on a whole cylinder: those still count.
    if widest < 0.35 { TAU } else { TAU - widest }
}

/// The unit direction a point leaves the axis at, towards the middle of the face's points.
fn bulge(center: Vec3, axis: UnitVec3, pts: &[Vec3]) -> Option<Vec3> {
    let sum = pts.iter().fold(Vec3::ZERO, |acc, p| {
        let d = *p - center;
        acc + (d - axis * d.dot(axis)).normalized().unwrap_or(Vec3::ZERO)
    });
    sum.normalized()
}

/// Closest points of two infinite lines (a point and a unit direction each).
fn line_line(p1: Vec3, d1: UnitVec3, p2: Vec3, d2: UnitVec3) -> (Vec3, Vec3) {
    let w = p1 - p2;
    let b = d1.dot(d2);
    let denom = 1.0 - b * b;
    if denom < 1e-12 {
        return (p1, p2 + d2 * w.dot(d2));
    }
    let (d, e) = (d1.dot(w), d2.dot(w));
    let s = (b * e - d) / denom;
    let t = (e - b * d) / denom;
    (p1 + d1 * s, p2 + d2 * t)
}

fn parallel(a: impl Into<Vec3>, b: impl Into<Vec3>) -> bool {
    a.into().cross(b).len() < 1e-6
}

/// A measurement from a round item's centre or axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Centre {
    pub distance: f64,
    pub from: Vec3,
    pub to: Vec3,
    /// Closest and farthest distances worked out exactly (`None`: measure them on the mesh).
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Distance between two non-negative in-plane circles' rims, closest and farthest, with
/// centres `d` apart.
fn rims(d: f64, ra: f64, rb: f64) -> (f64, f64) {
    let near = if d >= ra + rb {
        d - ra - rb
    } else if d <= (ra - rb).abs() {
        (ra - rb).abs() - d
    } else {
        0.0
    };
    (near, d + ra + rb)
}

/// Centre-to-centre (axis to axis, axis to plane, line or point) between two items, when one
/// of them is round. `None` for pairs without a round item and where an axis meets the other
/// item at an angle that gives no single distance (a cylinder crossing a plane).
pub(crate) fn centre(a: &Shape, b: &Shape) -> Option<Centre> {
    match (a, b) {
        (Shape::Round { .. }, _) => from_round(a, b),
        (_, Shape::Round { .. }) => from_round(b, a).map(|c| Centre { from: c.to, to: c.from, ..c }),
        _ => None,
    }
}

fn from_round(a: &Shape, b: &Shape) -> Option<Centre> {
    let Shape::Round { center: ca, axis: na, radius: ra, planar: pa, lo: loa, hi: hia, .. } = *a else { return None };
    let full_a = a.full();
    let c = match *b {
        Shape::Round { center: cb, axis: nb, radius: rb, planar: pb, lo: lob, hi: hib, .. } => {
            // A picked circle/arc has a fixed centre; only a cylindrical face has an
            // infinite axis. Do not project a rim's centre onto an imaginary extension.
            let (from, to) = match (pa, pb) {
                (true, true) => (ca, cb),
                (true, false) => (ca, cb + nb * (ca - cb).dot(nb)),
                (false, true) => (ca + na * (cb - ca).dot(na), cb),
                (false, false) => line_line(ca, na, cb, nb),
            };
            let (mut min, mut max) = (None, None);
            if parallel(na, nb) {
                let w = cb - ca;
                let along = w.dot(na);
                let d = (w - na * along).len();
                if full_a && b.full() {
                    // B's extent along A's axis.
                    let s = if na.dot(nb) >= 0.0 { 1.0 } else { -1.0 };
                    let (b0, b1) = (along + lob * s, along + hib * s);
                    let (b0, b1) = (b0.min(b1), b0.max(b1));
                    let gap = (loa.max(b0) - hia.min(b1)).max(0.0);
                    let reach = (hia - b0).abs().max((b1 - loa).abs());
                    let (near, far) = rims(d, ra, rb);
                    min = Some(near.hypot(gap));
                    max = Some(far.hypot(reach));
                }
            }
            Centre { distance: from.dist(to), from, to, min, max }
        }
        Shape::Plane { point, normal } => {
            let n = normal.normalized()?;
            let h = (ca - point).dot(n);
            if na.dot(n).abs() < 1e-6 {
                // Axis along the plane: axis to plane, and the rim's nearest and farthest.
                let d = h.abs();
                let (min, max) = if full_a { (Some((d - ra).max(0.0)), Some(d + ra)) } else { (None, None) };
                Centre { distance: d, from: ca, to: ca - n * h, min: if d >= ra { min } else { None }, max }
            } else if pa {
                // A circle's centre to the plane.
                Centre { distance: h.abs(), from: ca, to: ca - n * h, min: None, max: None }
            } else {
                return None;
            }
        }
        Shape::Line { a: la, b: lb } => {
            let dir = (lb - la).unit()?;
            let across = dir.dot(na).abs() < 1e-6;
            if pa {
                // A circle/arc has a fixed centre. Project that point onto the picked
                // finite edge, retaining the height difference and clamping to its ends.
                let to = la + dir * (ca - la).dot(dir).clamp(0.0, (lb - la).len());
                let offset = to - ca;
                let height = offset.dot(na);
                let radial = (offset - na * height).len();
                let min = (full_a && across && radial >= ra).then(|| (radial - ra).hypot(height));
                Centre { distance: ca.dist(to), from: ca, to, min, max: None }
            } else {
                let (p, q) = line_line(ca, na, la, dir);
                let d = p.dist(q);
                let min = (full_a && across && d >= ra).then_some(d - ra);
                Centre { distance: d, from: p, to: q, min, max: None }
            }
        }
        Shape::Point(p) => {
            if pa {
                let d = p.dist(ca);
                let in_plane = (p - ca).dot(na).abs() < 1e-6 * (1.0 + d);
                let (min, max) = if full_a && in_plane { (Some((d - ra).abs()), Some(d + ra)) } else { (None, None) };
                Centre { distance: d, from: ca, to: p, min, max }
            } else {
                let w = p - ca;
                let foot = ca + na * w.dot(na);
                Centre { distance: p.dist(foot), from: foot, to: p, min: None, max: None }
            }
        }
        Shape::Other => return None,
    };
    (c.distance.is_finite() && c.from.is_finite() && c.to.is_finite()).then_some(c)
}

/// A slot's sizes: width (the ends' diameter), the distance between the ends' centres and the
/// overall length, with the two centres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Slot {
    pub width: f64,
    pub centres: (Vec3, Vec3),
}

impl Slot {
    pub(crate) fn json(&self) -> Value {
        let c2c = self.centres.0.dist(self.centres.1);
        json!({"width": self.width, "center_distance": c2c, "overall_length": c2c + self.width, "centers": [self.centres.0, self.centres.1]})
    }
}

/// A slot's outline in its plane: two half-circle ends of one radius joined by two straight
/// sides. `arcs` are (centre, radius, swept angle) with pieces of one end already merged,
/// `lines` the straight pieces (2D, in the sketch plane).
pub(crate) fn slot_outline(
    arcs: &[(solidcraft_geom::Vec2, f64, f64)],
    lines: &[(solidcraft_geom::Vec2, solidcraft_geom::Vec2)],
) -> Option<(solidcraft_geom::Vec2, solidcraft_geom::Vec2, f64)> {
    let [(c1, r1, s1), (c2, r2, s2)] = arcs else { return None };
    let tol = 1e-6 * (1.0 + r1.abs() + c1.dist(*c2));
    if (r1 - r2).abs() > tol || (s1.abs() - PI).abs() > 1e-4 || (s2.abs() - PI).abs() > 1e-4 || c1.dist(*c2) <= tol || lines.is_empty() {
        return None;
    }
    let along = (*c2 - *c1).normalized()?;
    // Every straight piece runs along the slot, `r` to one side of the centre line.
    let side = |p: solidcraft_geom::Vec2| (p - *c1).cross(along);
    let ok = lines.iter().all(|(a, b)| {
        let d = *b - *a;
        d.cross(along).abs() <= tol * (1.0 + d.len()) && (side(*a).abs() - r1).abs() <= tol && (side(*b).abs() - r1).abs() <= tol
    });
    let both = lines.iter().any(|(a, _)| side(*a) > 0.0) && lines.iter().any(|(a, _)| side(*a) < 0.0);
    (ok && both).then_some((*c1, *c2, *r1))
}

/// A slot cut or boss on a solid, found from one of its half-cylinder ends: another half
/// cylinder of the same radius on a parallel axis, bulging the other way, and two planes along
/// the slot `r` either side of its centre line. `faces` gives each face's surface and points.
pub(crate) fn slot_from_faces(picked: &Shape, picked_pts: &[Vec3], faces: &[(solidcraft_kernel::FaceSurface, Vec<Vec3>)]) -> Option<Slot> {
    use solidcraft_kernel::FaceSurface;
    let Shape::Round { center, axis, radius, angle, planar: false, .. } = *picked else { return None };
    if (angle - PI).abs() > 0.3 {
        return None;
    }
    let tol = 1e-6 * (1.0 + radius) * 100.0;
    let out_a = bulge(center, axis, picked_pts)?;
    for (s, pts) in faces {
        let FaceSurface::Cylinder { origin, axis: ax, radius: r } = *s else { continue };
        if (r - radius).abs() > tol || !parallel(ax, axis) {
            continue;
        }
        let Some(Shape::Round { center: cb, angle: ab, .. }) = cylinder(origin, ax, r, pts) else { continue };
        let w = cb - center;
        let off = w - axis * w.dot(axis);
        let Some(along) = off.normalized() else { continue };
        if (ab - PI).abs() > 0.3 || off.len() <= tol {
            continue;
        }
        let Some(out_b) = bulge(cb, axis, pts) else { continue };
        // Each end bulges away from the other.
        if out_a.dot(along) > -0.5 || out_b.dot(along) < 0.5 {
            continue;
        }
        let across = axis.cross(along);
        let sides = faces.iter().filter(|(fs, _)| match *fs {
            FaceSurface::Plane { normal, point } => {
                parallel(normal, across) && ((point - center).dot(across).abs() - radius).abs() <= tol.max(radius * 1e-4)
            }
            _ => false,
        });
        let (mut pos, mut neg) = (false, false);
        for (fs, _) in sides {
            if let FaceSurface::Plane { point, .. } = fs {
                if (*point - center).dot(across) > 0.0 { pos = true } else { neg = true }
            }
        }
        if pos && neg {
            // Both centres level with the picked end's middle.
            return Some(Slot { width: 2.0 * radius, centres: (center, center + off) });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec2;

    fn circle(c: Vec3, r: f64) -> Shape {
        Shape::Round { center: c, axis: UnitVec3::Z, radius: r, angle: TAU, planar: true, lo: 0.0, hi: 0.0 }
    }

    #[test]
    fn two_circles_measure_centre_to_centre_with_rims() {
        let c = centre(&circle(Vec3::ZERO, 3.0), &circle(Vec3::new(30.0, 40.0, 0.0), 2.0)).unwrap();
        assert!((c.distance - 50.0).abs() < 1e-12);
        assert!((c.min.unwrap() - 45.0).abs() < 1e-12);
        assert!((c.max.unwrap() - 55.0).abs() < 1e-12);
    }

    #[test]
    fn circles_at_different_heights_keep_both_actual_centres() {
        let (a, b) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(34.0, 19.0, 32.0));
        let c = centre(&circle(a, 4.0), &circle(b, 4.0)).unwrap();
        assert_eq!((c.from, c.to), (a, b));
        assert!((c.distance - 2001_f64.sqrt()).abs() < 1e-12);
        assert!((c.min.unwrap() - (1517_f64.sqrt() - 8.0).hypot(22.0)).abs() < 1e-12);
        assert!((c.max.unwrap() - (1517_f64.sqrt() + 8.0).hypot(22.0)).abs() < 1e-12);
        let swapped = centre(&circle(b, 4.0), &circle(a, 4.0)).unwrap();
        assert_eq!((swapped.from, swapped.to, swapped.distance), (b, a, c.distance));
    }

    #[test]
    fn arcs_on_different_planes_measure_between_their_centres() {
        let a = Shape::Round { center: Vec3::ZERO, axis: UnitVec3::Z, radius: 2.0, angle: PI, planar: true, lo: 0.0, hi: 0.0 };
        let b = Shape::Round { center: Vec3::new(3.0, 4.0, 12.0), axis: UnitVec3::X, radius: 4.0, angle: PI / 2.0, planar: true, lo: 0.0, hi: 0.0 };
        let c = centre(&a, &b).unwrap();
        assert_eq!((c.from, c.to, c.distance), (Vec3::ZERO, Vec3::new(3.0, 4.0, 12.0), 13.0));
        assert!(c.min.is_none() && c.max.is_none());
    }

    #[test]
    fn a_circle_to_a_cylinder_uses_its_fixed_centre_and_the_cylinders_axis() {
        let a = circle(Vec3::new(0.0, 0.0, 10.0), 4.0);
        for axis in [UnitVec3::Z, UnitVec3::X] {
            let b = Shape::Round { center: Vec3::new(34.0, 19.0, 27.0), axis, radius: 4.0, angle: TAU, planar: false, lo: -5.0, hi: 5.0 };
            let c = centre(&a, &b).unwrap();
            let expected = if axis == UnitVec3::Z { Vec3::new(34.0, 19.0, 10.0) } else { Vec3::new(0.0, 19.0, 27.0) };
            assert_eq!((c.from, c.to), (Vec3::new(0.0, 0.0, 10.0), expected));
            assert!((c.distance - c.from.dist(expected)).abs() < 1e-12);
            let swapped = centre(&b, &a).unwrap();
            assert_eq!((swapped.from, swapped.to, swapped.distance), (c.to, c.from, c.distance));
        }
    }

    #[test]
    fn two_cylindrical_faces_still_measure_axis_pitch() {
        let a = Shape::Round { center: Vec3::new(0.0, 0.0, 5.0), axis: UnitVec3::Z, radius: 4.0, angle: TAU, planar: false, lo: -5.0, hi: 5.0 };
        let b = Shape::Round { center: Vec3::new(34.0, 19.0, 27.0), axis: UnitVec3::Z, radius: 4.0, angle: TAU, planar: false, lo: -5.0, hi: 5.0 };
        let c = centre(&a, &b).unwrap();
        assert_eq!((c.from, c.to), (Vec3::new(0.0, 0.0, 5.0), Vec3::new(34.0, 19.0, 5.0)));
        assert!((c.distance - 1517_f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn a_cylinder_to_a_side_plane_measures_from_its_axis() {
        let cyl = Shape::Round { center: Vec3::new(10.0, 15.0, 5.0), axis: UnitVec3::Z, radius: 2.5, angle: TAU, planar: false, lo: -5.0, hi: 5.0 };
        let plane = Shape::Plane { point: Vec3::ZERO, normal: -UnitVec3::X };
        let c = centre(&cyl, &plane).unwrap();
        assert!((c.distance - 10.0).abs() < 1e-12);
        assert!((c.min.unwrap() - 7.5).abs() < 1e-12 && (c.max.unwrap() - 12.5).abs() < 1e-12);
        // Swapped, the line runs the other way.
        let d = centre(&plane, &cyl).unwrap();
        assert_eq!((d.from, d.to), (c.to, c.from));
        // A cylinder crossing a plane has no single axis distance.
        assert!(centre(&cyl, &Shape::Plane { point: Vec3::ZERO, normal: UnitVec3::Z }).is_none());
    }

    #[test]
    fn an_axis_to_a_crossing_edge_is_the_distance_between_the_lines() {
        let cyl = Shape::Round { center: Vec3::new(10.0, 15.0, 5.0), axis: UnitVec3::Z, radius: 2.5, angle: TAU, planar: false, lo: -5.0, hi: 5.0 };
        let edge = Shape::Line { a: Vec3::new(0.0, 0.0, 10.0), b: Vec3::new(40.0, 0.0, 10.0) };
        let c = centre(&cyl, &edge).unwrap();
        assert!((c.distance - 15.0).abs() < 1e-12);
        assert!((c.min.unwrap() - 12.5).abs() < 1e-12);
        assert!(c.from.dist(Vec3::new(10.0, 15.0, 10.0)) < 1e-12 && c.to.dist(Vec3::new(10.0, 0.0, 10.0)) < 1e-12);
    }

    #[test]
    fn a_circle_or_arc_to_a_line_keeps_its_real_center() {
        let edge = Shape::Line { a: Vec3::new(0.0, 0.0, 6.0), b: Vec3::new(68.0, 0.0, 6.0) };
        for angle in [TAU, PI / 2.0] {
            let round = Shape::Round { center: Vec3::new(34.0, 25.0, 20.0), axis: UnitVec3::Z, radius: 12.0, angle, planar: true, lo: 0.0, hi: 0.0 };
            let c = centre(&round, &edge).unwrap();
            assert_eq!((c.from, c.to), (Vec3::new(34.0, 25.0, 20.0), Vec3::new(34.0, 0.0, 6.0)));
            assert!((c.distance - 25_f64.hypot(14.0)).abs() < 1e-12);
            if angle == TAU {
                assert!((c.min.unwrap() - 13_f64.hypot(14.0)).abs() < 1e-12);
            } else {
                assert!(c.min.is_none());
            }
            let swapped = centre(&edge, &round).unwrap();
            assert_eq!((swapped.from, swapped.to, swapped.distance), (c.to, c.from, c.distance));
        }
    }

    #[test]
    fn a_tilted_arc_to_a_sloped_line_keeps_all_components() {
        let edge = Shape::Line { a: Vec3::ZERO, b: Vec3::new(6.0, 0.0, 6.0) };
        for axis in [UnitVec3::X, UnitVec3::Y, UnitVec3::Z] {
            let arc = Shape::Round { center: Vec3::new(3.0, 4.0, 6.0), axis, radius: 2.0, angle: PI, planar: true, lo: 0.0, hi: 0.0 };
            let c = centre(&arc, &edge).unwrap();
            assert!(c.from.dist(Vec3::new(3.0, 4.0, 6.0)) < 1e-12);
            assert!(c.to.dist(Vec3::new(4.5, 0.0, 4.5)) < 1e-12);
            assert!((c.distance - 20.5_f64.sqrt()).abs() < 1e-12);
        }
    }

    #[test]
    fn a_circle_center_to_a_finite_edge_does_not_use_its_extension() {
        for (center, expected) in [(Vec3::new(20.0, 3.0, 4.0), Vec3::new(10.0, 0.0, 0.0)), (Vec3::new(-10.0, 3.0, 4.0), Vec3::ZERO)] {
            for (a, b) in [(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)), (Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO)] {
                let c = centre(&circle(center, 2.0), &Shape::Line { a, b }).unwrap();
                assert_eq!((c.from, c.to), (center, expected));
                assert!((c.distance - 125_f64.sqrt()).abs() < 1e-12);
            }
        }
        assert!(centre(&circle(Vec3::ZERO, 2.0), &Shape::Line { a: Vec3::ZERO, b: Vec3::ZERO }).is_none());
    }

    #[test]
    fn coverage_of_a_half_and_a_whole_ring() {
        let ring = |n: usize, span: f64| -> Vec<Vec3> {
            (0..=n).map(|i| Vec3::new((span * i as f64 / n as f64).cos(), (span * i as f64 / n as f64).sin(), 0.0)).collect()
        };
        assert!((coverage(Vec3::ZERO, UnitVec3::Z, &ring(32, PI)) - PI).abs() < 1e-9);
        assert_eq!(coverage(Vec3::ZERO, UnitVec3::Z, &ring(32, TAU)), TAU);
    }

    #[test]
    fn a_slot_outline_and_not_a_slot() {
        let arcs = [(Vec2::new(0.0, 0.0), 3.0, PI), (Vec2::new(20.0, 0.0), 3.0, PI)];
        let lines = [(Vec2::new(0.0, -3.0), Vec2::new(20.0, -3.0)), (Vec2::new(20.0, 3.0), Vec2::new(0.0, 3.0))];
        let (a, b, r) = slot_outline(&arcs, &lines).unwrap();
        assert_eq!((a.dist(b), r), (20.0, 3.0));
        let skew = [(Vec2::new(0.0, -3.0), Vec2::new(20.0, -2.0)), (Vec2::new(20.0, 3.0), Vec2::new(0.0, 3.0))];
        assert!(slot_outline(&arcs, &skew).is_none());
        assert!(slot_outline(&[(Vec2::new(0.0, 0.0), 3.0, PI), (Vec2::new(20.0, 0.0), 4.0, PI)], &lines).is_none());
    }
}
