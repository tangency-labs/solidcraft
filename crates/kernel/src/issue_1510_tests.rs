//! #1510: a body split by a plane that a face of the body lies in: the parts meet in the plane
//! and together are the body.

use solidcraft_geom::{Plane, Vec3};

use crate::{Body, BoolOp, build::box_solid, build::cylinder, ops::boolean, ops::split_by_plane};

fn cut(a: &Body, t: crate::Result<Body>) -> Body {
    boolean(a, &t.unwrap(), BoolOp::Cut).unwrap().unwrap()
}

/// A 2.5 mm plate with a through hole of radius 2.5 and a groove 4 mm wide and 1.5 deep from
/// the bottom, running from the hole's centre to the edge: the groove's ceiling lies at z = 1.5
/// and its boundary runs into the hole.
fn plate(width: f64) -> Body {
    let p = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 20.0, 2.5)).unwrap();
    let p = cut(&p, cylinder(Vec3::new(20.0, 10.0, -1.0), Vec3::Z, 2.5, 5.0));
    cut(&p, box_solid(Vec3::new(20.0, 10.0 - width / 2.0, -1.0), Vec3::new(41.0, 10.0 + width / 2.0, 1.5)))
}

#[test]
fn a_split_in_the_plane_of_a_face_keeps_all_of_the_body() {
    for width in [4.0, 5.0] {
        let b = plate(width);
        // Exact volumes (SMLib's, to 1e-8): a mesh's at a 1e-3 chord sags round the hole by more
        // than the millionth the parts are held to.
        let v = crate::measure(&b).unwrap().volume;
        let plane = Plane::from_normal(Vec3::new(0.0, 0.0, 1.5), Vec3::Z).unwrap();
        let parts = split_by_plane(&b, &plane).unwrap();
        assert_eq!(parts.len(), 2, "{width}");
        let vs: Vec<f64> = parts.iter().map(|p| crate::measure(p).unwrap().volume).collect();
        assert!((vs.iter().sum::<f64>() - v).abs() < 1e-6 * v, "{width}: {vs:?} against {v}");
        // Above: the plate from 1.5 to 2.5 less the hole; below: less the hole and the groove.
        let (r, h) = (2.5f64, width / 2.0);
        let hole = std::f64::consts::PI * r * r;
        // The strip 20 <= x <= 40, |y - 10| <= h, less the part of it in the hole.
        let groove = width * 20.0 - (h * (r * r - h * h).sqrt() + r * r * (h / r).asin());
        let want = [800.0 - hole, 1.5 * (800.0 - hole - groove)];
        for (got, want) in vs.iter().zip(want) {
            assert!((got - want).abs() < 0.05, "{width}: {vs:?} against {want}");
        }
        // Both parts end in the plane, not off it.
        let z: Vec<(f64, f64)> = parts.iter().map(|p| crate::measure(p).map(|m| (m.bbox.min.z, m.bbox.max.z)).unwrap()).collect();
        assert!((z[0].0 - 1.5).abs() < 1e-6 && (z[1].1 - 1.5).abs() < 1e-6, "{width}: {z:?}");
    }
}

/// A slab whose bottom rises 0.03 mm over its 700 mm length, its plane through the origin: its
/// normal is within 1e-9 of the split plane's, but it doesn't lie in the plane, so the part on
/// the upper side is the whole slab.
#[test]
fn a_face_only_nearly_in_the_plane_is_not_moved() {
    let x = Vec3::new(700.0, 0.0, 0.03).normalized().unwrap();
    let bottom = Plane::new(Vec3::ZERO, x, Vec3::Y).unwrap();
    let outer = solidcraft_geom::Loop2::polygon(&[
        solidcraft_geom::Vec2::new(0.0, 0.0),
        solidcraft_geom::Vec2::new(Vec3::new(700.0, 0.0, 0.03).len(), 0.0),
        solidcraft_geom::Vec2::new(Vec3::new(700.0, 0.0, 0.03).len(), 100.0),
        solidcraft_geom::Vec2::new(0.0, 100.0),
    ]);
    let b = crate::extrude(&bottom, &[solidcraft_geom::Region2 { outer, holes: vec![] }], 0.0, 10.0).unwrap().pop().unwrap();
    let plane = Plane::from_normal(Vec3::ZERO, Vec3::Z).unwrap();
    let v0 = b.volume_at(1e-2).unwrap();
    let up = crate::ops::half_space(&b, &plane, true).unwrap().0.unwrap();
    let v1 = up.volume_at(1e-2).unwrap();
    assert!((v1 - v0).abs() < 1e-3 * v0, "{v1} against {v0}");
}
