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
    let v0 = 104.0 * 64.0 * 42.0 - 100.0 * 60.0 * 40.0;
    let (out_edge, in_edge) = (Vec3::new(0.0, 32.0, 42.0), Vec3::new(0.0, 30.0, 40.0));
    let section = (1.0 - std::f64::consts::FRAC_PI_4) * 25.0;
    let alone = crate::fillet(&shelled, &[out_edge], 5.0).unwrap();
    assert!(alone.validity().is_empty(), "{:?}", alone.validity());
    let want = shelled.volume_at(0.01).unwrap() - section * 104.0;
    assert!((crate::measure(&alone).unwrap().volume - want).abs() < 1e-4 * want);
    // R7 alone passes the inside corner: the round is cut from the wall as if the hollow were
    // not there, which opens a slit along the hollow's 100 mm. Its section (1 - π/4)·49 along
    // 104, less its part over the hollow, the square [25, 30] × [35, 40] outside the circle about
    // (25, 35) of radius 7: 5 (5 - √24) - (F(5) - F(√24)), F(t) = (t √(49 - t²) + 49 asin(t/7)) / 2.
    let f = |t: f64| 0.5 * (t * (49.0 - t * t).sqrt() + 49.0 * (t / 7.0).asin());
    let over = 5.0 * (5.0 - 24f64.sqrt()) - (f(5.0) - f(24f64.sqrt()));
    let section7 = (1.0 - std::f64::consts::FRAC_PI_4) * 49.0;
    let wide = crate::fillet(&shelled, &[out_edge], 7.0).unwrap();
    assert!(wide.validity().is_empty(), "{:?}", wide.validity());
    let want = v0 - (section7 * 104.0 - over * 100.0);
    let v = crate::measure(&wide).unwrap().volume;
    assert!((v - want).abs() < 1e-7 * want, "{v} vs {want}");
    let both = crate::fillet(&shelled, &[out_edge, in_edge], 5.0).unwrap();
    assert!(both.validity().is_empty(), "{:?}", both.validity());
    let want = shelled.volume_at(0.01).unwrap() - section * 104.0 + section * 100.0;
    let v = crate::measure(&both).unwrap().volume;
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    let stepped = crate::fillet(&crate::fillet(&shelled, &[in_edge], 5.0).unwrap(), &[out_edge], 5.0).unwrap();
    assert!((crate::measure(&stepped).unwrap().volume - v).abs() < 1e-4 * want);
    // Both at R7: the arcs, about (25, 35) and (23, 33), cross only outside both quarter arcs,
    // and the inner round's material holds the part of the outer round over the hollow (at least
    // 9.7 mm from (23, 33)), so the wall stays closed: the outer round takes its whole section
    // along 104 and the inner one adds it along 100.
    let both = crate::fillet(&shelled, &[out_edge, in_edge], 7.0).unwrap();
    assert!(both.validity().is_empty(), "{:?}", both.validity());
    let want = v0 - section7 * 4.0;
    let v = crate::measure(&both).unwrap().volume;
    assert!((v - want).abs() < 1e-7 * want, "{v} vs {want}");
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
