#![allow(clippy::unwrap_used, clippy::panic)]
use crate::*;
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, UnitVec3, Vec2, Vec3};

fn fixture() -> (Body, Body, Vec3, f64) {
    let slope = 1f64.to_radians().tan();
    let hs = |normal: Vec3, point: Vec3| {
        let n = normal.normalized().unwrap();
        HalfSpace { n, d: n.dot(point) }
    };
    let stock = convex_polyhedron(&[
        hs(Vec3::X, Vec3::new(40., 0., 0.)),
        hs(-Vec3::X, Vec3::ZERO),
        hs(Vec3::Y, Vec3::new(0., 30., 0.)),
        hs(-Vec3::Y, Vec3::ZERO),
        hs(-Vec3::Z, Vec3::ZERO),
        hs(Vec3::new(-slope, 0., 1.), Vec3::new(0., 0., 20.)),
    ])
    .unwrap();
    let cutter = cylinder(Vec3::new(20., 15., -5.), Vec3::Z, 5., 40.).unwrap();
    let bored = boolean(&stock, &cutter, BoolOp::Cut).unwrap().unwrap();
    let web = box_solid(Vec3::new(14., 14., 16.05), Vec3::new(26., 16., 16.07)).unwrap();
    let target = boolean(&bored, &web, BoolOp::Union).unwrap().unwrap();
    let region = Region2 { outer: Loop2::circle(Vec2::new(20., 15.), 8.), holes: vec![Loop2::circle(Vec2::new(20., 15.), 5.).reversed()] };
    let ring = extrude(&Plane::XY, &[region], 14., 24.).unwrap().remove(0);
    let web_volume = 0.02 * 2. * (24f64.sqrt() + 25. * 0.2f64.asin());
    let fine_target = target.tessellate_with(1e-4, true).unwrap();
    let added = fine_target.measure().volume - bored.tessellate_with(1e-4, true).unwrap().measure().volume;
    assert!((added - web_volume).abs() < 0.005, "actual connected web adds {added}, expected {web_volume}");
    for x in [16., 20., 24.] {
        for y in [14.2, 15., 15.8] {
            assert!(fine_target.contains(Vec3::new(x, y, 16.06)), "source web must exist at {x},{y}");
        }
    }
    assert!(!fine_target.contains(Vec3::new(20., 12., 16.06)));
    let floor_volume = (20. + slope * 20.) * (1200. - 25. * std::f64::consts::PI);
    let ring_added = std::f64::consts::PI * (64. - 25.) * (24. - (20. + slope * 20.));
    (target, ring, Vec3::new(20., 15., 16.06), floor_volume + web_volume + ring_added)
}

#[test]
fn thin_connected_web_between_bore_stations_survives_union() {
    let (target, ring, probe, expected) = fixture();
    let target_before = step_export(&[&target], "source").unwrap();
    let ring_before = step_export(&[&ring], "ring").unwrap();
    let size = target.size().max(ring.size());
    let tol = (size * 5e-4).max(size * 1e-5);
    let ma = target.tessellate_with(tol, true).unwrap();
    let mb = ring.tessellate_with(tol, true).unwrap();
    assert!(ma.contains(probe), "fixture must contain the real connected web");
    assert!(!mb.contains(probe));
    let va = ma.measure().volume;
    let vb = mb.measure().volume;
    let slack = 2e-3 * (va + vb) + 1e-9;
    let accept = |body: &Body| {
        let mr = body.tessellate_with(tol, true).unwrap();
        let v = mr.measure().volume;
        let mismatch = super::mismatch(&ma, &mb, &mr, BoolOp::Union, 0.0);
        eprintln!("fallback candidate V={v}, expected={expected}, web={}, mismatch={mismatch:?}", mr.contains(probe));
        v > 0. && v >= va.max(vb) - slack && v <= va + vb + slack && mismatch.is_none_or(|f| f < 0.004)
    };
    let direct = crate::coplanar::bore_union(&target, &ring, &accept);
    eprintln!("direct fallback result={}", direct.is_some());
    let public = boolean(&target, &ring, BoolOp::Union);
    if let Ok(dir) = solidcraft_geom::env::var("BORE_AUDIT_DIR").ok_or(()) {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("target-with-web.step"), step_export(&[&target], "target with web").unwrap()).unwrap();
        if let Some(result) = &direct {
            std::fs::write(dir.join("direct-union.step"), step_export(&[result], "direct union").unwrap()).unwrap();
        }
        if let Ok(Some(result)) = &public {
            std::fs::write(dir.join("public-union.step"), step_export(&[result], "public union").unwrap()).unwrap();
        }
    }
    let direct_keeps_web = direct.as_ref().is_none_or(|result| result.tessellate_with(1e-3, true).unwrap().contains(probe));
    let public_keeps_web = match &public {
        Ok(Some(result)) => {
            let keep = result.tessellate_with(1e-3, true).unwrap().contains(probe);
            eprintln!("public Union result web={keep}");
            keep
        }
        result => {
            eprintln!("public Union honestly refused: {result:?}");
            true
        }
    };
    assert!(direct_keeps_web, "accepted fallback removed real web material");
    assert!(public_keeps_web, "public Union removed real web material");
    assert_eq!(step_export(&[&target], "source").unwrap(), target_before);
    assert_eq!(step_export(&[&ring], "ring").unwrap(), ring_before);
}

#[test]
fn unproved_curved_boundary_between_clear_endpoints_refuses_recut() {
    let a = Vec2::new(6., -1.);
    let b = Vec2::new(6., 1.);
    let c = Vec2::new(8., 1.);
    let d = Vec2::new(8., -1.);
    let region = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Cubic { p0: a, p1: Vec2::new(3., -0.5), p2: Vec2::new(3., 0.5), p3: b },
                Seg2::Line { a: b, b: c },
                Seg2::Line { a: c, b: d },
                Seg2::Line { a: d, b: a },
            ],
        }
        .reversed(),
        holes: vec![],
    };
    let body = extrude(&Plane::XY, &[region], 16.05, 16.07).unwrap().remove(0);
    assert!(a.len() > 5. && b.len() > 5., "endpoint samples miss the interior excursion");
    assert!(body.tessellate_with(1e-4, true).unwrap().contains(Vec3::new(4.5, 0., 16.06)));
    let before = step_export(&[&body], "curved source").unwrap();
    assert!(crate::bore_guard::boundaries_clear(&body, &[], Vec3::ZERO, UnitVec3::Z, 5., 14., 24., 1e-6).is_none());
    assert_eq!(step_export(&[&body], "curved source").unwrap(), before);
}

/// Posts sunk into one floor have their tops in a common plane but apart: they don't
/// coincide, so joining another post doesn't go through pushing faces apart (#261).
#[test]
fn coplanar_faces_apart_from_each_other_do_not_coincide() {
    let floor = box_solid(Vec3::new(0., 0., 0.), Vec3::new(80., 60., 2.)).unwrap();
    let post = |x: f64| cylinder(Vec3::new(x, 30., 1.), Vec3::Z, 3., 6.).unwrap();
    let with_one = boolean(&floor, &post(10.), BoolOp::Union).unwrap().unwrap();
    assert!(crate::coplanar::coincident_planes(&with_one, &post(60.)).is_empty());
    assert!(crate::coplanar::boolean_apart(&with_one, &post(60.), BoolOp::Union).is_none());
    // A post over the first one's top does meet it there.
    assert!(!crate::coplanar::coincident_planes(&with_one, &cylinder(Vec3::new(12., 30., 7.), Vec3::Z, 3., 6.).unwrap()).is_empty());
    let joined = boolean(&with_one, &post(60.), BoolOp::Union).unwrap().unwrap();
    let v = joined.tessellate_with(1e-3, true).unwrap().measure().volume;
    let expected = 80. * 60. * 2. + 2. * std::f64::consts::PI * 9. * 5.;
    assert!((v - expected).abs() < 0.5, "volume {v}, expected {expected}");
}
