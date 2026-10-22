//! #1080: fillets and chamfers that would run past the faces beside their edge (into a pocket,
//! a hole, another blend) are refused with the largest size that fits, not built through it.

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
        Ok(_) => panic!("a blend through the pocket was accepted"),
    }
}

#[test]
fn chamfer_into_a_pocket_is_refused_with_the_size_that_fits() {
    let p = pocketed();
    let m = too_large(crate::chamfer(&p, &[EDGE], 6.0));
    assert!(m.contains("runs past the faces beside the edge") && m.contains("the largest size that fits is 4 mm"), "{m}");
    // Up to the pocket's rim it builds: the triangle along the 20 mm edge.
    for s in [2.0, 4.0] {
        let b = crate::chamfer(&p, &[EDGE], s).unwrap();
        let v = b.volume_at(0.01).unwrap();
        assert!((p.volume_at(0.01).unwrap() - v - 0.5 * s * s * 20.0).abs() < 1e-6, "{s}: {v}");
    }
    // Unequal sides: along the top 6 is past the pocket, along the side it is not.
    let m = too_large(crate::chamfer_sides(&p, &[EDGE], 6.0, ChamferSide::Distance(2.0), false));
    assert!(m.contains("runs past"), "{m}");
    assert!(crate::chamfer_sides(&p, &[EDGE], 2.0, ChamferSide::Distance(6.0), false).is_ok());
}

#[test]
fn fillet_into_a_pocket_is_refused_and_one_that_fits_builds() {
    let p = pocketed();
    // The round build refuses it itself; the largest radius that fits is added.
    let m = crate::fillet(&p, &[EDGE], 5.0).unwrap_err().to_string();
    assert!(m.contains("the largest radius that fits is 4 mm"), "{m}");
    let b = crate::fillet(&p, &[EDGE], 3.0).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    assert_eq!(b.face_count(), p.face_count() + 1);
}

/// A rib 4 mm wide: chamfers of its two top edges meet in the middle of its top face at 2 mm;
/// past that they would cross each other. Neither reaches the far side's face below 4 mm,
/// so only the two blends clash.
#[test]
fn chamfers_crossing_each_other_are_refused() {
    let rib = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 4.0, 20.0)).unwrap();
    let edges = [Vec3::new(20.0, 0.0, 20.0), Vec3::new(20.0, 4.0, 20.0)];
    let m = too_large(crate::chamfer(&rib, &edges, 3.0));
    assert!(m.contains("would cross each other") && m.contains("the largest size that fits is 2 mm"), "{m}");
    let m = crate::fillet(&rib, &edges, 3.0).unwrap_err().to_string();
    assert!(m.contains("the largest radius that fits is 2 mm"), "{m}");
    // Below half the width both build.
    let b = crate::chamfer(&rib, &edges, 1.5).unwrap();
    assert!((b.volume_at(0.01).unwrap() - (3200.0 - 2.0 * 0.5 * 1.5 * 1.5 * 40.0)).abs() < 1e-6);
    assert!(crate::fillet(&rib, &edges, 1.5).is_ok());
}

/// Edges meeting at a corner mitre: their chamfers overlap there and are not refused for it.
#[test]
fn chamfers_meeting_at_a_corner_still_build() {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0)).unwrap();
    let edges = [Vec3::new(10.0, 0.0, 20.0), Vec3::new(20.0, 10.0, 20.0), Vec3::new(10.0, 20.0, 20.0), Vec3::new(0.0, 10.0, 20.0)];
    assert!(crate::chamfer(&b, &edges, 5.0).is_ok());
    assert!(crate::fillet(&b, &edges, 5.0).is_ok());
}

/// #1668: a box shelled outward 2 mm, open at the bottom. R5 on its top outer edge alone leaves
/// the sharp inside corner 2 mm away on its ball's side, 0.76 mm inside the round; it reaches
/// the corner only past 2√2 / (√2 - 1) ≈ 6.83 mm. With the inside edge rounded in the same
/// fillet, the two build as one feature as they do one after the other: the outer round takes
/// (1 - π/4) · 25 mm² along 104 mm, the inner one adds it along 100 mm.
#[test]
fn rounds_on_both_sides_of_a_wall_build_as_one_fillet() {
    let outer = box_solid(Vec3::new(-52.0, -32.0, 0.0), Vec3::new(52.0, 32.0, 42.0)).unwrap();
    let shelled = cut(&outer, [-50.0, -30.0, -1.0], [50.0, 30.0, 40.0]);
    let (out_edge, in_edge) = (Vec3::new(0.0, 32.0, 42.0), Vec3::new(0.0, 30.0, 40.0));
    let section = (1.0 - std::f64::consts::FRAC_PI_4) * 25.0;
    let alone = crate::fillet(&shelled, &[out_edge], 5.0).unwrap();
    assert!(alone.validity().is_empty(), "{:?}", alone.validity());
    let want = shelled.volume_at(0.01).unwrap() - section * 104.0;
    assert!((crate::measure(&alone).unwrap().volume - want).abs() < 1e-4 * want);
    let m = too_large(crate::fillet(&shelled, &[out_edge], 7.0));
    let largest: f64 = m.rsplit("that fits is ").next().and_then(|t| t.trim_end_matches(" mm").parse().ok()).unwrap_or(0.0);
    let limit = 2.0 * 2f64.sqrt() / (2f64.sqrt() - 1.0);
    assert!(largest <= limit && largest > limit - 0.5, "{m}");
    let both = crate::fillet(&shelled, &[out_edge, in_edge], 5.0).unwrap();
    assert!(both.validity().is_empty(), "{:?}", both.validity());
    let want = shelled.volume_at(0.01).unwrap() - section * 104.0 + section * 100.0;
    let v = crate::measure(&both).unwrap().volume;
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    let stepped = crate::fillet(&crate::fillet(&shelled, &[in_edge], 5.0).unwrap(), &[out_edge], 5.0).unwrap();
    assert!((crate::measure(&stepped).unwrap().volume - v).abs() < 1e-4 * want);
    // Past the wall's own room they still cross: R7's outer round reaches the inner one's arc.
    let m = too_large(crate::fillet(&shelled, &[out_edge, in_edge], 7.0));
    assert!(m.contains("the largest radius that fits"), "{m}");
}

/// A shell 3 mm thick bent over like an upside-down U: 60 wide, 20 long, 50 high, its top
/// plate from z = 47 to 50 and its walls from x = 0 to 3 and 57 to 60.
fn bent_shell() -> Body {
    let b = box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(60.0, 20.0, 50.0)).unwrap();
    cut(&b, [3.0, -1.0, -1.0], [57.0, 21.0, 47.0])
}

/// The corner left by a round of radius `r` on a 90° edge, per unit length.
fn corner(r: f64) -> f64 {
    r * r * (1.0 - std::f64::consts::FRAC_PI_4)
}

/// An outside round concentric with the inside one keeps the wall 3 mm thick: the inside round
/// lies inside the outside one's wedge, but on its ball's side of the arc, which the round
/// leaves as it is. It used to be refused as running into other geometry.
#[test]
fn outside_round_concentric_with_the_inside_round_of_a_wall_fits() {
    let s = bent_shell();
    let inside = [Vec3::new(3.0, 10.0, 47.0), Vec3::new(57.0, 10.0, 47.0)];
    let outside = [Vec3::new(0.0, 10.0, 50.0), Vec3::new(60.0, 10.0, 50.0)];
    let a = crate::fillet(&s, &inside, 15.0).unwrap();
    let b = crate::fillet(&a, &outside, 18.0).unwrap();
    assert!(b.validity().is_empty(), "{:?}", b.validity());
    // The inside rounds fill their corners, the outside ones take theirs away.
    let want = s.volume_at(0.01).unwrap() + 2.0 * 20.0 * (corner(15.0) - corner(18.0));
    let got = b.volume_at(0.01).unwrap();
    assert!((got - want).abs() < 1e-3 * want, "{got} for {want}");
    // Larger, the outside round's centre moves off the inside one's: it runs into the hollow
    // where its arc crosses the inside round's, at r = (18√2 - 15) / (√2 - 1) ≈ 25.24 mm.
    let m = too_large(crate::fillet(&a, &outside, 30.0));
    let largest: f64 = m.rsplit("that fits is ").next().and_then(|t| t.trim_end_matches(" mm").parse().ok()).unwrap_or(0.0);
    let limit = (18.0 * 2f64.sqrt() - 15.0) / (2f64.sqrt() - 1.0);
    assert!(largest <= limit && largest > limit - 0.5, "{m}");
}
