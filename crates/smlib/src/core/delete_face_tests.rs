//! Delete Face on fillet and chamfer faces ([`super::delete_face::delete_fillet_faces`]): each
//! round or chamfer taken off gives the sharp body back, against closed forms.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use super::blend::{Corners, blend};
use super::boolean::boolean;
use super::delete_face::delete_fillet_faces;
use super::sweep::extrude;
use super::{Body, counts, make_box, make_cone, make_cylinder, volume};
use crate::sweeps::{Curve, DraftCorner};
use crate::{Blend, BooleanOp};

const TOL: f64 = 0.01;

fn round(body: &Body, edge: [f64; 3], r: f64) -> Body {
    blend(body, Blend::Round, &[edge], &[r], TOL, &[], &[], Corners::Filled).unwrap()
}

/// A plate 40 × 30 × 5 with a boss Ø10, 8 high, on it: 6000 + 200π.
fn bossed() -> Body {
    let plate = make_box([0.0; 3], [40.0, 30.0, 5.0]).unwrap();
    let boss = make_cylinder([20.0, 15.0, 5.0], 5.0, 8.0).unwrap();
    boolean(&plate, &boss, BooleanOp::Union).unwrap().unwrap()
}

#[test]
fn deleting_a_round_on_a_box_edge_gives_the_sharp_box() {
    let cube = make_box([0.0; 3], [20.0; 3]).unwrap();
    let rounded = round(&cube, [10.0, 20.0, 20.0], 2.0);
    // On the round, half way round: the axis at y = z = 18, 2 out at 45°.
    let at = 18.0 + 2.0 * FRAC_1_SQRT_2;
    let sharp = delete_fillet_faces(&rounded, &[[10.0, at, at]], TOL).unwrap();
    assert!((volume(&sharp, 1e-9).unwrap() - 8000.0).abs() < 1e-6, "{}", volume(&sharp, 1e-9).unwrap());
    assert_eq!(counts(&sharp).unwrap(), [6, 12, 8]);
}

#[test]
fn deleting_a_boss_foot_round_or_top_round_gives_the_sharp_boss() {
    let sharp = 6000.0 + 200.0 * PI;
    // Edges are picked at +y, off the circles' seam at +x.
    // The foot round, r 1: its tube's centre circle at radius 6, height 6; the point 45° in.
    let foot = round(&bossed(), [20.0, 20.0, 5.0], 1.0);
    let at = 6.0 - FRAC_1_SQRT_2;
    let back = delete_fillet_faces(&foot, &[[20.0, 15.0 + at, at]], TOL).unwrap();
    assert!((volume(&back, 1e-9).unwrap() - sharp).abs() < 1e-6, "{}", volume(&back, 1e-9).unwrap());
    assert_eq!(counts(&back).unwrap()[0], 8);
    // The top round, r 1: its tube's centre circle at radius 4, height 12; the point 45° out.
    let top = round(&bossed(), [20.0, 20.0, 13.0], 1.0);
    let (r, z) = (4.0 + FRAC_1_SQRT_2, 12.0 + FRAC_1_SQRT_2);
    let back = delete_fillet_faces(&top, &[[20.0, 15.0 + r, z]], TOL).unwrap();
    assert!((volume(&back, 1e-9).unwrap() - sharp).abs() < 1e-6, "{}", volume(&back, 1e-9).unwrap());
    assert_eq!(counts(&back).unwrap()[0], 8);
}

fn chamfer(body: &Body, edge: [f64; 3], d: f64) -> Body {
    blend(body, Blend::Chamfer, &[edge], &[d], TOL, &[], &[], Corners::Filled).unwrap()
}

#[test]
fn deleting_a_chamfer_on_a_box_edge_gives_the_sharp_box() {
    let cube = make_box([0.0; 3], [20.0; 3]).unwrap();
    let chamfered = chamfer(&cube, [10.0, 20.0, 20.0], 2.0);
    // On the chamfer, half way across: (19, 19) in y, z.
    let sharp = delete_fillet_faces(&chamfered, &[[10.0, 19.0, 19.0]], TOL).unwrap();
    assert!((volume(&sharp, 1e-9).unwrap() - 8000.0).abs() < 1e-6, "{}", volume(&sharp, 1e-9).unwrap());
    assert_eq!(counts(&sharp).unwrap(), [6, 12, 8]);
}

/// A drafted boss (a frustum, radius 5 to 3 over 10): its top edge chamfered 1 comes off back to the sharp frustum, 490π/3.
#[test]
fn deleting_a_chamfer_on_a_drafted_boss_gives_the_sharp_frustum() {
    let frustum = make_cone([20.0, 15.0, 0.0], 5.0, 3.0, 10.0).unwrap();
    let chamfered = blend(&frustum, Blend::Chamfer, &[[20.0, 18.0, 10.0]], &[1.0], TOL, &[], &[], Corners::Filled).unwrap();
    // On the chamfer, between the top (r 2) and the wall (r 3.2 at z 9): (r 2.6, z 9.5) at +y.
    let sharp = delete_fillet_faces(&chamfered, &[[20.0, 17.6, 9.5]], TOL).unwrap();
    let want = 490.0 * PI / 3.0;
    assert!((volume(&sharp, 1e-9).unwrap() - want).abs() < 1e-6, "{}", volume(&sharp, 1e-9).unwrap());
    assert_eq!(counts(&sharp).unwrap()[0], 3);
}

/// A chamfer wider than its edge, between walls at 120°: the sides give the prism back.
#[test]
fn deleting_a_chamfer_wider_than_its_edge_gives_the_prism() {
    let s3 = 3.0_f64.sqrt();
    let hex: Vec<[f64; 3]> = (0..6).map(|i| f64::from(i) * PI / 3.0).map(|a| [2.0 * a.cos(), 2.0 * a.sin(), 0.0]).collect();
    let outline: Vec<Curve> = (0..6).map(|i| Curve::Line { start: hex[i], end: hex[(i + 1) % 6] }).collect();
    let prism = extrude(&outline, [0.0, 0.0, 1.0], 20.0, 0.0, DraftCorner::default(), true).unwrap();
    assert!((volume(&prism, 1e-9).unwrap() - 120.0 * s3).abs() < 1e-6);
    // The top edge from (1, √3) to (−1, √3); the chamfer half way across.
    let chamfered = chamfer(&prism, [0.0, s3, 20.0], 1.5);
    let back = delete_fillet_faces(&chamfered, &[[0.0, s3 - 0.75, 19.25]], TOL).unwrap();
    assert!((volume(&back, 1e-9).unwrap() - 120.0 * s3).abs() < 1e-6, "{}", volume(&back, 1e-9).unwrap());
    assert_eq!(counts(&back).unwrap(), [8, 18, 12]);
}

/// A plate 40 × 30 × 5 with a through hole Ø6: 6000 − 45π.
fn holed() -> Body {
    let plate = make_box([0.0; 3], [40.0, 30.0, 5.0]).unwrap();
    let pin = make_cylinder([20.0, 15.0, -1.0], 3.0, 7.0).unwrap();
    boolean(&plate, &pin, BooleanOp::Subtract).unwrap().unwrap()
}

#[test]
fn deleting_a_hole_chamfer_gives_the_plain_hole() {
    let plain = 6000.0 - 45.0 * PI;
    // The hole's top edge at +y (off the circle's seam at +x), chamfered 1.
    let chamfered = chamfer(&holed(), [20.0, 18.0, 5.0], 1.0);
    // On the cone, half way down: radius 3.5, height 4.5.
    let back = delete_fillet_faces(&chamfered, &[[20.0, 18.5, 4.5]], TOL).unwrap();
    assert!((volume(&back, 1e-9).unwrap() - plain).abs() < 1e-6, "{}", volume(&back, 1e-9).unwrap());
    assert_eq!(counts(&back).unwrap()[0], counts(&holed()).unwrap()[0]);
}

#[test]
fn a_face_that_is_not_a_fillet_is_refused() {
    let cube = make_box([0.0; 3], [20.0; 3]).unwrap();
    assert!(delete_fillet_faces(&cube, &[[10.0, 10.0, 20.0]], TOL).is_err());
}

#[test]
fn two_points_on_one_round_and_two_rounds_at_once_give_the_sharp_box() {
    let cube = make_box([0.0; 3], [20.0; 3]).unwrap();
    let rounded = round(&cube, [10.0, 20.0, 20.0], 2.0);
    let at = 18.0 + 2.0 * FRAC_1_SQRT_2;
    let sharp = delete_fillet_faces(&rounded, &[[5.0, at, at], [15.0, at, at]], TOL).unwrap();
    assert!((volume(&sharp, 1e-9).unwrap() - 8000.0).abs() < 1e-6, "{}", volume(&sharp, 1e-9).unwrap());
    assert_eq!(counts(&sharp).unwrap(), [6, 12, 8]);
    let both = round(&rounded, [10.0, 0.0, 20.0], 2.0);
    let near = 2.0 - 2.0 * FRAC_1_SQRT_2;
    let sharp = delete_fillet_faces(&both, &[[10.0, at, at], [10.0, near, at]], TOL).unwrap();
    assert!((volume(&sharp, 1e-9).unwrap() - 8000.0).abs() < 1e-6, "{}", volume(&sharp, 1e-9).unwrap());
    assert_eq!(counts(&sharp).unwrap(), [6, 12, 8]);
}
