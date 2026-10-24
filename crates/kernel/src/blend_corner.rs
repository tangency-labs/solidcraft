//! Fillets with a radius per edge. SMLib rounds every picked edge at its own radius in one
//! blend, with the corners where the rounds meet: an end in the face beyond, two rounds trimming
//! each other, or a corner patch where three meet.

use solidcraft_geom::Vec3;

use crate::body::Body;
use crate::{KernelError, Result};

/// Fillet each edge nearest a point with its own radius.
pub fn fillet_radii(body: &Body, edges: &[(Vec3, f64)]) -> Result<Body> {
    body.require_brep("fillet")?;
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    if edges.iter().any(|(p, r)| !(p.is_finite() && r.is_finite() && *r > 1e-6 && *r < 1e6)) {
        return Err(KernelError::Invalid("fillet size must be positive".into()));
    }
    let (points, radii): (Vec<Vec3>, Vec<f64>) = edges.iter().copied().unzip();
    crate::blend_smlib::fillet_each(body, &points, &radii)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::blend::Faces;
    use crate::{box_solid, fillet, measure};
    use solidcraft_geom::brep::eval::CurveEval;

    /// The 20 mm box and the three edges at its corner (20, 0, 20): `a` along y, `b` along x,
    /// `c` up.
    fn setup() -> (Body, Vec3, Vec3, Vec3) {
        let b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        (b, Vec3::new(20.0, 10.0, 20.0), Vec3::new(10.0, 0.0, 20.0), Vec3::new(20.0, 0.0, 10.0))
    }

    fn vol(b: &Body) -> f64 {
        measure(b).unwrap().volume
    }

    /// The body minus the material outside each round (the strips between the faces and the
    /// cylinders), for rounds `ra`, `rb`, `rc` on edges a, b, c: the strips integrated exactly
    /// and their overlap at the corner numerically.
    fn strips(ra: f64, rb: f64, rc: f64) -> f64 {
        let len = |d: f64, r: f64| if d < r { r - (r * r - (r - d) * (r - d)).max(0.0).sqrt() } else { 0.0 };
        let (n, l) = (800, 5.0);
        let hh = l / n as f64;
        let (mut union, mut sum) = (0.0, 0.0);
        for i in 0..n {
            let dx = (i as f64 + 0.5) * hh;
            for j in 0..n {
                let dy = (j as f64 + 0.5) * hh;
                let tc = rc > 0.0 && dx < rc && dy < rc && (rc - dx).powi(2) + (rc - dy).powi(2) > rc * rc;
                let (a, b) = (len(dx, ra).min(l), len(dy, rb).min(l));
                union += if tc { l } else { a.max(b) };
                sum += a + b + if tc { l } else { 0.0 };
            }
        }
        let full = (1.0 - std::f64::consts::FRAC_PI_4) * (ra * ra + rb * rb + rc * rc) * 20.0;
        8000.0 - full + (sum - union) * hh * hh
    }

    /// Largest angle (degrees) between the two faces' normals along edges between curved faces.
    fn kink(b: &Body) -> f64 {
        let rec = Faces::of(b).unwrap();
        let mut worst: f64 = 0.0;
        for (i, e) in rec.rec.edges.iter().enumerate() {
            let faces: Vec<usize> = rec.faces_of(i).into_iter().map(|f| f as usize).collect();
            let [f, g] = faces[..] else { continue };
            if rec.plane_normal(f).is_some() || rec.plane_normal(g).is_some() {
                continue;
            }
            let c = CurveEval::new(e.parameters()).unwrap();
            let [t0, t1] = e.interval;
            for k in 1..10 {
                let p = c.point(t0 + (t1 - t0) * k as f64 / 10.0).unwrap();
                let (a, bb) = (rec.normal_at(f, p, 1e-3).unwrap(), rec.normal_at(g, p, 1e-3).unwrap());
                worst = worst.max(a.dot(bb).abs().min(1.0).acos().to_degrees());
            }
        }
        worst
    }

    /// The body's vertex points.
    fn vertices(b: &Body) -> Vec<Vec3> {
        crate::native::brep_record(b).unwrap().vertices.iter().map(|v| Vec3::new(v.point[0], v.point[1], v.point[2])).collect()
    }

    #[test]
    fn analytic_volumes_of_the_reference() {
        // One round and the sphere corner of three equal ones.
        let one = 8000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((strips(2.0, 0.0, 0.0) - one).abs() < 1e-6);
        // Rounds 2 and 3 on a and b: the overlap is ∫ (2 − √(4 − (z − 18)²))(3 − √(9 − (z − 17)²)) dz
        // over z from 18 to 20 (Simpson, 200000 steps).
        assert!((strips(2.0, 3.0, 0.0) - 7945.579002).abs() < 1e-3);
    }

    #[test]
    fn a_round_next_to_a_round_of_another_radius() {
        let (bx, a, b, _) = setup();
        let ra = fillet(&bx, &[a], 2.0).unwrap();
        // The round of b ends on the round of a: their cylinders trim each other.
        let r = fillet(&ra, &[b], 3.0).unwrap();
        assert_eq!(r.face_count(), 8);
        assert_eq!(r.unmeshed_faces(), 0);
        assert!((vol(&r) - strips(2.0, 3.0, 0.0)).abs() < 0.05, "{} vs {}", vol(&r), strips(2.0, 3.0, 0.0));
        // The same in one go with a radius per edge, either way round.
        for sel in [[(a, 2.0), (b, 3.0)], [(b, 3.0), (a, 2.0)]] {
            let s = fillet_radii(&bx, &sel).unwrap();
            assert!((vol(&s) - vol(&r)).abs() < 1e-3);
        }
        let s = fillet_radii(&bx, &[(a, 3.0), (b, 2.0)]).unwrap();
        assert!((vol(&s) - strips(3.0, 2.0, 0.0)).abs() < 0.05);
        // Equal radii: the mitre.
        let e = fillet(&ra, &[b], 2.0).unwrap();
        assert!((vol(&e) - strips(2.0, 2.0, 0.0)).abs() < 0.05);
    }

    #[test]
    fn issue1557_vertical_after_bottom_rounds() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        let vertical = Vec3::new(25.0, 0.0, 6.0);
        let seam = Vec3::new(24.287266336668306, 0.7127336633316916, 0.48266373325510686);
        let mut together = bottom.to_vec();
        together.push(vertical);
        let reference = fillet(&b, &together, 2.0).unwrap();
        // The vertical edge, alone or with the seam, with its tangent chain (as the Fillet feature
        // asks for it): every pick gives the same body.
        for picks in [vec![vertical], vec![vertical, seam], vec![seam, vertical]] {
            let result = fillet(&rounded, &crate::tangent_chain(&rounded, &picks).unwrap(), 2.0).unwrap();
            assert_eq!(result.face_count(), reference.face_count());
            assert_eq!(result.unmeshed_faces(), 0);
            assert!(result.validity().is_empty(), "{:?}", result.validity());
            assert!((vol(&result) - vol(&reference)).abs() < 0.05);
            assert!(!result.contains(Vec3::new(24.9, 0.1, 5.0)));
            assert!(result.contains(Vec3::new(24.0, 1.0, 5.0)));
        }
    }

    #[test]
    fn issue1557_connected_rounds_keep_their_radii() {
        let b = box_solid(Vec3::ZERO, Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom = [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)];
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        let vertical = Vec3::new(25.0, 0.0, 6.0);
        let seam = Vec3::new(24.287266336668306, 0.7127336633316916, 0.48266373325510686);
        for radius in [0.5, 1.0, 3.0] {
            let mut together: Vec<_> = bottom.iter().map(|p| (*p, 2.0)).collect();
            together.push((vertical, radius));
            let reference = fillet_radii(&b, &together).unwrap();
            let result = fillet(&rounded, &[seam, vertical], radius).unwrap();
            assert!(result.validity().is_empty(), "{:?}", result.validity());
            assert!((vol(&result) - vol(&reference)).abs() < 0.05, "radius {radius}");
            // The old bottom radius is still 2, away from the newly rounded corner.
            assert!(!result.contains(Vec3::new(12.5, 0.5, 0.5)));
            assert!(result.contains(Vec3::new(12.5, 1.0, 1.0)));
        }
        // A tangent boundary of an earlier round is not a sharp edge, and cannot be
        // reinterpreted as the sharp edge exposed when that round is taken off.
        assert!(fillet(&rounded, &[Vec3::new(12.5, 0.0, 2.0)], 1.0).is_err());
        // Consuming a mitre must not silently discard its independently requested radius,
        // or turn a mitre-only selection into a different sharp edge.
        assert!(fillet_radii(&rounded, &[(vertical, 1.0), (seam, 2.0)]).is_err());
        assert!(fillet_radii(&rounded, &[(seam, 2.0)]).is_err());
    }

    #[test]
    fn issue2422_vertical_after_separate_bottom_rounds() {
        let origin = Vec3::new(-5.0, 7.2, -2.0);
        let b = box_solid(origin, origin + Vec3::new(40.0, 80.0, 27.0)).unwrap();
        let bottom = [Vec3::new(15.0, 7.2, -2.0), Vec3::new(35.0, 47.2, -2.0), Vec3::new(15.0, 87.2, -2.0), Vec3::new(-5.0, 47.2, -2.0)];
        // The issue has no design attached. Reconstruct its outside corner and bottom mitre,
        // using the reported picks, with and without the open cavity visible in the screenshot.
        let picks = [Vec3::new(-5.0, 87.2, 12.5), Vec3::new(-4.283446394180081, 86.48344639418008, -1.5001085324947512)];
        for base in [b.clone(), crate::shell(&b, &[Vec3::new(15.0, 47.2, 25.0)], 1.0).unwrap()] {
            let mut rounded = base;
            for edge in bottom {
                rounded = fillet(&rounded, &[edge], 2.0).unwrap();
            }
            let reference = fillet(&rounded, &picks[..1], 2.0).unwrap();
            for selection in [picks, [picks[1], picks[0]]] {
                let result = fillet(&rounded, &selection, 2.0).unwrap();
                assert_eq!(result.face_count(), reference.face_count());
                assert_eq!(result.unmeshed_faces(), 0);
                assert!(result.validity().is_empty(), "{:?}", result.validity());
                assert!((vol(&result) - vol(&reference)).abs() < 0.05);
                assert!(!result.contains(Vec3::new(-4.9, 87.1, 10.0)));
                assert!(result.contains(Vec3::new(-4.1, 86.3, 10.0)));
            }
        }
    }

    #[test]
    fn issue1557_all_vertical_corners_match_analytic_volume() {
        let origin = Vec3::new(10.0, -5.0, 7.0);
        let b = box_solid(origin, origin + Vec3::new(25.0, 25.0, 10.0)).unwrap();
        let bottom =
            [Vec3::new(12.5, 0.0, 0.0), Vec3::new(25.0, 12.5, 0.0), Vec3::new(12.5, 25.0, 0.0), Vec3::new(0.0, 12.5, 0.0)].map(|p| p + origin);
        let vertical =
            [Vec3::new(0.0, 0.0, 6.0), Vec3::new(25.0, 0.0, 6.0), Vec3::new(25.0, 25.0, 6.0), Vec3::new(0.0, 25.0, 6.0)].map(|p| p + origin);
        let rounded = fillet(&b, &bottom, 2.0).unwrap();
        // Each vertical edge with its tangent chain (on through the bottom rounds' corners), as
        // the Fillet feature asks for it.
        let result = fillet(&rounded, &crate::tangent_chain(&rounded, &vertical).unwrap(), 2.0).unwrap();
        assert!(result.validity().is_empty(), "{:?}", result.validity());
        assert!(kink(&result) < 0.05);
        // Integrate rounded-square horizontal sections. Above z=r the area is
        // L²-(4-π)r²; below, a square of side L-2r plus a disk offset of radius
        // sqrt(r²-(z-r)²). This includes the four spherical bottom corners.
        let (l, h, r) = (25.0, 10.0, 2.0);
        let pi = std::f64::consts::PI;
        let a = l - 2.0 * r;
        let expected = (h - r) * (l * l - (4.0 - pi) * r * r) + a * a * r + pi * a * r * r + 2.0 * pi * r * r * r / 3.0;
        assert!((vol(&result) - expected).abs() < 0.1, "{} vs {expected}", vol(&result));
    }

    #[test]
    fn three_rounds_at_a_corner() {
        let (bx, a, b, c) = setup();
        // Two rounds meeting in a mitre, then the third edge: a sphere octant.
        let ab = fillet(&bx, &[a, b], 2.0).unwrap();
        let r = fillet(&ab, &[c], 2.0).unwrap();
        let sphere = 8000.0 - 3.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 18.0 - 8.0 * (1.0 - std::f64::consts::PI / 6.0);
        assert_eq!(r.face_count(), 10);
        assert!((vol(&r) - sphere).abs() < 0.05, "{} vs {sphere}", vol(&r));
        assert!(kink(&r) < 0.01);
        // Different radii: a smooth patch over the corner, slightly more than the strips take.
        for (rs, sel) in [((2.0, 2.0, 3.0), fillet(&ab, &[c], 3.0)), ((2.0, 3.0, 4.0), fillet_radii(&bx, &[(a, 2.0), (b, 3.0), (c, 4.0)]))] {
            let r = sel.unwrap();
            assert_eq!(r.face_count(), 10);
            assert_eq!(r.unmeshed_faces(), 0);
            let k = kink(&r);
            assert!(k < 0.05, "kink {k}°");
            let (v, s) = (vol(&r), strips(rs.0, rs.1, rs.2));
            assert!(v < s + 0.05 && v > s - 3.0, "{v} vs {s}");
        }
    }

    #[test]
    fn corners_refuse_what_they_cannot_build() {
        let (bx, a, b, _) = setup();
        // Larger than the faces, or not a size: errors, no panic. Large but fitting is fine.
        assert!(fillet_radii(&bx, &[(a, 15.0), (b, 2.0)]).is_ok_and(|r| r.unmeshed_faces() == 0));
        assert!(fillet_radii(&bx, &[(a, 25.0), (b, 2.0)]).is_err());
        assert!(fillet_radii(&bx, &[(a, f64::NAN)]).is_err());
        assert!(fillet_radii(&bx, &[(a, 0.0)]).is_err());
        assert!(fillet_radii(&bx, &[]).is_err());
        assert!(fillet_radii(&bx, &[(Vec3::new(500.0, 0.0, 0.0), 1.0)]).is_err());
    }

    #[test]
    fn sloped_basin_rim_fillets() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let pocket =
            crate::loft(&[(Plane::XY.offset(30.0), rectangle(20.0, 4.0, 80.0, 50.0)), (Plane::XY.offset(10.0), rectangle(30.0, 20.0, 70.0, 40.0))])
                .unwrap();
        let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let picks = [Vec3::new(50.0, 4.0, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
        for count in 1..=4 {
            let result = fillet(&basin, &picks[..count], 2.0).unwrap();
            assert!(result.validity().is_empty());
            assert_eq!(result.unmeshed_faces(), 0);
            assert!(vol(&result) < vol(&basin));
            let mesh = result.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
            assert!(mesh.contains(Vec3::new(50.0, 2.0, 27.0)));
            assert!(!mesh.contains(Vec3::new(50.0, 3.9, 29.99)));
            // The small blend removes only a strip at the rim, and never escapes
            // the slab. Topology checks alone do not catch crossed faces.
            assert!(vol(&basin) - vol(&result) < 2.0_f64.powi(2) * (60.0 + 46.0) * 2.0);
            for p in vertices(&result) {
                assert!(p.x >= 0.0 && p.x <= 100.0 && p.y >= 0.0 && p.y <= 60.0 && p.z >= 0.0 && p.z <= 30.0 + 1e-8);
            }
        }
    }

    #[test]
    fn sloped_rim_fillets_cannot_extend_outside_the_material() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        for margin in [0.5, 1.0] {
            let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
            let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
            let pocket = crate::loft(&[
                (Plane::XY.offset(30.0), rectangle(20.0, margin, 80.0, 50.0)),
                (Plane::XY.offset(10.0), rectangle(30.0, 20.0, 70.0, 40.0)),
            ])
            .unwrap();
            let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
            let picks = [Vec3::new(50.0, margin, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
            for count in [1, 2, 4] {
                assert!(fillet(&basin, &picks[..count], 4.0).is_err());
                assert!(fillet(&basin, &picks[..count], margin * 0.5).is_ok());
            }
        }
    }

    #[test]
    fn chamfers_meet_on_unequal_sloped_rim_walls() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        for margin in [1.0, 4.0] {
            let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
            let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
            let pocket = crate::loft(&[
                (Plane::XY.offset(30.0), rectangle(20.0, margin, 80.0, 50.0)),
                (Plane::XY.offset(10.0), rectangle(30.0, 20.0, 70.0, 40.0)),
            ])
            .unwrap();
            let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
            let picks = [Vec3::new(50.0, margin, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
            for count in [2, 3, 4] {
                if margin == 1.0 {
                    assert!(crate::chamfer(&basin, &picks[..count], 2.0).is_err());
                    assert!(crate::chamfer(&basin, &picks[..count], 1.0).is_err());
                }
                let distance = if margin == 1.0 { 0.5 } else { 2.0 };
                let result = crate::chamfer(&basin, &picks[..count], distance).unwrap();
                assert!(result.validity().is_empty());
                assert_eq!(result.unmeshed_faces(), 0);
                assert!(vol(&result) < vol(&basin));
                assert!(vol(&basin) - vol(&result) < distance.powi(2) * (60.0 + 50.0 - margin));
                let reverse: Vec<_> = picks[..count].iter().copied().rev().collect();
                let reversed = crate::chamfer(&basin, &reverse, distance).unwrap();
                assert!((vol(&result) - vol(&reversed)).abs() < 1e-6);
                // These are exact planes, with every boundary on its owning face.
                let rec = Faces::of(&result).unwrap();
                for f in 0..rec.rec.faces.len() {
                    let Some(n) = rec.plane_normal(f) else { continue };
                    let points: Vec<Vec3> = rec.uses(f).filter_map(|u| rec.ends(u.edge as usize)).flat_map(|(a, b)| [a, b]).collect();
                    let origin = points[0];
                    for p in points {
                        assert!((p - origin).dot(n).abs() < 1e-7);
                    }
                }
            }
        }
    }

    #[test]
    fn equal_slope_rim_loops_cannot_escape_the_face() {
        use solidcraft_geom::{Loop2, Plane, Vec2};
        let rectangle = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let slab = box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let pocket =
            crate::loft(&[(Plane::XY.offset(30.0), rectangle(20.0, 1.0, 80.0, 50.0)), (Plane::XY.offset(10.0), rectangle(30.0, 11.0, 70.0, 40.0))])
                .unwrap();
        let basin = crate::boolean(&slab, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let picks = [Vec3::new(50.0, 1.0, 30.0), Vec3::new(80.0, 27.0, 30.0), Vec3::new(50.0, 50.0, 30.0), Vec3::new(20.0, 27.0, 30.0)];
        assert!(crate::chamfer(&basin, &picks, 2.0).is_err());
        assert!(fillet(&basin, &picks, 4.0).is_err());
        assert!(crate::chamfer(&basin, &picks, 0.5).is_ok());
        assert!(fillet(&basin, &picks, 0.5).is_ok());
    }
}
