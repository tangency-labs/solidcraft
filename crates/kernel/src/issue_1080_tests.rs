//! #1080: fillets and chamfers that would run past the faces beside their edge. A convex blend
//! running over a face's far edge into a pocket or a hole (a cliff) is built as if its faces went
//! on and what it removes is cut from the body, so it never bridges the opening (the owner's
//! blend-overflow decision, 237f0ae74); blends that would cross each other are refused as too
//! large, and the largest size that fits is found by searching ([`crate::fillet_max`],
//! [`crate::chamfer_max`]).

use solidcraft_geom::Vec3;

use crate::{Body, BoolOp, ChamferSide, KernelError, build::box_solid, ops::boolean};

fn cut(a: &Body, lo: [f64; 3], hi: [f64; 3]) -> Body {
    let t = box_solid(Vec3::new(lo[0], lo[1], lo[2]), Vec3::new(hi[0], hi[1], hi[2])).unwrap();
    boolean(a, &t, BoolOp::Cut).unwrap().unwrap()
}

/// A 40 × 20 × 20 block with a 10 × 10 pocket, 10 deep, 4 mm in from its top edge at x = 40.
fn pocketed() -> Body {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 20.0, 20.0)).unwrap();
    cut(&b, [26.0, 5.0, 10.0], [36.0, 15.0, 20.0])
}

const EDGE: Vec3 = Vec3 { x: 40.0, y: 10.0, z: 20.0 };

fn too_large(r: crate::Result<Body>) -> String {
    match r {
        Err(KernelError::TooLarge(m)) => m,
        Err(e) => panic!("refused, but not as too large: {e}"),
        Ok(_) => panic!("a blend that runs into another was accepted"),
    }
}

/// The pocketed block's volume: 16000 less the 1000 mm³ pocket.
const POCKETED: f64 = 15000.0;

/// The 6 mm chamfer runs 2 mm past the pocket's rim at x = 36: its triangle
/// (40 − x) + (20 − z) <= 6, area 18 along the 20 mm edge, less its part over the pocket
/// (x in [34, 36], z >= 54 − x: area 2, along the pocket's 10 mm), is cut away: 360 − 20.
#[test]
fn chamfer_into_a_pocket_is_cut_by_it() {
    let p = pocketed();
    let b = crate::chamfer(&p, &[EDGE], 6.0).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    let v = crate::measure(&b).unwrap().volume;
    assert!((v - (POCKETED - 340.0)).abs() < 1e-6, "{v}");
    // Up to the pocket's rim it builds: the triangle along the 20 mm edge.
    for s in [2.0, 4.0] {
        let b = crate::chamfer(&p, &[EDGE], s).unwrap();
        let v = b.volume_at(0.01).unwrap();
        assert!((p.volume_at(0.01).unwrap() - v - 0.5 * s * s * 20.0).abs() < 1e-6, "{s}: {v}");
    }
    // Unequal sides, 6 along the top and 2 down the side: the triangle
    // (40 − x) / 6 + (20 − z) / 2 <= 1, area 6 along 20, less its part over the pocket
    // (x in [34, 36], a triangle 2 by 2/3), along 10: 120 − 20/3. The other way round it stays
    // clear of the pocket: 120.
    let b = crate::chamfer_sides(&p, &[EDGE], 6.0, ChamferSide::Distance(2.0), false).unwrap();
    let v = crate::measure(&b).unwrap().volume;
    assert!((v - (POCKETED - (120.0 - 20.0 / 3.0))).abs() < 1e-6, "{v}");
    let b = crate::chamfer_sides(&p, &[EDGE], 2.0, ChamferSide::Distance(6.0), false).unwrap();
    let v = crate::measure(&b).unwrap().volume;
    assert!((v - (POCKETED - 120.0)).abs() < 1e-6, "{v}");
}

/// R5 runs 1 mm past the pocket's rim: the round's section (1 − π/4)·25 along 20, less its part
/// over the pocket, the square [35, 36] × [15, 20] outside the circle about (35, 15) of radius 5,
/// ∫₀¹ (5 − √(25 − t²)) dt = 5 − (√24 / 2 + (25/2)·asin(1/5)), along 10.
#[test]
fn fillet_into_a_pocket_is_cut_by_it_and_one_that_fits_builds() {
    let p = pocketed();
    let over = 5.0 - (24f64.sqrt() / 2.0 + 12.5 * (0.2f64).asin());
    let want = POCKETED - ((1.0 - std::f64::consts::FRAC_PI_4) * 25.0 * 20.0 - over * 10.0);
    let b = crate::fillet(&p, &[EDGE], 5.0).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    let v = crate::measure(&b).unwrap().volume;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
    let b = crate::fillet(&p, &[EDGE], 3.0).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    assert_eq!(b.face_count(), p.face_count() + 1);
}

/// A rib 4 mm wide: chamfers of its two top edges meet in the middle of its top face at 2 mm;
/// past that they would cross each other. Neither reaches the far side's face below 4 mm,
/// so only the two blends clash: refused as too large, and the largest that fits is 2 mm.
#[test]
fn chamfers_crossing_each_other_are_refused() {
    let rib = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 4.0, 20.0)).unwrap();
    let edges = [Vec3::new(20.0, 0.0, 20.0), Vec3::new(20.0, 4.0, 20.0)];
    too_large(crate::chamfer(&rib, &edges, 3.0));
    too_large(crate::fillet(&rib, &edges, 3.0));
    assert_eq!(crate::chamfer_max(&rib, &edges, 3.0).unwrap().map(|x| x.0), Some(2.0));
    assert_eq!(crate::fillet_max(&rib, &edges, 3.0).unwrap().map(|x| x.0), Some(2.0));
    // Below half the width both build.
    let b = crate::chamfer(&rib, &edges, 1.5).unwrap();
    assert!((b.volume_at(0.01).unwrap() - (3200.0 - 2.0 * 0.5 * 1.5 * 1.5 * 40.0)).abs() < 1e-6);
    // Two rounds of radius 1.5 along 40: each removes (1 − π/4)·1.5² of section.
    let b = crate::fillet(&rib, &edges, 1.5).unwrap();
    let want = 3200.0 - 2.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 2.25 * 40.0;
    let v = crate::measure(&b).unwrap().volume;
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}

/// Edges meeting at a corner mitre: their chamfers overlap there and are not refused for it.
#[test]
fn chamfers_meeting_at_a_corner_still_build() {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
    let edges = [Vec3::new(10.0, 0.0, 20.0), Vec3::new(20.0, 10.0, 20.0), Vec3::new(10.0, 20.0, 20.0), Vec3::new(0.0, 10.0, 20.0)];
    // The chamfers leave the 20 x 20 x 15 block under a frustum from 20 x 20 to 10 x 10, 5 high:
    // 6000 + (5/3)(400 + 100 + √(400·100)).
    let v = crate::measure(&crate::chamfer(&b, &edges, 5.0).unwrap()).unwrap().volume;
    let want = 6000.0 + 5.0 / 3.0 * (400.0 + 100.0 + 200.0);
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
    // The rounds: (1 − π/4)·25 of section along each edge's 10 mm between the corners, and in each
    // 5 mm corner cube the two cylinders' intersection is kept, ∫₀⁵ (25 − w²) dw = 250/3 of 125.
    let v = crate::measure(&crate::fillet(&b, &edges, 5.0).unwrap()).unwrap().volume;
    let want = 8000.0 - 4.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 25.0 * 10.0 - 4.0 * (125.0 - 250.0 / 3.0);
    assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
}
