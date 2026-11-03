use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::*;

/// Robustness seed 57's part: a block with an arc side, a ball cut from the edge of its back
/// face, just touching its bottom face.
fn block_with_ball_cut() -> Body {
    let p = Vec2::new;
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let (c, r) = (p(24.0625, 11.25), 26.5625);
    let start = (p(0.0, 22.5) - c).angle();
    let sweep = ((p(0.0, 0.0) - c).angle() - start).rem_euclid(std::f64::consts::TAU);
    let segs = vec![
        line(p(0.0, 0.0), p(47.5, 0.0)),
        line(p(47.5, 0.0), p(47.5, 22.5)),
        line(p(47.5, 22.5), p(27.5, 22.5)),
        line(p(27.5, 22.5), p(27.5, 12.5)),
        line(p(27.5, 12.5), p(15.0, 12.5)),
        line(p(15.0, 12.5), p(15.0, 22.5)),
        line(p(15.0, 22.5), p(0.0, 22.5)),
        Seg2::Arc { center: c, radius: r, start, sweep },
    ];
    let block = crate::extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 17.5).unwrap().remove(0);
    let ball = crate::sphere(Vec3::new(2.5, 22.5, 5.0), 5.0).unwrap();
    boolean(&block, &ball, BoolOp::Cut).unwrap().unwrap()
}

/// The ball's rim on the back face dips to 0.04 mm above the bottom: a 0.5 mm chamfer of it
/// would run off the back face, and what was built grew the part (by 0.65 mm³, more with other
/// edge orders). On a convex edge a chamfer takes material away, so it is refused, as too large
/// (SMLib's NO_ROOM).
#[test]
fn a_chamfer_that_would_grow_the_part_on_a_convex_edge_is_refused() {
    let body = block_with_ball_cut();
    let rim = Vec3::new(3.1526309873924174, 22.5, 0.04277569594852188);
    let e = chamfer(&body, &[rim], 0.5).expect_err("refused");
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
}

/// Curved convex edges that a chamfer does take material from still build: a hole's rim in a
/// plate.
#[test]
fn a_chamfer_of_a_hole_rim_still_builds() {
    let plate = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(40.0, 30.0, 10.0)).unwrap();
    let hole = crate::cylinder(Vec3::new(20.0, 15.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
    let body = boolean(&plate, &hole, BoolOp::Cut).unwrap().unwrap();
    let v0 = crate::measure(&body).unwrap().volume;
    let out = chamfer(&body, &[Vec3::new(25.0, 15.0, 10.0)], 1.0).unwrap();
    assert!(out.validity().is_empty(), "{:?}", out.validity());
    // A ring of a 1 mm chamfer round a 5 mm hole: (1/2)·2π·(5 + 1/3).
    let taken = v0 - crate::measure(&out).unwrap().volume;
    let want = std::f64::consts::PI * (5.0 + 1.0 / 3.0);
    assert!((taken - want).abs() < 0.02 * want, "{taken} vs {want}");
}
